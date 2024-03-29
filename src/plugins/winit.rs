use std::ops::Deref;
use std::sync::Arc;

use bevy_app::{AppExit, PluginsState};
use bevy_app::prelude::*;
use bevy_ecs::event::ManualEventReader;
use bevy_ecs::prelude::*;
use cfg_if::cfg_if;
use winit::dpi::PhysicalSize;
use winit::event::Event;
use winit::event_loop::{EventLoop, EventLoopBuilder, EventLoopWindowTarget};
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

pub struct WinitPlugin;

impl Plugin for WinitPlugin {
    fn build(&self, app: &mut App) {
        let event_loop: EventLoop<UserEvent> = EventLoopBuilder::with_user_event().build().unwrap();
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

        let window = Arc::new(window_builder.build(&event_loop).unwrap());

        app.add_event::<WindowEvent>();
        app.add_event::<WindowResized>();
        app.insert_non_send_resource(event_loop);
        app.insert_resource(Window(window));

        app.set_runner(move |mut app| {
            if app.plugins_state() == PluginsState::Ready {
                app.finish();
                app.cleanup();
            }

            let event_loop = app
                .world
                .remove_non_send_resource::<EventLoop<UserEvent>>()
                .unwrap();

            let event_handler =
                move |event: Event<UserEvent>, target: &EventLoopWindowTarget<UserEvent>| {
                    let mut app_exist_event_reader = ManualEventReader::<AppExit>::default();
                    let app_exist_events = app.world.get_resource_mut::<Events<AppExit>>().unwrap();

                    if app_exist_event_reader
                        .read(&app_exist_events)
                        .last()
                        .is_some()
                    {
                        target.exit();
                    }

                    if let Event::WindowEvent {
                        event: window_event,
                        ..
                    } = event
                    {
                        app.world.send_event(WindowEvent(window_event.clone()));

                        match window_event {
                            winit::event::WindowEvent::RedrawRequested => {
                                app.update();
                                app.world.get_resource::<Window>().unwrap().request_redraw();
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