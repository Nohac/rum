pub mod libvirt;

use crate::config::Config;
use crate::error::RumError;

pub trait Backend {
    fn up(&self, config: &Config) -> impl Future<Output = Result<(), RumError>> + Send;
    fn down(&self, config: &Config) -> impl Future<Output = Result<(), RumError>> + Send;
}

pub fn create_backend() -> libvirt::LibvirtBackend {
    libvirt::LibvirtBackend
}
