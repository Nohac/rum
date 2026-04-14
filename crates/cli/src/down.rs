use ecsdk::app::AsyncApp;
use ecsdk::prelude::*;
use orchestrator::OrchestratorMessage;

use crate::exit;
use crate::protocol::{DownRequest, DownResponse};

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
pub fn build_down_client(mut app: AsyncApp<OrchestratorMessage>) -> AsyncApp<OrchestratorMessage> {
    DownFeature::register_client(&mut app);
    app.add_plugins(RumDownClientPlugin);
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
