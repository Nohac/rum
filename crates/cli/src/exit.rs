use ecsdk::prelude::*;
use orchestrator::instance::instance_phase::{Failed, Running, Stopped};
use orchestrator::EntityError;

/// Exit the local client when the daemon disconnects.
pub fn on_server_disconnect(
    mut disconnects: MessageReader<ServerDisconnected>,
    mut exit: MessageWriter<AppExit>,
) {
    if disconnects.read().next().is_some() {
        tracing::info!("rum daemon disconnected");
        exit.write(AppExit::Success);
    }
}

/// Exit once the managed instance reaches running.
pub fn on_running(
    _trigger: On<Add, Running>,
    mut exit: MessageWriter<AppExit>,
) {
    tracing::info!("managed instance reached running state");
    exit.write(AppExit::Success);
}

/// Exit once the managed instance reaches stopped.
pub fn on_stopped(
    _trigger: On<Add, Stopped>,
    mut exit: MessageWriter<AppExit>,
) {
    tracing::info!("managed instance reached stopped state");
    exit.write(AppExit::Success);
}

/// Exit and log the recorded entity error once the instance fails.
pub fn on_failed(
    trigger: On<Add, Failed>,
    errors: Query<&EntityError>,
    mut exit: MessageWriter<AppExit>,
) {
    let entity = trigger.event_target();
    if let Ok(error) = errors.get(entity) {
        tracing::error!(entity = entity.index().index(), error = %error.0, "managed instance failed");
    } else {
        tracing::error!(entity = entity.index().index(), "managed instance failed");
    }
    exit.write(AppExit::Success);
}
