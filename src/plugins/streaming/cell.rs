use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::hash::BuildHasherDefault;
use std::ops::{Deref, DerefMut};

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bytesize::ByteSize;
use caches::{Cache, DefaultEvictCallback, RawLRU};
use egui::ahash::{HashMapExt, HashSetExt};
use flume::{Receiver, Sender, TryRecvError};
use glam::{IVec3, Vec3, Vec4};
use itertools::Itertools;
use rustc_hash::{FxHashMap, FxHashSet, FxHasher};
use thousands::Separable;

use point_converter::cell::CellId;

use crate::plugins::camera::frustum::{Aabb, Corners, Frustum};
use crate::plugins::camera::projection::PerspectiveProjection;
use crate::plugins::camera::{Camera, UpdateFrustum, Visibility};
use crate::plugins::render::point::Point;
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
            .insert_resource(StreamingFrustumsScale::default())
            .insert_resource(LoadedCells::default())
            .insert_resource(MissingCells::default())
            .insert_resource(LoadingCells::default())
            .add_systems(PostStartup, add_streaming_frustums)
            .add_systems(
                Update,
                (
                    (receive_cell, update_streaming_frustums.after(UpdateFrustum)),
                    update_cells,
                    enqueue_cells_to_load,
                )
                    .chain()
                    .run_if(in_state(MetadataState::Loaded))
                    .run_if(in_state(StreamState::Enabled)),
            )
            .add_systems(OnEnter(MetadataState::Loaded), set_view_distance)
            .add_systems(OnExit(MetadataState::Loaded), cleanup_cells)
            .add_systems(
                PostUpdate,
                shader::update_loaded_cells_buffer
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

#[derive(Component)]
pub struct CellData {
    pub id: CellId,
    pub pos: Vec3,
    pub size: f32,
    pub number_of_points: u32,
}

#[derive(Default, Resource)]
struct LoadedCells(FxHashMap<CellId, Entity>);

#[derive(Resource)]
struct MissingCells(RawLRU<CellId, (), DefaultEvictCallback, BuildHasherDefault<FxHasher>>);

impl Default for MissingCells {
    fn default() -> Self {
        Self(RawLRU::with_hasher(10000, BuildHasherDefault::default()).unwrap())
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

#[derive(Resource)]
struct StreamingFrustumsScale(f32);

impl StreamingFrustumsScale {
    const MIN: f32 = 1.0;
    const MAX: f32 = 5.0;
}

impl Default for StreamingFrustumsScale {
    fn default() -> Self {
        Self(2.0)
    }
}

#[derive(Debug)]
pub struct StreamingFrustum {
    pub far_corners: Corners,
    pub far_plane: Vec4,
    pub aabb: Aabb,
}

#[derive(Debug, Component)]
pub struct StreamingFrustums(Vec<StreamingFrustum>);

impl Deref for StreamingFrustums {
    type Target = Vec<StreamingFrustum>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for StreamingFrustums {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
fn cleanup_cells(
    mut commands: Commands,
    cell_query: Query<Entity, With<CellData>>,
    mut loaded_cells: ResMut<LoadedCells>,
    mut missing_cells: ResMut<MissingCells>,
    mut loading_cells: ResMut<LoadingCells>,
) {
    loading_cells.should_load.clear();
    loading_cells.loading.clear();
    loaded_cells.0.clear();
    *missing_cells = MissingCells::default();

    for entity in cell_query.iter() {
        commands.entity(entity).despawn();
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
                            number_of_points: header.number_of_points,
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

fn add_streaming_frustums(mut commands: Commands, camera_query: Query<Entity, With<Camera>>) {
    for entity in camera_query.iter() {
        commands
            .entity(entity)
            .insert(StreamingFrustums(Vec::new()));
    }
}

fn update_streaming_frustums(
    active_metadata: ActiveMetadataRes,
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
    let metadata = &active_metadata.metadata;

    for (transform, projection, frustum, mut streaming_frustums) in camera_query.iter_mut() {
        if !(frustum.is_changed() || streaming_frustums_scale.is_changed()) {
            continue;
        }

        let mut new_projection = projection.clone();

        let near_aabb = {
            let mut near_corners_iter = frustum.near.iter().copied();
            let first_corner = near_corners_iter.next().unwrap();
            near_corners_iter.fold(Aabb::new(first_corner, first_corner), |mut acc, corner| {
                acc.extend(corner);
                acc
            })
        };

        let forward = transform.forward();
        let far_normal = frustum.planes.far.truncate();

        **streaming_frustums = (0..metadata.hierarchies)
            .map(|hierarchy| {
                let cell_size = metadata.cell_size(hierarchy);
                let far_distance = (cell_size * streaming_frustums_scale.0).min(projection.far);
                let center_on_far_plane = transform.translation + far_distance * forward;

                new_projection.far = far_distance;
                let far_corners = Frustum::far_corners(transform, &new_projection);

                let mut aabb = far_corners
                    .iter()
                    .copied()
                    .fold(near_aabb, |mut acc, corner| {
                        acc.extend(corner);
                        acc
                    });

                aabb.clamp(metadata.bounding_box.min, metadata.bounding_box.max);

                StreamingFrustum {
                    far_corners,
                    far_plane: far_normal.extend(center_on_far_plane.dot(far_normal)),
                    aabb,
                }
            })
            .collect();
    }
}

fn update_cells(
    mut commands: Commands,
    camera_query: Query<
        (&StreamingFrustums, &Transform, &Frustum),
        (With<Camera>, Changed<StreamingFrustums>),
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
            let min_cell_index = metadata.cell_index(streaming_frustum.aabb.min, cell_size);
            let max_cell_index = metadata.cell_index(streaming_frustum.aabb.max, cell_size);

            frustum_planes.far = streaming_frustum.far_plane;

            let ids = (min_cell_index.x..=max_cell_index.x)
                .cartesian_product(min_cell_index.y..=max_cell_index.y)
                .cartesian_product(min_cell_index.z..=max_cell_index.z)
                .map(|((x, y), z)| IVec3::new(x, y, z))
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
                if let Some(status) = loaded_cells.0.remove(&cell_to_load.id) {
                    new_loaded.insert(cell_to_load.id, status);
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
        let mut query = world.query::<&CellData>();
        let sum = query.iter(world).map(|it| it.number_of_points).sum::<u32>();
        ui.label(format!("Loaded points: {}", sum.separate_with_commas()));
        ui.label(format!(
            "Loaded size: {}",
            ByteSize(sum as u64 * std::mem::size_of::<Point>() as u64)
        ));
    }
}
