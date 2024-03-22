use std::fs::{create_dir, create_dir_all, File};
use std::io::{BufWriter, Cursor, ErrorKind, Write};
use std::path::{Path, PathBuf};

use glam::IVec3;

use lfu::LFUCache;

use crate::cell::{Cell, CellAddPointError};
use crate::metadata::{BoundingBox, Metadata};
use crate::point::Point;

pub struct Converter {
    metadata: Metadata,
    working_directory: PathBuf,
    cell_cache: LFUCache<PathBuf, Cell>,
}

impl Converter {
    pub fn new(metadata: Metadata, working_directory: &Path) -> Self {
        if let Err(err) = create_dir_all(working_directory) {
            match err.kind() {
                ErrorKind::AlreadyExists => {}
                _ => {
                    panic!("{}", err);
                }
            }
        }

        Self {
            metadata,
            working_directory: working_directory.to_path_buf(),
            cell_cache: LFUCache::with_capacity(100).expect("Capacity should not be 0"), // TODO which capacity for LFU?
        }
    }

    fn hierarchy_dir_name(hierarchy: u32) -> String {
        format!("h_{}", hierarchy)
    }

    fn cell_file_name(cell_index: IVec3) -> String {
        format!("c_{}_{}_{}", cell_index.x, cell_index.y, cell_index.z)
    }

    pub fn add_point(&mut self, point: Point) {
        self.add_point_in_hierarchy(point, 0);
    }

    fn add_point_in_hierarchy(&mut self, point: Point, hierarchy: u32) {
        let cell_size = self.metadata.max_cell_size / 2u32.pow(hierarchy) as f32;

        let cell_index = (point.pos / cell_size).round().as_ivec3();
        let cell_pos = cell_index.as_vec3() * cell_size;

        let hierarchy_dir = self
            .working_directory
            .join(Self::hierarchy_dir_name(hierarchy));

        if self.metadata.hierarchies <= hierarchy {
            self.metadata.hierarchies += 1;

            if let Err(err) = create_dir(&hierarchy_dir) {
                match err.kind() {
                    ErrorKind::AlreadyExists => {}
                    _ => {
                        panic!("{}", err);
                    }
                }
            }
        }

        let cell_path = hierarchy_dir
            .join(Self::cell_file_name(cell_index))
            .with_extension("bin");

        if !self.cell_cache.contains(&cell_path) {
            let cell = self.load_or_create_cell(&cell_path);

            if let Some((old_cell_path, old_cell)) = self.cell_cache.set(cell_path.clone(), cell) {
                Self::save_cell(&old_cell_path, &old_cell).unwrap();
            }
        }

        let cell = self
            .cell_cache
            .get_mut(&cell_path)
            .expect("Cell should have been inserted if it didn't exist");

        match cell.add_point(point, cell_size, cell_pos, &self.metadata) {
            Ok(_) => {
                self.metadata.number_of_points += 1;
                self.update_bounding_box(point);
            }
            Err(CellAddPointError::PointLimitReached) => {
                let overflow = cell.extract_and_close_overflow();

                // subtract points or they will be counted twice
                self.metadata.number_of_points -= overflow.len() as u64;

                for point in overflow {
                    self.add_point_in_hierarchy(point, hierarchy + 1);
                }

                self.add_point_in_hierarchy(point, hierarchy + 1);
            }
            Err(CellAddPointError::GridPositionOccupied) => {
                self.add_point_in_hierarchy(point, hierarchy + 1);
            }
        }
    }

    fn update_bounding_box(&mut self, point: Point) {
        if self.metadata.number_of_points == 1 {
            self.metadata.bounding_box = BoundingBox::new(point.pos, point.pos);
        } else {
            self.metadata.bounding_box.extend(point);
        }
    }

    fn load_or_create_cell(&self, cell_path: &Path) -> Cell {
        match std::fs::read(cell_path) {
            Ok(bytes) => {
                let mut cursor = Cursor::new(bytes);
                Cell::read_from(&mut cursor, &self.metadata).unwrap()
            }
            Err(err) => match err.kind() {
                ErrorKind::NotFound => Cell::new(self.metadata.number_of_sub_grid_cells() as usize),
                _ => {
                    panic!("{}", err);
                }
            },
        }
    }

    fn save_cell(cell_path: &Path, cell: &Cell) -> Result<(), std::io::Error> {
        let file = File::create(cell_path)?;
        let mut buf_writer = BufWriter::new(file);
        cell.write_to(&mut buf_writer)?;
        buf_writer.flush()?;

        Ok(())
    }

    fn save_cache(&self) -> Result<(), std::io::Error> {
        for (cell_path, cell) in &self.cell_cache {
            Self::save_cell(&cell_path, cell)?;
        }

        Ok(())
    }

    fn save_metadata(&self) -> Result<(), std::io::Error> {
        let path = self
            .working_directory
            .join("metadata")
            .with_extension("bin");

        let file = File::create(path)?;
        let mut buf_writer = BufWriter::new(file);
        self.metadata.write_to(&mut buf_writer)?;
        buf_writer.flush()?;

        Ok(())
    }

    pub fn done(self) {
        self.save_cache().unwrap();
        self.save_metadata().unwrap();
    }
}
