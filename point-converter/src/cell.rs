use std::hash::BuildHasherDefault;
use std::io::{Read, Write};
use std::path::Path;

use byteorder::{ReadBytesExt, WriteBytesExt};
use glam::{IVec3, UVec3, Vec3};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::metadata::{Metadata, MetadataConfig};
use crate::point::Point;
use crate::Endianess;

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct CellId {
    pub hierarchy: u32,
    pub index: IVec3,
}

impl CellId {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn path(&self, working_dir: &Path) -> std::path::PathBuf {
        working_dir
            .join(Metadata::hierarchy_string(self.hierarchy))
            .join(self.index_string())
            .with_extension(Cell::EXTENSION)
    }

    #[cfg(target_arch = "wasm32")]
    pub fn path(&self) -> [String; 2] {
        [
            Metadata::hierarchy_string(self.hierarchy),
            format!("{}.{}", self.index_string(), Cell::EXTENSION),
        ]
    }

    pub fn index_string(&self) -> String {
        format!("c_{}_{}_{}", self.index.x, self.index.y, self.index.z)
    }
}

#[derive(Debug)]
pub struct Cell {
    header: Header,
    points: Vec<Point>,
    points_grid: FxHashSet<u32>,
    overflow: FxHashMap<IVec3, Option<Vec<Point>>>,
}

impl Cell {
    pub const EXTENSION: &'static str = "bin";

    pub fn new(id: CellId, size: f32, pos: Vec3, capacity: usize) -> Self {
        Self {
            header: Header::new(id, size, pos),
            points: Vec::with_capacity(capacity),
            points_grid: FxHashSet::with_capacity_and_hasher(
                capacity,
                BuildHasherDefault::default(),
            ),
            overflow: FxHashMap::default(),
        }
    }

    pub fn header(&self) -> &Header {
        &self.header
    }

    pub fn points(&self) -> &[Point] {
        &self.points
    }

    pub fn overflow_points(&self) -> impl Iterator<Item = &Point> {
        self.overflow
            .values()
            .filter_map(|sub_cell| sub_cell.as_ref().map(|points| points.iter()))
            .flatten()
    }

    pub fn all_points(&self) -> impl Iterator<Item = &Point> {
        self.points.iter().chain(self.overflow_points())
    }

    pub fn add_point(&mut self, point: Point, config: &MetadataConfig) -> bool {
        let sub_grid_index = self
            .header
            .sub_grid_index_for_point(point, config.sub_grid_dimension);

        if self.points_grid.insert(sub_grid_index) {
            self.points.push(point);
            self.header.total_number_of_points += 1;
            self.header.number_of_points += 1;

            return true;
        }

        false
    }

    pub fn add_point_in_overflow(
        &mut self,
        next_cell_index: IVec3,
        point: Point,
        config: &MetadataConfig,
    ) -> AddPointOverflowResult {
        let next_cell = self.overflow.entry(next_cell_index).or_insert_with(|| {
            Some(Vec::with_capacity(
                config.cell_point_overflow_limit as usize,
            ))
        });

        if let Some(next_cell) = next_cell {
            return if next_cell.len() < config.cell_point_overflow_limit as usize {
                next_cell.push(point);
                self.header.total_number_of_points += 1;
                self.header.number_of_overflow_points += 1;

                AddPointOverflowResult::Success
            } else {
                AddPointOverflowResult::Full
            };
        }

        AddPointOverflowResult::Closed
    }

    pub fn close_overflow(&mut self, next_cell_index: IVec3) -> Vec<Point> {
        let overflow = self.overflow.get_mut(&next_cell_index).unwrap();
        let overflow = overflow.take().unwrap();

        self.header.total_number_of_points -= self.overflow.len() as u32;
        self.header.number_of_overflow_points -= self.overflow.len() as u32;

        overflow
    }

    pub fn write_to(&self, writer: &mut dyn Write) -> Result<(), std::io::Error> {
        self.header.write_to(writer)?;

        for point in &self.points {
            point.write_to(writer)?;
        }

        writer.write_u8(self.overflow.len() as u8)?;

        for (next_cell_index, points) in &self.overflow {
            writer.write_i32::<Endianess>(next_cell_index.x)?;
            writer.write_i32::<Endianess>(next_cell_index.y)?;
            writer.write_i32::<Endianess>(next_cell_index.z)?;

            if let Some(points) = points {
                writer.write_u32::<Endianess>(points.len() as u32)?;

                for point in points {
                    point.write_to(writer)?;
                }
            } else {
                writer.write_u32::<Endianess>(0)?;
            }
        }

        Ok(())
    }

