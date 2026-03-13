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
        match crate::vm::destroy::destroy_vm(&sc).await {
            Ok(()) => {
                super::update_vm_phase(&cmd, vm, true, None);
            }
            Err(e) => {
                let msg = e.to_string();
                super::update_vm_phase(&cmd, vm, false, Some(msg));
            }
        }
    });
}
