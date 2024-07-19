use crate::plugins::render::bind_groups::camera::CameraBindGroupLayout;
use crate::plugins::render::bind_groups::resource::ResourceBindGroupLayout;
use crate::plugins::render::point::Point;
use crate::plugins::wgpu::{Device, SurfaceConfig, WgpuWrapper};
use crate::texture::Texture;
use bevy_ecs::change_detection::Res;
use bevy_ecs::prelude::{Commands, Resource};

#[derive(Resource)]
pub struct PointRenderPipeline(pub WgpuWrapper<wgpu::RenderPipeline>);

pub fn create_render_pipeline(
    mut commands: Commands,
    device: Res<Device>,
    config: Res<SurfaceConfig>,
    camera_bind_group_layout: Res<CameraBindGroupLayout>,
    resource_bind_group_layout: Res<ResourceBindGroupLayout>,
) {
    let shader = device.create_shader_module(wgpu::include_wgsl!("render.wgsl"));

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("point-renderer-pipeline-layout"),
        bind_group_layouts: &[&camera_bind_group_layout.0, &resource_bind_group_layout.0],
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

    commands.insert_resource(PointRenderPipeline(WgpuWrapper(pipeline)));
}
