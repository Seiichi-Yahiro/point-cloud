mod pipelines;

use crate::plugins::camera::{Camera, Visibility};
use crate::plugins::cell::shader::{
    CellIndirectBuffer, CellInputVertexBuffer, CellOutputVertexBuffer,
};
use crate::plugins::cell::{CellHeader, StreamState};
use crate::plugins::render::bind_groups::camera::CameraBindGroup;
use crate::plugins::render::bind_groups::cell::CellBindGroup;
use crate::plugins::render::bind_groups::resource::ResourceBindGroup;
use crate::plugins::render::bind_groups::texture::TextureBindGroup;
use crate::plugins::render::point::pipelines::compute::PointComputePipeLine;
use crate::plugins::render::point::pipelines::render::PointRenderPipeline;
use crate::plugins::render::{bind_groups, BindGroupLayoutSet, BindGroupSet, PipelineSet};
use crate::plugins::wgpu::{CommandEncoders, GlobalRenderResources, RenderPassSet};
use crate::plugins::winit::Render;
use crate::transform::Transform;
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
                    bind_groups::texture::create_bind_group_layout,
                )
                    .in_set(BindGroupLayoutSet),
                (
                    bind_groups::resource::create_bind_group,
                    bind_groups::camera::create_bind_group,
                    bind_groups::cell::create_bind_group,
                    bind_groups::texture::create_bind_group,
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
                bind_groups::texture::create_bind_group,
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
    camera_query: Query<(&CameraBindGroup, &Transform), With<Camera>>,
    cell_query: Query<(
        &CellBindGroup,
        &CellInputVertexBuffer,
        &CellOutputVertexBuffer,
        &CellIndirectBuffer,
        &CellHeader,
        &Visibility,
    )>,
    resource_bind_group: Res<ResourceBindGroup>,
    texture_bind_group: Res<TextureBindGroup>,
    stream_state: Res<State<StreamState>>,
) {
    global_render_resources
        .encoders
        .encode::<PointRenderPlugin>(|encoder| {
            for (camera_bind_group, camera_transform) in camera_query.iter() {
                let cell_groups = cell_query
                    .iter()
                    .filter(|(_, _, _, _, _, visibility)| visibility.visible)
                    .map(|it| {
                        (
                            it,
                            it.4 .0.pos.distance(camera_transform.translation) as u32,
                        )
                    })
                    .sorted_unstable_by_key(|(_, distance)| *distance)
                    .group_by(|(_, distance)| distance.checked_ilog2().unwrap_or(0));

                for (_, group) in &cell_groups {
                    let cells = group.map(|(it, _)| (it.0, it.1, it.2, it.3)).collect_vec();

                    if *stream_state == StreamState::Enabled {
                        for (_bind_group, _input, _output, indirect) in &cells {
                            encoder.clear_buffer(
                                &indirect.0,
                                std::mem::size_of::<u32>() as wgpu::BufferAddress,
                                Some(std::mem::size_of::<u32>() as wgpu::BufferAddress),
                            );
                        }

                        let mut compute_pass =
                            encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                                label: Some("point-compute-pass"),
                                timestamp_writes: None,
                            });

                        compute_pass.set_pipeline(&compute_pipeline.0);
                        compute_pass.set_bind_group(0, &camera_bind_group.0, &[]);
                        compute_pass.set_bind_group(1, &resource_bind_group.0, &[]);
                        compute_pass.set_bind_group(3, &texture_bind_group.0, &[]);

                        for (bind_group, input, _output, _indirect) in &cells {
                            compute_pass.set_bind_group(2, &bind_group.0, &[]);
                            compute_pass.dispatch_workgroups(input.len().div_ceil(128), 1, 1);
                        }

                        drop(compute_pass);
                    }

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
                    render_pass.set_bind_group(0, &camera_bind_group.0, &[]);
                    render_pass.set_bind_group(1, &resource_bind_group.0, &[]);

                    for (_bind_group, _input, output, indirect) in &cells {
                        render_pass.set_vertex_buffer(0, output.0.slice(..));
                        render_pass.draw_indirect(&indirect.0, 0);
                    }

                    drop(render_pass);
                }
            }
        });
}
