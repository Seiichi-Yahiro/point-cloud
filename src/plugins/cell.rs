use std::hash::{BuildHasherDefault, Hash};
use std::io::Read;

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_state::prelude::*;
use bytesize::ByteSize;
use caches::{Cache, LRUCache};
use egui::ahash::HashSetExt;
use glam::IVec3;
use itertools::Itertools;
use rustc_hash::{FxHashMap, FxHashSet, FxHasher};
use thousands::Separable;

use bounding_volume::Aabb;
use point_converter::cell::{Cell, CellId};

use crate::plugins::asset::source::{Source, SourceError};
use crate::plugins::asset::{
    Asset, AssetEvent, AssetHandle, AssetLoadedEvent, AssetManagerRes, AssetPlugin, LoadAssetMsg,
};
use crate::plugins::camera::projection::PerspectiveProjection;
use crate::plugins::camera::{Camera, UpdateFrustum, Visibility};
use crate::plugins::cell::frustums::StreamingFrustumsScale;
use crate::plugins::cell::shader::{CellBufferBundle, FrustumsSettings};
use crate::plugins::metadata::{
    ActiveMetadata, MetadataState, UpdatedMetadataBoundingBoxEvent, UpdatedMetadataHierarchiesEvent,
};
use crate::plugins::render::point::Point;
use crate::plugins::render::BufferSet;
use crate::plugins::wgpu::Device;
use crate::sorted_hash::SortedHashMap;
use crate::transform::Transform;

pub mod frustums;
pub mod shader;

impl Asset for Cell {
    type Id = CellId;

    fn read_from(reader: &mut dyn Read) -> Result<Self, SourceError> {
        Cell::read_from(reader).map_err(SourceError::from)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn save(&self, source: Source) -> Result<(), SourceError> {
        use std::fs::{create_dir_all, File};
        use std::io::{BufWriter, ErrorKind, Write};

        match source {
            Source::Path(path) => {
                log::debug!("Saving cell at {:?}", path);

                if let Err(err) = create_dir_all(path.parent().unwrap()) {
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
            Source::URL(_) => Err(SourceError::Other {
                message: "URL saving is not supported".to_string(),
                name: ErrorKind::Unsupported,
            }),
            Source::None => Err(SourceError::NoSource),
        }
    }
}

pub struct CellPlugin;

impl Plugin for CellPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(AssetPlugin::<Cell>::default())
            .insert_state(StreamState::Enabled)
            .insert_resource(frustums::StreamingFrustumsScale::default())
            .insert_resource(VisibleCells::default())
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
            .add_systems(
                Startup,
                (
                    shader::create_loaded_cells_buffer,
                    shader::create_frustums_buffer,
                    shader::create_frustums_settings_buffer,
                )
                    .in_set(BufferSet),
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
                    shader::update_loaded_cells_buffer.run_if(resource_changed::<LoadedCells>),
                    shader::update_frustums_buffer,
                    (
                        shader::set_frustums_settings_max_hierarchy
                            .run_if(on_event::<UpdatedMetadataHierarchiesEvent>()),
                        shader::update_frustums_settings_buffer
                            .run_if(resource_changed::<FrustumsSettings>),
                    )
                        .chain(),
                )
                    .chain()
                    .in_set(CellStreamingSet)
                    .in_set(BufferSet),
            );
    }
}

fn set_view_distance(
    active_metadata: ActiveMetadata,
    mut camera_query: Query<&mut PerspectiveProjection, With<Camera>>,
) {
    for mut projection in camera_query.iter_mut() {
        projection.far = active_metadata.get().config.max_cell_size * StreamingFrustumsScale::MAX;
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash, SystemSet)]
pub struct CellStreamingSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, States)]
pub enum StreamState {
    Enabled,
    Paused,
}

