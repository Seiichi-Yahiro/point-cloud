use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::ops::{Deref, DerefMut};

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use egui::ahash::{HashMapExt, HashSetExt};
use flume::{Receiver, Sender, TryRecvError};
use glam::{IVec3, Vec3};
use itertools::Itertools;
use rustc_hash::{FxHashMap, FxHashSet};

use point_converter::cell::CellId;

use crate::plugins::camera::frustum::{Aabb, Frustum};
use crate::plugins::camera::projection::PerspectiveProjection;
use crate::plugins::camera::{Camera, FrustumCull, UpdateFrustum, Visibility};
use crate::plugins::render::vertex::VertexBuffer;
use crate::plugins::streaming::cell::loader::{spawn_cell_loader, LoadCellMsg, LoadedCellMsg};
use crate::plugins::streaming::cell::shader::{CellBindGroupData, CellBindGroupLayout};
use crate::plugins::streaming::metadata::{ActiveMetadataRes, MetadataState};
use crate::plugins::wgpu::Device;
use crate::transform::Transform;

mod loader;
pub mod shader;

pub struct CellPlugin;

impl Plugin for CellPlugin {
    fn build(&self, app: &mut App) {
        let (load_sender, load_receiver) = flume::unbounded::<LoadCellMsg>();
        let (loaded_sender, loaded_receiver) = flume::unbounded::<LoadedCellMsg>();

        spawn_cell_loader(load_receiver, loaded_sender);

        let channels = Channels {
            load_sender,
            loaded_receiver,
        };

        #[cfg(not(target_arch = "wasm32"))]
        app.insert_resource(channels);

        #[cfg(target_arch = "wasm32")]
        app.insert_non_send_resource(channels);

        shader::setup(&mut app.world);

        app.insert_state(StreamState::Enabled)
            .insert_resource(Cells::default())
            .add_systems(PostStartup, add_hierarchy_spheres)
            .add_systems(
                Update,
                (
                    (receive_cell, update_hierarchy_spheres.after(UpdateFrustum)),
                    update_cells,
                    enqueue_cells_to_load,
                )
                    .chain()
                    .run_if(in_state(MetadataState::Loaded))
                    .run_if(in_state(StreamState::Enabled)),
            )
            .add_systems(OnExit(MetadataState::Loaded), cleanup_cells)
            .add_systems(
                PostUpdate,
                shader::update_visible_cells_buffer.after(FrustumCull),
            );
    }
}

#[cfg_attr(not(target_arch = "wasm32"), derive(Resource))]
struct Channels {
    load_sender: Sender<LoadCellMsg>,
    loaded_receiver: Receiver<LoadedCellMsg>,
}

#[cfg(not(target_arch = "wasm32"))]
type ChannelsRes<'w> = Res<'w, Channels>;

#[cfg(target_arch = "wasm32")]
type ChannelsRes<'w> = NonSend<'w, Channels>;

impl Drop for Channels {
    fn drop(&mut self) {
        self.load_sender.send(LoadCellMsg::Stop).unwrap();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, States)]
enum StreamState {
    Enabled,
    Paused,
}

#[derive(Component)]
pub struct CellData {
    pub id: CellId,
    pub pos: Vec3,
    pub size: f32,
}

#[derive(Debug)]
enum LoadedCellStatus {
    Missing,
    Loaded(Entity),
}

#[derive(Resource)]
struct Cells {
    loaded: FxHashMap<CellId, LoadedCellStatus>,
    should_load: BTreeSet<CellToLoad>,
    loading: FxHashSet<CellId>,
}

impl Cells {
    const MAX_LOADING_SIZE: usize = 10;
}

impl Default for Cells {
    fn default() -> Self {
        Self {
            loaded: FxHashMap::default(),
            should_load: BTreeSet::new(),
            loading: FxHashSet::with_capacity(Self::MAX_LOADING_SIZE),
        }
    }
}

struct CellToLoad {
    id: CellId,
    priority: u32,
}

