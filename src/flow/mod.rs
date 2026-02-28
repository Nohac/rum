//! Event-driven flow system.
//!
//! Each command (up, down, destroy, provision) is a separate `Flow` —
//! a set of transition rules mapping `(VmState, Event) -> (VmState, Vec<Effect>)`.
//! The server event loop drives flows by dispatching effects to workers and
//! feeding their completion events back into the flow.

pub mod event_loop;
pub mod first_boot;
pub mod reboot;
pub mod reattach;
pub mod shutdown;
pub mod destroy;
pub mod reprovision;

use std::path::PathBuf;

use crate::error::RumError;
use crate::vm_state::VmState;

// ── Events ──────────────────────────────────────────────────────────

/// Events emitted by workers or received from clients.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// Flow just started — triggers initial effects.
    FlowStarted,

    // Worker completion events
    ImageReady(PathBuf),
    ImageFailed(String),
    VmPrepared,
    PrepareFailed(String),
    DomainStarted,
    BootFailed(String),
    AgentConnected,
    AgentTimeout(String),
    ScriptStarted { name: String },
    ScriptCompleted { name: String },
    ScriptFailed { name: String, error: String },
    AllScriptsComplete,
    ServicesStarted,

    // Client command events (received via roam RPC)
    InitShutdown,
    ForceStop,

    // Shutdown completion
    ShutdownComplete,
    DomainStopped,
    CleanupComplete,
    DestroyComplete,

    // Detach
    Detach,
}

// ── Effects ─────────────────────────────────────────────────────────

/// Effects dispatched to workers by the event loop.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// Download/verify base image.
    EnsureImage,
    /// Create overlay, seed ISO, domain XML, define domain, ensure networks.
    PrepareVm,
    /// Start the libvirt domain.
    BootVm,
    /// Wait for the guest agent to become reachable.
    ConnectAgent,
    /// Run a provisioning script via the guest agent.
    RunScript { name: String },
    /// Start log subscription + port forwards.
    StartServices,
    /// ACPI shutdown with timeout + force fallback.
    ShutdownDomain,
    /// Force-destroy the libvirt domain.
    DestroyDomain,
    /// Remove work directory and artifacts.
    CleanupArtifacts,
}

// ── Flow trait ──────────────────────────────────────────────────────

/// A Flow defines a set of transitions driven by a specific command.
/// Each flow declares its valid entry states and provides pure
/// transition functions.
pub trait Flow: Send {
    /// Which VmStates this flow can start from.
    fn valid_entry_states(&self) -> &[VmState];

    /// Pure transition: given current state and an event, return the new
    /// state and any effects to dispatch.
    ///
    /// Unknown events should return the current state unchanged with no
    /// effects (log a warning).
    fn transition(&self, state: &VmState, event: &Event) -> (VmState, Vec<Effect>);
}

/// Result of running a flow to completion.
pub enum FlowResult {
    /// Flow completed, VM is in this state.
    Done(VmState),
    /// Flow completed, chain into the next flow starting from this state.
    Chain(Box<dyn Flow>, VmState),
}

// ── Flow selection ──────────────────────────────────────────────────

/// Select the appropriate flow based on the command and current VM state.
///
/// `system_scripts` run only on first boot (and reprovision).
/// `boot_scripts` run on every boot (first boot AND subsequent reboots).
pub fn select_flow(
    command: &FlowCommand,
    state: &VmState,
    system_scripts: Vec<String>,
    boot_scripts: Vec<String>,
) -> Result<Box<dyn Flow>, RumError> {
    match command {
        FlowCommand::Up => match state {
            VmState::Virgin | VmState::ImageCached
            | VmState::Prepared | VmState::PartialBoot => {
                Ok(Box::new(first_boot::FirstBootFlow::new(system_scripts, boot_scripts)))
            }
            VmState::Provisioned => Ok(Box::new(reboot::RebootFlow::new(boot_scripts))),
            VmState::Running => Ok(Box::new(reattach::ReattachFlow)),
            VmState::RunningStale => Err(RumError::RequiresRestart {
                name: "VM".into(),
            }),
        },
        FlowCommand::Down => {
            flow_requires_state(state, &[VmState::Running, VmState::RunningStale])?;
            Ok(Box::new(shutdown::ShutdownFlow))
        }
        FlowCommand::Destroy => {
            Ok(Box::new(destroy::DestroyFlow))
        }
        FlowCommand::Provision => {
            flow_requires_state(state, &[VmState::Running])?;
            Ok(Box::new(reprovision::ReprovisionFlow::new(system_scripts, boot_scripts)))
        }
    }
}

/// Commands that drive flows (subset of CLI commands).
#[derive(Debug, Clone)]
pub enum FlowCommand {
    Up,
    Down,
    Destroy,
    Provision,
}

fn flow_requires_state(state: &VmState, valid: &[VmState]) -> Result<(), RumError> {
    if valid.contains(state) {
        Ok(())
    } else {
        Err(RumError::Validation {
            message: format!(
                "command requires VM to be in one of {:?}, but current state is {:?}",
                valid, state,
            ),
        })
    }
}
