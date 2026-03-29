#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("{context}: {source}")]
    Io {
        context: String,
        #[source]
        source: std::io::Error,
    },
    #[error("agent did not respond within {timeout_secs}s: {message}")]
    AgentTimeout { timeout_secs: u64, message: String },
    #[error("{context}: {message}")]
    Rpc { context: String, message: String },
    #[error("copy failed: {message}")]
    CopyFailed { message: String },
    #[error("provision failed: {script}")]
    ProvisionFailed { script: String },
}
