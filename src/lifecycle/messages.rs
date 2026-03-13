use bevy::ecs::prelude::*;
use bevy_replicon::prelude::Replicated;
use ecsdk_core::ApplyMessage;
use ecsdk_tasks::TaskQueue;
use seldom_state::prelude::*;

use crate::config::SystemConfig;
use crate::phase::{FlowIntent, ShutdownRequested, VmPhase};

#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct VmIdentity(pub String);

impl VmIdentity {
    pub fn from_config(sys_config: &SystemConfig) -> Self {
        Self(sys_config.display_name().to_string())
    }
}

#[derive(Clone, Debug)]
pub enum RumMessage {
    SpawnVm(Box<SpawnVmData>),
    UpdateVmPhase {
        vm: VmIdentity,
        success: bool,
        error: Option<String>,
    },
    RequestShutdown,
    RequestForceStop,
}

#[derive(Clone, Debug)]
pub struct SpawnVmData {
    pub sys_config: SystemConfig,
    pub intent: FlowIntent,
    pub initial_phase: VmPhase,
    pub scripts: Vec<String>,
    pub total_steps: usize,
}

pub fn update_vm_phase(
    queue: &TaskQueue<RumMessage>,
    vm: VmIdentity,
    success: bool,
    error: Option<String>,
) {
    queue.send_state(RumMessage::UpdateVmPhase { vm, success, error });
    queue.wake();
}

impl ApplyMessage for RumMessage {
    fn apply(&self, world: &mut World) {
        match self {
            Self::SpawnVm(data) => {
                let SpawnVmData {
                    sys_config,
                    intent,
                    initial_phase,
                    scripts,
                    total_steps,
                } = data.as_ref();

                tracing::debug!(message = "SpawnVm", ?intent);
                let sm = super::machine::build_sm_for_intent(*intent);

                let mut entity = world.spawn((
                    VmIdentity::from_config(sys_config),
                    super::prepare::VmConfig(sys_config.clone()),
                    *intent,
                    *initial_phase,
                    sm,
                    super::provision::ScriptQueue::new(scripts.clone()),
                    crate::render::StepProgress {
                        current: 0,
                        total: *total_steps,
                    },
                    Replicated,
                ));
                initial_phase.insert_marker_world(&mut entity);
            }
            Self::UpdateVmPhase { vm, success, error } => {
                let target = {
                    let mut query = world.query::<(Entity, &VmIdentity)>();
                    query.iter(world).find_map(|(entity, identity)| {
                        (identity.0 == vm.0).then_some(entity)
                    })
                };

                let Some(target) = target else {
                    tracing::warn!(vm = %vm.0, "received UpdateVmPhase for unknown VM");
                    return;
                };

                if let Ok(mut e) = world.get_entity_mut(target) {
                    if let Some(error) = error {
                        e.insert(super::terminal::VmError(error.clone()));
                    }
                    if *success {
                        e.insert(Done::Success);
                    } else {
                        e.insert(Done::Failure);
                    }
                }
            }
            Self::RequestShutdown => {
                world.resource_mut::<ShutdownRequested>().0 = true;
            }
            Self::RequestForceStop => {
                world.resource_mut::<ShutdownRequested>().0 = true;
                world.resource_mut::<ecsdk_core::AppExit>().0 = true;
            }
        }
    }
}
