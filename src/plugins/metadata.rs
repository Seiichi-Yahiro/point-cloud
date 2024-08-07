use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_ecs::system::{RunSystemOnce, SystemParam, SystemState};
use bevy_state::prelude::*;
use bounding_volume::Aabb;
use glam::Vec3;
use std::io::Read;
use std::path::PathBuf;
use thousands::Separable;
use url::Url;

use point_converter::metadata::Metadata;

use crate::event_set::prelude::*;
use crate::plugins::asset::source::{Directory, Source, SourceError};
use crate::plugins::asset::{
    Asset, AssetEvent, AssetHandle, AssetLoadedEvent, AssetManagerRes, AssetManagerResMut,
    AssetPlugin, LoadAssetMsg, MutAsset,
};
use crate::plugins::camera::Camera;
use crate::plugins::render::BufferSet;
use crate::transform::Transform;

pub mod shader;

impl Asset for Metadata {
    type Id = String;

    fn read_from(reader: &mut dyn Read) -> Result<Self, SourceError> {
        let result = Metadata::read_from(reader);
        let kind = std::io::ErrorKind::InvalidData;

        result.map_err(|err| SourceError::Other {
            message: err.to_string(),
            #[cfg(not(target_arch = "wasm32"))]
            name: kind,
            #[cfg(target_arch = "wasm32")]
            name: kind.to_string(),
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn save(&self, source: Source) -> Result<(), SourceError> {
        use std::fs::File;
        use std::io::{BufWriter, ErrorKind, Write};

        match source {
            Source::Path(path) => {
                log::debug!("Saving metadata at {:?}", path);

                let file = File::create(path)?;
                let mut buf_writer = BufWriter::new(file);
                self.write_to(&mut buf_writer)
                    .map_err(|err| SourceError::Other {
                        message: err.to_string(),
                        name: ErrorKind::InvalidData,
                    })?;
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

pub struct MetadataPlugin {
    pub url: Option<Url>,
}

impl Plugin for MetadataPlugin {
    fn build(&self, app: &mut App) {
        #[cfg(target_arch = "wasm32")]
        {
            app.add_systems(
                Update,
                handle_selection.run_if(in_state(MetadataState::Selecting)),
            );
        }

        app.add_plugins(AssetPlugin::<Metadata>::default())
            .insert_state(MetadataState::NotLoaded)
            .insert_resource(DefaultURL(self.url.clone()))
            .add_event::<UpdateMetadataEvent>()
            .add_event_set::<UpdatedMetadataEventSet>()
            .add_systems(PreStartup, setup)
            .add_systems(Startup, shader::create_metadata_buffer.in_set(BufferSet))
            .add_systems(
                Update,
                receive_metadata.run_if(on_event::<AssetEvent<Metadata>>()),
            )
            .add_systems(
                PostUpdate,
                (
                    update_metadata.run_if(on_event::<UpdateMetadataEvent>()),
                    shader::update_metadata_buffer
                        .run_if(on_event::<UpdatedMetadataHierarchiesEvent>()),
                )
                    .chain()
                    .in_set(BufferSet),
            )
            .add_systems(
                OnEnter(MetadataState::Loaded),
                (
                    look_at_bounding_box,
                    shader::update_metadata_buffer.in_set(BufferSet),
                ),
            );
    }
}

#[derive(Debug, Resource)]
struct DefaultURL(Option<Url>);

#[derive(Debug, Copy, Clone, Event)]
pub struct UpdatedMetadataHierarchiesEvent;

#[derive(Debug, Copy, Clone, Event)]
pub struct UpdatedMetadataBoundingBoxEvent;

event_set!(pub UpdatedMetadataEventSet {
    UpdatedMetadataHierarchiesEvent,
    UpdatedMetadataBoundingBoxEvent
});

#[derive(Debug, Copy, Clone, Event)]
pub enum UpdateMetadataEvent {
    NumberOfPoints(i32),
    IncreaseHierarchy(u32),
    ExtendBoundingBox(Aabb),
}

fn update_metadata(
    mut events: EventReader<UpdateMetadataEvent>,
    mut active_metadata: ActiveMetadataMut,
    mut updated_metadata_event_set: UpdatedMetadataEventSet,
) {
    let mut metadata = active_metadata.get_mut();

    for event in events.read() {
        match event {
            UpdateMetadataEvent::NumberOfPoints(points) => {
                metadata.number_of_points = metadata
                    .number_of_points
                    .wrapping_add_signed(*points as i64);
            }
            UpdateMetadataEvent::IncreaseHierarchy(hierarchy) => {
                if metadata.hierarchies <= *hierarchy {
                    metadata.hierarchies = *hierarchy + 1;
                    updated_metadata_event_set.dispatch(UpdatedMetadataHierarchiesEvent);
                }
            }
            UpdateMetadataEvent::ExtendBoundingBox(aabb) => {
                if metadata.number_of_points == 0 {
                    metadata.bounding_box = *aabb;
                } else {
                    metadata.bounding_box.extend_aabb(aabb);
                }

                updated_metadata_event_set.dispatch(UpdatedMetadataBoundingBoxEvent);
            }
        }
    }
}

fn setup(
    mut commands: Commands,
    mut metadata_manager: AssetManagerResMut<Metadata>,
    mut next_metadata_state: ResMut<NextState<MetadataState>>,
    default_url: Res<DefaultURL>,
) {
    let handle = metadata_manager.insert(
        "Unknown".to_string(),
        Metadata::default(),
        Source::None,
        false,
    );
    commands.insert_resource(LoadedMetadata { active: handle });

    if let Some(url) = &default_url.0 {
        metadata_manager
            .load_sender()
            .send(LoadAssetMsg {
                id: "Unknown URL".to_string(),
                source: Source::URL(url.clone()),
                reply_sender: None,
            })
            .unwrap();

        next_metadata_state.set(MetadataState::Loading);
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

#[derive(Debug, Resource)]
pub struct LoadedMetadata {
    active: AssetHandle<Metadata>,
}

impl LoadedMetadata {
    pub fn get_active(&self) -> &AssetHandle<Metadata> {
        &self.active
    }
    pub fn set_active(&mut self, handle: AssetHandle<Metadata>) {
        self.active = handle;
    }
}

#[derive(SystemParam)]
pub struct ActiveMetadata<'w> {
    loaded_metadata: Res<'w, LoadedMetadata>,
    metadata_manager: AssetManagerRes<'w, Metadata>,
}

#[derive(SystemParam)]
struct ActiveMetadataMut<'w> {
    loaded_metadata: Res<'w, LoadedMetadata>,
    metadata_manager: AssetManagerResMut<'w, Metadata>,
}

fn get_working_directory(source: &Source) -> Option<Directory> {
    match source {
        #[cfg(not(target_arch = "wasm32"))]
        Source::Path(path) => Some(Directory::Path(path.parent().unwrap().to_path_buf())),
        #[cfg(target_arch = "wasm32")]
        Source::PathInDirectory { directory, .. } => Some(Directory::WebDir(directory.clone())),
        Source::URL(url) => {
            let path = PathBuf::from(url.path());
            let dir = path.parent().unwrap();
            let mut url = url.clone();
            url.set_path(dir.to_str().unwrap());
            Some(Directory::URL(url))
        }
        Source::None => None,
    }
}

impl<'w> ActiveMetadata<'w> {
    pub fn get(&self) -> &Metadata {
        let handle = &self.loaded_metadata.active;
        self.metadata_manager.get_asset(handle)
    }

    pub fn get_working_directory(&self) -> Option<Directory> {
        let handle = &self.loaded_metadata.active;
        let source = self.metadata_manager.get_asset_source(handle);
        get_working_directory(source)
    }
}

impl<'w> ActiveMetadataMut<'w> {
    pub fn get_mut(&mut self) -> MutAsset<Metadata> {
        let handle = &self.loaded_metadata.active;
        self.metadata_manager.get_asset_mut(handle)
    }
}

fn receive_metadata(
    mut loaded_metadata_events: EventReader<AssetEvent<Metadata>>,
    metadata_manager: AssetManagerRes<Metadata>,
    mut loaded_metadata: ResMut<LoadedMetadata>,
    mut next_metadata_state: ResMut<NextState<MetadataState>>,
) {
    for event in loaded_metadata_events.read() {
        match event {
            AssetEvent::Created { handle }
            | AssetEvent::Loaded(AssetLoadedEvent::Success { handle }) => {
                let metadata = metadata_manager.get_asset(handle);

                log::debug!(
                    "Loaded metadata for {} with {} points",
                    metadata.name,
                    metadata.number_of_points
                );

                loaded_metadata.set_active(handle.clone());

                next_metadata_state.set(MetadataState::Loaded);
            }
            AssetEvent::Changed { .. } => {}
            AssetEvent::Loaded(AssetLoadedEvent::Error { id, error }) => {
                log::error!("Failed to load metadata {}: {:?}", id, error);
                next_metadata_state.set(MetadataState::NotLoaded);
            }
        }
    }
}

fn look_at_bounding_box(
    mut query: Query<&mut Transform, With<Camera>>,
    active_metadata: ActiveMetadata,
) {
    let aabb = active_metadata.get().bounding_box;
    let center = (aabb.min + aabb.max) / 2.0;

    let center_max_z = center.with_z(aabb.max.z);

    for mut transform in query.iter_mut() {
        *transform = Transform::from_translation(aabb.max + (center_max_z - aabb.max) / 2.0)
            .looking_at(center, Vec3::Z);
    }
}

#[cfg(target_arch = "wasm32")]
enum MetadataSelection {
    Load(Source),
    Canceled { had_metadata: bool },
}

#[cfg(target_arch = "wasm32")]
fn handle_selection(
    world: &mut World,
    params: &mut SystemState<(AssetManagerRes<Metadata>, ResMut<NextState<MetadataState>>)>,
) {
    let receiver = world
        .remove_non_send_resource::<flume::Receiver<MetadataSelection>>()
        .unwrap();

    match receiver.try_recv() {
        Ok(MetadataSelection::Load(source)) => {
            let (metadata_manager, mut metadata_state) = params.get_mut(world);

            metadata_state.set(MetadataState::Loading);

            let id = format!("{:?}", source);
            log::debug!("{:?}", source); // TODO does this make sense?

            metadata_manager
                .load_sender()
                .send(LoadAssetMsg {
                    id,
                    source,
                    reply_sender: None,
                })
                .unwrap();
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
        Err(flume::TryRecvError::Disconnected) => {
            panic!("Sender disconnected while waiting for metadata selection");
        }
        Err(flume::TryRecvError::Empty) => {
            world.insert_non_send_resource(receiver);
        }
    }
}

pub fn draw_ui(ui: &mut egui::Ui, world: &mut World) {
    let mut params = SystemState::<ActiveMetadata>::new(world);
    let active_metadata = params.get(world);
    let metadata = active_metadata.get();

    ui.label(format!("Cloud name: {}", metadata.name));
    ui.label(format!(
        "Total points: {}",
        metadata.number_of_points.separate_with_commas()
    ));
    ui.label(format!("Hierarchies: {}", metadata.hierarchies));

    ui.collapsing("Extends", |ui| {
        let extends = metadata.bounding_box.max - metadata.bounding_box.min;

        ui.label(format!("x: {}", extends.x));
        ui.label(format!("y: {}", extends.y));
        ui.label(format!("z: {}", extends.z));
    });

    if ui.button("Look at bounding box").clicked() {
        world.run_system_once(look_at_bounding_box);
    }

    select_metadata(ui, world);
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
        let path = {
            let window: &winit::window::Window = world
                .get_resource::<crate::plugins::winit::Window>()
                .unwrap();

            rfd::FileDialog::new()
                .add_filter(Metadata::FILE_NAME, &[Metadata::EXTENSION])
                .set_parent(window)
                .pick_file()
        };

        if let Some(path) = path {
            let mut params = SystemState::<(
                AssetManagerRes<Metadata>,
                ResMut<NextState<MetadataState>>,
            )>::new(world);
            let (metadata_manager, mut next_metadata_state) = params.get_mut(world);

            next_metadata_state.set(MetadataState::Loading);

            // TODO reuse already loaded metadata

            let id = path.to_str().unwrap().to_string();
            let source = Source::Path(path);

            metadata_manager
                .load_sender()
                .send(LoadAssetMsg {
                    id,
                    source,
                    reply_sender: None,
                })
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
            if let Ok(dir) = crate::web::WebDir::choose().await {
                let source = Source::PathInDirectory {
                    directory: dir,
                    path: std::path::PathBuf::from(Metadata::FILE_NAME)
                        .with_extension(Metadata::EXTENSION),
                };

                load_sender.send(MetadataSelection::Load(source)).unwrap();
            } else {
                load_sender
                    .send(MetadataSelection::Canceled { had_metadata })
                    .unwrap();
            }
        });
    }
}
