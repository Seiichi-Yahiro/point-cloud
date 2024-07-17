use crate::plugins::cell::shader::{FrustumsBuffer, FrustumsSettings, LoadedCellsBuffer};
use crate::plugins::metadata::shader::MetadataBuffer;
use crate::plugins::wgpu::Device;
use bevy_ecs::prelude::*;

#[derive(Resource)]
pub struct ResourceBindGroupLayout(pub wgpu::BindGroupLayout);

pub fn create_bind_group_layout(mut commands: Commands, device: Res<Device>) {
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("point-resource-bind-group-layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0, // metadata
                visibility: wgpu::ShaderStages::COMPUTE | wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1, // loaded cells
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2, // frustums
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 3, // frustums settings
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    });

    commands.insert_resource(ResourceBindGroupLayout(layout));
}

#[derive(Resource)]
pub struct ResourceBindGroup(pub wgpu::BindGroup);

pub fn create_bind_group(
    mut commands: Commands,
    device: Res<Device>,
    layout: Res<ResourceBindGroupLayout>,
    metadata: Res<MetadataBuffer>,
    loaded_cells: Res<LoadedCellsBuffer>,
    frustums: Res<FrustumsBuffer>,
    frustums_settings: Res<FrustumsSettings>,
) {
    if !(metadata.is_changed()
        || loaded_cells.is_changed()
        || frustums.is_changed()
        || frustums_settings.is_changed())
    {
        return;
    }

    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("point-resource-bind-group"),
        layout: &layout.0,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: metadata.buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: loaded_cells.buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: frustums.buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: frustums_settings.buffer.as_entire_binding(),
            },
        ],
    });

    commands.insert_resource(ResourceBindGroup(group));
}
