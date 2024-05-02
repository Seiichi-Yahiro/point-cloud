use crate::{cell, converter, metadata};
use std::io::ErrorKind;

pub fn convert_own(
    path: &std::path::Path,
    converter: &mut converter::Converter,
) -> Result<(), std::io::Error> {
    match metadata::Metadata::from_path(path) {
        Ok(metadata) => {
            log::info!("Found metadata with {} points", metadata.number_of_points);

            let working_dir = path.parent().unwrap();

            for hierarchy in 0..metadata.hierarchies {
                log::info!(
                    "Reading hierarchy directory {}/{}",
                    hierarchy + 1,
                    metadata.hierarchies
                );

                let hierarchy_dir = working_dir.join(format!("h_{}", hierarchy));

                let hierarchy_instant = std::time::Instant::now();

                convert_hierarchy(&metadata, &hierarchy_dir, converter);

                log::info!(
                    "Finished hierarchy after {} ms",
                    hierarchy_instant.elapsed().as_millis()
                );
            }

            Ok(())
        }
        Err(err) => Err(std::io::Error::new(
            ErrorKind::InvalidInput,
            format!("Couldn't parse metadata at {:?}: {}", path, err),
        )),
    }
}

fn convert_hierarchy(
    metadata: &metadata::Metadata,
    path: &std::path::Path,
    converter: &mut converter::Converter,
) {
    match path.read_dir() {
        Ok(read_dir) => {
            for entry in read_dir {
                match entry {
                    Ok(cell_entry) => {
                        let cell_path = cell_entry.path();

                        match cell::Cell::from_path(&cell_path, &metadata.config) {
                            Ok(cell) => {
                                for point in cell.all_points() {
                                    converter.add_point(*point);
                                }
                            }
                            Err(err) => {
                                let file_name = cell_path.file_name().unwrap();
                                log::error!("Failed to read cell {:?}: {}", file_name, err);
                            }
                        }
                    }
                    Err(err) => {
                        log::error!("Failed to read cell {}", err);
                    }
                }
            }
        }
        Err(err) => {
            log::error!("Failed to read dir {:?}: {}", path, err);
        }
    }
}
