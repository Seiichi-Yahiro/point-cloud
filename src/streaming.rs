use std::fmt::Debug;

use flume::{RecvError, TryRecvError};

use point_converter::cell::{Cell, CellId};
use point_converter::metadata::Metadata;

use crate::camera::Camera;

enum LoadFile {
    Metadata(WorkingDir),
    Cell {
        id: CellId,
        sub_grid_dimension: u32,
        working_dir: WorkingDir,
    },
    Stop,
}

enum LoadedFile {
    Metadata(Metadata),
    Cell(Cell),
}

pub trait CellStreamer: Debug {
    fn load_metadata(&self);

    fn metadata(&self) -> Option<&Metadata>;

    fn load_cell(&self, cell_id: CellId);

    fn update(&mut self, camera: &Camera);
}

#[derive(Debug)]
pub struct EmptyCellStreamer;

impl CellStreamer for EmptyCellStreamer {
    fn load_metadata(&self) {}

    fn metadata(&self) -> Option<&Metadata> {
        None
    }

    fn load_cell(&self, _cell_id: CellId) {}

    fn update(&mut self, _camera: &Camera) {}
}

#[cfg(not(target_arch = "wasm32"))]
type WorkingDir = std::path::PathBuf;

#[cfg(target_arch = "wasm32")]
type WorkingDir = web_sys::FileSystemDirectoryHandle;

#[derive(Debug)]
pub struct LocalCellStreamer {
    working_dir: WorkingDir,
    metadata: Option<Metadata>,
    cells: Vec<Cell>,
    load_sender: flume::Sender<LoadFile>,
    loaded_receiver: flume::Receiver<LoadedFile>,
}

