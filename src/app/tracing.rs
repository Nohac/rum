use tracing_subscriber::EnvFilter;
use tracing_subscriber::Layer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use crate::logging;
use crate::progress::OutputMode;

pub struct LoggingHandles {
    pub file_handle: logging::DeferredFileHandle,
}

pub fn init_tracing(mode: OutputMode) -> LoggingHandles {
    let terminal_filter = match mode {
        OutputMode::Verbose => EnvFilter::new("debug"),
        OutputMode::Normal | OutputMode::Quiet | OutputMode::Silent => EnvFilter::new("off"),
        OutputMode::Plain => EnvFilter::from_default_env()
            .add_directive("rum=info".parse().expect("valid log directive")),
    };

    let terminal_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_filter(terminal_filter);

    let (file_writer, file_handle) = logging::DeferredFileWriter::new();
    let file_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_writer(file_writer)
        .with_filter(EnvFilter::new("rum=debug"));

    tracing_subscriber::registry()
        .with(terminal_layer)
        .with(file_layer)
        .init();

    LoggingHandles { file_handle }
}
