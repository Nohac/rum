use ecsdk::app::AsyncApp;
use ecsdk::prelude::*;
use ecsdk::network::{IsomorphicApp, IsomorphicPlugin};
use orchestrator::instance::instance_phase::{
    Booting, ConnectingGuest, Failed, Preparing, Provisioning, Recovering, Running, ShuttingDown,
    Stopped,
};
use orchestrator::{EntityError, OrchestratorMessage};

/// Client-side observers for the first plain `rum up` flow.
///
/// The initial client is intentionally minimal: it prints replicated phase
/// changes and exits once the machine reaches a terminal or ready state.
pub struct RumClientPlugin;

impl Plugin for RumClientPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_recovering);
        app.add_observer(on_preparing);
        app.add_observer(on_booting);
        app.add_observer(on_connecting_guest);
        app.add_observer(on_provisioning);
        app.add_observer(on_running);
        app.add_observer(on_shutting_down);
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

/// Build the plain client app used by the initial `rum up` command.
pub fn build_up_client(socket_path: std::path::PathBuf) -> AsyncApp<OrchestratorMessage> {
    let mut iso = IsomorphicApp::<OrchestratorMessage>::new();
    iso.add_plugin(crate::network::SharedNetworkPlugin::new(socket_path));
    iso.add_plugin(ClientOnlyPlugin);
    iso.build_client()
}

fn on_server_disconnect(
    mut disconnects: MessageReader<ServerDisconnected>,
    mut exit: MessageWriter<AppExit>,
) {
    if disconnects.read().next().is_some() {
        exit.write(AppExit::Success);
    }
}

fn print_phase(label: &str) {
    println!("{label}");
}

fn on_recovering(_trigger: On<Add, Recovering>) {
    print_phase("recovering instance state");
}

fn on_preparing(_trigger: On<Add, Preparing>) {
    print_phase("preparing machine");
}

fn on_booting(_trigger: On<Add, Booting>) {
    print_phase("booting machine");
}

fn on_connecting_guest(_trigger: On<Add, ConnectingGuest>) {
    print_phase("connecting to guest");
}

fn on_provisioning(_trigger: On<Add, Provisioning>) {
    print_phase("running provisioning");
}

fn on_running(_trigger: On<Add, Running>, mut exit: MessageWriter<AppExit>) {
    print_phase("machine is running");
    exit.write(AppExit::Success);
}

fn on_shutting_down(_trigger: On<Add, ShuttingDown>) {
    print_phase("shutting down");
}

fn on_stopped(_trigger: On<Add, Stopped>, mut exit: MessageWriter<AppExit>) {
    print_phase("machine stopped");
    exit.write(AppExit::Success);
}

fn on_failed(
    trigger: On<Add, Failed>,
    errors: Query<&EntityError>,
    mut exit: MessageWriter<AppExit>,
) {
    let entity = trigger.event_target();
    if let Ok(error) = errors.get(entity) {
        eprintln!("machine failed: {}", error.0);
    } else {
        eprintln!("machine failed");
    }
    exit.write(AppExit::Success);
}
