//! Shutdown flow: ACPI shutdown with timeout + force fallback.

use crate::vm_state::VmState;
use super::{Effect, Event, Flow};

pub struct ShutdownFlow;

impl Flow for ShutdownFlow {
    fn valid_entry_states(&self) -> &[VmState] {
        &[VmState::Running, VmState::RunningStale]
    }

    fn expected_steps(&self, _entry_state: &VmState) -> usize {
        1 // shutdown
    }

    fn transition(&self, state: &VmState, event: &Event) -> (VmState, Vec<Effect>) {
        match (state, event) {
            // ── FlowStarted → initiate ACPI shutdown ──
            (VmState::Running | VmState::RunningStale, Event::FlowStarted) => {
                (VmState::Running, vec![Effect::ShutdownDomain])
            }

            // ── Shutdown completed gracefully ──
            (VmState::Running, Event::ShutdownComplete) => {
                (VmState::Provisioned, vec![])
            }

            // ── Force stop (timeout fallback) → destroy domain ──
            (VmState::Running, Event::ForceStop) => {
                (VmState::Running, vec![Effect::DestroyDomain])
            }

            // ── Domain stopped (after destroy or external stop) ──
            (VmState::Running, Event::DomainStopped) => {
                (VmState::Provisioned, vec![])
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
    fn happy_path_graceful_shutdown() {
        let flow = ShutdownFlow;

        let (state, effects) = flow.transition(&VmState::Running, &Event::FlowStarted);
        assert_eq!(state, VmState::Running);
        assert_eq!(effects, vec![Effect::ShutdownDomain]);

        let (state, effects) = flow.transition(&state, &Event::ShutdownComplete);
        assert_eq!(state, VmState::Provisioned);
        assert!(effects.is_empty());
    }

    #[test]
    fn happy_path_from_running_stale() {
        let flow = ShutdownFlow;

        let (state, effects) = flow.transition(&VmState::RunningStale, &Event::FlowStarted);
        assert_eq!(state, VmState::Running);
        assert_eq!(effects, vec![Effect::ShutdownDomain]);
    }

    #[test]
    fn force_stop_fallback() {
        let flow = ShutdownFlow;

        // Start shutdown
        let (state, _) = flow.transition(&VmState::Running, &Event::FlowStarted);

        // Timeout → force stop
        let (state, effects) = flow.transition(&state, &Event::ForceStop);
        assert_eq!(state, VmState::Running);
        assert_eq!(effects, vec![Effect::DestroyDomain]);

        // Domain stopped after destroy
        let (state, effects) = flow.transition(&state, &Event::DomainStopped);
        assert_eq!(state, VmState::Provisioned);
        assert!(effects.is_empty());
    }

    // ── Unknown events ──

    #[test]
    fn unknown_event_returns_same_state() {
        let flow = ShutdownFlow;
        let (state, effects) = flow.transition(&VmState::Running, &Event::Detach);
        assert_eq!(state, VmState::Running);
        assert!(effects.is_empty());
    }

    #[test]
    fn services_started_ignored() {
        let flow = ShutdownFlow;
        let (state, effects) = flow.transition(&VmState::Running, &Event::ServicesStarted);
        assert_eq!(state, VmState::Running);
        assert!(effects.is_empty());
    }

    // ── Entry states ──

    #[test]
    fn valid_entry_states() {
        let flow = ShutdownFlow;
        let states = flow.valid_entry_states();
        assert!(states.contains(&VmState::Running));
        assert!(states.contains(&VmState::RunningStale));
        assert_eq!(states.len(), 2);
    }
}