#[derive(Debug, Default, Resource)]
struct VisibleCells {
    hierarchies: Vec<FxHashSet<IVec3>>,
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

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct CellSortValue {
    hierarchy: u32,
    distance_to_camera: u32,
}

#[derive(Resource)]
struct LoadingCells {
    should_load: SortedHashMap<CellId, CellSortValue, ()>,
    loading: FxHashSet<CellId>,
}

impl LoadingCells {
    const MAX_LOADING_SIZE: usize = 10;
}

impl Default for LoadingCells {
    fn default() -> Self {
        Self {
            should_load: SortedHashMap::new(),
            loading: FxHashSet::with_capacity(Self::MAX_LOADING_SIZE),
        }
    }
}

fn cleanup_cells(
    mut commands: Commands,
    cell_query: Query<Entity, With<CellHeader>>,
    mut visible_cells: ResMut<VisibleCells>,
    mut loaded_cells: ResMut<LoadedCells>,
    mut missing_cells: ResMut<MissingCells>,
    mut loading_cells: ResMut<LoadingCells>,
) {
    visible_cells.hierarchies.clear();
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
    buffer_bundle: CellBufferBundle,
    visibility: Visibility,
}

impl CellBundle {
    fn new(cell_handle: AssetHandle<Cell>, cell: &Cell, device: &wgpu::Device) -> Self {
        Self {
            cell_handle,
            header: CellHeader(cell.header().clone()),
            buffer_bundle: CellBufferBundle::new(device, cell),
            visibility: Visibility::new(true),
        }
    }
}

fn receive_cell(
    mut commands: Commands,
    cell_manager: AssetManagerRes<Cell>,
    mut assets_events: EventReader<AssetEvent<Cell>>,
    device: Res<Device>,
    visible_cells: Res<VisibleCells>,
    mut loaded_cells: ResMut<LoadedCells>,
    mut missing_cells: ResMut<MissingCells>,
    mut loading_cells: ResMut<LoadingCells>,
) {
    for event in assets_events.read() {
        match event {
            AssetEvent::Created { handle } => {
                let id = handle.id();
                missing_cells.0.remove(id);

                if visible_cells
                    .hierarchies
                    .get(id.hierarchy as usize)
                    .map(|cell_indices| cell_indices.contains(&id.index))
                    .unwrap_or(false)
                {
                    log::debug!("Received created cell {:?}", id);
                    loading_cells.should_load.remove(id);

                    // TODO delay reading of cell
                    let cell = cell_manager.get_asset(handle);
                    let cell_bundle = CellBundle::new(handle.clone(), cell, &device);
                    let entity = commands.spawn(cell_bundle).id();

                    loaded_cells.0.insert(*id, entity);
                }
            }
            AssetEvent::Changed { handle } => {
                if let Some(entity) = loaded_cells.0.get(handle.id()) {
                    log::debug!("Reloading points for {:?}", handle.id());

                    let cell = cell_manager.get_asset(handle);
                    let cell_buffer_bundle = CellBufferBundle::new(&device, cell);

                    commands.entity(*entity).insert(cell_buffer_bundle);
                }
            }
            AssetEvent::Loaded(AssetLoadedEvent::Success { handle }) => {
                let id = handle.id();

                if !loading_cells.loading.remove(id) {
                    log::debug!("Cell {:?} was loaded but not needed", id);
                    continue;
                }

                log::debug!("Loaded cell: {:?}", id);

                // TODO delay reading of cell
                let cell = cell_manager.get_asset(handle);
                let cell_bundle = CellBundle::new(handle.clone(), cell, &device);

                let entity = commands.spawn(cell_bundle).id();

                if let Some(old) = loaded_cells.0.insert(*id, entity) {
                    log::warn!("Loaded cell {:?} already existed", id);
                    if let Some(mut entity_commands) = commands.get_entity(old) {
                        entity_commands.despawn();
                    }
                }
            }
            AssetEvent::Loaded(AssetLoadedEvent::Error { id, error }) => {
                if !loading_cells.loading.remove(id) {
                    continue;
                }

                match error {
                    SourceError::NotFound(_) => {
                        log::debug!("Cell is missing: {:?}", id);
                        missing_cells.0.put(*id, ());
                    }
                    SourceError::NoSource => {}
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
    mut visible_cells: ResMut<VisibleCells>,
    mut loaded_cells: ResMut<LoadedCells>,
    mut missing_cells: ResMut<MissingCells>,
    mut loading_cells: ResMut<LoadingCells>,
    camera_query: Query<(Ref<frustums::StreamingFrustums>, &Transform), With<Camera>>,
    active_metadata: ActiveMetadata,
    mut updated_bounding_box_events: EventReader<UpdatedMetadataBoundingBoxEvent>,
) {
    let metadata = active_metadata.get();
    let updated_metadata = updated_bounding_box_events.read().count() > 0;

    for (streaming_frustums, transform) in camera_query.iter() {
        if !(streaming_frustums.is_changed() || updated_metadata) {
            continue;
        }

        let mut new_visible_cells = Vec::with_capacity(metadata.hierarchies as usize);

        for (hierarchy, streaming_frustum) in streaming_frustums.iter().enumerate() {
            let old_visible_cells = visible_cells.hierarchies.get(hierarchy);

            let hierarchy = hierarchy as u32;

            let cell_size = metadata.config.cell_size(hierarchy);
            let half_cell_size = cell_size / 2.0;

            let mut frustum_aabb = streaming_frustum.aabb();
            frustum_aabb.clamp(metadata.bounding_box.min, metadata.bounding_box.max);
            let min_cell_index = metadata.config.cell_index(frustum_aabb.min, cell_size);
            let max_cell_index = metadata.config.cell_index(frustum_aabb.max, cell_size);

            let visible_cells: FxHashSet<IVec3> = (min_cell_index.x..=max_cell_index.x)
                .cartesian_product(min_cell_index.y..=max_cell_index.y)
                .cartesian_product(min_cell_index.z..=max_cell_index.z)
                .map(|((x, y), z)| IVec3::new(x, y, z))
                .filter(|cell_index| {
                    let cell_pos = metadata.config.cell_pos(*cell_index, cell_size);
                    let cell_aabb = Aabb::new(cell_pos - half_cell_size, cell_pos + half_cell_size);
                    !streaming_frustum.cull_aabb(cell_aabb)
                })
                .collect();

            let not_visible_anymore_cells = old_visible_cells
                .map_or_else::<Box<dyn Iterator<Item = &IVec3>>, _, _>(
                    || Box::new(std::iter::empty()),
                    |old| Box::new(old.difference(&visible_cells)),
                )
                .map(|cell_index| CellId {
                    hierarchy,
                    index: *cell_index,
                });

            for cell_id in not_visible_anymore_cells {
                if let Some(entity) = loaded_cells.0.remove(&cell_id) {
                    commands.entity(entity).despawn();
                } else if loading_cells.should_load.remove(&cell_id).is_none() {
                    loading_cells.loading.remove(&cell_id);
                }
            }

            let completely_new_visible_cells = old_visible_cells
                .map_or_else::<Box<dyn Iterator<Item = &IVec3>>, _, _>(
                    || Box::new(visible_cells.iter()),
                    |old| Box::new(visible_cells.difference(old)),
                )
                .map(|cell_index| CellId {
                    hierarchy,
                    index: *cell_index,
                })
                .filter(|cell_id| missing_cells.0.get(cell_id).is_none());

            for cell_id in completely_new_visible_cells {
                let cell_pos = metadata.config.cell_pos(cell_id.index, cell_size);
                let distance_to_camera = (cell_pos - transform.translation).length_squared() as u32;

                let sort_value = CellSortValue {
                    hierarchy: cell_id.hierarchy,
                    distance_to_camera,
                };

                loading_cells.should_load.insert(cell_id, sort_value, ());
            }

            new_visible_cells.push(visible_cells);
        }

        visible_cells.hierarchies = new_visible_cells;
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
            let cell_id = cell_to_load.keys.hash_key;
            loading_cells.loading.insert(cell_id);

            let working_directory = active_metadata.get_working_directory();
            let source = working_directory.map_or(Source::None, |dir| dir.join(&cell_id.path()));

            cell_manager
                .load_sender()
                .send(LoadAssetMsg {
                    id: cell_id,
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
