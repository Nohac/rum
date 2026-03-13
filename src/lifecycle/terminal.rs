use bevy::ecs::prelude::*;

use crate::phase::FlowIntent;
use crate::phase::vm_phase::*;
use crate::phase::VmPhase;

#[derive(Component, serde::Serialize, serde::Deserialize)]
pub struct VmError(pub String);

pub fn advance_step_progress(
    mut query: Query<&mut crate::render::StepProgress, Changed<VmPhase>>,
) {
    for mut progress in &mut query {
        progress.current += 1;
    }
}

pub fn on_stopped(_trigger: On<Insert, Stopped>, mut exit: ResMut<ecsdk_core::AppExit>) {
    exit.0 = true;
}

pub fn on_destroyed(_trigger: On<Insert, Destroyed>, mut exit: ResMut<ecsdk_core::AppExit>) {
    exit.0 = true;
}

pub fn on_running(
    trigger: On<Insert, Running>,
    intents: Query<&FlowIntent>,
    configs: Query<&crate::lifecycle::VmConfig>,
) {
    let entity = trigger.event_target();
    let Ok(intent) = intents.get(entity) else {
        return;
    };
    if !matches!(intent, FlowIntent::FirstBoot | FlowIntent::Reboot) {
        return;
    }

    let Ok(config) = configs.get(entity) else {
        return;
    };

    let sys_config = &config.0;
    let marker = crate::paths::provisioned_marker(&sys_config.id, sys_config.name.as_deref());
    if !marker.exists() {
        let _ = std::fs::write(marker, b"");
    }
}

pub fn on_failed(
    trigger: On<Insert, Failed>,
    errors: Query<&VmError>,
    mut exit: ResMut<ecsdk_core::AppExit>,
) {
    let entity = trigger.event_target();
    if let Ok(err) = errors.get(entity) {
        tracing::error!("VM failed: {}", err.0);
    }
    exit.0 = true;
}
