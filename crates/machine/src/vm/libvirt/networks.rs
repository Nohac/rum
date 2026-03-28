use virt::connect::Connect;
use virt::network::Network;

use crate::config::SystemConfig;
use crate::error::RumError;

fn ensure_network_active(conn: &Connect, name: &str) -> Result<Network, RumError> {
    let net = Network::lookup_by_name(conn, name).map_err(|_| RumError::Libvirt {
        message: format!("network '{name}' not found"),
        hint: format!("define the network with `virsh net-define` and `virsh net-start {name}`"),
    })?;

    if !net.is_active().unwrap_or(false) {
        tracing::info!(name, "starting inactive network");
        net.create().map_err(|e| RumError::Libvirt {
            message: format!("failed to start network '{name}': {e}"),
            hint: format!("try `sudo virsh net-start {name}`"),
        })?;
    }

    Ok(net)
}

fn ensure_extra_network(conn: &Connect, name: &str, ip_hint: &str) -> Result<Network, RumError> {
    match Network::lookup_by_name(conn, name) {
        Ok(net) => {
            if !net.is_active().unwrap_or(false) {
                tracing::info!(name, "starting inactive network");
                net.create().map_err(|e| RumError::Libvirt {
                    message: format!("failed to start network '{name}': {e}"),
                    hint: "check libvirt permissions".into(),
                })?;
            }
            Ok(net)
        }
        Err(_) => {
            let subnet = domain::derive_subnet(name, ip_hint);
            let xml = domain::generate_network_xml(name, &subnet);
            tracing::info!(name, subnet, "auto-creating host-only network");
            let net = Network::define_xml(conn, &xml).map_err(|e| RumError::Libvirt {
                message: format!("failed to define network '{name}': {e}"),
                hint: "check libvirt permissions".into(),
            })?;
            net.create().map_err(|e| RumError::Libvirt {
                message: format!("failed to start network '{name}': {e}"),
                hint: "check libvirt permissions".into(),
            })?;
            Ok(net)
        }
    }
}

fn add_dhcp_reservation(
    net: &Network,
    net_name: &str,
    mac: &str,
    ip: &str,
    hostname: &str,
) -> Result<(), RumError> {
    let host_xml = format!("<host mac='{mac}' name='{hostname}' ip='{ip}'/>");

    let modify = virt::sys::VIR_NETWORK_UPDATE_COMMAND_ADD_LAST;
    let section = virt::sys::VIR_NETWORK_SECTION_IP_DHCP_HOST;
    let flags =
        virt::sys::VIR_NETWORK_UPDATE_AFFECT_LIVE | virt::sys::VIR_NETWORK_UPDATE_AFFECT_CONFIG;

    match net.update(modify, section, -1, &host_xml, flags) {
        Ok(_) => {
            tracing::info!(net_name, mac, ip, "added DHCP reservation");
        }
        Err(e) => {
            let modify_cmd = virt::sys::VIR_NETWORK_UPDATE_COMMAND_MODIFY;
            net.update(modify_cmd, section, -1, &host_xml, flags)
                .map_err(|e2| RumError::Libvirt {
                    message: format!(
                        "failed to set DHCP reservation in '{net_name}': add={e}, modify={e2}"
                    ),
                    hint: format!("ensure network '{net_name}' has a DHCP range configured"),
                })?;
            tracing::info!(net_name, mac, ip, "updated DHCP reservation");
        }
    }

    Ok(())
}

pub fn ensure_networks(conn: &Connect, sys_config: &SystemConfig) -> Result<(), RumError> {
    let config = &sys_config.config;

    if config.network.nat {
        ensure_network_active(conn, "default")?;
    }

    for (i, iface) in config.network.interfaces.iter().enumerate() {
        let libvirt_name = domain::prefixed_name(&sys_config.id, &iface.network);
        let net = ensure_extra_network(conn, &libvirt_name, &iface.ip)?;

        if !iface.ip.is_empty() {
            let mac = domain::generate_mac(sys_config.display_name(), i);
            add_dhcp_reservation(&net, &libvirt_name, &mac, &iface.ip, sys_config.hostname())?;
        }
    }

    Ok(())
}
