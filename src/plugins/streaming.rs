use bevy_app::prelude::*;
use bevy_ecs::prelude::*;

use crate::plugins::streaming::cell::CellPlugin;
use crate::plugins::streaming::metadata::MetadataPlugin;

pub mod cell;
mod loader;
pub mod metadata;

pub struct StreamingPlugin;

impl Plugin for StreamingPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((MetadataPlugin, CellPlugin));
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub type Directory = std::path::PathBuf;

#[cfg(target_arch = "wasm32")]
pub type Directory = web_sys::FileSystemDirectoryHandle;

#[derive(Debug, Clone)]
pub enum Source {
    Directory(Directory),
    URL,
    None,
}

pub fn draw_ui(ui: &mut egui::Ui, world: &mut World) {
    metadata::draw_ui(ui, world);
    cell::draw_ui(ui, world);
}
