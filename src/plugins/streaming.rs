use std::collections::VecDeque;
use std::ops::{Deref, DerefMut};

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use cfg_if::cfg_if;
use egui::ahash::{HashMapExt, HashSetExt};
use flume::TryRecvError;
use glam::{IVec3, Vec3};
use itertools::Itertools;
use rustc_hash::{FxHashMap, FxHashSet};

use point_converter::cell::CellId;
use point_converter::metadata::Metadata;

use crate::plugins::camera::frustum::{Aabb, Frustum};
use crate::plugins::camera::projection::PerspectiveProjection;
use crate::plugins::camera::{Camera, UpdateFrustum, Visibility};
use crate::plugins::render::vertex::VertexBuffer;
use crate::plugins::streaming::loader::{spawn_loader, LoadFile, LoadedCell, LoadedMetadata};
use crate::plugins::wgpu::Device;
use crate::transform::Transform;

mod loader;

#[derive(Component)]
pub struct CellData {
    pub id: CellId,
    pub pos: Vec3,
    pub size: f32,
}

pub struct StreamingPlugin;

impl Plugin for StreamingPlugin {
    fn build(&self, app: &mut App) {
        let (load_sender, load_receiver) = flume::unbounded::<LoadFile>();

        let (loaded_metadata_sender, loaded_metadata_receiver) =
            flume::bounded::<LoadedMetadata>(1);

        let (loaded_cell_sender, loaded_cell_receiver) = flume::unbounded::<LoadedCell>();

        spawn_loader(load_receiver, loaded_metadata_sender, loaded_cell_sender);

        let channels = Channels {
            load_sender,
            loaded_metadata_receiver,
            loaded_cell_receiver,
        };

        cfg_if! {
            if #[cfg(target_arch = "wasm32")] {
                app.insert_non_send_resource(ActiveMetadata::default()).insert_non_send_resource(channels);
            } else {
                app.insert_resource(ActiveMetadata::default()).insert_resource(channels);
            }
        }

        app.insert_state(MetadataState::NotLoaded)
            .insert_state(StreamState::Enabled)
            .insert_resource(Cells::default())
            .add_systems(PostStartup, add_hierarchy_spheres)
            .add_systems(
                Update,
                (
                    receive_metadata.run_if(in_state(MetadataState::Loading)),
                    (
                        (receive_cell, update_hierarchy_spheres.after(UpdateFrustum)),
                        update_cells,
                        enqueue_cells_to_load,
                    )
                        .chain()
                        .run_if(in_state(MetadataState::Loaded))
                        .run_if(in_state(StreamState::Enabled)),
                ),
            )
            .add_systems(OnEnter(MetadataState::Loaded), look_at_bounding_box)
            .add_systems(OnExit(MetadataState::Loaded), despawn_cells);

        #[cfg(target_arch = "wasm32")]
        app.add_systems(
            Update,
            handle_selection.run_if(in_state(MetadataState::Selecting)),
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, States)]
pub enum MetadataState {
    NotLoaded,
    #[cfg(target_arch = "wasm32")]
    Selecting,
    Loading,
    Loaded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, States)]
enum StreamState {
    Enabled,
    Paused,
}

#[cfg_attr(not(target_arch = "wasm32"), derive(Resource))]
pub struct ActiveMetadata {
    pub source: Source,
    pub metadata: Metadata,
}

