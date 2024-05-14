use std::collections::hash_map::Entry;
use std::fs::{create_dir, create_dir_all, File};
use std::hash::BuildHasherDefault;
use std::io::{BufWriter, Cursor, ErrorKind, Write};
use std::path::{Path, PathBuf};

use bounding_volume::Aabb;
use caches::{Cache, LRUCache, PutResult};
use glam::IVec3;
use rustc_hash::{FxHashMap, FxHasher};

pub use las::BatchedLasPointReader;
pub use own::BatchedPointCloudPointReader;
pub use ply::BatchedPlyPointReader;

use crate::cell::{Cell, CellId};
use crate::metadata::{Metadata, MetadataConfig};
use crate::point::Point;

mod las;
mod own;
mod ply;

pub trait BatchedPointReader {
    fn get_batch(&mut self, size: usize) -> Result<Vec<Point>, std::io::Error>;

    fn total_points(&self) -> u64;

    fn remaining_points(&self) -> u64;
}

pub fn group_points(
    points: Vec<Point>,
    hierarchy: u32,
    config: &MetadataConfig,
) -> FxHashMap<IVec3, Vec<Point>> {
    let cell_size = config.cell_size(hierarchy);

    let mut map = FxHashMap::<IVec3, Vec<Point>>::default();

    for point in points {
        let cell_index = config.cell_index(point.pos, cell_size);
        map.entry(cell_index).or_default().push(point);
    }

    map
}

fn merge_point_maps(left: &mut FxHashMap<IVec3, Vec<Point>>, right: FxHashMap<IVec3, Vec<Point>>) {
    for (cell_index, mut points) in right {
        match left.entry(cell_index) {
            Entry::Occupied(mut entry) => {
                entry.get_mut().append(&mut points);
            }
            Entry::Vacant(entry) => {
                entry.insert(points);
            }
        }
    }
}

pub fn add_points_to_cell(
    config: &MetadataConfig,
    points: Vec<Point>,
    cell: &mut Cell,
) -> FxHashMap<IVec3, Vec<Point>> {
    let overflow_points = cell.add_points(points, config);
    let overflow_points = group_points(overflow_points, cell.header().id.hierarchy + 1, config);
    cell.add_points_in_overflow(overflow_points, config)
}

pub struct Converter {
    metadata: Metadata,
    working_directory: PathBuf,
    cell_cache: LRUCache<CellId, Cell, BuildHasherDefault<FxHasher>>,
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
            cell_cache: LRUCache::with_hasher(100, BuildHasherDefault::default()).unwrap(),
        }
    }

    fn update_bounding_box(&mut self, points: &[Point]) {
        if let Some(aabb) = Aabb::from(points.iter().map(|point| point.pos)) {
            if self.metadata.number_of_points == 0 {
                self.metadata.bounding_box = aabb;
            } else {
                self.metadata.bounding_box.extend_aabb(&aabb);
            }
        }
    }

    pub fn add_points_batch(&mut self, points: Vec<Point>) {
        self.update_bounding_box(&points);
        self.metadata.number_of_points += points.len() as u64;

        let grouped_points = group_points(points, 0, &self.metadata.config);
        self.add_points_in_hierarchy(0, &self.metadata.config.clone(), grouped_points);
    }

    fn add_points_in_hierarchy(
        &mut self,
        hierarchy: u32,
        config: &MetadataConfig,
        grouped_points: FxHashMap<IVec3, Vec<Point>>,
    ) {
        self.create_hierarchy_folder(hierarchy);

        let mut next_hierarchy_points = FxHashMap::default();

        for (cell_index, points) in grouped_points {
            let cell_id = CellId {
                hierarchy,
                index: cell_index,
            };

            let cell = self.get_cell_mut(cell_id);
            let remaining_points = add_points_to_cell(config, points, cell);

            merge_point_maps(&mut next_hierarchy_points, remaining_points);
        }

        if !next_hierarchy_points.is_empty() {
            self.add_points_in_hierarchy(hierarchy + 1, config, next_hierarchy_points);
        }
    }

    fn create_hierarchy_folder(&mut self, hierarchy: u32) {
        if self.metadata.hierarchies <= hierarchy {
            self.metadata.hierarchies += 1;

            let path = self
                .working_directory
                .join(Metadata::hierarchy_string(hierarchy));

            if let Err(err) = create_dir(path) {
                match err.kind() {
                    ErrorKind::AlreadyExists => {}
                    _ => {
                        panic!("{:?}", err);
                    }
                }
            }
        }
    }

    fn get_cell_mut(&mut self, cell_id: CellId) -> &mut Cell {
        if !self.cell_cache.contains(&cell_id) {
            let cell =
                self.load_or_create_cell(&self.working_directory.join(cell_id.path()), cell_id);

            if let PutResult::Evicted {
                key: old_cell_id,
                value: old_cell,
            } = self.cell_cache.put(cell_id, cell)
            {
                Self::save_cell(&self.working_directory.join(old_cell_id.path()), &old_cell)
                    .unwrap();
            }
        }

        self.cell_cache
            .get_mut(&cell_id)
            .expect("Cell should have been inserted if it didn't exist")
    }

    fn load_cell(&self, cell_path: &Path) -> Result<Cell, std::io::Error> {
        std::fs::read(cell_path).and_then(|bytes| {
            let mut cursor = Cursor::new(bytes);
            Cell::read_from(&mut cursor)
        })
    }

    fn load_or_create_cell(&self, cell_path: &Path, id: CellId) -> Cell {
        match self.load_cell(cell_path) {
            Ok(cell) => cell,
            Err(err) => match err.kind() {
                ErrorKind::NotFound => {
                    let cell_size = self.metadata.config.cell_size(id.hierarchy);
                    let cell_pos = self.metadata.config.cell_pos(id.index, cell_size);
                    Cell::new(
                        id,
                        self.metadata.config.sub_grid_dimension,
                        cell_size,
                        cell_pos,
                        50_000,
                    )
                }
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

    pub fn save_cache(&self) -> Result<(), std::io::Error> {
        for (cell_id, cell) in &self.cell_cache {
            Self::save_cell(&self.working_directory.join(cell_id.path()), cell)?;
        }

        Ok(())
    }

    pub fn save_metadata(&self) -> Result<(), std::io::Error> {
        let path = self
            .working_directory
            .join(Metadata::FILE_NAME)
            .with_extension(Metadata::EXTENSION);

        let file = File::create(path)?;
        let mut buf_writer = BufWriter::new(file);
        self.metadata.write_to(&mut buf_writer)?;
        buf_writer.flush()?;

        Ok(())
    }
}

impl Drop for Converter {
    fn drop(&mut self) {
        self.save_cache().unwrap();
        self.save_metadata().unwrap();
    }
}
