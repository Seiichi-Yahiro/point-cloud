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
    std::thread::Builder::new()
        .name("Metadata Loader".to_string())
        .spawn(move || {
            pollster::block_on(load_from_source(source, sender));
        })
        .expect("Failed to spawn metadata loader thread");

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
    let file_name = format!("{}.{}", Metadata::FILE_NAME, Metadata::EXTENSION);

    let bytes = dir.get_file_handle(&file_name).await?.read_bytes().await?;
    let mut cursor = std::io::Cursor::new(bytes);

    let metadata =
        Metadata::read_from(&mut cursor).map_err(|err| js_sys::Error::new(&err.to_string()))?;

    log::info!("Loaded metadata for {}", metadata.name);

    Ok(metadata)
}