impl Default for ActiveMetadata {
    fn default() -> Self {
        Self {
            source: Source::None,
            metadata: Metadata::default(),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub type ActiveMetadataRes<'w> = Res<'w, ActiveMetadata>;

#[cfg(target_arch = "wasm32")]
pub type ActiveMetadataRes<'w> = NonSend<'w, ActiveMetadata>;

#[cfg(not(target_arch = "wasm32"))]
pub type ActiveMetadataResMut<'w> = ResMut<'w, ActiveMetadata>;

#[cfg(target_arch = "wasm32")]
pub type ActiveMetadataResMut<'w> = NonSendMut<'w, ActiveMetadata>;

#[cfg(not(target_arch = "wasm32"))]
type Directory = std::path::PathBuf;

#[cfg(target_arch = "wasm32")]
type Directory = web_sys::FileSystemDirectoryHandle;

#[derive(Debug, Clone)]
pub enum Source {
    Directory(Directory),
    URL,
    None,
}

#[cfg_attr(not(target_arch = "wasm32"), derive(Resource))]
struct Channels {
    load_sender: flume::Sender<LoadFile>,
    loaded_metadata_receiver: flume::Receiver<LoadedMetadata>,
    loaded_cell_receiver: flume::Receiver<LoadedCell>,
}

#[cfg(not(target_arch = "wasm32"))]
type ChannelsRes<'w> = Res<'w, Channels>;

#[cfg(target_arch = "wasm32")]
type ChannelsRes<'w> = NonSend<'w, Channels>;

impl Drop for Channels {
    fn drop(&mut self) {
        self.load_sender.send(LoadFile::Stop).unwrap();
    }
}

fn despawn_cells(mut commands: Commands, query: Query<Entity, With<CellData>>) {
    for entity in query.iter() {
        commands.entity(entity).despawn();
    }
}

fn receive_metadata(
    channels: ChannelsRes,
    mut active_metadata: ActiveMetadataResMut,
    mut cells: ResMut<Cells>,
    mut next_metadata_state: ResMut<NextState<MetadataState>>,
) {
    match channels.loaded_metadata_receiver.try_recv() {
        Ok(LoadedMetadata { source, metadata }) => match metadata {
            Ok(metadata) => {
                log::debug!(
                    "Loaded metadata for {} with {} points",
                    metadata.name,
                    metadata.number_of_points
                );

                next_metadata_state.set(MetadataState::Loaded);
                *active_metadata = ActiveMetadata { metadata, source };

                cells.should_load.clear();
                cells.loading.clear();
                cells.loaded.clear();
            }
            Err(err) => {
                log::error!("Failed to load metadata: {:?}", err);
                next_metadata_state.set(MetadataState::NotLoaded);
            }
        },
        Err(TryRecvError::Disconnected) => {
            panic!("Failed to stream files as the sender was dropped");
        }
        Err(TryRecvError::Empty) => {}
    }
}

fn receive_cell(
    mut commands: Commands,
    channels: ChannelsRes,
    device: Res<Device>,
    mut cells: ResMut<Cells>,
) {
    for _ in 0..Cells::MAX_LOADING_SIZE {
        match channels.loaded_cell_receiver.try_recv() {
            Ok(LoadedCell { id, cell }) => {
                match cells.loading.pop_front() {
                    Some(queued_id) => {
                        if queued_id != id {
                            cells.loading.push_front(queued_id);

                            log::debug!("Cell {:?} was loaded but no longer needed", id);
                            return;
                        }
                    }
                    None => {
                        log::debug!("Cell {:?} was loaded but no longer needed", id);
                        return;
                    }
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

                        let buffer = VertexBuffer::new(&device, &points);
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
                            .spawn((cell_data, buffer, aabb, Visibility::new(true)))
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

#[derive(Debug)]
enum LoadedCellStatus {
    Missing,
    Loaded(Entity),
}

#[derive(Resource)]
struct Cells {
    loaded: FxHashMap<CellId, LoadedCellStatus>,
    should_load: FxHashSet<CellId>,
    loading: VecDeque<CellId>,
}

impl Cells {
    const MAX_LOADING_SIZE: usize = 10;
}

impl Default for Cells {
    fn default() -> Self {
        Self {
            loaded: FxHashMap::default(),
            should_load: FxHashSet::default(),
            loading: VecDeque::with_capacity(Self::MAX_LOADING_SIZE),
        }
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
    camera_query: Query<&HierarchySpheres, (With<Camera>, Changed<HierarchySpheres>)>,
    active_metadata: ActiveMetadataRes,
    mut cells: ResMut<Cells>,
) {
    let metadata = &active_metadata.metadata;

    for hierarchy_spheres in camera_query.iter() {
        let mut new_loaded = FxHashMap::with_capacity(cells.loaded.capacity());
        let mut new_should_load = FxHashSet::with_capacity(cells.should_load.capacity());

        for (hierarchy, sphere) in hierarchy_spheres.iter().enumerate() {
            let hierarchy = hierarchy as u32;

            let cell_size = metadata.cell_size(hierarchy);
            let min_cell_index = metadata.cell_index(sphere.pos - sphere.radius, cell_size);
            let max_cell_index = metadata.cell_index(sphere.pos + sphere.radius, cell_size);

            let ids = (min_cell_index.x..=max_cell_index.x)
                .cartesian_product(min_cell_index.y..=max_cell_index.y)
                .cartesian_product(min_cell_index.z..=max_cell_index.z)
                .map(|((x, y), z)| IVec3::new(x, y, z))
                .map(move |index| CellId { index, hierarchy });

            // copy or insert cells that need to be loaded
            for id in ids {
                if let Some(status) = cells.loaded.remove(&id) {
                    new_loaded.insert(id, status);
                } else if cells.should_load.remove(&id) || !cells.loading.contains(&id) {
                    new_should_load.insert(id);
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

    for id in cells
        .should_load
        .iter()
        .take(free_load_slots)
        .copied()
        .collect_vec()
    {
        cells.should_load.remove(&id);
        cells.loading.push_back(id);

        channels
            .load_sender
            .send(LoadFile::Cell {
                id,
                sub_grid_dimension: active_metadata.metadata.sub_grid_dimension,
                source: active_metadata.source.clone(),
            })
            .unwrap();
    }
}

fn look_at_bounding_box(
    mut query: Query<&mut Transform, With<Camera>>,
    active_metadata: ActiveMetadataRes,
) {
    let aabb = active_metadata.metadata.bounding_box;
    let center = (aabb.min + aabb.max) / 2.0;

    let center_max_z = center.with_z(aabb.max.z);

    for mut transform in query.iter_mut() {
        *transform = Transform::from_translation(aabb.max + (center_max_z - aabb.max) / 2.0)
            .looking_at(center, Vec3::Z);
    }
}

#[cfg(target_arch = "wasm32")]
enum MetadataSelection {
    Load(LoadFile),
    Canceled { had_metadata: bool },
}

#[cfg(target_arch = "wasm32")]
fn handle_selection(world: &mut World) {
    let receiver = world
        .remove_non_send_resource::<flume::Receiver<MetadataSelection>>()
        .unwrap();

    match receiver.try_recv() {
        Ok(MetadataSelection::Load(load_file)) => {
            world
                .get_resource_mut::<NextState<MetadataState>>()
                .unwrap()
                .set(MetadataState::Loading);

            let channels = world.get_non_send_resource::<Channels>().unwrap();
            channels.load_sender.send(load_file).unwrap();
        }
        Ok(MetadataSelection::Canceled { had_metadata }) => {
            let previous_state = if had_metadata {
                MetadataState::Loaded
            } else {
                MetadataState::NotLoaded
            };

            world
                .get_resource_mut::<NextState<MetadataState>>()
                .unwrap()
                .set(previous_state);
        }
        Err(TryRecvError::Disconnected) => {
            panic!("Sender disconnected while waiting for metadata selection");
        }
        Err(TryRecvError::Empty) => {
            world.insert_non_send_resource(receiver);
        }
    }
}

pub fn draw_ui(ui: &mut egui::Ui, world: &mut World) {
    {
        #[cfg(not(target_arch = "wasm32"))]
        let metadata = &world.get_resource::<ActiveMetadata>().unwrap().metadata;

        #[cfg(target_arch = "wasm32")]
        let metadata = &world
            .get_non_send_resource::<ActiveMetadata>()
            .unwrap()
            .metadata;

        ui.label(format!("Cloud name: {}", metadata.name));
        ui.label(format!("Points: {}", metadata.number_of_points));
        ui.label(format!("Hierarchies: {}", metadata.hierarchies));

        ui.collapsing("Extends", |ui| {
            let extends = metadata.bounding_box.max - metadata.bounding_box.min;

            ui.label(format!("x: {}", extends.x));
            ui.label(format!("y: {}", extends.y));
            ui.label(format!("z: {}", extends.z));
        });
    }

    select_metadata(ui, world);

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

#[cfg(not(target_arch = "wasm32"))]
fn select_metadata(ui: &mut egui::Ui, world: &mut World) {
    let current_metadata_state = *world.get_resource::<State<MetadataState>>().unwrap().get();

    let button = egui::Button::new("Choose metadata...");

    let enabled = match current_metadata_state {
        MetadataState::Loading => false,
        MetadataState::NotLoaded | MetadataState::Loaded => true,
    };

    if ui.add_enabled(enabled, button).clicked() {
        let dir = {
            let window: &winit::window::Window = world
                .get_resource::<crate::plugins::winit::Window>()
                .unwrap();

            rfd::FileDialog::new()
                .add_filter("metadata", &["json"])
                .set_parent(window)
                .pick_file()
                .and_then(|it| it.parent().map(std::path::Path::to_path_buf))
        };

        if let Some(dir) = dir {
            let mut params = bevy_ecs::system::SystemState::<(
                ChannelsRes,
                ResMut<NextState<MetadataState>>,
            )>::new(world);
            let (channels, mut next_metadata_state) = params.get_mut(world);

            next_metadata_state.set(MetadataState::Loading);

            channels
                .load_sender
                .send(LoadFile::Metadata(Source::Directory(dir)))
                .unwrap();
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn select_metadata(ui: &mut egui::Ui, world: &mut World) {
    let current_metadata_state = *world.get_resource::<State<MetadataState>>().unwrap().get();

    let button = egui::Button::new("Choose dir...");
    let enabled = match current_metadata_state {
        MetadataState::Selecting | MetadataState::Loading => false,
        MetadataState::NotLoaded | MetadataState::Loaded => true,
    };

    if ui.add_enabled(enabled, button).clicked() {
        let mut next_metadata_state = world
            .get_resource_mut::<NextState<MetadataState>>()
            .unwrap();

        next_metadata_state.set(MetadataState::Selecting);

        let had_metadata = match current_metadata_state {
            MetadataState::NotLoaded => false,
            MetadataState::Selecting => {
                unreachable!("Choosing metadata should be disabled while selecting one");
            }
            MetadataState::Loading => {
                unreachable!("Choosing metadata should be disabled while loading one");
            }
            MetadataState::Loaded => true,
        };

        let (load_sender, load_receiver) = flume::bounded::<MetadataSelection>(1);
        world.insert_non_send_resource(load_receiver);

        wasm_bindgen_futures::spawn_local(async move {
            use wasm_bindgen::JsCast;

            if let Ok(dir) = crate::web::chooseDir().await {
                let dir = dir
                    .dyn_into::<web_sys::FileSystemDirectoryHandle>()
                    .unwrap();

                load_sender
                    .send(MetadataSelection::Load(LoadFile::Metadata(
                        Source::Directory(dir),
                    )))
                    .unwrap();
            } else {
                load_sender
                    .send(MetadataSelection::Canceled { had_metadata })
                    .unwrap();
            }
        });
    }
}
