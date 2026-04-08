use ecsdk::app::AsyncApp;
use ecsdk::prelude::*;
use orchestrator::instance::instance_phase::{Failed, Stopped};
use orchestrator::{EntityError, OrchestratorMessage};

use crate::protocol::{DownRequest, DownResponse};
use crate::render::{RenderMode, RumRenderPlugin};

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
) -> AsyncApp<OrchestratorMessage> {
    let iso = crate::app::create_isomorphic_app(socket_path);
    let mut app = iso.build_client();
    DownFeature::register_client(&mut app);
    app.add_plugins((RumRenderPlugin::new(render_mode), RumDownClientPlugin));
    app
}

struct RumDownClientPlugin;

impl Plugin for RumDownClientPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_stopped);
        app.add_observer(on_failed);
        app.add_systems(Update, on_server_disconnect);
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

fn on_server_disconnect(
    mut disconnects: MessageReader<ServerDisconnected>,
    mut exit: MessageWriter<AppExit>,
) {
    if disconnects.read().next().is_some() {
        tracing::info!("rum daemon disconnected");
        exit.write(AppExit::Success);
    }
}

fn on_stopped(_trigger: On<Add, Stopped>, mut exit: MessageWriter<AppExit>) {
    tracing::info!("managed instance reached stopped state");
    exit.write(AppExit::Success);
}

fn on_failed(
    trigger: On<Add, Failed>,
    errors: Query<&EntityError>,
    mut exit: MessageWriter<AppExit>,
) {
    let entity = trigger.event_target();
    if let Ok(error) = errors.get(entity) {
        tracing::error!(entity = entity.index().index(), error = %error.0, "managed instance failed");
    } else {
        tracing::error!(entity = entity.index().index(), "managed instance failed");
    }
    exit.write(AppExit::Success);
}
