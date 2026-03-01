//! Server-side event loop that drives flows.
//!
//! The event loop receives events from workers (async blocks in FuturesUnordered)
//! and client commands (via mpsc channel), feeds them into the flow's transition
//! function, and dispatches the resulting effects as new workers.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use futures_util::stream::FuturesUnordered;
use futures_util::StreamExt;
use tokio::sync::{broadcast, mpsc};

use crate::agent::AgentClient;
use crate::config::SystemConfig;
use crate::error::RumError;
use crate::observer::{EffectData, Transition};
use crate::vm_state::VmState;

use super::{Effect, Event, Flow};

/// A handle to a newly opened effect data stream.
///
/// Workers create these and send them via `FlowContext::effect_stream_tx`.
/// The caller (main.rs) receives them and drives them through the observer.
pub struct EffectStreamHandle {
    pub name: String,
    pub rx: roam::Rx<EffectData>,
}

// ── Accumulated state ─────────────────────────────────────────────

/// Shared state bag for passing results between workers.
///
/// Workers write their outputs here; subsequent workers read them.
/// Protected by a mutex since workers run concurrently in FuturesUnordered.
#[derive(Default)]
pub struct AccumulatedState {
    pub base_image: Option<PathBuf>,
    pub vsock_cid: Option<u32>,
    pub agent: Option<AgentClient>,
}

// ── FlowContext ────────────────────────────────────────────────────

/// Shared context for the event loop and its workers.
pub struct FlowContext {
    /// Receives commands from connected clients (InitShutdown, ForceStop, etc.)
    pub command_rx: mpsc::Receiver<Event>,

    /// Broadcasts transitions to all connected observer clients.
    pub transition_tx: broadcast::Sender<Transition>,

    /// Workers send new effect data streams here (e.g., script log lines).
    /// The caller (main.rs) receives them and drives them through the observer.
    pub effect_stream_tx: mpsc::UnboundedSender<EffectStreamHandle>,

    /// System configuration.
    pub sys_config: SystemConfig,

    /// Accumulated state shared between workers.
    pub state: Arc<Mutex<AccumulatedState>>,
}

impl FlowContext {
    pub fn new(
        sys_config: SystemConfig,
        command_rx: mpsc::Receiver<Event>,
        transition_tx: broadcast::Sender<Transition>,
        effect_stream_tx: mpsc::UnboundedSender<EffectStreamHandle>,
    ) -> Self {
        Self {
            command_rx,
            transition_tx,
            effect_stream_tx,
            sys_config,
            state: Arc::new(Mutex::new(AccumulatedState::default())),
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
        workers.push(make_worker(effect, &ctx.sys_config, &ctx.state, &ctx.effect_stream_tx));
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
            // All workers done and command channel closed.
            else => break,
        };

        let (new_state, effects) = flow.transition(&state, &event);
        publish_transition(&ctx.transition_tx, &state, &new_state, &event);
        state = new_state;

        let no_new_effects = effects.is_empty();
        for effect in effects {
            workers.push(make_worker(effect, &ctx.sys_config, &ctx.state, &ctx.effect_stream_tx));
        }

        if state.is_terminal() {
            break;
        }

        // Quiescent: no pending workers, no effects emitted, and NOT in
        // an interactive-wait state (Running). The flow has settled in a
        // stable non-interactive state like Provisioned — exit.
        if workers.is_empty() && no_new_effects && !state.is_interactive_wait() {
            break;
        }
    }

    Ok(state)
}

// ── Worker dispatch ────────────────────────────────────────────────

