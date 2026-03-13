use bevy::ecs::prelude::*;
use ecsdk_tasks::{SpawnTask, TaskQueue};

use crate::phase::vm_phase::*;

type Tq = TaskQueue<super::RumMessage>;

pub fn on_shutting_down(
    trigger: On<Insert, ShuttingDown>,
    mut commands: Commands,
    configs: Query<&super::prepare::VmConfig>,
    identities: Query<&super::VmIdentity>,
) {
    let entity = trigger.event_target();
    let Ok(config) = configs.get(entity) else {
        return;
    };
    let Ok(identity) = identities.get(entity) else {
        return;
    };
    let sc = config.0.clone();
    let vm = identity.clone();

    commands.entity(entity).spawn_task(move |cmd: Tq| async move {
        if let Err(e) = crate::vm::shutdown::shutdown_vm(&sc).await {
            tracing::warn!("shutdown failed: {e}");
        }
        super::update_vm_phase(&cmd, vm, true, None);
    });
}
