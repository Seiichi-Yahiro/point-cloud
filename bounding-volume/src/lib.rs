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

    pub fn extend_aabb(&mut self, other: &Self) {
        self.min = self.min.min(other.min);
        self.max = self.max.max(other.max);
    }

    pub fn clamp(&mut self, min: Vec3, max: Vec3) {
        self.min = self.min.max(min);
        self.max = self.max.min(max);
    }

    pub fn from<T: IntoIterator<Item = Vec3>>(iterator: T) -> Option<Self> {
        let mut point_iter = iterator.into_iter();

        if let Some(first_point) = point_iter.next() {
            let mut aabb = Self::new(first_point, first_point);

            for point in point_iter {
                aabb.extend(point);
            }

            Some(aabb)
        } else {
            None
        }
    }
}
