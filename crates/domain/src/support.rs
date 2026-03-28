//! Helper functions for domain XML processing.

use std::path::Path;

use facet_xml as xml;

use crate::{DomainConfig, ResolvedDrive, ResolvedMount};

use super::build::generate_domain_xml;
use super::model::LiveVsock;

/// Generate a deterministic MAC address from VM name and interface index.
///
/// Uses the locally-administered OUI prefix `52:54:00` (standard for
/// QEMU/KVM) followed by 3 bytes derived from a simple hash.
pub fn generate_mac(vm_name: &str, index: usize) -> String {
    // Simple FNV-1a-inspired hash for deterministic output
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in vm_name.bytes() {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash ^= index as u64;
    hash = hash.wrapping_mul(0x100000001b3);

    let bytes = hash.to_le_bytes();
    format!(
        "52:54:00:{:02x}:{:02x}:{:02x}",
        bytes[0], bytes[1], bytes[2]
    )
}

/// Extract the auto-assigned vsock CID from a full domain XML string.
///
/// Locates the `<vsock>...</vsock>` section in the XML, deserializes it
/// with `facet_xml::from_str()`, and returns the CID if present.
/// TODO: Pass pre-parsed xml instead of using "find"
pub fn parse_vsock_cid(domain_xml: &str) -> Option<u32> {
    let vsock_start = domain_xml.find("<vsock")?;
    let vsock_end = domain_xml[vsock_start..]
        .find("</vsock>")
        .map(|i| vsock_start + i + "</vsock>".len())?;
    let vsock_section = &domain_xml[vsock_start..vsock_end];

    let live: LiveVsock = xml::from_str(vsock_section).ok()?;
    live.cid.address.as_deref()?.parse::<u32>().ok()
}

/// Check if the generated XML differs from the saved XML on disk.
pub fn xml_has_changed(
    config: &DomainConfig,
    overlay_path: &Path,
    seed_path: &Path,
    mounts: &[ResolvedMount],
    drives: &[ResolvedDrive],
    existing_xml_path: &Path,
) -> bool {
    let new_xml = generate_domain_xml(config, overlay_path, seed_path, mounts, drives);
    match std::fs::read_to_string(existing_xml_path) {
        Ok(existing) => existing != new_xml,
        Err(_) => true,
    }
}
