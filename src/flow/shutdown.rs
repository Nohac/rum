//! Shutdown flow: ACPI shutdown with timeout + force fallback.

use crate::vm_state::VmState;
use super::{Effect, Event, Flow};

pub struct ShutdownFlow;

impl Flow for ShutdownFlow {
    fn valid_entry_states(&self) -> &[VmState] {
        &[VmState::Running, VmState::RunningStale]
    }

    fn transition(&self, state: &VmState, event: &Event) -> (VmState, Vec<Effect>) {
        todo!("ShutdownFlow transition table")
    }
}
