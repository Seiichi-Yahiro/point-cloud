use std::collections::HashSet;

use glam::Vec2;
use winit::event::{
    DeviceEvent, ElementState, Event, KeyEvent as WinitKeyEvent, MouseButton, WindowEvent,
};
use winit::keyboard::{KeyCode, PhysicalKey};

pub struct InputData {
    pressed_keys: HashSet<KeyCode>,
    key_events: Vec<KeyEvent>,
    mouse_button_events: Vec<MouseButtonEvent>,
    mouse_delta: Vec2,
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
        }
    }
}

impl InputData {
    pub fn process_event<T>(&mut self, event: &Event<T>) {
        match event {
            Event::WindowEvent {
                event: window_event,
                ..
            } => match window_event {
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
                _ => {}
            },
            Event::DeviceEvent {
                event: DeviceEvent::MouseMotion { delta },
                ..
            } => {
                self.mouse_delta = Vec2::new(delta.0 as f32, delta.1 as f32);
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
