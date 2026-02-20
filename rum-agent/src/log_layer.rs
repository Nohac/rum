use std::time::{SystemTime, UNIX_EPOCH};

use tokio::sync::broadcast;
use tracing::field::{Field, Visit};
use tracing_subscriber::layer::Context;
use tracing_subscriber::Layer;

use rum_agent::{LogEvent, LogLevel, LogStream};

pub struct BroadcastLayer {
    tx: broadcast::Sender<LogEvent>,
}

pub fn log_broadcast_layer() -> (BroadcastLayer, broadcast::Sender<LogEvent>) {
    let (tx, _) = broadcast::channel(256);
    let layer = BroadcastLayer { tx: tx.clone() };
    (layer, tx)
}

impl<S: tracing::Subscriber> Layer<S> for BroadcastLayer {
    fn on_event(&self, event: &tracing::Event<'_>, _cx: Context<'_, S>) {
        if self.tx.receiver_count() == 0 {
            return;
        }

        let level = match *event.metadata().level() {
            tracing::Level::TRACE => LogLevel::Trace,
            tracing::Level::DEBUG => LogLevel::Debug,
            tracing::Level::INFO => LogLevel::Info,
            tracing::Level::WARN => LogLevel::Warn,
            tracing::Level::ERROR => LogLevel::Error,
        };

        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);

        let timestamp_us = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros() as u64;

        let log_event = LogEvent {
            timestamp_us,
            level,
            target: event.metadata().target().to_string(),
            message: visitor.into_message(),
            stream: LogStream::Log,
        };

        let _ = self.tx.send(log_event);
    }
}

#[derive(Default)]
struct MessageVisitor {
    message: String,
    fields: Vec<String>,
}

impl MessageVisitor {
    fn into_message(self) -> String {
        if self.fields.is_empty() {
            self.message
        } else {
            format!("{} {}", self.message, self.fields.join(" "))
        }
    }
}

impl Visit for MessageVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{value:?}");
        } else {
            self.fields.push(format!("{}={:?}", field.name(), value));
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        } else {
            self.fields.push(format!("{}={}", field.name(), value));
        }
    }
}
