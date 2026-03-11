mod interactive;
mod plain;
mod json;

use bevy::app::prelude::*;
use bevy::ecs::prelude::*;

use crate::cli::OutputFormat;

// ── Components ──────────────────────────────────────────────────

#[derive(Component, serde::Serialize, serde::Deserialize)]
pub struct StepProgress {
    pub current: usize,
    pub total: usize,
}

#[derive(Resource)]
pub struct RenderMode(pub OutputFormat);

// ── Plugin ──────────────────────────────────────────────────────

pub struct RumRenderPlugin(pub OutputFormat);

impl Plugin for RumRenderPlugin {
    fn build(&self, app: &mut App) {
        match self.0 {
            OutputFormat::Interactive | OutputFormat::Auto => {
                app.add_systems(PostUpdate, interactive::render);
            }
            OutputFormat::Plain => {
                app.add_systems(PostUpdate, plain::render);
            }
            OutputFormat::Json => {
                app.add_systems(PostUpdate, json::render);
            }
        }
    }
}
