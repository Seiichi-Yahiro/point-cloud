pub(crate) use byteorder::LittleEndian as Endianess;

use crate::converter::BatchedPointReader;

pub mod cell;
pub mod converter;
pub mod metadata;
pub mod point;

pub fn convert_from_paths<O: AsRef<std::path::Path>>(paths: &[std::path::PathBuf], output: O) {
    let metadata = load_metadata(output.as_ref());
    let mut converter = converter::Converter::new(metadata, output.as_ref());

    let total_instant = std::time::Instant::now();

    for (path_index, path) in paths.iter().enumerate() {
        log::info!(
            "Converting file {}/{}, {:?}",
            path_index + 1,
            paths.len(),
            path
        );

        if let Some(mut batched_reader) = get_batched_point_reader(path) {
            let total_points = batched_reader.total_points();
            log::info!("Converting {} points", total_points);

            let mut file_instant = std::time::Instant::now();

            loop {
                match batched_reader.get_batch(10_000) {
                    Ok(batch) => {
                        converter.add_points_batch(batch);
                    }
                    Err(err) => {
                        log::error!("{:?}", err);
                        break;
                    }
                }

                let remaining_points = batched_reader.remaining_points();

                if file_instant.elapsed() > std::time::Duration::from_millis(5000) {
                    log::info!("Remaining points: {}", remaining_points);
                    file_instant = std::time::Instant::now();
                }

                if remaining_points == 0 {
                    break;
                }
            }
        }
    }

    log::info!(
        "Finished converting after {} ms",
        total_instant.elapsed().as_millis()
    );
}

pub fn get_batched_point_reader<P: AsRef<std::path::Path>>(
    path: P,
) -> Option<Box<dyn BatchedPointReader + Send>> {
    let extension = path
        .as_ref()
        .extension()
        .and_then(|it| it.to_str())
        .map(String::from);

    extension.and_then::<Box<dyn BatchedPointReader + Send>, _>(|extension| {
        match extension.as_str() {
            "las" | "laz" => Some(Box::new(converter::BatchedLasPointReader::new(path))),
            "ply" => Some(Box::new(converter::BatchedPlyPointReader::new(path))),
            metadata::Metadata::EXTENSION => Some(Box::new(
                converter::BatchedPointCloudPointReader::new(path).unwrap(),
            )),
            _ => {
                log::warn!("Unsupported file format '{}'", extension);
                None
            }
        }
    })
}

fn load_metadata(output: &std::path::Path) -> metadata::Metadata {
    match std::fs::read(
        output
            .join(metadata::Metadata::FILE_NAME)
            .with_extension(metadata::Metadata::EXTENSION),
    ) {
        Ok(bytes) => {
            log::info!("Found an existing metadata file.");
            metadata::Metadata::read_from(&mut std::io::Cursor::new(bytes)).unwrap()
        }
        Err(_) => {
            log::info!("Found no metadata file. A new one will be created.");
            metadata::Metadata::default()
        }
    }
}
