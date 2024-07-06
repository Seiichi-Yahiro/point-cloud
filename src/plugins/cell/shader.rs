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
use crate::plugins::wgpu::{Device, Queue};
use crate::transform::Transform;

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
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0, // point-input-vertex-buffer
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1, // point-output-vertex-buffer
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2, // point-indirect-buffer
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3, // cell-buffer
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

        Self(layout)
    }
}

#[derive(Component)]
pub struct CellBindGroupData {
    input_length: u32,
    pub input_points: wgpu::Buffer,
    pub output_points: wgpu::Buffer,
    pub indirect_points: wgpu::Buffer,
    pub cell_buffer: wgpu::Buffer,
    pub bind_group: wgpu::BindGroup,
}

impl CellBindGroupData {
    pub fn input_length(&self) -> u32 {
        self.input_length
    }

    pub(super) fn new(
        device: &wgpu::Device,
        layout: &CellBindGroupLayout,
        points: &[Point],
        cell_id: CellId,
    ) -> Self {
        let input_vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("point-input-vertex-buffer"),
            contents: bytemuck::cast_slice(points),
            usage: wgpu::BufferUsages::STORAGE,
        });

        let output_vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("point-output-vertex-buffer"),
            size: std::mem::size_of_val(points) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::VERTEX,
            mapped_at_creation: false,
        });

        let indirect_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
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

        let cell = Cell {
            hierarchy: cell_id.hierarchy,
            index: cell_id.index,
        };

        let cell_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("cell-buffer"),
            contents: bytemuck::bytes_of(&cell),
            usage: wgpu::BufferUsages::UNIFORM,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cell-bind-group"),
            layout: &layout.0,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: input_vertex_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: output_vertex_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: indirect_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: cell_buffer.as_entire_binding(),
                },
            ],
        });

        Self {
            input_length: points.len() as u32,
            input_points: input_vertex_buffer,
            output_points: output_vertex_buffer,
            indirect_points: indirect_buffer,
            cell_buffer,
            bind_group,
        }
    }
}

#[derive(Resource)]
pub struct CellsBindGroupLayout(pub wgpu::BindGroupLayout);

impl CellsBindGroupLayout {
    fn new(device: &wgpu::Device) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cells-bind-group-layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0, // loaded cells
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1, // frustums
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2, // frustums settings
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

        Self(layout)
    }
}

#[derive(Resource)]
pub struct CellsBindGroup(pub wgpu::BindGroup);

impl CellsBindGroup {
    fn new(
        device: &wgpu::Device,
        layout: &CellsBindGroupLayout,
        loaded_cells: &LoadedCellsBuffer,
        frustums: &FrustumsBuffer,
        frustums_settings: &FrustumsSettings,
    ) -> Self {
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cells-bind-group"),
            layout: &layout.0,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: loaded_cells.buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: frustums.buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: frustums_settings.buffer.as_entire_binding(),
                },
            ],
        });

        Self(group)
    }
}

#[derive(Resource)]
pub struct LoadedCellsBuffer {
    buffer: wgpu::Buffer,
    capacity: usize,
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

        Self { buffer, capacity }
    }
}

#[derive(Resource)]
pub struct FrustumsBuffer {
    buffer: wgpu::Buffer,
    capacity: usize,
}

impl FrustumsBuffer {
    fn new(capacity: usize, device: &wgpu::Device) -> Self {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("frustums-buffer"),
            size: (std::mem::size_of::<f32>() * capacity) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self { buffer, capacity }
    }
}

#[derive(Resource)]
pub struct FrustumsSettings {
    pub size_by_distance: bool,
    pub max_hierarchy: u32,
    buffer: wgpu::Buffer,
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
            buffer,
        }
    }
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

pub(super) fn update_cells_bind_group(
    device: Res<Device>,
    mut cells_bind_group: ResMut<CellsBindGroup>,
    cells_bind_group_layout: Res<CellsBindGroupLayout>,
    loaded_cells_buffer: Res<LoadedCellsBuffer>,
    frustums_buffer: Res<FrustumsBuffer>,
    frustums_settings: Res<FrustumsSettings>,
) {
    if loaded_cells_buffer.is_changed() || frustums_buffer.is_changed() {
        *cells_bind_group = CellsBindGroup::new(
            &device,
            &cells_bind_group_layout,
            &loaded_cells_buffer,
            &frustums_buffer,
            &frustums_settings,
        );
    }
}

pub(super) fn setup(world: &mut World) {
    let device = world.get_resource::<Device>().unwrap();

    let cell_bind_group_layout = CellBindGroupLayout::new(device);

    let cells_bind_group_layout = CellsBindGroupLayout::new(device);
    let loaded_cells_buffer = LoadedCellsBuffer::new(0, device);
    let frustums_buffer = FrustumsBuffer::new(1, device);
    let frustums_settings = FrustumsSettings::new(device);
    let cells_bind_group = CellsBindGroup::new(
        device,
        &cells_bind_group_layout,
        &loaded_cells_buffer,
        &frustums_buffer,
        &frustums_settings,
    );

    world.insert_resource(cell_bind_group_layout);

    world.insert_resource(cells_bind_group_layout);
    world.insert_resource(loaded_cells_buffer);
    world.insert_resource(frustums_buffer);
    world.insert_resource(frustums_settings);
    world.insert_resource(cells_bind_group);
}