impl PartialEq for CellToLoad {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for CellToLoad {}

impl PartialOrd for CellToLoad {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for CellToLoad {
    fn cmp(&self, other: &Self) -> Ordering {
        self.id
            .hierarchy
            .cmp(&other.id.hierarchy)
            .then(self.priority.cmp(&other.priority))
    }
}

#[derive(Debug)]
pub struct Sphere {
    pub pos: Vec3,
    pub radius: f32,
}

#[derive(Debug, Component)]
pub struct HierarchySpheres(Vec<Sphere>);

impl Deref for HierarchySpheres {
    type Target = Vec<Sphere>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for HierarchySpheres {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
fn cleanup_cells(
    mut commands: Commands,
    cell_query: Query<Entity, With<CellData>>,
    mut cells: ResMut<Cells>,
) {
    cells.should_load.clear();
    cells.loading.clear();
    cells.loaded.clear();

    for entity in cell_query.iter() {
        commands.entity(entity).despawn();
    }
}

fn receive_cell(
    mut commands: Commands,
    channels: ChannelsRes,
    device: Res<Device>,
    cell_bind_group_layout: Res<CellBindGroupLayout>,
    mut cells: ResMut<Cells>,
) {
    for _ in 0..Cells::MAX_LOADING_SIZE {
        match channels.loaded_receiver.try_recv() {
            Ok(LoadedCellMsg { id, cell }) => {
                if !cells.loading.remove(&id) {
                    log::debug!("Cell {:?} was loaded but no longer needed", id);
                    continue;
                }

                match cell {
                    Ok(Some(cell)) => {
                        log::debug!("Loaded cell: {:?}", id);

                        let points = cell
                            .points()
                            .iter()
                            .map(|it| crate::plugins::render::point::Point {
                                position: it.pos,
                                color: it.color,
                            })
                            .collect_vec();

                        let vertex_buffer = VertexBuffer::new(&device, &points);
                        let cell_bind_group_data =
                            CellBindGroupData::new(&device, &cell_bind_group_layout, id);

                        let header = cell.header();
                        let cell_data = CellData {
                            id,
                            pos: header.pos,
                            size: header.size,
                        };

                        let aabb = Aabb::new(
                            cell_data.pos - cell_data.size / 2.0,
                            cell_data.pos + cell_data.size / 2.0,
                        );

                        let entity = commands
                            .spawn((
                                cell_data,
                                vertex_buffer,
                                cell_bind_group_data,
                                aabb,
                                Visibility::new(true),
                            ))
                            .id();

                        cells.loaded.insert(id, LoadedCellStatus::Loaded(entity));
                    }
                    Ok(None) => {
                        log::debug!("Cell is missing: {:?}", id);
                        cells.loaded.insert(id, LoadedCellStatus::Missing);
                    }
                    Err(err) => {
                        // TODO do something with the failed cell
                        log::error!("Failed to load cell {:?}: {:?}", id, err);
                    }
                }
            }
            Err(TryRecvError::Disconnected) => {
                panic!("Failed to stream files as the sender was dropped");
            }
            Err(TryRecvError::Empty) => {
                return;
            }
        }
    }
}

fn add_hierarchy_spheres(mut commands: Commands, camera_query: Query<Entity, With<Camera>>) {
    for entity in camera_query.iter() {
        commands.entity(entity).insert(HierarchySpheres(Vec::new()));
    }
}

fn update_hierarchy_spheres(
    active_metadata: ActiveMetadataRes,
    mut camera_query: Query<
        (&Transform, &PerspectiveProjection, &mut HierarchySpheres),
        (With<Camera>, Changed<Frustum>),
    >,
) {
    let metadata = &active_metadata.metadata;

    for (transform, projection, mut hierarchy_spheres) in camera_query.iter_mut() {
        let forward = transform.forward();

        **hierarchy_spheres = (0..metadata.hierarchies)
            .map(|hierarchy| {
                let radius = metadata.cell_size(hierarchy);
                let pos = transform.translation + forward * (projection.near + radius / 2.0);
                Sphere { radius, pos }
            })
            .collect();
    }
}

fn update_cells(
    mut commands: Commands,
    camera_query: Query<(&HierarchySpheres, &Transform), (With<Camera>, Changed<HierarchySpheres>)>,
    active_metadata: ActiveMetadataRes,
    mut cells: ResMut<Cells>,
) {
    let metadata = &active_metadata.metadata;

    for (hierarchy_spheres, transform) in camera_query.iter() {
        let mut new_loaded = FxHashMap::with_capacity(cells.loaded.capacity());
        let mut new_should_load = BTreeSet::new();

        for (hierarchy, sphere) in hierarchy_spheres.iter().enumerate() {
            let hierarchy = hierarchy as u32;

            let cell_size = metadata.cell_size(hierarchy);
            let min_cell_index = metadata.cell_index(
                (sphere.pos - sphere.radius).max(metadata.bounding_box.min),
                cell_size,
            );
            let max_cell_index = metadata.cell_index(
                (sphere.pos + sphere.radius).min(metadata.bounding_box.max),
                cell_size,
            );

            let ids = (min_cell_index.x..=max_cell_index.x)
                .cartesian_product(min_cell_index.y..=max_cell_index.y)
                .cartesian_product(min_cell_index.z..=max_cell_index.z)
                .map(|((x, y), z)| IVec3::new(x, y, z))
                .map(move |index| CellId { index, hierarchy })
                .map(|cell_id| {
                    let cell_pos = metadata.cell_pos(cell_id.index, cell_size);
                    let distance_to_camera =
                        (cell_pos - transform.translation).length_squared() as u32;
                    CellToLoad {
                        id: cell_id,
                        priority: distance_to_camera,
                    }
                });

            // copy or insert cells that need to be loaded
            for cell_to_load in ids {
                if let Some(status) = cells.loaded.remove(&cell_to_load.id) {
                    new_loaded.insert(cell_to_load.id, status);
                } else if cells.should_load.remove(&cell_to_load)
                    || !cells.loading.contains(&cell_to_load.id)
                {
                    new_should_load.insert(cell_to_load);
                }
            }
        }

        for status in cells.loaded.values() {
            match status {
                LoadedCellStatus::Missing => {}
                LoadedCellStatus::Loaded(entity) => {
                    commands.entity(*entity).despawn();
                }
            }
        }

        cells.loaded = new_loaded;
        cells.should_load = new_should_load;
    }
}

fn enqueue_cells_to_load(
    mut cells: ResMut<Cells>,
    active_metadata: ActiveMetadataRes,
    channels: ChannelsRes,
) {
    let free_load_slots = Cells::MAX_LOADING_SIZE - cells.loading.len();

    for _ in 0..free_load_slots {
        if let Some(cell_to_load) = cells.should_load.pop_first() {
            cells.loading.insert(cell_to_load.id);

            channels
                .load_sender
                .send(LoadCellMsg::Cell {
                    id: cell_to_load.id,
                    sub_grid_dimension: active_metadata.metadata.sub_grid_dimension,
                    source: active_metadata.source.clone(),
                })
                .unwrap();
        } else {
            break;
        }
    }
}

pub fn draw_ui(ui: &mut egui::Ui, world: &mut World) {
    {
        let mut is_streaming_paused =
            match *world.get_resource::<State<StreamState>>().unwrap().get() {
                StreamState::Enabled => false,
                StreamState::Paused => true,
            };

        if ui
            .checkbox(&mut is_streaming_paused, "Pause streaming")
            .changed()
        {
            let mut next_stream_state = world.get_resource_mut::<NextState<StreamState>>().unwrap();

            let next_state = if is_streaming_paused {
                StreamState::Paused
            } else {
                StreamState::Enabled
            };

            next_stream_state.set(next_state);
        }
    }

    {
        let cells = world.get_resource::<Cells>().unwrap();

        ui.label(format!("Loaded cells: {}", cells.loaded.len()));
        ui.label(format!("Cells to load: {}", cells.should_load.len()));
    }
}
