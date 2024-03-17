use std::sync::Arc;

use cfg_if::cfg_if;
use winit::dpi::PhysicalSize;
use winit::event_loop::EventLoop;
use winit::window::{Window, WindowBuilder};

pub struct ViewportDescriptor {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
}

impl ViewportDescriptor {
    pub fn new<T>(event_loop: &EventLoop<T>, instance: &wgpu::Instance) -> Self {
        let mut window_builder = WindowBuilder::new().with_title("Point Cloud");

        cfg_if! {
            if #[cfg(target_arch = "wasm32")] {
                use wasm_bindgen::JsCast;
                use winit::platform::web::WindowBuilderExtWebSys;

                let canvas = web_sys::window()
                    .unwrap()
                    .document()
                    .unwrap()
                    .get_element_by_id("point-cloud-canvas")
                    .unwrap()
                    .dyn_into::<web_sys::HtmlCanvasElement>()
                    .unwrap();

                window_builder = window_builder.with_canvas(Some(canvas));
            } else {
                window_builder = window_builder.with_inner_size(PhysicalSize::new(800, 600));
            }
        }

        let window = window_builder.build(event_loop).unwrap();

        let window = Arc::new(window);
        let surface = instance.create_surface(Arc::clone(&window)).unwrap();

        Self { window, surface }
    }

    pub fn surface(&self) -> &wgpu::Surface {
        &self.surface
    }

    pub fn build(self, adapter: &wgpu::Adapter, device: &wgpu::Device) -> Viewport {
        let size = self.window.inner_size();

        let config = self
            .surface
            .get_default_config(adapter, size.width, size.height)
            .unwrap();
        self.surface.configure(device, &config);

        Viewport {
            window: self.window,
            surface: self.surface,
            config,
        }
    }
}

pub struct Viewport {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
}

impl Viewport {
    pub fn window(&self) -> Arc<Window> {
        Arc::clone(&self.window)
    }

    pub fn surface(&self) -> &wgpu::Surface {
        &self.surface
    }

    pub fn config(&self) -> &wgpu::SurfaceConfiguration {
        &self.config
    }

    pub fn resize(&mut self, device: &wgpu::Device, physical_size: PhysicalSize<u32>) {
        self.config.width = physical_size.width.max(1);
        self.config.height = physical_size.height.max(1);
        self.surface.configure(device, &self.config);
        self.window.request_redraw();
    }
}
