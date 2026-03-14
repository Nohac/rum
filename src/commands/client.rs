use bevy::ecs::prelude::*;
use bevy_replicon::shared::message::client_event::ClientTriggerExt;
use ecsdk_core::CmdQueue;

use crate::cli::OutputFormat;
use crate::config::SystemConfig;
use crate::error::RumError;
use crate::render::RumRenderPlugin;
use crate::replicon::ShutdownRequest;

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

    // Spawn daemon if not already running
    crate::app::ensure_daemon(sys_config).await?;

    // --detach: daemon is running, we're done
    if detach {
        eprintln!("Daemon started for '{}'.", sys_config.display_name());
        return Ok(());
    }

    // Run client ECS app with replicon + render
    let (mut app, rx) = crate::app::ensure_daemon_and_connect(sys_config).await?;
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
