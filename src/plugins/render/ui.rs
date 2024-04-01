use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use egui::{Context, Visuals};
use egui::epaint::Shadow;
use egui_wgpu::{Renderer, ScreenDescriptor};
use egui_winit::State;

use crate::plugins::render::{GlobalRenderResources, RenderPassSet};
use crate::plugins::wgpu::{Device, Queue, SurfaceConfig};
use crate::plugins::winit::{Window, WindowEvent};

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup)
            .add_systems(PreUpdate, handle_input)
            .add_systems(Last, (prepare, ui, draw).chain().in_set(RenderPassSet::UI));
    }
}

#[derive(Resource)]
struct Egui {
    pub context: Context,
    state: State,
    renderer: Renderer,
}
fn setup(
    mut commands: Commands,
    window: Res<Window>,
    device: Res<Device>,
    config: Res<SurfaceConfig>,
) {
    let egui_context = Context::default();

    let visuals = Visuals {
        dark_mode: true,
        window_shadow: Shadow::NONE,
        window_rounding: egui::Rounding::same(2.0),
        ..Default::default()
    };

    egui_context.set_visuals(visuals);

    let window: &winit::window::Window = &window;

    let egui_state = State::new(
        egui_context.clone(),
        egui_context.viewport_id(),
        window,
        egui_context.native_pixels_per_point(),
        None,
    );

    let egui_renderer = Renderer::new(&device, config.format, None, 1);

    commands.insert_resource(Egui {
        context: egui_context,
        state: egui_state,
        renderer: egui_renderer,
    });
}

fn handle_input(
    mut egui: ResMut<Egui>,
    mut window_events: EventReader<WindowEvent>,
    window: Res<Window>,
) {
    for event in window_events.read() {
        if egui.state.on_window_event(&window, event).repaint {
            window.request_redraw();
        }
    }
}

fn prepare(mut egui: ResMut<Egui>, window: Res<Window>) {
    let raw_input = egui.state.take_egui_input(&window);
    egui.context.begin_frame(raw_input);
}

fn ui(world: &mut World) {
    egui::Window::new("UI")
        .resizable(false)
        .default_width(150.0)
        .show(
            &world.get_resource::<Egui>().unwrap().context.clone(),
            |ui| {
                crate::plugins::fps::draw_ui(ui, world);
                ui.separator();
                crate::plugins::camera::fly_cam::draw_ui(ui, world);
                ui.separator();
                crate::plugins::streaming::draw_ui(ui, world);
            },
        );
}

fn draw(
    mut egui: ResMut<Egui>,
    window: Res<Window>,
    device: Res<Device>,
    queue: Res<Queue>,
    config: Res<SurfaceConfig>,
    mut global_render_resources: ResMut<GlobalRenderResources>,
) {
    let global_render_resources = &mut *global_render_resources;

    let full_output = egui.context.end_frame();

    egui.state
        .handle_platform_output(&window, full_output.platform_output);

    let tris = egui
        .context
        .tessellate(full_output.shapes, egui.context.pixels_per_point());

    for (id, image_delta) in &full_output.textures_delta.set {
        egui.renderer
            .update_texture(&device, &queue, *id, image_delta);
    }

    let screen_descriptor = ScreenDescriptor {
        size_in_pixels: [config.width, config.height],
        pixels_per_point: egui.context.pixels_per_point(),
    };

    egui.renderer.update_buffers(
        &device,
        &queue,
        &mut global_render_resources.encoder,
        &tris,
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

    egui.renderer.render(&mut rpass, &tris, &screen_descriptor);

    drop(rpass);

    for x in &full_output.textures_delta.free {
        egui.renderer.free_texture(x)
    }
}