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

use crate::plugins::camera::Camera;
use crate::plugins::camera::projection::PerspectiveProjection;
use crate::plugins::render::vertex::{Vertex, VertexBuffer};
use crate::plugins::streaming::loader::{LoadedFile, LoadFile, spawn_loader};
use crate::plugins::wgpu::Device;
use crate::transform::Transform;

mod loader;

pub struct StreamingPlugin;

impl Plugin for StreamingPlugin {
    fn build(&self, app: &mut App) {
        let (load_sender, load_receiver) = flume::unbounded::<LoadFile>();
        let (loaded_sender, loaded_receiver) = flume::unbounded::<LoadedFile>();

        spawn_loader(load_receiver, loaded_sender);

        let channels = Channels {
            source_receiver: None,
            load_sender,
            loaded_receiver,
        };

        cfg_if! {
            if #[cfg(target_arch = "wasm32")] {
                app.insert_non_send_resource(ActiveMetadata::default()).insert_non_send_resource(channels);
            } else {
                app.insert_resource(ActiveMetadata::default()).insert_resource(channels);
            }
        }

        app.insert_resource(Cells::default()).add_systems(
            Update,
            (
                trigger_metadata_loading,
                (update_cells, receive_files, trigger_cell_loading).chain(),
            ),
        );
    }
}

#[derive(Default)]
#[cfg_attr(not(target_arch = "wasm32"), derive(Resource))]
pub struct ActiveMetadata {
    metadata: Option<Metadata>,
    source: Option<Source>,
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
        self.metadata.as_ref()
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
pub struct Channels {
    source_receiver: Option<flume::Receiver<Source>>,
    load_sender: flume::Sender<LoadFile>,
    loaded_receiver: flume::Receiver<LoadedFile>,
}

#[cfg(not(target_arch = "wasm32"))]
pub type ChannelsRes<'w> = Res<'w, Channels>;

#[cfg(target_arch = "wasm32")]
pub type ChannelsRes<'w> = NonSend<'w, Channels>;

#[cfg(not(target_arch = "wasm32"))]
pub type ChannelsResMut<'w> = ResMut<'w, Channels>;

#[cfg(target_arch = "wasm32")]
pub type ChannelsResMut<'w> = NonSendMut<'w, Channels>;

impl Drop for Channels {
    fn drop(&mut self) {
        self.load_sender.send(LoadFile::Stop).unwrap();
    }
}

impl Channels {
    pub fn set_directory_receiver(&mut self, receiver: flume::Receiver<Source>) {
        self.source_receiver = Some(receiver);
    }
}

fn trigger_metadata_loading(
    mut commands: Commands,
    mut active_metadata: ActiveMetadataResMut,
    mut channels: ChannelsResMut,
    mut cells: ResMut<Cells>,
) {
    if let Some(receiver) = channels.source_receiver.take() {
        match receiver.try_recv() {
            Ok(source) => {
                active_metadata.source = Some(source.clone());

                channels
                    .load_sender
                    .send(LoadFile::Metadata(source))
                    .unwrap();

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
            Err(TryRecvError::Disconnected) => {
                // should only happen in wasm build
                // meaning the dir selection was canceled
            }
            Err(TryRecvError::Empty) => {
                channels.source_receiver = Some(receiver);
            }
        }
    }
}

fn receive_files(
    mut commands: Commands,
    channels: ChannelsRes,
    mut active_metadata: ActiveMetadataResMut,
    device: Res<Device>,
    mut cells: ResMut<Cells>,
) {
    match channels.loaded_receiver.try_recv() {
        Ok(LoadedFile::Metadata(metadata)) => {
            active_metadata.metadata = Some(metadata);
        }
        Ok(LoadedFile::Cell { id, cell }) => {
            cells.loading = None;

            if !cells.should_load.remove(&id) {
                // cell was no longer needed after it finished loading
                return;
            }

            if let Some(cell) = cell {
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
            } else {
                // TODO handle error cells
                cells.loaded.insert(id, LoadedCellStatus::Missing);
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
        if let Some(metadata) = active_metadata.metadata() {
            cells.loading = Some(id);

            channels
                .load_sender
                .send(LoadFile::Cell {
                    id,
                    sub_grid_dimension: metadata.sub_grid_dimension,
                    source: active_metadata
                        .source
                        .clone()
                        .expect("Source should always exist when metadata exists"),
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

pub fn draw_ui(ui: &mut egui::Ui, world: &mut World) {
    {
        let cells = world.get_resource::<Cells>().unwrap();

        ui.label(format!("Loaded cells: {}", cells.loaded.len()));
        ui.label(format!("Cells to load: {}", cells.should_load.len()));
        ui.label(format!("Is loading: {}", cells.loading.is_some()));
    }

    #[cfg(not(target_arch = "wasm32"))]
    if ui.button("Choose metadata...").clicked() {
        let (sender, receiver) = flume::bounded(1);
        world
            .get_resource_mut::<Channels>()
            .unwrap()
            .set_directory_receiver(receiver);

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
            sender.send(Source::Directory(dir)).unwrap();
        }
    }

    #[cfg(target_arch = "wasm32")]
    if ui.button("Choose dir...").clicked() {
        let (sender, receiver) = flume::bounded(1);
        world
            .get_non_send_resource_mut::<Channels>()
            .unwrap()
            .set_directory_receiver(receiver);

        wasm_bindgen_futures::spawn_local(async move {
            use wasm_bindgen::JsCast;

            if let Ok(dir) = crate::web::chooseDir().await {
                let dir = dir
                    .dyn_into::<web_sys::FileSystemDirectoryHandle>()
                    .unwrap();

                sender.send(Source::Directory(dir)).unwrap();
            }
        });
    }
}
