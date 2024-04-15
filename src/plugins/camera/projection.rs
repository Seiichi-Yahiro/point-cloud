use bevy_ecs::prelude::Component;
use glam::Mat4;

#[derive(Debug, Clone, Component)]
pub struct PerspectiveProjection {
    pub fov_y: f32,
    pub aspect_ratio: f32,
    pub near: f32,
    pub far: f32,
}

impl PerspectiveProjection {
    pub fn compute_matrix(&self) -> Mat4 {
        Mat4::perspective_rh(self.fov_y, self.aspect_ratio, self.near, self.far)
    }

    pub fn slope(&self) -> f32 {
        (self.fov_y * 0.5).tan()
    }
}

impl Default for PerspectiveProjection {
    fn default() -> Self {
        Self {
            fov_y: std::f32::consts::FRAC_PI_4,
            aspect_ratio: 1.0,
            near: 1.0,
            far: 1000.0,
        }
    }
}
