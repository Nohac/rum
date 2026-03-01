use virt::connect::Connect;
use virt::domain::Domain;
use virt::error as virt_error;

use crate::config::SystemConfig;
use crate::error::RumError;
use crate::{domain_xml, paths};

pub(crate) struct ConnGuard(Connect);

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

pub struct LibvirtBackend;

impl super::Backend for LibvirtBackend {
    async fn ssh(&self, sys_config: &SystemConfig, args: &[String]) -> Result<(), RumError> {
        let vm_name = sys_config.display_name();
        let id = &sys_config.id;
        let name_opt = sys_config.name.as_deref();
        let conn = connect(sys_config)?;

        let dom = Domain::lookup_by_name(&conn, vm_name).map_err(|_| RumError::SshNotReady {
            name: vm_name.to_string(),
            reason: "VM is not defined".into(),
        })?;

        if !is_running(&dom) {
            return Err(RumError::SshNotReady {
                name: vm_name.to_string(),
                reason: "VM is not running".into(),
            });
        }

        let ip = get_vm_ip(&dom, sys_config)?;
        let ssh_key_path = paths::ssh_key_path(id, name_opt);

        if !ssh_key_path.exists() {
            return Err(RumError::SshNotReady {
                name: vm_name.to_string(),
                reason: "SSH key not found (run `rum up` first)".into(),
            });
        }

        drop(conn);

        let ssh_config = &sys_config.config.ssh;
        let cmd_parts: Vec<&str> = ssh_config.command.split_whitespace().collect();
        let program = cmd_parts[0];
        let cmd_args = &cmd_parts[1..];

        let key_str = ssh_key_path.to_string_lossy();
        let user_host = format!("{}@{}", ssh_config.user, ip);

        // Use exec() to replace the rum process with the ssh command, giving
        // it full terminal control.
        use std::os::unix::process::CommandExt;
        let mut command = std::process::Command::new(program);
        command.args(cmd_args);
        command.args(["-i", &key_str]);
        // Only inject host-key options for plain `ssh`. Custom commands like
        // `kitty +kitten ssh` manage host verification themselves and these
        // options can interfere with their terminal protocol.
        if program == "ssh" {
            command.args([
                "-o",
                "StrictHostKeyChecking=no",
                "-o",
                "UserKnownHostsFile=/dev/null",
            ]);
        }
        command.arg(&user_host);
        command.args(args);

        // exec() replaces this process — only returns on error
        let err = command.exec();
        Err(RumError::Io {
            context: format!("exec {}", ssh_config.command),
            source: err,
        })
    }
}

/// Look up the vsock CID for a running VM.
///
/// Connects to libvirt, verifies the domain exists and is running,
/// then parses the auto-assigned CID from the live domain XML.
pub fn get_vsock_cid(sys_config: &SystemConfig) -> Result<u32, RumError> {
    let vm_name = sys_config.display_name();
    let conn = connect(sys_config)?;

    let dom = Domain::lookup_by_name(&conn, vm_name).map_err(|_| RumError::DomainNotFound {
        name: vm_name.to_string(),
    })?;

    if !is_running(&dom) {
        return Err(RumError::ExecNotReady {
            name: vm_name.to_string(),
            reason: "VM is not running".into(),
        });
    }

    parse_vsock_cid(&dom).ok_or_else(|| RumError::ExecNotReady {
        name: vm_name.to_string(),
        reason: "could not determine vsock CID from domain XML".into(),
    })
}

fn get_vm_ip(dom: &Domain, sys_config: &SystemConfig) -> Result<String, RumError> {
    let vm_name = sys_config.display_name();
    let ifaces = dom
        .interface_addresses(virt::sys::VIR_DOMAIN_INTERFACE_ADDRESSES_SRC_LEASE, 0)
        .map_err(|_| RumError::SshNotReady {
            name: vm_name.to_string(),
            reason: "could not query network interfaces".into(),
        })?;

    let ssh_interface = &sys_config.config.ssh.interface;

    if ssh_interface.is_empty() {
        // NAT mode: return first IPv4 address that doesn't belong to an extra interface
        let extra_macs: Vec<String> = sys_config
            .config
            .network
            .interfaces
            .iter()
            .enumerate()
            .map(|(i, _)| domain_xml::generate_mac(vm_name, i))
            .collect();

        for iface in &ifaces {
            let iface_mac = iface.hwaddr.to_lowercase();
            if extra_macs.iter().any(|m| m.to_lowercase() == iface_mac) {
                continue;
            }
            for addr in &iface.addrs {
                // IPv4 only (type 0 in libvirt)
                if addr.typed == 0 {
                    return Ok(addr.addr.clone());
                }
            }
        }
    } else {
        // Named interface: find matching MAC from config interfaces
        let iface_idx = sys_config
            .config
            .network
            .interfaces
            .iter()
            .position(|i| i.network == *ssh_interface);

        if let Some(idx) = iface_idx {
            let expected_mac = domain_xml::generate_mac(vm_name, idx).to_lowercase();
            for iface in &ifaces {
                if iface.hwaddr.to_lowercase() == expected_mac {
                    for addr in &iface.addrs {
                        if addr.typed == 0 {
                            return Ok(addr.addr.clone());
                        }
                    }
                }
            }
        }
    }

    Err(RumError::SshNotReady {
        name: vm_name.to_string(),
        reason: "no IP address found (VM may still be booting)".into(),
    })
}

pub(crate) fn connect(sys_config: &SystemConfig) -> Result<ConnGuard, RumError> {
    // Suppress libvirt's default error handler that prints to stderr.
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

pub(crate) fn is_running(dom: &Domain) -> bool {
    dom.is_active().unwrap_or(false)
}

pub(crate) async fn shutdown_domain(dom: &Domain) -> Result<(), RumError> {
    if !is_running(dom) {
        return Ok(());
    }
    dom.shutdown().map_err(|e| RumError::Libvirt {
        message: format!("shutdown failed: {e}"),
        hint: "VM may not support ACPI shutdown".into(),
    })?;

    // Brief wait for shutdown
    for _ in 0..10 {
        if !is_running(dom) {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }

    // Force
    dom.destroy().map_err(|e| RumError::Libvirt {
        message: format!("force stop failed: {e}"),
        hint: "check libvirt permissions".into(),
    })?;
    Ok(())
}

/// Extract the auto-assigned vsock CID from a running domain's live XML.
fn parse_vsock_cid(dom: &Domain) -> Option<u32> {
    let xml = dom.get_xml_desc(0).ok()?;
    domain_xml::parse_vsock_cid(&xml)
}
