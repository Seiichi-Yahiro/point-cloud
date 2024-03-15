use cfg_if::cfg_if;
use winit::dpi::PhysicalSize;
use winit::event::{Event, WindowEvent};
use winit::event_loop::{EventLoop, EventLoopBuilder, EventLoopWindowTarget};

use crate::gpu::GPU;
use crate::viewport::{Viewport, ViewportDescriptor};

type UserEvent = ();

pub struct App {
    viewport: Viewport,
    gpu: GPU,
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

        let mut app = App { viewport, gpu };

        let event_handler =
            move |event: Event<UserEvent>, target: &EventLoopWindowTarget<UserEvent>| match event {
                Event::WindowEvent {
                    event: window_event,
                    ..
                } => match window_event {
                    WindowEvent::RedrawRequested => {
                        app.render();
                    }
                    WindowEvent::Resized(new_size) => {
                        app.resize(new_size);
                    }
                    WindowEvent::CloseRequested => {
                        target.exit();
                    }
                    _ => {}
                },
                _ => {}
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
    }

    fn render(&self) {
        let frame = self
            .viewport
            .surface()
            .get_current_texture()
            .expect("Failed to acquire next swap chain texture");

        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .gpu
            .device()
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());

        let render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
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
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        drop(render_pass);

        self.gpu.queue().submit(Some(encoder.finish()));
        frame.present();
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
