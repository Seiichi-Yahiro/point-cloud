use std::collections::hash_map::Entry;
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
    pub fn path(&self) -> std::path::PathBuf {
        let mut path = std::path::PathBuf::from(Metadata::hierarchy_string(self.hierarchy));
        path.push(self.index_string());
        path.set_extension(Cell::EXTENSION);
        path
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
    pub(crate) overflow: FxHashMap<IVec3, Option<Vec<Point>>>,
}

impl Cell {
    pub const EXTENSION: &'static str = "bin";

    pub fn new(id: CellId, sub_grid_dimension: u32, size: f32, pos: Vec3, capacity: usize) -> Self {
        Self {
            header: Header::new(id, sub_grid_dimension, size, pos),
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

    pub fn add_point(&mut self, point: Point) -> bool {
        let sub_grid_index = self
            .header
            .sub_grid_index_for_point(point, self.header.sub_grid_dimension);

        if self.points_grid.insert(sub_grid_index) {
            self.points.push(point);
            self.header.total_number_of_points += 1;
            self.header.number_of_points += 1;

            return true;
        }

        false
    }

    pub fn add_points(&mut self, points: Vec<Point>) -> Vec<Point> {
        let mut overflow_points = Vec::with_capacity(points.capacity());

        for point in points {
            if !self.add_point(point) {
                overflow_points.push(point);
            }
        }

        overflow_points
    }

    pub fn add_points_in_overflow(
        &mut self,
        overflow_points: FxHashMap<IVec3, Vec<Point>>,
        config: &MetadataConfig,
    ) -> FxHashMap<IVec3, Vec<Point>> {
        let mut remaining_overflow_points = FxHashMap::default();

        for (cell_index, mut points) in overflow_points {
            match self.overflow.entry(cell_index) {
                Entry::Vacant(entry) => {
                    if points.len() <= config.cell_point_overflow_limit as usize {
                        self.header.total_number_of_points += points.len() as u32;
                        self.header.number_of_overflow_points += points.len() as u32;
                        entry.insert(Some(points));
                    } else {
                        remaining_overflow_points.insert(cell_index, points);
                        entry.insert(None);
                    }
                }
                Entry::Occupied(mut entry) => match entry.get_mut() {
                    None => {
                        remaining_overflow_points.insert(cell_index, points);
                    }
                    Some(cell_points) => {
                        let cell_points_len = cell_points.len() as u32;
                        let points_len = points.len() as u32;

                        cell_points.append(&mut points);

                        if cell_points.len() < config.cell_point_overflow_limit as usize {
                            self.header.total_number_of_points += points_len;
                            self.header.number_of_overflow_points += points_len;
                        } else {
                            self.header.total_number_of_points -= cell_points_len;
                            self.header.number_of_overflow_points -= cell_points_len;

                            let points = entry.insert(None).unwrap();
                            remaining_overflow_points.insert(cell_index, points);
                        }
                    }
                },
            }
        }

        remaining_overflow_points
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

    pub fn read_from(reader: &mut dyn Read) -> Result<Self, std::io::Error> {
        let header = Header::read_from(reader)?;

        let mut points = Vec::with_capacity(header.number_of_points as usize);

        let mut points_grid = FxHashSet::with_capacity_and_hasher(
            header.number_of_points as usize,
            BuildHasherDefault::default(),
        );

        for _ in 0..header.number_of_points {
            let point = Point::read_from(reader)?;

            let sub_grid_index = header.sub_grid_index_for_point(point, header.sub_grid_dimension);

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

    pub fn from_path<T: AsRef<Path>>(path: T) -> Result<Self, std::io::Error> {
        let file = std::fs::File::open(path)?;
        let mut buf_reader = std::io::BufReader::new(file);
        Self::read_from(&mut buf_reader)
    }
}

#[derive(Debug, Clone)]
pub struct Header {
    /// A unique id over all cells
    pub id: CellId,

    /// Number of points in this cell.
    pub total_number_of_points: u32,

    /// Number of points.
    pub number_of_points: u32,

    /// Number of overflowing points which would belong to the next hierarchy.
    pub number_of_overflow_points: u32,

    /// Inner sub grid. [sub_grid_dimension]^3 is the number of points a cell can hold.
    pub sub_grid_dimension: u32,

    /// The side length of the cubic cell.
    pub size: f32,

    /// The position of the cell in the world.
    /// This is the center of the cell.
    pub pos: Vec3,
}

impl Header {
    pub fn new(id: CellId, sub_grid_dimension: u32, size: f32, pos: Vec3) -> Self {
        Self {
            id,
            total_number_of_points: 0,
            number_of_points: 0,
            number_of_overflow_points: 0,
            sub_grid_dimension,
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

        writer.write_u32::<Endianess>(self.sub_grid_dimension)?;
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

        let sub_grid_dimension = reader.read_u32::<Endianess>()?;
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
            sub_grid_dimension,
            size,
            pos,
        })
    }
}
