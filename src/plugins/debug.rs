use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_ecs::system::SystemId;
use glam::Vec3;

use crate::plugins::camera::frustum::Frustum;
use crate::plugins::camera::{Camera, Visibility};
use crate::plugins::render::line::utils::{line_box, line_strip};
use crate::plugins::render::line::Line;
use crate::plugins::render::vertex::VertexBuffer;
use crate::plugins::streaming::cell::{CellData, StreamingFrustums};
use crate::plugins::streaming::metadata::{ActiveMetadataRes, MetadataState};
use crate::plugins::wgpu::Device;

pub struct DebugPlugin;

impl Plugin for DebugPlugin {
    fn build(&self, app: &mut App) {
        {
            let toggle_frustum = app.world.register_system(toggle_frustum);
            let toggle_bounding_box = app.world.register_system(toggle_bounding_box);
            let toggle_grid = app.world.register_system(toggle_grid);
            let toggle_streaming_frustums = app.world.register_system(toggle_streaming_frustums);
            let toggle_hierarchy = app.world.register_system(toggle_hierarchy);

            app.insert_resource(OneShotSystems {
                toggle_frustum,
                toggle_bounding_box,
                toggle_grid,
                toggle_streaming_frustums,
                toggle_hierarchy,
            });
        }

        app.insert_resource(State {
            show_frustum: false,
            show_bounding_box: false,
            grid: GridSettings {
                show: false,
                hierarchies: Vec::new(),
            },
            streaming_frustums_visibility: StreamingFrustumsVisibility {
                show: false,
                hierarchies: Vec::new(),
            },
            hierarchy_visibility: HierarchyVisibility {
                show_all: true,
                hierarchies: Vec::new(),
            },
        });

        app.add_systems(
            Update,
            (add_grid_for_new_cells, set_visibility_for_new_cells)
                .run_if(in_state(MetadataState::Loaded)),
        )
        .add_systems(
            OnEnter(MetadataState::Loaded),
            (
                update_hierarchies,
                (|| true)
                    .pipe(toggle_bounding_box)
                    .run_if(|state: Res<State>| state.show_bounding_box),
            ),
        )
        .add_systems(
            OnExit(MetadataState::Loaded),
            (|| false)
                .pipe(toggle_bounding_box)
                .run_if(|state: Res<State>| state.show_bounding_box),
        );
    }
}

#[derive(Resource)]
struct OneShotSystems {
    toggle_frustum: SystemId<bool>,
    toggle_bounding_box: SystemId<bool>,
    toggle_grid: SystemId<(bool, u32)>,
    toggle_streaming_frustums: SystemId<(bool, u32)>,
    toggle_hierarchy: SystemId<(bool, u32)>,
}

#[derive(Resource)]
struct State {
    show_frustum: bool,
    show_bounding_box: bool,
    grid: GridSettings,
    streaming_frustums_visibility: StreamingFrustumsVisibility,
    hierarchy_visibility: HierarchyVisibility,
}

struct GridSettings {
    show: bool,
    hierarchies: Vec<bool>,
}

struct HierarchyVisibility {
    show_all: bool,
    hierarchies: Vec<bool>,
}

struct StreamingFrustumsVisibility {
    show: bool,
    hierarchies: Vec<bool>,
}

#[derive(Component)]
struct FrustumLine;

