use std::ops::{Deref, DerefMut};
use std::sync::Arc;

use crate::plugins::render::BufferSet;
use crate::plugins::winit::{Window, WindowResized};
use crate::texture::Texture;
use bevy_app::prelude::*;
use bevy_app::{AppExit, MainScheduleOrder};
use bevy_ecs::prelude::*;
use bevy_ecs::schedule::ScheduleLabel;
use bevy_ecs::system::SystemParam;
use egui::ahash::HashMapExt;
use rustc_hash::FxHashMap;
use wgpu::{SurfaceError, TextureFormat};

#[derive(Resource)]
pub struct Device(wgpu::Device);

impl Deref for Device {
    type Target = wgpu::Device;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Resource)]
pub struct Queue(wgpu::Queue);

impl Deref for Queue {
    type Target = wgpu::Queue;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Resource)]
pub struct Surface(wgpu::Surface<'static>);

impl Deref for Surface {
    type Target = wgpu::Surface<'static>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Resource)]
pub struct SurfaceConfig(wgpu::SurfaceConfiguration);

impl Deref for SurfaceConfig {
    type Target = wgpu::SurfaceConfiguration;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for SurfaceConfig {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

pub struct WGPUPlugin;

impl WGPUPlugin {
    pub async fn build(window: Arc<winit::window::Window>, app: &mut App) {
        let size = window.inner_size();
        let instance = wgpu::Instance::default();
        let surface = instance.create_surface(window).unwrap();

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await
            .expect("Failed to find an appropriate adapter");

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: None,
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                },
                None,
            )
            .await
            .expect("Failed to create device");

        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .copied()
            .find(|format| *format == TextureFormat::Bgra8Unorm)
            .unwrap_or(surface_caps.formats[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width,
            height: size.height,
            present_mode: wgpu::PresentMode::AutoNoVsync,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: Vec::new(),
            desired_maximum_frame_latency: 2,
        };

        surface.configure(&device, &config);

        app.insert_resource(Device(device));
        app.insert_resource(Queue(queue));
        app.insert_resource(Surface(surface));
        app.insert_resource(SurfaceConfig(config));

        let mut encoders = CommandEncoders::new();
        encoders.register::<Self>();
        app.insert_resource(encoders);

        app.add_systems(Startup, setup_depth_texture.in_set(BufferSet))
            .add_systems(
                PreUpdate,
                (resize_window, update_depth_texture.in_set(BufferSet))
                    .chain()
                    .run_if(on_event::<WindowResized>()),
            );

        app.add_schedule(Schedule::new(Render));
        let mut main_schedule_order = app.world.resource_mut::<MainScheduleOrder>();
        main_schedule_order.insert_after(Last, Render);

        app.configure_sets(Render, RenderPassSet.run_if(resource_exists::<RenderView>));

        app.add_systems(
            Render,
            (
                begin_frame.before(RenderPassSet),
                clear_pass.in_set(RenderPassSet),
                end_frame.after(RenderPassSet),
            ),
        );
    }
}

#[derive(Debug, PartialEq, Eq, Copy, Clone, Hash, ScheduleLabel)]
pub struct Render;

#[derive(Debug, PartialEq, Eq, Copy, Clone, Hash, SystemSet)]
pub struct RenderPassSet;

fn resize_window(
    mut window_resized_events: EventReader<WindowResized>,
    surface: Res<Surface>,
    mut config: ResMut<SurfaceConfig>,
    device: Res<Device>,
    window: Res<Window>,
) {
    if let Some(resized) = window_resized_events.read().last() {
        let size = resized.physical_size;
        config.width = size.width.max(1);
        config.height = size.height.max(1);
        surface.configure(&device, &config);
        window.request_redraw();
    }
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
fn setup_depth_texture(mut commands: Commands, device: Res<Device>, config: Res<SurfaceConfig>) {
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

#[derive(Debug, Resource)]
pub struct CommandEncoders {
    order: Vec<&'static str>,
    encoders: FxHashMap<&'static str, wgpu::CommandEncoder>,
}

impl CommandEncoders {
    fn new() -> Self {
        Self {
            order: Vec::new(),
            encoders: FxHashMap::new(),
        }
    }

    pub fn register<T>(&mut self) {
        let id = std::any::type_name::<T>();
        self.order.push(id);
    }

    pub fn encode<T>(&mut self, f: impl FnOnce(&mut wgpu::CommandEncoder)) {
        let id = std::any::type_name::<T>();
        let encoder = self.encoders.get_mut(id).unwrap();
        f(encoder);
    }

    fn prepare(&mut self, device: &wgpu::Device) {
        for id in &self.order {
            self.encoders.insert(
                id,
                device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some(id) }),
            );
        }
    }

    fn finish(&mut self) -> Vec<wgpu::CommandBuffer> {
        let mut buffers = Vec::with_capacity(self.order.len());

        for id in &self.order {
            let encoder = self.encoders.remove(id).unwrap();
            buffers.push(encoder.finish());
        }

        buffers
    }
}

#[derive(Resource)]
pub struct RenderView {
    frame: wgpu::SurfaceTexture,
    pub view: wgpu::TextureView,
}
fn begin_frame(
    mut commands: Commands,
    surface: Res<Surface>,
    device: Res<Device>,
    mut window_resized: EventWriter<WindowResized>,
    mut app_exit: EventWriter<AppExit>,
    window: Res<Window>,
    mut encoders: ResMut<CommandEncoders>,
) {
    let frame = match surface.get_current_texture() {
        Ok(frame) => frame,
        Err(err) => {
            match err {
                SurfaceError::Timeout => {
                    log::warn!("Timeout while trying to acquire next frame!")
                }
                SurfaceError::Outdated => {
                    // happens when window gets minimized
                }
                SurfaceError::Lost => {
                    window_resized.send(WindowResized {
                        physical_size: window.inner_size(),
                    });
                }
                SurfaceError::OutOfMemory => {
                    log::error!("Application is out of memory!");
                    app_exit.send(AppExit);
                }
            }
            return;
        }
    };

    let view = frame
        .texture
        .create_view(&wgpu::TextureViewDescriptor::default());

    commands.insert_resource(RenderView { frame, view });

    encoders.prepare(&device);
}

fn clear_pass(mut global_render_resources: GlobalRenderResources) {
    global_render_resources
        .encoders
        .encode::<WGPUPlugin>(|encoder| {
            encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("clear-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &global_render_resources.render_view.view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.16,
                            g: 0.16,
                            b: 0.16,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &global_render_resources.depth_texture.view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
        });
}

fn end_frame(world: &mut World) {
    if let Some(render_view) = world.remove_resource::<RenderView>() {
        let buffers = world
            .get_resource_mut::<CommandEncoders>()
            .unwrap()
            .finish();

        world.get_resource::<Queue>().unwrap().submit(buffers);

        render_view.frame.present();
    } else {
        world.get_resource::<Queue>().unwrap().submit(None);
    }
}

#[derive(SystemParam)]
pub struct GlobalRenderResources<'w> {
    pub render_view: Res<'w, RenderView>,
    pub depth_texture: Res<'w, GlobalDepthTexture>,
    pub encoders: ResMut<'w, CommandEncoders>,
}
