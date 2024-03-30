use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use glam::{EulerRot, Quat, Vec3};
use winit::event::MouseButton;
use winit::keyboard::KeyCode;

use crate::plugins::input::{CursorEvent, PressedKeys, PressedMouseButtons};
use crate::transform::Transform;

pub struct FlyCamPlugin;

impl Plugin for FlyCamPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, update);
    }
}

#[derive(Debug, Component)]
pub struct FlyCamController {
    keybindings: FlyCamKeybindings,
    mouse_sensitivity: f32,
    movement_speed: f32,
    look_around: bool,
}

impl FlyCamController {
    pub fn new() -> Self {
        Self {
            keybindings: FlyCamKeybindings::default(),
            mouse_sensitivity: 0.002,
            movement_speed: 0.1,
            look_around: false,
        }
    }
}

#[derive(Debug, Copy, Clone)]
struct FlyCamKeybindings {
    forward: KeyCode,
    backward: KeyCode,
    left: KeyCode,
    right: KeyCode,
    ascend: KeyCode,
    descend: KeyCode,
    look_around: MouseButton,
}

impl Default for FlyCamKeybindings {
    fn default() -> Self {
        Self {
            forward: KeyCode::KeyW,
            backward: KeyCode::KeyS,
            left: KeyCode::KeyA,
            right: KeyCode::KeyD,
            ascend: KeyCode::Space,
            descend: KeyCode::ShiftLeft,
            look_around: MouseButton::Right,
        }
    }
}

fn update(
    mut query: Query<(&mut FlyCamController, &mut Transform)>,
    pressed_keys: Res<PressedKeys>,
    pressed_mouse_buttons: Res<PressedMouseButtons>,
    mut cursor_events: EventReader<CursorEvent>,
) {
    for (mut fly_cam, mut transform) in query.iter_mut() {
        let forward = transform.forward();
        let right = transform.right();
        let up = Vec3::Y;

        let mut velocity = Vec3::ZERO;

        if pressed_keys.is_pressed(&fly_cam.keybindings.forward) {
            velocity += forward;
        }

        if pressed_keys.is_pressed(&fly_cam.keybindings.backward) {
            velocity -= forward;
        }

        if pressed_keys.is_pressed(&fly_cam.keybindings.left) {
            velocity -= right;
        }

        if pressed_keys.is_pressed(&fly_cam.keybindings.right) {
            velocity += right;
        }

        if pressed_keys.is_pressed(&fly_cam.keybindings.ascend) {
            velocity += up;
        }

        if pressed_keys.is_pressed(&fly_cam.keybindings.descend) {
            velocity -= up;
        }

        velocity = velocity.normalize_or_zero() * fly_cam.movement_speed;

        fly_cam.look_around = pressed_mouse_buttons.is_pressed(&fly_cam.keybindings.look_around);

        if fly_cam.look_around {
            for cursor_event in cursor_events.read() {
                let relative_yaw = -cursor_event.delta.x as f32 * fly_cam.mouse_sensitivity;
                let relative_pitch = -cursor_event.delta.y as f32 * fly_cam.mouse_sensitivity;

                let (yaw, pitch, roll) = transform.rotation.to_euler(EulerRot::YXZ);

                let new_yaw = yaw + relative_yaw;
                let new_pitch = (pitch + relative_pitch).clamp(-1.54, 1.54);

                transform.rotation = Quat::from_euler(EulerRot::YXZ, new_yaw, new_pitch, roll);
            }
        } else {
            cursor_events.clear();
        }

        if velocity != Vec3::ZERO {
            transform.translation += velocity;
        }
    }
}
