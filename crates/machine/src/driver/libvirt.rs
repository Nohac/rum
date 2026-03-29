use std::path::Path;
use std::sync::Arc;

use virt::connect::Connect;
use virt::domain::Domain;
use virt::error as virt_error;
use virt::network::Network;

use crate::config::SystemConfig;
use crate::error::Error;
use crate::layout::MachineLayout;
use crate::driver::VirtualMachine;
use crate::qcow2;
use crate::state::MachineState;
use crate::{cloudinit, image};

#[derive(Clone)]
pub struct LibvirtMachine {
    system: Arc<SystemConfig>,
    layout: MachineLayout,
}

impl LibvirtMachine {
    pub fn new(system: SystemConfig) -> Self {
        let layout = MachineLayout::from_config(&system);
        Self {
            system: Arc::new(system),
            layout,
        }
    }

    pub fn system(&self) -> &SystemConfig {
        &self.system
    }

    pub fn layout(&self) -> &MachineLayout {
        &self.layout
    }

    pub async fn ensure_image(&self, base_url: &str, cache_dir: &Path) -> Result<std::path::PathBuf, Error> {
        image::ensure_base_image(base_url, cache_dir).await
    }

    pub async fn ssh(&self, args: &[String]) -> Result<(), Error> {
        let vm_name = self.name();
        let conn = self.connect()?;

        let dom = Domain::lookup_by_name(&conn, vm_name).map_err(|_| Error::SshNotReady {
            name: vm_name.to_string(),
            reason: "VM is not defined".into(),
        })?;

        if !self.is_running(&dom) {
            return Err(Error::SshNotReady {
                name: vm_name.to_string(),
                reason: "VM is not running".into(),
            });
        }

        let ip = self.get_vm_ip(&dom)?;
        let ssh_key_path = &self.layout.ssh_key_path;

        if !ssh_key_path.exists() {
            return Err(Error::SshNotReady {
                name: vm_name.to_string(),
                reason: "SSH key not found (run `rum up` first)".into(),
            });
        }

        drop(conn);

        let ssh_config = &self.system.config.ssh;
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

    pub fn get_vsock_cid(&self) -> Result<u32, Error> {
        let vm_name = self.name();
        let conn = self.connect()?;

        let dom = Domain::lookup_by_name(&conn, vm_name).map_err(|_| Error::DomainNotFound {
            name: vm_name.to_string(),
        })?;

        if !self.is_running(&dom) {
            return Err(Error::ExecNotReady {
                name: vm_name.to_string(),
                reason: "VM is not running".into(),
            });
        }

        self.parse_vsock_cid(&dom).ok_or_else(|| Error::ExecNotReady {
            name: vm_name.to_string(),
            reason: "could not determine vsock CID from domain XML".into(),
        })
    }

    fn connect(&self) -> Result<Connect, Error> {
        virt_error::clear_error_callback();

        Connect::open(Some(self.system.libvirt_uri())).map_err(|e| Error::Libvirt {
            message: format!("failed to connect to libvirt: {e}"),
            hint: format!(
                "ensure libvirtd is running and you have access to {}",
                self.system.libvirt_uri()
            ),
        })
    }

    fn define_domain(&self, conn: &Connect, xml: &str) -> Result<Domain, Error> {
        Domain::define_xml(conn, xml).map_err(|e| Error::Libvirt {
            message: format!("failed to define domain: {e}"),
            hint: "check the generated domain XML for errors".into(),
        })
    }

    fn is_running(&self, dom: &Domain) -> bool {
        dom.is_active().unwrap_or(false)
    }

    async fn shutdown_domain(&self, dom: &Domain) -> Result<(), Error> {
        if !self.is_running(dom) {
            return Ok(());
        }
        dom.shutdown().map_err(|e| Error::Libvirt {
            message: format!("shutdown failed: {e}"),
            hint: "VM may not support ACPI shutdown".into(),
        })?;

        for _ in 0..10 {
            if !self.is_running(dom) {
                return Ok(());
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }

        dom.destroy().map_err(|e| Error::Libvirt {
            message: format!("force stop failed: {e}"),
            hint: "check libvirt permissions".into(),
        })?;
        Ok(())
    }

    fn parse_vsock_cid(&self, dom: &Domain) -> Option<u32> {
        let xml = dom.get_xml_desc(0).ok()?;
        domain::parse_vsock_cid(&xml)
    }

    fn ensure_network_active(&self, conn: &Connect, name: &str) -> Result<Network, Error> {
        let net = Network::lookup_by_name(conn, name).map_err(|_| Error::Libvirt {
            message: format!("network '{name}' not found"),
            hint: format!("define the network with `virsh net-define` and `virsh net-start {name}`"),
        })?;

        if !net.is_active().unwrap_or(false) {
            tracing::info!(name, "starting inactive network");
            net.create().map_err(|e| Error::Libvirt {
                message: format!("failed to start network '{name}': {e}"),
                hint: format!("try `sudo virsh net-start {name}`"),
            })?;
        }

        Ok(net)
    }

    fn ensure_extra_network(&self, conn: &Connect, name: &str, ip_hint: &str) -> Result<Network, Error> {
        match Network::lookup_by_name(conn, name) {
            Ok(net) => {
                if !net.is_active().unwrap_or(false) {
                    tracing::info!(name, "starting inactive network");
                    net.create().map_err(|e| Error::Libvirt {
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
                let net = Network::define_xml(conn, &xml).map_err(|e| Error::Libvirt {
                    message: format!("failed to define network '{name}': {e}"),
                    hint: "check libvirt permissions".into(),
                })?;
                net.create().map_err(|e| Error::Libvirt {
                    message: format!("failed to start network '{name}': {e}"),
                    hint: "check libvirt permissions".into(),
                })?;
                Ok(net)
            }
        }
    }

    fn add_dhcp_reservation(
        &self,
        net: &Network,
        net_name: &str,
        mac: &str,
        ip: &str,
        hostname: &str,
    ) -> Result<(), Error> {
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
                    .map_err(|e2| Error::Libvirt {
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

    fn ensure_networks(&self, conn: &Connect) -> Result<(), Error> {
        let config = &self.system.config;

        if config.network.nat {
            self.ensure_network_active(conn, "default")?;
        }

        for (i, iface) in config.network.interfaces.iter().enumerate() {
            let libvirt_name = domain::prefixed_name(&self.system.id, &iface.network);
            let net = self.ensure_extra_network(conn, &libvirt_name, &iface.ip)?;

            if !iface.ip.is_empty() {
                let mac = domain::generate_mac(self.name(), i);
                self.add_dhcp_reservation(&net, &libvirt_name, &mac, &iface.ip, self.system.hostname())?;
            }
        }

        Ok(())
    }

    fn get_vm_ip(&self, dom: &Domain) -> Result<String, Error> {
        let vm_name = self.name();
        let ifaces = dom
            .interface_addresses(virt::sys::VIR_DOMAIN_INTERFACE_ADDRESSES_SRC_LEASE, 0)
            .map_err(|_| Error::SshNotReady {
                name: vm_name.to_string(),
                reason: "could not query network interfaces".into(),
            })?;

        let ssh_interface = &self.system.config.ssh.interface;

        if ssh_interface.is_empty() {
            let extra_macs: Vec<String> = self
                .system
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
                    if addr.typed == 0 {
                        return Ok(addr.addr.clone());
                    }
                }
            }
        } else {
            let iface_idx = self
                .system
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
}

impl VirtualMachine for LibvirtMachine {
    type Error = Error;

    fn id(&self) -> &str {
        &self.system.id
    }

    fn name(&self) -> &str {
        self.system.display_name()
    }

    fn recover_state(&self) -> Result<MachineState, Error> {
        let config = &self.system.config;
        let mounts = self.system.resolve_mounts()?;
        let drives = self.system.resolve_drives()?;

        let ssh_keys = if self.layout.ssh_key_path.with_extension("pub").exists() {
            std::fs::read_to_string(self.layout.ssh_key_path.with_extension("pub"))
                .map(|k| vec![k.trim().to_string()])
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        let seed_config = cloudinit::SeedConfig {
            hostname: self.system.hostname(),
            user_name: &config.user.name,
            user_groups: &config.user.groups,
            mounts: &mounts,
            autologin: config.advanced.autologin,
            ssh_keys: &ssh_keys,
            agent_binary: Some(crate::agent_client::AGENT_BINARY),
        };
        let seed_hash = cloudinit::seed_hash(&seed_config);
        let seed_path = self.layout.seed_path(&seed_hash);

        let domain_config = domain::DomainConfig {
            id: self.system.id.clone(),
            name: self.system.display_name().to_string(),
            domain_type: config.advanced.domain_type.clone(),
            machine: config.advanced.machine.clone(),
            memory_mb: config.resources.memory_mb,
            cpus: config.resources.cpus,
            nat: config.network.nat,
            interfaces: config
                .network
                .interfaces
                .iter()
                .map(|iface| domain::InterfaceConfig {
                    network: iface.network.clone(),
                })
                .collect(),
        };
        let domain_mounts: Vec<domain::ResolvedMount> = mounts
            .iter()
            .map(|mount| domain::ResolvedMount {
                source: mount.source.clone(),
                target: mount.target.clone(),
                readonly: mount.readonly,
                tag: mount.tag.clone(),
            })
            .collect();
        let domain_drives: Vec<domain::ResolvedDrive> = drives
            .iter()
            .map(|drive| domain::ResolvedDrive {
                path: drive.path.clone(),
                dev: drive.dev.clone(),
            })
            .collect();

        let conn = self.connect()?;
        let domain = Domain::lookup_by_name(&conn, self.name()).ok();
        let running = domain.as_ref().is_some_and(|dom| dom.is_active().unwrap_or(false));

        let stale = running
            && domain::xml_has_changed(
                &domain_config,
                &self.layout.overlay_path,
                &seed_path,
                &domain_mounts,
                &domain_drives,
                &self.layout.xml_path,
            );

        let overlay_exists = self.layout.overlay_path.exists();
        let marker_exists = self.layout.provisioned_marker.exists();

        if running && stale {
            return Ok(MachineState::StaleConfig);
        }
        if running {
            return Ok(MachineState::Running);
        }
        if overlay_exists && marker_exists {
            return Ok(MachineState::Stopped);
        }
        if overlay_exists && domain.is_some() {
            return Ok(MachineState::PartialBoot);
        }
        if overlay_exists {
            return Ok(MachineState::Prepared);
        }
        if image::is_cached(&config.image.base, &crate::paths::cache_dir()) {
            return Ok(MachineState::ImageCached);
        }
        Ok(MachineState::Missing)
    }

    async fn prepare(&self, base_image: &Path) -> Result<(), Error> {
        let config = &self.system.config;

        let mounts = self.system.resolve_mounts()?;
        let drives = self.system.resolve_drives()?;

        ensure_ssh_keypair(&self.layout.ssh_key_path).await?;
        let ssh_keys =
            collect_ssh_keys(&self.layout.ssh_key_path, &config.ssh.authorized_keys).await?;

        let seed_config = cloudinit::SeedConfig {
            hostname: self.system.hostname(),
            user_name: &config.user.name,
            user_groups: &config.user.groups,
            mounts: &mounts,
            autologin: config.advanced.autologin,
            ssh_keys: &ssh_keys,
            agent_binary: Some(crate::agent_client::AGENT_BINARY),
        };
        let seed_hash = cloudinit::seed_hash(&seed_config);
        let seed_path = self.layout.seed_path(&seed_hash);

        let disk_size = crate::util::parse_size(&config.resources.disk)?;

        if !self.layout.overlay_path.exists() {
            qcow2::create_qcow2_overlay(&self.layout.overlay_path, base_image, Some(disk_size))?;
        }
        for drive in &drives {
            if !drive.path.exists() {
                qcow2::create_qcow2(&drive.path, &drive.size)?;
            }
        }

        if !seed_path.exists() {
            if let Ok(mut entries) = tokio::fs::read_dir(&self.layout.work_dir).await {
                while let Ok(Some(entry)) = entries.next_entry().await {
                    let file_name = entry.file_name();
                    if let Some(name) = file_name.to_str()
                        && name.starts_with("seed-")
                        && name.ends_with(".iso")
                    {
                        let _ = tokio::fs::remove_file(entry.path()).await;
                    }
                }
            }
            cloudinit::generate_seed_iso(&seed_path, &seed_config).await?;
        }

        let domain_config = domain::DomainConfig {
            id: self.system.id.clone(),
            name: self.name().to_string(),
            domain_type: config.advanced.domain_type.clone(),
            machine: config.advanced.machine.clone(),
            memory_mb: config.resources.memory_mb,
            cpus: config.resources.cpus,
            nat: config.network.nat,
            interfaces: config
                .network
                .interfaces
                .iter()
                .map(|iface| domain::InterfaceConfig {
                    network: iface.network.clone(),
                })
                .collect(),
        };
        let domain_mounts: Vec<domain::ResolvedMount> = mounts
            .iter()
            .map(|mount| domain::ResolvedMount {
                source: mount.source.clone(),
                target: mount.target.clone(),
                readonly: mount.readonly,
                tag: mount.tag.clone(),
            })
            .collect();
        let domain_drives: Vec<domain::ResolvedDrive> = drives
            .iter()
            .map(|drive| domain::ResolvedDrive {
                path: drive.path.clone(),
                dev: drive.dev.clone(),
            })
            .collect();

        let xml = domain::generate_domain_xml(
            &domain_config,
            &self.layout.overlay_path,
            &seed_path,
            &domain_mounts,
            &domain_drives,
        );
        let conn = self.connect()?;

        match Domain::lookup_by_name(&conn, self.name()) {
            Ok(dom) => {
                if domain::xml_has_changed(
                    &domain_config,
                    &self.layout.overlay_path,
                    &seed_path,
                    &domain_mounts,
                    &domain_drives,
                    &self.layout.xml_path,
                ) {
                    if self.is_running(&dom) {
                        return Err(Error::RequiresRestart {
                            name: self.name().to_string(),
                        });
                    }
                    dom.undefine().map_err(|e| Error::Libvirt {
                        message: format!("failed to undefine domain: {e}"),
                        hint: "check libvirt permissions".into(),
                    })?;
                    self.define_domain(&conn, &xml)?;
                    tracing::info!(vm_name = self.name(), "domain redefined with updated config");
                }
            }
            Err(_) => {
                self.define_domain(&conn, &xml)?;
                tracing::info!(vm_name = self.name(), "domain defined");
            }
        }

        tokio::fs::write(&self.layout.xml_path, &xml)
            .await
            .map_err(|e| Error::Io {
                context: format!("saving domain XML to {}", self.layout.xml_path.display()),
                source: e,
            })?;

        tokio::fs::write(
            &self.layout.config_path_file,
            self.system.config_path.to_string_lossy().as_bytes(),
        )
        .await
        .map_err(|e| Error::Io {
            context: format!("saving config path to {}", self.layout.config_path_file.display()),
            source: e,
        })?;

        self.ensure_networks(&conn)?;
        Ok(())
    }

    async fn boot(&self) -> Result<u32, Error> {
        let conn = self.connect()?;

        let dom = Domain::lookup_by_name(&conn, self.name()).map_err(|e| Error::Libvirt {
            message: format!("domain lookup failed: {e}"),
            hint: "domain should have been defined in prepare".into(),
        })?;

        if !self.is_running(&dom) {
            dom.create().map_err(|e| Error::Libvirt {
                message: format!("failed to start domain: {e}"),
                hint: "check `virsh -c qemu:///system start` for details".into(),
            })?;
            tracing::info!(vm_name = self.name(), "VM started");
        }

        self.parse_vsock_cid(&dom).ok_or_else(|| Error::Libvirt {
            message: "could not determine vsock CID from live XML".into(),
            hint: "ensure the domain XML includes a <vsock> device".into(),
        })
    }

    async fn shutdown(&self) -> Result<(), Error> {
        let conn = self.connect()?;

        let dom = Domain::lookup_by_name(&conn, self.name()).map_err(|e| Error::Libvirt {
            message: format!("domain lookup failed: {e}"),
            hint: "VM may not be defined".into(),
        })?;

        self.shutdown_domain(&dom).await
    }

    async fn destroy(&self) -> Result<(), Error> {
        let config = &self.system.config;
        virt_error::clear_error_callback();

        if let Ok(conn) = self.connect() {
            if let Ok(dom) = Domain::lookup_by_name(&conn, self.name()) {
                if dom.is_active().unwrap_or(false) {
                    let _ = dom.destroy();
                }
                let _ = dom.undefine();
            }

            for iface in &config.network.interfaces {
                let net_name = domain::prefixed_name(&self.system.id, &iface.network);
                if let Ok(net) = Network::lookup_by_name(&conn, &net_name) {
                    if net.is_active().unwrap_or(false) {
                        let _ = net.destroy();
                    }
                    let _ = net.undefine();
                }
            }
        }

        if self.layout.work_dir.exists() {
            tokio::fs::remove_dir_all(&self.layout.work_dir)
                .await
                .map_err(|e| Error::Io {
                    context: format!("removing {}", self.layout.work_dir.display()),
                    source: e,
                })?;
        }

        Ok(())
    }
}

async fn ensure_ssh_keypair(key_path: &Path) -> Result<(), Error> {
    if key_path.exists() {
        return Ok(());
    }

    if let Some(parent) = key_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| Error::Io {
                context: format!("creating directory {}", parent.display()),
                source: e,
            })?;
    }

    let keypair = ssh_key::private::Ed25519Keypair::random(&mut rand_core::OsRng);
    let private = ssh_key::PrivateKey::from(keypair);

    let openssh_private = private
        .to_openssh(ssh_key::LineEnding::LF)
        .map_err(|e| Error::Io {
            context: format!("encoding SSH private key: {e}"),
            source: std::io::Error::other(e.to_string()),
        })?;
    tokio::fs::write(key_path, openssh_private.as_bytes())
        .await
        .map_err(|e| Error::Io {
            context: format!("writing SSH key to {}", key_path.display()),
            source: e,
        })?;

    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(key_path, std::fs::Permissions::from_mode(0o600))
            .await
            .map_err(|e| Error::Io {
                context: format!("setting permissions on {}", key_path.display()),
                source: e,
            })?;
    }

    let pub_key = private.public_key().to_openssh().map_err(|e| Error::Io {
        context: format!("encoding SSH public key: {e}"),
        source: std::io::Error::other(e.to_string()),
    })?;
    let pub_path = key_path.with_extension("pub");
    tokio::fs::write(&pub_path, pub_key.as_bytes())
        .await
        .map_err(|e| Error::Io {
            context: format!("writing SSH public key to {}", pub_path.display()),
            source: e,
        })?;

    tracing::info!(path = %key_path.display(), "generated SSH keypair");
    Ok(())
}

async fn collect_ssh_keys(key_path: &Path, extra_keys: &[String]) -> Result<Vec<String>, Error> {
    let pub_path = key_path.with_extension("pub");
    let auto_pub = tokio::fs::read_to_string(&pub_path)
        .await
        .map_err(|e| Error::Io {
            context: format!("reading SSH public key from {}", pub_path.display()),
            source: e,
        })?;
    let mut keys = vec![auto_pub.trim().to_string()];
    keys.extend(extra_keys.iter().cloned());
    Ok(keys)
}
