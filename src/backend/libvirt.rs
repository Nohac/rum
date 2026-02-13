use crate::config::Config;
use crate::error::RumError;

pub struct LibvirtBackend;

impl super::Backend for LibvirtBackend {
    async fn up(&self, config: &Config) -> Result<(), RumError> {
        tracing::info!(name = %config.name, "would start VM (not yet implemented)");
        Err(RumError::NotImplemented {
            command: "up".into(),
        })
    }

    async fn down(&self, config: &Config) -> Result<(), RumError> {
        tracing::info!(name = %config.name, "would stop VM (not yet implemented)");
        Err(RumError::NotImplemented {
            command: "down".into(),
        })
    }
}
