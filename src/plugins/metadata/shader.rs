use bevy_ecs::prelude::*;
use itertools::Itertools;

use crate::plugins::metadata::ActiveMetadata;
use crate::plugins::wgpu::{Device, Queue};

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
    buffer_capacity: usize,
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

        let buffer_capacity = 0;
        let (buffer, group) = Self::create_buffer_and_bind_group(buffer_capacity, device, &layout);

        Self {
            layout,
            group,
            buffer,
            buffer_capacity,
        }
    }

    fn create_buffer_and_bind_group(
        capacity: usize,
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
    ) -> (wgpu::Buffer, wgpu::BindGroup) {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("metadata-buffer"),
            size: (std::mem::size_of::<u32>() + std::mem::size_of::<Hierarchy>() * capacity)
                as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("metadata-bind-group"),
            layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });

        (buffer, group)
    }

    fn set_capacity(&mut self, capacity: usize, device: &wgpu::Device) {
        let (buffer, group) = Self::create_buffer_and_bind_group(capacity, device, &self.layout);
        self.buffer = buffer;
        self.group = group;
        self.buffer_capacity = capacity;
    }
}

pub(in crate::plugins) fn update_metadata_buffer(
    device: Res<Device>,
    queue: Res<Queue>,
    mut metadata_bind_group_data: ResMut<MetadataBindGroupData>,
    active_metadata: ActiveMetadata,
) {
    let metadata = active_metadata.get().unwrap();

    let hierarchies = (0..metadata.hierarchies)
        .map(|hierarchy| {
            let cell_size = metadata.config.cell_size(hierarchy);
            let spacing = metadata.config.cell_spacing(cell_size);

            Hierarchy { cell_size, spacing }
        })
        .collect_vec();

    if hierarchies.len() != metadata_bind_group_data.buffer_capacity {
        metadata_bind_group_data.set_capacity(hierarchies.len(), &device);
    }

    queue.write_buffer(
        &metadata_bind_group_data.buffer,
        0,
        bytemuck::bytes_of(&(hierarchies.len() as u32)),
    );

    if !hierarchies.is_empty() {
        queue.write_buffer(
            &metadata_bind_group_data.buffer,
            std::mem::size_of::<u32>() as wgpu::BufferAddress,
            bytemuck::cast_slice(&hierarchies),
        );
    }
}

pub(in crate::plugins) fn setup(world: &mut World) {
    let device = world.get_resource::<Device>().unwrap();
    let metadata_bind_group_data = MetadataBindGroupData::new(device);
    world.insert_resource(metadata_bind_group_data);
}
