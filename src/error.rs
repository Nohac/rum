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

    #[error("{command} is not yet implemented")]
    NotImplemented { command: String },
}
