//! JSON-lines observer — structured output for machine consumption.

use std::future::Future;
use std::pin::Pin;

use super::{EffectData, Observer, Transition};

/// JSON-lines observer.
///
/// Emits one JSON object per line to stdout for each transition and
/// effect data item. Uses manual `format!` strings since rum does not
/// use serde.
#[derive(Clone, Default)]
pub struct JsonObserver;

impl JsonObserver {
    pub fn new() -> Self {
        Self
    }
}

/// Format a transition as a JSON object string.
fn json_transition(t: &Transition) -> String {
    format!(
        r#"{{"type":"transition","from":"{:?}","to":"{:?}","event":"{:?}"}}"#,
        t.old_state, t.new_state, t.event,
    )
}

/// Format an effect data item as a JSON object string.
fn json_effect(stream_name: &str, data: &EffectData) -> String {
    match data {
        EffectData::LogLine(line) => {
            // Escape backslashes and double quotes for JSON safety.
            let escaped = line.replace('\\', "\\\\").replace('"', "\\\"");
            format!(
                r#"{{"type":"effect","stream":"{stream_name}","kind":"log","data":"{escaped}"}}"#,
            )
        }
        EffectData::Progress { current, total } => {
            format!(
                r#"{{"type":"effect","stream":"{stream_name}","kind":"progress","current":{current},"total":{total}}}"#,
            )
        }
        EffectData::Info(info) => {
            let escaped = info.replace('\\', "\\\\").replace('"', "\\\"");
            format!(
                r#"{{"type":"effect","stream":"{stream_name}","kind":"info","data":"{escaped}"}}"#,
            )
        }
    }
}

impl Observer for JsonObserver {
    fn on_transition(
        &mut self,
        t: &Transition,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        let line = json_transition(t);
        Box::pin(async move {
            println!("{line}");
        })
    }

    fn on_effect_stream(
        &mut self,
        name: &str,
        mut rx: roam::Rx<EffectData>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        let stream_name = name.to_string();
        Box::pin(async move {
            while let Ok(Some(data)) = rx.recv().await {
                println!("{}", json_effect(&stream_name, &data));
            }
        })
    }

    fn clone_for_stream(&self) -> Box<dyn Observer> {
        Box::new(self.clone())
    }
}
