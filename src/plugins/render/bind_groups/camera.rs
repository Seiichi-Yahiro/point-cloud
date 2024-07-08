use crate::plugins::camera::{ViewProjectionBuffer, ViewportBuffer};
use crate::plugins::wgpu::Device;
use bevy_ecs::prelude::*;

#[derive(Resource)]
pub struct CameraBindGroupLayout(pub wgpu::BindGroupLayout);

pub fn create_bind_group_layout(mut commands: Commands, device: Res<Device>) {
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("camera-bind-group-layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0, // camera
                visibility: wgpu::ShaderStages::all(),
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1, // viewport
                visibility: wgpu::ShaderStages::all(),
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    });

    commands.insert_resource(CameraBindGroupLayout(layout));
}

#[derive(Component)]
pub struct CameraBindGroup(pub wgpu::BindGroup);

impl CameraBindGroup {
    fn new(
        device: &wgpu::Device,
        layout: &CameraBindGroupLayout,
        view_projection: &ViewProjectionBuffer,
        viewport: &ViewportBuffer,
    ) -> Self {
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("camera-bind-group"),
            layout: &layout.0,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: view_projection.0.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: viewport.0.as_entire_binding(),
                },
            ],
        });

        Self(group)
    }
}

pub fn create_bind_group(
    mut commands: Commands,
    device: Res<Device>,
    layout: Res<CameraBindGroupLayout>,
    query: Query<(Entity, &ViewProjectionBuffer, &ViewportBuffer), Without<CameraBindGroup>>,
) {
    for (entity, view_projection, viewport) in query.iter() {
        let group = CameraBindGroup::new(&device, &layout, view_projection, viewport);
        commands.entity(entity).insert(group);
    }
}

pub fn update_bind_group(
    device: Res<Device>,
    layout: Res<CameraBindGroupLayout>,
    mut query: Query<
        (&ViewProjectionBuffer, &ViewportBuffer, &mut CameraBindGroup),
        Or<(Changed<ViewProjectionBuffer>, Changed<ViewportBuffer>)>,
    >,
) {
    for (view_projection, viewport, mut bind_group) in query.iter_mut() {
        *bind_group = CameraBindGroup::new(&device, &layout, view_projection, viewport);
    }
}
