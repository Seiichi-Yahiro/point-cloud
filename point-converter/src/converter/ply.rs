use crate::{converter, log_progress, point};

pub fn convert_ply(
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
