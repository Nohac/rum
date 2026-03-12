use crate::config::SystemConfig;
use crate::daemon::StatusInfo;

/// Offline status check when no daemon is running.
/// Queries libvirt directly to determine VM state.
pub fn offline_status(sys_config: &SystemConfig) -> StatusInfo {
    use virt::connect::Connect;
    use virt::domain::Domain;

    virt::error::clear_error_callback();
    let vm_name = sys_config.display_name();

    let Ok(mut conn) = Connect::open(Some(sys_config.libvirt_uri())) else {
        return StatusInfo {
            state: "not defined".to_string(),
            ips: Vec::new(),
            daemon_running: false,
        };
    };

    let info = match Domain::lookup_by_name(&conn, vm_name) {
        Ok(dom) => {
            let state = if dom.is_active().unwrap_or(false) {
                "running".to_string()
            } else {
                "stopped".to_string()
            };

            let mut ips = Vec::new();
            if dom.is_active().unwrap_or(false)
                && let Ok(ifaces) = dom.interface_addresses(
                    virt::sys::VIR_DOMAIN_INTERFACE_ADDRESSES_SRC_LEASE,
                    0,
                )
            {
                for iface in &ifaces {
                    for addr in &iface.addrs {
                        ips.push(addr.addr.clone());
                    }
                }
            }

            StatusInfo {
                state,
                ips,
                daemon_running: false,
            }
        }
        Err(_) => StatusInfo {
            state: "not defined".to_string(),
            ips: Vec::new(),
            daemon_running: false,
        },
    };

    let _ = conn.close();
    info
}

#[derive(facet::Facet)]
pub struct StatusJson {
    pub name: String,
    pub state: String,
    pub ips: Vec<String>,
    pub daemon: bool,
}
