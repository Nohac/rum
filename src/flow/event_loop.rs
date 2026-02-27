//! Server-side event loop that drives flows.
//!
//! The event loop receives events from workers (async blocks in FuturesUnordered)
//! and client commands (via mpsc channel), feeds them into the flow's transition
//! function, and dispatches the resulting effects as new workers.

use std::future::Future;
use std::pin::Pin;

use futures_util::stream::FuturesUnordered;
use futures_util::StreamExt;
use tokio::sync::{broadcast, mpsc};

use crate::config::SystemConfig;
use crate::error::RumError;
use crate::observer::Transition;
use crate::vm_state::VmState;

use super::{Effect, Event, Flow};

// ── FlowContext ────────────────────────────────────────────────────

/// Shared context for the event loop and its workers.
pub struct FlowContext {
    /// Receives commands from connected clients (InitShutdown, ForceStop, etc.)
    pub command_rx: mpsc::Receiver<Event>,

    /// Broadcasts transitions to all connected observer clients.
    pub transition_tx: broadcast::Sender<Transition>,

    /// System configuration.
    pub sys_config: SystemConfig,
}

impl FlowContext {
    pub fn new(
        sys_config: SystemConfig,
        command_rx: mpsc::Receiver<Event>,
        transition_tx: broadcast::Sender<Transition>,
    ) -> Self {
        Self {
            command_rx,
            transition_tx,
            sys_config,
        }
    }
}

// ── Event loop ─────────────────────────────────────────────────────

/// Run the server event loop for a given flow.
///
/// Drives the flow by:
/// 1. Seeding initial effects from FlowStarted
/// 2. Dispatching effects as async worker blocks in FuturesUnordered
/// 3. Feeding worker completion events back into the flow
/// 4. Publishing transitions to observers via broadcast
/// 5. Exiting when terminal state is reached
pub async fn run_event_loop(
    flow: Box<dyn Flow>,
    initial_state: VmState,
    ctx: &mut FlowContext,
) -> Result<VmState, RumError> {
    let mut state = initial_state;
    let mut workers: FuturesUnordered<Pin<Box<dyn Future<Output = Event> + Send>>> =
        FuturesUnordered::new();

    // Seed initial effects.
    let (new_state, effects) = flow.transition(&state, &Event::FlowStarted);
    publish_transition(&ctx.transition_tx, &state, &new_state, &Event::FlowStarted);
    state = new_state;

    for effect in effects {
        workers.push(make_worker(effect, &ctx.sys_config));
    }

    if state.is_terminal() {
        return Ok(state);
    }

    loop {
        let event = tokio::select! {
            // Commands from clients (e.g., InitShutdown).
            Some(cmd) = ctx.command_rx.recv() => cmd,
            // Worker completions.
            Some(evt) = workers.next() => evt,
            // All workers done and no commands — should only happen in
            // terminal state or if the flow is stuck.
            else => break,
        };

        let (new_state, effects) = flow.transition(&state, &event);
        publish_transition(&ctx.transition_tx, &state, &new_state, &event);
        state = new_state;

        for effect in effects {
            workers.push(make_worker(effect, &ctx.sys_config));
        }

        if state.is_terminal() {
            break;
        }
    }

    Ok(state)
}

// ── Worker dispatch ────────────────────────────────────────────────

/// Map an Effect to an async worker that produces an Event on completion.
///
/// Workers are self-contained async blocks. They call into `crate::workers`
/// which are still `todo!()` stubs — the match arms here show the intended
/// wiring while returning placeholder events for effects whose workers
/// are not yet implemented.
fn make_worker(
    effect: Effect,
    sys_config: &SystemConfig,
) -> Pin<Box<dyn Future<Output = Event> + Send>> {
    match effect {
        Effect::EnsureImage => {
            let base = sys_config.config.image.base.clone();
            Box::pin(async move {
                let cache = crate::paths::cache_dir();
                match crate::workers::ensure_image(&base, &cache).await {
                    Ok(path) => Event::ImageReady(path),
                    Err(e) => Event::ImageFailed(e.to_string()),
                }
            })
        }
        Effect::PrepareVm => {
            // TODO: needs base_image path from previous step — will require
            // passing accumulated state through the flow in a future change.
            Box::pin(async move {
                Event::VmPrepared // placeholder
            })
        }
        Effect::BootVm => {
            Box::pin(async move {
                Event::DomainStarted // placeholder
            })
        }
        Effect::ConnectAgent => {
            Box::pin(async move {
                Event::AgentConnected // placeholder
            })
        }
        Effect::RunScript { name } => {
            Box::pin(async move { Event::ScriptCompleted { name } })
        }
        Effect::StartServices => {
            Box::pin(async move { Event::ServicesStarted })
        }
        Effect::ShutdownDomain => {
            Box::pin(async move { Event::ShutdownComplete })
        }
        Effect::DestroyDomain => {
            Box::pin(async move { Event::DomainStopped })
        }
        Effect::CleanupArtifacts => {
            Box::pin(async move { Event::CleanupComplete })
        }
    }
}

// ── Helpers ────────────────────────────────────────────────────────

