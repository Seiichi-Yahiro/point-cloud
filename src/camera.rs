use glam::{Mat4, Vec3};

use crate::transform::Transform;

#[derive(Debug, Default, Clone)]
pub struct Camera {
    pub transform: Transform,
    pub projection: PerspectiveProjection,
}

impl Camera {
    pub fn new(position: Vec3, looking_at: Vec3) -> Self {
        Self {
            transform: Transform::from_translation(position).looking_at(looking_at, Vec3::Y),
            projection: PerspectiveProjection::default(),
        }
    }

    pub fn get_view_matrix(&self) -> Mat4 {
        self.transform.compute_matrix().inverse()
    }

    pub fn get_projection_matrix(&self) -> Mat4 {
        self.projection.get_projection_matrix()
    }

    pub fn get_view_projection_matrix(&self) -> Mat4 {
        self.get_projection_matrix() * self.get_view_matrix()
    }
}

#[derive(Debug, Clone)]
pub struct PerspectiveProjection {
    pub fov_y: f32,
    pub aspect_ratio: f32,
    pub near: f32,
    pub far: f32,
}

impl PerspectiveProjection {
    pub fn get_projection_matrix(&self) -> Mat4 {
        Mat4::perspective_rh(self.fov_y, self.aspect_ratio, self.near, self.far)
    }
}

impl Default for PerspectiveProjection {
    fn default() -> Self {
        Self {
            fov_y: std::f32::consts::FRAC_PI_4,
            aspect_ratio: 1.0,
            near: 0.1,
            far: 1000.0,
        }
    }
}
