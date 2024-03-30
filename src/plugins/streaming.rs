use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_ecs::system::{SystemId, SystemState};
use cfg_if::cfg_if;
use egui::ahash::{HashMapExt, HashSetExt};
use flume::TryRecvError;
use glam::{IVec3, Vec3, Vec3Swizzles};
use itertools::Itertools;
use rustc_hash::{FxHashMap, FxHashSet};

use point_converter::cell::CellId;
use point_converter::metadata::Metadata;

use crate::plugins::camera::Camera;
use crate::plugins::camera::projection::PerspectiveProjection;
use crate::plugins::render::vertex::{Vertex, VertexBuffer};
use crate::plugins::streaming::loader::{LoadedFile, LoadFile, spawn_loader};
use crate::plugins::wgpu::Device;
use crate::transform::Transform;

mod loader;

#[derive(Resource)]
struct OneShotSystems {
    look_at_bounding_box: SystemId,
}

pub struct StreamingPlugin;

impl Plugin for StreamingPlugin {
    fn build(&self, app: &mut App) {
        let look_at_bounding_box_system = app.world.register_system(look_at_bounding_box);

        app.world.insert_resource(OneShotSystems {
            look_at_bounding_box: look_at_bounding_box_system,
        });

        let (load_sender, load_receiver) = flume::unbounded::<LoadFile>();
        let (loaded_sender, loaded_receiver) = flume::unbounded::<LoadedFile>();

        spawn_loader(load_receiver, loaded_sender);

        let channels = Channels {
            load_sender,
            loaded_receiver,
        };

        cfg_if! {
            if #[cfg(target_arch = "wasm32")] {
                app.insert_non_send_resource(ActiveMetadata::None).insert_non_send_resource(channels);
            } else {
                app.insert_resource(ActiveMetadata::None).insert_resource(channels);
            }
        }

        app.insert_resource(Cells::default()).add_systems(
            Update,
            (update_cells, receive_files, trigger_cell_loading).chain(),
        );
    }
}

#[cfg_attr(not(target_arch = "wasm32"), derive(Resource))]
pub enum ActiveMetadata {
    Loaded { source: Source, metadata: Metadata },
    None,
}

#[cfg(not(target_arch = "wasm32"))]
pub type ActiveMetadataRes<'w> = Res<'w, ActiveMetadata>;

#[cfg(target_arch = "wasm32")]
pub type ActiveMetadataRes<'w> = NonSend<'w, ActiveMetadata>;

#[cfg(not(target_arch = "wasm32"))]
pub type ActiveMetadataResMut<'w> = ResMut<'w, ActiveMetadata>;

#[cfg(target_arch = "wasm32")]
pub type ActiveMetadataResMut<'w> = NonSendMut<'w, ActiveMetadata>;

