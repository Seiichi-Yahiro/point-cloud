use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_ecs::system::SystemId;

use crate::plugins::camera::projection::PerspectiveProjection;
use crate::plugins::render::line::Line;
use crate::plugins::render::line::utils::{line_box, line_strip};
use crate::plugins::render::vertex::VertexBuffer;
use crate::plugins::streaming::ActiveMetadataRes;
use crate::plugins::wgpu::Device;
use crate::transform::Transform;

pub struct DebugPlugin;

impl Plugin for DebugPlugin {
    fn build(&self, app: &mut App) {
        let toggle_frustum = app.world.register_system(toggle_frustum);
        let toggle_bounding_box = app.world.register_system(toggle_bounding_box);

        app.insert_resource(OneShotSystems {
            toggle_frustum,
            toggle_bounding_box,
        });

        app.insert_resource(State {
            show_frustum: false,
            show_bounding_box: false,
        });

        app.add_systems(Update, watch_metadata_change);
    }
}

#[derive(Resource)]
struct OneShotSystems {
    toggle_frustum: SystemId<bool>,
    toggle_bounding_box: SystemId<bool>,
}

#[derive(Resource)]
struct State {
    show_frustum: bool,
    show_bounding_box: bool,
}

#[derive(Component)]
struct FrustumLine;

fn toggle_frustum(
    show: In<bool>,
    mut commands: Commands,
    camera_query: Query<(&Transform, &PerspectiveProjection)>,
    device: Res<Device>,
    frustum_query: Query<Entity, With<FrustumLine>>,
) {
    if *show {
        for (transform, projection) in camera_query.iter() {
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

            let near_top_left = center_on_near_plane + near_up - near_right;
            let near_top_right = center_on_near_plane + near_up + near_right;
            let near_bottom_left = center_on_near_plane - near_up - near_right;
            let near_bottom_right = center_on_near_plane - near_up + near_right;

            let far_up = cam_up * half_height_far;
            let far_right = cam_right * half_width_far;

            let far_top_left = center_on_far_plane + far_up - far_right;
            let far_top_right = center_on_far_plane + far_up + far_right;
            let far_bottom_left = center_on_far_plane - far_up - far_right;
            let far_bottom_right = center_on_far_plane - far_up + far_right;

            let mut connections = vec![
                Line {
                    start: near_top_left,
                    end: far_top_left,
                    color: [0, 255, 0, 255],
                },
                Line {
                    start: near_top_right,
                    end: far_top_right,
                    color: [0, 255, 0, 255],
                },
                Line {
                    start: near_bottom_left,
                    end: far_bottom_left,
                    color: [0, 255, 0, 255],
                },
                Line {
                    start: near_bottom_right,
                    end: far_bottom_right,
                    color: [0, 255, 0, 255],
                },
            ];

            connections.append(&mut line_strip(
                [255, 0, 0, 255],
                &[
                    near_top_left,
                    near_top_right,
                    near_bottom_right,
                    near_bottom_left,
                    near_top_left,
                ],
            ));

            connections.append(&mut line_strip(
                [0, 0, 255, 255],
                &[
                    far_top_left,
                    far_top_right,
                    far_bottom_right,
                    far_bottom_left,
                    far_top_left,
                ],
            ));

            commands.spawn((FrustumLine, VertexBuffer::new(&device, &connections)));
        }
    } else {
        for entity in frustum_query.iter() {
            commands.entity(entity).despawn();
        }
    }
}

fn watch_metadata_change(
    mut commands: Commands,
    one_shot_systems: Res<OneShotSystems>,
    active_metadata: ActiveMetadataRes,
    state: Res<State>,
) {
    if active_metadata.is_changed() && state.show_bounding_box {
        commands.run_system_with_input(one_shot_systems.toggle_bounding_box, false);
        commands.run_system_with_input(one_shot_systems.toggle_bounding_box, true);
    }
}

#[derive(Component)]
struct BoundingBoxLine;

fn toggle_bounding_box(
    show: In<bool>,
    mut commands: Commands,
    active_metadata: ActiveMetadataRes,
    device: Res<Device>,
    bounding_box_query: Query<Entity, With<BoundingBoxLine>>,
) {
    if *show {
        if let Some(metadata) = active_metadata.metadata() {
            let aabb = metadata.bounding_box.flip_yz();
            let lines = line_box(
                [255, 0, 0, 255],
                (aabb.min + aabb.max) / 2.0,
                (aabb.max - aabb.min) / 2.0,
            );
            commands.spawn((BoundingBoxLine, VertexBuffer::new(&device, &lines)));
        }
    } else {
        for entity in bounding_box_query.iter() {
            commands.entity(entity).despawn();
        }
    }
}

pub fn draw_ui(ui: &mut egui::Ui, world: &mut World) {
    world.resource_scope(|world, mut state: Mut<State>| {
        if ui.checkbox(&mut state.show_frustum, "Frustum").changed() {
            let toggle_frustum = world
                .get_resource::<OneShotSystems>()
                .unwrap()
                .toggle_frustum;

            world
                .run_system_with_input(toggle_frustum, state.show_frustum)
                .unwrap();
        }

        if ui
            .checkbox(&mut state.show_bounding_box, "Bounding Box")
            .changed()
        {
            let toggle_bounding_box = world
                .get_resource::<OneShotSystems>()
                .unwrap()
                .toggle_bounding_box;

            world
                .run_system_with_input(toggle_bounding_box, state.show_bounding_box)
                .unwrap();
        }
    });
}
