use virt::connect::Connect;
use virt::domain::Domain;
use virt::error as virt_error;

use crate::config::SystemConfig;
use crate::driver::LibvirtDriver;
use crate::error::Error;
use crate::layout::MachineLayout;
use crate::{cloudinit, image, paths};

/// Recoverable lifecycle state for one persisted runtime instance.
///
/// The state is instance-owned rather than driver-owned because it is derived
/// from both backend state and on-disk artifacts such as overlays, seed ISOs,
/// and provision markers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstanceState {
    Missing,
    ImageCached,
    Prepared,
    PartialBoot,
    Stopped,
    Running,
    StaleConfig,
}

/// Persistent instance view used by higher-level orchestration.
///
/// `Instance` is the boundary between orchestration and backend operations:
/// it owns recovered state and persisted identity, while the contained driver
/// performs backend-specific actions.
#[derive(Clone)]
pub struct Instance {
    driver: LibvirtDriver,
}

impl Instance {
    /// Create an instance view for the given system config.
    ///
    /// This does not mutate backend state. It prepares the persistent identity
    /// and backend handle needed for recovery and later operations.
    pub fn new(system: SystemConfig) -> Self {
        Self {
            driver: LibvirtDriver::new(system),
        }
    }

    /// Return a clonable backend driver for this instance.
    pub fn driver(&self) -> LibvirtDriver {
        self.driver.clone()
    }

    /// Access the resolved system config for this instance.
    pub fn system(&self) -> &SystemConfig {
        self.driver.system()
    }

    /// Access the derived on-disk layout for this instance.
    pub fn layout(&self) -> &MachineLayout {
        self.driver.layout()
    }

    /// Recover the current lifecycle state from libvirt and persisted artifacts.
    ///
    /// This is intentionally separate from the driver trait: the driver owns
    /// operational backend methods, while the instance owns recovery from
    /// combined backend and local persisted state.
    pub fn recover(&self) -> Result<InstanceState, Error> {
        let system = self.system();
        let layout = self.layout();
        let config = &system.config;
        let mounts = system.resolve_mounts()?;
        let drives = system.resolve_drives()?;

        let ssh_keys = if layout.ssh_key_path.with_extension("pub").exists() {
            std::fs::read_to_string(layout.ssh_key_path.with_extension("pub"))
                .map(|k| vec![k.trim().to_string()])
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        let seed_config = cloudinit::SeedConfig {
            hostname: system.hostname(),
            user_name: &config.user.name,
            user_groups: &config.user.groups,
            mounts: &mounts,
            autologin: config.advanced.autologin,
            ssh_keys: &ssh_keys,
            agent_binary: Some(crate::guest::AGENT_BINARY),
        };
        let seed_hash = cloudinit::seed_hash(&seed_config);
        let seed_path = layout.seed_path(&seed_hash);

        let domain_config = domain::DomainConfig {
            id: system.id.clone(),
            name: system.display_name().to_string(),
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

        virt_error::clear_error_callback();
        let conn = Connect::open(Some(system.libvirt_uri())).map_err(|e| Error::Libvirt {
            message: format!("failed to connect to libvirt: {e}"),
            hint: format!(
                "ensure libvirtd is running and you have access to {}",
                system.libvirt_uri()
            ),
        })?;
        let domain = Domain::lookup_by_name(&conn, system.display_name()).ok();
        let running = domain.as_ref().is_some_and(|dom| dom.is_active().unwrap_or(false));

        let stale = running
            && domain::xml_has_changed(
                &domain_config,
                &layout.overlay_path,
                &seed_path,
                &domain_mounts,
                &domain_drives,
                &layout.xml_path,
            );

        let overlay_exists = layout.overlay_path.exists();
        let marker_exists = layout.provisioned_marker.exists();

        let image_cached = image::is_cached(&config.image.base, &paths::cache_dir());

        // The match order encodes precedence between mutually overlapping facts.
        // For example, a running domain with stale XML is considered
        // `StaleConfig`, not merely `Running`.
        let state = match (running, stale, overlay_exists, marker_exists, domain.is_some(), image_cached) {
            (true, true, _, _, _, _) => InstanceState::StaleConfig,
            (true, false, _, _, _, _) => InstanceState::Running,
            (false, _, true, true, _, _) => InstanceState::Stopped,
            (false, _, true, false, true, _) => InstanceState::PartialBoot,
            (false, _, true, false, false, _) => InstanceState::Prepared,
            (false, _, false, _, _, true) => InstanceState::ImageCached,
            (false, _, false, _, _, false) => InstanceState::Missing,
        };

        Ok(state)
    }
}