fn toggle_frustum(
    show: In<bool>,
    mut commands: Commands,
    camera_query: Query<&Frustum, With<Camera>>,
    device: Res<Device>,
    frustum_query: Query<Entity, With<FrustumLine>>,
) {
    if *show {
        for frustum in camera_query.iter() {
            let mut connections = vec![
                Line {
                    start: frustum.near.top_left,
                    end: frustum.far.top_left,
                    color: [0, 255, 0, 255],
                },
                Line {
                    start: frustum.near.top_right,
                    end: frustum.far.top_right,
                    color: [0, 255, 0, 255],
                },
                Line {
                    start: frustum.near.bottom_left,
                    end: frustum.far.bottom_left,
                    color: [0, 255, 0, 255],
                },
                Line {
                    start: frustum.near.bottom_right,
                    end: frustum.far.bottom_right,
                    color: [0, 255, 0, 255],
                },
            ];

            connections.append(&mut line_strip(
                [255, 0, 0, 255],
                &[
                    frustum.near.top_left,
                    frustum.near.top_right,
                    frustum.near.bottom_right,
                    frustum.near.bottom_left,
                    frustum.near.top_left,
                ],
            ));

            connections.append(&mut line_strip(
                [0, 0, 255, 255],
                &[
                    frustum.far.top_left,
                    frustum.far.top_right,
                    frustum.far.bottom_right,
                    frustum.far.bottom_left,
                    frustum.far.top_left,
                ],
            ));

            connections.extend(
                [
                    Line {
                        start: Vec3::ZERO,
                        end: frustum.planes.top.truncate() * frustum.planes.top.w,
                        color: [255, 255, 0, 255],
                    },
                    Line {
                        start: Vec3::ZERO,
                        end: frustum.planes.right.truncate() * frustum.planes.right.w,
                        color: [255, 255, 0, 255],
                    },
                    Line {
                        start: Vec3::ZERO,
                        end: frustum.planes.bottom.truncate() * frustum.planes.bottom.w,
                        color: [255, 255, 0, 255],
                    },
                    Line {
                        start: Vec3::ZERO,
                        end: frustum.planes.left.truncate() * frustum.planes.left.w,
                        color: [255, 255, 0, 255],
                    },
                ]
                .iter(),
            );

            commands.spawn((FrustumLine, VertexBuffer::new(&device, &connections)));
        }
    } else {
        for entity in frustum_query.iter() {
            commands.entity(entity).despawn();
        }
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
        let aabb = active_metadata.metadata.bounding_box;
        let lines = line_box(
            [255, 0, 0, 255],
            (aabb.min + aabb.max) / 2.0,
            (aabb.max - aabb.min) / 2.0,
        );
        commands.spawn((BoundingBoxLine, VertexBuffer::new(&device, &lines)));
    } else {
        for entity in bounding_box_query.iter() {
            commands.entity(entity).despawn();
        }
    }
}

fn update_hierarchies(mut state: ResMut<State>, active_metadata: ActiveMetadataRes) {
    let hierarchies = active_metadata.metadata.hierarchies as usize;
    state.grid.hierarchies = vec![true; hierarchies];
    state.streaming_frustums_visibility.hierarchies = vec![true; hierarchies];
    state.hierarchy_visibility.hierarchies = vec![true; hierarchies];
}

fn add_grid_for_new_cells(
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
            cell_data.pos,
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
                    [255, if hierarchy % 2 == 0 { 180 } else { 90 }, 0, 255],
                    cell_data.pos,
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

#[derive(Component)]
struct StreamingFrustumLine(u32);

fn toggle_streaming_frustums(
    In((show, hierarchy)): In<(bool, u32)>,
    mut commands: Commands,
    device: Res<Device>,
    add_query: Query<&StreamingFrustums, With<Camera>>,
    remove_query: Query<(Entity, &StreamingFrustumLine)>,
) {
    if show {
        for streaming_frustums in add_query.iter() {
            let streaming_frustum = &streaming_frustums[hierarchy as usize];
            let far_corners = &streaming_frustum.far_corners;

            let lines = line_strip(
                [255, 0, if hierarchy % 2 == 0 { 180 } else { 90 }, 255],
                &[
                    far_corners.top_left,
                    far_corners.top_right,
                    far_corners.bottom_right,
                    far_corners.bottom_left,
                    far_corners.top_left,
                ],
            );

            let buffer = VertexBuffer::new(&device, &lines);
            commands.spawn((StreamingFrustumLine(hierarchy), buffer));
        }
    } else {
        for (entity, line) in remove_query.iter() {
            if line.0 == hierarchy {
                commands.entity(entity).despawn();
            }
        }
    }
}

fn set_visibility_for_new_cells(
    state: Res<State>,
    mut cell_query: Query<(&CellData, &mut Visibility), Added<CellData>>,
) {
    if state.hierarchy_visibility.show_all {
        return;
    }

    for (cell_data, mut visibility) in cell_query.iter_mut() {
        visibility.visible = state
            .hierarchy_visibility
            .hierarchies
            .get(cell_data.id.hierarchy as usize)
            .copied()
            .unwrap_or(true);
    }
}

fn toggle_hierarchy(
    In((show, hierarchy)): In<(bool, u32)>,
    mut cell_query: Query<(&CellData, &mut Visibility)>,
) {
    for (cell_data, mut visibility) in cell_query.iter_mut() {
        if cell_data.id.hierarchy == hierarchy {
            visibility.visible = show;
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

        draw_ui_grid(ui, world, &mut state);
        draw_ui_streaming_frustums(ui, world, &mut state);
        draw_ui_visible_hierarchies(ui, world, &mut state);
    });
}

fn draw_ui_grid(ui: &mut egui::Ui, world: &mut World, state: &mut State) {
    let id = ui.make_persistent_id("collapsing_grid_header");
    egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false)
        .show_header(ui, |ui| {
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
        })
        .body(|ui| {
            for (hierarchy, show) in state.grid.hierarchies.iter_mut().enumerate() {
                if ui.checkbox(show, hierarchy.to_string()).changed() {
                    let toggle_grid = world.get_resource::<OneShotSystems>().unwrap().toggle_grid;
                    world
                        .run_system_with_input(toggle_grid, (*show, hierarchy as u32))
                        .unwrap();
                }
            }
        });
}

