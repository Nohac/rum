//! Reprovision flow: re-run provision scripts on a running VM.

use crate::vm_state::VmState;
use super::{Effect, Event, Flow};

pub struct ReprovisionFlow {
    /// All scripts in execution order: system scripts first, then boot scripts.
    scripts: Vec<String>,
}

impl ReprovisionFlow {
    pub fn new(system_scripts: Vec<String>, boot_scripts: Vec<String>) -> Self {
        let mut scripts = system_scripts;
        scripts.extend(boot_scripts);
        Self { scripts }
    }

    /// Find the index of the next script after the one that just completed.
    fn next_script_after(&self, name: &str) -> Option<&str> {
        let idx = self.scripts.iter().position(|s| s == name)?;
        self.scripts.get(idx + 1).map(|s| s.as_str())
    }
}

impl Flow for ReprovisionFlow {
    fn valid_entry_states(&self) -> &[VmState] {
        &[VmState::Running]
    }

    fn expected_steps(&self, _entry_state: &VmState) -> usize {
        self.scripts.len()
    }

    fn transition(&self, state: &VmState, event: &Event) -> (VmState, Vec<Effect>) {
        match (state, event) {
            // ── FlowStarted → run first script or no-op ──
            (VmState::Running, Event::FlowStarted) => {
                if self.scripts.is_empty() {
                    (VmState::Running, vec![])
                } else {
                    let first = self.scripts[0].clone();
                    (VmState::Running, vec![Effect::RunScript { name: first }])
                }
            }

            // ── Script completed → next script or done ──
            (VmState::Running, Event::ScriptCompleted { name }) => {
                if let Some(next) = self.next_script_after(name) {
                    (VmState::Running, vec![Effect::RunScript { name: next.to_string() }])
                } else {
                    (VmState::Running, vec![])
                }
            }

            // ── Script failed → report error but stay running ──
            (VmState::Running, Event::ScriptFailed { .. }) => {
                (VmState::Running, vec![])
            }

            // ── Client commands ──
            (VmState::Running, Event::InitShutdown) => {
                (VmState::Running, vec![Effect::ShutdownDomain])
            }
            (VmState::Running, Event::ShutdownComplete) => {
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
    fn happy_path_no_scripts() {
        let flow = ReprovisionFlow::new(vec![], vec![]);

        let (state, effects) = flow.transition(&VmState::Running, &Event::FlowStarted);
        assert_eq!(state, VmState::Running);
        assert!(effects.is_empty());
    }

    #[test]
    fn happy_path_with_scripts() {
        let flow = ReprovisionFlow::new(vec!["a.sh".into()], vec!["b.sh".into(), "c.sh".into()]);

        let (state, effects) = flow.transition(&VmState::Running, &Event::FlowStarted);
        assert_eq!(state, VmState::Running);
        assert_eq!(effects, vec![Effect::RunScript { name: "a.sh".into() }]);

        let (state, effects) = flow.transition(&state, &Event::ScriptCompleted { name: "a.sh".into() });
        assert_eq!(state, VmState::Running);
        assert_eq!(effects, vec![Effect::RunScript { name: "b.sh".into() }]);

        let (state, effects) = flow.transition(&state, &Event::ScriptCompleted { name: "b.sh".into() });
        assert_eq!(state, VmState::Running);
        assert_eq!(effects, vec![Effect::RunScript { name: "c.sh".into() }]);

        let (state, effects) = flow.transition(&state, &Event::ScriptCompleted { name: "c.sh".into() });
        assert_eq!(state, VmState::Running);
        assert!(effects.is_empty());
    }

    // ── Script failure ──

    #[test]
    fn script_failed_stays_running() {
        let flow = ReprovisionFlow::new(vec!["a.sh".into()], vec!["b.sh".into()]);

        let (state, _) = flow.transition(&VmState::Running, &Event::FlowStarted);

        let (state, effects) = flow.transition(
            &state,
            &Event::ScriptFailed { name: "a.sh".into(), error: "exit 1".into() },
        );
        assert_eq!(state, VmState::Running);
        assert!(effects.is_empty());
    }

    // ── Client commands ──

    #[test]
    fn init_shutdown() {
        let flow = ReprovisionFlow::new(vec![], vec![]);
        let (state, effects) = flow.transition(&VmState::Running, &Event::InitShutdown);
        assert_eq!(state, VmState::Running);
        assert_eq!(effects, vec![Effect::ShutdownDomain]);
    }

    #[test]
    fn shutdown_complete() {
        let flow = ReprovisionFlow::new(vec![], vec![]);
        let (state, effects) = flow.transition(&VmState::Running, &Event::ShutdownComplete);
        assert_eq!(state, VmState::Provisioned);
        assert!(effects.is_empty());
    }

    // ── Unknown events ──

    #[test]
    fn unknown_event_returns_same_state() {
        let flow = ReprovisionFlow::new(vec![], vec![]);
        let (state, effects) = flow.transition(&VmState::Running, &Event::Detach);
        assert_eq!(state, VmState::Running);
        assert!(effects.is_empty());
    }

    #[test]
    fn domain_started_ignored() {
        let flow = ReprovisionFlow::new(vec![], vec![]);
        let (state, effects) = flow.transition(&VmState::Running, &Event::DomainStarted);
        assert_eq!(state, VmState::Running);
        assert!(effects.is_empty());
    }

    // ── Entry states ──

    #[test]
    fn valid_entry_states() {
        let flow = ReprovisionFlow::new(vec![], vec![]);
        let states = flow.valid_entry_states();
        assert_eq!(states, &[VmState::Running]);
    }
}
