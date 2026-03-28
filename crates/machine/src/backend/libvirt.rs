use virt::domain::Domain;

use crate::config::SystemConfig;
use crate::error::Error;
use crate::paths;
use crate::vm::libvirt::{connect, is_running};

pub async fn ssh(sys_config: &SystemConfig, args: &[String]) -> Result<(), Error> {
    let vm_name = sys_config.display_name();
    let id = &sys_config.id;
    let name_opt = sys_config.name.as_deref();
    let conn = connect(sys_config)?;

    let dom = Domain::lookup_by_name(&conn, vm_name).map_err(|_| Error::SshNotReady {
        name: vm_name.to_string(),
        reason: "VM is not defined".into(),
    })?;

    if !is_running(&dom) {
        return Err(Error::SshNotReady {
            name: vm_name.to_string(),
            reason: "VM is not running".into(),
        });
    }

    let ip = get_vm_ip(&dom, sys_config)?;
    let ssh_key_path = paths::ssh_key_path(id, name_opt);

    if !ssh_key_path.exists() {
        return Err(Error::SshNotReady {
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

    use std::os::unix::process::CommandExt;
    let mut command = std::process::Command::new(program);
    command.args(cmd_args);
    command.args(["-i", &key_str]);
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

    let err = command.exec();
    Err(Error::Io {
        context: format!("exec {}", ssh_config.command),
        source: err,
    })
}

/// Look up the vsock CID for a running VM.
///
/// Connects to libvirt, verifies the domain exists and is running,
/// then parses the auto-assigned CID from the live domain XML.
pub fn get_vsock_cid(sys_config: &SystemConfig) -> Result<u32, Error> {
    let vm_name = sys_config.display_name();
    let conn = crate::vm::libvirt::connect(sys_config)?;

    let dom = Domain::lookup_by_name(&conn, vm_name).map_err(|_| Error::DomainNotFound {
        name: vm_name.to_string(),
    })?;

    if !crate::vm::libvirt::is_running(&dom) {
        return Err(Error::ExecNotReady {
            name: vm_name.to_string(),
            reason: "VM is not running".into(),
        });
    }

    crate::vm::libvirt::parse_vsock_cid(&dom).ok_or_else(|| Error::ExecNotReady {
        name: vm_name.to_string(),
        reason: "could not determine vsock CID from domain XML".into(),
    })
}

fn get_vm_ip(dom: &Domain, sys_config: &SystemConfig) -> Result<String, Error> {
    let vm_name = sys_config.display_name();
    let ifaces = dom
        .interface_addresses(virt::sys::VIR_DOMAIN_INTERFACE_ADDRESSES_SRC_LEASE, 0)
        .map_err(|_| Error::SshNotReady {
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
            .map(|(i, _)| domain::generate_mac(vm_name, i))
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
            let expected_mac = domain::generate_mac(vm_name, idx).to_lowercase();
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

    Err(Error::SshNotReady {
        name: vm_name.to_string(),
        reason: "no IP address found (VM may still be booting)".into(),
    })
}
