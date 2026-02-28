//! FirstBoot flow: full pipeline from Virgin/ImageCached to Running.

use crate::vm_state::VmState;
use super::{Effect, Event, Flow};

pub struct FirstBootFlow {
    /// All scripts in execution order: system scripts first, then boot scripts.
    scripts: Vec<String>,
}

impl FirstBootFlow {
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

impl Flow for FirstBootFlow {
    fn valid_entry_states(&self) -> &[VmState] {
        &[VmState::Virgin, VmState::ImageCached, VmState::Prepared, VmState::PartialBoot]
    }

    fn transition(&self, state: &VmState, event: &Event) -> (VmState, Vec<Effect>) {
        match (state, event) {
            // ── FlowStarted from various entry states ──
            (VmState::Virgin, Event::FlowStarted) => {
                (VmState::ImageCached, vec![Effect::EnsureImage])
            }
            (VmState::ImageCached, Event::FlowStarted) => {
                // Re-verify image
                (VmState::ImageCached, vec![Effect::EnsureImage])
            }
            (VmState::Prepared, Event::FlowStarted) => {
                // Already prepared, skip to boot
                (VmState::Prepared, vec![Effect::BootVm])
            }
            (VmState::PartialBoot, Event::FlowStarted) => {
                // Retry boot
                (VmState::PartialBoot, vec![Effect::BootVm])
            }

            // ── Image ready → prepare VM ──
            (VmState::ImageCached, Event::ImageReady(_)) => {
                (VmState::Prepared, vec![Effect::PrepareVm])
            }

            // ── VM prepared → boot ──
            (VmState::Prepared, Event::VmPrepared) => {
                (VmState::Prepared, vec![Effect::BootVm])
            }

            // ── Domain started → connect agent ──
            (VmState::Prepared | VmState::PartialBoot, Event::DomainStarted) => {
                (VmState::PartialBoot, vec![Effect::ConnectAgent])
            }

            // ── Agent connected → run scripts or start services ──
            (VmState::PartialBoot, Event::AgentConnected) => {
                if self.scripts.is_empty() {
                    (VmState::Running, vec![Effect::StartServices])
                } else {
                    let first = self.scripts[0].clone();
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

            // ── Services started → terminal (wait for commands) ──
            (VmState::Running, Event::ServicesStarted) => {
                (VmState::Running, vec![])
            }

            // ── Client commands ──
            (VmState::Running, Event::InitShutdown) => {
                (VmState::Running, vec![Effect::ShutdownDomain])
            }
            (_, Event::ShutdownComplete) => {
                (VmState::Provisioned, vec![])
            }
            (_, Event::ForceStop) => {
                (VmState::Virgin, vec![Effect::DestroyDomain, Effect::CleanupArtifacts])
            }

            // ── Error events → clean up ──
            (_, Event::ImageFailed(_))
            | (_, Event::PrepareFailed(_))
            | (_, Event::BootFailed(_))
            | (_, Event::AgentTimeout(_))
            | (_, Event::ScriptFailed { .. }) => {
                (VmState::Virgin, vec![Effect::CleanupArtifacts])
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
    use std::path::PathBuf;

    // ── Happy path ──

    #[test]
    fn happy_path_from_virgin_no_scripts() {
        let flow = FirstBootFlow::new(vec![], vec![]);

        let (state, effects) = flow.transition(&VmState::Virgin, &Event::FlowStarted);
        assert_eq!(state, VmState::ImageCached);
        assert_eq!(effects, vec![Effect::EnsureImage]);

        let (state, effects) = flow.transition(&state, &Event::ImageReady(PathBuf::from("/img")));
        assert_eq!(state, VmState::Prepared);
        assert_eq!(effects, vec![Effect::PrepareVm]);

        let (state, effects) = flow.transition(&state, &Event::VmPrepared);
        assert_eq!(state, VmState::Prepared);
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
    fn happy_path_from_virgin_with_scripts() {
        let flow = FirstBootFlow::new(vec!["setup.sh".into()], vec!["deploy.sh".into()]);

        let (state, _) = flow.transition(&VmState::Virgin, &Event::FlowStarted);
        let (state, _) = flow.transition(&state, &Event::ImageReady(PathBuf::from("/img")));
        let (state, _) = flow.transition(&state, &Event::VmPrepared);
        let (state, _) = flow.transition(&state, &Event::DomainStarted);

        // Agent connected → first script
        let (state, effects) = flow.transition(&state, &Event::AgentConnected);
        assert_eq!(state, VmState::PartialBoot);
        assert_eq!(effects, vec![Effect::RunScript { name: "setup.sh".into() }]);

        // First script done → second script
        let (state, effects) = flow.transition(&state, &Event::ScriptCompleted { name: "setup.sh".into() });
        assert_eq!(state, VmState::PartialBoot);
        assert_eq!(effects, vec![Effect::RunScript { name: "deploy.sh".into() }]);

        // Second script done → start services
        let (state, effects) = flow.transition(&state, &Event::ScriptCompleted { name: "deploy.sh".into() });
        assert_eq!(state, VmState::Running);
        assert_eq!(effects, vec![Effect::StartServices]);
    }

    #[test]
    fn happy_path_from_image_cached() {
        let flow = FirstBootFlow::new(vec![], vec![]);

        let (state, effects) = flow.transition(&VmState::ImageCached, &Event::FlowStarted);
        assert_eq!(state, VmState::ImageCached);
        assert_eq!(effects, vec![Effect::EnsureImage]);

        let (state, effects) = flow.transition(&state, &Event::ImageReady(PathBuf::from("/img")));
        assert_eq!(state, VmState::Prepared);
        assert_eq!(effects, vec![Effect::PrepareVm]);
    }

    #[test]
    fn happy_path_from_prepared() {
        let flow = FirstBootFlow::new(vec![], vec![]);

        let (state, effects) = flow.transition(&VmState::Prepared, &Event::FlowStarted);
        assert_eq!(state, VmState::Prepared);
        assert_eq!(effects, vec![Effect::BootVm]);

        let (state, effects) = flow.transition(&state, &Event::DomainStarted);
        assert_eq!(state, VmState::PartialBoot);
        assert_eq!(effects, vec![Effect::ConnectAgent]);
    }

    #[test]
    fn happy_path_from_partial_boot() {
        let flow = FirstBootFlow::new(vec![], vec![]);

        let (state, effects) = flow.transition(&VmState::PartialBoot, &Event::FlowStarted);
        assert_eq!(state, VmState::PartialBoot);
        assert_eq!(effects, vec![Effect::BootVm]);

        let (state, effects) = flow.transition(&state, &Event::DomainStarted);
        assert_eq!(state, VmState::PartialBoot);
        assert_eq!(effects, vec![Effect::ConnectAgent]);
    }

    // ── Error events ──

    #[test]
    fn image_failed_cleans_up() {
        let flow = FirstBootFlow::new(vec![], vec![]);
        let (state, effects) = flow.transition(&VmState::ImageCached, &Event::ImageFailed("404".into()));
        assert_eq!(state, VmState::Virgin);
        assert_eq!(effects, vec![Effect::CleanupArtifacts]);
    }

    #[test]
    fn prepare_failed_cleans_up() {
        let flow = FirstBootFlow::new(vec![], vec![]);
        let (state, effects) = flow.transition(&VmState::Prepared, &Event::PrepareFailed("disk full".into()));
        assert_eq!(state, VmState::Virgin);
        assert_eq!(effects, vec![Effect::CleanupArtifacts]);
    }

    #[test]
    fn boot_failed_cleans_up() {
        let flow = FirstBootFlow::new(vec![], vec![]);
        let (state, effects) = flow.transition(&VmState::Prepared, &Event::BootFailed("no kvm".into()));
        assert_eq!(state, VmState::Virgin);
        assert_eq!(effects, vec![Effect::CleanupArtifacts]);
    }

    #[test]
    fn agent_timeout_cleans_up() {
        let flow = FirstBootFlow::new(vec![], vec![]);
        let (state, effects) = flow.transition(&VmState::PartialBoot, &Event::AgentTimeout("30s".into()));
        assert_eq!(state, VmState::Virgin);
        assert_eq!(effects, vec![Effect::CleanupArtifacts]);
    }

    #[test]
    fn script_failed_cleans_up() {
        let flow = FirstBootFlow::new(vec!["setup.sh".into()], vec![]);
        let (state, effects) = flow.transition(
            &VmState::PartialBoot,
            &Event::ScriptFailed { name: "setup.sh".into(), error: "exit 1".into() },
        );
        assert_eq!(state, VmState::Virgin);
        assert_eq!(effects, vec![Effect::CleanupArtifacts]);
    }

    // ── Client commands ──

    #[test]
    fn init_shutdown_when_running() {
        let flow = FirstBootFlow::new(vec![], vec![]);
        let (state, effects) = flow.transition(&VmState::Running, &Event::InitShutdown);
        assert_eq!(state, VmState::Running);
        assert_eq!(effects, vec![Effect::ShutdownDomain]);
    }

    #[test]
    fn shutdown_complete_from_running() {
        let flow = FirstBootFlow::new(vec![], vec![]);
        let (state, effects) = flow.transition(&VmState::Running, &Event::ShutdownComplete);
        assert_eq!(state, VmState::Provisioned);
        assert!(effects.is_empty());
    }

    #[test]
    fn force_stop_destroys_everything() {
        let flow = FirstBootFlow::new(vec![], vec![]);

        // ForceStop from PartialBoot
        let (state, effects) = flow.transition(&VmState::PartialBoot, &Event::ForceStop);
        assert_eq!(state, VmState::Virgin);
        assert_eq!(effects, vec![Effect::DestroyDomain, Effect::CleanupArtifacts]);

        // ForceStop from Running
        let (state, effects) = flow.transition(&VmState::Running, &Event::ForceStop);
        assert_eq!(state, VmState::Virgin);
        assert_eq!(effects, vec![Effect::DestroyDomain, Effect::CleanupArtifacts]);
    }

    // ── Unknown events ──

    #[test]
    fn unknown_event_returns_same_state() {
        let flow = FirstBootFlow::new(vec![], vec![]);
        let (state, effects) = flow.transition(&VmState::Virgin, &Event::ServicesStarted);
        assert_eq!(state, VmState::Virgin);
        assert!(effects.is_empty());
    }

    #[test]
    fn detach_returns_same_state() {
        let flow = FirstBootFlow::new(vec![], vec![]);
        let (state, effects) = flow.transition(&VmState::Running, &Event::Detach);
        assert_eq!(state, VmState::Running);
        assert!(effects.is_empty());
    }

    // ── Entry states ──

    #[test]
    fn valid_entry_states() {
        let flow = FirstBootFlow::new(vec![], vec![]);
        let states = flow.valid_entry_states();
        assert!(states.contains(&VmState::Virgin));
        assert!(states.contains(&VmState::ImageCached));
        assert!(states.contains(&VmState::Prepared));
        assert!(states.contains(&VmState::PartialBoot));
        assert!(!states.contains(&VmState::Running));
        assert!(!states.contains(&VmState::Provisioned));
    }
}
