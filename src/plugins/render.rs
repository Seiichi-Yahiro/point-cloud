use std::ops::Deref;

use crate::plugins::render::line::LineRenderPlugin;
use bevy_app::prelude::*;
use bevy_app::AppExit;
use bevy_ecs::prelude::*;
use bevy_ecs::system::SystemState;
use wgpu::SurfaceError;

use crate::plugins::render::point::PointRenderPlugin;
use crate::plugins::render::ui::UiPlugin;
use crate::plugins::wgpu::{Device, Queue, Surface, SurfaceConfig};
use crate::plugins::winit::{Window, WindowResized};
use crate::texture::Texture;

pub mod line;
pub mod point;
mod ui;
pub mod vertex;

pub struct RenderPlugin;

impl Plugin for RenderPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup)
            .add_systems(
                PreUpdate,
                update_depth_texture.run_if(on_event::<WindowResized>()),
            )
            .configure_sets(
                Last,
                (
                    RenderSet.run_if(resource_exists::<GlobalRenderResources>),
                    (RenderPassSet::Point, RenderPassSet::Line, RenderPassSet::UI)
                        .chain()
                        .in_set(RenderSet),
                ),
            )
            .add_plugins(PointRenderPlugin)
            .add_plugins(LineRenderPlugin)
            .add_plugins(UiPlugin)
            .add_systems(
                Last,
                (
                    prepare.before(RenderSet),
                    present.in_set(RenderSet).after(RenderPassSet::UI),
                ),
            );
    }
}

#[derive(SystemSet, Hash, Eq, PartialEq, Copy, Clone, Debug)]
pub struct RenderSet;

#[derive(SystemSet, Hash, Eq, PartialEq, Copy, Clone, Debug)]
enum RenderPassSet {
    Point,
    Line,
    UI,
}

#[derive(Resource)]
pub struct GlobalDepthTexture(Texture);

impl GlobalDepthTexture {
    pub fn new(texture: Texture) -> Self {
        Self(texture)
    }
}

impl Deref for GlobalDepthTexture {
    type Target = Texture;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
fn setup(mut commands: Commands, device: Res<Device>, config: Res<SurfaceConfig>) {
    let depth_texture = Texture::create_depth_texture(&device, config.width, config.height);
    commands.insert_resource(GlobalDepthTexture::new(depth_texture));
}

fn update_depth_texture(
    mut window_resized: EventReader<WindowResized>,
    mut depth_texture: ResMut<GlobalDepthTexture>,
    device: Res<Device>,
) {
    if let Some(resized) = window_resized.read().last() {
        let texture = Texture::create_depth_texture(
            &device,
            resized.physical_size.width,
            resized.physical_size.height,
        );

        *depth_texture = GlobalDepthTexture::new(texture);
    }
}

#[derive(Resource)]
struct GlobalRenderResources {
    frame: wgpu::SurfaceTexture,
    view: wgpu::TextureView,
    encoder: wgpu::CommandEncoder,
}

fn prepare(
    world: &mut World,
    params: &mut SystemState<(Res<Surface>, Res<Device>, Res<Queue>, Res<Window>)>,
) {
    let (surface, device, queue, window) = params.get(world);

    let frame = match surface.get_current_texture() {
        Ok(frame) => frame,
        Err(err) => {
            queue.submit(None);

            match err {
                SurfaceError::Timeout => {
                    log::warn!("Timeout while trying to acquire next frame!")
                }
                SurfaceError::Outdated => {
                    // happens when window gets minimized
                }
                SurfaceError::Lost => {
                    world
                        .send_event(WindowResized {
                            physical_size: window.inner_size(),
                        })
                        .unwrap();
                }
                SurfaceError::OutOfMemory => {
                    log::error!("Application is out of memory!");
                    world.send_event(AppExit);
                }
            }
            return;
        }
    };

    let view = frame
        .texture
        .create_view(&wgpu::TextureViewDescriptor::default());

    let encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());

    world.insert_resource(GlobalRenderResources {
        frame,
        view,
        encoder,
    });
}

fn present(world: &mut World) {
    let render_resources = world.remove_resource::<GlobalRenderResources>().unwrap();

    world
        .get_resource::<Queue>()
        .unwrap()
        .submit(Some(render_resources.encoder.finish()));

    render_resources.frame.present();
}
