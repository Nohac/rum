use bevy::app::prelude::*;

use crate::cli::OutputFormat;

pub struct RumRenderPlugin(pub OutputFormat);

impl Plugin for RumRenderPlugin {
    fn build(&self, app: &mut App) {
        match self.0 {
            OutputFormat::Interactive | OutputFormat::Auto => {
                super::interactive::build(app);
            }
            OutputFormat::Plain => {
                app.add_systems(PostUpdate, super::plain::render);
            }
            OutputFormat::Json => {
                app.add_systems(PostUpdate, super::json::render);
            }
        }
    }
}
