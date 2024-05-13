use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::hash::BuildHasherDefault;
use std::io::Read;

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bytesize::ByteSize;
use caches::{Cache, LRUCache};
use egui::ahash::{HashMapExt, HashSetExt};
use glam::IVec3;
use itertools::Itertools;
use rustc_hash::{FxHashMap, FxHashSet, FxHasher};
use thousands::Separable;

use point_converter::cell::{Cell, CellId};
use point_converter::metadata::MetadataConfig;

use crate::plugins::asset::source::SourceError;
use crate::plugins::asset::{
    Asset, AssetHandle, AssetManagerRes, AssetPlugin, LoadAssetMsg, LoadedAssetEvent,
};
use crate::plugins::camera::frustum::Aabb;
use crate::plugins::camera::projection::PerspectiveProjection;
use crate::plugins::camera::{Camera, UpdateFrustum, Visibility};
use crate::plugins::cell::shader::{CellBindGroupData, CellBindGroupLayout, FrustumsSettings};
use crate::plugins::metadata::{ActiveMetadata, MetadataState};
use crate::plugins::render::point::Point;
use crate::plugins::render::vertex::VertexBuffer;
use crate::plugins::wgpu::Device;
use crate::transform::Transform;

pub mod frustums;
pub mod shader;

impl Asset for Cell {
    type Id = CellId;

    fn read_from(reader: &mut dyn Read) -> Result<Self, SourceError> {
        // TODO config
        Cell::read_from(reader, &MetadataConfig::default()).map_err(SourceError::from)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn save(&self, source: crate::plugins::asset::source::Source) -> Result<(), SourceError> {
        use crate::plugins::asset::source::Source;
        use std::fs::{create_dir, File};
        use std::io::{BufWriter, ErrorKind, Write};

        match source {
            Source::Path(path) => {
                log::debug!("Saving cell at {:?}", path);

                if let Err(err) = create_dir(path.parent().unwrap()) {
                    match err.kind() {
                        ErrorKind::AlreadyExists => {}
                        _ => {
                            return Err(err.into());
                        }
                    }
                }

                let file = File::create(path)?;
                let mut buf_writer = BufWriter::new(file);
                self.write_to(&mut buf_writer)?;
                buf_writer.flush().map_err(SourceError::from)
            }
            Source::URL(_) => {
                todo!()
            }
            Source::None => Ok(()),
        }
    }
}

pub struct CellPlugin;

impl Plugin for CellPlugin {
    fn build(&self, app: &mut App) {
        shader::setup(&mut app.world);

        app.add_plugins(AssetPlugin::<Cell>::default())
            .insert_state(StreamState::Enabled)
            .insert_resource(frustums::StreamingFrustumsScale::default())
            .insert_resource(LoadedCells::default())
            .insert_resource(MissingCells::default())
            .insert_resource(LoadingCells::default())
            .insert_resource(Stats::default())
            .configure_sets(
                Update,
                CellStreamingSet
                    .run_if(in_state(MetadataState::Loaded))
                    .run_if(in_state(StreamState::Enabled)),
            )
            .configure_sets(
                PostUpdate,
                CellStreamingSet
                    .run_if(in_state(MetadataState::Loaded))
                    .run_if(in_state(StreamState::Enabled)),
            )
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
                    .in_set(CellStreamingSet),
            )
            .add_systems(
                OnEnter(MetadataState::Loaded),
                (
                    set_view_distance,
                    shader::set_frustums_settings_max_hierarchy,
                ),
            )
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
                    .in_set(CellStreamingSet),
            );
    }
}

fn set_view_distance(
    active_metadata: ActiveMetadata,
    mut camera_query: Query<&mut PerspectiveProjection, With<Camera>>,
) {
    for mut projection in camera_query.iter_mut() {
        projection.far = active_metadata.get().config.max_cell_size;
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash, SystemSet)]
pub struct CellStreamingSet;

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
    cell_handle: AssetHandle<Cell>,
    header: CellHeader,
    vertex_buffer: VertexBuffer<Point>,
    bind_group_data: CellBindGroupData,
    visibility: Visibility,
}

impl CellBundle {
    fn new(
        cell_handle: AssetHandle<Cell>,
        cell: &Cell,
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
            cell_handle,
            header: CellHeader(header),
            vertex_buffer,
            bind_group_data,
            visibility: Visibility::new(true),
        }
    }
}

fn receive_cell(
    mut commands: Commands,
    cell_manager: AssetManagerRes<Cell>,
    mut loaded_assets_events: EventReader<LoadedAssetEvent<Cell>>,
    device: Res<Device>,
    cell_bind_group_layout: Res<CellBindGroupLayout>,
    mut loaded_cells: ResMut<LoadedCells>,
    mut missing_cells: ResMut<MissingCells>, // TODO move missing into manager?
    mut loading_cells: ResMut<LoadingCells>,
) {
    for event in loaded_assets_events.read() {
        match event {
            LoadedAssetEvent::Success { handle } => {
                let id = handle.id();

                if !loading_cells.loading.remove(id) {
                    log::debug!("Cell {:?} was loaded but not needed", id);
                    continue;
                }

                log::debug!("Loaded cell: {:?}", id);

                // TODO delay reading of cell
                let cell = cell_manager.get(handle).asset();
                let cell_bundle =
                    CellBundle::new(handle.clone(), cell, &device, &cell_bind_group_layout);
                let entity = commands.spawn(cell_bundle).id();

                loaded_cells.0.insert(*id, entity);
            }
            LoadedAssetEvent::Error { id, error } => {
                loading_cells.loading.remove(id);

                match error {
                    SourceError::NotFound(_) => {
                        log::debug!("Cell is missing: {:?}", id);
                        missing_cells.0.put(*id, ());
                    }
                    _ => {
                        // TODO do something with the failed cell
                        log::error!("Failed to load cell {:?}: {:?}", id, error);
                    }
                }
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
    active_metadata: ActiveMetadata,
) {
    let metadata = active_metadata.get();

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
    active_metadata: ActiveMetadata,
    cell_manager: AssetManagerRes<Cell>,
) {
    let free_load_slots = LoadingCells::MAX_LOADING_SIZE - loading_cells.loading.len();

    for _ in 0..free_load_slots {
        if let Some(cell_to_load) = loading_cells.should_load.pop_first() {
            loading_cells.loading.insert(cell_to_load.id);

            let working_directory = active_metadata.get_working_directory().unwrap();
            let source = working_directory.join(&cell_to_load.id.path());

            cell_manager
                .load_sender()
                .send(LoadAssetMsg {
                    id: cell_to_load.id,
                    source,
                    reply_sender: None,
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
