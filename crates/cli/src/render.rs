mod plain;

use clap::ValueEnum;
use ecsdk::prelude::*;

/// Output mode for the first CLI renderer.
#[derive(Clone, Copy, PartialEq, Eq, Debug, ValueEnum)]
pub enum RenderMode {
    Plain,
    None,
}

/// Install the currently supported rum renderer.
pub struct RumRenderPlugin {
    mode: RenderMode,
}

impl RumRenderPlugin {
    pub fn new(mode: RenderMode) -> Self {
        Self { mode }
    }
}

impl Plugin for RumRenderPlugin {
    fn build(&self, app: &mut App) {
        match self.mode {
            RenderMode::Plain => {
                app.add_systems(PostUpdate, plain::render_plain);
            }
            RenderMode::None => {}
        }
    }
}
