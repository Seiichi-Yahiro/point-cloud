use std::fmt::Debug;
use std::hash::BuildHasherDefault;
use std::io::ErrorKind;

use caches::{Cache, DefaultEvictCallback, RawLRU};
use flume::{RecvError, TryRecvError};
use glam::IVec3;
use itertools::Itertools;
use rustc_hash::FxHasher;

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
    Cell { id: CellId, cell: Option<Cell> },
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
enum CellStatus {
    Loading,
    Loaded(Cell),
    Missing,
}

#[derive(Debug)]
pub struct LocalCellStreamer {
    working_dir: WorkingDir,
    metadata: Option<Metadata>,
    load_sender: flume::Sender<LoadFile>,
    loaded_receiver: flume::Receiver<LoadedFile>,
    cells: RawLRU<CellId, CellStatus, DefaultEvictCallback, BuildHasherDefault<FxHasher>>,
}

impl LocalCellStreamer {
    pub fn new(working_dir: WorkingDir) -> Self {
        let (load_sender, load_receiver) = flume::unbounded::<LoadFile>();
        let (loaded_sender, loaded_receiver) = flume::unbounded::<LoadedFile>();

        Self::spawn_loader(load_receiver, loaded_sender);

        Self {
            working_dir,
            metadata: None,
            cells: RawLRU::with_hasher(200, BuildHasherDefault::default()).unwrap(),
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

                            if let Err(err) = sender.send(LoadedFile::Cell {
                                cell: Some(cell),
                                id,
                            }) {
                                log::error!("{:?}", err);
                                return;
                            }
                        }
                        Err(err) => match err.kind() {
                            ErrorKind::NotFound => {
                                log::warn!("Couldn't find cell {:?}", id);

                                if let Err(err) = sender.send(LoadedFile::Cell { cell: None, id }) {
                                    log::error!("{:?}", err);
                                    return;
                                }
                            }
                            _ => {
                                log::error!("Failed to load cell {:?}: {:?}", id, err);
                            }
                        },
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
                        let result = Self::load_metadata(working_dir).await.and_then(|metadata| {
                            sender
                                .send(LoadedFile::Metadata(metadata))
                                .map_err(|err| js_sys::Error::new(&err.to_string()))
                        });

                        if let Err(err) = result {
                            log::error!("Failed to load metadata: {:?}", err);
                        }
                    }
                    Ok(LoadFile::Cell {
                        id,
                        sub_grid_dimension,
                        working_dir,
                    }) => {
                        let result = Self::load_cell(id, sub_grid_dimension, working_dir)
                            .await
                            .and_then(|cell| {
                                sender
                                    .send(LoadedFile::Cell {
                                        cell: Some(cell),
                                        id,
                                    })
                                    .map_err(|err| js_sys::Error::new(&err.to_string()))
                            });

                        if let Err(err) = result {
                            if err.name() == "NotFoundError" {
                                log::warn!("Couldn't find cell {:?}", id);

                                if let Err(err) = sender.send(LoadedFile::Cell { cell: None, id }) {
                                    log::error!("{:?}", err);
                                    return;
                                }
                            } else {
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

    #[cfg(target_arch = "wasm32")]
    async fn load_metadata(working_dir: WorkingDir) -> Result<Metadata, js_sys::Error> {
        use wasm_bindgen::JsCast;

        let buffer = crate::web::readBytes(&working_dir, "metadata.json").await?;
        let array_buffer = buffer.dyn_into::<js_sys::ArrayBuffer>().unwrap();

        let bytes = js_sys::Uint8Array::new(&array_buffer).to_vec();
        let mut cursor = std::io::Cursor::new(bytes);

        let metadata =
            Metadata::read_from(&mut cursor).map_err(|err| js_sys::Error::new(&err.to_string()))?;

        log::info!("Loaded metadata for {}", metadata.name);

        Ok(metadata)
    }

    #[cfg(target_arch = "wasm32")]
    async fn load_cell(
        id: CellId,
        sub_grid_dimension: u32,
        working_dir: WorkingDir,
    ) -> Result<Cell, js_sys::Error> {
        use wasm_bindgen::JsCast;

        let [hierarchy_dir, file_name] = id.path();

        let buffer = crate::web::readCell(&working_dir, &hierarchy_dir, &file_name).await?;
        let array_buffer = buffer.dyn_into::<js_sys::ArrayBuffer>().unwrap();

        let bytes = js_sys::Uint8Array::new(&array_buffer).to_vec();
        let mut cursor = std::io::Cursor::new(bytes);

        let cell = Cell::read_from(&mut cursor, sub_grid_dimension)
            .map_err(|err| js_sys::Error::new(&err.to_string()))?;

        log::debug!("Loaded cell {:?}", id);

        Ok(cell)
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
            Ok(LoadedFile::Cell { id, cell }) => {
                let cell_status = cell.map(CellStatus::Loaded).unwrap_or(CellStatus::Missing);
                self.cells.put(id, cell_status);
            }
            Err(TryRecvError::Disconnected) => {
                panic!("Failed to stream files as the sender was dropped");
            }
            Err(TryRecvError::Empty) => {}
        }

        if let Some(metadata) = &self.metadata {
            for cell_id in get_cells_to_load(metadata, camera, 0) {
                if self.cells.get(&cell_id).is_some() {
                    continue;
                }

                self.cells.put(cell_id, CellStatus::Loading);
                self.load_cell(cell_id);
            }
        }
    }
}

fn get_cells_to_load(
    metadata: &Metadata,
    camera: &Camera,
    hierarchy: u32,
) -> impl Iterator<Item = CellId> {
    let far = camera.projection.far / 2u32.pow(hierarchy) as f32;
    let fov_y = camera.projection.fov_y;
    let aspect_ratio = camera.projection.aspect_ratio;

    let half_height_far = far * (fov_y * 0.5).tan();
    let half_width_far = half_height_far * aspect_ratio;

    let far_radius =
        ((half_width_far * 2.0).powi(2) + (half_height_far * 2.0).powi(2)).sqrt() / 2.0;

    let radius = far_radius.max(far / 2.0) * 1.2;
    let pos = camera.transform.translation + camera.transform.forward() * radius / 2.0;

    let cell_size = metadata.cell_size(hierarchy);
    let min_cell_index = metadata.cell_index(pos - radius, cell_size);
    let max_cell_index = metadata.cell_index(pos + radius, cell_size);

    (min_cell_index.x..=max_cell_index.x)
        .cartesian_product(min_cell_index.y..=max_cell_index.y)
        .cartesian_product(min_cell_index.z..=max_cell_index.z)
        .map(|((x, y), z)| IVec3::new(x, y, z))
        .map(move |index| CellId { index, hierarchy })
}
