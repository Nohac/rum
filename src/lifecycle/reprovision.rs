use seldom_state::prelude::*;

use crate::phase::vm_phase::*;

use super::shutdown_requested;

pub fn build_sm() -> StateMachine {
    StateMachine::default()
        .trans::<Provisioning, _>(done(Some(Done::Success)), StartingServices)
        .trans::<Provisioning, _>(done(Some(Done::Failure)), Failed)
        .trans::<StartingServices, _>(done(Some(Done::Success)), Running)
        .trans::<Running, _>(shutdown_requested, ShuttingDown)
        .trans::<ShuttingDown, _>(done(Some(Done::Success)), Stopped)
        .set_trans_logging(true)
}
