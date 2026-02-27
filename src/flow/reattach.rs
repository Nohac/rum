//! Reattach flow: connect to an already-running VM.

use crate::vm_state::VmState;
use super::{Effect, Event, Flow};

pub struct ReattachFlow;

impl Flow for ReattachFlow {
    fn valid_entry_states(&self) -> &[VmState] {
        &[VmState::Running]
    }

    fn transition(&self, state: &VmState, event: &Event) -> (VmState, Vec<Effect>) {
        todo!("ReattachFlow transition table")
    }
}
