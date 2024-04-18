use bevy_a11y::AccessibilityPlugin;
use bevy_input::InputPlugin;
use bevy_time::TimePlugin;
use bevy_window::{Window, WindowPlugin};
use bevy_winit::WinitPlugin;
use cfg_if::cfg_if;

use crate::plugins::camera::CameraPlugin;
use crate::plugins::debug::DebugPlugin;
use crate::plugins::fps::FPSPlugin;
use crate::plugins::render::RenderPlugin;
use crate::plugins::streaming::StreamingPlugin;
use crate::plugins::wgpu::WGPUPlugin;

pub struct App;

impl App {
    pub async fn run() {
        setup_logger();

        let mut app = bevy_app::App::new();

        app.add_plugins((
            TimePlugin,
            InputPlugin,
            WindowPlugin {
                primary_window: Some(Window {
                    title: "Point Cloud".to_string(),
                    canvas: Some("#point-cloud-canvas".to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            AccessibilityPlugin,
            WinitPlugin::default(),
        ));

        WGPUPlugin::build(&mut app).await;

        app.add_plugins((
            CameraPlugin,
            RenderPlugin,
            StreamingPlugin,
            FPSPlugin,
            DebugPlugin,
        ))
        .run();
    }
}

fn setup_logger() {
    cfg_if! {
        if #[cfg(target_arch = "wasm32")] {
            std::panic::set_hook(Box::new(console_error_panic_hook::hook));
            console_log::init_with_level(log::Level::Debug).expect("Couldn't initialize logger");
        } else {
            env_logger::init();
        }
    }
}
