use bevy_app::prelude::*;

use crate::plugins::render::line::LineRenderPlugin;
use crate::plugins::render::point::PointRenderPlugin;
use crate::plugins::render::ui::UiPlugin;

pub mod line;
pub mod point;
mod ui;
pub mod vertex;

pub struct RenderPlugin;

impl Plugin for RenderPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(PointRenderPlugin)
            .add_plugins(LineRenderPlugin)
            .add_plugins(UiPlugin);
    }
}
