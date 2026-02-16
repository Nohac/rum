use std::path::PathBuf;

/// Base image cache directory: `~/.cache/rum/images/`
pub fn cache_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("rum")
        .join("images")
}

/// Per-VM work directory: `~/.local/share/rum/<id>-<name>/` or `~/.local/share/rum/<id>/`
pub fn work_dir(id: &str, name: Option<&str>) -> PathBuf {
    let dir_name = match name {
        Some(n) => format!("{id}-{n}"),
        None => id.to_string(),
    };
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("rum")
        .join(dir_name)
}

/// Path to the qcow2 overlay for a VM.
pub fn overlay_path(id: &str, name: Option<&str>) -> PathBuf {
    work_dir(id, name).join("overlay.qcow2")
}

/// Path to the cloud-init seed ISO for a VM, keyed by content hash.
pub fn seed_path(id: &str, name: Option<&str>, hash: &str) -> PathBuf {
    work_dir(id, name).join(format!("seed-{hash}.iso"))
}

/// Path to the saved domain XML for a VM.
pub fn domain_xml_path(id: &str, name: Option<&str>) -> PathBuf {
    work_dir(id, name).join("domain.xml")
}

/// Path to an extra drive image for a VM.
pub fn drive_path(id: &str, name: Option<&str>, drive_name: &str) -> PathBuf {
    work_dir(id, name).join(format!("drive-{drive_name}.qcow2"))
}

/// Path to the config_path file that records which config file created this work dir.
pub fn config_path_file(id: &str, name: Option<&str>) -> PathBuf {
    work_dir(id, name).join("config_path")
}
