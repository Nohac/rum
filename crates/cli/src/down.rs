use ecsdk::app::AsyncApp;
use ecsdk::prelude::*;
use orchestrator::OrchestratorMessage;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::exit;
use crate::protocol::{DownRequest, DownResponse};
use crate::render::{RenderMode, RumRenderPlugin};
use crate::restart::ProtocolRestartPlugin;

/// Isomorphic request feature that lets a client ask the daemon to shut down
/// the managed machine.
pub struct DownFeature;

impl RequestPlugin for DownFeature {
    type Request = DownRequest;
    type Trigger = ecsdk::network::InitialConnection;

    fn auto_register_client() -> bool {
        false
    }

    fn build_server(app: &mut App) {
        app.add_observer(handle_down_request);
    }

    fn build_client(app: &mut App) {
        app.add_observer(handle_down_response);
    }
}

/// Build the client app used by `rum down`.
pub fn build_down_client(
    socket_path: std::path::PathBuf,
    render_mode: RenderMode,
    restart_requested: Arc<AtomicBool>,
) -> AsyncApp<OrchestratorMessage> {
    let iso = crate::app::create_isomorphic_app(socket_path);
    let mut app = iso.build_client();
    DownFeature::register_client(&mut app);
    app.add_plugins((
        RumRenderPlugin::new(render_mode),
        ProtocolRestartPlugin::new(restart_requested),
        RumDownClientPlugin,
    ));
    app
}

struct RumDownClientPlugin;

impl Plugin for RumDownClientPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(exit::on_stopped);
        app.add_observer(exit::on_failed);
        app.add_systems(Update, exit::on_server_disconnect);
    }
}

fn handle_down_request(trigger: On<FromClient<DownRequest>>, mut commands: Commands) {
    DownRequest::reply(
        &mut commands,
        trigger.event().client_id,
        DownResponse { accepted: true },
    );
    commands.send_msg(OrchestratorMessage::RequestShutdown);
}

fn handle_down_response(trigger: On<DownResponse>) {
    if trigger.event().accepted {
        tracing::info!("shutdown request accepted");
    }
}
