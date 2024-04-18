use std::ops::{Deref, DerefMut};

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_ecs::system::{SystemParam, SystemState};
use bevy_window::{PrimaryWindow, RawHandleWrapper, WindowResized};
use bevy_winit::WinitWindows;
use wgpu::TextureFormat;

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
    pub async fn build(app: &mut App) {
        let mut params: SystemState<(
            NonSend<WinitWindows>,
            Query<(Entity, &RawHandleWrapper), With<PrimaryWindow>>,
        )> = SystemState::new(&mut app.world);

        let (winit_windows, query) = params.get(&app.world);
        let (window_entity, window_wrapper) = query.get_single().unwrap();
        let window = winit_windows.get_window(window_entity).unwrap();

        let size = window.inner_size();
        let instance = wgpu::Instance::default();
        let surface = instance
            .create_surface(unsafe { window_wrapper.get_handle() })
            .unwrap();

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

        app.add_systems(First, resize_window.run_if(on_event::<WindowResized>()));
    }
}

#[derive(SystemParam)]
pub struct WinitWindow<'w, 's> {
    primary_window_query: Query<'w, 's, Entity, With<PrimaryWindow>>,
    winit_windows: NonSend<'w, WinitWindows>,
}

impl<'w, 's> WinitWindow<'w, 's> {
    pub fn get(&self) -> &winit::window::Window {
        let window_entity = self.primary_window_query.get_single().unwrap();
        self.winit_windows.get_window(window_entity).unwrap()
    }
}

fn resize_window(
    surface: Res<Surface>,
    mut config: ResMut<SurfaceConfig>,
    device: Res<Device>,
    winit_window: WinitWindow,
) {
    let size = winit_window.get().inner_size();

    config.width = size.width.max(1);
    config.height = size.height.max(1);
    surface.configure(&device, &config);
}
