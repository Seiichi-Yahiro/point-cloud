use bevy_ecs::prelude::*;
use itertools::Itertools;

use crate::plugins::metadata::ActiveMetadata;
use crate::plugins::wgpu::{Device, Queue, WgpuWrapper};

#[repr(C)]
#[derive(Debug, Default, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct Hierarchy {
    cell_size: f32,
    spacing: f32,
}

#[derive(Resource)]
pub struct MetadataBuffer {
    pub buffer: WgpuWrapper<wgpu::Buffer>,
    capacity: usize,
}

impl MetadataBuffer {
    fn new(device: &wgpu::Device, capacity: usize) -> Self {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("metadata-buffer"),
            size: (std::mem::size_of::<u32>() + std::mem::size_of::<Hierarchy>() * capacity)
                as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            buffer: WgpuWrapper(buffer),
            capacity,
        }
    }
}

pub(in crate::plugins) fn create_metadata_buffer(mut commands: Commands, device: Res<Device>) {
    let buffer = MetadataBuffer::new(&device, 0);
    commands.insert_resource(buffer);
}

pub(in crate::plugins) fn update_metadata_buffer(
    device: Res<Device>,
    queue: Res<Queue>,
    mut metadata_buffer: ResMut<MetadataBuffer>,
    active_metadata: ActiveMetadata,
) {
    let metadata = active_metadata.get();

    let hierarchies = (0..metadata.hierarchies)
        .map(|hierarchy| {
            let cell_size = metadata.config.cell_size(hierarchy);
            let spacing = metadata.config.cell_spacing(cell_size);

            Hierarchy { cell_size, spacing }
        })
        .collect_vec();

    if hierarchies.len() != metadata_buffer.capacity {
        *metadata_buffer = MetadataBuffer::new(&device, hierarchies.len());
    }

    queue.write_buffer(
        &metadata_buffer.buffer,
        0,
        bytemuck::bytes_of(&(hierarchies.len() as u32)),
    );

    if !hierarchies.is_empty() {
        queue.write_buffer(
            &metadata_buffer.buffer,
            std::mem::size_of::<u32>() as wgpu::BufferAddress,
            bytemuck::cast_slice(&hierarchies),
        );
    }
}
