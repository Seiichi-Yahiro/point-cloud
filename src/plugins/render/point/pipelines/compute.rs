use crate::plugins::render::bind_groups::camera::CameraBindGroupLayout;
use crate::plugins::render::bind_groups::cell::CellBindGroupLayout;
use crate::plugins::render::bind_groups::resource::ResourceBindGroupLayout;
use crate::plugins::render::bind_groups::texture::TextureBindGroupLayout;
use crate::plugins::wgpu::{Device, WgpuWrapper};
use bevy_ecs::change_detection::Res;
use bevy_ecs::prelude::{Commands, Resource};

#[derive(Resource)]
pub struct PointComputePipeLine(pub WgpuWrapper<wgpu::ComputePipeline>);

pub fn create_compute_pipeline(
    mut commands: Commands,
    device: Res<Device>,
    camera_bind_group_layout: Res<CameraBindGroupLayout>,
    resource_bind_group_layout: Res<ResourceBindGroupLayout>,
    cell_bind_group_layout: Res<CellBindGroupLayout>,
    texture_bind_group_layout: Res<TextureBindGroupLayout>,
) {
    let compute_shader = device.create_shader_module(wgpu::include_wgsl!("compute.wgsl"));

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("point-compute-pipeline-layout"),
        bind_group_layouts: &[
            &camera_bind_group_layout.0,
            &resource_bind_group_layout.0,
            &cell_bind_group_layout.0,
            &texture_bind_group_layout.0,
        ],
        push_constant_ranges: &[],
    });

    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("point-compute-pipeline"),
        layout: Some(&pipeline_layout),
        module: &compute_shader,
        entry_point: "main",
    });

    commands.insert_resource(PointComputePipeLine(WgpuWrapper(pipeline)));
}
