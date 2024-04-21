use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::hash::BuildHasherDefault;

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bytesize::ByteSize;
use caches::{Cache, LRUCache};
use egui::ahash::{HashMapExt, HashSetExt};
use flume::{Receiver, Sender, TryRecvError};
use itertools::Itertools;
use rustc_hash::{FxHashMap, FxHashSet, FxHasher};
use thousands::Separable;

use point_converter::cell::{Cell, CellId};

use crate::plugins::camera::frustum::{Aabb, Frustum};
use crate::plugins::camera::projection::PerspectiveProjection;
use crate::plugins::camera::{Camera, UpdateFrustum, Visibility};
use crate::plugins::render::point::Point;
use crate::plugins::render::vertex::VertexBuffer;
use crate::plugins::streaming::cell::loader::{spawn_cell_loader, LoadCellMsg, LoadedCellMsg};
use crate::plugins::streaming::cell::shader::{CellBindGroupData, CellBindGroupLayout};
use crate::plugins::streaming::metadata::{ActiveMetadataRes, MetadataState};
use crate::plugins::wgpu::Device;
use crate::transform::Transform;

pub mod frustums;
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
            .insert_resource(frustums::StreamingFrustumsScale::default())
            .insert_resource(LoadedCells::default())
            .insert_resource(MissingCells::default())
            .insert_resource(LoadingCells::default())
            .insert_resource(Stats::default())
            .add_systems(PostStartup, frustums::add_streaming_frustums)
            .add_systems(
                Update,
                (
                    (
                        receive_cell,
                        frustums::update_streaming_frustums.after(UpdateFrustum),
                    ),
                    update_cells,
                    (
                        enqueue_cells_to_load,
                        count_points.run_if(resource_changed::<LoadedCells>),
                    ),
                )
                    .chain()
                    .run_if(in_state(MetadataState::Loaded))
                    .run_if(in_state(StreamState::Enabled)),
            )
            .add_systems(OnEnter(MetadataState::Loaded), set_view_distance)
            .add_systems(OnEnter(MetadataState::Loading), cleanup_cells)
            .add_systems(
                PostUpdate,
                shader::update_loaded_cells_buffer
                    .run_if(in_state(MetadataState::Loaded))
                    .run_if(in_state(StreamState::Enabled))
                    .run_if(resource_changed::<LoadedCells>),
            );
    }
}

