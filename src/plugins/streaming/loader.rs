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

#[derive(Debug)]
pub enum LoadedFile {
    Metadata(Metadata),
    Cell { id: CellId, cell: Option<Cell> },
}

#[cfg(not(target_arch = "wasm32"))]
pub fn spawn_loader(receiver: flume::Receiver<LoadFile>, sender: flume::Sender<LoadedFile>) {
    log::debug!("Spawning loader thread");

    std::thread::spawn(move || loop {
        match receiver.recv() {
            Ok(LoadFile::Metadata(Source::Directory(dir))) => {
                let path = dir.join("metadata").with_extension("json");

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
            Ok(LoadFile::Metadata(Source::URL)) => {
                todo!();
            }
            Ok(LoadFile::Cell {
                id,
                sub_grid_dimension,
                source: Source::Directory(dir),
            }) => {
                let path = id.path(&dir);

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
                        std::io::ErrorKind::NotFound => {
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
            Ok(LoadFile::Cell {
                source: Source::URL,
                ..
            }) => {
                todo!()
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
pub fn spawn_loader(receiver: flume::Receiver<LoadFile>, sender: flume::Sender<LoadedFile>) {
    log::debug!("Spawning loader thread");

    wasm_bindgen_futures::spawn_local(async move {
        loop {
            match receiver.recv_async().await {
                Ok(LoadFile::Metadata(Source::Directory(dir))) => {
                    let result = load_metadata(dir).await.and_then(|metadata| {
                        sender
                            .send(LoadedFile::Metadata(metadata))
                            .map_err(|err| js_sys::Error::new(&err.to_string()))
                    });

                    if let Err(err) = result {
                        log::error!("Failed to load metadata: {:?}", err);
                    }
                }
                Ok(LoadFile::Metadata(Source::URL)) => {
                    todo!()
                }
                Ok(LoadFile::Cell {
                    id,
                    sub_grid_dimension,
                    source: Source::Directory(dir),
                }) => {
                    let result = load_cell(id, sub_grid_dimension, dir)
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
                Ok(LoadFile::Cell {
                    source: Source::URL,
                    ..
                }) => {
                    todo!()
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
async fn load_metadata(
    dir: crate::plugins::streaming::Directory,
) -> Result<Metadata, js_sys::Error> {
    use wasm_bindgen::JsCast;

    let buffer = crate::web::readBytes(&dir, "metadata.json").await?;
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
