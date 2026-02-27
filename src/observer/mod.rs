//! Client-side observer model.
//!
//! Observers consume state transitions and effect data streams from the
//! daemon (over roam) and render output. Different implementations handle
//! interactive TTY, plain text, and JSON output modes.
//!
//! Observers are driven in parallel via `FuturesUnordered` — transitions
//! are processed sequentially (ordering matters), but per-effect stream
//! observers run concurrently.

pub mod interactive;
pub mod plain;
pub mod json;

use std::future::Future;
use std::pin::Pin;

use crate::vm_state::VmState;
use crate::flow::Event;

// ── Types ───────────────────────────────────────────────────────────

/// A state transition published by the server.
#[derive(Debug, Clone)]
pub struct Transition {
    pub old_state: VmState,
    pub new_state: VmState,
    pub event: Event,
}

impl Transition {
    pub fn new(old: VmState, new: VmState, event: Event) -> Self {
        Self {
            old_state: old,
            new_state: new,
            event,
        }
    }
}

/// Data from an effect stream (log line, progress update, etc.)
///
/// Derives `Facet` so it can be sent/received over `roam::Rx<EffectData>`.
#[derive(Debug, Clone, facet::Facet)]
#[repr(u8)]
pub enum EffectData {
    LogLine(String),
    Progress { current: u64, total: u64 },
    Info(String),
}

/// Notification that a new effect stream has been opened.
#[derive(Debug, Clone)]
pub struct EffectStreamNotification {
    pub stream_id: String,
    pub name: String,
}

// ── Observer trait ──────────────────────────────────────────────────

/// Observer handles rendering for streams from the daemon.
///
/// The client creates one observer instance per active stream and drives
/// them all in parallel via `FuturesUnordered`.
///
/// Implementations should be `Clone`-able (for per-stream tasks) while
/// sharing rendering state through `Arc`.
///
/// Uses boxed futures for dyn-compatibility — each method returns a
/// `Pin<Box<dyn Future>>` so we can use `Box<dyn Observer>`.
pub trait Observer: Send + 'static {
    /// Handle a state transition. Called once per transition on the
    /// transition observer task (sequentially).
    fn on_transition(
        &mut self,
        t: &Transition,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>>;

    /// Handle a new effect stream. Called once when the stream opens;
    /// the implementation consumes the stream to completion.
    fn on_effect_stream(
        &mut self,
        name: &str,
        rx: roam::Rx<EffectData>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>>;

    /// Create a clone suitable for driving a parallel stream task.
    fn clone_for_stream(&self) -> Box<dyn Observer>;
}

// ── Client loop functions ─────────────────────────────────────────

/// Run the observe loop — subscribe to transitions and render via observer.
///
/// Loops on the transition receiver until the VM reaches a terminal state
/// or the channel closes. Effect streams will be integrated in phase 6;
/// for now this only handles transitions.
pub async fn run_observe_loop(
    transition_rx: &mut tokio::sync::mpsc::Receiver<Transition>,
    observer: &mut dyn Observer,
) -> Result<(), crate::error::RumError> {
    while let Some(t) = transition_rx.recv().await {
        observer.on_transition(&t).await;
        if t.new_state.is_terminal() {
            break;
        }
    }
    Ok(())
}

/// Attached client: owns shutdown lifecycle.
///
/// First Ctrl+C sends InitShutdown, second Ctrl+C or 30 s timeout sends
/// ForceStop. The shutdown handler races against the observe loop.
pub async fn run_attached_client(
    transition_rx: &mut tokio::sync::mpsc::Receiver<Transition>,
    observer: &mut dyn Observer,
) -> Result<(), crate::error::RumError> {
    let shutdown_handler = async {
        // First Ctrl+C — would send InitShutdown via roam RPC in full impl.
        tokio::signal::ctrl_c().await.ok();
        // Race second Ctrl+C vs 30 s timeout — would send ForceStop.
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = tokio::time::sleep(std::time::Duration::from_secs(30)) => {}
        }
    };

    tokio::select! {
        result = run_observe_loop(transition_rx, observer) => result,
        _ = shutdown_handler => Ok(()),
    }
}

/// Observer-only client: Ctrl+C simply disconnects without sending
/// any shutdown commands to the daemon.
pub async fn run_observer_client(
    transition_rx: &mut tokio::sync::mpsc::Receiver<Transition>,
    observer: &mut dyn Observer,
) -> Result<(), crate::error::RumError> {
    tokio::select! {
        result = run_observe_loop(transition_rx, observer) => result,
        _ = tokio::signal::ctrl_c() => Ok(()),
    }
}
