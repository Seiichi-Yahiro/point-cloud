use std::sync::Arc;

use cfg_if::cfg_if;

use crate::plugins::camera::CameraPlugin;
use crate::plugins::fps::FPSPlugin;
use crate::plugins::input::InputPlugin;
use crate::plugins::render::RenderPlugin;
use crate::plugins::ui::UiPlugin;
use crate::plugins::wgpu::WGPUPlugin;
use crate::plugins::winit::{Window, WinitPlugin};

/*#[derive(Debug)]
enum UserEvent {
    ChangeCellStreamer(Box<dyn CellStreamer>),
}*/

pub struct App;

impl App {
    pub async fn run() {
        setup_logger();

        let mut app = bevy_app::App::new();
        app.add_plugins(WinitPlugin);

        WGPUPlugin::build(
            Arc::clone(app.world.get_resource::<Window>().unwrap()),
            &mut app,
        )
        .await;

        app.add_plugins((InputPlugin, CameraPlugin, RenderPlugin, FPSPlugin, UiPlugin))
            .run();

        /*let event_loop: EventLoop<UserEvent> = EventLoopBuilder::with_user_event().build().unwrap();
        let event_loop_proxy = event_loop.create_proxy();

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
        let ui = EguiRenderer::new(gpu.device(), viewport.config().format, &viewport.window());

        let mut app = App {
            viewport,
            gpu,
            cell_streamer: Box::new(EmptyCellStreamer),
            point_renderer,
            ui,
            input_data: InputData::default(),
            fps: FPS::new(),
            event_loop_proxy,
        };

        let event_handler =
            move |event: Event<UserEvent>, target: &EventLoopWindowTarget<UserEvent>| {
                app.input_data.process_event(&event);

                match event {
                    Event::WindowEvent {
                        event: window_event,
                        ..
                    } => {
                        app.window_event(window_event, target);
                    }
                    Event::UserEvent(user_event) => match user_event {
                        UserEvent::ChangeCellStreamer(cell_streamer) => {
                            app.change_cell_streamer(cell_streamer);
                        }
                    },
                    _ => {}
                }
            };

        cfg_if! {
            if #[cfg(target_arch = "wasm32")] {
                use winit::platform::web::EventLoopExtWebSys;
                event_loop.spawn(event_handler);
            } else {
                event_loop.run(event_handler).unwrap();
            }
        }*/
    }
}

/*    fn window_event(
        &mut self,
        window_event: WindowEvent,
        target: &EventLoopWindowTarget<UserEvent>,
    ) {
        let ui_event_response = self.ui.handle_input(&self.viewport.window(), &window_event);

        if ui_event_response.repaint {
            self.viewport.window().request_redraw();
        }

        if ui_event_response.consumed {
            return;
        }

        match window_event {
            WindowEvent::RedrawRequested => {
                self.update();

                if let Err(err) = self.draw() {
                    match err {
                        wgpu::SurfaceError::Timeout => {
                            log::warn!("Timeout while trying to acquire next frame!")
                        }
                        wgpu::SurfaceError::Outdated => {
                            // happens when window gets minimized
                        }
                        wgpu::SurfaceError::Lost => {
                            self.resize(self.viewport.window().inner_size());
                        }
                        wgpu::SurfaceError::OutOfMemory => {
                            log::error!("Application is out of memory!");
                            target.exit();
                        }
                    }
                }

                self.input_data.clear_events();
                self.viewport.window().request_redraw();
            }
            WindowEvent::Resized(new_size) => {
                self.resize(new_size);
            }
            WindowEvent::CloseRequested => {
                target.exit();
            }
            #[cfg(not(target_arch = "wasm32"))]
            WindowEvent::DroppedFile(path) => {
                if let Some(dir) = path.parent() {
                    let cell_streamer = Box::new(LocalCellStreamer::new(dir.to_path_buf()));
                    self.event_loop_proxy
                        .send_event(UserEvent::ChangeCellStreamer(cell_streamer))
                        .unwrap();
                }
            }
            _ => {}
        }
    }

    fn change_cell_streamer(&mut self, cell_streamer: Box<dyn CellStreamer>) {
        self.cell_streamer = cell_streamer;
        self.cell_streamer.load_metadata();
    }

    fn resize(&mut self, physical_size: PhysicalSize<u32>) {
        self.viewport.resize(self.gpu.device(), physical_size);
        self.point_renderer
            .resize(self.gpu.device(), self.viewport.config());
    }

    fn update(&mut self) {
        self.fps.update();
        self.cell_streamer.update(self.point_renderer.camera());
        self.point_renderer
            .update(self.gpu.queue(), &self.input_data);
    }

    fn draw(&mut self) -> Result<(), wgpu::SurfaceError> {
        let frame = self.viewport.surface().get_current_texture()?;

        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .gpu
            .device()
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());

        self.point_renderer.draw(&view, &mut encoder);
        self.draw_ui(&view, &mut encoder);

        self.gpu.queue().submit(Some(encoder.finish()));
        frame.present();

        Ok(())
    }

    fn draw_ui(&mut self, render_target: &wgpu::TextureView, encoder: &mut wgpu::CommandEncoder) {
        self.ui.draw(
            &self.gpu,
            encoder,
            &self.viewport.window(),
            render_target,
            ScreenDescriptor {
                size_in_pixels: [self.viewport.config().width, self.viewport.config().height],
                pixels_per_point: self.ui.context.pixels_per_point(),
            },
            |context| {
                egui::Window::new("UI")
                    .resizable(false)
                    .show(context, |ui| {
                        ui.label(self.fps.to_string());

                        #[cfg(not(target_arch = "wasm32"))]
                        if ui.button("Choose metadata...").clicked() {
                            let dir = rfd::FileDialog::new()
                                .add_filter("metadata", &["json"])
                                .set_parent(&self.viewport.window())
                                .pick_file()
                                .and_then(|it| it.parent().map(std::path::Path::to_path_buf));

                            if let Some(dir) = dir {
                                self.event_loop_proxy
                                    .send_event(UserEvent::ChangeCellStreamer(Box::new(
                                        LocalCellStreamer::new(dir.to_path_buf()),
                                    )))
                                    .unwrap();
                            }
                        }

                        #[cfg(target_arch = "wasm32")]
                        if ui.button("Choose dir...").clicked() {
                            let event_loop_proxy = self.event_loop_proxy.clone();

                            wasm_bindgen_futures::spawn_local(async move {
                                use wasm_bindgen::JsCast;

                                if let Ok(dir) = crate::web::chooseDir().await {
                                    let dir = dir
                                        .dyn_into::<web_sys::FileSystemDirectoryHandle>()
                                        .unwrap();

                                    event_loop_proxy
                                        .send_event(UserEvent::ChangeCellStreamer(Box::new(
                                            LocalCellStreamer::new(dir),
                                        )))
                                        .unwrap();
                                }
                            });
                        }
                    });
            },
        );
    }
}*/

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
