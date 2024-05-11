use std::path::Path;

use las::{Read, Reader};

use crate::converter::BatchedPointReader;
use crate::point::Point;

pub struct BatchedLasPointReader {
    reader: Reader<'static>,
    read_points: u64,
}

impl BatchedLasPointReader {
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        Self {
            reader: Reader::from_path(path).unwrap(),
            read_points: 0,
        }
    }
}

impl BatchedPointReader for BatchedLasPointReader {
    fn get_batch(&mut self, size: usize) -> Result<Vec<Point>, std::io::Error> {
        self.reader
            .read_n(size as u64)
            .map(|points| {
                self.read_points += points.len() as u64;

                points
                    .into_iter()
                    .map(|las_point| {
                        let color = las_point.color.unwrap_or_default();

                        Point {
                            pos: glam::Vec3::new(
                                las_point.x as f32,
                                las_point.y as f32,
                                las_point.z as f32,
                            ),
                            color: [color.red as u8, color.green as u8, color.blue as u8, 255],
                        }
                    })
                    .collect()
            })
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err))
    }

    fn total_points(&self) -> u64 {
        self.reader.header().number_of_points()
    }

    fn remaining_points(&self) -> u64 {
        self.total_points() - self.read_points
    }
}
