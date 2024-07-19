use crate::plugins::cell::shader::{
    CellIdBuffer, CellIndirectBuffer, CellInputVertexBuffer, CellOutputVertexBuffer,
};
use crate::plugins::cell::CellHeader;
use crate::plugins::wgpu::{Device, WgpuWrapper};
use bevy_ecs::prelude::*;

#[derive(Resource)]
pub struct CellBindGroupLayout(pub WgpuWrapper<wgpu::BindGroupLayout>);

pub fn create_bind_group_layout(mut commands: Commands, device: Res<Device>) {
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("point-cell-bind-group-layout"),
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

    commands.insert_resource(CellBindGroupLayout(WgpuWrapper(layout)));
}

#[derive(Component)]
pub struct CellBindGroup(pub WgpuWrapper<wgpu::BindGroup>);

impl CellBindGroup {
    fn new(
        device: &wgpu::Device,
        layout: &CellBindGroupLayout,
        input: &CellInputVertexBuffer,
        output: &CellOutputVertexBuffer,
        indirect: &CellIndirectBuffer,
        id: &CellIdBuffer,
    ) -> Self {
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("point-cell-bind-group"),
            layout: &layout.0,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: input.buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: output.0.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: indirect.0.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: id.0.as_entire_binding(),
                },
            ],
        });

        Self(WgpuWrapper(group))
    }
}

pub fn create_bind_group(
    mut commands: Commands,
    device: Res<Device>,
    layout: Res<CellBindGroupLayout>,
    query: Query<
        (
            Entity,
            &CellHeader,
            &CellInputVertexBuffer,
            &CellOutputVertexBuffer,
            &CellIndirectBuffer,
            &CellIdBuffer,
        ),
        Without<CellBindGroup>,
    >,
) {
    for (entity, header, input, output, indirect, id) in query.iter() {
        if header.0.total_number_of_points == 0 {
            continue;
        }
        let group = CellBindGroup::new(&device, &layout, input, output, indirect, id);
        commands.entity(entity).insert(group);
    }
}

pub fn update_bind_group(
    device: Res<Device>,
    layout: Res<CellBindGroupLayout>,
    mut query: Query<
        (
            &CellInputVertexBuffer,
            &CellOutputVertexBuffer,
            &CellIndirectBuffer,
            &CellIdBuffer,
            &mut CellBindGroup,
        ),
        Or<(
            Changed<CellInputVertexBuffer>,
            Changed<CellOutputVertexBuffer>,
            Changed<CellIndirectBuffer>,
            Changed<CellIdBuffer>,
        )>,
    >,
) {
    for (input, output, indirect, id, mut bind_group) in query.iter_mut() {
        *bind_group = CellBindGroup::new(&device, &layout, input, output, indirect, id);
    }
}
