use std::sync::Arc;

use cfg_if::cfg_if;

use crate::plugins::camera::CameraPlugin;
use crate::plugins::cell::CellPlugin;
use crate::plugins::debug::DebugPlugin;
use crate::plugins::fps::FPSPlugin;
use crate::plugins::input::InputPlugin;
use crate::plugins::metadata::MetadataPlugin;
use crate::plugins::render::RenderPlugin;
use crate::plugins::thread_pool::ThreadPoolPlugin;
use crate::plugins::wgpu::WGPUPlugin;
use crate::plugins::winit::{Window, WinitPlugin};

pub struct App;

impl App {
    pub async fn run() {
        setup_logger();

        let mut app = bevy_app::App::new();
        app.add_plugins(WinitPlugin::new("point-cloud-canvas".to_string()));

        WGPUPlugin::build(
            Arc::clone(app.world.get_resource::<Window>().unwrap()),
            &mut app,
        )
        .await;

        app.add_plugins((
            ThreadPoolPlugin,
            InputPlugin,
            CameraPlugin,
            FPSPlugin,
            MetadataPlugin,
            CellPlugin,
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
