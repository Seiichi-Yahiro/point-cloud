pub mod cell;
#[cfg(not(target_arch = "wasm32"))]
pub mod converter;
pub mod metadata;
pub mod point;

#[cfg(not(target_arch = "wasm32"))]
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
                    if let Err(err) = convert_las(path, &mut converter) {
                        log::error!("Failed, {:?}", err);
                    }
                }
                "ply" => {
                    if let Err(err) = convert_ply(path, &mut converter) {
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

#[cfg(not(target_arch = "wasm32"))]
fn load_metadata(output: &std::path::Path) -> metadata::Metadata {
    match std::fs::read(output.join("metadata").with_extension("bin")) {
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

#[cfg(not(target_arch = "wasm32"))]
fn convert_las(path: &std::path::Path, converter: &mut converter::Converter) -> las::Result<()> {
    use las::Read;
    let mut reader = las::Reader::from_path(path)?;

    let number_of_points = reader.header().number_of_points();

    let file_instant = std::time::Instant::now();

    for (i, wrapped_point) in reader.points().enumerate() {
        let las_point = wrapped_point?;
        let color = las_point.color.unwrap_or_default();

        let point = point::Point {
            pos: glam::Vec3::new(las_point.x as f32, las_point.y as f32, las_point.z as f32),
            color: [color.red as u8, color.green as u8, color.blue as u8, 255],
        };

        converter.add_point(point);
        log_progress(i, number_of_points as usize);
    }

    log::info!(
        "Finished file after {} ms",
        file_instant.elapsed().as_millis()
    );

    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn convert_ply(
    path: &std::path::Path,
    converter: &mut converter::Converter,
) -> Result<(), std::io::Error> {
    let file = std::fs::File::open(path).unwrap();
    let mut buf_reader = std::io::BufReader::new(file);

    let parser = ply_rs::parser::Parser::<point::Point>::new();
    let header = parser.read_header(&mut buf_reader)?;

    if let Some(element) = header.elements.get("vertex") {
        let number_of_points = element.count;

        log::info!("Will load {} points", number_of_points);

        let file_instant = std::time::Instant::now();

        let points = parser.read_payload_for_element(&mut buf_reader, element, &header)?;

        log::info!("Finished loading points will start converting now.");

        for (i, point) in points.into_iter().enumerate() {
            converter.add_point(point);
            log_progress(i, number_of_points);
        }

        log::info!(
            "Finished file after {} ms",
            file_instant.elapsed().as_millis()
        );
    }

    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn log_progress(i: usize, number_of_points: usize) {
    if i % 10_000_000 == 0 {
        log::info!("{}/{}", i, number_of_points);
    }
}
