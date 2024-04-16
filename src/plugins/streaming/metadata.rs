use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use flume::{Receiver, Sender, TryRecvError};
use glam::Vec3;
use thousands::Separable;

use point_converter::metadata::Metadata;

use crate::plugins::camera::Camera;
use crate::plugins::streaming::metadata::loader::{spawn_metadata_loader, LoadedMetadataMsg};
use crate::plugins::streaming::Source;
use crate::transform::Transform;

mod loader;
pub mod shader;

pub struct MetadataPlugin;

impl Plugin for MetadataPlugin {
    fn build(&self, app: &mut App) {
        let (loaded_sender, loaded_receiver) = flume::unbounded::<LoadedMetadataMsg>();

        let channels = Channels {
            loaded_sender,
            loaded_receiver,
        };

        #[cfg(not(target_arch = "wasm32"))]
        app.insert_resource(channels);

        #[cfg(target_arch = "wasm32")]
        {
            app.insert_non_send_resource(channels)
                .insert_non_send_resource(ActiveMetadata::default())
                .add_systems(
                    Update,
                    handle_selection.run_if(in_state(MetadataState::Selecting)),
                );
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            app.insert_resource(ActiveMetadata::default());
        }

        shader::setup(&mut app.world);

        app.insert_state(MetadataState::NotLoaded)
            .add_systems(
                Update,
                receive_metadata.run_if(in_state(MetadataState::Loading)),
            )
            .add_systems(
                OnEnter(MetadataState::Loaded),
                (look_at_bounding_box, shader::write_buffer),
            );
    }
}

#[cfg_attr(not(target_arch = "wasm32"), derive(Resource))]
struct Channels {
    loaded_sender: Sender<LoadedMetadataMsg>,
    loaded_receiver: Receiver<LoadedMetadataMsg>,
}

#[cfg(not(target_arch = "wasm32"))]
type ChannelsRes<'w> = Res<'w, Channels>;

#[cfg(target_arch = "wasm32")]
type ChannelsRes<'w> = NonSend<'w, Channels>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, States)]
pub enum MetadataState {
    NotLoaded,
    #[cfg(target_arch = "wasm32")]
    Selecting,
    Loading,
    Loaded,
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

fn receive_metadata(
    channels: ChannelsRes,
    mut active_metadata: ActiveMetadataResMut,
    mut next_metadata_state: ResMut<NextState<MetadataState>>,
) {
    match channels.loaded_receiver.try_recv() {
        Ok(LoadedMetadataMsg { source, metadata }) => match metadata {
            Ok(metadata) => {
                log::debug!(
                    "Loaded metadata for {} with {} points",
                    metadata.name,
                    metadata.number_of_points
                );

                next_metadata_state.set(MetadataState::Loaded);
                *active_metadata = ActiveMetadata { metadata, source };
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
    Load(Source),
    Canceled { had_metadata: bool },
}

#[cfg(target_arch = "wasm32")]
fn handle_selection(world: &mut World) {
    let receiver = world
        .remove_non_send_resource::<Receiver<MetadataSelection>>()
        .unwrap();

    match receiver.try_recv() {
        Ok(MetadataSelection::Load(source)) => {
            world
                .get_resource_mut::<NextState<MetadataState>>()
                .unwrap()
                .set(MetadataState::Loading);

            let channels = world.get_non_send_resource::<Channels>().unwrap();
            spawn_metadata_loader(source, channels.loaded_sender.clone());
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
    #[cfg(not(target_arch = "wasm32"))]
    let metadata = &world.get_resource::<ActiveMetadata>().unwrap().metadata;

    #[cfg(target_arch = "wasm32")]
    let metadata = &world
        .get_non_send_resource::<ActiveMetadata>()
        .unwrap()
        .metadata;

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
            spawn_metadata_loader(Source::Directory(dir), channels.loaded_sender.clone());
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
                    .send(MetadataSelection::Load(Source::Directory(dir)))
                    .unwrap();
            } else {
                load_sender
                    .send(MetadataSelection::Canceled { had_metadata })
                    .unwrap();
            }
        });
    }
}
