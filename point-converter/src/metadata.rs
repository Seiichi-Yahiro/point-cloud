use std::io::{Read, Write};

use glam::Vec3;
use serde::{Deserialize, Serialize};

use crate::point::Point;

#[derive(Debug, Clone, Serialize, Deserialize)]
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

    pub fn write_to(&self, writer: &mut dyn Write) -> serde_json::Result<()> {
        serde_json::to_writer_pretty(writer, self)
    }

    pub fn read_from(reader: &mut dyn Read) -> serde_json::Result<Self> {
        serde_json::from_reader(reader)
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

#[derive(Debug, Copy, Clone, Default, Serialize, Deserialize)]
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
}
