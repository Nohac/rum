use virt::connect::Connect;
use virt::domain::Domain;
use virt::error as virt_error;

use crate::config::SystemConfig;
use crate::error::RumError;

pub struct ConnGuard(pub Connect);

impl std::ops::Deref for ConnGuard {
    type Target = Connect;

    fn deref(&self) -> &Connect {
        &self.0
    }
}

impl Drop for ConnGuard {
    fn drop(&mut self) {
        self.0.close().ok();
    }
}

pub fn connect(sys_config: &SystemConfig) -> Result<ConnGuard, RumError> {
    virt_error::clear_error_callback();

    Connect::open(Some(sys_config.libvirt_uri()))
        .map(ConnGuard)
        .map_err(|e| RumError::Libvirt {
            message: format!("failed to connect to libvirt: {e}"),
            hint: format!(
                "ensure libvirtd is running and you have access to {}",
                sys_config.libvirt_uri()
            ),
        })
}

pub fn define_domain(conn: &Connect, xml: &str) -> Result<Domain, RumError> {
    Domain::define_xml(conn, xml).map_err(|e| RumError::Libvirt {
        message: format!("failed to define domain: {e}"),
        hint: "check the generated domain XML for errors".into(),
    })
}

pub fn is_running(dom: &Domain) -> bool {
    dom.is_active().unwrap_or(false)
}

pub async fn shutdown_domain(dom: &Domain) -> Result<(), RumError> {
    if !is_running(dom) {
        return Ok(());
    }
    dom.shutdown().map_err(|e| RumError::Libvirt {
        message: format!("shutdown failed: {e}"),
        hint: "VM may not support ACPI shutdown".into(),
    })?;

    for _ in 0..10 {
        if !is_running(dom) {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }

    dom.destroy().map_err(|e| RumError::Libvirt {
        message: format!("force stop failed: {e}"),
        hint: "check libvirt permissions".into(),
    })?;
    Ok(())
}

pub fn parse_vsock_cid(dom: &Domain) -> Option<u32> {
    let xml = dom.get_xml_desc(0).ok()?;
    domain::parse_vsock_cid(&xml)
}
