use bevy_easings::Lerp;
use bevy_ecs::prelude::Component;
use glam::{Mat3, Mat4, Quat, Vec3};
use std::ops::Mul;

#[derive(Debug, PartialEq, Copy, Clone, Component)]
pub struct Transform {
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl Lerp for Transform {
    type Scalar = f32;

    fn lerp(&self, other: &Self, scalar: &Self::Scalar) -> Self {
        Transform {
            translation: self.translation.lerp(other.translation, *scalar),
            scale: self.scale.lerp(other.scale, *scalar),
            rotation: self.rotation.lerp(other.rotation, *scalar),
        }
    }
}

impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Transform {
    pub const IDENTITY: Self = Transform {
        translation: Vec3::ZERO,
        rotation: Quat::IDENTITY,
        scale: Vec3::ONE,
    };

    pub const fn from_xyz(x: f32, y: f32, z: f32) -> Self {
        Self::from_translation(Vec3::new(x, y, z))
    }

    pub const fn from_translation(translation: Vec3) -> Self {
        Transform {
            translation,
            ..Self::IDENTITY
        }
    }

    pub const fn from_rotation(rotation: Quat) -> Self {
        Transform {
            rotation,
            ..Self::IDENTITY
        }
    }

    pub const fn from_scale(scale: Vec3) -> Self {
        Transform {
            scale,
            ..Self::IDENTITY
        }
    }

    pub fn looking_at(mut self, target: Vec3, up: Vec3) -> Self {
        self.look_at(target, up);
        self
    }

    pub fn look_at(&mut self, target: Vec3, up: Vec3) {
        self.look_to(target - self.translation, up);
    }

    pub fn look_to(&mut self, direction: Vec3, up: Vec3) {
        let back = -direction.try_normalize().unwrap_or(Vec3::NEG_Z);
        let up = up.try_normalize().unwrap_or(Vec3::Y);
        let right = up
            .cross(back)
            .try_normalize()
            .unwrap_or_else(|| up.any_orthonormal_vector());
        let up = back.cross(right);
        self.rotation = Quat::from_mat3(&Mat3::from_cols(right, up, back));
    }

    #[inline]
    pub fn local_x(&self) -> Vec3 {
        self.rotation * Vec3::X
    }

    #[inline]
    pub fn left(&self) -> Vec3 {
        -self.local_x()
    }

    #[inline]
    pub fn right(&self) -> Vec3 {
        self.local_x()
    }

    #[inline]
    pub fn local_y(&self) -> Vec3 {
        self.rotation * Vec3::Y
    }

    #[inline]
    pub fn up(&self) -> Vec3 {
        self.local_y()
    }

    #[inline]
    pub fn down(&self) -> Vec3 {
        -self.local_y()
    }

    #[inline]
    pub fn local_z(&self) -> Vec3 {
        self.rotation * Vec3::Z
    }

    #[inline]
    pub fn forward(&self) -> Vec3 {
        -self.local_z()
    }

    #[inline]
    pub fn back(&self) -> Vec3 {
        self.local_z()
    }

    pub fn mul_transform(&self, transform: Transform) -> Self {
        let translation = self.transform_point(transform.translation);
        let rotation = self.rotation * transform.rotation;
        let scale = self.scale * transform.scale;
        Transform {
            translation,
            rotation,
            scale,
        }
    }

    pub fn transform_point(&self, mut point: Vec3) -> Vec3 {
        point = self.scale * point;
        point = self.rotation * point;
        point += self.translation;
        point
    }

    pub fn compute_matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.translation)
    }
}

impl Mul<Transform> for Transform {
    type Output = Transform;

    fn mul(self, transform: Transform) -> Self::Output {
        self.mul_transform(transform)
    }
}

impl Mul<Vec3> for Transform {
    type Output = Vec3;

    fn mul(self, value: Vec3) -> Self::Output {
        self.transform_point(value)
    }
}
