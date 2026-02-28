//! Standalone worker functions for VM lifecycle operations.
//!
//! Each function is a self-contained async operation with clean inputs and
//! outputs. These are called by the event loop's `make_worker()` to execute
//! effects. No progress/UI coupling — that's the observer's job.

use std::path::{Path, PathBuf};

use ssh_key::private::Ed25519Keypair;
use ssh_key::PrivateKey;
use virt::connect::Connect;
use virt::domain::Domain;
use virt::error as virt_error;
use virt::network::Network;

use crate::agent::AgentClient;
use crate::config::SystemConfig;
use crate::error::RumError;
use crate::progress::{OutputMode, StepProgress};
use crate::{cloudinit, domain_xml, image, network_xml, overlay, paths, qcow2};

/// Download or verify the base image. Returns path to cached image.
pub async fn ensure_image(
    base_url: &str,
    cache_dir: &Path,
) -> Result<PathBuf, RumError> {
    image::ensure_base_image(base_url, cache_dir).await
}

/// Create overlay, extra drives, seed ISO, domain XML, define domain,
/// ensure networks. Full artifact preparation.
pub async fn prepare_vm(
    sys_config: &SystemConfig,
    base_image: &Path,
) -> Result<(), RumError> {
    let id = &sys_config.id;
    let name_opt = sys_config.name.as_deref();
    let vm_name = sys_config.display_name();
    let config = &sys_config.config;
    let work = paths::work_dir(id, name_opt);
    let overlay_path = paths::overlay_path(id, name_opt);

    // Resolve mounts and drives early so we fail fast on bad config
    let mounts = sys_config.resolve_mounts()?;
    let drives = sys_config.resolve_drives()?;

    // Generate SSH keypair in work dir if absent, collect all authorized keys
    let ssh_key_path = paths::ssh_key_path(id, name_opt);
    ensure_ssh_keypair(&ssh_key_path).await?;
    let ssh_keys = collect_ssh_keys(&ssh_key_path, &config.ssh.authorized_keys).await?;

    let agent_binary = crate::agent::AGENT_BINARY;

    let seed_config = cloudinit::SeedConfig {
        hostname: sys_config.hostname(),
        user_name: &config.user.name,
        user_groups: &config.user.groups,
        mounts: &mounts,
        autologin: config.advanced.autologin,
        ssh_keys: &ssh_keys,
        agent_binary: Some(agent_binary),
    };
    let seed_hash = cloudinit::seed_hash(&seed_config);
    let seed_path = paths::seed_path(id, name_opt, &seed_hash);
    let xml_path = paths::domain_xml_path(id, name_opt);

    let disk_size = crate::util::parse_size(&config.resources.disk)?;

    // Create overlay + extra drives
    if !overlay_path.exists() {
        overlay::create_overlay(base_image, &overlay_path, Some(disk_size)).await?;
    }
    for d in &drives {
        if !d.path.exists() {
            qcow2::create_qcow2(&d.path, &d.size)?;
        }
    }

    // Generate seed ISO if inputs changed
    if !seed_path.exists() {
        // Remove old seed ISOs with different hashes
        if let Ok(mut entries) = tokio::fs::read_dir(&work).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let fname = entry.file_name();
                if let Some(s) = fname.to_str()
                    && s.starts_with("seed-")
                    && s.ends_with(".iso")
                {
                    let _ = tokio::fs::remove_file(entry.path()).await;
                }
            }
        }
        cloudinit::generate_seed_iso(&seed_path, &seed_config).await?;
    }

    // Configure domain + network
    let xml = domain_xml::generate_domain_xml(
        sys_config,
        &overlay_path,
        &seed_path,
        &mounts,
        &drives,
    );

    let conn = connect(sys_config)?;

    let existing = Domain::lookup_by_name(&conn, vm_name);
    match existing {
        Ok(dom) => {
            if domain_xml::xml_has_changed(
                sys_config,
                &overlay_path,
                &seed_path,
                &mounts,
                &drives,
                &xml_path,
            ) {
                if is_running(&dom) {
                    return Err(RumError::RequiresRestart {
                        name: vm_name.to_string(),
                    });
                }
                dom.undefine().map_err(|e| RumError::Libvirt {
                    message: format!("failed to undefine domain: {e}"),
                    hint: "check libvirt permissions".into(),
                })?;
                define_domain(&conn, &xml)?;
                tracing::info!(vm_name, "domain redefined with updated config");
            }
        }
        Err(_) => {
            define_domain(&conn, &xml)?;
            tracing::info!(vm_name, "domain defined");
        }
    }

    // Save XML for future change detection
    tokio::fs::write(&xml_path, &xml)
        .await
        .map_err(|e| RumError::Io {
            context: format!("saving domain XML to {}", xml_path.display()),
            source: e,
        })?;

    // Write config_path file for stale state detection
    let cp_file = paths::config_path_file(id, name_opt);
    tokio::fs::write(
        &cp_file,
        sys_config.config_path.to_string_lossy().as_bytes(),
    )
    .await
    .map_err(|e| RumError::Io {
        context: format!("saving config path to {}", cp_file.display()),
        source: e,
    })?;

    ensure_networks(&conn, sys_config)?;
    Ok(())
}

