use std::ops::{Deref, DerefMut};

use bevy_ecs::prelude::*;
use glam::{IVec3, Vec4};
use itertools::Itertools;

use crate::plugins::camera::frustum::{Aabb, Corners, Frustum};
use crate::plugins::camera::projection::PerspectiveProjection;
use crate::plugins::camera::Camera;
use crate::plugins::streaming::metadata::ActiveMetadataRes;
use crate::transform::Transform;

#[derive(Resource)]
pub struct StreamingFrustumsScale(f32);

impl StreamingFrustumsScale {
    const MIN: f32 = 1.0;
    const MAX: f32 = 5.0;
}

impl Default for StreamingFrustumsScale {
    fn default() -> Self {
        Self(2.0)
    }
}

#[derive(Debug)]
pub struct StreamingFrustum {
    pub far_corners: Corners,
    pub far_plane: Vec4,
    pub aabb: Aabb,
    pub min_cell_index: IVec3,
    pub max_cell_index: IVec3,
}

impl StreamingFrustum {
    pub fn cell_indices(&self) -> impl Iterator<Item = IVec3> {
        (self.min_cell_index.x..=self.max_cell_index.x)
            .cartesian_product(self.min_cell_index.y..=self.max_cell_index.y)
            .cartesian_product(self.min_cell_index.z..=self.max_cell_index.z)
            .map(|((x, y), z)| IVec3::new(x, y, z))
    }
}

#[derive(Debug, Component)]
pub struct StreamingFrustums(Vec<StreamingFrustum>);

impl Deref for StreamingFrustums {
    type Target = Vec<StreamingFrustum>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for StreamingFrustums {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

pub fn add_streaming_frustums(mut commands: Commands, camera_query: Query<Entity, With<Camera>>) {
    for entity in camera_query.iter() {
        commands
            .entity(entity)
            .insert(StreamingFrustums(Vec::new()));
    }
}

pub fn update_streaming_frustums(
    active_metadata: ActiveMetadataRes,
    mut camera_query: Query<
        (
            &Transform,
            &PerspectiveProjection,
            Ref<Frustum>,
            &mut StreamingFrustums,
        ),
        With<Camera>,
    >,
    streaming_frustums_scale: Res<StreamingFrustumsScale>,
) {
    let metadata = &active_metadata.metadata;

    for (transform, projection, frustum, mut streaming_frustums) in camera_query.iter_mut() {
        if !(frustum.is_changed() || streaming_frustums_scale.is_changed()) {
            continue;
        }

        let mut new_projection = projection.clone();

        let near_aabb = {
            let mut near_corners_iter = frustum.near.iter().copied();
            let first_corner = near_corners_iter.next().unwrap();
            near_corners_iter.fold(Aabb::new(first_corner, first_corner), |mut acc, corner| {
                acc.extend(corner);
                acc
            })
        };

        let forward = transform.forward();
        let far_normal = frustum.planes.far.truncate();

        **streaming_frustums = (0..metadata.hierarchies)
            .map(|hierarchy| {
                let cell_size = metadata.cell_size(hierarchy);
                let far_distance = (cell_size * streaming_frustums_scale.0).min(projection.far);
                let center_on_far_plane = transform.translation + far_distance * forward;

                new_projection.far = far_distance;
                let far_corners = Frustum::far_corners(transform, &new_projection);

                let mut aabb = far_corners
                    .iter()
                    .copied()
                    .fold(near_aabb, |mut acc, corner| {
                        acc.extend(corner);
                        acc
                    });

                aabb.clamp(metadata.bounding_box.min, metadata.bounding_box.max);

                let min_cell_index = metadata.cell_index(aabb.min, cell_size);
                let max_cell_index = metadata.cell_index(aabb.max, cell_size);

                StreamingFrustum {
                    far_corners,
                    far_plane: far_normal.extend(center_on_far_plane.dot(far_normal)),
                    aabb,
                    min_cell_index,
                    max_cell_index,
                }
            })
            .collect();
    }
}

pub fn draw_ui(ui: &mut egui::Ui, world: &mut World) {
    ui.label("Load distance scale:");

    let mut streaming_frustums_scale = world.get_resource_mut::<StreamingFrustumsScale>().unwrap();
    let mut scale = streaming_frustums_scale.0;

    let slider = egui::Slider::new(
        &mut scale,
        StreamingFrustumsScale::MIN..=StreamingFrustumsScale::MAX,
    )
    .step_by(0.1);

    if ui.add(slider).changed() {
        streaming_frustums_scale.0 = scale;
    }
}
