use crate::config::SystemConfig;
use crate::error::RumError;

#[allow(async_fn_in_trait)] // trait is internal-only
pub trait Backend {
    async fn ssh(&self, sys_config: &SystemConfig, args: &[String]) -> Result<(), RumError>;
}

pub fn create_backend() -> super::libvirt::LibvirtBackend {
    super::libvirt::LibvirtBackend
}
