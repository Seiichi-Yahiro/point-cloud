use crate::plugins::streaming::metadata::ActiveMetadataRes;
use crate::plugins::wgpu::{Device, Queue};
use bevy_ecs::prelude::*;
use itertools::Itertools;

#[repr(C)]
#[derive(Debug, Default, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct Hierarchy {
    cell_size: f32,
    spacing: f32,
}

#[derive(Resource)]
pub struct MetadataBindGroupData {
    pub layout: wgpu::BindGroupLayout,
    pub group: wgpu::BindGroup,
    buffer: wgpu::Buffer,
}

impl MetadataBindGroupData {
    pub fn new(device: &wgpu::Device) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("metadata-bind-group-layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("metadata-buffer"),
            size: (std::mem::size_of::<u32>() + std::mem::size_of::<Hierarchy>() * 20)
                as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("metadata-bind-group"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });

        Self {
            layout,
            group,
            buffer,
        }
    }
}

pub(super) fn write_buffer(
    queue: Res<Queue>,
    metadata_bind_group_data: Res<MetadataBindGroupData>,
    active_metadata: ActiveMetadataRes,
) {
    let metadata = &active_metadata.metadata;

    let hierarchies = (0..metadata.hierarchies)
        .map(|hierarchy| {
            let cell_size = metadata.cell_size(hierarchy);
            let spacing = metadata.cell_spacing(cell_size);

            Hierarchy { cell_size, spacing }
        })
        .collect_vec();

    queue.write_buffer(
        &metadata_bind_group_data.buffer,
        0,
        bytemuck::bytes_of(&(hierarchies.len() as u32)),
    );

    queue.write_buffer(
        &metadata_bind_group_data.buffer,
        std::mem::size_of::<u32>() as wgpu::BufferAddress,
        bytemuck::cast_slice(&hierarchies),
    );
}

pub(super) fn setup(world: &mut World) {
    let device = world.get_resource::<Device>().unwrap();
    let metadata_bind_group_data = MetadataBindGroupData::new(device);
    world.insert_resource(metadata_bind_group_data);
}
