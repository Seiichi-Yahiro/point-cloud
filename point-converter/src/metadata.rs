use std::io::{ErrorKind, Read, Write};
use std::path::Path;

use glam::{IVec3, Vec3, Vec3Swizzles};
use serde::{Deserialize, Serialize};

use crate::point::Point;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metadata {
    /// The current version of this metadata file.
    pub version: String,

    /// The name of the point cloud.
    pub name: String,

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

    pub fn cell_size(&self, hierarchy: u32) -> f32 {
        self.max_cell_size / 2u32.pow(hierarchy) as f32
    }

    pub fn cell_index(&self, pos: Vec3, cell_size: f32) -> IVec3 {
        (pos / cell_size).round().as_ivec3()
    }

    pub fn write_to(&self, writer: &mut dyn Write) -> serde_json::Result<()> {
        serde_json::to_writer_pretty(writer, self)
    }

    pub fn read_from(reader: &mut dyn Read) -> serde_json::Result<Self> {
        serde_json::from_reader(reader)
    }

    pub fn from_path<T: AsRef<Path>>(path: T) -> Result<Self, std::io::Error> {
        let file = std::fs::File::open(path)?;
        let buf_reader = std::io::BufReader::new(file);
        serde_json::from_reader(buf_reader)
            .map_err(|err| std::io::Error::new(ErrorKind::InvalidData, err))
    }
}

impl Default for Metadata {
    fn default() -> Self {
        Self {
            version: "1.0".to_string(),
            name: "Unknown".to_string(),
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

    pub fn flip_yz(&self) -> Self {
        let flip_z = Vec3::new(1.0, 1.0, -1.0);
        let min = self.min.xzy() * flip_z;
        let max = self.max.xzy() * flip_z;
        Self { min, max }
    }
}
