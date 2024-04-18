use std::ops::Deref;

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_window::WindowResized;
use glam::{UVec2, Vec3};
use wgpu::util::DeviceExt;

use crate::plugins::camera::fly_cam::{FlyCamController, FlyCamPlugin};
use crate::plugins::camera::frustum::Frustum;
use crate::plugins::camera::projection::PerspectiveProjection;
use crate::plugins::wgpu::{Device, Queue, SurfaceConfig};
use crate::transform::Transform;

pub mod fly_cam;
pub mod frustum;
pub mod projection;

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        let view_projection_bind_group_layout = app
            .world
            .get_resource::<Device>()
            .unwrap()
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("view-bind-group-layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });

        app.insert_resource(ViewBindGroupLayout(view_projection_bind_group_layout));

        app.add_plugins(FlyCamPlugin)
            .add_systems(Startup, setup)
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
                ),
            );
    }
}

#[derive(Debug, PartialEq, Eq, Hash, Copy, Clone, SystemSet)]
pub struct CameraControlSet;

#[derive(Debug, PartialEq, Eq, Hash, Copy, Clone, SystemSet)]
pub struct UpdateFrustum;

#[derive(Resource)]
pub struct ViewBindGroupLayout(wgpu::BindGroupLayout);

impl Deref for ViewBindGroupLayout {
    type Target = wgpu::BindGroupLayout;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Component)]
pub struct Camera {
    pub view_projection: wgpu::Buffer,
    pub viewport: wgpu::Buffer,
    pub bind_group: wgpu::BindGroup,
}

#[derive(Debug, Copy, Clone, Component)]
pub struct Visibility {
    pub visible: bool,
}

impl Visibility {
    pub fn new(visible: bool) -> Self {
        Self { visible }
    }
}

fn setup(
    mut commands: Commands,
    device: Res<Device>,
    config: Res<SurfaceConfig>,
    view_projection_bind_group_layout: Res<ViewBindGroupLayout>,
) {
    let transform =
        Transform::from_translation(Vec3::new(0.0, -1.0, 0.0)).looking_at(Vec3::ZERO, Vec3::Z);

    let projection = PerspectiveProjection::default();

    let view_projection_uniform = {
        let view_mat = transform.compute_matrix().inverse();
        let projection_mat = projection.compute_matrix();
        let view_projection_mat = projection_mat * view_mat;

        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("view-projection-uniform"),
            contents: bytemuck::cast_slice(&[view_mat, projection_mat, view_projection_mat]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        })
    };

    let viewport_uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("viewport-uniform"),
        contents: bytemuck::cast_slice(&[UVec2::new(config.width, config.height)]),
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });

    let view_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("view-bind-group"),
        layout: &view_projection_bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: view_projection_uniform.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: viewport_uniform.as_entire_binding(),
            },
        ],
    });

    commands.spawn((
        Camera {
            view_projection: view_projection_uniform,
            viewport: viewport_uniform,
            bind_group: view_bind_group,
        },
        FlyCamController::new(),
        transform,
        projection,
        Frustum::default(),
    ));
}

fn update_aspect_ratio(
    mut query: Query<&mut PerspectiveProjection>,
    surface_config: Res<SurfaceConfig>,
) {
    let aspect_ratio = surface_config.width as f32 / surface_config.height as f32;
    for mut projection in query.iter_mut() {
        projection.aspect_ratio = aspect_ratio;
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
        (&Camera, &Transform, &PerspectiveProjection),
        Or<(Changed<Transform>, Changed<PerspectiveProjection>)>,
    >,
) {
    for (camera, transform, projection) in view_projection_query.iter() {
        let view_mat = transform.compute_matrix().inverse();
        let projection_mat = projection.compute_matrix();
        let view_projection_mat = projection_mat * view_mat;

        queue.write_buffer(
            &camera.view_projection,
            0,
            bytemuck::cast_slice(&[view_mat, projection_mat, view_projection_mat]),
        );
    }
}

fn write_viewport_uniform(
    queue: Res<Queue>,
    config: Res<SurfaceConfig>,
    camera_query: Query<&Camera>,
) {
    for camera in camera_query.iter() {
        queue.write_buffer(
            &camera.viewport,
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
