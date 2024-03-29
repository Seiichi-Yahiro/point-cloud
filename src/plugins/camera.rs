use std::ops::Deref;

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use glam::Vec3;
use wgpu::util::DeviceExt;

use crate::plugins::camera::fly_cam::{FlyCamController, FlyCamPlugin};
use crate::plugins::camera::projection::PerspectiveProjection;
use crate::plugins::wgpu::{Device, Queue};
use crate::plugins::winit::WindowResized;
use crate::transform::Transform;

pub mod fly_cam;
pub mod projection;

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        let view_projection_bind_group_layout = app
            .world
            .get_resource::<Device>()
            .unwrap()
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("view-projection-bind-group-layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        app.insert_resource(ViewProjectionBindGroupLayout(
            view_projection_bind_group_layout,
        ));

        app.add_plugins(FlyCamPlugin)
            .add_systems(Startup, setup)
            .add_systems(PreUpdate, update_aspect_ratio)
            .add_systems(PostUpdate, write_buffer);
    }
}

#[derive(Resource)]
pub struct ViewProjectionBindGroupLayout(wgpu::BindGroupLayout);

impl Deref for ViewProjectionBindGroupLayout {
    type Target = wgpu::BindGroupLayout;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Component)]
pub struct Camera {
    pub uniform: wgpu::Buffer,
    pub bind_group: wgpu::BindGroup,
}

fn setup(
    mut commands: Commands,
    device: Res<Device>,
    view_projection_bind_group_layout: Res<ViewProjectionBindGroupLayout>,
) {
    let transform =
        Transform::from_translation(Vec3::new(0.0, 0.0, 2.0)).looking_at(Vec3::ZERO, Vec3::Y);
    let projection = PerspectiveProjection::default();

    let view_projection_uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("view-projection-uniform"),
        contents: bytemuck::cast_slice(&[
            transform.compute_matrix().inverse(),
            projection.compute_matrix(),
        ]),
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });

    let view_projection_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("view-projection-bind-group"),
        layout: &view_projection_bind_group_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: view_projection_uniform.as_entire_binding(),
        }],
    });

    commands.spawn((
        Camera {
            uniform: view_projection_uniform,
            bind_group: view_projection_bind_group,
        },
        FlyCamController::new(),
        transform,
        projection,
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

fn write_buffer(
    queue: Res<Queue>,
    query: Query<
        (&Camera, &Transform, &PerspectiveProjection),
        Or<(Changed<Transform>, Changed<PerspectiveProjection>)>,
    >,
) {
    for (camera, transform, projection) in query.iter() {
        queue.write_buffer(
            &camera.uniform,
            0,
            bytemuck::cast_slice(&[
                transform.compute_matrix().inverse(),
                projection.compute_matrix(),
            ]),
        );
    }
}
