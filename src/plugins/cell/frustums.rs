use std::ops::{Deref, DerefMut};

use bevy_ecs::prelude::*;
use bevy_ecs::system::SystemState;

use crate::plugins::camera::frustum::Frustum;
use crate::plugins::camera::projection::PerspectiveProjection;
use crate::plugins::camera::Camera;
use crate::plugins::cell::shader::FrustumsSettings;
use crate::plugins::metadata::{ActiveMetadata, UpdatedMetadataHierarchiesEvent};
use crate::transform::Transform;

#[derive(Resource)]
pub struct StreamingFrustumsScale(f32);

impl StreamingFrustumsScale {
    pub const MIN: f32 = 1.0;
    pub const MAX: f32 = 5.0;
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
    active_metadata: ActiveMetadata,
    mut updated_metadata_hierarchies_events: EventReader<UpdatedMetadataHierarchiesEvent>,
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
    let metadata = active_metadata.get();
    let updated_metadata = updated_metadata_hierarchies_events.read().count() > 0;

    for (transform, projection, frustum, mut streaming_frustums) in camera_query.iter_mut() {
        if !(frustum.is_changed() || streaming_frustums_scale.is_changed() || updated_metadata) {
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
        let mut params = SystemState::<(ActiveMetadata, ResMut<FrustumsSettings>)>::new(world);
        let (active_metadata, mut frustums_settings) = params.get_mut(world);

        let mut size_by_distance = frustums_settings.size_by_distance;
        if ui
            .checkbox(&mut size_by_distance, "Size points by distance")
            .changed()
        {
            frustums_settings.size_by_distance = size_by_distance;
        }

        let hierarchies = active_metadata.get().hierarchies;

        let mut max_hierarchy = frustums_settings.max_hierarchy;
        let slider = egui::Slider::new(&mut max_hierarchy, 0..=hierarchies.saturating_sub(1));

        if ui.add_enabled(size_by_distance, slider).changed() {
            frustums_settings.max_hierarchy = max_hierarchy;
        }
    }
}
