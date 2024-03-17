use cfg_if::cfg_if;
use wgpu::SurfaceError;
use winit::dpi::PhysicalSize;
use winit::event::{Event, WindowEvent};
use winit::event_loop::{EventLoop, EventLoopBuilder, EventLoopWindowTarget};

use crate::gpu::GPU;
use crate::input_data::InputData;
use crate::point_renderer::PointRenderer;
use crate::viewport::{Viewport, ViewportDescriptor};

type UserEvent = ();

pub struct App {
    viewport: Viewport,
    gpu: GPU,
    point_renderer: PointRenderer,
    input_data: InputData,
}

impl App {
    pub async fn run() {
        setup_logger();

        let event_loop: EventLoop<UserEvent> = EventLoopBuilder::with_user_event().build().unwrap();
        let instance = wgpu::Instance::default();

        let viewport_desc = ViewportDescriptor::new(&event_loop, &instance);

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(viewport_desc.surface()),
                ..Default::default()
            })
            .await
            .expect("Failed to find an appropriate adapter");

        let gpu = GPU::new(&adapter).await;
        let viewport = viewport_desc.build(&adapter, gpu.device());
        let point_renderer = PointRenderer::new(gpu.device(), viewport.config());

        let mut app = App {
            viewport,
            gpu,
            point_renderer,
            input_data: InputData::default(),
        };

        let event_handler =
            move |event: Event<UserEvent>, target: &EventLoopWindowTarget<UserEvent>| {
                app.input_data.process_event(&event);

                if let Event::WindowEvent {
                    event: window_event,
                    ..
                } = event
                {
                    match window_event {
                        WindowEvent::RedrawRequested => {
                            app.update();

                            if let Err(err) = app.draw() {
                                match err {
                                    SurfaceError::Timeout => {
                                        log::warn!("Timeout while trying to acquire next frame!")
                                    }
                                    SurfaceError::Outdated => {
                                        // happens when window gets minimized
                                    }
                                    SurfaceError::Lost => {
                                        app.resize(app.viewport.window().inner_size());
                                    }
                                    SurfaceError::OutOfMemory => {
                                        log::error!("Application is out of memory!");
                                        target.exit();
                                    }
                                }
                            }

                            app.input_data.clear_events();
                            app.viewport.window().request_redraw();
                        }
                        WindowEvent::Resized(new_size) => {
                            app.resize(new_size);
                        }
                        WindowEvent::CloseRequested => {
                            target.exit();
                        }
                        _ => {}
                    }
                }
            };

        cfg_if! {
            if #[cfg(target_arch = "wasm32")] {
                use winit::platform::web::EventLoopExtWebSys;
                event_loop.spawn(event_handler);
            } else {
                event_loop.run(event_handler).unwrap();
            }
        }
    }

    fn resize(&mut self, physical_size: PhysicalSize<u32>) {
        self.viewport.resize(self.gpu.device(), physical_size);
        self.point_renderer
            .resize(self.gpu.device(), self.viewport.config());
    }

    fn update(&mut self) {
        self.point_renderer
            .update(self.gpu.queue(), &self.input_data);
    }

    fn draw(&self) -> Result<(), wgpu::SurfaceError> {
        let frame = self.viewport.surface().get_current_texture()?;

        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .gpu
            .device()
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());

        self.point_renderer.draw(&view, &mut encoder);

        self.gpu.queue().submit(Some(encoder.finish()));
        frame.present();

        Ok(())
    }
}

fn setup_logger() {
    cfg_if! {
        if #[cfg(target_arch = "wasm32")] {
            std::panic::set_hook(Box::new(console_error_panic_hook::hook));
            console_log::init_with_level(log::Level::Debug).expect("Couldn't initialize logger");
        } else {
            env_logger::init();
        }
    }
}
