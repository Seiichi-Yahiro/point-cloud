use std::io::{Error, ErrorKind};
use std::path::Path;

use crate::cell::Cell;
use crate::converter::BatchedPointReader;
use crate::metadata::Metadata;
use crate::point::Point;

pub struct BatchedPointCloudPointReader {
    metadata: Metadata,
    point_iterator: Box<dyn Iterator<Item = Point> + Send>,
    read_points: u64,
}

impl BatchedPointCloudPointReader {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self, Error> {
        match Metadata::from_path(path.as_ref()) {
            Ok(metadata) => {
                let working_directory = path.as_ref().parent().unwrap().to_path_buf();

                let point_iterator = (0..metadata.hierarchies)
                    .map(move |hierarchy| {
                        working_directory.join(Metadata::hierarchy_string(hierarchy))
                    })
                    .map(|hierarchy_path| hierarchy_path.read_dir())
                    .filter_map(|read_dir_result| match read_dir_result {
                        Ok(read_dir) => Some(read_dir),
                        Err(err) => {
                            log::error!("Failed to read dir: {}", err);
                            None
                        }
                    })
                    .flatten()
                    .filter_map(|dir_entry_result| match dir_entry_result {
                        Ok(dir_entry) => Some(dir_entry.path()),
                        Err(err) => {
                            log::error!("Failed to read file: {}", err);
                            None
                        }
                    })
                    .map(Cell::from_path)
                    .filter_map(|cell_result| match cell_result {
                        Ok(cell) => Some(cell),
                        Err(err) => {
                            log::error!("Failed to read cell {}", err);
                            None
                        }
                    })
                    .flat_map(|cell| cell.all_points().copied().collect::<Vec<_>>());

                Ok(Self {
                    metadata,
                    point_iterator: Box::new(point_iterator),
                    read_points: 0,
                })
            }
            Err(err) => Err(Error::new(
                ErrorKind::InvalidInput,
                format!("Couldn't parse metadata at {:?}: {}", path.as_ref(), err),
            )),
        }
    }
}

impl BatchedPointReader for BatchedPointCloudPointReader {
    fn get_batch(&mut self, size: usize) -> Result<Vec<Point>, Error> {
        let batch_size = self.remaining_points().min(size as u64);
        let mut batch = Vec::with_capacity(batch_size as usize);

        for _ in 0..batch_size {
            if let Some(point) = self.point_iterator.next() {
                batch.push(point);
                self.read_points += 1;
            }
        }

        Ok(batch)
    }

    fn total_points(&self) -> u64 {
        self.metadata.number_of_points
    }

    fn remaining_points(&self) -> u64 {
        self.total_points() - self.read_points
    }
}
