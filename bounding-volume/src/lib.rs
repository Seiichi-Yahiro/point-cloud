use glam::Vec3;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb {
    pub fn new(min: Vec3, max: Vec3) -> Self {
        Self { min, max }
    }

    pub fn center(&self) -> Vec3 {
        (self.min + self.max) / 2.0
    }

    pub fn extends(&self) -> Vec3 {
        (self.max - self.min) / 2.0
    }

    pub fn extend(&mut self, point: Vec3) {
        self.min = self.min.min(point);
        self.max = self.max.max(point);
    }

    pub fn clamp(&mut self, min: Vec3, max: Vec3) {
        self.min = self.min.max(min);
        self.max = self.max.min(max);
    }
}
