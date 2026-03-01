//! Shared vocabulary for VM lifecycle state.
//!
//! `VmState` is the single source of truth for where a VM is in its lifecycle.
//! Reconstructed from artifacts + libvirt on every invocation, then tracked
//! in memory by the event loop.

use virt::connect::Connect;
use virt::domain::Domain;

use crate::config::SystemConfig;
use crate::{domain_xml, paths};

/// The persisted VM lifecycle state — what has been done so far.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmState {
    /// Nothing exists. Clean slate.
    Virgin,

    /// Base image is cached (shared across VMs).
    /// Overlay/seed/domain do not exist for this VM.
    ImageCached,

    /// Artifacts created: overlay, seed ISO, domain defined in libvirt.
    /// VM has never been started (or was destroyed and re-prepared).
    Prepared,

    /// VM was started but provisioning never completed.
    /// The .provisioned marker does not exist.
    PartialBoot,

    /// VM is defined, has been fully provisioned at least once, but is
    /// currently stopped (e.g. after `rum down`).
    Provisioned,

    /// VM is running and reachable.
    Running,

    /// VM is running but config has changed since the domain was defined.
    /// Most mutations are blocked until resolved (restart or destroy).
    RunningStale,
}

impl VmState {
    /// Terminal states end a flow's event loop.
    pub fn is_terminal(self) -> bool {
        matches!(self, VmState::Virgin)
    }

    /// States where the event loop should park and wait for client commands
    /// (e.g., Ctrl+C → InitShutdown). When workers complete and the flow
    /// reaches one of these states with no pending effects, the event loop
    /// should NOT exit — it should wait for external commands.
    pub fn is_interactive_wait(self) -> bool {
        matches!(self, VmState::Running | VmState::RunningStale)
    }
}

/// Reconstruct the current VM state from artifacts + libvirt.
///
/// This replaces the scattered boolean checks in the old `up()`:
/// `overlay_path.exists()`, `marker.exists()`, `Domain::lookup_by_name()`, etc.
pub fn detect_state(sys_config: &SystemConfig, conn: &Connect) -> VmState {
    let id = &sys_config.id;
    let name_opt = sys_config.name.as_deref();
    let vm_name = sys_config.display_name();

    let overlay = paths::overlay_path(id, name_opt);
    let marker = paths::provisioned_marker(id, name_opt);

    let domain = Domain::lookup_by_name(conn, vm_name).ok();
    let running = domain
        .as_ref()
        .is_some_and(|d| d.is_active().unwrap_or(false));

    let stale = running && {
        // Check if domain XML has changed since last define
        let mounts = sys_config.resolve_mounts().unwrap_or_default();
        let drives = sys_config.resolve_drives().unwrap_or_default();
        let seed_hash = {
            // We need the seed path to check XML changes, reconstruct it
            let ssh_key_path = paths::ssh_key_path(id, name_opt);
            let ssh_keys = if ssh_key_path.with_extension("pub").exists() {
                std::fs::read_to_string(ssh_key_path.with_extension("pub"))
                    .map(|k| vec![k.trim().to_string()])
                    .unwrap_or_default()
            } else {
                Vec::new()
            };
            let seed_config = crate::cloudinit::SeedConfig {
                hostname: sys_config.hostname(),
                user_name: &sys_config.config.user.name,
                user_groups: &sys_config.config.user.groups,
                mounts: &mounts,
                autologin: sys_config.config.advanced.autologin,
                ssh_keys: &ssh_keys,
                agent_binary: Some(crate::agent::AGENT_BINARY),
            };
            crate::cloudinit::seed_hash(&seed_config)
        };
        let overlay_path = paths::overlay_path(id, name_opt);
        let seed_path = paths::seed_path(id, name_opt, &seed_hash);
        let xml_path = paths::domain_xml_path(id, name_opt);
        domain_xml::xml_has_changed(sys_config, &overlay_path, &seed_path, &mounts, &drives, &xml_path)
    };

    let overlay_exists = overlay.exists();
    let marker_exists = marker.exists();

    match (overlay_exists, running, marker_exists, stale) {
        (_, true, _, true) => VmState::RunningStale,
        (_, true, _, false) => VmState::Running,
        (true, false, true, _) => VmState::Provisioned,
        (true, false, false, _) if domain.is_some() => VmState::PartialBoot,
        (true, false, false, _) => VmState::Prepared,
        _ if domain.is_some() => VmState::Prepared,
        _ => {
            // Check if base image is cached
            let cache = paths::cache_dir();
            let base = &sys_config.config.image.base;
            if crate::image::is_cached(base, &cache) {
                VmState::ImageCached
            } else {
                VmState::Virgin
            }
        }
    }
}
