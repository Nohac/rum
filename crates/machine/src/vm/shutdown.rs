use virt::domain::Domain;

use crate::config::SystemConfig;
use crate::error::RumError;

pub async fn shutdown_vm(sys_config: &SystemConfig) -> Result<(), RumError> {
    let vm_name = sys_config.display_name();
    let conn = crate::vm::libvirt::connect(sys_config)?;

    let dom = Domain::lookup_by_name(&conn, vm_name).map_err(|e| RumError::Libvirt {
        message: format!("domain lookup failed: {e}"),
        hint: "VM may not be defined".into(),
    })?;

    crate::vm::libvirt::shutdown_domain(&dom).await
}
