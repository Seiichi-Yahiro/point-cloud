use std::fmt::{Display, Formatter};

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_ecs::system::Resource;
use web_time::{Duration, Instant};

pub struct FPSPlugin;

impl Plugin for FPSPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(FPS::new()).add_systems(Update, update);
    }
}

#[derive(Debug, Resource)]
pub struct FPS {
    last_second: Instant,
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
            last_second: Instant::now(),
            value: 0,
            frame_count: 0,
        }
    }

    fn update(&mut self) {
        self.frame_count += 1;
        
        if self.last_second.elapsed() >= Duration::from_secs(1) {
            self.value = self.frame_count;
            self.frame_count = 0;
            self.last_second = Instant::now();
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
