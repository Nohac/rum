pub mod libvirt;

use crate::config::Config;
use crate::error::RumError;

#[allow(async_fn_in_trait)]
pub trait Backend {
    async fn up(&self, config: &Config, reset: bool) -> Result<(), RumError>;
    async fn down(&self, config: &Config) -> Result<(), RumError>;
    async fn destroy(&self, config: &Config, purge: bool) -> Result<(), RumError>;
    async fn status(&self, config: &Config) -> Result<(), RumError>;
}

pub fn create_backend() -> libvirt::LibvirtBackend {
    libvirt::LibvirtBackend
}
