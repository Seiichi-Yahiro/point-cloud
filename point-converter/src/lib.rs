pub(crate) use byteorder::LittleEndian as Endianess;

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

        if let Some(extension) = path.extension().and_then(|it| it.to_str()) {
            match extension {
                "las" | "laz" => {
                    if let Err(err) = converter::convert_las(path, &mut converter) {
                        log::error!("Failed, {:?}", err);
                    }
                }
                "ply" => {
                    if let Err(err) = converter::convert_ply(path, &mut converter) {
                        log::error!("Failed, {:?}", err);
                    }
                }
                metadata::Metadata::EXTENSION => {
                    if let Err(err) = converter::convert_own(path, &mut converter) {
                        log::error!("Failed, {:?}", err);
                    }
                }
                _ => {
                    log::warn!("Unsupported file format '{}'", extension)
                }
            }
        }
    }

    log::info!(
        "Finished converting after {} ms",
        total_instant.elapsed().as_millis()
    );
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

fn log_progress(i: usize, number_of_points: usize) {
    if i % (number_of_points as f32 * 0.05) as usize == 0 {
        log::info!("{}/{}", i, number_of_points);
    }
}
