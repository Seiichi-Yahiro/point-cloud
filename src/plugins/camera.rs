use std::ops::Deref;

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use glam::{UVec2, Vec3};
use wgpu::util::DeviceExt;

use crate::plugins::camera::fly_cam::{FlyCamController, FlyCamPlugin};
use crate::plugins::camera::frustum::{Aabb, Frustum};
use crate::plugins::camera::projection::PerspectiveProjection;
use crate::plugins::wgpu::{Device, Queue, SurfaceConfig};
use crate::plugins::winit::WindowResized;
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
                        visibility: wgpu::ShaderStages::VERTEX,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::VERTEX,
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
            .add_systems(PreUpdate, update_aspect_ratio)
            .add_systems(
                PostUpdate,
                (
                    write_view_projection_uniform,
                    write_viewport_uniform.run_if(resource_changed::<SurfaceConfig>),
                    (update_frustum, frustum_cull).chain(),
                ),
            );
    }
}

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
    pub visible_entities: Vec<Entity>,
    pub frustum_cull_settings: FrustumCullSettings,
}

#[derive(Debug, Copy, Clone)]
struct FrustumCullSettings {
    pub enabled: bool,
    pub paused: bool,
}

fn setup(
    mut commands: Commands,
    device: Res<Device>,
    config: Res<SurfaceConfig>,
    view_projection_bind_group_layout: Res<ViewBindGroupLayout>,
) {
    let transform =
        Transform::from_translation(Vec3::new(0.0, 0.0, 2.0)).looking_at(Vec3::ZERO, Vec3::Y);
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
            visible_entities: Vec::new(),
            frustum_cull_settings: FrustumCullSettings {
                enabled: true,
                paused: false,
            },
        },
        FlyCamController::new(),
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

fn frustum_cull(
    mut camera_query: Query<(&mut Camera, &Frustum)>,
    object_query: Query<(Entity, &Aabb)>,
) {
    for (mut camera, frustum) in camera_query.iter_mut() {
        if camera.frustum_cull_settings.paused {
            continue;
        }

        camera.visible_entities.clear();

        if camera.frustum_cull_settings.enabled {
            for (entity, aabb) in object_query.iter() {
                if !frustum.cull_aabb(*aabb) {
                    camera.visible_entities.push(entity);
                }
            }
        } else {
            for (entity, _aabb) in object_query.iter() {
                camera.visible_entities.push(entity);
            }
        }
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
    ui.label("Frustum culling:");

    let mut query = world.query::<&mut Camera>();
    for mut camera in query.iter_mut(world) {
        let mut enabled = camera.frustum_cull_settings.enabled;
        if ui.checkbox(&mut enabled, "Enabled").changed() {
            camera.frustum_cull_settings.enabled = enabled;

            if !enabled {
                camera.frustum_cull_settings.paused = false;
            }
        }

        let mut paused = camera.frustum_cull_settings.paused;
        let paused_check_box = egui::Checkbox::new(&mut paused, "Paused");

        if ui.add_enabled(enabled, paused_check_box).changed() {
            camera.frustum_cull_settings.paused = paused;
        }
    }
}
