mod pipelines;

use crate::plugins::camera::{Camera, Visibility};
use crate::plugins::cell::shader::{
    CellIndirectBuffer, CellInputVertexBuffer, CellOutputVertexBuffer,
};
use crate::plugins::render::bind_groups::camera::CameraBindGroup;
use crate::plugins::render::bind_groups::cell::CellBindGroup;
use crate::plugins::render::bind_groups::resource::ResourceBindGroup;
use crate::plugins::render::point::pipelines::compute::PointComputePipeLine;
use crate::plugins::render::point::pipelines::render::PointRenderPipeline;
use crate::plugins::render::{bind_groups, BindGroupLayoutSet, BindGroupSet, PipelineSet};
use crate::plugins::wgpu::{CommandEncoders, GlobalRenderResources, RenderPassSet};
use crate::plugins::winit::Render;
use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use glam::Vec3;
use itertools::Itertools;

#[repr(C)]
#[derive(Debug, Default, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Point {
    pub position: Vec3,
    pub color: [u8; 4],
}

impl Point {
    pub fn instance_desc() -> wgpu::VertexBufferLayout<'static> {
        const ATTRIBS: [wgpu::VertexAttribute; 2] =
            wgpu::vertex_attr_array![0 => Float32x3, 1 => Uint32];

        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &ATTRIBS,
        }
    }
}

pub struct PointRenderPlugin;

impl Plugin for PointRenderPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Startup,
            (
                (
                    bind_groups::resource::create_bind_group_layout,
                    bind_groups::camera::create_bind_group_layout,
                    bind_groups::cell::create_bind_group_layout,
                )
                    .in_set(BindGroupLayoutSet),
                (
                    bind_groups::resource::create_bind_group,
                    bind_groups::camera::create_bind_group,
                    bind_groups::cell::create_bind_group,
                )
                    .in_set(BindGroupSet),
                (
                    pipelines::compute::create_compute_pipeline,
                    pipelines::render::create_render_pipeline,
                )
                    .in_set(PipelineSet),
            ),
        )
        .add_systems(
            PostUpdate,
            (
                bind_groups::resource::create_bind_group,
                (
                    bind_groups::camera::update_bind_group,
                    bind_groups::camera::create_bind_group,
                ),
                (
                    bind_groups::cell::update_bind_group,
                    bind_groups::cell::create_bind_group,
                ),
            )
                .in_set(BindGroupSet),
        )
        .add_systems(Render, draw.in_set(RenderPassSet));

        app.world
            .get_resource_mut::<CommandEncoders>()
            .unwrap()
            .register::<Self>();
    }
}

fn draw(
    mut global_render_resources: GlobalRenderResources,
    compute_pipeline: Res<PointComputePipeLine>,
    render_pipeline: Res<PointRenderPipeline>,
    camera_query: Query<&CameraBindGroup, With<Camera>>,
    cell_query: Query<(
        &CellBindGroup,
        &CellInputVertexBuffer,
        &CellOutputVertexBuffer,
        &CellIndirectBuffer,
        &Visibility,
    )>,
    resource_bind_group: Res<ResourceBindGroup>,
) {
    global_render_resources
        .encoders
        .encode::<PointRenderPlugin>(|encoder| {
            let cells = cell_query
                .iter()
                .filter(|(_, _, _, _, visibility)| visibility.visible)
                .collect_vec();

            for (_cell_bind_group, _cell_input, _cell_output, cell_indirect, _) in &cells {
                encoder.clear_buffer(
                    &cell_indirect.0,
                    std::mem::size_of::<u32>() as wgpu::BufferAddress,
                    Some(std::mem::size_of::<u32>() as wgpu::BufferAddress),
                );
            }

            let mut compute_pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("point-compute-pass"),
                timestamp_writes: None,
            });

            compute_pass.set_pipeline(&compute_pipeline.0);

            for camera_bind_group in camera_query.iter() {
                compute_pass.set_bind_group(0, &camera_bind_group.0, &[]);
                compute_pass.set_bind_group(1, &resource_bind_group.0, &[]);

                for (cell_bind_group, cell_input, _cell_output, _cell_indirect, _) in &cells {
                    compute_pass.set_bind_group(2, &cell_bind_group.0, &[]);
                    compute_pass.dispatch_workgroups(cell_input.len().div_ceil(128), 1, 1);
                }
            }

            drop(compute_pass);

            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("point-render-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &global_render_resources.render_view.view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &global_render_resources.depth_texture.view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            render_pass.set_pipeline(&render_pipeline.0);

            for camera_bind_group in camera_query.iter() {
                render_pass.set_bind_group(0, &camera_bind_group.0, &[]);
                render_pass.set_bind_group(1, &resource_bind_group.0, &[]);

                for (_cell_bind_group, _cell_input, cell_output, cell_indirect, _) in &cells {
                    render_pass.set_vertex_buffer(0, cell_output.0.slice(..));
                    render_pass.draw_indirect(&cell_indirect.0, 0);
                }
            }

            drop(render_pass);
        });
}
