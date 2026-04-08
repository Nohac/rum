use ecsdk::app::AsyncApp;
use ecsdk::network::IsomorphicPlugin;
use ecsdk::prelude::*;
use orchestrator::instance::instance_phase::{
    Failed, Running, Stopped,
};
use orchestrator::{EntityError, OrchestratorMessage};

use crate::render::{RenderMode, RumRenderPlugin};

/// Client-side observers for the first plain `rum up` flow.
///
/// The initial client stays small: rendering is delegated to a renderer plugin,
/// while the client plugin owns only exit conditions and connection behavior.
pub struct RumClientPlugin;

impl Plugin for RumClientPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_running);
        app.add_observer(on_stopped);
        app.add_observer(on_failed);
        app.add_systems(Update, on_server_disconnect);
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

fn on_server_disconnect(
    mut disconnects: MessageReader<ServerDisconnected>,
    mut exit: MessageWriter<AppExit>,
) {
    if disconnects.read().next().is_some() {
        tracing::info!("rum daemon disconnected");
        exit.write(AppExit::Success);
    }
}

fn on_running(_trigger: On<Add, Running>, mut exit: MessageWriter<AppExit>) {
    tracing::info!("managed instance reached running state");
    exit.write(AppExit::Success);
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
