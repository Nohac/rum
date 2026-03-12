use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use bevy::app::prelude::*;
use bevy::ecs::prelude::*;
use bevy::state::prelude::*;
use bevy_replicon::prelude::*;

use crate::config::SystemConfig;
use crate::daemon::StatusInfo;
use crate::error::RumError;
use crate::replicon::{
    ForceStopRequest, RumClientPlugin, ShutdownRequest, SshConfigRequest, SshConfigResponse,
    StatusRequest, StatusResponse,
};

static REQUEST_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Resource)]
struct StatusClientState {
    request_id: u64,
    sent: bool,
    result: Arc<Mutex<Option<StatusInfo>>>,
}

#[derive(Resource)]
struct SshConfigClientState {
    request_id: u64,
    sent: bool,
    result: Arc<Mutex<Option<Result<String, String>>>>,
}

#[derive(Resource)]
struct ShutdownClientState {
    sent: Arc<Mutex<bool>>,
    force: bool,
}

fn next_request_id() -> u64 {
    REQUEST_ID.fetch_add(1, Ordering::Relaxed)
}

fn send_status_request(
    state: Res<State<ClientState>>,
    mut commands: Commands,
    mut request: ResMut<StatusClientState>,
) {
    if *state.get() != ClientState::Connected || request.sent {
        return;
    }
    commands.client_trigger(StatusRequest {
        request_id: request.request_id,
    });
    request.sent = true;
}

fn receive_status_response(
    mut exit: ResMut<ecsdk_core::AppExit>,
    request: Res<StatusClientState>,
    responses: Query<&StatusResponse>,
) {
    for response in &responses {
        if response.request_id == request.request_id {
            *request.result.lock().expect("status result lock poisoned") = Some(StatusInfo {
                state: response.state.clone(),
                ips: response.ips.clone(),
                daemon_running: response.daemon_running,
            });
            exit.0 = true;
            return;
        }
    }
}

fn send_ssh_config_request(
    state: Res<State<ClientState>>,
    mut commands: Commands,
    mut request: ResMut<SshConfigClientState>,
) {
    if *state.get() != ClientState::Connected || request.sent {
        return;
    }
    commands.client_trigger(SshConfigRequest {
        request_id: request.request_id,
    });
    request.sent = true;
}

fn receive_ssh_config_response(
    mut exit: ResMut<ecsdk_core::AppExit>,
    request: Res<SshConfigClientState>,
    responses: Query<&SshConfigResponse>,
) {
    for response in &responses {
        if response.request_id == request.request_id {
            let result = if response.error.is_empty() {
                Ok(response.text.clone())
            } else {
                Err(response.error.clone())
            };
            *request
                .result
                .lock()
                .expect("ssh-config result lock poisoned") = Some(result);
            exit.0 = true;
            return;
        }
    }
}

fn send_shutdown_request(
    state: Res<State<ClientState>>,
    mut commands: Commands,
    request: Res<ShutdownClientState>,
) {
    if *state.get() != ClientState::Connected {
        return;
    }

    let mut sent = request.sent.lock().expect("shutdown sent lock poisoned");
    if *sent {
        return;
    }

    if request.force {
        commands.client_trigger(ForceStopRequest);
    } else {
        commands.client_trigger(ShutdownRequest);
    }
    *sent = true;
}

pub async fn request_status(sys_config: &SystemConfig) -> Result<StatusInfo, RumError> {
    let socket_path = crate::paths::socket_path(&sys_config.id, sys_config.name.as_deref());
    let request_id = next_request_id();
    let result = Arc::new(Mutex::new(None));

    let (mut app, rx) = ecsdk_app::setup::<crate::lifecycle::RumMessage>();
    app.add_plugins(RumClientPlugin {
        socket_path: socket_path.clone(),
    });
    app.insert_resource(StatusClientState {
        request_id,
        sent: false,
        result: result.clone(),
    });
    app.add_systems(Update, (send_status_request, receive_status_response));
    ecsdk_app::run_async(app, rx).await;

    result
        .lock()
        .expect("status result lock poisoned")
        .clone()
        .ok_or_else(|| RumError::Daemon {
            message: format!(
                "did not receive status response from daemon for '{}'",
                sys_config.display_name()
            ),
        })
}

pub async fn request_ssh_config(sys_config: &SystemConfig) -> Result<String, RumError> {
    let socket_path = crate::paths::socket_path(&sys_config.id, sys_config.name.as_deref());
    let request_id = next_request_id();
    let result = Arc::new(Mutex::new(None));

    let (mut app, rx) = ecsdk_app::setup::<crate::lifecycle::RumMessage>();
    app.add_plugins(RumClientPlugin {
        socket_path: socket_path.clone(),
    });
    app.insert_resource(SshConfigClientState {
        request_id,
        sent: false,
        result: result.clone(),
    });
    app.add_systems(Update, (send_ssh_config_request, receive_ssh_config_response));
    ecsdk_app::run_async(app, rx).await;

    result
        .lock()
        .expect("ssh-config result lock poisoned")
        .clone()
        .ok_or_else(|| RumError::Daemon {
            message: format!(
                "did not receive ssh-config response from daemon for '{}'",
                sys_config.display_name()
            ),
        })?
        .map_err(|message| RumError::Daemon { message })
}

pub async fn request_shutdown(sys_config: &SystemConfig, force: bool) -> Result<(), RumError> {
    let socket_path = crate::paths::socket_path(&sys_config.id, sys_config.name.as_deref());
    let sent = Arc::new(Mutex::new(false));

    let (mut app, rx) = ecsdk_app::setup::<crate::lifecycle::RumMessage>();
    app.add_plugins(RumClientPlugin {
        socket_path: socket_path.clone(),
    });
    app.insert_resource(ShutdownClientState {
        sent: sent.clone(),
        force,
    });
    app.add_systems(Update, send_shutdown_request);
    ecsdk_app::run_async(app, rx).await;

    if *sent.lock().expect("shutdown sent lock poisoned") {
        Ok(())
    } else {
        Err(RumError::Daemon {
            message: format!(
                "could not connect to daemon for '{}'",
                sys_config.display_name()
            ),
        })
    }
}
