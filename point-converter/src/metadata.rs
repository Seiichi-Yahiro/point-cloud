use std::io::{Read, Write};

use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use glam::Vec3;

use crate::point::Point;

#[derive(Debug, Clone)]
pub struct Metadata {
    /// Total number of points.
    pub number_of_points: u64,

    /// Number of existing hierarchy levels.
    pub hierarchies: u32,

    /// Number of points a cell can hold.
    pub cell_point_limit: u32,

    /// Number of points a cell can hold additionally before creating new cells in the next lower
    /// hierarchy level.
    pub cell_point_overflow_limit: u32,

    /// [sub_grid_dimension]^3 is the number of points a cell can hold.
    /// Doesn't count for minimum sized cells of the lowest hierarchy level.
    pub sub_grid_dimension: u32,

    /// Size of the largest cell of the largest hierarchy level.
    pub max_cell_size: f32,

    /// A 3D Bounding box of the point cloud with min max values for every dimension.
    pub bounding_box: BoundingBox,
}

impl Metadata {
    pub fn number_of_sub_grid_cells(&self) -> u32 {
        self.sub_grid_dimension.pow(3)
    }

    pub fn write_to(&self, writer: &mut dyn Write) -> Result<(), std::io::Error> {
        writer.write_u64::<BigEndian>(self.number_of_points)?;
        writer.write_u32::<BigEndian>(self.hierarchies)?;
        writer.write_u32::<BigEndian>(self.cell_point_limit)?;
        writer.write_u32::<BigEndian>(self.cell_point_overflow_limit)?;
        writer.write_u32::<BigEndian>(self.sub_grid_dimension)?;
        writer.write_f32::<BigEndian>(self.max_cell_size)?;
        self.bounding_box.write_to(writer)?;

        Ok(())
    }

    pub fn read_from(reader: &mut dyn Read) -> Result<Self, std::io::Error> {
        let number_of_points = reader.read_u64::<BigEndian>()?;
        let hierarchies = reader.read_u32::<BigEndian>()?;
        let cell_point_limit = reader.read_u32::<BigEndian>()?;
        let cell_point_overflow_limit = reader.read_u32::<BigEndian>()?;
        let sub_grid_dimension = reader.read_u32::<BigEndian>()?;
        let max_cell_size = reader.read_f32::<BigEndian>()?;
        let bounding_box = BoundingBox::read_from(reader)?;

        Ok(Self {
            number_of_points,
            hierarchies,
            cell_point_limit,
            cell_point_overflow_limit,
            sub_grid_dimension,
            max_cell_size,
            bounding_box,
        })
    }
}

impl Default for Metadata {
    fn default() -> Self {
        Self {
            number_of_points: 0,
            max_cell_size: 1000.0,
            hierarchies: 0,
            bounding_box: BoundingBox::default(),
            sub_grid_dimension: 128,
            cell_point_overflow_limit: 30_000,
            cell_point_limit: 100_000,
        }
    }
}

#[derive(Debug, Copy, Clone, Default)]
/// A 3D Bounding box with min max values .
pub struct BoundingBox {
    pub min: Vec3,
    pub max: Vec3,
}

// TODO Bounding box can be wrong as it always contains the origin
impl BoundingBox {
    pub fn new(min: Vec3, max: Vec3) -> Self {
        Self { min, max }
    }

    pub fn extend(&mut self, point: Point) {
        self.min = self.min.min(point.pos);
        self.max = self.max.max(point.pos);
    }

    pub fn write_to(&self, writer: &mut dyn Write) -> Result<(), std::io::Error> {
        writer.write_f32::<BigEndian>(self.min.x)?;
        writer.write_f32::<BigEndian>(self.min.y)?;
        writer.write_f32::<BigEndian>(self.min.z)?;

        writer.write_f32::<BigEndian>(self.max.x)?;
        writer.write_f32::<BigEndian>(self.max.y)?;
        writer.write_f32::<BigEndian>(self.max.z)?;

        Ok(())
    }

    pub fn read_from(reader: &mut dyn Read) -> Result<Self, std::io::Error> {
        let x_min = reader.read_f32::<BigEndian>()?;
        let y_min = reader.read_f32::<BigEndian>()?;
        let z_min = reader.read_f32::<BigEndian>()?;

        let x_max = reader.read_f32::<BigEndian>()?;
        let y_max = reader.read_f32::<BigEndian>()?;
        let z_max = reader.read_f32::<BigEndian>()?;

        Ok(Self {
            min: Vec3::new(x_min, y_min, z_min),
            max: Vec3::new(x_max, y_max, z_max),
        })
    }
}