fn draw_ui_streaming_frustums(ui: &mut egui::Ui, world: &mut World, state: &mut State) {
    let id = ui.make_persistent_id("streaming_frustums_header");
    egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false)
        .show_header(ui, |ui| {
            if ui
                .checkbox(
                    &mut state.streaming_frustums_visibility.show,
                    "Streaming Frustums",
                )
                .changed()
            {
                let toggle_streaming_frustums = world
                    .get_resource::<OneShotSystems>()
                    .unwrap()
                    .toggle_streaming_frustums;

                for (hierarchy, show) in state
                    .streaming_frustums_visibility
                    .hierarchies
                    .iter()
                    .enumerate()
                {
                    if *show {
                        world
                            .run_system_with_input(
                                toggle_streaming_frustums,
                                (state.streaming_frustums_visibility.show, hierarchy as u32),
                            )
                            .unwrap();
                    }
                }
            }
        })
        .body(|ui| {
            for (hierarchy, show) in state
                .streaming_frustums_visibility
                .hierarchies
                .iter_mut()
                .enumerate()
            {
                if ui.checkbox(show, hierarchy.to_string()).changed() {
                    let toggle_streaming_frustums = world
                        .get_resource::<OneShotSystems>()
                        .unwrap()
                        .toggle_streaming_frustums;

                    world
                        .run_system_with_input(toggle_streaming_frustums, (*show, hierarchy as u32))
                        .unwrap();
                }
            }
        });
}

fn draw_ui_visible_hierarchies(ui: &mut egui::Ui, world: &mut World, state: &mut State) {
    let id = ui.make_persistent_id("collapsing_visible_hierarchies_header");
    egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false)
        .show_header(ui, |ui| {
            if ui
                .checkbox(
                    &mut state.hierarchy_visibility.show_all,
                    "Show all hierarchies",
                )
                .changed()
            {
                let toggle_hierarchy = world
                    .get_resource::<OneShotSystems>()
                    .unwrap()
                    .toggle_hierarchy;

                if state.hierarchy_visibility.show_all {
                    for hierarchy in 0..state.hierarchy_visibility.hierarchies.len() {
                        world
                            .run_system_with_input(toggle_hierarchy, (true, hierarchy as u32))
                            .unwrap();
                    }
                } else {
                    for (hierarchy, show) in
                        state.hierarchy_visibility.hierarchies.iter().enumerate()
                    {
                        world
                            .run_system_with_input(toggle_hierarchy, (*show, hierarchy as u32))
                            .unwrap();
                    }
                }
            }
        })
        .body(|ui| {
            ui.label("Visible hierarchies:");

            ui.set_enabled(!state.hierarchy_visibility.show_all);

            for (hierarchy, show) in state
                .hierarchy_visibility
                .hierarchies
                .iter_mut()
                .enumerate()
            {
                if ui.checkbox(show, hierarchy.to_string()).changed() {
                    let toggle_hierarchy = world
                        .get_resource::<OneShotSystems>()
                        .unwrap()
                        .toggle_hierarchy;

                    world
                        .run_system_with_input(toggle_hierarchy, (*show, hierarchy as u32))
                        .unwrap();
                }
            }
        });
}
