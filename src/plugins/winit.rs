use std::ops::Deref;
use std::sync::Arc;

use bevy_app::prelude::*;
use bevy_app::{AppExit, PluginsState};
use bevy_ecs::event::ManualEventReader;
use bevy_ecs::prelude::*;
use bevy_ecs::schedule::ScheduleLabel;
use cfg_if::cfg_if;
use web_time::{Duration, Instant};
use winit::dpi::PhysicalSize;
use winit::event::Event;
use winit::event_loop::{ControlFlow, EventLoop, EventLoopBuilder, EventLoopWindowTarget};
use winit::window::WindowBuilder;

type UserEvent = ();

#[derive(Resource)]
pub struct Window(Arc<winit::window::Window>);

impl Deref for Window {
    type Target = Arc<winit::window::Window>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, Event)]
pub struct WindowResized {
    pub physical_size: PhysicalSize<u32>,
}

#[derive(Debug, Event)]
pub struct WindowEvent(winit::event::WindowEvent);

impl Deref for WindowEvent {
    type Target = winit::event::WindowEvent;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub struct WinitPlugin {
    pub canvas_id: Option<String>,
}

impl WinitPlugin {
    pub fn new(canvas_id: Option<String>) -> Self {
        Self { canvas_id }
    }
}

impl Plugin for WinitPlugin {
    fn build(&self, app: &mut App) {
        let event_loop: EventLoop<UserEvent> = EventLoopBuilder::with_user_event().build().unwrap();
        let mut window_builder = WindowBuilder::new().with_title("Point Cloud");

        cfg_if! {
            if #[cfg(target_arch = "wasm32")] {
                use wasm_bindgen::JsCast;
                use winit::platform::web::WindowBuilderExtWebSys;

                let canvas_id = self.canvas_id.as_ref().expect("No canvas id provided");

                let canvas = web_sys::window()
                .unwrap()
                .document()
                .unwrap()
                .get_element_by_id(canvas_id)
                .expect(&format!("Couldn't find canvas with id: {}", canvas_id))
                .dyn_into::<web_sys::HtmlCanvasElement>()
                .unwrap();

                window_builder = window_builder.with_prevent_default(true)
                    .with_canvas(Some(canvas))
                    .with_append(true);

            } else {
                window_builder = window_builder.with_inner_size(PhysicalSize::new(1280, 720));
            }
        }

        let window = Arc::new(window_builder.build(&event_loop).unwrap());

        app.add_event::<WindowEvent>();
        app.add_event::<WindowResized>();
        app.insert_non_send_resource(event_loop);
        app.insert_resource(Window(window.clone()));

        app.add_schedule(Schedule::new(Render));

        app.set_runner(move |mut app| {
            if app.plugins_state() == PluginsState::Ready {
                app.finish();
                app.cleanup();
                app.update();
            }

            let event_loop = app
                .world
                .remove_non_send_resource::<EventLoop<UserEvent>>()
                .unwrap();

            event_loop.set_control_flow(ControlFlow::Poll);
            let mut last_frame_time = Instant::now();
            let mut accumulated_time = Duration::from_millis(0);
            let frame_duration = Duration::from_secs_f64(1.0 / 60.0);

            let mut app_exit_event_reader = ManualEventReader::<AppExit>::default();

            let event_handler =
                move |event: Event<UserEvent>, target: &EventLoopWindowTarget<UserEvent>| {
                    let app_exit_events = app.world.get_resource_mut::<Events<AppExit>>().unwrap();

                    if app_exit_event_reader
                        .read(&app_exit_events)
                        .last()
                        .is_some()
                    {
                        target.exit();
                        return;
                    }

                    match event {
                        Event::AboutToWait => {
                            window.request_redraw();
                        }
                        Event::WindowEvent {
                            event: window_event,
                            ..
                        } => {
                            app.world.send_event(WindowEvent(window_event.clone()));

                            match window_event {
                                winit::event::WindowEvent::RedrawRequested => {
                                    let current_time = Instant::now();
                                    let mut delta_time = current_time - last_frame_time;

                                    if delta_time > frame_duration {
                                        delta_time = frame_duration;
                                    }

                                    accumulated_time += delta_time;

                                    while accumulated_time >= frame_duration {
                                        app.update();
                                        accumulated_time -= frame_duration;
                                    }

                                    app.world.run_schedule(Render);
                                    last_frame_time = current_time;
                                }
                                winit::event::WindowEvent::Resized(new_size) => {
                                    app.world
                                        .send_event(WindowResized {
                                            physical_size: PhysicalSize::new(
                                                new_size.width.max(1),
                                                new_size.height.max(1),
                                            ),
                                        })
                                        .unwrap();
                                }
                                winit::event::WindowEvent::CloseRequested => {
                                    target.exit();
                                }
                                _ => {}
                            }
                        }
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
            }
        });
    }
}

#[derive(Debug, PartialEq, Eq, Copy, Clone, Hash, ScheduleLabel)]
pub struct Render;
