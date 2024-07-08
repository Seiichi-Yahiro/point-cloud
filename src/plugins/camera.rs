use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use glam::{Mat4, UVec2, Vec3};
use wgpu::util::DeviceExt;

use crate::plugins::camera::fly_cam::{FlyCamController, FlyCamPlugin};
use crate::plugins::camera::frustum::Frustum;
use crate::plugins::camera::projection::PerspectiveProjection;
use crate::plugins::render::BufferSet;
use crate::plugins::wgpu::{Device, Queue, SurfaceConfig};
use crate::plugins::winit::WindowResized;
use crate::transform::Transform;

pub mod fly_cam;
pub mod frustum;
pub mod projection;

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(FlyCamPlugin)
            .add_systems(Startup, setup.in_set(BufferSet))
            .add_systems(
                PreUpdate,
                update_aspect_ratio.run_if(on_event::<WindowResized>()),
            )
            .add_systems(
                Update,
                update_frustum.in_set(UpdateFrustum).after(CameraControlSet),
            )
            .add_systems(
                PostUpdate,
                (
                    write_view_projection_uniform,
                    write_viewport_uniform.run_if(resource_changed::<SurfaceConfig>),
                )
                    .in_set(BufferSet),
            );
    }
}

#[derive(Debug, PartialEq, Eq, Hash, Copy, Clone, SystemSet)]
pub struct CameraControlSet;

#[derive(Debug, PartialEq, Eq, Hash, Copy, Clone, SystemSet)]
pub struct UpdateFrustum;

#[derive(Component)]
pub struct ViewProjectionBuffer(pub wgpu::Buffer);

impl ViewProjectionBuffer {
    fn new(
        device: &wgpu::Device,
        transform: &Transform,
        projection: &PerspectiveProjection,
    ) -> Self {
        let view_mat = transform.compute_matrix().inverse();
        let projection_mat = projection.compute_matrix();
        let view_projection_mat = projection_mat * view_mat;

        let matrices = [view_mat, projection_mat, view_projection_mat];
        let translation = transform.translation.extend(0.0); // extend for alignment

        let a: &[u8] = bytemuck::cast_slice(&matrices);
        let b: &[u8] = bytemuck::bytes_of(&translation);

        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("view-projection-uniform"),
            contents: &[a, b].concat(),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        Self(buffer)
    }
}

#[derive(Component)]
pub struct ViewportBuffer(pub wgpu::Buffer);

impl ViewportBuffer {
    fn new(device: &wgpu::Device, config: &wgpu::SurfaceConfiguration) -> Self {
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("viewport-uniform"),
            contents: bytemuck::cast_slice(&[UVec2::new(config.width, config.height)]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        Self(buffer)
    }
}

#[derive(Component)]
pub struct Camera;

#[derive(Debug, Copy, Clone, Component)]
pub struct Visibility {
    pub visible: bool,
}

impl Visibility {
    pub fn new(visible: bool) -> Self {
        Self { visible }
    }
}

fn setup(mut commands: Commands, device: Res<Device>, config: Res<SurfaceConfig>) {
    let transform =
        Transform::from_translation(Vec3::new(0.0, -1.0, 0.0)).looking_at(Vec3::ZERO, Vec3::Z);

    let projection = PerspectiveProjection::default();

    commands.spawn((
        Camera,
        FlyCamController::new(),
        ViewProjectionBuffer::new(&device, &transform, &projection),
        ViewportBuffer::new(&device, &config),
        transform,
        projection,
        Frustum::default(),
    ));
}

fn update_aspect_ratio(
    mut window_resized: EventReader<WindowResized>,
    mut query: Query<&mut PerspectiveProjection>,
) {
    if let Some(resized) = window_resized.read().last() {
        for mut projection in query.iter_mut() {
            projection.aspect_ratio =
                resized.physical_size.width as f32 / resized.physical_size.height as f32;
        }
    }
}

fn update_frustum(
    mut camera_query: Query<
        (&mut Frustum, &Transform, &PerspectiveProjection),
        (
            With<Camera>,
            Or<(Changed<Transform>, Changed<PerspectiveProjection>)>,
        ),
    >,
) {
    for (mut frustum, transform, projection) in camera_query.iter_mut() {
        *frustum = Frustum::new(transform, projection);
    }
}

fn write_view_projection_uniform(
    queue: Res<Queue>,
    view_projection_query: Query<
        (&Transform, &PerspectiveProjection, &ViewProjectionBuffer),
        (
            With<Camera>,
            Or<(Changed<Transform>, Changed<PerspectiveProjection>)>,
        ),
    >,
) {
    for (transform, projection, buffer) in view_projection_query.iter() {
        let view_mat = transform.compute_matrix().inverse();
        let projection_mat = projection.compute_matrix();
        let view_projection_mat = projection_mat * view_mat;

        queue.write_buffer(
            &buffer.0,
            0,
            bytemuck::cast_slice(&[view_mat, projection_mat, view_projection_mat]),
        );

        queue.write_buffer(
            &buffer.0,
            (std::mem::size_of::<Mat4>() * 3) as wgpu::BufferAddress,
            bytemuck::bytes_of(&transform.translation),
        );
    }
}

fn write_viewport_uniform(
    queue: Res<Queue>,
    config: Res<SurfaceConfig>,
    camera_query: Query<&ViewportBuffer, With<Camera>>,
) {
    for buffer in camera_query.iter() {
        queue.write_buffer(
            &buffer.0,
            0,
            bytemuck::cast_slice(&[UVec2::new(config.width, config.height)]),
        );
    }
}

pub fn draw_ui(ui: &mut egui::Ui, world: &mut World) {
    let mut query = world.query_filtered::<&Transform, With<Camera>>();
    for transform in query.iter_mut(world) {
        ui.collapsing("Position", |ui| {
            ui.label(format!("x: {}", transform.translation.x));
            ui.label(format!("y: {}", transform.translation.y));
            ui.label(format!("z: {}", transform.translation.z));
        });
    }

    fly_cam::draw_ui(ui, world);
}
