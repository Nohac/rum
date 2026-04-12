use ecsdk::app::AsyncApp;
use ecsdk::network::IsomorphicPlugin;
use ecsdk::prelude::*;
use orchestrator::OrchestratorMessage;

use crate::exit;
use crate::render::{RenderMode, RumRenderPlugin};

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

struct ClientOnlyPlugin;

impl IsomorphicPlugin for ClientOnlyPlugin {
    fn build_client(&self, app: &mut App) {
        app.add_plugins(RumClientPlugin);
    }
}

/// Build the client app used by the initial `rum up` command.
pub fn build_up_client(
    socket_path: std::path::PathBuf,
    render_mode: RenderMode,
) -> AsyncApp<OrchestratorMessage> {
    let mut iso = crate::app::create_isomorphic_app(socket_path);
    iso.add_plugin(ClientOnlyPlugin);
    let mut app = iso.build_client();
    app.add_plugins(RumRenderPlugin::new(render_mode));
    app
}
