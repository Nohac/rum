use virt::connect::Connect;
use virt::domain::Domain;
use virt::error as virt_error;

use crate::config::SystemConfig;
use crate::paths;

#[derive(Debug, Clone)]
pub struct StatusInfo {
    pub state: String,
    pub ips: Vec<String>,
    pub daemon_running: bool,
}

fn connect_libvirt(sys_config: &SystemConfig) -> Result<Connect, String> {
    virt_error::clear_error_callback();
    Connect::open(Some(sys_config.libvirt_uri())).map_err(|e| format!("libvirt connect failed: {e}"))
}

fn lookup_domain(conn: &Connect, sys_config: &SystemConfig) -> Result<Domain, String> {
    Domain::lookup_by_name(conn, sys_config.display_name())
        .map_err(|_| format!("VM '{}' is not defined", sys_config.display_name()))
}

pub fn current_status(sys_config: &SystemConfig, daemon_running: bool) -> StatusInfo {
    let Ok(conn) = connect_libvirt(sys_config) else {
        return StatusInfo {
            state: "not defined".to_string(),
            ips: Vec::new(),
            daemon_running,
        };
    };

    match Domain::lookup_by_name(&conn, sys_config.display_name()) {
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
                daemon_running,
            }
        }
        Err(_) => StatusInfo {
            state: "not defined".to_string(),
            ips: Vec::new(),
            daemon_running,
        },
    }
}

pub fn ssh_config(sys_config: &SystemConfig) -> Result<String, String> {
    let conn = connect_libvirt(sys_config)?;
    let dom = lookup_domain(&conn, sys_config)?;

    if !dom.is_active().unwrap_or(false) {
        return Err(format!("VM '{}' is not running", sys_config.display_name()));
    }

    let ip = get_first_ip(&dom)
        .ok_or_else(|| format!("no IP found for VM '{}'", sys_config.display_name()))?;

    let ssh_user = &sys_config.config.ssh.user;
    let ssh_key_path = paths::ssh_key_path(&sys_config.id, sys_config.name.as_deref());

    Ok(format!(
        "Host {vm}\n  \
         HostName {ip}\n  \
         User {user}\n  \
         IdentityFile {key}\n  \
         StrictHostKeyChecking no\n  \
         UserKnownHostsFile /dev/null\n  \
         LogLevel ERROR",
        vm = sys_config.display_name(),
        user = ssh_user,
        key = ssh_key_path.display(),
    ))
}

pub(super) fn get_first_ip(dom: &Domain) -> Option<String> {
    let ifaces = dom
        .interface_addresses(virt::sys::VIR_DOMAIN_INTERFACE_ADDRESSES_SRC_LEASE, 0)
        .ok()?;

    for iface in &ifaces {
        for addr in &iface.addrs {
            if addr.typed == 0 {
                return Some(addr.addr.clone());
            }
        }
    }
    None
}
