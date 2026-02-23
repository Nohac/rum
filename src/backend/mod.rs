pub mod libvirt;

use crate::config::SystemConfig;
use crate::error::RumError;
use crate::progress::OutputMode;

#[allow(async_fn_in_trait)] // trait is internal-only
pub trait Backend {
    async fn up(
        &self,
        sys_config: &SystemConfig,
        reset: bool,
        detach: bool,
        mode: OutputMode,
    ) -> Result<(), RumError>;
    async fn ssh(&self, sys_config: &SystemConfig, args: &[String]) -> Result<(), RumError>;
}

pub fn create_backend() -> libvirt::LibvirtBackend {
    libvirt::LibvirtBackend
}
