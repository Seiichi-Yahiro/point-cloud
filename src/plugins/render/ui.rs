use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_ecs::system::SystemState;
use bevy_egui::{EguiContextQuery, EguiContexts, EguiPlugin};
use egui_wgpu::{Renderer, ScreenDescriptor};

use crate::plugins::render::{GlobalRenderResources, RenderPassSet};
use crate::plugins::wgpu::{Device, Queue, SurfaceConfig};

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(EguiPlugin)
            .add_systems(Startup, setup)
            .add_systems(Last, (ui, draw).chain().in_set(RenderPassSet::UI));
    }
}

#[derive(Resource)]
struct EguiRenderer {
    renderer: Renderer,
}
fn setup(mut commands: Commands, device: Res<Device>, config: Res<SurfaceConfig>) {
    let egui_renderer = Renderer::new(&device, config.format, None, 1);

    commands.insert_resource(EguiRenderer {
        renderer: egui_renderer,
    });
}

fn ui(world: &mut World, params: &mut SystemState<EguiContexts>) {
    egui::Window::new("UI")
        .resizable(true)
        .default_size((150.0, 200.0))
        .vscroll(true)
        .show(&params.get_mut(world).ctx_mut().clone(), |ui| {
            crate::plugins::fps::draw_ui(ui, world);

            egui::CollapsingHeader::new("Streaming")
                .default_open(true)
                .show(ui, |ui| {
                    crate::plugins::streaming::draw_ui(ui, world);
                });

            ui.collapsing("Camera", |ui| {
                crate::plugins::camera::draw_ui(ui, world);
            });

            ui.collapsing("Debug", |ui| {
                crate::plugins::debug::draw_ui(ui, world);
            });
        });
}

fn draw(
    mut egui_renderer: ResMut<EguiRenderer>,
    mut contexts: Query<EguiContextQuery>,
    device: Res<Device>,
    queue: Res<Queue>,
    config: Res<SurfaceConfig>,
    mut global_render_resources: ResMut<GlobalRenderResources>,
) {
    let global_render_resources = &mut *global_render_resources;

    for mut context in contexts.iter_mut() {
        let tris = &context.render_output.paint_jobs;

        for (id, image_delta) in &context.render_output.textures_delta.set {
            egui_renderer
                .renderer
                .update_texture(&device, &queue, *id, image_delta);
        }

        let screen_descriptor = ScreenDescriptor {
            size_in_pixels: [config.width, config.height],
            pixels_per_point: context.ctx.get_mut().pixels_per_point(),
        };

        egui_renderer.renderer.update_buffers(
            &device,
            &queue,
            &mut global_render_resources.encoder,
            tris,
            &screen_descriptor,
        );

        let mut rpass =
            global_render_resources
                .encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("egui-main-render-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &global_render_resources.view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });

        egui_renderer
            .renderer
            .render(&mut rpass, tris, &screen_descriptor);

        drop(rpass);

        for x in &context.render_output.textures_delta.free {
            egui_renderer.renderer.free_texture(x)
        }
    }
}
