use crate::{converter, log_progress, point};

pub fn convert_las(
    path: &std::path::Path,
    converter: &mut converter::Converter,
) -> las::Result<()> {
    use las::Read;
    let mut reader = las::Reader::from_path(path)?;

    let number_of_points = reader.header().number_of_points();
    log::info!("Will load {} points", number_of_points);

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
