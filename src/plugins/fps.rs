use std::fmt::{Display, Formatter};

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_ecs::system::Resource;

pub struct FPSPlugin;

impl Plugin for FPSPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(FPS::new()).add_systems(Update, update);
    }
}

#[derive(Debug, Resource)]
pub struct FPS {
    #[cfg(not(target_arch = "wasm32"))]
    last_second: std::time::Instant,

    #[cfg(target_arch = "wasm32")]
    last_second: f64,

    value: u32,
    frame_count: u32,
}

impl Display for FPS {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} FPS", self.value)
    }
}

impl FPS {
    fn new() -> Self {
        FPS {
            #[cfg(not(target_arch = "wasm32"))]
            last_second: std::time::Instant::now(),

            #[cfg(target_arch = "wasm32")]
            last_second: web_sys::window()
                .and_then(|window| window.performance())
                .unwrap()
                .now(),

            value: 0,
            frame_count: 0,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn update(&mut self) {
        self.frame_count += 1;

        if self.last_second.elapsed() >= std::time::Duration::from_secs(1) {
            self.value = self.frame_count;
            self.frame_count = 0;
            self.last_second = std::time::Instant::now();
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn update(&mut self) {
        self.frame_count += 1;

        let now = web_sys::window()
            .and_then(|window| window.performance())
            .unwrap()
            .now();

        if now - self.last_second >= 1000.0 {
            self.value = self.frame_count;
            self.frame_count = 0;
            self.last_second = now;
        }
    }
}

fn update(mut fps: ResMut<FPS>) {
    fps.update();
}

pub fn draw_ui(ui: &mut egui::Ui, world: &mut World) {
    let fps = world.get_resource::<FPS>().unwrap().to_string();
    ui.label(fps);
}