/// Start the libvirt domain. Returns vsock CID.
pub async fn boot_vm(
    sys_config: &SystemConfig,
) -> Result<u32, RumError> {
    let vm_name = sys_config.display_name();
    let conn = connect(sys_config)?;

    let dom = Domain::lookup_by_name(&conn, vm_name).map_err(|e| RumError::Libvirt {
        message: format!("domain lookup failed: {e}"),
        hint: "domain should have been defined in prepare_vm".into(),
    })?;

    if !is_running(&dom) {
        dom.create().map_err(|e| RumError::Libvirt {
            message: format!("failed to start domain: {e}"),
            hint: "check `virsh -c qemu:///system start` for details".into(),
        })?;
        tracing::info!(vm_name, "VM started");
    }

    let cid = parse_vsock_cid(&dom).ok_or_else(|| RumError::Libvirt {
        message: "could not determine vsock CID from live XML".into(),
        hint: "ensure the domain XML includes a <vsock> device".into(),
    })?;

    Ok(cid)
}

/// Wait for guest agent to become reachable.
pub async fn connect_agent(cid: u32) -> Result<AgentClient, RumError> {
    crate::agent::wait_for_agent(cid).await
}

/// Run provision scripts via the guest agent.
///
/// Creates a hidden (Quiet) StepProgress internally so that the agent module's
/// `run_provision` has the `&mut StepProgress` it requires, without coupling
/// workers to the UI.
pub async fn run_provision(
    agent: &AgentClient,
    scripts: Vec<rum_agent::ProvisionScript>,
    logs_dir: &Path,
) -> Result<(), RumError> {
    let mut progress = StepProgress::new(scripts.len(), OutputMode::Quiet);
    crate::agent::run_provision(agent, scripts, &mut progress, logs_dir).await
}

/// ACPI shutdown with timeout, force-destroy fallback.
pub async fn shutdown_vm(
    sys_config: &SystemConfig,
) -> Result<(), RumError> {
    let vm_name = sys_config.display_name();
    let conn = connect(sys_config)?;

    let dom = Domain::lookup_by_name(&conn, vm_name).map_err(|e| RumError::Libvirt {
        message: format!("domain lookup failed: {e}"),
        hint: "VM may not be defined".into(),
    })?;

    crate::backend::libvirt::shutdown_domain(&dom).await
}

/// Force-destroy domain if running, undefine, remove artifacts.
pub async fn destroy_vm(
    sys_config: &SystemConfig,
) -> Result<(), RumError> {
    let id = &sys_config.id;
    let name_opt = sys_config.name.as_deref();
    let vm_name = sys_config.display_name();
    let config = &sys_config.config;

    virt_error::clear_error_callback();

    if let Ok(conn) = Connect::open(Some(sys_config.libvirt_uri())).map(ConnGuard) {
        if let Ok(dom) = Domain::lookup_by_name(&conn, vm_name) {
            if dom.is_active().unwrap_or(false) {
                let _ = dom.destroy();
            }
            let _ = dom.undefine();
        }

        // Tear down auto-created networks
        for iface in &config.network.interfaces {
            let net_name = network_xml::prefixed_name(id, &iface.network);
            if let Ok(net) = Network::lookup_by_name(&conn, &net_name) {
                if net.is_active().unwrap_or(false) {
                    let _ = net.destroy();
                }
                let _ = net.undefine();
            }
        }
    }

    // Remove work dir
    let work = paths::work_dir(id, name_opt);
    if work.exists() {
        tokio::fs::remove_dir_all(&work)
            .await
            .map_err(|e| RumError::Io {
                context: format!("removing {}", work.display()),
                source: e,
            })?;
    }

    Ok(())
}

