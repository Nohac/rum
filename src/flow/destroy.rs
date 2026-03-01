//! Destroy flow: force-kill VM, undefine domain, remove all artifacts.

use crate::vm_state::VmState;
use super::{Effect, Event, Flow};

pub struct DestroyFlow;

impl Flow for DestroyFlow {
    fn valid_entry_states(&self) -> &[VmState] {
        // Can destroy from any state except Virgin (nothing to destroy)
        &[
            VmState::ImageCached,
            VmState::Prepared,
            VmState::PartialBoot,
            VmState::Provisioned,
            VmState::Running,
            VmState::RunningStale,
        ]
    }

    fn expected_steps(&self, entry_state: &VmState) -> usize {
        match entry_state {
            VmState::Running | VmState::RunningStale => 2, // destroy + cleanup
            _ => 1, // cleanup only
        }
    }

    fn transition(&self, state: &VmState, event: &Event) -> (VmState, Vec<Effect>) {
        match (state, event) {
            // ── FlowStarted: running VMs need force-destroy first ──
            (VmState::Running | VmState::RunningStale, Event::FlowStarted) => {
                (VmState::Running, vec![Effect::DestroyDomain])
            }
            // Non-running states: skip destroy, go straight to cleanup
            (_, Event::FlowStarted) => {
                (*state, vec![Effect::CleanupArtifacts])
            }

            // ── Domain destroyed or stopped → cleanup artifacts ──
            (_, Event::DestroyComplete) | (_, Event::DomainStopped) => {
                (VmState::Virgin, vec![Effect::CleanupArtifacts])
            }

            // ── Cleanup complete → terminal ──
            (_, Event::CleanupComplete) => {
                (VmState::Virgin, vec![])
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

    // ── Happy path from running ──

    #[test]
    fn happy_path_from_running() {
        let flow = DestroyFlow;

        let (state, effects) = flow.transition(&VmState::Running, &Event::FlowStarted);
        assert_eq!(state, VmState::Running);
        assert_eq!(effects, vec![Effect::DestroyDomain]);

        let (state, effects) = flow.transition(&state, &Event::DestroyComplete);
        assert_eq!(state, VmState::Virgin);
        assert_eq!(effects, vec![Effect::CleanupArtifacts]);

        let (state, effects) = flow.transition(&state, &Event::CleanupComplete);
        assert_eq!(state, VmState::Virgin);
        assert!(effects.is_empty());
    }

    #[test]
    fn happy_path_from_running_stale() {
        let flow = DestroyFlow;

        let (state, effects) = flow.transition(&VmState::RunningStale, &Event::FlowStarted);
        assert_eq!(state, VmState::Running);
        assert_eq!(effects, vec![Effect::DestroyDomain]);
    }

    // ── Happy path from non-running ──

    #[test]
    fn happy_path_from_provisioned() {
        let flow = DestroyFlow;

        let (state, effects) = flow.transition(&VmState::Provisioned, &Event::FlowStarted);
        assert_eq!(state, VmState::Provisioned);
        assert_eq!(effects, vec![Effect::CleanupArtifacts]);

        let (state, effects) = flow.transition(&state, &Event::CleanupComplete);
        assert_eq!(state, VmState::Virgin);
        assert!(effects.is_empty());
    }

    #[test]
    fn happy_path_from_prepared() {
        let flow = DestroyFlow;

        let (state, effects) = flow.transition(&VmState::Prepared, &Event::FlowStarted);
        assert_eq!(state, VmState::Prepared);
        assert_eq!(effects, vec![Effect::CleanupArtifacts]);
    }

    #[test]
    fn happy_path_from_image_cached() {
        let flow = DestroyFlow;

        let (state, effects) = flow.transition(&VmState::ImageCached, &Event::FlowStarted);
        assert_eq!(state, VmState::ImageCached);
        assert_eq!(effects, vec![Effect::CleanupArtifacts]);
    }

    #[test]
    fn happy_path_from_partial_boot() {
        let flow = DestroyFlow;

        let (state, effects) = flow.transition(&VmState::PartialBoot, &Event::FlowStarted);
        assert_eq!(state, VmState::PartialBoot);
        assert_eq!(effects, vec![Effect::CleanupArtifacts]);
    }

    // ── Domain stopped event ──

    #[test]
    fn domain_stopped_triggers_cleanup() {
        let flow = DestroyFlow;
        let (state, effects) = flow.transition(&VmState::Running, &Event::DomainStopped);
        assert_eq!(state, VmState::Virgin);
        assert_eq!(effects, vec![Effect::CleanupArtifacts]);
    }

    // ── Unknown events ──

    #[test]
    fn unknown_event_returns_same_state() {
        let flow = DestroyFlow;
        let (state, effects) = flow.transition(&VmState::Running, &Event::Detach);
        assert_eq!(state, VmState::Running);
        assert!(effects.is_empty());
    }

    #[test]
    fn services_started_ignored() {
        let flow = DestroyFlow;
        let (state, effects) = flow.transition(&VmState::Provisioned, &Event::ServicesStarted);
        assert_eq!(state, VmState::Provisioned);
        assert!(effects.is_empty());
    }

    // ── Entry states ──

    #[test]
    fn valid_entry_states() {
        let flow = DestroyFlow;
        let states = flow.valid_entry_states();
        assert!(!states.contains(&VmState::Virgin));
        assert!(states.contains(&VmState::Running));
        assert!(states.contains(&VmState::RunningStale));
        assert!(states.contains(&VmState::Provisioned));
        assert!(states.contains(&VmState::Prepared));
        assert!(states.contains(&VmState::ImageCached));
        assert!(states.contains(&VmState::PartialBoot));
    }
}
