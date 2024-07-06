use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use glam::Vec3;

use crate::plugins::camera::{Camera, ViewBindGroupLayout, Visibility};
use crate::plugins::cell::shader::{
    CellBindGroupData, CellBindGroupLayout, CellsBindGroup, CellsBindGroupLayout,
    PointVertexBuffers, PointVertexBuffersBindGroupLayout,
};
use crate::plugins::metadata::shader::MetadataBindGroupData;
use crate::plugins::wgpu::{
    CommandEncoders, Device, GlobalRenderResources, Render, RenderPassSet, SurfaceConfig,
};
use crate::texture::Texture;

#[repr(C)]
#[derive(Debug, Default, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Point {
    pub position: Vec3,
    pub color: [u8; 4],
}

impl Point {
    pub fn instance_desc() -> wgpu::VertexBufferLayout<'static> {
        const ATTRIBS: [wgpu::VertexAttribute; 2] =
            wgpu::vertex_attr_array![0 => Float32x3, 1 => Unorm8x4];

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
        app.add_systems(Startup, (create_render_pipeline, create_compute_pipeline))
            .add_systems(Render, draw.in_set(RenderPassSet));

        app.world
            .get_resource_mut::<CommandEncoders>()
            .unwrap()
            .register::<Self>();
    }
}

#[derive(Resource)]
struct ComputeResources {
    pipeline: wgpu::ComputePipeline,
}

fn create_compute_pipeline(
    mut commands: Commands,
    device: Res<Device>,
    view_projection_bind_group_layout: Res<ViewBindGroupLayout>,
    point_vertex_buffers_bind_group_layout: Res<PointVertexBuffersBindGroupLayout>,
) {
    let compute_shader = device.create_shader_module(wgpu::include_wgsl!("point/cull.wgsl"));

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("point-frustum-cull-pipeline-layout"),
        bind_group_layouts: &[
            &view_projection_bind_group_layout,
            &point_vertex_buffers_bind_group_layout.0,
        ],
        push_constant_ranges: &[],
    });

    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("point-frustum-cull-pipeline"),
        layout: Some(&pipeline_layout),
        module: &compute_shader,
        entry_point: "main",
    });

    commands.insert_resource(ComputeResources { pipeline });
}

#[derive(Resource)]
struct RenderResources {
    pipeline: wgpu::RenderPipeline,
}

fn create_render_pipeline(
    mut commands: Commands,
    device: Res<Device>,
    config: Res<SurfaceConfig>,
    view_projection_bind_group_layout: Res<ViewBindGroupLayout>,
    metadata_bind_group_data: Res<MetadataBindGroupData>,
    cell_bind_group_layout: Res<CellBindGroupLayout>,
    cells_bind_group_layout: Res<CellsBindGroupLayout>,
) {
    let shader = device.create_shader_module(wgpu::include_wgsl!("point/point.wgsl"));

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("point-renderer-pipeline-layout"),
        bind_group_layouts: &[
            &view_projection_bind_group_layout,
            &metadata_bind_group_data.layout,
            &cell_bind_group_layout.0,
            &cells_bind_group_layout.0,
        ],
        push_constant_ranges: &[],
    });

    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("point-renderer-pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: "vs_main",
            buffers: &[Point::instance_desc()],
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleStrip,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            polygon_mode: wgpu::PolygonMode::Fill,
            unclipped_depth: false,
            conservative: false,
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: Texture::DEPTH_TEXTURE_FORMAT,
            depth_write_enabled: true,
            depth_compare: wgpu::CompareFunction::Less,
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState {
            count: 1,
            mask: !0,
            alpha_to_coverage_enabled: false,
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: "fs_main",
            targets: &[Some(wgpu::ColorTargetState {
                format: config.format,
                blend: Some(wgpu::BlendState::REPLACE),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview: None,
    });

    commands.insert_resource(RenderResources { pipeline });
}

fn draw(
    mut global_render_resources: GlobalRenderResources,
    local_render_resources: Res<RenderResources>,
    compute_resources: Res<ComputeResources>,
    camera_query: Query<&Camera>,
    buffers: Query<(&PointVertexBuffers, &CellBindGroupData, &Visibility)>,
    metadata_bind_group_data: Res<MetadataBindGroupData>,
    cells_bind_group_data: Res<CellsBindGroup>,
) {
    global_render_resources
        .encoders
        .encode::<PointRenderPlugin>(|encoder| {
            for (vertex_buffers, _cell_bind_group_data, visibility) in &buffers {
                if !visibility.visible {
                    continue;
                }

                encoder.clear_buffer(
                    &vertex_buffers.indirect,
                    std::mem::size_of::<u32>() as wgpu::BufferAddress,
                    Some(std::mem::size_of::<u32>() as wgpu::BufferAddress),
                );
            }

            let mut compute_pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("point-compute-pass"),
                timestamp_writes: None,
            });

            compute_pass.set_pipeline(&compute_resources.pipeline);

            for camera in camera_query.iter() {
                compute_pass.set_bind_group(0, &camera.bind_group, &[]);

                for (vertex_buffers, _cell_bind_group_data, visibility) in &buffers {
                    if !visibility.visible {
                        continue;
                    }

                    compute_pass.set_bind_group(1, &vertex_buffers.bind_group, &[]);

                    compute_pass.dispatch_workgroups(
                        vertex_buffers.input_length().div_ceil(128),
                        1,
                        1,
                    );
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

            for camera in camera_query.iter() {
                render_pass.set_pipeline(&local_render_resources.pipeline);
                render_pass.set_bind_group(0, &camera.bind_group, &[]);
                render_pass.set_bind_group(1, &metadata_bind_group_data.group, &[]);
                render_pass.set_bind_group(3, &cells_bind_group_data.0, &[]);

                for (vertex_buffers, cell_bind_group_data, visibility) in &buffers {
                    if !visibility.visible {
                        continue;
                    }

                    render_pass.set_bind_group(2, &cell_bind_group_data.group, &[]);
                    render_pass.set_vertex_buffer(0, vertex_buffers.output.slice(..));
                    render_pass.draw_indirect(&vertex_buffers.indirect, 0);
                }
            }

            drop(render_pass);
        });
}
