use virt::domain::Domain;

use crate::config::SystemConfig;
use crate::error::Error;

pub async fn boot_vm(sys_config: &SystemConfig) -> Result<u32, Error> {
    let vm_name = sys_config.display_name();
    let conn = crate::vm::libvirt::connect(sys_config)?;

    let dom = Domain::lookup_by_name(&conn, vm_name).map_err(|e| Error::Libvirt {
        message: format!("domain lookup failed: {e}"),
        hint: "domain should have been defined in prepare_vm".into(),
    })?;

    if !crate::vm::libvirt::is_running(&dom) {
        dom.create().map_err(|e| Error::Libvirt {
            message: format!("failed to start domain: {e}"),
            hint: "check `virsh -c qemu:///system start` for details".into(),
        })?;
        tracing::info!(vm_name, "VM started");
    }

    crate::vm::libvirt::parse_vsock_cid(&dom).ok_or_else(|| Error::Libvirt {
        message: "could not determine vsock CID from live XML".into(),
        hint: "ensure the domain XML includes a <vsock> device".into(),
    })
}
