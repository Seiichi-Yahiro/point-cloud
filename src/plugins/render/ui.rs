use bevy_app::prelude::*;
use bevy_diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy_ecs::prelude::*;
use egui::epaint::Shadow;
use egui::{Context, Visuals};
use egui_wgpu::{Renderer, ScreenDescriptor};
use egui_winit::State;

use crate::plugins::wgpu::{
    CommandEncoders, Device, GlobalRenderResources, Queue, Render, RenderPassSet, SurfaceConfig,
};
use crate::plugins::winit::{Window, WindowEvent};

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(SidePanelOpened(true))
            .add_systems(Startup, setup)
            .add_systems(PreUpdate, handle_input)
            .add_systems(Render, (prepare, ui, draw).chain().in_set(RenderPassSet));

        app.world_mut()
            .get_resource_mut::<CommandEncoders>()
            .unwrap()
            .register::<Self>();
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

#[derive(Resource)]
struct SidePanelOpened(bool);

fn ui(world: &mut World) {
    let context = world.get_resource::<Egui>().unwrap().context.clone();

    world.resource_scope(|world, mut side_panel_opened: Mut<SidePanelOpened>| {
        let response = egui::SidePanel::left("UI")
            .resizable(true)
            .width_range(150.0..=250.0)
            .default_width(150.0)
            .show_animated(&context, side_panel_opened.0, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    {
                        let diagnostics = world.get_resource::<DiagnosticsStore>().unwrap();

                        let fps = diagnostics
                            .get(&FrameTimeDiagnosticsPlugin::FPS)
                            .and_then(|it| it.smoothed());

                        if let Some(fps) = fps {
                            ui.label(format!("FPS: {:>4.0}", fps));
                        }

                        let frame_time = diagnostics
                            .get(&FrameTimeDiagnosticsPlugin::FRAME_TIME)
                            .and_then(|it| it.smoothed());

                        if let Some(frame_time) = frame_time {
                            ui.label(format!("Frame: {:>2.2} ms", frame_time));
                        }
                    }

                    egui::CollapsingHeader::new("Metadata")
                        .default_open(true)
                        .show(ui, |ui| {
                            crate::plugins::metadata::draw_ui(ui, world);
                        });

                    egui::CollapsingHeader::new("Cells")
                        .default_open(true)
                        .show(ui, |ui| {
                            crate::plugins::cell::draw_ui(ui, world);
                        });

                    #[cfg(not(target_arch = "wasm32"))]
                    ui.collapsing("Converter", |ui| {
                        crate::plugins::converter::draw_ui(ui, world);
                    });

                    ui.collapsing("Camera", |ui| {
                        crate::plugins::camera::draw_ui(ui, world);
                    });

                    ui.collapsing("Debug", |ui| {
                        crate::plugins::debug::draw_ui(ui, world);
                    });
                });
            });

        egui::Window::new("Hamburger")
            .resizable(false)
            .collapsible(false)
            .title_bar(false)
            .current_pos(egui::Pos2::new(
                response.map_or(0.0, |it| it.response.rect.width()) + 1.0,
                1.0,
            ))
            .frame(egui::Frame::none())
            .show(&context, |ui| {
                if ui.button("☰").clicked() {
                    side_panel_opened.0 = !side_panel_opened.0;
                }
            });
    });
}

fn draw(
    mut egui: ResMut<Egui>,
    window: Res<Window>,
    device: Res<Device>,
    queue: Res<Queue>,
    config: Res<SurfaceConfig>,
    mut global_render_resources: GlobalRenderResources,
) {
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

    global_render_resources
        .encoders
        .encode::<UiPlugin>(|encoder| {
            egui.renderer
                .update_buffers(&device, &queue, encoder, &tris, &screen_descriptor);

            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui-main-render-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &global_render_resources.render_view.view,
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
        });

    for x in &full_output.textures_delta.free {
        egui.renderer.free_texture(x)
    }
}
