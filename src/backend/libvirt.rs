
use indicatif::ProgressBar;
use virt::connect::Connect;
use virt::domain::Domain;
use virt::error as virt_error;
use virt::network::Network;

use crate::config::SystemConfig;
use crate::error::RumError;
use crate::{cloudinit, domain_xml, image, network_xml, overlay, paths, qcow2};

use ssh_key::private::Ed25519Keypair;
use ssh_key::PrivateKey;

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

pub struct LibvirtBackend;

impl super::Backend for LibvirtBackend {
    async fn up(&self, sys_config: &SystemConfig, reset: bool) -> Result<(), RumError> {
        let id = &sys_config.id;
        let name_opt = sys_config.name.as_deref();
        let vm_name = sys_config.display_name();
        let config = &sys_config.config;
        let work = paths::work_dir(id, name_opt);
        let overlay_path = paths::overlay_path(id, name_opt);

        // Resolve mounts and drives early so we fail fast on bad config
        let mounts = sys_config.resolve_mounts()?;
        if !mounts.is_empty() {
            for m in &mounts {
                tracing::info!(
                    source = %m.source.display(),
                    target = %m.target,
                    tag = %m.tag,
                    readonly = m.readonly,
                    "virtiofs mount"
                );
            }
        }

        let drives = sys_config.resolve_drives()?;
        if !drives.is_empty() {
            for d in &drives {
                tracing::info!(
                    name = %d.name,
                    size = %d.size,
                    dev = %d.dev,
                    "extra drive"
                );
            }
        }

        let resolved_fs = sys_config.resolve_fs(&drives)?;

        // Generate SSH keypair in work dir if absent, collect all authorized keys
        let ssh_key_path = paths::ssh_key_path(id, name_opt);
        ensure_ssh_keypair(&ssh_key_path).await?;
        let ssh_keys = collect_ssh_keys(&ssh_key_path, &config.ssh.authorized_keys).await?;

        let agent_binary = &*crate::agent::AGENT_BINARY;

        let seed_hash = cloudinit::seed_hash(
            sys_config.hostname(),
            config.provision.system.as_ref().map(|s| s.script.as_str()),
            config.provision.boot.as_ref().map(|s| s.script.as_str()),
            &mounts,
            &drives,
            &resolved_fs,
            config.advanced.autologin,
            &ssh_keys,
            Some(agent_binary.as_slice()),
        );
        let seed_path = paths::seed_path(id, name_opt, &seed_hash);
        let xml_path = paths::domain_xml_path(id, name_opt);
        let cache = paths::cache_dir();

        let conn = connect(sys_config)?;

        // --reset: stop, undefine, wipe artifacts
        if reset {
            tracing::info!(vm_name, "resetting VM");
            if let Ok(dom) = Domain::lookup_by_name(&conn, vm_name) {
                let _ = shutdown_domain(&dom).await;
                let _ = dom.undefine();
            }
            let _ = tokio::fs::remove_dir_all(&work).await;
        }

        // 1. Ensure base image
        println!("Ensuring base image...");
        let base = image::ensure_base_image(&config.image.base, &cache).await?;

        // 2. Create overlay if absent
        if !overlay_path.exists() {
            println!("Creating disk overlay...");
            overlay::create_overlay(&base, &overlay_path).await?;
        }

        // 2b. Create extra drive images if absent
        for d in &drives {
            if !d.path.exists() {
                println!("Creating drive '{}'...", d.name);
                qcow2::create_qcow2(&d.path, &d.size)?;
            }
        }

        // 3. Generate seed ISO if inputs changed (hash-keyed filename)
        if !seed_path.exists() {
            println!("Generating cloud-init seed...");
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
            cloudinit::generate_seed_iso(
                &seed_path,
                sys_config.hostname(),
                config.provision.system.as_ref().map(|s| s.script.as_str()),
                config.provision.boot.as_ref().map(|s| s.script.as_str()),
                &mounts,
                &resolved_fs,
                config.advanced.autologin,
                &ssh_keys,
                Some(agent_binary.as_slice()),
            )
            .await?;
        }

        // 4. Generate domain XML
        let xml = domain_xml::generate_domain_xml(
            sys_config,
            &overlay_path,
            &seed_path,
            &mounts,
            &drives,
        );

        // 5. Define or redefine domain
        println!("Configuring VM...");
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

        // 6. Ensure networks are active
        println!("Checking network...");
        ensure_networks(&conn, sys_config)?;

        // 7. Start if not running
        let dom = Domain::lookup_by_name(&conn, vm_name).map_err(|e| RumError::Libvirt {
            message: format!("domain lookup failed: {e}"),
            hint: "domain should have been defined above".into(),
        })?;

        if !is_running(&dom) {
            println!("Starting VM...");
            dom.create().map_err(|e| RumError::Libvirt {
                message: format!("failed to start domain: {e}"),
                hint: "check `virsh -c qemu:///system start` for details".into(),
            })?;
            tracing::info!(vm_name, "VM started");
        } else {
            tracing::info!(vm_name, "VM already running");
        }

        // 7b. Wait for agent readiness over vsock
        if let Some(cid) = parse_vsock_cid(&dom) {
            println!("Waiting for agent...");
            crate::agent::wait_for_agent(cid).await?;
        } else {
            tracing::warn!("could not determine vsock CID from live XML");
        }

        // 8. Start inotify bridge in the background (non-blocking)
        // The bridge task waits for the VM IP internally via virsh subprocess,
        // so we don't hold any libvirt handles and can drop conn immediately.
        let watch_handle = if mounts.iter().any(|m| m.inotify && !m.readonly) {
            Some(crate::watch::start_inotify_bridge(
                &mounts,
                sys_config.libvirt_uri().to_string(),
                vm_name.to_string(),
                config.ssh.user.clone(),
                ssh_key_path.clone(),
            ))
        } else {
            None
        };

        drop(conn);

        // 9. Wait for Ctrl+C (no console attach — keeps log output clean)
        println!("VM is running. Press Ctrl+C to stop...");
        tokio::signal::ctrl_c().await.ok();

        // Stop inotify bridge on console detach
        if let Some(handle) = watch_handle {
            handle.abort();
        }

        Ok(())
    }

