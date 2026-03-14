use seldom_state::prelude::*;

use crate::phase::vm_phase::*;

use super::shutdown_requested;

pub fn build_sm() -> StateMachine {
    StateMachine::default()
        .trans::<StartingServices, _>(shutdown_requested, Destroying)
        .trans::<StartingServices, _>(done(Some(Done::Success)), Running)
        .trans::<Running, _>(shutdown_requested, ShuttingDown)
        .trans::<ShuttingDown, _>(done(Some(Done::Success)), Stopped)
        .trans::<Destroying, _>(done(Some(Done::Success)), Destroyed)
        .trans::<Destroying, _>(done(Some(Done::Failure)), Failed)
        .set_trans_logging(true)
}