impl ActiveMetadata {
    pub fn metadata(&self) -> Option<&Metadata> {
        match self {
            ActiveMetadata::Loaded { metadata, .. } => Some(metadata),
            ActiveMetadata::None => None,
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
type Directory = std::path::PathBuf;

#[cfg(target_arch = "wasm32")]
type Directory = web_sys::FileSystemDirectoryHandle;

#[derive(Debug, Clone)]
pub enum Source {
    Directory(Directory),
    URL,
}

#[cfg_attr(not(target_arch = "wasm32"), derive(Resource))]
struct Channels {
    load_sender: flume::Sender<LoadFile>,
    loaded_receiver: flume::Receiver<LoadedFile>,
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

fn receive_files(
    mut commands: Commands,
    channels: ChannelsRes,
    mut active_metadata: ActiveMetadataResMut,
    device: Res<Device>,
    mut cells: ResMut<Cells>,
    one_shot_systems: Res<OneShotSystems>,
) {
    match channels.loaded_receiver.try_recv() {
        Ok(LoadedFile::Metadata { source, metadata }) => match metadata {
            Ok(metadata) => {
                log::debug!(
                    "Loaded metadata for {} with {} points",
                    metadata.name,
                    metadata.number_of_points
                );

                commands.run_system(one_shot_systems.look_at_bounding_box);

                *active_metadata = ActiveMetadata::Loaded { metadata, source };

                cells.should_load.clear();

                for status in cells.loaded.values() {
                    match status {
                        LoadedCellStatus::Missing => {}
                        LoadedCellStatus::Loaded(entity) => {
                            commands.entity(*entity).despawn();
                        }
                    }
                }

                cells.loaded.clear();
            }
            Err(err) => {
                log::error!("Failed to load metadata: {:?}", err);
            }
        },
        Ok(LoadedFile::Cell { id, cell }) => {
            cells.loading = None;

            if !cells.should_load.remove(&id) {
                log::debug!(
                    "Cell {:?} was no longer needed after it finished loading",
                    id
                );
                return;
            }

            match cell {
                Ok(Some(cell)) => {
                    log::debug!("Loaded cell: {:?}", id);

                    let points = cell
                        .points()
                        .iter()
                        .map(|it| Vertex {
                            position: Vec3::new(it.pos.x, it.pos.z, -it.pos.y),
                            color: it.color,
                        })
                        .collect_vec();

                    let entity = commands.spawn(VertexBuffer::new(&device, &points)).id();
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
        Err(TryRecvError::Empty) => {}
    }
}

fn trigger_cell_loading(
    mut cells: ResMut<Cells>,
    active_metadata: ActiveMetadataRes,
    channels: ChannelsRes,
) {
    if cells.loading.is_some() {
        return;
    }

    if let Some(id) = cells.should_load.iter().copied().next() {
        if let ActiveMetadata::Loaded { metadata, source } = &*active_metadata {
            cells.loading = Some(id);

            channels
                .load_sender
                .send(LoadFile::Cell {
                    id,
                    sub_grid_dimension: metadata.sub_grid_dimension,
                    source: source.clone(),
                })
                .unwrap();
        }
    }
}

#[derive(Debug)]
enum LoadedCellStatus {
    Missing,
    Loaded(Entity),
}

#[derive(Default, Resource)]
struct Cells {
    loaded: FxHashMap<CellId, LoadedCellStatus>,
    should_load: FxHashSet<CellId>,
    loading: Option<CellId>,
}

fn update_cells(
    mut commands: Commands,
    camera_query: Query<(Ref<Transform>, Ref<PerspectiveProjection>), With<Camera>>,
    active_metadata: ActiveMetadataRes,
    mut cells: ResMut<Cells>,
) {
    if let Ok((transform, projection)) = camera_query.get_single() {
        if !(transform.is_changed() || projection.is_changed() || active_metadata.is_changed()) {
            return;
        }

        if let Some(metadata) = active_metadata.metadata() {
            let mut new_loaded = FxHashMap::with_capacity(cells.loaded.capacity());
            let mut new_loading = FxHashSet::with_capacity(cells.should_load.capacity());

            for hierarchy in 0..metadata.hierarchies {
                let far = projection.far / 2u32.pow(hierarchy) as f32;
                let fov_y = projection.fov_y;
                let aspect_ratio = projection.aspect_ratio;

                let half_height_far = far * (fov_y * 0.5).tan();
                let half_width_far = half_height_far * aspect_ratio;

                let far_radius =
                    ((half_width_far * 2.0).powi(2) + (half_height_far * 2.0).powi(2)).sqrt() / 2.0;

                let radius = far_radius.max(far / 2.0) * 1.2;
                let pos = transform.translation + transform.forward() * radius / 2.0;

                let cell_size = metadata.cell_size(hierarchy);
                let min_cell_index = metadata.cell_index(pos - radius, cell_size);
                let max_cell_index = metadata.cell_index(pos + radius, cell_size);

                let ids = (min_cell_index.x..=max_cell_index.x)
                    .cartesian_product(min_cell_index.y..=max_cell_index.y)
                    .cartesian_product(min_cell_index.z..=max_cell_index.z)
                    .map(|((x, y), z)| IVec3::new(x, y, z))
                    .map(move |index| CellId { index, hierarchy });

                // copy or insert cells that need to be loaded
                for id in ids {
                    if let Some(status) = cells.loaded.remove(&id) {
                        new_loaded.insert(id, status);
                    } else {
                        cells.should_load.remove(&id);
                        new_loading.insert(id);
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
            cells.should_load = new_loading;
        }
    }
}

fn look_at_bounding_box(
    mut query: Query<&mut Transform, With<Camera>>,
    active_metadata: ActiveMetadataRes,
) {
    if let Some(metadata) = active_metadata.metadata() {
        let flip_z = Vec3::new(1.0, 1.0, -1.0);
        let min = metadata.bounding_box.min.xzy() * flip_z;
        let max = metadata.bounding_box.max.xzy() * flip_z;
        let center = (min + max) / 2.0;

        let center_max_y = center.with_y(max.y);

        for mut transform in query.iter_mut() {
            *transform = Transform::from_translation(max + (center_max_y - max) / 2.0)
                .looking_at(center, Vec3::Y);
        }
    }
}

pub fn draw_ui(ui: &mut egui::Ui, world: &mut World) {
    {
        let cells = world.get_resource::<Cells>().unwrap();

        ui.label(format!("Loaded cells: {}", cells.loaded.len()));
        ui.label(format!("Cells to load: {}", cells.should_load.len()));
        ui.label(format!("Is loading: {}", cells.loading.is_some()));
    }

    #[cfg(not(target_arch = "wasm32"))]
    if ui.button("Choose metadata...").clicked() {
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
            let mut params = SystemState::<ChannelsRes>::new(world);
            let channels = params.get(world);

            channels
                .load_sender
                .send(LoadFile::Metadata(Source::Directory(dir)))
                .unwrap();
        }
    }

    #[cfg(target_arch = "wasm32")]
    if ui.button("Choose dir...").clicked() {
        let mut params = SystemState::<ChannelsRes>::new(world);
        let channels = params.get(world);

        let load_sender = channels.load_sender.clone();

        wasm_bindgen_futures::spawn_local(async move {
            use wasm_bindgen::JsCast;

            if let Ok(dir) = crate::web::chooseDir().await {
                let dir = dir
                    .dyn_into::<web_sys::FileSystemDirectoryHandle>()
                    .unwrap();

                load_sender
                    .send(LoadFile::Metadata(Source::Directory(dir)))
                    .unwrap();
            }
        });
    }
}
