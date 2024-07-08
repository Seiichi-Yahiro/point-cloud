use crate::plugins::render::line::LineRenderPlugin;
use crate::plugins::render::point::PointRenderPlugin;
use crate::plugins::render::ui::UiPlugin;
use bevy_app::prelude::*;
use bevy_ecs::prelude::*;

mod bind_groups;
pub mod line;
pub mod point;
mod ui;
pub mod vertex;

pub struct RenderPlugin;

impl Plugin for RenderPlugin {
    fn build(&self, app: &mut App) {
        app.configure_sets(
            Startup,
            ((BufferSet, BindGroupLayoutSet), BindGroupSet, PipelineSet).chain(),
        )
        .configure_sets(PostUpdate, (BufferSet, BindGroupSet).chain())
        .add_plugins((PointRenderPlugin, LineRenderPlugin, UiPlugin));
    }
}

#[derive(Debug, PartialEq, Eq, Hash, Copy, Clone, SystemSet)]
pub struct BufferSet;

#[derive(Debug, PartialEq, Eq, Hash, Copy, Clone, SystemSet)]
pub struct BindGroupLayoutSet;

#[derive(Debug, PartialEq, Eq, Hash, Copy, Clone, SystemSet)]
pub struct BindGroupSet;

#[derive(Debug, PartialEq, Eq, Hash, Copy, Clone, SystemSet)]
pub struct PipelineSet;
