//! Reboot flow: boot a previously-provisioned VM (after `rum down`).

use crate::vm_state::VmState;
use super::{Effect, Event, Flow};

pub struct RebootFlow {
    boot_scripts: Vec<String>,
}

impl RebootFlow {
    pub fn new(boot_scripts: Vec<String>) -> Self {
        Self { boot_scripts }
    }
}

impl Flow for RebootFlow {
    fn valid_entry_states(&self) -> &[VmState] {
        &[VmState::Provisioned]
    }

    fn transition(&self, state: &VmState, event: &Event) -> (VmState, Vec<Effect>) {
        todo!("RebootFlow transition table")
    }
}
