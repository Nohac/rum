//! Reattach flow: connect to an already-running VM.

use crate::vm_state::VmState;
use super::{Effect, Event, Flow};

pub struct ReattachFlow;

impl Flow for ReattachFlow {
    fn valid_entry_states(&self) -> &[VmState] {
        &[VmState::Running]
    }

    fn expected_steps(&self, _entry_state: &VmState) -> usize {
        1 // ready
    }

    fn transition(&self, state: &VmState, event: &Event) -> (VmState, Vec<Effect>) {
        match (state, event) {
            // ── FlowStarted → start services (log sub, port forwards) ──
            (VmState::Running, Event::FlowStarted) => {
                (VmState::Running, vec![Effect::StartServices])
            }

            // ── Services started → terminal ──
            (VmState::Running, Event::ServicesStarted) => {
                (VmState::Running, vec![])
            }

            // ── Client commands ──
            (VmState::Running, Event::InitShutdown) => {
                (VmState::Running, vec![Effect::ShutdownDomain])
            }
            (VmState::Running, Event::ShutdownComplete) => {
                (VmState::Provisioned, vec![])
            }
            (VmState::Running, Event::ForceStop) => {
                (VmState::Provisioned, vec![Effect::ShutdownDomain])
            }

            // ── Unknown event → same state, no effects ──
            _ => {
                (*state, vec![])
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Happy path ──

    #[test]
    fn happy_path() {
        let flow = ReattachFlow;

        let (state, effects) = flow.transition(&VmState::Running, &Event::FlowStarted);
        assert_eq!(state, VmState::Running);
        assert_eq!(effects, vec![Effect::StartServices]);

        let (state, effects) = flow.transition(&state, &Event::ServicesStarted);
        assert_eq!(state, VmState::Running);
        assert!(effects.is_empty());
    }

    // ── Client commands ──

    #[test]
    fn init_shutdown() {
        let flow = ReattachFlow;
        let (state, effects) = flow.transition(&VmState::Running, &Event::InitShutdown);
        assert_eq!(state, VmState::Running);
        assert_eq!(effects, vec![Effect::ShutdownDomain]);
    }

    #[test]
    fn shutdown_complete() {
        let flow = ReattachFlow;
        let (state, effects) = flow.transition(&VmState::Running, &Event::ShutdownComplete);
        assert_eq!(state, VmState::Provisioned);
        assert!(effects.is_empty());
    }

    #[test]
    fn force_stop() {
        let flow = ReattachFlow;
        let (state, effects) = flow.transition(&VmState::Running, &Event::ForceStop);
        assert_eq!(state, VmState::Provisioned);
        assert_eq!(effects, vec![Effect::ShutdownDomain]);
    }

    // ── Unknown events ──

    #[test]
    fn unknown_event_returns_same_state() {
        let flow = ReattachFlow;
        let (state, effects) = flow.transition(&VmState::Running, &Event::Detach);
        assert_eq!(state, VmState::Running);
        assert!(effects.is_empty());
    }

    #[test]
    fn domain_started_ignored() {
        let flow = ReattachFlow;
        let (state, effects) = flow.transition(&VmState::Running, &Event::DomainStarted);
        assert_eq!(state, VmState::Running);
        assert!(effects.is_empty());
    }

    // ── Entry states ──

    #[test]
    fn valid_entry_states() {
        let flow = ReattachFlow;
        let states = flow.valid_entry_states();
        assert_eq!(states, &[VmState::Running]);
    }
}
