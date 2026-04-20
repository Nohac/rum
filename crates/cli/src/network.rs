use std::path::PathBuf;

use ecsdk::prelude::*;
use ecsdk::tasks::SpawnTask;
use interprocess::local_socket::traits::tokio::Listener as _;
use orchestrator::{
    EntityError, InstanceLabel, InstancePhase, ProvisionLogEntry, RecoveredState,
};

/// Socket path shared by the local daemon/client pair.
#[derive(Resource, Clone)]
struct SocketPath(PathBuf);

/// Shared client/server wiring for the first rum daemon connection.
///
/// This plugin mirrors the `compose` example structure: shared replication
/// registration happens in `build_shared`, while the actual local-socket
/// listener/connector startup is split between `build_server` and
/// `build_client`.
pub struct SharedNetworkPlugin {
    socket_path: PathBuf,
}

impl SharedNetworkPlugin {
    pub fn new(socket_path: PathBuf) -> Self {
        Self { socket_path }
    }
}

impl IsomorphicPlugin for SharedNetworkPlugin {
    fn build_shared(&self, app: &mut App) {
        app.insert_resource(SocketPath(self.socket_path.clone()));
        app.replicate::<RecoveredState>();
        app.replicate::<EntityError>();
        app.replicate::<InstanceLabel>();
        app.replicate::<ProvisionLogEntry>();
        InstancePhase::replicate_markers(app);
    }

    fn build_server(&self, app: &mut App) {
        app.add_systems(Startup, spawn_server_listener);
    }

    fn build_client(&self, app: &mut App) {
        app.add_systems(Startup, spawn_client_connection);
    }
}

fn spawn_server_listener(mut commands: Commands, socket_path: Res<SocketPath>) {
    let socket_path = socket_path.0.clone();
    commands.spawn_empty().spawn_task(move |task| async move {
        let listener =
            crate::ipc::create_listener(&socket_path).expect("failed to bind rum socket");
        tracing::info!(socket = %socket_path.display(), "rum daemon listening");

        loop {
            let stream = match listener.accept().await {
                Ok(stream) => stream,
                Err(error) => {
                    tracing::warn!(%error, "failed to accept client connection");
                    continue;
                }
            };

            tracing::info!("accepted client connection");

            task.queue_cmd_wake(move |world: &mut World| {
                ecsdk::network::AcceptClientCmd { stream }.apply(world);
            });
        }
    });
}

fn spawn_client_connection(mut commands: Commands, socket_path: Res<SocketPath>) {
    let socket_path = socket_path.0.clone();
    commands.spawn_empty().spawn_task(move |task| async move {
        tracing::info!(socket = %socket_path.display(), "connecting to rum daemon");
        match crate::ipc::connect(&socket_path).await {
            Ok(stream) => {
                tracing::info!("connected to rum daemon");
                task.queue_cmd_wake(move |world: &mut World| {
                    ecsdk::network::ConnectClientCmd { stream }.apply(world);
                });
            }
            Err(error) => {
                tracing::warn!(%error, socket = %socket_path.display(), "failed to connect to rum daemon");
                task.queue_cmd_wake(|world: &mut World| {
                    world.write_message(AppExit::Success);
                });
            }
        }
    });
}
