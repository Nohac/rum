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

    #[error("mount source not found: {path}")]
    #[diagnostic(help("check that the directory exists"))]
    MountSourceNotFound { path: String },

    #[error("failed to detect git repository: {message}")]
    #[diagnostic(help("source = \"git\" requires rum.toml to be inside a git repository"))]
    GitRepoDetection { message: String },

    #[error("{command} is not yet implemented")]
    NotImplemented { command: String },

    #[error("SSH not ready for '{name}': {reason}")]
    #[diagnostic(help("ensure the VM is running with `rum status`"))]
    SshNotReady { name: String, reason: String },

    #[error("exec not ready for '{name}': {reason}")]
    #[diagnostic(help("ensure the VM is running with `rum up` first"))]
    ExecNotReady { name: String, reason: String },

    #[error("init cancelled by user")]
    InitCancelled,

    #[error("{message}")]
    #[diagnostic(help("check that the VM booted and rum-agent started"))]
    AgentTimeout { message: String },

    #[error("provisioning failed: script '{script}' exited with non-zero status")]
    #[diagnostic(help("run `rum log --failed` to see the full script output"))]
    ProvisionFailed { script: String },

    #[error("daemon error: {message}")]
    Daemon { message: String },

    #[error("failed to write config: {path}")]
    ConfigWrite {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("copy failed: {message}")]
    #[diagnostic(help("ensure the VM is running and the path is accessible"))]
    CopyFailed { message: String },
}
