use bevy_ecs::prelude::Component;
use glam::{Vec3, Vec4};

use crate::plugins::camera::projection::PerspectiveProjection;
use crate::transform::Transform;

#[derive(Debug, Default, Clone)]
pub struct Corners {
    pub top_left: Vec3,
    pub top_right: Vec3,
    pub bottom_left: Vec3,
    pub bottom_right: Vec3,
}

/// Planes in Hessian normal form.
///
/// ax + by + cz + d = 0\
/// (a,b,c) is the normal\
/// d is the distance from the origin along the normal
///
/// Encoded into a Vec3 where (x,y,z) is the normal and w the distance.
#[derive(Debug, Default, Clone)]
pub struct Planes {
    pub near: Vec4,
    pub far: Vec4,
    pub top: Vec4,
    pub bottom: Vec4,
    pub left: Vec4,
    pub right: Vec4,
}

impl Planes {
    pub fn iter(&self) -> std::array::IntoIter<&Vec4, 6> {
        [
            &self.near,
            &self.far,
            &self.top,
            &self.bottom,
            &self.left,
            &self.right,
        ]
        .into_iter()
    }
}

impl<'a> IntoIterator for &'a Planes {
    type Item = &'a Vec4;
    type IntoIter = std::array::IntoIter<&'a Vec4, 6>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

#[derive(Debug, Default, Clone, Component)]
pub struct Frustum {
    pub near: Corners,
    pub far: Corners,
    pub planes: Planes,
}

impl Frustum {
    pub fn new(transform: &Transform, projection: &PerspectiveProjection) -> Self {
        let cam_pos = transform.translation;
        let cam_forward = transform.forward();
        let cam_right = transform.right();
        let cam_up = transform.up();

        let slope = (projection.fov_y * 0.5).tan();

        let half_height_near = projection.near * slope;
        let half_width_near = half_height_near * projection.aspect_ratio;

        let half_height_far = projection.far * slope;
        let half_width_far = half_height_far * projection.aspect_ratio;

        let center_on_near_plane = cam_pos + projection.near * cam_forward;
        let center_on_far_plane = cam_pos + projection.far * cam_forward;

        let near_up = cam_up * half_height_near;
        let near_right = cam_right * half_width_near;

        let near = Corners {
            top_left: center_on_near_plane + near_up - near_right,
            top_right: center_on_near_plane + near_up + near_right,
            bottom_left: center_on_near_plane - near_up - near_right,
            bottom_right: center_on_near_plane - near_up + near_right,
        };

        let far_up = cam_up * half_height_far;
        let far_right = cam_right * half_width_far;

        let far = Corners {
            top_left: center_on_far_plane + far_up - far_right,
            top_right: center_on_far_plane + far_up + far_right,
            bottom_left: center_on_far_plane - far_up - far_right,
            bottom_right: center_on_far_plane - far_up + far_right,
        };

        let normal_near = cam_forward;
        let normal_far = -cam_forward;
        let normal_top = (near.top_left - cam_pos)
            .cross(near.top_right - cam_pos)
            .normalize_or_zero();
        let normal_bottom = (near.bottom_right - cam_pos)
            .cross(near.bottom_left - cam_pos)
            .normalize_or_zero();
        let normal_left = (near.bottom_left - cam_pos)
            .cross(near.top_left - cam_pos)
            .normalize_or_zero();
        let normal_right = (near.top_right - cam_pos)
            .cross(near.bottom_right - cam_pos)
            .normalize_or_zero();

        let near_plane = normal_near.extend((cam_forward * projection.near).dot(normal_near));
        let far_plane = normal_far.extend((cam_forward * projection.far).dot(normal_far));
        let top_plane = normal_top.extend(cam_pos.dot(normal_top));
        let bottom_plane = normal_bottom.extend(cam_pos.dot(normal_bottom));
        let left_plane = normal_left.extend(cam_pos.dot(normal_left));
        let right_plane = normal_right.extend(cam_pos.dot(normal_right));

        Self {
            near,
            far,
            planes: Planes {
                near: near_plane,
                far: far_plane,
                top: top_plane,
                bottom: bottom_plane,
                left: left_plane,
                right: right_plane,
            },
        }
    }
}
