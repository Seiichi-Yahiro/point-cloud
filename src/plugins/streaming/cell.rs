use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::hash::BuildHasherDefault;

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bytesize::ByteSize;
use caches::{Cache, LRUCache};
use egui::ahash::{HashMapExt, HashSetExt};
use flume::{Receiver, Sender, TryRecvError, TrySendError};
use glam::IVec3;
use itertools::Itertools;
use rustc_hash::{FxHashMap, FxHashSet, FxHasher};
use thousands::Separable;

use point_converter::cell::{Cell, CellId};
use point_converter::metadata::MetadataConfig;

use crate::plugins::camera::frustum::Aabb;
use crate::plugins::camera::projection::PerspectiveProjection;
use crate::plugins::camera::{Camera, UpdateFrustum, Visibility};
use crate::plugins::render::point::Point;
use crate::plugins::render::vertex::VertexBuffer;
use crate::plugins::streaming::cell::loader::{LoadCellMsg, LoadedCellMsg};
use crate::plugins::streaming::cell::shader::{
    CellBindGroupData, CellBindGroupLayout, FrustumsSettings,
};
use crate::plugins::streaming::metadata::{ActiveMetadataRes, MetadataState};
use crate::plugins::wgpu::Device;
use crate::transform::Transform;

pub mod frustums;
mod loader;
pub mod shader;

pub struct CellPlugin;

impl Plugin for CellPlugin {
    fn build(&self, app: &mut App) {
        #[cfg(not(target_arch = "wasm32"))]
        app.world.insert_resource(CellLoader::default());

        #[cfg(target_arch = "wasm32")]
        app.world.insert_non_send_resource(CellLoader::default());

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
            .add_systems(
                OnEnter(MetadataState::Loaded),
                (
                    set_view_distance,
                    start_cell_loader,
                    shader::set_frustums_settings_max_hierarchy,
                ),
            )
            .add_systems(OnExit(MetadataState::Loaded), stop_cell_loader)
            .add_systems(OnEnter(MetadataState::Loading), cleanup_cells)
            .add_systems(
                PostUpdate,
                (
                    (
                        shader::update_loaded_cells_buffer.run_if(resource_changed::<LoadedCells>),
                        shader::update_frustums_buffer,
                        shader::update_frustums_settings_buffer
                            .run_if(resource_changed::<FrustumsSettings>),
                    ),
                    shader::update_cells_bind_group,
                )
                    .chain()
                    .run_if(in_state(MetadataState::Loaded))
                    .run_if(in_state(StreamState::Enabled)),
            );
    }
}

fn set_view_distance(
    active_metadata: ActiveMetadataRes,
    mut camera_query: Query<&mut PerspectiveProjection, With<Camera>>,
) {
    for mut projection in camera_query.iter_mut() {
        projection.far = active_metadata.metadata.config.max_cell_size;
    }
}

#[derive(Default)]
#[cfg_attr(not(target_arch = "wasm32"), derive(Resource))]
enum CellLoader {
    #[default]
    Stopped,
    Running {
        load_sender: Sender<LoadCellMsg>,
        loaded_receiver: Receiver<LoadedCellMsg>,
    },
}

impl CellLoader {
    fn start(&mut self, config: &MetadataConfig) {
        if let CellLoader::Stopped = self {
            let (load_sender, load_receiver) = flume::unbounded::<LoadCellMsg>();
            let (loaded_sender, loaded_receiver) = flume::unbounded::<LoadedCellMsg>();

            loader::spawn_cell_loader(config.clone(), load_receiver, loaded_sender);

            *self = CellLoader::Running {
                load_sender,
                loaded_receiver,
            };
        }
    }

    fn stop(&mut self) {
        if let CellLoader::Running { load_sender, .. } = self {
            load_sender.send(LoadCellMsg::Stop).unwrap();
            *self = CellLoader::Stopped;
        }
    }

    fn send(&self, msg: LoadCellMsg) -> Result<(), TrySendError<LoadCellMsg>> {
        if let CellLoader::Running { load_sender, .. } = self {
            load_sender.try_send(msg)
        } else {
            Err(TrySendError::Disconnected(msg))
        }
    }

    fn receive(&self) -> Result<LoadedCellMsg, TryRecvError> {
        if let CellLoader::Running {
            loaded_receiver, ..
        } = self
        {
            loaded_receiver.try_recv()
        } else {
            Err(TryRecvError::Disconnected)
        }
    }
}

