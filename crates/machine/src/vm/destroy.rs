use virt::connect::Connect;
use virt::domain::Domain;
use virt::error as virt_error;
use virt::network::Network;

use crate::config::SystemConfig;
use crate::error::Error;
use crate::paths;

pub async fn destroy_vm(sys_config: &SystemConfig) -> Result<(), Error> {
    let id = &sys_config.id;
    let name_opt = sys_config.name.as_deref();
    let vm_name = sys_config.display_name();
    let config = &sys_config.config;

    virt_error::clear_error_callback();

    if let Ok(conn) = Connect::open(Some(sys_config.libvirt_uri())).map(crate::vm::libvirt::ConnGuard) {
        if let Ok(dom) = Domain::lookup_by_name(&conn, vm_name) {
            if dom.is_active().unwrap_or(false) {
                let _ = dom.destroy();
            }
            let _ = dom.undefine();
        }

        for iface in &config.network.interfaces {
            let net_name = domain::prefixed_name(id, &iface.network);
            if let Ok(net) = Network::lookup_by_name(&conn, &net_name) {
                if net.is_active().unwrap_or(false) {
                    let _ = net.destroy();
                }
                let _ = net.undefine();
            }
        }
    }

    let work = paths::work_dir(id, name_opt);
    if work.exists() {
        tokio::fs::remove_dir_all(&work)
            .await
            .map_err(|e| Error::Io {
                context: format!("removing {}", work.display()),
                source: e,
            })?;
    }

    Ok(())
}
