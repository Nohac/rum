use miette::Diagnostic;
use thiserror::Error;

#[derive(Debug, Error, Diagnostic)]
pub enum RumError {
    #[error("failed to load config from {path}")]
    ConfigLoad {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to parse config from {path}: {message}")]
    ConfigParse { path: String, message: String },

    #[error("validation error: {message}")]
    Validation { message: String },

    #[error("failed to download image: {message}")]
    ImageDownload {
        message: String,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    #[error("{command} failed: {message}")]
    #[diagnostic(help("ensure {command} is installed and accessible"))]
    ExternalCommand { command: String, message: String },

    #[error("libvirt error: {message}")]
    #[diagnostic(help("{hint}"))]
    Libvirt { message: String, hint: String },

    #[error("config changed while VM '{name}' is running — restart required")]
    #[diagnostic(help("run `rum down` then `rum up`, or use `rum up --reset`"))]
    RequiresRestart { name: String },

    #[error("domain '{name}' not found")]
    #[diagnostic(help("run `rum up` to create the VM first"))]
    DomainNotFound { name: String },

    #[error("timed out waiting for IP on '{name}' after {timeout_s}s")]
    IpTimeout { name: String, timeout_s: u64 },

    #[error("{context}")]
    Io {
        context: String,
        #[source]
        source: std::io::Error,
    },

    #[error("{command} is not yet implemented")]
    NotImplemented { command: String },
}