impl LocalCellStreamer {
    pub fn new(working_dir: WorkingDir) -> Self {
        let (load_sender, load_receiver) = flume::unbounded::<LoadFile>();
        let (loaded_sender, loaded_receiver) = flume::unbounded::<LoadedFile>();

        Self::spawn_loader(load_receiver, loaded_sender);

        Self {
            working_dir,
            metadata: None,
            cells: Vec::new(),
            load_sender,
            loaded_receiver,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn spawn_loader(receiver: flume::Receiver<LoadFile>, sender: flume::Sender<LoadedFile>) {
        log::debug!("Spawning loader thread");

        std::thread::spawn(move || loop {
            match receiver.recv() {
                Ok(LoadFile::Metadata(working_dir)) => {
                    let path = working_dir.join("metadata").with_extension("json");

                    match Metadata::from_path(path) {
                        Ok(metadata) => {
                            log::info!("Loaded metadata for {}", metadata.name);

                            if let Err(err) = sender.send(LoadedFile::Metadata(metadata)) {
                                log::error!("{:?}", err);
                                return;
                            }
                        }
                        Err(err) => {
                            log::error!("Failed to load metadata: {:?}", err);
                        }
                    }
                }
                Ok(LoadFile::Cell {
                    id,
                    sub_grid_dimension,
                    working_dir,
                }) => {
                    let path = id.path(&working_dir);

                    match Cell::from_path(path, sub_grid_dimension) {
                        Ok(cell) => {
                            log::debug!("Loaded cell {:?}", id);

                            if let Err(err) = sender.send(LoadedFile::Cell(cell)) {
                                log::error!("{:?}", err);
                                return;
                            }
                        }
                        Err(err) => {
                            log::error!("Failed to load cell {:?}: {:?}", id, err);
                        }
                    }
                }
                Ok(LoadFile::Stop) => {
                    log::debug!("Stopping loader thread");
                    return;
                }
                Err(RecvError::Disconnected) => {
                    log::error!("Loader threads sender has disconnected");
                    return;
                }
            }
        });
    }

    #[cfg(target_arch = "wasm32")]
    fn spawn_loader(receiver: flume::Receiver<LoadFile>, sender: flume::Sender<LoadedFile>) {
        log::debug!("Spawning loader thread");

        wasm_bindgen_futures::spawn_local(async move {
            use wasm_bindgen::JsCast;

            loop {
                match receiver.recv_async().await {
                    Ok(LoadFile::Metadata(working_dir)) => {
                        let array_buffer =
                            match crate::web::readBytes(&working_dir, "metadata.json").await {
                                Ok(buffer) => buffer.dyn_into::<js_sys::ArrayBuffer>().unwrap(),
                                Err(err) => {
                                    log::error!("Failed to load metadata {:?}", err);
                                    continue;
                                }
                            };

                        let bytes = js_sys::Uint8Array::new(&array_buffer).to_vec();
                        let mut cursor = std::io::Cursor::new(bytes);

                        match Metadata::read_from(&mut cursor) {
                            Ok(metadata) => {
                                log::info!("Loaded metadata for {}", metadata.name);
                                sender.send(LoadedFile::Metadata(metadata)).unwrap();
                            }
                            Err(err) => {
                                log::error!("Failed to load metadata: {:?}", err);
                            }
                        }
                    }
                    Ok(LoadFile::Cell {
                        id,
                        sub_grid_dimension,
                        working_dir,
                    }) => {
                        let [hierarchy_dir, file_name] = id.path();

                        let array_buffer =
                            match crate::web::readCell(&working_dir, &hierarchy_dir, &file_name)
                                .await
                            {
                                Ok(buffer) => buffer.dyn_into::<js_sys::ArrayBuffer>().unwrap(),
                                Err(err) => {
                                    log::error!("Failed to load cell {:?}: {:?}", id, err);
                                    continue;
                                }
                            };

                        let bytes = js_sys::Uint8Array::new(&array_buffer).to_vec();
                        let mut cursor = std::io::Cursor::new(bytes);

                        match Cell::read_from(&mut cursor, sub_grid_dimension) {
                            Ok(cell) => {
                                log::debug!("Loaded cell {:?}", id);

                                if let Err(err) = sender.send(LoadedFile::Cell(cell)) {
                                    log::error!("{:?}", err);
                                    return;
                                }
                            }
                            Err(err) => {
                                log::error!("Failed to load cell {:?}: {:?}", id, err);
                            }
                        }
                    }
                    Ok(LoadFile::Stop) => {
                        log::debug!("Stopping loader thread");
                        return;
                    }
                    Err(RecvError::Disconnected) => {
                        log::error!("Loader threads sender has disconnected");
                        return;
                    }
                }
            }
        });
    }
}

impl Drop for LocalCellStreamer {
    fn drop(&mut self) {
        self.load_sender.send(LoadFile::Stop).unwrap();
    }
}

impl CellStreamer for LocalCellStreamer {
    fn load_metadata(&self) {
        #[cfg(not(target_arch = "wasm32"))]
        log::info!("Trying to load metadata from {:?}", self.working_dir);

        #[cfg(target_arch = "wasm32")]
        log::info!("Trying to load metadata from {}", self.working_dir.name());

        if let Err(err) = self
            .load_sender
            .send(LoadFile::Metadata(self.working_dir.clone()))
        {
            log::error!("{:?}", err);
        }
    }

    fn metadata(&self) -> Option<&Metadata> {
        self.metadata.as_ref()
    }

    fn load_cell(&self, cell_id: CellId) {
        if let Some(metadata) = &self.metadata {
            if let Err(err) = self.load_sender.send(LoadFile::Cell {
                id: cell_id,
                sub_grid_dimension: metadata.sub_grid_dimension,
                working_dir: self.working_dir.clone(),
            }) {
                log::error!("{:?}", err);
            }
        }
    }

    fn update(&mut self, camera: &Camera) {
        match self.loaded_receiver.try_recv() {
            Ok(LoadedFile::Metadata(metadata)) => {
                self.metadata = Some(metadata);
            }
            Ok(LoadedFile::Cell(cell)) => {
                self.cells.push(cell);
            }
            Err(TryRecvError::Disconnected) => {
                panic!("Failed to stream files as the sender was dropped");
            }
            Err(TryRecvError::Empty) => {}
        }
    }
}
