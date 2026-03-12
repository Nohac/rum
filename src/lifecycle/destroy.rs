use seldom_state::prelude::*;

use crate::phase::vm_phase::*;

pub fn build_sm() -> StateMachine {
    StateMachine::default()
        .trans::<Destroying, _>(done(Some(Done::Success)), Destroyed)
        .trans::<Destroying, _>(done(Some(Done::Failure)), Failed)
        .set_trans_logging(true)
}

use bevy::ecs::prelude::*;
use ecsdk_tasks::{SpawnTask, TaskQueue};

type Tq = TaskQueue<super::RumMessage>;

pub fn on_destroying(
    trigger: On<Insert, Destroying>,
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
        match crate::vm::destroy::destroy_vm(&sc).await {
            Ok(()) => {
                cmd.send(move |world: &mut World| {
                    world.entity_mut(entity).insert(Done::Success);
                })
                .wake();
            }
            Err(e) => {
                let msg = e.to_string();
                cmd.send(move |world: &mut World| {
                    world
                        .entity_mut(entity)
                        .insert((super::terminal::VmError(msg), Done::Failure));
                })
                .wake();
            }
        }
    });
}
