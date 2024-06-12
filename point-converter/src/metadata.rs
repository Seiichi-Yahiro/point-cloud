use std::io::{ErrorKind, Read, Write};
use std::path::Path;

use glam::{IVec3, Vec3};
use serde::{Deserialize, Serialize};

use bounding_volume::Aabb;

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

    /// A 3D Bounding box of the point cloud with min max values for every dimension.
    pub bounding_box: Aabb,

    /// Configuration
    pub config: MetadataConfig,
}

impl Default for Metadata {
    fn default() -> Self {
        Self {
            version: "1.0".to_string(),
            name: "Unknown".to_string(),
            number_of_points: 0,
            hierarchies: 0,
            bounding_box: Aabb::default(),
            config: MetadataConfig::default(),
        }
    }
}

impl Metadata {
    pub const FILE_NAME: &'static str = "metadata";
    pub const EXTENSION: &'static str = "json";

    pub fn hierarchy_string(hierarchy: u32) -> String {
        format!("h_{}", hierarchy)
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetadataConfig {
    /// Number of points a cell can hold additionally before creating new cells in the next lower
    /// hierarchy level.
    pub cell_point_overflow_limit: u32,

    /// [sub_grid_dimension]^3 is the number of points a cell can hold.
    pub sub_grid_dimension: u32,

    /// Size of the largest cell of the largest hierarchy level.
    pub max_cell_size: f32,
}

impl Default for MetadataConfig {
    fn default() -> Self {
        Self {
            sub_grid_dimension: 96,
            cell_point_overflow_limit: 5_000, // smaller values give better quality but more small files
            max_cell_size: 1000.0,
        }
    }
}

impl MetadataConfig {
    pub fn cell_size(&self, hierarchy: u32) -> f32 {
        self.max_cell_size / 2u32.pow(hierarchy) as f32
    }

    pub fn cell_index(&self, pos: Vec3, cell_size: f32) -> IVec3 {
        (pos / cell_size).floor().as_ivec3()
    }

    pub fn cell_pos(&self, cell_index: IVec3, cell_size: f32) -> Vec3 {
        cell_index.as_vec3() * cell_size + cell_size / 2.0
    }

    pub fn cell_spacing(&self, cell_size: f32) -> f32 {
        let cell_size = cell_size / self.sub_grid_dimension as f32;
        let cell_radius = cell_size * 0.5;
        cell_radius.hypot(cell_radius * 0.5) * 1.05
    }
}
