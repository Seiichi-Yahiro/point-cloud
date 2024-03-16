use std::collections::HashSet;

use glam::Vec2;
use winit::dpi::PhysicalPosition;
use winit::event::{ElementState, KeyEvent as WinitKeyEvent, MouseButton, WindowEvent};
use winit::keyboard::{KeyCode, PhysicalKey};

pub struct InputData {
    pressed_keys: HashSet<KeyCode>,
    key_events: Vec<KeyEvent>,
    mouse_button_events: Vec<MouseButtonEvent>,
    mouse_delta: Vec2,
    last_cursor_pos: Option<PhysicalPosition<f64>>,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct KeyEvent {
    pub key: KeyCode,
    pub state: ElementState,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct MouseButtonEvent {
    pub button: MouseButton,
    pub state: ElementState,
}

impl Default for InputData {
    fn default() -> Self {
        Self {
            pressed_keys: HashSet::new(),
            key_events: Vec::with_capacity(5),
            mouse_button_events: Vec::with_capacity(3),
            mouse_delta: Vec2::ZERO,
            last_cursor_pos: None,
        }
    }
}

impl InputData {
    pub fn process_event(&mut self, event: &WindowEvent) {
        match event {
            WindowEvent::KeyboardInput {
                event:
                    WinitKeyEvent {
                        state,
                        physical_key: PhysicalKey::Code(key),
                        ..
                    },
                ..
            } => {
                if *state == ElementState::Pressed {
                    self.pressed_keys.insert(*key);
                } else {
                    self.pressed_keys.remove(key);
                }

                self.key_events.push(KeyEvent {
                    key: *key,
                    state: *state,
                });
            }
            WindowEvent::MouseInput { state, button, .. } => {
                self.mouse_button_events.push(MouseButtonEvent {
                    button: *button,
                    state: *state,
                });
            }
            WindowEvent::CursorMoved { position, .. } => {
                if let Some(last_position) = self.last_cursor_pos {
                    self.mouse_delta.x = (position.x - last_position.x) as f32;
                    self.mouse_delta.y = (position.y - last_position.y) as f32;
                }

                self.last_cursor_pos = Some(*position);
            }
            _ => {}
        }
    }

    pub fn pressed_keys(&self) -> &HashSet<KeyCode> {
        &self.pressed_keys
    }

    pub fn key_events(&self) -> &[KeyEvent] {
        &self.key_events
    }

    pub fn mouse_button_events(&self) -> &[MouseButtonEvent] {
        &self.mouse_button_events
    }

    pub fn mouse_delta(&self) -> Vec2 {
        self.mouse_delta
    }

    pub fn clear_events(&mut self) {
        self.key_events.clear();
        self.mouse_button_events.clear();
        self.mouse_delta = Vec2::ZERO;
    }
}