/// Map an Effect to an async worker that produces an Event on completion.
///
/// Workers are self-contained async blocks that call into `crate::workers`.
/// They read/write `AccumulatedState` to pass results between steps.
fn make_worker(
    effect: Effect,
    sys_config: &SystemConfig,
    acc: &Arc<Mutex<AccumulatedState>>,
    effect_stream_tx: &mpsc::UnboundedSender<EffectStreamHandle>,
) -> Pin<Box<dyn Future<Output = Event> + Send>> {
    match effect {
        Effect::EnsureImage => {
            let base = sys_config.config.image.base.clone();
            let acc = Arc::clone(acc);
            Box::pin(async move {
                let cache = crate::paths::cache_dir();
                match crate::workers::ensure_image(&base, &cache).await {
                    Ok(path) => {
                        acc.lock().unwrap().base_image = Some(path.clone());
                        Event::ImageReady(path)
                    }
                    Err(e) => Event::ImageFailed(e.to_string()),
                }
            })
        }
        Effect::PrepareVm => {
            let sc = sys_config.clone();
            let acc = Arc::clone(acc);
            Box::pin(async move {
                let base_image = acc.lock().unwrap().base_image.clone();
                let Some(base_image) = base_image else {
                    return Event::PrepareFailed("base image path not available".into());
                };
                match crate::workers::prepare_vm(&sc, &base_image).await {
                    Ok(()) => Event::VmPrepared,
                    Err(e) => Event::PrepareFailed(e.to_string()),
                }
            })
        }
        Effect::BootVm => {
            let sc = sys_config.clone();
            let acc = Arc::clone(acc);
            Box::pin(async move {
                match crate::workers::boot_vm(&sc).await {
                    Ok(cid) => {
                        acc.lock().unwrap().vsock_cid = Some(cid);
                        Event::DomainStarted
                    }
                    Err(e) => Event::BootFailed(e.to_string()),
                }
            })
        }
        Effect::ConnectAgent => {
            let acc = Arc::clone(acc);
            Box::pin(async move {
                let cid = acc.lock().unwrap().vsock_cid;
                let Some(cid) = cid else {
                    return Event::AgentTimeout("vsock CID not available".into());
                };
                match crate::workers::connect_agent(cid).await {
                    Ok(client) => {
                        acc.lock().unwrap().agent = Some(client);
                        Event::AgentConnected
                    }
                    Err(e) => Event::AgentTimeout(e.to_string()),
                }
            })
        }
        Effect::RunScript { name } => {
            let acc = Arc::clone(acc);
            let sc = sys_config.clone();
            let stream_tx = effect_stream_tx.clone();
            Box::pin(async move {
                let agent = acc.lock().unwrap().agent.clone();
                let Some(agent) = agent else {
                    return Event::ScriptFailed {
                        name,
                        error: "agent not connected".into(),
                    };
                };
                let script = build_provision_script(&sc, &name);
                let Some(script) = script else {
                    return Event::ScriptFailed {
                        name,
                        error: "unknown script".into(),
                    };
                };

                // Create effect stream for log lines.
                let (effect_tx, effect_rx) = roam::channel::<EffectData>();
                let _ = stream_tx.send(EffectStreamHandle {
                    name: format!("script:{name}"),
                    rx: effect_rx,
                });

                // Run the script via agent, forwarding log lines.
                let logs_dir = crate::paths::logs_dir(&sc.id, sc.name.as_deref());
                let (prov_tx, prov_rx) = roam::channel::<rum_agent::ProvisionEvent>();
                let agent_clone = agent.clone();
                let scripts = vec![script];
                let task = tokio::spawn(async move {
                    agent_clone.provision(scripts, prov_tx).await
                });

                let prov_rx = Arc::new(tokio::sync::Mutex::new(prov_rx));
                let mut logger = crate::logging::ScriptLogger::new(&logs_dir, &name).ok();
                let mut failed = false;

                {
                    let mut rx = prov_rx.lock().await;
                    while let Ok(Some(event)) = rx.recv().await {
                        match event {
                            rum_agent::ProvisionEvent::Done(code) => {
                                if let Some(lg) = logger.take() {
                                    lg.finish(code == 0);
                                }
                                if code != 0 {
                                    failed = true;
                                }
                                break;
                            }
                            rum_agent::ProvisionEvent::Stdout(ref line)
                            | rum_agent::ProvisionEvent::Stderr(ref line) => {
                                if let Some(ref mut lg) = logger {
                                    lg.write_line(line);
                                }
                                let _ = effect_tx.send(&EffectData::LogLine(line.clone())).await;
                            }
                        }
                    }
                }

                // Drop effect_tx to close the stream (signals observer).
                drop(effect_tx);

                crate::logging::rotate_logs(&logs_dir, &name, 10);

                let result = task.await
                    .map_err(|e| e.to_string())
                    .and_then(|r| r.map_err(|e| e.to_string()));

                if failed {
                    Event::ScriptFailed {
                        name,
                        error: "script exited with non-zero code".into(),
                    }
                } else if let Err(e) = result {
                    Event::ScriptFailed {
                        name,
                        error: e,
                    }
                } else {
                    Event::ScriptCompleted { name }
                }
            })
        }
        Effect::StartServices => {
            let acc = Arc::clone(acc);
            let sc = sys_config.clone();
            Box::pin(async move {
                let cid = acc.lock().unwrap().vsock_cid;
                let Some(cid) = cid else {
                    // No vsock CID — services can't start but this isn't fatal
                    tracing::warn!("no vsock CID available, skipping services");
                    return Event::ServicesStarted;
                };
                match crate::workers::start_services(cid, &sc).await {
                    Ok(_handles) => {
                        // Handles are kept alive by tokio spawn — they'll run
                        // until the process exits or tasks are aborted.
                        Event::ServicesStarted
                    }
                    Err(e) => {
                        tracing::warn!("failed to start services: {e}");
                        Event::ServicesStarted // non-fatal
                    }
                }
            })
        }
        Effect::ShutdownDomain => {
            let sc = sys_config.clone();
            Box::pin(async move {
                match crate::workers::shutdown_vm(&sc).await {
                    Ok(()) => Event::ShutdownComplete,
                    Err(e) => {
                        tracing::warn!("shutdown failed, force-stopping: {e}");
                        Event::ShutdownComplete
                    }
                }
            })
        }
        Effect::DestroyDomain => {
            let sc = sys_config.clone();
            Box::pin(async move {
                match crate::workers::destroy_vm(&sc).await {
                    Ok(()) => Event::DomainStopped,
                    Err(e) => {
                        tracing::warn!("destroy failed: {e}");
                        Event::DomainStopped
                    }
                }
            })
        }
        Effect::CleanupArtifacts => {
            let sc = sys_config.clone();
            Box::pin(async move {
                let work = crate::paths::work_dir(&sc.id, sc.name.as_deref());
                if work.exists() {
                    let _ = tokio::fs::remove_dir_all(&work).await;
                }
                Event::CleanupComplete
            })
        }
    }
}