    async fn down(&self, sys_config: &SystemConfig) -> Result<(), RumError> {
        let vm_name = sys_config.display_name();
        let conn = connect(sys_config)?;

        let dom = Domain::lookup_by_name(&conn, vm_name).map_err(|_| RumError::DomainNotFound {
            name: vm_name.to_string(),
        })?;

        if !is_running(&dom) {
            println!("VM '{vm_name}' is not running.");
            return Ok(());
        }

        // ACPI shutdown
        tracing::info!(vm_name, "sending ACPI shutdown");
        dom.shutdown().map_err(|e| RumError::Libvirt {
            message: format!("shutdown failed: {e}"),
            hint: "VM may not have ACPI support".into(),
        })?;

        // Wait up to 30s for shutdown
        let spinner = ProgressBar::new_spinner();
        spinner.set_message(format!("Waiting for VM '{vm_name}' to shut down..."));
        spinner.enable_steady_tick(std::time::Duration::from_millis(120));
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            if !is_running(&dom) {
                spinner.finish_with_message(format!("VM '{vm_name}' stopped."));
                return Ok(());
            }
            if tokio::time::Instant::now() >= deadline {
                spinner.finish_and_clear();
                break;
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            spinner.tick();
        }

        // Force stop
        tracing::warn!(vm_name, "ACPI shutdown timed out, force stopping");
        dom.destroy().map_err(|e| RumError::Libvirt {
            message: format!("force stop failed: {e}"),
            hint: "check libvirt permissions".into(),
        })?;
        println!("VM '{vm_name}' force stopped.");

        Ok(())
    }

