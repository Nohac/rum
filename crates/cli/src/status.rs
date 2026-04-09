use ecsdk::app::AsyncApp;
use ecsdk::prelude::*;
use orchestrator::{EntityError, InstanceLabel, InstancePhase, OrchestratorMessage, RecoveredState};

use crate::protocol::{StatusRequest, StatusResponse};

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
pub fn build_status_client(socket_path: std::path::PathBuf) -> AsyncApp<OrchestratorMessage> {
    let iso = crate::app::create_isomorphic_app(socket_path);
    let mut app = iso.build_client();
    StatusFeature::register_client(&mut app);
    app.add_plugins(RumStatusClientPlugin);
    app
}

struct RumStatusClientPlugin;

impl Plugin for RumStatusClientPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, on_server_disconnect);
    }
}

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

fn handle_status_response(trigger: On<StatusResponse>, mut exit: MessageWriter<AppExit>) {
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

    exit.write(AppExit::Success);
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
