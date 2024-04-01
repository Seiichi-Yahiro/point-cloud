use bevy_ecs::prelude::Component;
use wgpu::util::DeviceExt;

#[derive(Debug, Component)]
pub struct VertexBuffer<T> {
    pub buffer: wgpu::Buffer,
    len: u32,
    _phantom: std::marker::PhantomData<T>,
}

impl<T: bytemuck::NoUninit> VertexBuffer<T> {
    pub fn new(device: &wgpu::Device, vertices: &[T]) -> Self {
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("vertex-buffer"),
            contents: bytemuck::cast_slice(vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        Self {
            buffer,
            len: vertices.len() as u32,
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<T> VertexBuffer<T> {
    pub fn len(&self) -> u32 {
        self.len
    }
}
