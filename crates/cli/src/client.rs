use ecsdk::app::AsyncApp;
use ecsdk::prelude::*;
use orchestrator::OrchestratorMessage;

use crate::exit;

/// Client-side observers for the first plain `rum up` flow.
///
/// The initial client stays small: rendering is delegated to a renderer plugin,
/// while the client plugin owns only exit conditions and connection behavior.
pub struct RumClientPlugin;

impl Plugin for RumClientPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(exit::on_running);
        app.add_observer(exit::on_stopped);
        app.add_observer(exit::on_failed);
        app.add_systems(Update, exit::on_server_disconnect);
    }
}

/// Build the client app used by the initial `rum up` command.
pub fn build_up_client(mut app: AsyncApp<OrchestratorMessage>) -> AsyncApp<OrchestratorMessage> {
    app.add_plugins(RumClientPlugin);
    app
}
