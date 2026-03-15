use bevy::app::prelude::*;
use bevy::ecs::prelude::*;
use bevy_replicon::prelude::ClientTriggerExt;

use crate::cli::OutputFormat;
use crate::config::SystemConfig;
use crate::daemon::StatusInfo;
use crate::error::RumError;
use crate::lifecycle::VmError;
use crate::phase::VmPhase;
use crate::render::RumRenderPlugin;
use crate::replicon::{
    DaemonControl, DaemonSnapshot, ForceStopRequest, ShutdownRequest, SshConfigRequest,
    StatusRequest,
};

fn daemon_snapshot(app: &mut App) -> Option<DaemonSnapshot> {
    let world = app.world_mut();
    let mut query = world.query_filtered::<&DaemonSnapshot, With<DaemonControl>>();
    query.iter(world).next().cloned()
}

fn shutdown_failure(app: &mut App) -> Option<String> {
    let world = app.world_mut();
    let mut query = world.query::<(&VmPhase, Option<&VmError>)>();
    for (phase, error) in query.iter(world) {
        if *phase == VmPhase::Failed {
            return Some(
                error
                    .map(|err| err.0.clone())
                    .unwrap_or_else(|| "shutdown failed".to_string()),
            );
        }
    }
    None
}

#[derive(Resource, Default)]
struct StatusRequestState {
    baseline_revision: Option<u64>,
    sent: bool,
}

#[derive(Resource, Default)]
struct SshConfigRequestState {
    baseline_revision: Option<u64>,
    sent: bool,
}

fn send_shutdown_request(mut commands: Commands) {
    commands.client_trigger(ShutdownRequest);
}

fn send_force_stop_request(
    mut commands: Commands,
    mut exit: ResMut<ecsdk_core::AppExit>,
) {
    commands.client_trigger(ForceStopRequest);
    exit.0 = true;
}

fn request_status_snapshot(
    mut commands: Commands,
    snapshots: Query<&DaemonSnapshot, With<DaemonControl>>,
    mut request: ResMut<StatusRequestState>,
) {
    if request.sent {
        return;
    }
    let Ok(snapshot) = snapshots.single() else {
        return;
    };

    request.baseline_revision = Some(snapshot.status_revision);
    request.sent = true;
    commands.client_trigger(StatusRequest);
}

fn request_ssh_config_snapshot(
    mut commands: Commands,
    snapshots: Query<&DaemonSnapshot, With<DaemonControl>>,
    mut request: ResMut<SshConfigRequestState>,
) {
    if request.sent {
        return;
    }
    let Ok(snapshot) = snapshots.single() else {
        return;
    };

    request.baseline_revision = Some(snapshot.ssh_config_revision);
    request.sent = true;
    commands.client_trigger(SshConfigRequest);
}

fn exit_on_status_snapshot(
    request: Res<StatusRequestState>,
    snapshots: Query<&DaemonSnapshot, With<DaemonControl>>,
    mut exit: ResMut<ecsdk_core::AppExit>,
) {
    let Some(baseline) = request.baseline_revision else {
        return;
    };
    let Ok(snapshot) = snapshots.single() else {
        return;
    };
    if request.sent && snapshot.status_ready && snapshot.status_revision > baseline {
        exit.0 = true;
    }
}

fn exit_on_ssh_config_snapshot(
    request: Res<SshConfigRequestState>,
    snapshots: Query<&DaemonSnapshot, With<DaemonControl>>,
    mut exit: ResMut<ecsdk_core::AppExit>,
) {
    let Some(baseline) = request.baseline_revision else {
        return;
    };
    let Ok(snapshot) = snapshots.single() else {
        return;
    };
    if request.sent && snapshot.ssh_config_ready && snapshot.ssh_config_revision > baseline {
        exit.0 = true;
    }
}

fn exit_on_terminal_phase(
    mut exit: ResMut<ecsdk_core::AppExit>,
    phases: Query<&VmPhase, Changed<VmPhase>>,
) {
    for phase in &phases {
        if phase.is_terminal() {
            exit.0 = true;
            return;
        }
    }
}

pub async fn request_status(sys_config: &SystemConfig) -> Result<StatusInfo, RumError> {
    let (mut app, rx) = crate::app::connect_existing_daemon(sys_config).await?;
    app.init_resource::<StatusRequestState>();
    app.add_systems(Update, (request_status_snapshot, exit_on_status_snapshot).chain());
    ecsdk_app::run_async(&mut app, rx).await;

    daemon_snapshot(&mut app)
        .filter(|snapshot| snapshot.status_ready)
        .map(|snapshot| StatusInfo {
            state: snapshot.state,
            ips: snapshot.ips,
            daemon_running: snapshot.daemon_running,
        })
        .ok_or_else(|| RumError::Daemon {
            message: format!(
                "did not receive status from daemon for '{}'",
                sys_config.display_name()
            ),
        })
}

pub async fn request_ssh_config(sys_config: &SystemConfig) -> Result<String, RumError> {
    let (mut app, rx) = crate::app::connect_existing_daemon(sys_config).await?;
    app.init_resource::<SshConfigRequestState>();
    app.add_systems(
        Update,
        (request_ssh_config_snapshot, exit_on_ssh_config_snapshot).chain(),
    );
    ecsdk_app::run_async(&mut app, rx).await;

    let snapshot = daemon_snapshot(&mut app)
        .filter(|snapshot| snapshot.ssh_config_ready)
        .ok_or_else(|| RumError::Daemon {
            message: format!(
                "did not receive ssh-config from daemon for '{}'",
                sys_config.display_name()
            ),
        })?;

    if snapshot.ssh_config_error.is_empty() {
        Ok(snapshot.ssh_config)
    } else {
        Err(RumError::Daemon {
            message: snapshot.ssh_config_error,
        })
    }
}

pub async fn request_shutdown(
    sys_config: &SystemConfig,
    output_format: &OutputFormat,
) -> Result<(), RumError> {
    let (mut app, rx) = crate::app::connect_existing_daemon(sys_config).await?;
    app.add_plugins(RumRenderPlugin(output_format.clone()));
    app.add_systems(Startup, send_shutdown_request);
    app.add_systems(Update, exit_on_terminal_phase);
    ecsdk_app::run_async(&mut app, rx).await;

    if let Some(message) = shutdown_failure(&mut app) {
        Err(RumError::Daemon { message })
    } else {
        Ok(())
    }
}

pub async fn request_force_stop(sys_config: &SystemConfig) -> Result<(), RumError> {
    let (mut app, rx) = crate::app::connect_existing_daemon(sys_config).await?;
    app.add_systems(Startup, send_force_stop_request);
    ecsdk_app::run_async(&mut app, rx).await;
    Ok(())
}
