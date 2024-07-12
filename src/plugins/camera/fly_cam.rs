use crate::plugins::camera::CameraControlSet;
use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_time::Time;
use glam::{EulerRot, Quat, Vec3};
use winit::event::{MouseButton, MouseScrollDelta};
use winit::keyboard::KeyCode;

use crate::plugins::input::{CursorEvent, MouseWheelEvent, PressedKeys, PressedMouseButtons};
use crate::transform::Transform;

pub struct FlyCamPlugin;

impl Plugin for FlyCamPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, update_movement_speed.in_set(CameraControlSet))
            .add_systems(FixedUpdate, update.in_set(CameraControlSet));
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
    const MIN_MOVEMENT_SPEED: f32 = 5.0;
    const MAX_MOVEMENT_SPEED: f32 = 1000.0;
    const MOVEMENT_SPEED_STEP: f32 = 5.0;

    pub fn new() -> Self {
        Self {
            keybindings: FlyCamKeybindings::default(),
            mouse_sensitivity: 0.15,
            movement_speed: 50.0,
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
    time: Res<Time>,
) {
    for (mut fly_cam, mut transform) in query.iter_mut() {
        let forward = transform.forward();
        let right = transform.right();
        let up = Vec3::Z;

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

        velocity = velocity.normalize_or_zero() * fly_cam.movement_speed * time.delta_seconds();

        fly_cam.look_around = pressed_mouse_buttons.is_pressed(&fly_cam.keybindings.look_around);

        if fly_cam.look_around {
            for cursor_event in cursor_events.read() {
                let relative_yaw =
                    -cursor_event.delta.x as f32 * fly_cam.mouse_sensitivity * time.delta_seconds();
                let relative_pitch =
                    -cursor_event.delta.y as f32 * fly_cam.mouse_sensitivity * time.delta_seconds();

                transform.rotation *= Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);

                let (yaw, pitch, _roll) = transform.rotation.to_euler(EulerRot::ZXY);

                let new_yaw = yaw + relative_yaw;
                let new_pitch = (pitch + relative_pitch).clamp(-1.54, 1.54);

                transform.rotation = Quat::from_euler(EulerRot::ZXY, new_yaw, new_pitch, 0.0);

                transform.rotation *= Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
            }
        } else {
            cursor_events.clear();
        }

        if velocity != Vec3::ZERO {
            transform.translation += velocity;
        }
    }
}

fn update_movement_speed(
    mut query: Query<&mut FlyCamController>,
    mut mouse_wheel_event: EventReader<MouseWheelEvent>,
) {
    let mut fly_cam = query.get_single_mut().unwrap();

    if !fly_cam.look_around {
        return;
    }

    for event in mouse_wheel_event.read() {
        let y_delta = match event.delta {
            MouseScrollDelta::LineDelta(_, y) => y,
            MouseScrollDelta::PixelDelta(pos) => pos.y as f32,
        };

        let y_delta = if y_delta == 0.0 {
            0.0
        } else {
            y_delta.signum() * FlyCamController::MOVEMENT_SPEED_STEP
        };

        fly_cam.movement_speed = (fly_cam.movement_speed + y_delta).clamp(
            FlyCamController::MIN_MOVEMENT_SPEED,
            FlyCamController::MAX_MOVEMENT_SPEED,
        );
    }
}

pub fn draw_ui(ui: &mut egui::Ui, world: &mut World) {
    let mut fly_cam = world
        .query::<&mut FlyCamController>()
        .get_single_mut(world)
        .unwrap();

    ui.label("Speed:");

    let movement_speed = egui::Slider::new(
        &mut fly_cam.movement_speed,
        FlyCamController::MIN_MOVEMENT_SPEED..=FlyCamController::MAX_MOVEMENT_SPEED,
    )
    .step_by(FlyCamController::MOVEMENT_SPEED_STEP as f64);

    ui.add(movement_speed);
}
