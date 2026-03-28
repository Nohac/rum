use std::path::Path;

/// Generate a filesystem tag from a mount target path.
/// E.g. `/mnt/project` → `mnt_project`
pub(crate) fn sanitize_tag(target: &str) -> String {
    target.replace('/', "_").trim_start_matches('_').to_string()
}

/// Derive the VM name from the config filename.
/// `rum.toml` → None, `dev.rum.toml` → Some("dev")
pub(super) fn derive_name(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?;
    if stem == "rum" {
        return None;
    }
    // For `dev.rum.toml`, file_stem gives `dev.rum`, we want `dev`
    // For `rum.toml`, file_stem gives `rum`, handled above
    let name = stem.strip_suffix(".rum").unwrap_or(stem);
    Some(name.to_string())
}

/// Compute an 8-hex-char ID from the canonicalized config path and optional name.
/// Including the name ensures `rum.toml` and `dev.rum.toml` in the same dir get
/// different IDs.
pub(super) fn config_id(canonical_path: &Path, name: Option<&str>) -> String {
    let mut hash: u64 = 0xcbf29ce484222325; // FNV-1a offset basis
    for b in canonical_path.to_string_lossy().bytes() {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    if let Some(n) = name {
        for b in n.bytes() {
            hash ^= b as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    format!("{:08x}", hash as u32)
}
