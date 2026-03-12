use bevy::ecs::prelude::*;

use crate::cli::OutputFormat;

#[derive(Component, serde::Serialize, serde::Deserialize)]
pub struct StepProgress {
    pub current: usize,
    pub total: usize,
}

#[derive(Resource)]
pub struct RenderMode(pub OutputFormat);
