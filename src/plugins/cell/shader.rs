use bevy_ecs::prelude::*;
use glam::{IVec3, Vec3};
use itertools::Itertools;
use wgpu::util::DeviceExt;

use point_converter::cell::CellId;

use crate::plugins::camera::Camera;
use crate::plugins::cell::frustums::StreamingFrustums;
use crate::plugins::cell::CellHeader;
use crate::plugins::metadata::ActiveMetadata;
use crate::plugins::render::point::Point;
use crate::plugins::wgpu::{Device, Queue, WgpuWrapper};
use crate::transform::Transform;

#[repr(C)]
#[derive(Debug, Default, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct Cell {
    hierarchy: u32,
    index: IVec3,
}

#[derive(Bundle)]
pub struct CellBufferBundle {
    input: CellInputVertexBuffer,
    output: CellOutputVertexBuffer,
    indirect: CellIndirectBuffer,
    id: CellIdBuffer,
}

impl CellBufferBundle {
    pub fn new(device: &wgpu::Device, cell: &point_converter::cell::Cell) -> Self {
        let points = cell
            .all_points()
            .map(|it| Point {
                position: it.pos,
                color: it.color,
            })
            .collect_vec();

        Self {
            input: CellInputVertexBuffer::new(device, &points),
            output: CellOutputVertexBuffer::new(device, points.len()),
            indirect: CellIndirectBuffer::new(device),
            id: CellIdBuffer::new(device, cell.header().id),
        }
    }
}

#[derive(Component)]
pub struct CellInputVertexBuffer {
    pub buffer: WgpuWrapper<wgpu::Buffer>,
    len: u32,
}

impl CellInputVertexBuffer {
    pub fn new(device: &wgpu::Device, points: &[Point]) -> Self {
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("point-input-vertex-buffer"),
            contents: bytemuck::cast_slice(points),
            usage: wgpu::BufferUsages::STORAGE,
        });

        Self {
            buffer: WgpuWrapper(buffer),
            len: points.len() as u32,
        }
    }

    pub fn len(&self) -> u32 {
        self.len
    }
}

#[derive(Component)]
pub struct CellOutputVertexBuffer(pub WgpuWrapper<wgpu::Buffer>);

impl CellOutputVertexBuffer {
    pub fn new(device: &wgpu::Device, number_of_points: usize) -> Self {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("point-output-vertex-buffer"),
            size: (std::mem::size_of::<Point>() * number_of_points) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::VERTEX,
            mapped_at_creation: false,
        });

        Self(WgpuWrapper(buffer))
    }
}

#[derive(Component)]
pub struct CellIndirectBuffer(pub WgpuWrapper<wgpu::Buffer>);

impl CellIndirectBuffer {
    pub fn new(device: &wgpu::Device) -> Self {
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("point-indirect-buffer"),
            contents: wgpu::util::DrawIndirectArgs {
                vertex_count: 4,
                instance_count: 0,
                first_vertex: 0,
                first_instance: 0,
            }
            .as_bytes(),
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::INDIRECT
                | wgpu::BufferUsages::COPY_DST,
        });

        Self(WgpuWrapper(buffer))
    }
}

#[derive(Component)]
pub struct CellIdBuffer(pub WgpuWrapper<wgpu::Buffer>);

impl CellIdBuffer {
    pub fn new(device: &wgpu::Device, cell_id: CellId) -> Self {
        let cell = Cell {
            hierarchy: cell_id.hierarchy,
            index: cell_id.index,
        };

        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("cell-buffer"),
            contents: bytemuck::bytes_of(&cell),
            usage: wgpu::BufferUsages::UNIFORM,
        });

        Self(WgpuWrapper(buffer))
    }
}

#[derive(Resource)]
pub struct LoadedCellsBuffer {
    pub buffer: WgpuWrapper<wgpu::Buffer>,
    pub capacity: usize,
}

