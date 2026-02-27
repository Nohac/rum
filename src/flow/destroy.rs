//! Destroy flow: force-kill VM, undefine domain, remove all artifacts.

use crate::vm_state::VmState;
use super::{Effect, Event, Flow};

pub struct DestroyFlow;

impl Flow for DestroyFlow {
    fn valid_entry_states(&self) -> &[VmState] {
        // Can destroy from any state except Virgin (nothing to destroy)
        &[
            VmState::ImageCached,
            VmState::Prepared,
            VmState::PartialBoot,
            VmState::Provisioned,
            VmState::Running,
            VmState::RunningStale,
        ]
    }

    fn transition(&self, state: &VmState, event: &Event) -> (VmState, Vec<Effect>) {
        todo!("DestroyFlow transition table")
    }
}
