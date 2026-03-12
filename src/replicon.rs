use bevy::app::prelude::*;
use bevy::ecs::prelude::*;
use bevy::state::prelude::*;
use bevy_replicon::prelude::*;
use ecsdk_core::AppExit;
use ecsdk_replicon::{AcceptClientCmd, ConnectClientCmd};
use ecsdk_tasks::SpawnCmdTask;
use serde::{Deserialize, Serialize};

use crate::phase::{ShutdownRequested, VmPhase};

// ── Protocol events ──────────────────────────────────────────────

#[derive(Event, Serialize, Deserialize)]
pub struct ShutdownRequest;

#[derive(Event, Serialize, Deserialize)]
pub struct ForceStopRequest;

#[derive(Event, Serialize, Deserialize)]
pub struct ServerExitNotice;

#[derive(Component, Serialize, Deserialize, Default)]
pub struct DaemonControl;

#[derive(Resource, Clone)]
pub struct DaemonConfig(pub crate::config::SystemConfig);

#[derive(Component, Serialize, Deserialize, Clone, Default)]
pub struct DaemonSnapshot {
    pub ready: bool,
    pub state: String,
    pub ips: Vec<String>,
    pub daemon_running: bool,
    pub ssh_config: String,
    pub ssh_config_error: String,
}

// ── Shared replication plugin ────────────────────────────────────

pub struct SharedReplicationPlugin;

impl Plugin for SharedReplicationPlugin {
    fn build(&self, app: &mut App) {
        app.replicate::<VmPhase>();
        app.replicate::<crate::render::StepProgress>();
        app.replicate::<crate::lifecycle::VmError>();
        app.replicate::<DaemonSnapshot>();
        app.add_server_event::<ServerExitNotice>(Channel::Ordered);
        app.add_client_event::<ShutdownRequest>(Channel::Ordered);
        app.add_client_event::<ForceStopRequest>(Channel::Ordered);
    }
}

// ── Server plugin ────────────────────────────────────────────────

fn handle_shutdown_request(
    _trigger: On<FromClient<ShutdownRequest>>,
    mut shutdown: ResMut<ShutdownRequested>,
) {
    shutdown.0 = true;
}

fn handle_force_stop_request(
    _trigger: On<FromClient<ForceStopRequest>>,
    mut exit: ResMut<AppExit>,
) {
    exit.0 = true;
}

fn refresh_daemon_snapshot(
    sys_config: Res<DaemonConfig>,
    mut query: Query<&mut DaemonSnapshot, With<DaemonControl>>,
) {
    let Ok(mut snapshot) = query.single_mut() else {
        return;
    };

    let status = crate::daemon::current_status(&sys_config.0, true);
    snapshot.ready = true;
    snapshot.state = status.state;
    snapshot.ips = status.ips;
    snapshot.daemon_running = status.daemon_running;
    match crate::daemon::ssh_config(&sys_config.0) {
        Ok(text) => {
            snapshot.ssh_config = text;
            snapshot.ssh_config_error.clear();
        }
        Err(error) => {
            snapshot.ssh_config.clear();
            snapshot.ssh_config_error = error;
        }
    }
}

fn spawn_daemon_control(mut commands: Commands) {
    commands.spawn((DaemonControl, DaemonSnapshot::default(), Replicated));
}

fn send_exit_notice(mut commands: Commands, exit: Res<AppExit>, mut sent: Local<bool>) {
    if exit.0 && !*sent {
        commands.server_trigger(ToClients {
            mode: SendMode::Broadcast,
            message: ServerExitNotice,
        });
        *sent = true;
    }
}

pub struct RumServerPlugin {
    pub socket_path: std::path::PathBuf,
}

impl Plugin for RumServerPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.add_plugins(bevy::time::TimePlugin);
        app.add_plugins(RepliconPlugins.build().set(ServerPlugin::new(PostUpdate)));
        app.add_plugins(SharedReplicationPlugin);
        app.add_plugins(ecsdk_replicon::ServerTransportPlugin);

        let socket_path = self.socket_path.clone();
        app.add_systems(Startup, move |mut commands: Commands| {
            spawn_server_listener(&mut commands, socket_path.clone());
        });
        app.add_systems(Startup, spawn_daemon_control);

        app.add_systems(Update, (refresh_daemon_snapshot, send_exit_notice));
        app.add_observer(handle_shutdown_request);
        app.add_observer(handle_force_stop_request);
    }
}

fn spawn_server_listener(commands: &mut Commands, socket_path: std::path::PathBuf) {
    let path = socket_path.clone();
    commands
        .spawn_empty()
        .spawn_cmd_task(move |cmd| async move {
            // Remove stale socket
            let _ = tokio::fs::remove_file(&path).await;
            let listener = tokio::net::UnixListener::bind(&path).expect("bind daemon socket");
            tracing::info!(sock = %path.display(), "daemon listening");

            loop {
                let stream = match listener.accept().await {
                    Ok((stream, _)) => stream,
                    Err(e) => {
                        tracing::warn!("accept failed: {e}");
                        continue;
                    }
                };

                cmd.send(move |world: &mut World| {
                    AcceptClientCmd { stream }.apply(world);
                })
                .wake();
            }
        });
}

// ── Client plugin ────────────────────────────────────────────────

fn on_server_exit(_trigger: On<ServerExitNotice>, mut exit: ResMut<AppExit>) {
    exit.0 = true;
}

fn detect_disconnect(
    state: Res<State<ClientState>>,
    mut exit: ResMut<AppExit>,
    mut was_connected: Local<bool>,
) {
    if *state.get() == ClientState::Connected {
        *was_connected = true;
    } else if *was_connected {
        exit.0 = true;
    }
}

pub struct RumClientPlugin {
    pub socket_path: std::path::PathBuf,
}

fn configure_client_runtime(app: &mut App) {
    app.add_plugins(bevy::state::app::StatesPlugin);
    app.add_plugins(bevy::time::TimePlugin);
    app.add_plugins(RepliconPlugins);
    app.add_plugins(SharedReplicationPlugin);
    app.add_plugins(ecsdk_replicon::ClientTransportPlugin);

    app.add_observer(on_server_exit);
    app.add_systems(Update, detect_disconnect);
}

pub fn connect_client(world: &mut World, stream: tokio::net::UnixStream) {
    ConnectClientCmd { stream }.apply(world);
}

pub struct RumClientCorePlugin;

impl Plugin for RumClientCorePlugin {
    fn build(&self, app: &mut App) {
        configure_client_runtime(app);
    }
}

impl Plugin for RumClientPlugin {
    fn build(&self, app: &mut App) {
        configure_client_runtime(app);
        let socket_path = self.socket_path.clone();
        app.add_systems(Startup, move |mut commands: Commands| {
            spawn_client_connection(&mut commands, socket_path.clone());
        });
    }
}

fn spawn_client_connection(commands: &mut Commands, socket_path: std::path::PathBuf) {
    commands
        .spawn_empty()
        .spawn_cmd_task(move |cmd| async move {
            match tokio::net::UnixStream::connect(&socket_path).await {
                Ok(stream) => {
                    cmd.send(move |world: &mut World| {
                        ConnectClientCmd { stream }.apply(world);
                    })
                    .wake();
                }
                Err(e) => {
                    tracing::warn!("failed to connect to daemon: {e}");
                    cmd.send(|world: &mut World| {
                        world.resource_mut::<AppExit>().0 = true;
                    })
                    .wake();
                }
            }
        });
}
