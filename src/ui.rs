use egui::{Context, Visuals};
use egui::epaint::Shadow;
use egui_wgpu::{Renderer, ScreenDescriptor};
use egui_winit::{EventResponse, State};
use winit::event::WindowEvent;
use winit::window::Window;

use crate::gpu::GPU;

pub struct EguiRenderer {
    pub context: Context,
    state: State,
    renderer: Renderer,
}

impl EguiRenderer {
    pub fn new(
        device: &wgpu::Device,
        output_color_format: wgpu::TextureFormat,
        window: &Window,
    ) -> EguiRenderer {
        let egui_context = Context::default();

        let visuals = Visuals {
            dark_mode: true,
            window_shadow: Shadow::NONE,
            window_rounding: egui::Rounding::same(2.0),
            ..Default::default()
        };

        egui_context.set_visuals(visuals);

        let egui_state = State::new(
            egui_context.clone(),
            egui_context.viewport_id(),
            &window,
            egui_context.native_pixels_per_point(),
            None,
        );

        let egui_renderer = Renderer::new(device, output_color_format, None, 1);

        EguiRenderer {
            context: egui_context,
            state: egui_state,
            renderer: egui_renderer,
        }
    }

    pub fn handle_input(&mut self, window: &Window, event: &WindowEvent) -> EventResponse {
        self.state.on_window_event(window, event)
    }

    pub fn draw<F: FnOnce(&Context)>(
        &mut self,
        gpu: &GPU,
        encoder: &mut wgpu::CommandEncoder,
        window: &Window,
        render_target: &wgpu::TextureView,
        screen_descriptor: ScreenDescriptor,
        run_ui: F,
    ) {
        let raw_input = self.state.take_egui_input(window);
        let full_output = self.context.run(raw_input, run_ui);

        self.state
            .handle_platform_output(window, full_output.platform_output);

        let tris = self
            .context
            .tessellate(full_output.shapes, self.context.pixels_per_point());

        for (id, image_delta) in &full_output.textures_delta.set {
            self.renderer
                .update_texture(gpu.device(), gpu.queue(), *id, image_delta);
        }

        self.renderer.update_buffers(
            gpu.device(),
            gpu.queue(),
            encoder,
            &tris,
            &screen_descriptor,
        );

        let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("egui-main-render-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: render_target,
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

        self.renderer.render(&mut rpass, &tris, &screen_descriptor);

        drop(rpass);

        for x in &full_output.textures_delta.free {
            self.renderer.free_texture(x)
        }
    }
}
