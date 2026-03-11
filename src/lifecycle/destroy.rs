use seldom_state::prelude::*;

use crate::phase::vm_phase::*;

pub fn build_sm() -> StateMachine {
    StateMachine::default()
        .trans::<Destroying, _>(done(Some(Done::Success)), Destroyed)
        .trans::<Destroying, _>(done(Some(Done::Failure)), Failed)
        .set_trans_logging(true)
}
