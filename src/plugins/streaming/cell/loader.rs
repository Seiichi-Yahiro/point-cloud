use flume::{Receiver, RecvError, Sender};

use point_converter::cell::{Cell, CellId};

use crate::plugins::streaming::loader::{no_source_error, LoadError};
use crate::plugins::streaming::{Directory, Source};

#[derive(Debug)]
pub enum LoadCellMsg {
    Cell {
        id: CellId,
        sub_grid_dimension: u32,
        source: Source,
    },
    Stop,
}

#[derive(Debug)]
pub struct LoadedCellMsg {
    pub id: CellId,
    pub cell: Result<Option<Cell>, LoadError>,
}

pub fn spawn_cell_loader(receiver: Receiver<LoadCellMsg>, sender: Sender<LoadedCellMsg>) {
    log::debug!("Spawning cell loader thread");

    let future = async move {
        let loader = Loader { receiver, sender };
        loader.run().await;
    };

    #[cfg(not(target_arch = "wasm32"))]
    std::thread::Builder::new()
        .name("Cell Loader".to_string())
        .spawn(move || {
            pollster::block_on(future);
        })
        .expect("Failed to spawn cell loader thread");

    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_futures::spawn_local(future);
}

struct Loader {
    receiver: Receiver<LoadCellMsg>,
    sender: Sender<LoadedCellMsg>,
}

impl Loader {
    async fn run(&self) {
        loop {
            match self.receiver.recv_async().await {
                Ok(LoadCellMsg::Cell {
                    id,
                    sub_grid_dimension,
                    source,
                }) => {
                    let load_result = match source {
                        Source::Directory(dir) => {
                            match Self::load_from_directory(id, dir, sub_grid_dimension).await {
                                Ok(cell) => Ok(Some(cell)),
                                Err(err) => {
                                    #[cfg(not(target_arch = "wasm32"))]
                                    match err.kind() {
                                        std::io::ErrorKind::NotFound => Ok(None),
                                        _ => Err(err),
                                    }

                                    #[cfg(target_arch = "wasm32")]
                                    if err.name() == "NotFoundError" {
                                        Ok(None)
                                    } else {
                                        Err(err)
                                    }
                                }
                            }
                        }
                        Source::URL => {
                            todo!()
                        }
                        Source::None => Err(no_source_error()),
                    };

                    self.sender
                        .send(LoadedCellMsg {
                            cell: load_result,
                            id,
                        })
                        .unwrap();
                }
                Ok(LoadCellMsg::Stop) => {
                    log::debug!("Stopping cell loader thread");
                    return;
                }
                Err(RecvError::Disconnected) => {
                    log::error!("Cell loader thread's sender has disconnected");
                    return;
                }
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    async fn load_from_directory(
        id: CellId,
        dir: Directory,
        sub_grid_dimension: u32,
    ) -> Result<Cell, LoadError> {
        Cell::from_path(id.path(&dir), sub_grid_dimension)
    }

    #[cfg(target_arch = "wasm32")]
    async fn load_from_directory(
        id: CellId,
        dir: Directory,
        sub_grid_dimension: u32,
    ) -> Result<Cell, LoadError> {
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
}
