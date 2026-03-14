use bevy::app::App;
use ecsdk_app::Receivers;

use crate::config::SystemConfig;
use crate::error::RumError;
use crate::lifecycle::RumMessage;
use crate::replicon::{RumClientCorePlugin, connect_client};

pub async fn ensure_daemon(sys_config: &SystemConfig) -> Result<(), RumError> {
    let socket_path = crate::paths::socket_path(&sys_config.id, sys_config.name.as_deref());
    if tokio::net::UnixStream::connect(&socket_path).await.is_err() {
        crate::daemon::spawn_background(sys_config).await?;
    }
    Ok(())
}

pub async fn connect_existing_daemon(
    sys_config: &SystemConfig,
) -> Result<(App, Receivers<RumMessage>), RumError> {
    if !crate::daemon::is_daemon_running(sys_config) {
        return Err(RumError::Daemon {
            message: format!(
                "no daemon running for '{}'. Run `rum up` first.",
                sys_config.display_name()
            ),
        });
    }

    let socket_path = crate::paths::socket_path(&sys_config.id, sys_config.name.as_deref());
    let stream = tokio::net::UnixStream::connect(&socket_path)
        .await
        .map_err(|e| RumError::Daemon {
            message: format!(
                "failed to connect to daemon for '{}': {e}",
                sys_config.display_name()
            ),
        })?;

    let (mut app, rx) = ecsdk_app::setup::<RumMessage>();
    app.add_plugins(RumClientCorePlugin);
    connect_client(app.world_mut(), stream);
    Ok((app, rx))
}

pub async fn ensure_daemon_and_connect(
    sys_config: &SystemConfig,
) -> Result<(App, Receivers<RumMessage>), RumError> {
    ensure_daemon(sys_config).await?;
    connect_existing_daemon(sys_config).await
}
