use ecsdk::app::AsyncApp;
use ecsdk::prelude::*;
use orchestrator::{EntityError, InstanceLabel, InstancePhase, OrchestratorMessage, RecoveredState};

use crate::exit;
use crate::protocol::{StatusRequest, StatusResponse};

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
        app.add_systems(Update, exit::on_server_disconnect);
    }
}

/// Build the client app used by `rum status`.
pub fn build_status_client(
    mut app: AsyncApp<OrchestratorMessage>,
    mode: StatusMode,
) -> AsyncApp<OrchestratorMessage> {
    StatusFeature::register_client(&mut app);
    app.add_plugins(RumStatusClientPlugin { mode });
    app
}

struct RumStatusClientPlugin {
    mode: StatusMode,
}

impl Plugin for RumStatusClientPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(StatusClientMode(self.mode));

        if self.mode == StatusMode::WaitReady {
            app.add_observer(exit::on_running);
            app.add_observer(exit::on_stopped);
            app.add_observer(exit::on_failed);
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