impl Drop for CellLoader {
    fn drop(&mut self) {
        if let CellLoader::Running { load_sender, .. } = self {
            load_sender.send(LoadCellMsg::Stop).unwrap();
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
type CellLoaderRes<'w> = Res<'w, CellLoader>;

#[cfg(target_arch = "wasm32")]
type CellLoaderRes<'w> = NonSend<'w, CellLoader>;

#[cfg(not(target_arch = "wasm32"))]
type CellLoaderResMut<'w> = ResMut<'w, CellLoader>;

#[cfg(target_arch = "wasm32")]
type CellLoaderResMut<'w> = NonSendMut<'w, CellLoader>;

fn start_cell_loader(active_metadata: ActiveMetadataRes, mut cell_loader: CellLoaderResMut) {
    cell_loader.start(&active_metadata.metadata.config);
}

fn stop_cell_loader(mut cell_loader: CellLoaderResMut) {
    cell_loader.stop();
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
            .all_points()
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
    cell_loader: CellLoaderRes,
    device: Res<Device>,
    cell_bind_group_layout: Res<CellBindGroupLayout>,
    mut loaded_cells: ResMut<LoadedCells>,
    mut missing_cells: ResMut<MissingCells>,
    mut loading_cells: ResMut<LoadingCells>,
) {
    for _ in 0..LoadingCells::MAX_LOADING_SIZE {
        match cell_loader.receive() {
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
    mut loaded_cells: ResMut<LoadedCells>,
    mut missing_cells: ResMut<MissingCells>,
    mut loading_cells: ResMut<LoadingCells>,
    camera_query: Query<
        (&frustums::StreamingFrustums, &Transform),
        (With<Camera>, Changed<frustums::StreamingFrustums>),
    >,
    active_metadata: ActiveMetadataRes,
) {
    let metadata = &active_metadata.metadata;

    for (streaming_frustums, transform) in camera_query.iter() {
        let mut new_loaded = FxHashMap::with_capacity(loaded_cells.0.capacity());
        let mut new_should_load = BTreeSet::new();

        for (hierarchy, streaming_frustum) in streaming_frustums.iter().enumerate() {
            let hierarchy = hierarchy as u32;

            let cell_size = metadata.config.cell_size(hierarchy);
            let half_cell_size = cell_size / 2.0;

            let mut frustum_aabb = streaming_frustum.aabb();
            frustum_aabb.clamp(metadata.bounding_box.min, metadata.bounding_box.max);
            let min_cell_index = metadata.config.cell_index(frustum_aabb.min, cell_size);
            let max_cell_index = metadata.config.cell_index(frustum_aabb.max, cell_size);

            let new_visible_cells = (min_cell_index.x..=max_cell_index.x)
                .cartesian_product(min_cell_index.y..=max_cell_index.y)
                .cartesian_product(min_cell_index.z..=max_cell_index.z)
                .map(|((x, y), z)| IVec3::new(x, y, z))
                .map(|cell_index| CellId {
                    hierarchy,
                    index: cell_index,
                })
                .filter(|cell_id| missing_cells.0.get(cell_id).is_none())
                .filter(|cell_id| {
                    let cell_pos = metadata.config.cell_pos(cell_id.index, cell_size);
                    let cell_aabb = Aabb::new(cell_pos - half_cell_size, cell_pos + half_cell_size);
                    !streaming_frustum.cull_aabb(cell_aabb)
                })
                .map(|cell_id| {
                    let cell_pos = metadata.config.cell_pos(cell_id.index, cell_size);
                    let distance_to_camera =
                        (cell_pos - transform.translation).length_squared() as u32;
                    CellToLoad {
                        id: cell_id,
                        priority: distance_to_camera,
                    }
                });

            for cell_to_load in new_visible_cells {
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
    cell_loader: CellLoaderRes,
) {
    let free_load_slots = LoadingCells::MAX_LOADING_SIZE - loading_cells.loading.len();

    for _ in 0..free_load_slots {
        if let Some(cell_to_load) = loading_cells.should_load.pop_first() {
            loading_cells.loading.insert(cell_to_load.id);

            cell_loader
                .send(LoadCellMsg::Cell {
                    id: cell_to_load.id,
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
        .map(|header| header.0.total_number_of_points as u64)
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
