use std::hash::Hash;

use bevy_app::prelude::*;
use bevy_ecs::event::{EventReader, EventWriter};
use bevy_ecs::prelude::*;
use egui::ahash::HashSet;
use winit::dpi::PhysicalPosition;
use winit::event::{ElementState, MouseButton};
use winit::keyboard::{KeyCode, PhysicalKey};

use crate::plugins::winit::WindowEvent;

pub struct InputPlugin;

impl Plugin for InputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PressedKeys>()
            .init_resource::<PressedMouseButtons>()
            .add_event::<KeyEvent>()
            .add_event::<MouseButtonEvent>()
            .add_event::<CursorEvent>()
            .add_systems(First, handle_window_events);
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Event)]
pub struct KeyEvent {
    pub key: KeyCode,
    pub state: ElementState,
}

#[derive(Resource)]
pub struct PressedButton<T: Hash + Eq> {
    pressed: HashSet<T>,
}

impl<T: Hash + Eq> PressedButton<T> {
    pub fn is_pressed(&self, button: &T) -> bool {
        self.pressed.contains(button)
    }
}

pub type PressedKeys = PressedButton<KeyCode>;
pub type PressedMouseButtons = PressedButton<MouseButton>;

impl Default for PressedKeys {
    fn default() -> Self {
        Self {
            pressed: HashSet::default(),
        }
    }
}

impl Default for PressedMouseButtons {
    fn default() -> Self {
        Self {
            pressed: HashSet::default(),
        }
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Event)]
pub struct MouseButtonEvent {
    pub button: MouseButton,
    pub state: ElementState,
}

#[derive(Debug, Copy, Clone, Event)]
pub struct CursorEvent {
    pub position: PhysicalPosition<f64>,
    pub delta: PhysicalPosition<f64>,
}

fn handle_window_events(
    mut window_events: EventReader<WindowEvent>,
    mut key_events: EventWriter<KeyEvent>,
    mut mouse_button_events: EventWriter<MouseButtonEvent>,
    mut cursor_events: EventWriter<CursorEvent>,
    mut last_cursor_position: Local<Option<PhysicalPosition<f64>>>,
    mut pressed_keys: ResMut<PressedKeys>,
    mut pressed_mouse_buttons: ResMut<PressedMouseButtons>,
) {
    use winit::event::KeyEvent as WinitKeyEvent;
    use winit::event::WindowEvent as WinitWindowEvent;

    for event in window_events.read() {
        let event: &WinitWindowEvent = event;

        match event {
            WinitWindowEvent::KeyboardInput {
                event:
                    WinitKeyEvent {
                        state,
                        physical_key: PhysicalKey::Code(key),
                        ..
                    },
                ..
            } => {
                if *state == ElementState::Pressed {
                    pressed_keys.pressed.insert(*key);
                } else {
                    pressed_keys.pressed.remove(key);
                }

                key_events.send(KeyEvent {
                    key: *key,
                    state: *state,
                });
            }
            WinitWindowEvent::MouseInput { state, button, .. } => {
                if *state == ElementState::Pressed {
                    pressed_mouse_buttons.pressed.insert(*button);
                } else {
                    pressed_mouse_buttons.pressed.remove(button);
                }

                mouse_button_events.send(MouseButtonEvent {
                    button: *button,
                    state: *state,
                });
            }
            WinitWindowEvent::CursorLeft { .. } => {
                *last_cursor_position = None;
            }
            WinitWindowEvent::CursorMoved { position, .. } => {
                let delta = last_cursor_position
                    .map(|it| {
                        let delta_x = position.x - it.x;
                        let delta_y = position.y - it.y;
                        PhysicalPosition::new(delta_x, delta_y)
                    })
                    .unwrap_or_default();

                *last_cursor_position = Some(*position);

                cursor_events.send(CursorEvent {
                    position: *position,
                    delta,
                });
            }
            _ => {}
        }
    }
}
