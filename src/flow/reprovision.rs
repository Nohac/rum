//! Reprovision flow: re-run provision scripts on a running VM.

use crate::vm_state::VmState;
use super::{Effect, Event, Flow};

pub struct ReprovisionFlow {
    scripts: Vec<String>,
}

impl ReprovisionFlow {
    pub fn new(scripts: Vec<String>) -> Self {
        Self { scripts }
    }
}

impl Flow for ReprovisionFlow {
    fn valid_entry_states(&self) -> &[VmState] {
        &[VmState::Running]
    }

    fn transition(&self, state: &VmState, event: &Event) -> (VmState, Vec<Effect>) {
        todo!("ReprovisionFlow transition table")
    }
}
