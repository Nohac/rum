use std::path::PathBuf;

/// Base image cache directory: `~/.cache/rum/images/`
pub fn cache_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("rum")
        .join("images")
}

/// Per-VM work directory: `~/.local/share/rum/<name>/`
pub fn work_dir(name: &str) -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("rum")
        .join(name)
}

/// Path to the qcow2 overlay for a VM.
pub fn overlay_path(name: &str) -> PathBuf {
    work_dir(name).join("overlay.qcow2")
}

/// Path to the cloud-init seed ISO for a VM.
pub fn seed_path(name: &str) -> PathBuf {
    work_dir(name).join("seed.iso")
}

/// Path to the saved domain XML for a VM.
pub fn domain_xml_path(name: &str) -> PathBuf {
    work_dir(name).join("domain.xml")
}
