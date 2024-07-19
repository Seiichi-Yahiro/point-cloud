use bevy_app::*;
use bevy_ecs::prelude::*;
use glam::Vec3;

use crate::plugins::camera::Camera;
use crate::plugins::render::bind_groups::camera::{CameraBindGroup, CameraBindGroupLayout};
use crate::plugins::render::vertex::VertexBuffer;
use crate::plugins::render::PipelineSet;
use crate::plugins::wgpu::{
    CommandEncoders, Device, GlobalRenderResources, Render, RenderPassSet, SurfaceConfig,
    WgpuWrapper,
};
use crate::texture::Texture;

pub mod utils;

#[repr(C)]
#[derive(Debug, Default, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Line {
    pub start: Vec3,
    pub end: Vec3,
    pub color: [u8; 4],
}

impl Line {
    pub fn instance_desc() -> wgpu::VertexBufferLayout<'static> {
        const ATTRIBS: [wgpu::VertexAttribute; 3] =
            wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Unorm8x4];

        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &ATTRIBS,
        }
    }
}

pub struct LineRenderPlugin;

impl Plugin for LineRenderPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup.in_set(PipelineSet))
            .add_systems(Render, draw.in_set(RenderPassSet));

        app.world_mut()
            .get_resource_mut::<CommandEncoders>()
            .unwrap()
            .register::<Self>();
    }
}

#[derive(Resource)]
struct RenderResources {
    pipeline: WgpuWrapper<wgpu::RenderPipeline>,
}

fn setup(
    mut commands: Commands,
    device: Res<Device>,
    config: Res<SurfaceConfig>,
    camera_bind_group_layout: Res<CameraBindGroupLayout>,
) {
    let shader = device.create_shader_module(wgpu::include_wgsl!("line/line.wgsl"));

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("line-renderer-pipeline-layout"),
        bind_group_layouts: &[&camera_bind_group_layout.0],
        push_constant_ranges: &[],
    });

    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("line-renderer-pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: "vs_main",
            buffers: &[Line::instance_desc()],
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

    commands.insert_resource(RenderResources {
        pipeline: WgpuWrapper(pipeline),
    });
}

fn draw(
    mut global_render_resources: GlobalRenderResources,
    local_render_resources: Res<RenderResources>,
    camera_query: Query<&CameraBindGroup, With<Camera>>,
    vertex_buffers: Query<&VertexBuffer<Line>>,
) {
    global_render_resources
        .encoders
        .encode::<LineRenderPlugin>(|encoder| {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
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

            for camera_bind_group in camera_query.iter() {
                render_pass.set_pipeline(&local_render_resources.pipeline);
                render_pass.set_bind_group(0, &camera_bind_group.0, &[]);

                for vertex_buffer in vertex_buffers.iter() {
                    render_pass.set_vertex_buffer(0, vertex_buffer.buffer.slice(..));
                    render_pass.draw(0..4, 0..vertex_buffer.len());
                }
            }
        });
}
