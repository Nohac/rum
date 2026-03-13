use bevy::ecs::prelude::*;
use bevy_replicon::prelude::Replicated;
use ecsdk_core::ApplyMessage;
use seldom_state::prelude::*;

use crate::config::SystemConfig;
use crate::phase::{FlowIntent, ShutdownRequested, VmPhase};

#[derive(Clone, Debug)]
pub enum RumMessage {
    SpawnVm(Box<SpawnVmData>),
    MarkDone { entity: Entity, success: bool },
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
            Self::MarkDone { entity, success } => {
                if let Ok(mut e) = world.get_entity_mut(*entity) {
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
