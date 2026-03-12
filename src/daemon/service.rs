use std::path::PathBuf;

use tokio::sync::mpsc;
use virt::connect::Connect;
use virt::domain::Domain;
use virt::error as virt_error;

use crate::config::SystemConfig;
use crate::paths;

use super::rpc::{RumDaemon, StatusInfo};

// ── DaemonImpl: daemon-mode implementation ──────────────────────────

pub(super) enum DaemonAction {
    Shutdown,
    ForceStop,
}

#[derive(Clone)]
pub(super) struct DaemonImpl {
    uri: String,
    vm_name: String,
    ssh_user: String,
    ssh_key_path: PathBuf,
    action_tx: mpsc::Sender<DaemonAction>,
}

impl DaemonImpl {
    pub(super) fn new(sys_config: &SystemConfig, action_tx: mpsc::Sender<DaemonAction>) -> Self {
        Self {
            uri: sys_config.libvirt_uri().to_string(),
            vm_name: sys_config.display_name().to_string(),
            ssh_user: sys_config.config.ssh.user.clone(),
            ssh_key_path: paths::ssh_key_path(&sys_config.id, sys_config.name.as_deref()),
            action_tx,
        }
    }

    /// Signal the daemon loop to exit after a short delay, giving the
    /// RPC response time to flush back to the client before teardown.
    fn signal_exit_deferred(&self, action: DaemonAction) {
        let tx = self.action_tx.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            let _ = tx.send(action).await;
        });
    }

    fn connect_libvirt(&self) -> Result<Connect, String> {
        virt_error::clear_error_callback();
        Connect::open(Some(&self.uri)).map_err(|e| format!("libvirt connect failed: {e}"))
    }

    fn lookup_domain(&self, conn: &Connect) -> Result<Domain, String> {
        Domain::lookup_by_name(conn, &self.vm_name)
            .map_err(|_| format!("VM '{}' is not defined", self.vm_name))
    }
}

impl RumDaemon for DaemonImpl {
    async fn ping(&self, _cx: &roam::Context) -> Result<String, String> {
        Ok("daemon".into())
    }

    async fn shutdown(&self, _cx: &roam::Context) -> Result<String, String> {
        let conn = self.connect_libvirt()?;
        let dom = self.lookup_domain(&conn)?;

        if !dom.is_active().unwrap_or(false) {
            return Ok(format!("VM '{}' is not running.", self.vm_name));
        }

        // ACPI shutdown
        dom.shutdown()
            .map_err(|e| format!("shutdown failed: {e}"))?;

        // Wait up to 30s
        for _ in 0..30 {
            if !dom.is_active().unwrap_or(false) {
                self.signal_exit_deferred(DaemonAction::Shutdown);
                return Ok(format!("VM '{}' stopped.", self.vm_name));
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }

        // Force stop
        let _ = dom.destroy();
        self.signal_exit_deferred(DaemonAction::Shutdown);
        Ok(format!("VM '{}' force stopped.", self.vm_name))
    }

    async fn force_stop(&self, _cx: &roam::Context) -> Result<String, String> {
        let conn = self.connect_libvirt()?;

        if let Ok(dom) = Domain::lookup_by_name(&conn, &self.vm_name)
            && dom.is_active().unwrap_or(false)
        {
            let _ = dom.destroy();
        }

        self.signal_exit_deferred(DaemonAction::ForceStop);
        Ok(format!("VM '{}' force stopped.", self.vm_name))
    }

    async fn status(&self, _cx: &roam::Context) -> Result<StatusInfo, String> {
        let conn = self.connect_libvirt()?;

        match Domain::lookup_by_name(&conn, &self.vm_name) {
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

                Ok(StatusInfo {
                    state,
                    ips,
                    daemon_running: true,
                })
            }
            Err(_) => Ok(StatusInfo {
                state: "not defined".to_string(),
                ips: Vec::new(),
                daemon_running: true,
            }),
        }
    }

    async fn ssh_config(&self, _cx: &roam::Context) -> Result<String, String> {
        let conn = self.connect_libvirt()?;
        let dom = self.lookup_domain(&conn)?;

        if !dom.is_active().unwrap_or(false) {
            return Err(format!("VM '{}' is not running", self.vm_name));
        }

        let ip = get_first_ip(&dom)
            .ok_or_else(|| format!("no IP found for VM '{}'", self.vm_name))?;

        Ok(format!(
            "Host {vm}\n  \
             HostName {ip}\n  \
             User {user}\n  \
             IdentityFile {key}\n  \
             StrictHostKeyChecking no\n  \
             UserKnownHostsFile /dev/null\n  \
             LogLevel ERROR",
            vm = self.vm_name,
            user = self.ssh_user,
            key = self.ssh_key_path.display(),
        ))
    }
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