/// Build a single ProvisionScript by name from the system config.
///
/// Script names match those used in flows: "rum-drives", "rum-system", "rum-boot".
fn build_provision_script(
    sys_config: &SystemConfig,
    name: &str,
) -> Option<rum_agent::ProvisionScript> {
    let config = &sys_config.config;
    match name {
        "rum-drives" => {
            let drives = sys_config.resolve_drives().ok()?;
            let resolved_fs = sys_config.resolve_fs(&drives).ok()?;
            if resolved_fs.is_empty() {
                return None;
            }
            Some(rum_agent::ProvisionScript {
                name: "rum-drives".into(),
                title: "Setting up drives and filesystems".into(),
                content: crate::cloudinit::build_drive_script(&resolved_fs),
                order: 0,
                run_on: rum_agent::RunOn::System,
            })
        }
        "rum-system" => {
            let system = config.provision.system.as_ref()?;
            Some(rum_agent::ProvisionScript {
                name: "rum-system".into(),
                title: "Running system provisioning".into(),
                content: system.script.clone(),
                order: 1,
                run_on: rum_agent::RunOn::System,
            })
        }
        "rum-boot" => {
            let boot = config.provision.boot.as_ref()?;
            Some(rum_agent::ProvisionScript {
                name: "rum-boot".into(),
                title: "Running boot provisioning".into(),
                content: boot.script.clone(),
                order: 2,
                run_on: rum_agent::RunOn::Boot,
            })
        }
        _ => None,
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

        fn expected_steps(&self, _entry_state: &VmState) -> usize {
            0
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

        fn expected_steps(&self, _entry_state: &VmState) -> usize {
            1
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

        let (effect_stream_tx, _effect_stream_rx) = mpsc::unbounded_channel();
        let mut ctx = FlowContext::new(sys_config, cmd_rx, transition_tx, effect_stream_tx);
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

        let (effect_stream_tx, _effect_stream_rx) = mpsc::unbounded_channel();
        let mut ctx = FlowContext::new(sys_config, cmd_rx, transition_tx, effect_stream_tx);
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

            fn expected_steps(&self, _entry_state: &VmState) -> usize {
                1
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

        let (effect_stream_tx, _effect_stream_rx) = mpsc::unbounded_channel();
        let mut ctx = FlowContext::new(sys_config, cmd_rx, transition_tx, effect_stream_tx);

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
