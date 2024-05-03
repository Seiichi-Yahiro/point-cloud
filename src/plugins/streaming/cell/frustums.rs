use std::ops::{Deref, DerefMut};

use bevy_ecs::prelude::*;

use crate::plugins::camera::frustum::Frustum;
use crate::plugins::camera::projection::PerspectiveProjection;
use crate::plugins::camera::Camera;
use crate::plugins::streaming::cell::shader::FrustumsSettings;
use crate::plugins::streaming::metadata::{ActiveMetadata, ActiveMetadataRes};
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

#[derive(Debug, Component)]
pub struct StreamingFrustums(Vec<Frustum>);

impl Deref for StreamingFrustums {
    type Target = Vec<Frustum>;

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

        let forward = transform.forward();
        let far_normal = frustum.planes.far.truncate();

        **streaming_frustums = (0..metadata.hierarchies)
            .map(|hierarchy| {
                let cell_size = metadata.config.cell_size(hierarchy);

                let far_distance =
                    projection.near + (cell_size * streaming_frustums_scale.0).min(projection.far);
                let center_on_far_plane = transform.translation + far_distance * forward;

                new_projection.far = far_distance;
                let far_corners = Frustum::far_corners(transform, &new_projection);

                let mut new_frustum_planes = frustum.planes.clone();
                new_frustum_planes.far = far_normal.extend(center_on_far_plane.dot(far_normal));

                Frustum {
                    near: frustum.near.clone(),
                    far: far_corners,
                    planes: new_frustum_planes,
                }
            })
            .collect();
    }
}

pub fn draw_ui(ui: &mut egui::Ui, world: &mut World) {
    {
        ui.label("Load distance scale:");

        let mut streaming_frustums_scale =
            world.get_resource_mut::<StreamingFrustumsScale>().unwrap();
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

    {
        #[cfg(not(target_arch = "wasm32"))]
        let hierarchies = world
            .get_resource::<ActiveMetadata>()
            .unwrap()
            .metadata
            .hierarchies;

        #[cfg(target_arch = "wasm32")]
        let hierarchies = world
            .get_non_send_resource::<ActiveMetadata>()
            .unwrap()
            .metadata
            .hierarchies;

        let mut frustums_settings = world.get_resource_mut::<FrustumsSettings>().unwrap();

        let mut size_by_distance = frustums_settings.size_by_distance;
        if ui
            .checkbox(&mut size_by_distance, "Size points by distance")
            .changed()
        {
            frustums_settings.size_by_distance = size_by_distance;
        }

        let mut max_hierarchy = frustums_settings.max_hierarchy;
        let slider = egui::Slider::new(&mut max_hierarchy, 0..=hierarchies - 1);

        if ui.add_enabled(size_by_distance, slider).changed() {
            frustums_settings.max_hierarchy = max_hierarchy;
        }
    }
}