impl LoadedCellsBuffer {
    fn new(capacity: usize, device: &wgpu::Device) -> Self {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("loaded-cells-buffer"),
            size: (std::mem::size_of::<u32>() + std::mem::size_of::<Cell>() * capacity)
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

#[derive(Resource)]
pub struct FrustumsBuffer {
    pub buffer: WgpuWrapper<wgpu::Buffer>,
    pub capacity: usize,
}

impl FrustumsBuffer {
    fn new(capacity: usize, device: &wgpu::Device) -> Self {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("frustums-buffer"),
            size: (std::mem::size_of::<f32>() * capacity) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            buffer: WgpuWrapper(buffer),
            capacity,
        }
    }
}

#[derive(Resource)]
pub struct FrustumsSettings {
    pub size_by_distance: bool,
    pub max_hierarchy: u32,
    pub buffer: WgpuWrapper<wgpu::Buffer>,
}

impl FrustumsSettings {
    fn new(device: &wgpu::Device) -> Self {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("frustums-settings-buffer"),
            size: (std::mem::size_of::<u32>() + std::mem::size_of::<u32>()) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            size_by_distance: true,
            max_hierarchy: 0,
            buffer: WgpuWrapper(buffer),
        }
    }
}

pub(super) fn create_loaded_cells_buffer(mut commands: Commands, device: Res<Device>) {
    commands.insert_resource(LoadedCellsBuffer::new(0, &device));
}

pub(super) fn create_frustums_buffer(mut commands: Commands, device: Res<Device>) {
    commands.insert_resource(FrustumsBuffer::new(1, &device));
}

pub(super) fn create_frustums_settings_buffer(mut commands: Commands, device: Res<Device>) {
    commands.insert_resource(FrustumsSettings::new(&device));
}

pub(super) fn update_loaded_cells_buffer(
    queue: Res<Queue>,
    device: Res<Device>,
    mut loaded_cells_buffer: ResMut<LoadedCellsBuffer>,
    cell_query: Query<&CellHeader>,
) {
    let mut loaded_cells = cell_query
        .iter()
        .map(|cell_header| Cell {
            hierarchy: cell_header.0.id.hierarchy,
            index: cell_header.0.id.index,
        })
        .collect_vec();

    loaded_cells.sort_unstable_by(|a, b| {
        a.hierarchy
            .cmp(&b.hierarchy)
            .then(a.index.x.cmp(&b.index.x))
            .then(a.index.y.cmp(&b.index.y))
            .then(a.index.z.cmp(&b.index.z))
    });

    if loaded_cells.len() > loaded_cells_buffer.capacity {
        *loaded_cells_buffer = LoadedCellsBuffer::new(loaded_cells.len() + 50, &device);
    }

    queue.write_buffer(
        &loaded_cells_buffer.buffer,
        0,
        bytemuck::bytes_of(&(loaded_cells.len() as u32)),
    );

    queue.write_buffer(
        &loaded_cells_buffer.buffer,
        std::mem::size_of::<u32>() as wgpu::BufferAddress,
        bytemuck::cast_slice(&loaded_cells),
    );
}

pub(super) fn update_frustums_buffer(
    device: Res<Device>,
    queue: Res<Queue>,
    mut frustums_buffer: ResMut<FrustumsBuffer>,
    frustums_query: Query<
        (&StreamingFrustums, &Transform),
        (With<Camera>, Changed<StreamingFrustums>),
    >,
) {
    for (frustums, camera_transform) in frustums_query.iter() {
        let far_distances = frustums
            .iter()
            .map(|frustum| frustum.far.iter().sum::<Vec3>() / frustum.far.iter().len() as f32)
            .map(|far_center| camera_transform.translation.distance(far_center))
            .collect_vec();

        if far_distances.is_empty() {
            continue;
        }

        if frustums_buffer.capacity != far_distances.len() {
            *frustums_buffer = FrustumsBuffer::new(far_distances.len(), &device);
        }

        queue.write_buffer(
            &frustums_buffer.buffer,
            0,
            bytemuck::cast_slice(&far_distances),
        );
    }
}

pub(super) fn set_frustums_settings_max_hierarchy(
    active_metadata: ActiveMetadata,
    mut frustums_settings: ResMut<FrustumsSettings>,
) {
    frustums_settings.max_hierarchy = active_metadata.get().hierarchies.saturating_sub(1);
}

pub(super) fn update_frustums_settings_buffer(
    queue: Res<Queue>,
    frustums_settings: Res<FrustumsSettings>,
) {
    queue.write_buffer(
        &frustums_settings.buffer,
        0,
        bytemuck::bytes_of(&(frustums_settings.size_by_distance as u32)),
    );

    queue.write_buffer(
        &frustums_settings.buffer,
        std::mem::size_of::<u32>() as wgpu::BufferAddress,
        bytemuck::bytes_of(&frustums_settings.max_hierarchy),
    );
}
