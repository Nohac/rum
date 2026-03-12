use crate::config::SystemConfig;
use crate::error::RumError;

/// Local cleanup after force-stopping a VM: undefine domain,
/// tear down auto-created networks, remove work dir.
pub async fn destroy_cleanup(sys_config: &SystemConfig) -> Result<(), RumError> {
    use virt::connect::Connect;
    use virt::domain::Domain;
    use virt::network::Network;

    let id = &sys_config.id;
    let name_opt = sys_config.name.as_deref();
    let vm_name = sys_config.display_name();
    let config = &sys_config.config;

    virt::error::clear_error_callback();
    let mut had_domain = false;
    let mut had_artifacts = false;

    if let Ok(mut conn) = Connect::open(Some(sys_config.libvirt_uri())) {
        if let Ok(dom) = Domain::lookup_by_name(&conn, vm_name) {
            had_domain = true;
            if dom.is_active().unwrap_or(false) {
                let _ = dom.destroy();
            }
            let _ = dom.undefine();
        }

        // Tear down auto-created networks
        for iface in &config.network.interfaces {
            let net_name = crate::network_xml::prefixed_name(id, &iface.network);
            if let Ok(net) = Network::lookup_by_name(&conn, &net_name) {
                if net.is_active().unwrap_or(false) {
                    let _ = net.destroy();
                }
                let _ = net.undefine();
            }
        }

        let _ = conn.close();
    }

    // Remove work dir
    let work = crate::paths::work_dir(id, name_opt);
    if work.exists() {
        had_artifacts = true;
        tokio::fs::remove_dir_all(&work)
            .await
            .map_err(|e| RumError::Io {
                context: format!("removing {}", work.display()),
                source: e,
            })?;
    }

    match (had_domain, had_artifacts) {
        (true, _) => println!("VM '{vm_name}' destroyed."),
        (false, true) => println!("Removed artifacts for '{vm_name}'."),
        (false, false) => println!("VM '{vm_name}' not found — nothing to destroy."),
    }

    Ok(())
}
