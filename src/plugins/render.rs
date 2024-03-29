use std::ops::{Deref, DerefMut};

use bevy_app::AppExit;
use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use wgpu::SurfaceError;

use crate::plugins::camera::{Camera, ViewProjectionBindGroupLayout};
use crate::plugins::render::vertex::{Vertex, VertexBuffer};
use crate::plugins::wgpu::{Device, Queue, Surface, SurfaceConfig};
use crate::plugins::winit::{Window, WindowResized};
use crate::texture::Texture;

pub mod vertex;

pub struct RenderPlugin;

impl Plugin for RenderPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup)
            .add_systems(PreUpdate, update_depth_texture)
            .configure_sets(Last, RenderSet.run_if(resource_exists::<SurfaceTexture>))
            .add_systems(
                Last,
                (
                    prepare.before(RenderSet),
                    draw.in_set(RenderSet).in_set(MainDraw),
                    present
                        .after(RenderSet)
                        .run_if(resource_exists::<SurfaceTexture>),
                ),
            );
    }
}

#[derive(Resource)]
struct RenderResources {
    pipeline: wgpu::RenderPipeline,
    depth_texture: Texture,
}

fn setup(
    mut commands: Commands,
    device: Res<Device>,
    config: Res<SurfaceConfig>,
    view_projection_bind_group_layout: Res<ViewProjectionBindGroupLayout>,
) {
    let shader = device.create_shader_module(wgpu::include_wgsl!("render/point.wgsl"));

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("point-renderer-pipeline-layout"),
        bind_group_layouts: &[&view_projection_bind_group_layout],
        push_constant_ranges: &[],
    });

    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("point-renderer-pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: "vs_main",
            buffers: &[Vertex::instance_desc()],
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
        pipeline,
        depth_texture: Texture::create_depth_texture(&device, config.width, config.height),
    });
}

fn update_depth_texture(
    mut window_resized: EventReader<WindowResized>,
    mut render_resources: ResMut<RenderResources>,
    device: Res<Device>,
) {
    if let Some(resized) = window_resized.read().last() {
        render_resources.depth_texture = Texture::create_depth_texture(
            &device,
            resized.physical_size.width,
            resized.physical_size.height,
        );
    }
}

#[derive(Resource)]
pub struct SurfaceTexture(wgpu::SurfaceTexture);

impl Deref for SurfaceTexture {
    type Target = wgpu::SurfaceTexture;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Resource)]
pub struct TextureView(wgpu::TextureView);

impl Deref for TextureView {
    type Target = wgpu::TextureView;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Resource)]
pub struct CommandEncoder(wgpu::CommandEncoder);

impl Deref for CommandEncoder {
    type Target = wgpu::CommandEncoder;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for CommandEncoder {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

fn prepare(world: &mut World) {
    let frame = match world
        .get_resource::<Surface>()
        .unwrap()
        .get_current_texture()
    {
        Ok(frame) => frame,
        Err(err) => {
            match err {
                SurfaceError::Timeout => {
                    log::warn!("Timeout while trying to acquire next frame!")
                }
                SurfaceError::Outdated => {
                    // happens when window gets minimized
                }
                SurfaceError::Lost => {
                    world
                        .send_event(WindowResized {
                            physical_size: world.get_resource::<Window>().unwrap().inner_size(),
                        })
                        .unwrap();
                }
                SurfaceError::OutOfMemory => {
                    log::error!("Application is out of memory!");
                    world.send_event(AppExit);
                }
            }

            return;
        }
    };

    let view = frame
        .texture
        .create_view(&wgpu::TextureViewDescriptor::default());

    let encoder = world
        .get_resource::<Device>()
        .unwrap()
        .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());

    world.insert_resource(SurfaceTexture(frame));
    world.insert_resource(TextureView(view));
    world.insert_resource(CommandEncoder(encoder));
}

#[derive(SystemSet, Hash, Eq, PartialEq, Copy, Clone, Debug)]
pub struct RenderSet;

#[derive(SystemSet, Hash, Eq, PartialEq, Copy, Clone, Debug)]
pub struct MainDraw;

fn draw(
    view: Res<TextureView>,
    mut encoder: ResMut<CommandEncoder>,
    render_res: Res<RenderResources>,
    camera_query: Query<&Camera>,
    vertex_buffers: Query<&VertexBuffer>,
) {
    for camera in camera_query.iter() {
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view, // TODO render_target should come from camera
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.16,
                        g: 0.16,
                        b: 0.16,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &render_res.depth_texture.view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        render_pass.set_pipeline(&render_res.pipeline);
        render_pass.set_bind_group(0, &camera.bind_group, &[]);

        for vertex_buffer in vertex_buffers.iter() {
            render_pass.set_vertex_buffer(0, vertex_buffer.buffer.slice(..));
            render_pass.draw(0..4, 0..vertex_buffer.len());
        }
    }
}

fn present(world: &mut World) {
    let frame = world.remove_resource::<SurfaceTexture>().unwrap().0;
    let _view = world.remove_resource::<TextureView>().unwrap();
    let encoder = world.remove_resource::<CommandEncoder>().unwrap().0;

    world
        .get_resource::<Queue>()
        .unwrap()
        .submit(Some(encoder.finish()));

    frame.present();
}
