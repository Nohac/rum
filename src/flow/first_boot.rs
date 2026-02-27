//! FirstBoot flow: full pipeline from Virgin/ImageCached to Running.

use crate::vm_state::VmState;
use super::{Effect, Event, Flow};

pub struct FirstBootFlow {
    scripts: Vec<String>,
}

impl FirstBootFlow {
    pub fn new(scripts: Vec<String>) -> Self {
        Self { scripts }
    }
}

impl Flow for FirstBootFlow {
    fn valid_entry_states(&self) -> &[VmState] {
        &[VmState::Virgin, VmState::ImageCached, VmState::Prepared, VmState::PartialBoot]
    }

    fn transition(&self, state: &VmState, event: &Event) -> (VmState, Vec<Effect>) {
        todo!("FirstBootFlow transition table")
    }
}
