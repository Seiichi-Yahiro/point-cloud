use std::fmt::Debug;

use flume::TryRecvError;

use point_converter::cell::Cell;
use point_converter::metadata::Metadata;

use crate::camera::Camera;

enum LoadedFile {
    Metadata(Metadata),
    Cell(Cell),
}

pub trait CellStreamer: Debug {
    fn load_metadata(&self);

    fn metadata(&self) -> Option<&Metadata>;

    fn update(&mut self, camera: &Camera);
}

#[derive(Debug)]
pub struct EmptyCellStreamer;

impl CellStreamer for EmptyCellStreamer {
    fn load_metadata(&self) {}

    fn metadata(&self) -> Option<&Metadata> {
        None
    }

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
    receiver: flume::Receiver<LoadedFile>,
    sender: flume::Sender<LoadedFile>,
}

impl LocalCellStreamer {
    pub fn new(working_dir: WorkingDir) -> Self {
        let (sender, receiver) = flume::unbounded();

        Self {
            working_dir,
            metadata: None,
            cells: Vec::new(),
            sender,
            receiver,
        }
    }
}

impl CellStreamer for LocalCellStreamer {
    #[cfg(not(target_arch = "wasm32"))]
    fn load_metadata(&self) {
        let path = self.working_dir.join("metadata").with_extension("json");
        log::info!("Trying to load metadata from {:?}", path);

        let sender = self.sender.clone();

        std::thread::spawn(move || match Metadata::from_path(path) {
            Ok(metadata) => {
                log::info!("Loaded metadata for {}", metadata.name);
                sender.send(LoadedFile::Metadata(metadata)).unwrap();
            }
            Err(err) => {
                log::error!("Failed to load metadata: {:?}", err);
            }
        });
    }

    #[cfg(target_arch = "wasm32")]
    fn load_metadata(&self) {
        log::info!("Trying to load metadata from {}", self.working_dir.name());
        let sender = self.sender.clone();
        let dir = self.working_dir.clone();

        wasm_bindgen_futures::spawn_local(async move {
            use wasm_bindgen::JsCast;

            let array_buffer = match crate::web::readBytes(&dir, "metadata.json").await {
                Ok(buffer) => buffer.dyn_into::<js_sys::ArrayBuffer>().unwrap(),
                Err(err) => {
                    log::error!("Failed to load metadata {:?}", err);
                    return;
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
        });
    }

    fn metadata(&self) -> Option<&Metadata> {
        self.metadata.as_ref()
    }

    fn update(&mut self, camera: &Camera) {
        match self.receiver.try_recv() {
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
