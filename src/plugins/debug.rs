use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_ecs::system::SystemId;
use glam::Vec3;

use crate::plugins::camera::projection::PerspectiveProjection;
use crate::plugins::render::line::utils::{line_box, line_strip};
use crate::plugins::render::line::Line;
use crate::plugins::render::vertex::VertexBuffer;
use crate::plugins::streaming::{ActiveMetadataRes, CellData};
use crate::plugins::wgpu::Device;
use crate::transform::Transform;

pub struct DebugPlugin;

impl Plugin for DebugPlugin {
    fn build(&self, app: &mut App) {
        let toggle_frustum = app.world.register_system(toggle_frustum);
        let toggle_bounding_box = app.world.register_system(toggle_bounding_box);
        let toggle_grid = app.world.register_system(toggle_grid);

        app.insert_resource(OneShotSystems {
            toggle_frustum,
            toggle_bounding_box,
            toggle_grid,
        });

        app.insert_resource(State {
            show_frustum: false,
            show_bounding_box: false,
            grid: GridSettings {
                show: false,
                hierarchies: Vec::new(),
            },
        });

        app.add_systems(
            Update,
            (
                watch_metadata_change,
                (update_grid_hierarchies, add_grid).chain(),
            ),
        );
    }
}

#[derive(Resource)]
struct OneShotSystems {
    toggle_frustum: SystemId<bool>,
    toggle_bounding_box: SystemId<bool>,
    toggle_grid: SystemId<(bool, u32)>,
}

#[derive(Resource)]
struct State {
    show_frustum: bool,
    show_bounding_box: bool,
    grid: GridSettings,
}

struct GridSettings {
    show: bool,
    hierarchies: Vec<bool>,
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

fn update_grid_hierarchies(mut state: ResMut<State>, active_metadata: ActiveMetadataRes) {
    if !active_metadata.is_changed() {
        return;
    }

    if let Some(metadata) = active_metadata.metadata() {
        state.grid.hierarchies = vec![true; metadata.hierarchies as usize];
    }
}

fn add_grid(
    mut commands: Commands,
    device: Res<Device>,
    cell_query: Query<(Entity, &CellData), Added<CellData>>,
    state: Res<State>,
) {
    if !state.grid.show {
        return;
    }

    for (entity, cell_data) in cell_query.iter() {
        if !state
            .grid
            .hierarchies
            .get(cell_data.id.hierarchy as usize)
            .copied()
            .unwrap_or(false)
        {
            continue;
        }

        let lines = line_box(
            [
                255,
                if cell_data.id.hierarchy % 2 == 0 {
                    180
                } else {
                    90
                },
                0,
                255,
            ],
            Vec3::new(cell_data.pos.x, cell_data.pos.z, -cell_data.pos.y),
            Vec3::splat(cell_data.size / 2.0),
        );

        let buffer = VertexBuffer::new(&device, &lines);
        commands.entity(entity).insert(buffer);
    }
}

fn toggle_grid(
    In((show, hierarchy)): In<(bool, u32)>,
    mut commands: Commands,
    device: Res<Device>,
    add_query: Query<(Entity, &CellData), Without<VertexBuffer<Line>>>,
    remove_query: Query<(Entity, &CellData), With<VertexBuffer<Line>>>,
) {
    if show {
        for (entity, cell_data) in add_query.iter() {
            if cell_data.id.hierarchy == hierarchy {
                let lines = line_box(
                    [
                        255,
                        if cell_data.id.hierarchy % 2 == 0 {
                            180
                        } else {
                            90
                        },
                        0,
                        255,
                    ],
                    Vec3::new(cell_data.pos.x, cell_data.pos.z, -cell_data.pos.y),
                    Vec3::splat(cell_data.size / 2.0),
                );

                let buffer = VertexBuffer::new(&device, &lines);
                commands.entity(entity).insert(buffer);
            }
        }
    } else {
        for (entity, cell_data) in remove_query.iter() {
            if cell_data.id.hierarchy == hierarchy {
                commands.entity(entity).remove::<VertexBuffer<Line>>();
            }
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

        if ui.checkbox(&mut state.grid.show, "Grid").changed() {
            let toggle_grid = world.get_resource::<OneShotSystems>().unwrap().toggle_grid;

            for (hierarchy, show) in state.grid.hierarchies.iter().enumerate() {
                if *show {
                    world
                        .run_system_with_input(toggle_grid, (state.grid.show, hierarchy as u32))
                        .unwrap();
                }
            }
        }

        ui.collapsing("Grid Hierarchies", |ui| {
            for (hierarchy, show) in state.grid.hierarchies.iter_mut().enumerate() {
                if ui.checkbox(show, hierarchy.to_string()).changed() {
                    let toggle_grid = world.get_resource::<OneShotSystems>().unwrap().toggle_grid;
                    world
                        .run_system_with_input(toggle_grid, (*show, hierarchy as u32))
                        .unwrap();
                }
            }
        });
    });
}
