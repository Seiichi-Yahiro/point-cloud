use crate::plugins::wgpu::{Device, GlobalDepthTexture, WgpuWrapper};
use bevy_ecs::prelude::*;

#[derive(Resource)]
pub struct TextureBindGroupLayout(pub WgpuWrapper<wgpu::BindGroupLayout>);

pub fn create_bind_group_layout(mut commands: Commands, device: Res<Device>) {
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("point-texture-bind-group-layout"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0, // depth map
            visibility: wgpu::ShaderStages::all(),
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Depth,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        }],
    });

    commands.insert_resource(TextureBindGroupLayout(WgpuWrapper(layout)));
}

#[derive(Resource)]
pub struct TextureBindGroup(pub WgpuWrapper<wgpu::BindGroup>);

pub fn create_bind_group(
    mut commands: Commands,
    device: Res<Device>,
    layout: Res<TextureBindGroupLayout>,
    depth_texture: Res<GlobalDepthTexture>,
) {
    if !depth_texture.is_changed() {
        return;
    }

    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("point-texture-bind-group"),
        layout: &layout.0,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(&depth_texture.view),
        }],
    });

    commands.insert_resource(TextureBindGroup(WgpuWrapper(group)));
}
