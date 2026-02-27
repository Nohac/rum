//! Plain text observer — no ANSI, suitable for piped output.

use std::future::Future;
use std::pin::Pin;

use crate::flow::Event;
use crate::vm_state::VmState;

use super::{EffectData, Observer, Transition};

/// Plain text observer with step counters.
///
/// Produces simple `[N/M] description` lines to stderr, no colors or
/// escape codes. Suitable for CI logs and piped output.
#[derive(Clone)]
pub struct PlainObserver {
    step: usize,
    total_steps: usize,
}

impl PlainObserver {
    pub fn new(total_steps: usize) -> Self {
        Self {
            step: 0,
            total_steps,
        }
    }

    fn next_step(&mut self) -> String {
        self.step += 1;
        format!("[{}/{}]", self.step, self.total_steps)
    }
}

impl Observer for PlainObserver {
    fn on_transition(
        &mut self,
        t: &Transition,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        let message = match (&t.old_state, &t.new_state, &t.event) {
            (_, _, Event::ImageReady(_)) => {
                Some(format!("{} Base image ready", self.next_step()))
            }
            (_, VmState::Prepared, _) => {
                Some(format!("{} VM prepared", self.next_step()))
            }
            (_, VmState::PartialBoot, Event::DomainStarted) => {
                Some(format!("{} VM booted", self.next_step()))
            }
            (_, VmState::Running, _) => {
                Some(format!("{} Ready", self.next_step()))
            }
            (_, VmState::Provisioned, Event::ShutdownComplete) => {
                Some(format!("{} Shut down", self.next_step()))
            }
            (_, _, Event::ScriptCompleted { name }) => {
                Some(format!("{} {}", self.next_step(), name))
            }
            (_, _, Event::AllScriptsComplete) => {
                Some(format!("{} Provisioning complete", self.next_step()))
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
            // Print log lines with "  | " prefix. Ignore progress data.
            if stream_name.starts_with("script:") || stream_name == "image_download" {
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