    async fn destroy(&self, sys_config: &SystemConfig, purge: bool) -> Result<(), RumError> {
        let id = &sys_config.id;
        let name_opt = sys_config.name.as_deref();
        let vm_name = sys_config.display_name();
        let config = &sys_config.config;
        let conn = connect(sys_config)?;

        if let Ok(dom) = Domain::lookup_by_name(&conn, vm_name) {
            if is_running(&dom) {
                tracing::info!(vm_name, "stopping VM before destroy");
                let _ = dom.destroy();
            }
            dom.undefine().map_err(|e| RumError::Libvirt {
                message: format!("failed to undefine domain: {e}"),
                hint: "check libvirt permissions".into(),
            })?;
            tracing::info!(vm_name, "domain undefined");
        }

        // Tear down auto-created networks (derived from id + interface names)
        for iface in &config.network.interfaces {
            let net_name = network_xml::prefixed_name(id, &iface.network);
            if let Ok(net) = Network::lookup_by_name(&conn, &net_name) {
                if net.is_active().unwrap_or(false) {
                    let _ = net.destroy();
                }
                let _ = net.undefine();
                tracing::info!(net_name, "removed network");
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
            tracing::info!(path = %work.display(), "removed work directory");
        }

        // Purge: remove cached base image
        if purge && let Some(filename) = config.image.base.rsplit('/').next() {
            let cached = paths::cache_dir().join(filename);
            if cached.exists() {
                tokio::fs::remove_file(&cached)
                    .await
                    .map_err(|e| RumError::Io {
                        context: format!("removing cached image {}", cached.display()),
                        source: e,
                    })?;
                tracing::info!(path = %cached.display(), "removed cached base image");
            }
        }

        println!("VM '{vm_name}' destroyed.");
        Ok(())
    }

    async fn status(&self, sys_config: &SystemConfig) -> Result<(), RumError> {
        let vm_name = sys_config.display_name();
        let conn = connect(sys_config)?;

        match Domain::lookup_by_name(&conn, vm_name) {
            Ok(dom) => {
                let state = if is_running(&dom) {
                    "running"
                } else {
                    "stopped"
                };
                println!("VM '{vm_name}': {state}");

                if is_running(&dom) {
                    // Try to get IP from DHCP leases
                    match dom
                        .interface_addresses(virt::sys::VIR_DOMAIN_INTERFACE_ADDRESSES_SRC_LEASE, 0)
                    {
                        Ok(ifaces) => {
                            for iface in &ifaces {
                                for addr in &iface.addrs {
                                    println!("  IP: {}", addr.addr);
                                }
                            }
                        }
                        Err(_) => {
                            println!("  IP: unknown (DHCP lease not yet available)");
                        }
                    }
                }
            }
            Err(_) => {
                println!("VM '{vm_name}': not defined");
            }
        }

        Ok(())
    }

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

    async fn ssh_config(&self, sys_config: &SystemConfig) -> Result<(), RumError> {
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

        println!(
            "Host {vm_name}\n  \
             HostName {ip}\n  \
             User {user}\n  \
             IdentityFile {key}\n  \
             StrictHostKeyChecking no\n  \
             UserKnownHostsFile /dev/null\n  \
             LogLevel ERROR",
            user = sys_config.config.ssh.user,
            key = ssh_key_path.display(),
        );

        Ok(())
    }
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

/// Generate an Ed25519 SSH keypair at `key_path` (+ `.pub`) if it doesn't exist.
/// Sets 0600 permissions on the private key so OpenSSH accepts it.
async fn ensure_ssh_keypair(key_path: &std::path::Path) -> Result<(), RumError> {
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

    // OpenSSH refuses keys with open permissions — must be 0600
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
    key_path: &std::path::Path,
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

fn connect(sys_config: &SystemConfig) -> Result<ConnGuard, RumError> {
    // Suppress libvirt's default error handler that prints to stderr.
    // This installs a no-op callback so errors are only surfaced through
    // Rust's Result types, not printed to stderr by the C library.
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

/// Ensure an extra (non-NAT) network exists and is active.
/// Auto-creates it as a host-only network if it doesn't exist.
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
            // Auto-create a host-only network
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
            let mac = crate::domain_xml::generate_mac(sys_config.display_name(), i);
            add_dhcp_reservation(&net, &libvirt_name, &mac, &iface.ip, sys_config.hostname())?;
        }
    }

    Ok(())
}

/// Add or update a DHCP host reservation in a libvirt network.
fn add_dhcp_reservation(
    net: &Network,
    net_name: &str,
    mac: &str,
    ip: &str,
    hostname: &str,
) -> Result<(), RumError> {
    let host_xml = format!("<host mac='{mac}' name='{hostname}' ip='{ip}'/>",);

    // Try to update existing reservation first (modify), fall back to add
    let modify = virt::sys::VIR_NETWORK_UPDATE_COMMAND_ADD_LAST;
    let section = virt::sys::VIR_NETWORK_SECTION_IP_DHCP_HOST;
    let flags =
        virt::sys::VIR_NETWORK_UPDATE_AFFECT_LIVE | virt::sys::VIR_NETWORK_UPDATE_AFFECT_CONFIG;

    match net.update(modify, section, -1, &host_xml, flags) {
        Ok(_) => {
            tracing::info!(net_name, mac, ip, "added DHCP reservation");
        }
        Err(e) => {
            // If add fails (entry may already exist), try modify
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

fn is_running(dom: &Domain) -> bool {
    dom.is_active().unwrap_or(false)
}

async fn shutdown_domain(dom: &Domain) -> Result<(), RumError> {
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

/// Extract the auto-assigned vsock CID from the live domain XML.
///
/// After `dom.create()`, libvirt fills in the CID. The live XML contains
/// something like `<cid auto="yes" address="3"/>` inside `<vsock>`.
fn parse_vsock_cid(dom: &Domain) -> Option<u32> {
    let xml = dom.get_xml_desc(0).ok()?;

    // Find the <vsock section, then locate address='N' within it
    let vsock_start = xml.find("<vsock")?;
    let vsock_end = xml[vsock_start..].find("</vsock>").map(|i| vsock_start + i)?;
    let vsock_section = &xml[vsock_start..vsock_end];

    // Look for address="N" or address='N'
    let addr_prefix = "address=\"";
    let addr_start = vsock_section.find(addr_prefix).map(|i| i + addr_prefix.len())
        .or_else(|| {
            let alt = "address='";
            vsock_section.find(alt).map(|i| i + alt.len())
        })?;

    let remaining = &vsock_section[addr_start..];
    let addr_end = remaining.find(['"', '\''])?;
    remaining[..addr_end].parse::<u32>().ok()
}
