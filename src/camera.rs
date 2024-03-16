use glam::{EulerRot, Mat4, Quat, Vec3};
use winit::event::ElementState;
use winit::keyboard::KeyCode;

use crate::input_data::InputData;
use crate::transform::Transform;

#[derive(Debug, Default, Clone)]
pub struct Camera {
    pub transform: Transform,
    pub projection: PerspectiveProjection,
}

impl Camera {
    pub fn new(position: Vec3, looking_at: Vec3) -> Self {
        Self {
            transform: Transform::from_translation(position).looking_at(looking_at, Vec3::Y),
            projection: PerspectiveProjection::default(),
        }
    }

    pub fn get_view_matrix(&self) -> Mat4 {
        self.transform.compute_matrix().inverse()
    }

    pub fn get_projection_matrix(&self) -> Mat4 {
        self.projection.get_projection_matrix()
    }

    pub fn get_view_projection_matrix(&self) -> Mat4 {
        self.get_projection_matrix() * self.get_view_matrix()
    }
}

#[derive(Debug, Clone)]
pub struct PerspectiveProjection {
    pub fov_y: f32,
    pub aspect_ratio: f32,
    pub near: f32,
    pub far: f32,
}

impl PerspectiveProjection {
    pub fn get_projection_matrix(&self) -> Mat4 {
        Mat4::perspective_rh(self.fov_y, self.aspect_ratio, self.near, self.far)
    }
}

impl Default for PerspectiveProjection {
    fn default() -> Self {
        Self {
            fov_y: std::f32::consts::FRAC_PI_4,
            aspect_ratio: 1.0,
            near: 0.1,
            far: 1000.0,
        }
    }
}

#[derive(Debug)]
pub struct FlyCamController {
    camera: Camera,
    keybindings: FlyCamKeybindings,
    mouse_sensitivity: f32,
    movement_speed: f32,
    yaw: f32,
    pitch: f32,
    look_around: bool,
}

impl FlyCamController {
    pub fn new(camera: Camera) -> Self {
        Self {
            camera,
            keybindings: FlyCamKeybindings::default(),
            mouse_sensitivity: 1.2,
            movement_speed: 0.1,
            yaw: 0.0,
            pitch: 0.0,
            look_around: false,
        }
    }

    pub fn camera(&self) -> &Camera {
        &self.camera
    }

    pub fn camera_mut(&mut self) -> &mut Camera {
        &mut self.camera
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
    look_around: KeyCode,
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
            look_around: KeyCode::KeyE,
        }
    }
}

impl FlyCamController {
    pub fn update(&mut self, input_data: &InputData) {
        let pressed_keys = input_data.pressed_keys();

        let forward = self.camera.transform.forward();
        let right = self.camera.transform.right();
        let up = Vec3::Y;

        let mut velocity = Vec3::ZERO;

        if pressed_keys.contains(&self.keybindings.forward) {
            velocity += forward;
        }

        if pressed_keys.contains(&self.keybindings.backward) {
            velocity -= forward;
        }

        if pressed_keys.contains(&self.keybindings.left) {
            velocity -= right;
        }

        if pressed_keys.contains(&self.keybindings.right) {
            velocity += right;
        }

        if pressed_keys.contains(&self.keybindings.ascend) {
            velocity += up;
        }

        if pressed_keys.contains(&self.keybindings.descend) {
            velocity -= up;
        }

        velocity = velocity.normalize_or_zero() * self.movement_speed;

        if input_data.key_events().iter().any(|key_event| {
            key_event.key == self.keybindings.look_around
                && key_event.state == ElementState::Pressed
        }) {
            self.look_around = !self.look_around;
        }

        if self.look_around {
            let mouse_delta = input_data.mouse_delta();
            self.yaw -= (mouse_delta.x * self.mouse_sensitivity).to_radians();
            self.pitch -= (mouse_delta.y * self.mouse_sensitivity).to_radians();
            self.pitch = self.pitch.clamp(-1.54, 1.54);

            self.camera.transform.rotation =
                Quat::from_euler(EulerRot::YXZ, self.yaw, self.pitch, 0.0);
        }

        self.camera.transform.translation += velocity;
    }
}
