use std::ops::{Deref, DerefMut};
use std::sync::Arc;

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use wgpu::TextureFormat;

use crate::plugins::winit::Window;

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
            present_mode: surface_caps.present_modes[0],
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: Vec::new(),
            desired_maximum_frame_latency: 2,
        };

        surface.configure(&device, &config);

        app.insert_resource(Device(device));
        app.insert_resource(Queue(queue));
        app.insert_resource(Surface(surface));
        app.insert_resource(SurfaceConfig(config));

        app.add_systems(PreUpdate, resize_window);
    }
}

fn resize_window(
    mut window_resized_events: EventReader<crate::plugins::winit::WindowResized>,
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