/// Publish a state transition to all connected observers.
fn publish_transition(
    tx: &broadcast::Sender<Transition>,
    old: &VmState,
    new: &VmState,
    event: &Event,
) {
    let t = Transition::new(*old, *new, event.clone());
    // Ignore send error — no subscribers is fine.
    let _ = tx.send(t);
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal SystemConfig for testing (no real files needed).
    fn test_sys_config() -> crate::config::SystemConfig {
        let toml = "\
[image]\n\
base = \"https://example.com/test.qcow2\"\n\
\n\
[resources]\n\
cpus = 1\n\
memory_mb = 512\n";
        let config: crate::config::Config = facet_toml::from_str(toml).unwrap();
        crate::config::SystemConfig {
            id: "test1234".into(),
            name: None,
            config_path: std::path::PathBuf::from("/tmp/test-rum.toml"),
            config,
        }
    }

    /// Minimal flow: FlowStarted immediately transitions to terminal (Virgin).
    struct TerminalFlow;

    impl Flow for TerminalFlow {
        fn valid_entry_states(&self) -> &[VmState] {
            &[VmState::Running]
        }

        fn transition(&self, _state: &VmState, event: &Event) -> (VmState, Vec<Effect>) {
            match event {
                Event::FlowStarted => (VmState::Virgin, vec![]),
                _ => (VmState::Virgin, vec![]),
            }
        }
    }

    /// Flow that emits one effect, then goes terminal on the result.
    struct OneEffectFlow;

    impl Flow for OneEffectFlow {
        fn valid_entry_states(&self) -> &[VmState] {
            &[VmState::Running]
        }

        fn transition(&self, _state: &VmState, event: &Event) -> (VmState, Vec<Effect>) {
            match event {
                Event::FlowStarted => (VmState::Running, vec![Effect::DestroyDomain]),
                Event::DomainStopped => (VmState::Virgin, vec![]),
                _ => (VmState::Running, vec![]),
            }
        }
    }

    #[tokio::test]
    async fn immediate_terminal_state() {
        let (_cmd_tx, cmd_rx) = mpsc::channel(16);
        let (transition_tx, mut transition_rx) = broadcast::channel(16);

        // We need a SystemConfig — construct a minimal one for testing.
        // Since SystemConfig requires parsed config, we use a minimal toml.
        let sys_config = test_sys_config();

        let mut ctx = FlowContext::new(sys_config, cmd_rx, transition_tx);
        let result = run_event_loop(
            Box::new(TerminalFlow),
            VmState::Running,
            &mut ctx,
        )
        .await
        .unwrap();

        assert_eq!(result, VmState::Virgin);

        // Should have published exactly one transition (FlowStarted → Virgin).
        let t = transition_rx.try_recv().unwrap();
        assert_eq!(t.old_state, VmState::Running);
        assert_eq!(t.new_state, VmState::Virgin);
    }

    #[tokio::test]
    async fn effect_drives_next_transition() {
        let (_cmd_tx, cmd_rx) = mpsc::channel(16);
        let (transition_tx, mut transition_rx) = broadcast::channel(16);

        let sys_config = test_sys_config();

        let mut ctx = FlowContext::new(sys_config, cmd_rx, transition_tx);
        let result = run_event_loop(
            Box::new(OneEffectFlow),
            VmState::Running,
            &mut ctx,
        )
        .await
        .unwrap();

        assert_eq!(result, VmState::Virgin);

        // First transition: FlowStarted → Running (emits DestroyDomain).
        let t1 = transition_rx.try_recv().unwrap();
        assert_eq!(t1.old_state, VmState::Running);
        assert_eq!(t1.new_state, VmState::Running);

        // Second transition: DomainStopped → Virgin (terminal).
        let t2 = transition_rx.try_recv().unwrap();
        assert_eq!(t2.old_state, VmState::Running);
        assert_eq!(t2.new_state, VmState::Virgin);
    }

    #[tokio::test]
    async fn client_command_triggers_transition() {
        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let (transition_tx, mut transition_rx) = broadcast::channel(16);

        /// Flow that waits for a client InitShutdown to go terminal.
        struct WaitForShutdownFlow;

        impl Flow for WaitForShutdownFlow {
            fn valid_entry_states(&self) -> &[VmState] {
                &[VmState::Running]
            }

            fn transition(&self, _state: &VmState, event: &Event) -> (VmState, Vec<Effect>) {
                match event {
                    Event::FlowStarted => (VmState::Running, vec![]),
                    Event::InitShutdown => (VmState::Virgin, vec![]),
                    _ => (VmState::Running, vec![]),
                }
            }
        }

        let sys_config = test_sys_config();

        let mut ctx = FlowContext::new(sys_config, cmd_rx, transition_tx);

        // Send the command before starting the loop — it will be picked up
        // on the first select iteration.
        cmd_tx.send(Event::InitShutdown).await.unwrap();

        let result = run_event_loop(
            Box::new(WaitForShutdownFlow),
            VmState::Running,
            &mut ctx,
        )
        .await
        .unwrap();

        assert_eq!(result, VmState::Virgin);

        // FlowStarted transition.
        let t1 = transition_rx.try_recv().unwrap();
        assert_eq!(t1.new_state, VmState::Running);

        // InitShutdown transition.
        let t2 = transition_rx.try_recv().unwrap();
        assert_eq!(t2.new_state, VmState::Virgin);
    }

    #[test]
    fn publish_transition_works_with_no_subscribers() {
        let (tx, _) = broadcast::channel::<Transition>(16);
        // Should not panic even with no receivers.
        publish_transition(&tx, &VmState::Virgin, &VmState::Running, &Event::FlowStarted);
    }

    #[test]
    fn publish_transition_delivers_to_subscriber() {
        let (tx, mut rx) = broadcast::channel::<Transition>(16);
        publish_transition(&tx, &VmState::Virgin, &VmState::Running, &Event::FlowStarted);
        let t = rx.try_recv().unwrap();
        assert_eq!(t.old_state, VmState::Virgin);
        assert_eq!(t.new_state, VmState::Running);
    }
}
