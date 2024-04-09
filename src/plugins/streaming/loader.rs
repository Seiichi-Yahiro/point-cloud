use flume::RecvError;

use point_converter::cell::{Cell, CellId};
use point_converter::metadata::Metadata;

use crate::plugins::streaming::Source;

#[derive(Debug)]
pub enum LoadFile {
    Metadata(Source),
    Cell {
        id: CellId,
        sub_grid_dimension: u32,
        source: Source,
    },
    Stop,
}

#[cfg(not(target_arch = "wasm32"))]
type LoadedFileError = std::io::Error;

#[cfg(target_arch = "wasm32")]
type LoadedFileError = js_sys::Error;

#[derive(Debug)]
pub struct LoadedMetadata {
    pub source: Source,
    pub metadata: Result<Metadata, LoadedFileError>,
}

#[derive(Debug)]
pub struct LoadedCell {
    pub id: CellId,
    pub cell: Result<Option<Cell>, LoadedFileError>,
}

#[cfg(not(target_arch = "wasm32"))]
fn no_source_error() -> LoadedFileError {
    std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "No source to load from provided",
    )
}

#[cfg(target_arch = "wasm32")]
fn no_source_error() -> LoadedFileError {
    let err = js_sys::Error::new("No source to load from provided");
    err.set_cause(&wasm_bindgen::JsValue::from_str("NotFoundError"));
    err
}

#[cfg(not(target_arch = "wasm32"))]
pub fn spawn_loader(
    receiver: flume::Receiver<LoadFile>,
    metadata_sender: flume::Sender<LoadedMetadata>,
    cell_sender: flume::Sender<LoadedCell>,
) {
    log::debug!("Spawning loader thread");

    std::thread::spawn(move || loop {
        match receiver.recv() {
            Ok(LoadFile::Metadata(source)) => {
                let load_result = match &source {
                    Source::Directory(dir) => {
                        let path = dir.join("metadata").with_extension("json");
                        Metadata::from_path(path)
                    }
                    Source::URL => {
                        todo!()
                    }
                    Source::None => Err(no_source_error()),
                };

                metadata_sender
                    .send(LoadedMetadata {
                        metadata: load_result,
                        source,
                    })
                    .unwrap();
            }
            Ok(LoadFile::Cell {
                id,
                sub_grid_dimension,
                source: Source::Directory(dir),
            }) => {
                let path = id.path(&dir);

                match Cell::from_path(path, sub_grid_dimension) {
                    Ok(cell) => {
                        cell_sender
                            .send(LoadedCell {
                                cell: Ok(Some(cell)),
                                id,
                            })
                            .unwrap();
                    }
                    Err(err) => {
                        let load_result = match err.kind() {
                            std::io::ErrorKind::NotFound => Ok(None),
                            _ => Err(err),
                        };

                        cell_sender
                            .send(LoadedCell {
                                cell: load_result,
                                id,
                            })
                            .unwrap();
                    }
                }
            }
            Ok(LoadFile::Cell {
                source: Source::URL,
                ..
            }) => {
                todo!()
            }
            Ok(LoadFile::Cell {
                source: Source::None,
                id,
                ..
            }) => {
                cell_sender
                    .send(LoadedCell {
                        cell: Err(no_source_error()),
                        id,
                    })
                    .unwrap();
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
pub fn spawn_loader(
    receiver: flume::Receiver<LoadFile>,
    metadata_sender: flume::Sender<LoadedMetadata>,
    cell_sender: flume::Sender<LoadedCell>,
) {
    log::debug!("Spawning async loader task");

    wasm_bindgen_futures::spawn_local(async move {
        loop {
            match receiver.recv_async().await {
                Ok(LoadFile::Metadata(source)) => {
                    let load_result = match &source {
                        Source::Directory(dir) => load_metadata(dir).await,
                        Source::URL => {
                            todo!()
                        }
                        Source::None => Err(no_source_error()),
                    };

                    metadata_sender
                        .send(LoadedMetadata {
                            metadata: load_result,
                            source,
                        })
                        .unwrap();
                }
                Ok(LoadFile::Cell {
                    id,
                    sub_grid_dimension,
                    source: Source::Directory(dir),
                }) => match load_cell(id, sub_grid_dimension, dir).await {
                    Ok(cell) => {
                        cell_sender
                            .send(LoadedCell {
                                id,
                                cell: Ok(Some(cell)),
                            })
                            .unwrap();
                    }
                    Err(err) => {
                        let load_result = if err.name() == "NotFoundError" {
                            Ok(None)
                        } else {
                            Err(err)
                        };

                        cell_sender
                            .send(LoadedCell {
                                id,
                                cell: load_result,
                            })
                            .unwrap();
                    }
                },
                Ok(LoadFile::Cell {
                    source: Source::URL,
                    ..
                }) => {
                    todo!()
                }
                Ok(LoadFile::Cell {
                    source: Source::None,
                    id,
                    ..
                }) => {
                    cell_sender
                        .send(LoadedCell {
                            cell: Err(no_source_error()),
                            id,
                        })
                        .unwrap();
                }
                Ok(LoadFile::Stop) => {
                    log::debug!("Stopping async loader task");
                    return;
                }
                Err(RecvError::Disconnected) => {
                    log::error!("Async loader's sender has disconnected");
                    return;
                }
            }
        }
    });
}

#[cfg(target_arch = "wasm32")]
async fn load_metadata(
    dir: &crate::plugins::streaming::Directory,
) -> Result<Metadata, js_sys::Error> {
    use wasm_bindgen::JsCast;

    let buffer = crate::web::readBytes(dir, "metadata.json").await?;
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
    dir: crate::plugins::streaming::Directory,
) -> Result<Cell, js_sys::Error> {
    use wasm_bindgen::JsCast;

    let [hierarchy_dir, file_name] = id.path();

    let buffer = crate::web::readCell(&dir, &hierarchy_dir, &file_name).await?;
    let array_buffer = buffer.dyn_into::<js_sys::ArrayBuffer>().unwrap();

    let bytes = js_sys::Uint8Array::new(&array_buffer).to_vec();
    let mut cursor = std::io::Cursor::new(bytes);

    let cell = Cell::read_from(&mut cursor, sub_grid_dimension)
        .map_err(|err| js_sys::Error::new(&err.to_string()))?;

    log::debug!("Loaded cell {:?}", id);

    Ok(cell)
}
