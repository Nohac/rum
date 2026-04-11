use ecsdk::app::AsyncApp;
use ecsdk::prelude::*;
use orchestrator::instance::instance_phase::{Failed, Running, Stopped};
use orchestrator::{EntityError, InstanceLabel, InstancePhase, OrchestratorMessage, RecoveredState};

use crate::protocol::{StatusRequest, StatusResponse};
use crate::render::{RenderMode, RumRenderPlugin};

/// Client-side behavior for `rum status`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusMode {
    Snapshot,
    Watch,
    WaitReady,
}

/// Isomorphic request feature that lets a client ask the daemon for a current
/// status snapshot.
pub struct StatusFeature;

impl RequestPlugin for StatusFeature {
    type Request = StatusRequest;
    type Trigger = ecsdk::network::InitialConnection;

    fn auto_register_client() -> bool {
        false
    }

    fn build_server(app: &mut App) {
        app.add_observer(handle_status_request);
    }

    fn build_client(app: &mut App) {
        app.add_observer(handle_status_response);
    }
}

/// Build the client app used by `rum status`.
pub fn build_status_client(
    socket_path: std::path::PathBuf,
    render_mode: RenderMode,
    mode: StatusMode,
) -> AsyncApp<OrchestratorMessage> {
    let iso = crate::app::create_isomorphic_app(socket_path);
    let mut app = iso.build_client();
    StatusFeature::register_client(&mut app);

    if mode != StatusMode::Snapshot {
        app.add_plugins(RumRenderPlugin::new(render_mode));
    }

    app.add_plugins(RumStatusClientPlugin { mode });
    app
}

struct RumStatusClientPlugin {
    mode: StatusMode,
}

impl Plugin for RumStatusClientPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(StatusClientMode(self.mode));
        app.add_systems(Update, on_server_disconnect);
        app.add_observer(handle_status_response);

        if self.mode == StatusMode::WaitReady {
            app.add_observer(on_running_ready);
            app.add_observer(on_stopped_terminal);
            app.add_observer(on_failed_terminal);
        }
    }
}

#[derive(Resource, Clone, Copy)]
struct StatusClientMode(StatusMode);

#[allow(clippy::type_complexity)]
fn handle_status_request(
    trigger: On<FromClient<StatusRequest>>,
    query: Query<
        (
            Option<&InstanceLabel>,
            Option<&RecoveredState>,
            &InstancePhase,
            Option<&EntityError>,
        ),
        Without<ecsdk::network::InitialConnection>,
    >,
    mut commands: Commands,
) {
    let response = if let Some((label, recovered, phase, error)) = query.iter().next() {
        StatusResponse {
            found: true,
            label: label.map(|label| label.0.clone()),
            recovered_state: recovered.map(|recovered| recovered.0),
            phase: Some(*phase),
            error: error.map(|error| error.0.clone()),
        }
    } else {
        StatusResponse {
            found: false,
            label: None,
            recovered_state: None,
            phase: None,
            error: None,
        }
    };

    StatusRequest::reply(&mut commands, trigger.event().client_id, response);
}

fn handle_status_response(
    trigger: On<StatusResponse>,
    mode: Res<StatusClientMode>,
    mut exit: MessageWriter<AppExit>,
) {
    let status = trigger.event();

    if !status.found {
        println!("no managed instance");
        exit.write(AppExit::Success);
        return;
    }

    let label = status.label.as_deref().unwrap_or("instance");
    println!("{label}:");

    if let Some(recovered) = status.recovered_state {
        println!("  recovered state: {recovered}");
    }
    if let Some(phase) = status.phase {
        println!("  phase: {}", phase.label());
    }
    if let Some(error) = status.error.as_deref() {
        println!("  error: {error}");
    }

    if mode.0 == StatusMode::Snapshot {
        exit.write(AppExit::Success);
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

fn on_running_ready(_trigger: On<Add, Running>, mut exit: MessageWriter<AppExit>) {
    tracing::info!("managed instance reached running state");
    exit.write(AppExit::Success);
}

fn on_stopped_terminal(_trigger: On<Add, Stopped>, mut exit: MessageWriter<AppExit>) {
    tracing::info!("managed instance reached stopped state");
    exit.write(AppExit::Success);
}

fn on_failed_terminal(
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
