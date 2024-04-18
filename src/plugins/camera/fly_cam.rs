use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_input::mouse::MouseWheel;
use bevy_input::prelude::{KeyCode, MouseButton};
use bevy_input::ButtonInput;
use bevy_window::CursorMoved;
use glam::{EulerRot, Quat, Vec3};

use crate::plugins::camera::CameraControlSet;
use crate::transform::Transform;

pub struct FlyCamPlugin;

impl Plugin for FlyCamPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (update_movement_speed, update)
                .in_set(CameraControlSet)
                .chain(),
        );
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
    const MIN_MOVEMENT_SPEED: f32 = 0.1;
    const MAX_MOVEMENT_SPEED: f32 = 20.0;
    const MOVEMENT_SPEED_STEP: f32 = 0.1;

    pub fn new() -> Self {
        Self {
            keybindings: FlyCamKeybindings::default(),
            mouse_sensitivity: 0.002,
            movement_speed: 1.0,
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
    keyboard_input: Res<ButtonInput<KeyCode>>,
    mouse_button_input: Res<ButtonInput<MouseButton>>,
    mut cursor_moved_events: EventReader<CursorMoved>,
) {
    for (mut fly_cam, mut transform) in query.iter_mut() {
        let forward = transform.forward();
        let right = transform.right();
        let up = Vec3::Z;

        let mut velocity = Vec3::ZERO;

        if keyboard_input.pressed(fly_cam.keybindings.forward) {
            velocity += forward;
        }

        if keyboard_input.pressed(fly_cam.keybindings.backward) {
            velocity -= forward;
        }

        if keyboard_input.pressed(fly_cam.keybindings.left) {
            velocity -= right;
        }

        if keyboard_input.pressed(fly_cam.keybindings.right) {
            velocity += right;
        }

        if keyboard_input.pressed(fly_cam.keybindings.ascend) {
            velocity += up;
        }

        if keyboard_input.pressed(fly_cam.keybindings.descend) {
            velocity -= up;
        }

        velocity = velocity.normalize_or_zero() * fly_cam.movement_speed;

        fly_cam.look_around = mouse_button_input.pressed(fly_cam.keybindings.look_around);

        if fly_cam.look_around {
            for cursor_event in cursor_moved_events.read() {
                let delta = cursor_event.delta.unwrap_or_default();

                let relative_yaw = -delta.x * fly_cam.mouse_sensitivity;
                let relative_pitch = -delta.y * fly_cam.mouse_sensitivity;

                transform.rotation *= Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);

                let (yaw, pitch, _roll) = transform.rotation.to_euler(EulerRot::ZXY);

                let new_yaw = yaw + relative_yaw;
                let new_pitch = (pitch + relative_pitch).clamp(-1.54, 1.54);

                transform.rotation = Quat::from_euler(EulerRot::ZXY, new_yaw, new_pitch, 0.0);

                transform.rotation *= Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
            }
        } else {
            cursor_moved_events.clear();
        }

        if velocity != Vec3::ZERO {
            transform.translation += velocity;
        }
    }
}

fn update_movement_speed(
    mut query: Query<&mut FlyCamController>,
    mut mouse_wheel_events: EventReader<MouseWheel>,
) {
    let mut fly_cam = query.get_single_mut().unwrap();

    if !fly_cam.look_around {
        return;
    }

    for event in mouse_wheel_events.read() {
        let y_delta = if event.y == 0.0 {
            0.0
        } else {
            event.y.signum() * FlyCamController::MOVEMENT_SPEED_STEP
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
