use std::path::Path;

use virt::domain::Domain;

use crate::config::SystemConfig;
use crate::error::RumError;
use crate::{cloudinit, image, paths, qcow2};

pub async fn prepare_vm(sys_config: &SystemConfig, base_image: &Path) -> Result<(), RumError> {
    let id = &sys_config.id;
    let name_opt = sys_config.name.as_deref();
    let vm_name = sys_config.display_name();
    let config = &sys_config.config;
    let work = paths::work_dir(id, name_opt);
    let overlay_path = paths::overlay_path(id, name_opt);

    let mounts = sys_config.resolve_mounts()?;
    let drives = sys_config.resolve_drives()?;

    let ssh_key_path = paths::ssh_key_path(id, name_opt);
    crate::vm::ssh::ensure_ssh_keypair(&ssh_key_path).await?;
    let ssh_keys =
        crate::vm::ssh::collect_ssh_keys(&ssh_key_path, &config.ssh.authorized_keys).await?;

    let seed_config = cloudinit::SeedConfig {
        hostname: sys_config.hostname(),
        user_name: &config.user.name,
        user_groups: &config.user.groups,
        mounts: &mounts,
        autologin: config.advanced.autologin,
        ssh_keys: &ssh_keys,
        agent_binary: Some(crate::agent_client::AGENT_BINARY),
    };
    let seed_hash = cloudinit::seed_hash(&seed_config);
    let seed_path = paths::seed_path(id, name_opt, &seed_hash);
    let xml_path = paths::domain_xml_path(id, name_opt);

    let disk_size = crate::util::parse_size(&config.resources.disk)?;

    if !overlay_path.exists() {
        qcow2::create_qcow2_overlay(&overlay_path, base_image, Some(disk_size))?;
    }
    for d in &drives {
        if !d.path.exists() {
            qcow2::create_qcow2(&d.path, &d.size)?;
        }
    }

    if !seed_path.exists() {
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

    let domain_config = domain::DomainConfig {
        id: sys_config.id.clone(),
        name: sys_config.display_name().to_string(),
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
        &overlay_path,
        &seed_path,
        &domain_mounts,
        &domain_drives,
    );
    let conn = crate::vm::libvirt::connect(sys_config)?;

    match Domain::lookup_by_name(&conn, vm_name) {
        Ok(dom) => {
            if domain::xml_has_changed(
                &domain_config,
                &overlay_path,
                &seed_path,
                &domain_mounts,
                &domain_drives,
                &xml_path,
            ) {
                if crate::vm::libvirt::is_running(&dom) {
                    return Err(RumError::RequiresRestart {
                        name: vm_name.to_string(),
                    });
                }
                dom.undefine().map_err(|e| RumError::Libvirt {
                    message: format!("failed to undefine domain: {e}"),
                    hint: "check libvirt permissions".into(),
                })?;
                crate::vm::libvirt::define_domain(&conn, &xml)?;
                tracing::info!(vm_name, "domain redefined with updated config");
            }
        }
        Err(_) => {
            crate::vm::libvirt::define_domain(&conn, &xml)?;
            tracing::info!(vm_name, "domain defined");
        }
    }

    tokio::fs::write(&xml_path, &xml)
        .await
        .map_err(|e| RumError::Io {
            context: format!("saving domain XML to {}", xml_path.display()),
            source: e,
        })?;

    let cp_file = paths::config_path_file(id, name_opt);
    tokio::fs::write(&cp_file, sys_config.config_path.to_string_lossy().as_bytes())
        .await
        .map_err(|e| RumError::Io {
            context: format!("saving config path to {}", cp_file.display()),
            source: e,
        })?;

    crate::vm::libvirt::ensure_networks(&conn, sys_config)?;
    Ok(())
}

pub async fn ensure_image(base_url: &str, cache_dir: &Path) -> Result<std::path::PathBuf, RumError> {
    image::ensure_base_image(base_url, cache_dir).await
}
