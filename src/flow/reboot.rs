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

    /// Find the index of the next script after the one that just completed.
    fn next_script_after(&self, name: &str) -> Option<&str> {
        let idx = self.boot_scripts.iter().position(|s| s == name)?;
        self.boot_scripts.get(idx + 1).map(|s| s.as_str())
    }
}

impl Flow for RebootFlow {
    fn valid_entry_states(&self) -> &[VmState] {
        &[VmState::Provisioned]
    }

    fn transition(&self, state: &VmState, event: &Event) -> (VmState, Vec<Effect>) {
        match (state, event) {
            // ── FlowStarted → boot the VM ──
            (VmState::Provisioned, Event::FlowStarted) => {
                (VmState::Provisioned, vec![Effect::BootVm])
            }

            // ── Domain started → connect agent ──
            (VmState::Provisioned, Event::DomainStarted) => {
                (VmState::PartialBoot, vec![Effect::ConnectAgent])
            }

            // ── Agent connected → run boot scripts or start services ──
            (VmState::PartialBoot, Event::AgentConnected) => {
                if self.boot_scripts.is_empty() {
                    (VmState::Running, vec![Effect::StartServices])
                } else {
                    let first = self.boot_scripts[0].clone();
                    (VmState::PartialBoot, vec![Effect::RunScript { name: first }])
                }
            }

            // ── Script completed → next script or start services ──
            (VmState::PartialBoot, Event::ScriptCompleted { name }) => {
                if let Some(next) = self.next_script_after(name) {
                    (VmState::PartialBoot, vec![Effect::RunScript { name: next.to_string() }])
                } else {
                    (VmState::Running, vec![Effect::StartServices])
                }
            }

            // ── Services started → terminal ──
            (VmState::Running, Event::ServicesStarted) => {
                (VmState::Running, vec![])
            }

            // ── Client commands ──
            (_, Event::InitShutdown) => {
                (VmState::Running, vec![Effect::ShutdownDomain])
            }
            (_, Event::ShutdownComplete) => {
                (VmState::Provisioned, vec![])
            }
            (_, Event::ForceStop) => {
                // Don't destroy, just stop
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
    fn happy_path_no_scripts() {
        let flow = RebootFlow::new(vec![]);

        let (state, effects) = flow.transition(&VmState::Provisioned, &Event::FlowStarted);
        assert_eq!(state, VmState::Provisioned);
        assert_eq!(effects, vec![Effect::BootVm]);

        let (state, effects) = flow.transition(&state, &Event::DomainStarted);
        assert_eq!(state, VmState::PartialBoot);
        assert_eq!(effects, vec![Effect::ConnectAgent]);

        let (state, effects) = flow.transition(&state, &Event::AgentConnected);
        assert_eq!(state, VmState::Running);
        assert_eq!(effects, vec![Effect::StartServices]);

        let (state, effects) = flow.transition(&state, &Event::ServicesStarted);
        assert_eq!(state, VmState::Running);
        assert!(effects.is_empty());
    }

    #[test]
    fn happy_path_with_boot_scripts() {
        let flow = RebootFlow::new(vec!["boot-a.sh".into(), "boot-b.sh".into()]);

        let (state, _) = flow.transition(&VmState::Provisioned, &Event::FlowStarted);
        let (state, _) = flow.transition(&state, &Event::DomainStarted);

        // Agent connected → first boot script
        let (state, effects) = flow.transition(&state, &Event::AgentConnected);
        assert_eq!(state, VmState::PartialBoot);
        assert_eq!(effects, vec![Effect::RunScript { name: "boot-a.sh".into() }]);

        // First done → second
        let (state, effects) = flow.transition(&state, &Event::ScriptCompleted { name: "boot-a.sh".into() });
        assert_eq!(state, VmState::PartialBoot);
        assert_eq!(effects, vec![Effect::RunScript { name: "boot-b.sh".into() }]);

        // Second done → start services
        let (state, effects) = flow.transition(&state, &Event::ScriptCompleted { name: "boot-b.sh".into() });
        assert_eq!(state, VmState::Running);
        assert_eq!(effects, vec![Effect::StartServices]);
    }

    // ── Client commands ──

    #[test]
    fn init_shutdown() {
        let flow = RebootFlow::new(vec![]);
        let (state, effects) = flow.transition(&VmState::Running, &Event::InitShutdown);
        assert_eq!(state, VmState::Running);
        assert_eq!(effects, vec![Effect::ShutdownDomain]);
    }

    #[test]
    fn shutdown_complete() {
        let flow = RebootFlow::new(vec![]);
        let (state, effects) = flow.transition(&VmState::Running, &Event::ShutdownComplete);
        assert_eq!(state, VmState::Provisioned);
        assert!(effects.is_empty());
    }

    #[test]
    fn force_stop_does_not_destroy() {
        let flow = RebootFlow::new(vec![]);
        let (state, effects) = flow.transition(&VmState::Running, &Event::ForceStop);
        assert_eq!(state, VmState::Provisioned);
        assert_eq!(effects, vec![Effect::ShutdownDomain]);
    }

    #[test]
    fn force_stop_from_partial_boot() {
        let flow = RebootFlow::new(vec![]);
        let (state, effects) = flow.transition(&VmState::PartialBoot, &Event::ForceStop);
        assert_eq!(state, VmState::Provisioned);
        assert_eq!(effects, vec![Effect::ShutdownDomain]);
    }

    #[test]
    fn init_shutdown_from_partial_boot() {
        let flow = RebootFlow::new(vec![]);
        let (state, effects) = flow.transition(&VmState::PartialBoot, &Event::InitShutdown);
        assert_eq!(state, VmState::Running);
        assert_eq!(effects, vec![Effect::ShutdownDomain]);
    }

    // ── Unknown events ──

    #[test]
    fn unknown_event_returns_same_state() {
        let flow = RebootFlow::new(vec![]);
        let (state, effects) = flow.transition(&VmState::Provisioned, &Event::ServicesStarted);
        assert_eq!(state, VmState::Provisioned);
        assert!(effects.is_empty());
    }

    #[test]
    fn detach_returns_same_state() {
        let flow = RebootFlow::new(vec![]);
        let (state, effects) = flow.transition(&VmState::Running, &Event::Detach);
        assert_eq!(state, VmState::Running);
        assert!(effects.is_empty());
    }

    // ── Entry states ──

    #[test]
    fn valid_entry_states() {
        let flow = RebootFlow::new(vec![]);
        let states = flow.valid_entry_states();
        assert_eq!(states, &[VmState::Provisioned]);
    }
}
