use seldom_state::prelude::*;

use crate::phase::vm_phase::*;

pub fn build_sm() -> StateMachine {
    StateMachine::default()
        .trans::<ShuttingDown, _>(done(Some(Done::Success)), Stopped)
        .set_trans_logging(true)
}
