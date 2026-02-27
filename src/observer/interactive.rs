//! Interactive TTY observer — spinners, progress bars, colored output.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use crate::flow::Event;
use crate::vm_state::VmState;

use super::{EffectData, Observer, Transition};

/// Shared mutable state for the interactive observer.
struct ObserverState {
    step: usize,
    total_steps: usize,
}

/// Interactive TTY observer with step-based progress output.
///
/// Uses `Arc<Mutex<_>>` for shared state so `clone_for_stream()` is cheap.
#[derive(Clone)]
pub struct InteractiveObserver {
    state: Arc<Mutex<ObserverState>>,
}

impl InteractiveObserver {
    /// Create a new interactive observer.
    ///
    /// `total_steps` is the estimated number of steps for the current flow
    /// (e.g. 5 for a full first-boot: image, prepare, boot, provision, ready).
    pub fn new(total_steps: usize) -> Self {
        Self {
            state: Arc::new(Mutex::new(ObserverState {
                step: 0,
                total_steps,
            })),
        }
    }

    /// Increment the step counter and return `[step/total]` prefix.
    fn next_step(&self) -> String {
        let mut state = self.state.lock().unwrap();
        state.step += 1;
        format!("[{}/{}]", state.step, state.total_steps)
    }
}

impl Observer for InteractiveObserver {
    fn on_transition(
        &mut self,
        t: &Transition,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        let message = match (&t.old_state, &t.new_state, &t.event) {
            (_, VmState::Prepared, _) => {
                Some(format!("{} \u{2713} VM prepared", self.next_step()))
            }
            (_, VmState::PartialBoot, Event::DomainStarted) => {
                Some(format!("{} \u{2713} VM booted", self.next_step()))
            }
            (_, VmState::Running, _) => {
                Some(format!("{} \u{2713} Ready", self.next_step()))
            }
            (_, VmState::Provisioned, Event::ShutdownComplete) => {
                Some(format!("{} \u{2713} Shut down", self.next_step()))
            }
            (_, _, Event::ScriptCompleted { name }) => {
                Some(format!("{} \u{2713} {}", self.next_step(), name))
            }
            (_, _, Event::ImageReady(_)) => {
                Some(format!("{} \u{2713} Base image ready", self.next_step()))
            }
            _ => None,
        };

        Box::pin(async move {
            if let Some(msg) = message {
                eprintln!("{msg}");
            }
        })
    }

    fn on_effect_stream(
        &mut self,
        name: &str,
        mut rx: roam::Rx<EffectData>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        let stream_name = name.to_string();
        Box::pin(async move {
            if stream_name == "image_download" {
                while let Ok(Some(data)) = rx.recv().await {
                    if let EffectData::Progress { current, total } = data
                        && total > 0
                    {
                        let pct = (current as f64 / total as f64 * 100.0) as u64;
                        eprint!("\r  downloading... {pct}%");
                    }
                }
                eprintln!(); // newline after progress
            } else if stream_name.starts_with("script:") {
                while let Ok(Some(data)) = rx.recv().await {
                    if let EffectData::LogLine(line) = data {
                        eprintln!("  | {line}");
                    }
                }
            }
            // Unknown streams: silently drain
        })
    }

    fn clone_for_stream(&self) -> Box<dyn Observer> {
        Box::new(self.clone())
    }
}