    pub fn read_from(
        reader: &mut dyn Read,
        config: &MetadataConfig,
    ) -> Result<Self, std::io::Error> {
        let header = Header::read_from(reader)?;

        let mut points = Vec::with_capacity(header.number_of_points as usize);

        let mut points_grid = FxHashSet::with_capacity_and_hasher(
            header.number_of_points as usize,
            BuildHasherDefault::default(),
        );

        for _ in 0..header.number_of_points {
            let point = Point::read_from(reader)?;

            let sub_grid_index = header.sub_grid_index_for_point(point, config.sub_grid_dimension);

            points_grid.insert(sub_grid_index);
            points.push(point);
        }

        let overflow_len = reader.read_u8()? as usize;
        let mut overflow =
            FxHashMap::with_capacity_and_hasher(overflow_len, BuildHasherDefault::default());

        for _ in 0..overflow_len {
            let key = {
                let x = reader.read_i32::<Endianess>()?;
                let y = reader.read_i32::<Endianess>()?;
                let z = reader.read_i32::<Endianess>()?;
                IVec3::new(x, y, z)
            };

            let number_of_overflow_points = reader.read_u32::<Endianess>()? as usize;

            if number_of_overflow_points == 0 {
                overflow.insert(key, None);
            } else {
                let mut overflow_points = Vec::with_capacity(number_of_overflow_points);

                for _ in 0..number_of_overflow_points {
                    let point = Point::read_from(reader)?;
                    overflow_points.push(point);
                }

                overflow.insert(key, Some(overflow_points));
            }
        }

        Ok(Self {
            header,
            points,
            points_grid,
            overflow,
        })
    }

    pub fn from_path<T: AsRef<Path>>(
        path: T,
        config: &MetadataConfig,
    ) -> Result<Self, std::io::Error> {
        let file = std::fs::File::open(path)?;
        let mut buf_reader = std::io::BufReader::new(file);
        Self::read_from(&mut buf_reader, config)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum AddPointOverflowResult {
    Success,
    Full,
    Closed,
}

#[derive(Debug, Clone)]
pub struct Header {
    /// A unique id over all cells
    pub id: CellId,

    /// Number of points in this cell.
    pub total_number_of_points: u32,

    pub number_of_points: u32,

    pub number_of_overflow_points: u32,

    /// The side length of the cubic cell.
    pub size: f32,

    /// The position of the cell in the world.
    /// This is the center of the cell.
    pub pos: Vec3,
}

impl Header {
    pub fn new(id: CellId, size: f32, pos: Vec3) -> Self {
        Self {
            id,
            total_number_of_points: 0,
            number_of_points: 0,
            number_of_overflow_points: 0,
            size,
            pos,
        }
    }

    fn sub_grid_index_for_point(&self, point: Point, sub_grid_dimension: u32) -> u32 {
        let sub_cell_size = self.size / sub_grid_dimension as f32;
        let offset = point.pos - self.pos + self.size / 2.0;

        let sub_cell_id = (offset / sub_cell_size)
            .as_uvec3()
            .min(UVec3::splat(sub_grid_dimension - 1)); // TODO why is min needed? precision problem? or bug?

        sub_cell_id.x
            + sub_cell_id.y * sub_grid_dimension
            + sub_cell_id.z * sub_grid_dimension.pow(2)
    }

    pub fn write_to(&self, writer: &mut dyn Write) -> Result<(), std::io::Error> {
        writer.write_u32::<Endianess>(self.id.hierarchy)?;
        writer.write_i32::<Endianess>(self.id.index.x)?;
        writer.write_i32::<Endianess>(self.id.index.y)?;
        writer.write_i32::<Endianess>(self.id.index.z)?;

        writer.write_u32::<Endianess>(self.total_number_of_points)?;
        writer.write_u32::<Endianess>(self.number_of_points)?;
        writer.write_u32::<Endianess>(self.number_of_overflow_points)?;

        writer.write_f32::<Endianess>(self.size)?;

        writer.write_f32::<Endianess>(self.pos.x)?;
        writer.write_f32::<Endianess>(self.pos.y)?;
        writer.write_f32::<Endianess>(self.pos.z)?;

        Ok(())
    }

    pub fn read_from(reader: &mut dyn Read) -> Result<Self, std::io::Error> {
        let id = {
            let hierarchy = reader.read_u32::<Endianess>()?;
            let x = reader.read_i32::<Endianess>()?;
            let y = reader.read_i32::<Endianess>()?;
            let z = reader.read_i32::<Endianess>()?;
            CellId {
                hierarchy,
                index: IVec3::new(x, y, z),
            }
        };

        let total_number_of_points = reader.read_u32::<Endianess>()?;
        let number_of_points = reader.read_u32::<Endianess>()?;
        let number_of_overflow_points = reader.read_u32::<Endianess>()?;

        let size = reader.read_f32::<Endianess>()?;

        let pos = {
            let x = reader.read_f32::<Endianess>()?;
            let y = reader.read_f32::<Endianess>()?;
            let z = reader.read_f32::<Endianess>()?;
            Vec3::new(x, y, z)
        };

        Ok(Self {
            id,
            total_number_of_points,
            number_of_points,
            number_of_overflow_points,
            size,
            pos,
        })
    }
}
