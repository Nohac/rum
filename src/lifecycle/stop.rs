use bevy::ecs::prelude::*;
use ecsdk_tasks::{SpawnTask, TaskQueue};
use seldom_state::prelude::*;

use crate::phase::vm_phase::*;

type Tq = TaskQueue<super::RumMessage>;

pub fn on_shutting_down(
    trigger: On<Insert, ShuttingDown>,
    mut commands: Commands,
    configs: Query<&super::prepare::VmConfig>,
) {
    let entity = trigger.event_target();
    let Ok(config) = configs.get(entity) else {
        return;
    };
    let sc = config.0.clone();

    commands.entity(entity).spawn_task(move |cmd: Tq| async move {
        let entity = cmd.entity();
        if let Err(e) = crate::vm::shutdown::shutdown_vm(&sc).await {
            tracing::warn!("shutdown failed: {e}");
        }
        cmd.send(move |world: &mut World| {
            world.entity_mut(entity).insert(Done::Success);
        })
        .wake();
    });
}
