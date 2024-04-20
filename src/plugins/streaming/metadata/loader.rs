use flume::Sender;

use point_converter::metadata::Metadata;

use crate::plugins::streaming::loader::{no_source_error, LoadError};
use crate::plugins::streaming::{Directory, Source};

#[derive(Debug)]
pub struct LoadedMetadataMsg {
    pub source: Source,
    pub metadata: Result<Metadata, LoadError>,
}

pub fn spawn_metadata_loader(source: Source, sender: Sender<LoadedMetadataMsg>) {
    log::debug!("Spawning metadata loader thread");

    #[cfg(not(target_arch = "wasm32"))]
    std::thread::spawn(move || {
        pollster::block_on(load_from_source(source, sender));
    });

    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_futures::spawn_local(load_from_source(source, sender));
}

async fn load_from_source(source: Source, sender: Sender<LoadedMetadataMsg>) {
    let load_result = match &source {
        Source::Directory(dir) => load_from_directory(&dir).await,
        Source::URL => {
            todo!()
        }
        Source::None => Err(no_source_error()),
    };

    sender
        .send(LoadedMetadataMsg {
            metadata: load_result,
            source,
        })
        .unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
async fn load_from_directory(dir: &Directory) -> Result<Metadata, LoadError> {
    let path = dir
        .join(Metadata::FILE_NAME)
        .with_extension(Metadata::EXTENSION);
    Metadata::from_path(path)
}

#[cfg(target_arch = "wasm32")]
async fn load_from_directory(dir: &Directory) -> Result<Metadata, LoadError> {
    use wasm_bindgen::JsCast;

    let file_name = format!("{}.{}", Metadata::FILE_NAME, Metadata::EXTENSION);
    let buffer = crate::web::readBytes(dir, &file_name).await?;
    let array_buffer = buffer.dyn_into::<js_sys::ArrayBuffer>().unwrap();

    let bytes = js_sys::Uint8Array::new(&array_buffer).to_vec();
    let mut cursor = std::io::Cursor::new(bytes);

    let metadata =
        Metadata::read_from(&mut cursor).map_err(|err| js_sys::Error::new(&err.to_string()))?;

    log::info!("Loaded metadata for {}", metadata.name);

    Ok(metadata)
}
