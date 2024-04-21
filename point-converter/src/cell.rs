use std::hash::BuildHasherDefault;
use std::io::{Read, Write};
use std::path::Path;

use byteorder::{ReadBytesExt, WriteBytesExt};
use glam::{IVec3, UVec3, Vec3};
use rand::seq::SliceRandom;
use rand::thread_rng;
use rustc_hash::FxHashSet;

use crate::metadata::Metadata;
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
    grid: Option<FxHashSet<u32>>,
}

impl Cell {
    pub const EXTENSION: &'static str = "bin";

    pub fn new(id: CellId, size: f32, pos: Vec3, capacity: usize) -> Self {
        Self {
            header: Header::new(id, size, pos),
            points: Vec::with_capacity(capacity),
            grid: None,
        }
    }

    pub fn header(&self) -> &Header {
        &self.header
    }

    pub fn points(&self) -> &[Point] {
        &self.points
    }

    pub fn add_point(
        &mut self,
        point: Point,
        metadata: &Metadata,
    ) -> Result<(), CellAddPointError> {
        if let Some(grid) = &mut self.grid {
            if (self.points.len() as u32) >= metadata.cell_point_limit {
                return Err(CellAddPointError::PointLimitReached);
            }

            let sub_grid_index = self
                .header
                .sub_grid_index_for_point(point, metadata.sub_grid_dimension);

            if grid.insert(sub_grid_index) {
                self.points.push(point);
                self.header.number_of_points += 1;

                Ok(())
            } else {
                Err(CellAddPointError::GridPositionOccupied)
            }
        } else if (self.points.len() as u32)
            < metadata.cell_point_limit + metadata.cell_point_overflow_limit
        {
            self.points.push(point);
            self.header.number_of_points += 1;

            Ok(())
        } else {
            Err(CellAddPointError::OverflowLimitReached)
        }
    }

    pub fn apply_grid_and_extract_overflow(&mut self, metadata: &Metadata) -> Vec<Point> {
        let mut grid = FxHashSet::with_capacity_and_hasher(
            self.points.len() / 2,
            BuildHasherDefault::default(),
        );

        self.points.shuffle(&mut thread_rng());

        let (points, overflow) = std::mem::take(&mut self.points)
            .into_iter()
            .partition(|point| {
                let sub_grid_index = self
                    .header
                    .sub_grid_index_for_point(*point, metadata.sub_grid_dimension);

                grid.insert(sub_grid_index)
            });

        self.points = points;
        self.header.number_of_points -= overflow.len() as u32;

        self.grid = Some(grid);
        self.header.has_grid = true;

        overflow
    }

    pub fn write_to(&self, writer: &mut dyn Write) -> Result<(), std::io::Error> {
        self.header.write_to(writer)?;

        for point in &self.points {
            point.write_to(writer)?;
        }

        Ok(())
    }

    pub fn read_from(
        reader: &mut dyn Read,
        sub_grid_dimension: u32,
    ) -> Result<Self, std::io::Error> {
        let header = Header::read_from(reader)?;

        let mut points = Vec::with_capacity(header.number_of_points as usize);

        let grid = if header.has_grid {
            let mut grid = FxHashSet::with_capacity_and_hasher(
                header.number_of_points as usize,
                BuildHasherDefault::default(),
            );

            for _ in 0..header.number_of_points {
                let point = Point::read_from(reader)?;
                let sub_grid_index = header.sub_grid_index_for_point(point, sub_grid_dimension);

                grid.insert(sub_grid_index);
                points.push(point);
            }

            Some(grid)
        } else {
            for _ in 0..header.number_of_points {
                let point = Point::read_from(reader)?;
                points.push(point);
            }

            None
        };

        Ok(Self {
            header,
            grid,
            points,
        })
    }

    pub fn from_path<T: AsRef<Path>>(
        path: T,
        sub_grid_dimension: u32,
    ) -> Result<Self, std::io::Error> {
        let file = std::fs::File::open(path)?;
        let mut buf_reader = std::io::BufReader::new(file);
        Self::read_from(&mut buf_reader, sub_grid_dimension)
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum CellAddPointError {
    /// The cell is full.
    PointLimitReached,
    /// The cell is overflowing and needs to reduce points.
    OverflowLimitReached,
    /// The grid cell was already occupied by another point.
    GridPositionOccupied,
}

#[derive(Debug, Clone)]
pub struct Header {
    /// A unique id over all cells
    pub id: CellId,

    /// Number of points in this cell.
    pub number_of_points: u32,

    /// Does a grid exists?
    pub has_grid: bool,

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
            number_of_points: 0,
            has_grid: false,
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

        writer.write_u32::<Endianess>(self.number_of_points)?;
        writer.write_u8(self.has_grid as u8)?;
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

        let number_of_points = reader.read_u32::<Endianess>()?;
        let has_grid = reader.read_u8()? != 0;
        let size = reader.read_f32::<Endianess>()?;

        let pos = {
            let x = reader.read_f32::<Endianess>()?;
            let y = reader.read_f32::<Endianess>()?;
            let z = reader.read_f32::<Endianess>()?;
            Vec3::new(x, y, z)
        };

        Ok(Self {
            id,
            number_of_points,
            has_grid,
            size,
            pos,
        })
    }
}
