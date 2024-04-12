use crate::plugins::camera::Camera;
use crate::plugins::streaming::cell::CellData;
use crate::plugins::wgpu::{Device, Queue};
use bevy_ecs::prelude::*;
use glam::IVec3;
use itertools::Itertools;
use point_converter::cell::CellId;
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Debug, Default, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct Cell {
    hierarchy: u32,
    index: IVec3,
}

#[derive(Resource)]
pub struct CellBindGroupLayout(pub wgpu::BindGroupLayout);

impl CellBindGroupLayout {
    fn new(device: &wgpu::Device) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cell-bind-group-layout"),
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

        Self(layout)
    }
}

#[derive(Component)]
pub struct CellBindGroupData {
    buffer: wgpu::Buffer,
    pub group: wgpu::BindGroup,
}

impl CellBindGroupData {
    pub(super) fn new(
        device: &wgpu::Device,
        layout: &CellBindGroupLayout,
        cell_id: CellId,
    ) -> Self {
        let cell = Cell {
            hierarchy: cell_id.hierarchy,
            index: cell_id.index,
        };

        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("cell-buffer"),
            contents: bytemuck::bytes_of(&cell),
            usage: wgpu::BufferUsages::UNIFORM,
        });

        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cell-bind-group"),
            layout: &layout.0,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });

        Self { buffer, group }
    }
}

#[derive(Resource)]
pub struct VisibleCellsBindGroupData {
    pub layout: wgpu::BindGroupLayout,
    pub group: wgpu::BindGroup,
    buffer: wgpu::Buffer,
    buffer_capacity: usize,
}

impl VisibleCellsBindGroupData {
    fn new(device: &wgpu::Device) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("visible-cells-bind-group-layout"),
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

        let buffer_capacity = 100;
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
            label: Some("visible-cells-buffer"),
            size: (std::mem::size_of::<u32>() + std::mem::size_of::<Cell>() * capacity)
                as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("visible-cells-bind-group"),
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

pub(super) fn update_visible_cells_buffer(
    queue: Res<Queue>,
    device: Res<Device>,
    mut visible_cells_bind_group_data: ResMut<VisibleCellsBindGroupData>,
    camera_query: Query<&Camera>,
    cell_query: Query<&CellData>,
) {
    if let Ok(camera) = camera_query.get_single() {
        let mut visible_cells = cell_query
            .iter_many(camera.visible_entities.iter())
            .map(|cell_data| Cell {
                hierarchy: cell_data.id.hierarchy,
                index: cell_data.id.index,
            })
            .collect_vec();

        visible_cells.sort_unstable_by_key(|cell| cell.hierarchy);

        if visible_cells.len() > visible_cells_bind_group_data.buffer_capacity {
            visible_cells_bind_group_data.set_capacity(visible_cells.len() + 50, &device);
        }

        queue.write_buffer(
            &visible_cells_bind_group_data.buffer,
            0,
            bytemuck::bytes_of(&(visible_cells.len() as u32)),
        );

        queue.write_buffer(
            &visible_cells_bind_group_data.buffer,
            std::mem::size_of::<u32>() as wgpu::BufferAddress,
            bytemuck::cast_slice(&visible_cells),
        );
    }
}

pub(super) fn setup(world: &mut World) {
    let device = world.get_resource::<Device>().unwrap();

    let cell_bind_group_layout = CellBindGroupLayout::new(device);
    let visible_cells_bind_group_data = VisibleCellsBindGroupData::new(device);

    world.insert_resource(cell_bind_group_layout);
    world.insert_resource(visible_cells_bind_group_data);
}
