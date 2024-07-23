use std::cell::RefCell;
use std::ops::Deref;
use std::rc::Rc;
use std::sync::Arc;

use bevy_app::prelude::*;
use bevy_app::{AppExit, PluginsState};
use bevy_ecs::event::ManualEventReader;
use bevy_ecs::prelude::*;
use cfg_if::cfg_if;
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
                window_builder = window_builder.with_inner_size(PhysicalSize::new(1920, 1080));
            }
        }

        let window = Arc::new(window_builder.build(&event_loop).unwrap());

        app.add_event::<WindowEvent>();
        app.add_event::<WindowResized>();
        app.insert_non_send_resource(event_loop);
        app.insert_resource(Window(window.clone()));

        app.set_runner(move |mut app| {
            if app.plugins_state() == PluginsState::Ready {
                app.finish();
                app.cleanup();
                app.update();
            }

            let event_loop = app
                .world_mut()
                .remove_non_send_resource::<EventLoop<UserEvent>>()
                .unwrap();

            event_loop.set_control_flow(ControlFlow::Poll);

            let mut app_exit_event_reader = ManualEventReader::<AppExit>::default();

            let exit = Rc::new(RefCell::new(AppExit::Success));
            let winit_exit = exit.clone();

            let event_handler =
                move |event: Event<UserEvent>, target: &EventLoopWindowTarget<UserEvent>| {
                    let app_exit_events = app
                        .world_mut()
                        .get_resource_mut::<Events<AppExit>>()
                        .unwrap();

                    if let Some(app_exit) = app_exit_event_reader.read(&app_exit_events).next() {
                        *winit_exit.borrow_mut() = app_exit.clone();
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
                            app.world_mut()
                                .send_event(WindowEvent(window_event.clone()));

                            match window_event {
                                winit::event::WindowEvent::RedrawRequested => {
                                    app.update();
                                }
                                winit::event::WindowEvent::Resized(new_size) => {
                                    app.world_mut()
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

            let exit = exit.borrow().clone();
            exit
        });
    }
}
