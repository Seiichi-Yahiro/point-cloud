use std::fs::{create_dir, create_dir_all, File};
use std::hash::BuildHasherDefault;
use std::io::{BufWriter, Cursor, ErrorKind, Write};
use std::path::{Path, PathBuf};

use caches::{Cache, DefaultEvictCallback, PutResult, RawLRU};
use glam::Vec3;
use rustc_hash::FxHasher;

use crate::cell::{Cell, CellAddPointError, CellId};
use crate::metadata::{BoundingBox, Metadata};
use crate::point::Point;

pub struct Converter {
    metadata: Metadata,
    working_directory: PathBuf,
    cell_cache: RawLRU<CellId, Cell, DefaultEvictCallback, BuildHasherDefault<FxHasher>>,
}

impl Converter {
    pub fn new(metadata: Metadata, working_directory: &Path) -> Self {
        if let Err(err) = create_dir_all(working_directory) {
            match err.kind() {
                ErrorKind::AlreadyExists => {}
                _ => {
                    panic!("{:?}", err);
                }
            }
        }

        Self {
            metadata,
            working_directory: working_directory.to_path_buf(),
            cell_cache: RawLRU::with_hasher(100, BuildHasherDefault::default()).unwrap(),
        }
    }

    pub fn add_point(&mut self, point: Point) {
        self.add_point_in_hierarchy(point, 0);
    }

    fn add_point_in_hierarchy(&mut self, point: Point, hierarchy: u32) {
        let cell_size = self.metadata.cell_size(hierarchy);
        let cell_index = self.metadata.cell_index(point.pos, cell_size);
        let cell_pos = cell_index.as_vec3() * cell_size;

        let cell_id = CellId {
            hierarchy,
            index: cell_index,
        };

        if self.metadata.hierarchies <= hierarchy {
            self.metadata.hierarchies += 1;

            if let Err(err) = create_dir(cell_id.path(&self.working_directory).parent().unwrap()) {
                match err.kind() {
                    ErrorKind::AlreadyExists => {}
                    _ => {
                        panic!("{:?}", err);
                    }
                }
            }
        }

        if !self.cell_cache.contains(&cell_id) {
            let cell = self.load_or_create_cell(
                &cell_id.path(&self.working_directory),
                cell_size,
                cell_pos,
            );

            if let PutResult::Evicted {
                key: old_cell_id,
                value: old_cell,
            } = self.cell_cache.put(cell_id, cell)
            {
                Self::save_cell(&old_cell_id.path(&self.working_directory), &old_cell).unwrap();
            }
        }

        let cell = self
            .cell_cache
            .get_mut(&cell_id)
            .expect("Cell should have been inserted if it didn't exist");

        match cell.add_point(point, &self.metadata) {
            Ok(_) => {
                self.metadata.number_of_points += 1;
                self.update_bounding_box(point);
            }
            Err(CellAddPointError::OverflowLimitReached) => {
                let overflow = cell.apply_grid_and_extract_overflow(&self.metadata);

                // subtract points or they will be counted twice
                self.metadata.number_of_points -= overflow.len() as u64;

                for point in overflow {
                    self.add_point_in_hierarchy(point, hierarchy + 1);
                }

                self.add_point_in_hierarchy(point, hierarchy + 1);
            }
            Err(CellAddPointError::GridPositionOccupied)
            | Err(CellAddPointError::PointLimitReached) => {
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

    pub fn load_cell(&self, cell_path: &Path) -> Result<Cell, std::io::Error> {
        std::fs::read(cell_path).and_then(|bytes| {
            let mut cursor = Cursor::new(bytes);
            Cell::read_from(&mut cursor, self.metadata.sub_grid_dimension)
        })
    }

    fn load_or_create_cell(&self, cell_path: &Path, cell_size: f32, cell_pos: Vec3) -> Cell {
        match self.load_cell(cell_path) {
            Ok(cell) => cell,
            Err(err) => match err.kind() {
                ErrorKind::NotFound => Cell::new(
                    self.metadata.number_of_sub_grid_cells() as usize,
                    cell_size,
                    cell_pos,
                ),
                _ => {
                    panic!("{:?}", err);
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
        for (cell_id, cell) in &self.cell_cache {
            Self::save_cell(&cell_id.path(&self.working_directory), cell)?;
        }

        Ok(())
    }

    fn save_metadata(&self) -> Result<(), std::io::Error> {
        let path = self
            .working_directory
            .join("metadata")
            .with_extension("json");

        let file = File::create(path)?;
        let mut buf_writer = BufWriter::new(file);
        self.metadata.write_to(&mut buf_writer)?;
        buf_writer.flush()?;

        Ok(())
    }

    pub fn finish(self) {
        self.save_cache().unwrap();
        self.save_metadata().unwrap();
    }
}
