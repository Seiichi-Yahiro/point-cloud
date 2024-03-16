use glam::Vec3;
use wgpu::util::DeviceExt;

use crate::camera::Camera;

#[repr(C)]
#[derive(Debug, Default, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    pub position: Vec3,
    pub color: [u8; 4],
}

impl Vertex {
    fn desc() -> wgpu::VertexBufferLayout<'static> {
        const ATTRIBS: [wgpu::VertexAttribute; 2] =
            wgpu::vertex_attr_array![0 => Float32x3, 1 => Unorm8x4];

        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &ATTRIBS,
        }
    }
}

#[derive(Debug)]
struct VertexBuffer {
    buffer: wgpu::Buffer,
    len: u32,
}

pub struct PointRenderer {
    camera: Camera,
    pipeline: wgpu::RenderPipeline,
    view_projection_uniform: wgpu::Buffer,
    view_projection_bind_group: wgpu::BindGroup,
    point_buffers: Vec<VertexBuffer>,
}

impl PointRenderer {
    pub fn new(device: &wgpu::Device, config: &wgpu::SurfaceConfiguration) -> Self {
        let camera = Camera::new(Vec3::new(0.0, 0.0, 2.0), Vec3::ZERO);

        let shader = device.create_shader_module(wgpu::include_wgsl!("point.wgsl"));

        let view_projection_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("view-projection-bind-group-layout"),
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

        let view_projection_uniform =
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("view-projection-uniform"),
                contents: bytemuck::cast_slice(&[
                    camera.get_view_matrix(),
                    camera.get_projection_matrix(),
                ]),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });

        let view_projection_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("view-projection-bind-group"),
            layout: &view_projection_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: view_projection_uniform.as_entire_binding(),
            }],
        });

        // TODO remove
        let vertex_buffer = VertexBuffer {
            buffer: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("vertex-buffer"),
                contents: bytemuck::cast_slice(&[
                    Vertex {
                        position: Vec3::ZERO,
                        color: [255, 0, 0, 255],
                    },
                    Vertex {
                        position: Vec3::new(0.5, 0.5, -1.0),
                        color: [0, 255, 0, 255],
                    },
                ]),
                usage: wgpu::BufferUsages::VERTEX,
            }),
            len: 2,
        };

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
                buffers: &[Vertex::desc()],
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
            depth_stencil: None, // TODO
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

        Self {
            camera,
            pipeline,
            view_projection_uniform,
            view_projection_bind_group,
            point_buffers: vec![vertex_buffer],
        }
    }

    pub fn resize(&mut self, queue: &wgpu::Queue, config: &wgpu::SurfaceConfiguration) {
        self.camera.projection.aspect_ratio = config.width as f32 / config.height as f32;
        queue.write_buffer(
            &self.view_projection_uniform,
            0,
            bytemuck::cast_slice(&[
                self.camera.get_view_matrix(),
                self.camera.get_view_projection_matrix(),
            ]),
        )
    }

    pub fn update(&mut self) {}

    pub fn draw(&self, view: &wgpu::TextureView, encoder: &mut wgpu::CommandEncoder) {
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
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
            depth_stencil_attachment: None, // TODO
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_bind_group(0, &self.view_projection_bind_group, &[]);

        for point_buffer in &self.point_buffers {
            render_pass.set_vertex_buffer(0, point_buffer.buffer.slice(..));
            render_pass.draw(0..4, 0..point_buffer.len);
        }
    }
}
