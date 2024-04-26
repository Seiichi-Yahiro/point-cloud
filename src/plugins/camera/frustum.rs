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

impl Corners {
    pub fn iter(&self) -> std::array::IntoIter<&Vec3, 4> {
        [
            &self.top_left,
            &self.top_right,
            &self.bottom_left,
            &self.bottom_right,
        ]
        .into_iter()
    }
}

impl<'a> IntoIterator for &'a Corners {
    type Item = &'a Vec3;
    type IntoIter = std::array::IntoIter<&'a Vec3, 4>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// Planes in Hessian normal form.
///
/// ax + by + cz - d = 0\
/// (a,b,c) is the normal\
/// d is the distance from the origin along the normal
///
/// Encoded into a Vec4 where (x,y,z) is the normal and w the distance.
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

    /// Calculates if the provided aabb should be culled.
    pub fn cull_aabb(&self, aabb: Aabb) -> bool {
        for plane in self.iter() {
            let corner_x = if plane.x >= 0.0 {
                aabb.max.x
            } else {
                aabb.min.x
            };

            let corner_y = if plane.y >= 0.0 {
                aabb.max.y
            } else {
                aabb.min.y
            };

            let corner_z = if plane.z >= 0.0 {
                aabb.max.z
            } else {
                aabb.min.z
            };

            let nearest_corner = Vec4::new(corner_x, corner_y, corner_z, -1.0);

            // calculate signed distance
            // negative means outside (behind the plane)
            if plane.dot(nearest_corner) <= 0.0 {
                return true;
            }
        }

        false
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
    pub fn new(camera_transform: &Transform, projection: &PerspectiveProjection) -> Self {
        let near = Self::near_corners(camera_transform, projection);
        let far = Self::far_corners(camera_transform, projection);

        let cam_pos = camera_transform.translation;
        let cam_forward = camera_transform.forward();

        let center_on_near_plane = cam_pos + projection.near * cam_forward;
        let center_on_far_plane = cam_pos + projection.far * cam_forward;

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

        let near_plane = normal_near.extend(center_on_near_plane.dot(normal_near));
        let far_plane = normal_far.extend(center_on_far_plane.dot(normal_far));
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

    pub fn near_corners(
        camera_transform: &Transform,
        projection: &PerspectiveProjection,
    ) -> Corners {
        let half_height_near = projection.near * projection.slope();
        let half_width_near = half_height_near * projection.aspect_ratio;

        let near_up = camera_transform.up() * half_height_near;
        let near_right = camera_transform.right() * half_width_near;

        let center_on_near_plane =
            camera_transform.translation + projection.near * camera_transform.forward();

        Corners {
            top_left: center_on_near_plane + near_up - near_right,
            top_right: center_on_near_plane + near_up + near_right,
            bottom_left: center_on_near_plane - near_up - near_right,
            bottom_right: center_on_near_plane - near_up + near_right,
        }
    }

    pub fn far_corners(
        camera_transform: &Transform,
        projection: &PerspectiveProjection,
    ) -> Corners {
        let half_height_far = projection.far * projection.slope();
        let half_width_far = half_height_far * projection.aspect_ratio;

        let far_up = camera_transform.up() * half_height_far;
        let far_right = camera_transform.right() * half_width_far;

        let center_on_far_plane =
            camera_transform.translation + projection.far * camera_transform.forward();

        Corners {
            top_left: center_on_far_plane + far_up - far_right,
            top_right: center_on_far_plane + far_up + far_right,
            bottom_left: center_on_far_plane - far_up - far_right,
            bottom_right: center_on_far_plane - far_up + far_right,
        }
    }

    /// Calculates if the provided aabb should be culled.
    pub fn cull_aabb(&self, aabb: Aabb) -> bool {
        self.planes.cull_aabb(aabb)
    }

    pub fn aabb(&self) -> Aabb {
        let mut corners_iter = self.near.iter().chain(self.far.iter()).copied();
        let first_corner = corners_iter.next().unwrap();

        corners_iter.fold(Aabb::new(first_corner, first_corner), |mut acc, corner| {
            acc.extend(corner);
            acc
        })
    }
}

// TODO move to different module
#[derive(Debug, Clone, Copy, Component)]
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