fn set_view_distance(
    active_metadata: ActiveMetadataRes,
    mut camera_query: Query<&mut PerspectiveProjection, With<Camera>>,
) {
    for mut projection in camera_query.iter_mut() {
        projection.far = active_metadata.metadata.max_cell_size;
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

#[derive(Default, Resource)]
struct LoadedCells(FxHashMap<CellId, Entity>);

#[derive(Resource)]
struct MissingCells(LRUCache<CellId, (), BuildHasherDefault<FxHasher>>);

impl Default for MissingCells {
    fn default() -> Self {
        Self(LRUCache::with_hasher(10000, BuildHasherDefault::default()).unwrap())
    }
}

#[derive(Resource)]
struct LoadingCells {
    should_load: BTreeSet<CellToLoad>,
    loading: FxHashSet<CellId>,
}

impl LoadingCells {
    const MAX_LOADING_SIZE: usize = 10;
}

impl Default for LoadingCells {
    fn default() -> Self {
        Self {
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

fn cleanup_cells(
    mut commands: Commands,
    cell_query: Query<Entity, With<CellHeader>>,
    mut loaded_cells: ResMut<LoadedCells>,
    mut missing_cells: ResMut<MissingCells>,
    mut loading_cells: ResMut<LoadingCells>,
) {
    loading_cells.should_load.clear();
    loading_cells.loading.clear();
    loaded_cells.0.clear();
    missing_cells.0.purge();

    for entity in cell_query.iter() {
        commands.entity(entity).despawn();
    }
}

#[derive(Component)]
pub struct CellHeader(pub point_converter::cell::Header);

#[derive(Bundle)]
struct CellBundle {
    header: CellHeader,
    vertex_buffer: VertexBuffer<Point>,
    bind_group_data: CellBindGroupData,
    visibility: Visibility,
}

impl CellBundle {
    fn new(
        cell: Cell,
        device: &wgpu::Device,
        cell_bind_group_layout: &CellBindGroupLayout,
    ) -> Self {
        let header = cell.header().clone();

        let points = cell
            .points()
            .iter()
            .map(|it| Point {
                position: it.pos,
                color: it.color,
            })
            .collect_vec();

        let vertex_buffer = VertexBuffer::new(device, &points);
        let bind_group_data = CellBindGroupData::new(device, cell_bind_group_layout, header.id);

        Self {
            header: CellHeader(header),
            vertex_buffer,
            bind_group_data,
            visibility: Visibility::new(true),
        }
    }
}

fn receive_cell(
    mut commands: Commands,
    channels: ChannelsRes,
    device: Res<Device>,
    cell_bind_group_layout: Res<CellBindGroupLayout>,
    mut loaded_cells: ResMut<LoadedCells>,
    mut missing_cells: ResMut<MissingCells>,
    mut loading_cells: ResMut<LoadingCells>,
) {
    for _ in 0..LoadingCells::MAX_LOADING_SIZE {
        match channels.loaded_receiver.try_recv() {
            Ok(LoadedCellMsg { id, cell }) => {
                if !loading_cells.loading.remove(&id) {
                    log::debug!("Cell {:?} was loaded but no longer needed", id);
                    continue;
                }

                match cell {
                    Ok(Some(cell)) => {
                        log::debug!("Loaded cell: {:?}", id);

                        let cell_bundle = CellBundle::new(cell, &device, &cell_bind_group_layout);
                        let entity = commands.spawn(cell_bundle).id();

                        loaded_cells.0.insert(id, entity);
                    }
                    Ok(None) => {
                        log::debug!("Cell is missing: {:?}", id);
                        missing_cells.0.put(id, ());
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

fn update_cells(
    mut commands: Commands,
    camera_query: Query<
        (&frustums::StreamingFrustums, &Transform, &Frustum),
        (With<Camera>, Changed<frustums::StreamingFrustums>),
    >,
    active_metadata: ActiveMetadataRes,
    mut loaded_cells: ResMut<LoadedCells>,
    mut missing_cells: ResMut<MissingCells>,
    mut loading_cells: ResMut<LoadingCells>,
) {
    let metadata = &active_metadata.metadata;

    for (streaming_frustums, transform, frustum) in camera_query.iter() {
        let mut new_loaded = FxHashMap::with_capacity(loaded_cells.0.capacity());
        let mut new_should_load = BTreeSet::new();
        let mut frustum_planes = frustum.planes.clone();

        for (hierarchy, streaming_frustum) in streaming_frustums.iter().enumerate() {
            let hierarchy = hierarchy as u32;

            let cell_size = metadata.cell_size(hierarchy);
            let half_cell_size = cell_size / 2.0;

            frustum_planes.far = streaming_frustum.far_plane;

            let ids = streaming_frustum
                .cell_indices()
                .map(move |index| CellId { index, hierarchy })
                .filter(|cell_id| missing_cells.0.get(cell_id).is_none())
                .filter(|cell_id| {
                    let cell_pos = metadata.cell_pos(cell_id.index, cell_size);
                    let cell_aabb = Aabb::new(cell_pos - half_cell_size, cell_pos + half_cell_size);
                    !frustum_planes.cull_aabb(cell_aabb)
                })
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
                if let Some(entity) = loaded_cells.0.remove(&cell_to_load.id) {
                    new_loaded.insert(cell_to_load.id, entity);
                } else if loading_cells.should_load.remove(&cell_to_load)
                    || !loading_cells.loading.contains(&cell_to_load.id)
                {
                    new_should_load.insert(cell_to_load);
                }
            }
        }

        for (_, entity) in loaded_cells.0.drain() {
            commands.entity(entity).despawn();
        }

        loaded_cells.0 = new_loaded;
        loading_cells.should_load = new_should_load;
    }
}

fn enqueue_cells_to_load(
    mut loading_cells: ResMut<LoadingCells>,
    active_metadata: ActiveMetadataRes,
    channels: ChannelsRes,
) {
    let free_load_slots = LoadingCells::MAX_LOADING_SIZE - loading_cells.loading.len();

    for _ in 0..free_load_slots {
        if let Some(cell_to_load) = loading_cells.should_load.pop_first() {
            loading_cells.loading.insert(cell_to_load.id);

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

#[derive(Default, Resource)]
struct Stats {
    loaded_points: u64,
    loaded_points_byte_size: u64,
}

fn count_points(cell_header_query: Query<&CellHeader>, mut stats: ResMut<Stats>) {
    let total_points = cell_header_query
        .iter()
        .map(|header| header.0.number_of_points as u64)
        .sum();
    stats.loaded_points = total_points;
    stats.loaded_points_byte_size = std::mem::size_of::<Point>() as u64 * total_points;
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

    frustums::draw_ui(ui, world);

    {
        let loaded_cells = world.get_resource::<LoadedCells>().unwrap();
        let missing_cells = world.get_resource::<MissingCells>().unwrap();
        let loading_cells = world.get_resource::<LoadingCells>().unwrap();

        ui.label(format!("Loaded cells: {}", loaded_cells.0.len()));
        ui.label(format!("Missing cells: {}", missing_cells.0.len()));
        ui.label(format!(
            "Cells to load: {}",
            loading_cells.should_load.len()
        ));
    }

    {
        let stats = world.get_resource::<Stats>().unwrap();
        ui.label(format!(
            "Loaded points: {}",
            stats.loaded_points.separate_with_commas()
        ));
        ui.label(format!(
            "Loaded size: {}",
            ByteSize(stats.loaded_points_byte_size)
        ));
    }
}
