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
#[derive(Debug, Clone)]
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