/// Start log subscription + port forwards. Returns handles.
pub(crate) async fn start_services(
    cid: u32,
    sys_config: &SystemConfig,
) -> Result<crate::daemon::ServiceHandles, RumError> {
    let config = &sys_config.config;

    // Connect to agent via vsock
    let agent_client = crate::agent::wait_for_agent(cid).await.ok();

    // Log subscription
    let log_handle = agent_client
        .as_ref()
        .map(crate::agent::start_log_subscription);

    // Port forwards
    let forward_handles = if !config.ports.is_empty() {
        crate::agent::start_port_forwards(cid, &config.ports).await?
    } else {
        Vec::new()
    };

    Ok(crate::daemon::ServiceHandles {
        log_handle,
        forward_handles,
    })
}

// ── Private helpers (duplicated from backend/libvirt.rs) ────────────

/// RAII guard that closes the libvirt connection on drop.
struct ConnGuard(Connect);

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

fn connect(sys_config: &SystemConfig) -> Result<ConnGuard, RumError> {
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

fn define_domain(conn: &Connect, xml: &str) -> Result<Domain, RumError> {
    Domain::define_xml(conn, xml).map_err(|e| RumError::Libvirt {
        message: format!("failed to define domain: {e}"),
        hint: "check the generated domain XML for errors".into(),
    })
}

fn is_running(dom: &Domain) -> bool {
    dom.is_active().unwrap_or(false)
}

/// Extract the auto-assigned vsock CID from a running domain's live XML.
fn parse_vsock_cid(dom: &Domain) -> Option<u32> {
    let xml = dom.get_xml_desc(0).ok()?;
    domain_xml::parse_vsock_cid(&xml)
}

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
            let subnet = network_xml::derive_subnet(name, ip_hint);
            let xml = network_xml::generate_network_xml(name, &subnet);
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

fn ensure_networks(conn: &Connect, sys_config: &SystemConfig) -> Result<(), RumError> {
    let config = &sys_config.config;

    if config.network.nat {
        ensure_network_active(conn, "default")?;
    }

    for (i, iface) in config.network.interfaces.iter().enumerate() {
        let libvirt_name = network_xml::prefixed_name(&sys_config.id, &iface.network);
        let net = ensure_extra_network(conn, &libvirt_name, &iface.ip)?;

        if !iface.ip.is_empty() {
            let mac = domain_xml::generate_mac(sys_config.display_name(), i);
            add_dhcp_reservation(&net, &libvirt_name, &mac, &iface.ip, sys_config.hostname())?;
        }
    }

    Ok(())
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

/// Generate an Ed25519 SSH keypair at `key_path` (+ `.pub`) if it doesn't exist.
async fn ensure_ssh_keypair(key_path: &Path) -> Result<(), RumError> {
    if key_path.exists() {
        return Ok(());
    }

    if let Some(parent) = key_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| RumError::Io {
                context: format!("creating directory {}", parent.display()),
                source: e,
            })?;
    }

    let keypair = Ed25519Keypair::random(&mut rand_core::OsRng);
    let private = PrivateKey::from(keypair);

    let openssh_private = private
        .to_openssh(ssh_key::LineEnding::LF)
        .map_err(|e| RumError::Io {
            context: format!("encoding SSH private key: {e}"),
            source: std::io::Error::other(e.to_string()),
        })?;
    tokio::fs::write(key_path, openssh_private.as_bytes())
        .await
        .map_err(|e| RumError::Io {
            context: format!("writing SSH key to {}", key_path.display()),
            source: e,
        })?;

    // OpenSSH refuses keys with open permissions
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(key_path, std::fs::Permissions::from_mode(0o600))
            .await
            .map_err(|e| RumError::Io {
                context: format!("setting permissions on {}", key_path.display()),
                source: e,
            })?;
    }

    let pub_key = private.public_key().to_openssh().map_err(|e| RumError::Io {
        context: format!("encoding SSH public key: {e}"),
        source: std::io::Error::other(e.to_string()),
    })?;
    let pub_path = key_path.with_extension("pub");
    tokio::fs::write(&pub_path, pub_key.as_bytes())
        .await
        .map_err(|e| RumError::Io {
            context: format!("writing SSH public key to {}", pub_path.display()),
            source: e,
        })?;

    tracing::info!(path = %key_path.display(), "generated SSH keypair");
    Ok(())
}

/// Read the auto-generated public key and combine with any config-specified keys.
async fn collect_ssh_keys(
    key_path: &Path,
    extra_keys: &[String],
) -> Result<Vec<String>, RumError> {
    let pub_path = key_path.with_extension("pub");
    let auto_pub = tokio::fs::read_to_string(&pub_path)
        .await
        .map_err(|e| RumError::Io {
            context: format!("reading SSH public key from {}", pub_path.display()),
            source: e,
        })?;
    let mut keys = vec![auto_pub.trim().to_string()];
    keys.extend(extra_keys.iter().cloned());
    Ok(keys)
}
