use std::time::Duration;

use bevy::ecs::prelude::*;
use bevy_replicon::shared::message::client_event::ClientTriggerExt;
use ecsdk_core::CmdQueue;

use crate::cli::OutputFormat;
use crate::config::SystemConfig;
use crate::daemon;
use crate::error::RumError;
use crate::lifecycle::RumMessage;
use crate::render::RumRenderPlugin;
use crate::replicon::{RumClientPlugin, ShutdownRequest};

/// Run `rum up` — spawn daemon if needed, then connect as replicon client.
pub async fn run_up(
    sys_config: &SystemConfig,
    reset: bool,
    detach: bool,
    output_format: &OutputFormat,
) -> Result<(), RumError> {
    // --reset: wipe artifacts first
    if reset {
        crate::vm::destroy::destroy_vm(sys_config).await.ok();
    }

    let socket_path = crate::paths::socket_path(&sys_config.id, sys_config.name.as_deref());

    // Spawn daemon if not already running
    if !daemon_is_running(&socket_path).await {
        daemon::spawn_background(sys_config)?;
        if !wait_for_daemon(&socket_path).await {
            return Err(RumError::Daemon {
                message: "daemon did not become ready within 10s".into(),
            });
        }
    }

    // --detach: daemon is running, we're done
    if detach {
        eprintln!("Daemon started for '{}'.", sys_config.display_name());
        return Ok(());
    }

    // Run client ECS app with replicon + render
    let (mut app, rx) = ecsdk_app::setup::<RumMessage>();
    app.add_plugins(RumClientPlugin {
        socket_path: socket_path.clone(),
    });
    app.add_plugins(RumRenderPlugin(output_format.clone()));

    // Ctrl+C: send ShutdownRequest client event to daemon
    let cmd_queue = app.world().resource::<CmdQueue>().clone();
    tokio::spawn(async move {
        let mut first = true;
        loop {
            tokio::signal::ctrl_c().await.ok();
            if first {
                cmd_queue
                    .send(|world: &mut World| {
                        world.commands().client_trigger(ShutdownRequest);
                    })
                    .wake();
                first = false;
            } else {
                cmd_queue
                    .send(|world: &mut World| {
                        world.resource_mut::<ecsdk_core::AppExit>().0 = true;
                    })
                    .wake();
            }
        }
    });

    ecsdk_app::run_async(&mut app, rx).await;
    Ok(())
}

/// Check if a daemon is already listening on the socket.
async fn daemon_is_running(socket_path: &std::path::Path) -> bool {
    tokio::net::UnixStream::connect(socket_path).await.is_ok()
}

/// Wait for the daemon to become ready (socket connectable).
async fn wait_for_daemon(socket_path: &std::path::Path) -> bool {
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(200)).await;
        if daemon_is_running(socket_path).await {
            return true;
        }
    }
    false
}
