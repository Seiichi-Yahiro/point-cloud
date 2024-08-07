use bevy_core::FrameCountPlugin;
use bevy_diagnostic::FrameTimeDiagnosticsPlugin;
use bevy_state::app::StatesPlugin;
use bevy_time::TimePlugin;
use cfg_if::cfg_if;
use std::sync::Arc;
use url::Url;

use crate::plugins::camera::CameraPlugin;
use crate::plugins::cell::CellPlugin;
use crate::plugins::debug::DebugPlugin;
use crate::plugins::input::InputPlugin;
use crate::plugins::metadata::MetadataPlugin;
use crate::plugins::render::RenderPlugin;
use crate::plugins::thread_pool::ThreadPoolPlugin;
use crate::plugins::wgpu::WGPUPlugin;
use crate::plugins::winit::{Window, WinitPlugin};

pub struct App {
    pub canvas_id: Option<String>,
    pub url: Option<Url>,
}

impl App {
    pub async fn run(self) {
        setup_logger();

        let mut app = bevy_app::App::new();
        app.add_plugins(WinitPlugin::new(self.canvas_id));

        WGPUPlugin::build(
            Arc::clone(app.world().get_resource::<Window>().unwrap()),
            &mut app,
        )
        .await;

        app.add_plugins((
            StatesPlugin,
            TimePlugin,
            FrameCountPlugin,
            FrameTimeDiagnosticsPlugin,
        ))
        .add_plugins((InputPlugin, CameraPlugin))
        .add_plugins((
            ThreadPoolPlugin,
            MetadataPlugin { url: self.url },
            CellPlugin,
            #[cfg(not(target_arch = "wasm32"))]
            crate::plugins::converter::ConverterPlugin,
            DebugPlugin,
            RenderPlugin,
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
