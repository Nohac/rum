//! Libvirt domain XML generation using facet-xml struct serialization.
//!
//! # Caveats (facet-xml v0.43)
//!
//! - **Compact output only.** Pretty-print (`to_string_pretty`) corrupts text
//!   nodes by inserting whitespace inside `<name>`, `<memory>`, etc.
//!   Tracked upstream: <https://github.com/facet-rs/facet/issues/1982>
//! - **No self-closing tags.** Attribute-only elements like `<boot dev="hd">`
//!   render as `<boot dev="hd"></boot>` instead of `<boot dev="hd"/>`.
//!   Libvirt accepts both forms, so this is cosmetic only.
//! - **`#[facet(flatten)]` is broken** for enum variants — double-wraps
//!   elements. Avoid for now; use separate struct fields instead.

use std::path::Path;

use facet::Facet;
use facet_xml as xml;

use crate::config::{Config, ResolvedDrive, ResolvedMount};

// ── XML model structs ──────────────────────────────────────
//
// Each struct maps to a libvirt XML element. Attributes use
// `#[facet(xml::attribute)]`, text content uses `#[facet(xml::text)]`,
// and child elements are nested structs.

#[derive(Debug, Facet)]
#[facet(rename = "domain")]
struct Domain {
    #[facet(xml::attribute, rename = "type")]
    domain_type: String,
    name: String,
    memory: Memory,
    vcpu: u32,
    os: Os,
    #[facet(default, rename = "memoryBacking")]
    memory_backing: Option<MemoryBacking>,
    features: Features,
    devices: Devices,
}

#[derive(Debug, Facet)]
struct Memory {
    #[facet(xml::attribute)]
    unit: String,
    #[facet(xml::text)]
    value: u64,
}

// ── OS ─────────────────────────────────────────────────────

#[derive(Debug, Facet)]
struct Os {
    #[facet(rename = "type")]
    os_type: OsType,
    boot: Boot,
}

#[derive(Debug, Facet)]
#[facet(rename = "type")]
struct OsType {
    #[facet(xml::attribute)]
    arch: String,
    #[facet(xml::attribute)]
    machine: String,
    #[facet(xml::text)]
    value: String,
}

#[derive(Debug, Facet)]
struct Boot {
    #[facet(xml::attribute)]
    dev: String,
}

// ── memoryBacking (required for virtiofs) ──────────────────

#[derive(Debug, Facet)]
struct MemoryBacking {
    source: MemoryBackingSource,
    access: MemoryBackingAccess,
}

#[derive(Debug, Facet)]
struct MemoryBackingSource {
    #[facet(xml::attribute, rename = "type")]
    source_type: String,
}

#[derive(Debug, Facet)]
struct MemoryBackingAccess {
    #[facet(xml::attribute)]
    mode: String,
}

// ── features ───────────────────────────────────────────────

#[derive(Debug, Facet)]
struct Features {
    acpi: Empty,
    apic: Empty,
}

#[derive(Debug, Default, Facet)]
#[facet(default)]
struct Empty {}

// ── devices ────────────────────────────────────────────────

#[derive(Debug, Facet)]
struct Devices {
    disk: Vec<Disk>,
    filesystem: Vec<Filesystem>,
    interface: Interface,
    serial: Serial,
    console: Console,
}

#[derive(Debug, Facet)]
struct Disk {
    #[facet(xml::attribute, rename = "type")]
    disk_type: String,
    #[facet(xml::attribute)]
    device: String,
    driver: DiskDriver,
    source: DiskSource,
    target: DiskTarget,
    #[facet(default)]
    readonly: Option<Empty>,
}

#[derive(Debug, Facet)]
struct DiskDriver {
    #[facet(xml::attribute)]
    name: String,
    #[facet(xml::attribute, rename = "type")]
    driver_type: String,
}

#[derive(Debug, Facet)]
struct DiskSource {
    #[facet(xml::attribute)]
    file: String,
}

#[derive(Debug, Facet)]
struct DiskTarget {
    #[facet(xml::attribute)]
    dev: String,
    #[facet(xml::attribute)]
    bus: String,
}

// ── virtiofs filesystem ────────────────────────────────────

#[derive(Debug, Facet)]
struct Filesystem {
    #[facet(xml::attribute, rename = "type")]
    fs_type: String,
    #[facet(xml::attribute)]
    accessmode: String,
    driver: FsDriver,
    source: FsSource,
    target: FsTarget,
    #[facet(default)]
    readonly: Option<Empty>,
}

#[derive(Debug, Facet)]
struct FsDriver {
    #[facet(xml::attribute, rename = "type")]
    driver_type: String,
}

#[derive(Debug, Facet)]
struct FsSource {
    #[facet(xml::attribute)]
    dir: String,
}

#[derive(Debug, Facet)]
struct FsTarget {
    #[facet(xml::attribute)]
    dir: String,
}

// ── network ────────────────────────────────────────────────

#[derive(Debug, Facet)]
struct Interface {
    #[facet(xml::attribute, rename = "type")]
    iface_type: String,
    source: InterfaceSource,
    model: InterfaceModel,
}

#[derive(Debug, Facet)]
struct InterfaceSource {
    #[facet(xml::attribute)]
    network: String,
}

#[derive(Debug, Facet)]
struct InterfaceModel {
    #[facet(xml::attribute, rename = "type")]
    model_type: String,
}

// ── serial / console ───────────────────────────────────────

#[derive(Debug, Facet)]
struct Serial {
    #[facet(xml::attribute, rename = "type")]
    serial_type: String,
    target: SerialTarget,
}

#[derive(Debug, Facet)]
#[facet(rename = "target")]
struct SerialTarget {
    #[facet(xml::attribute)]
    port: String,
}

#[derive(Debug, Facet)]
struct Console {
    #[facet(xml::attribute, rename = "type")]
    console_type: String,
    target: ConsoleTarget,
}

#[derive(Debug, Facet)]
#[facet(rename = "target")]
struct ConsoleTarget {
    #[facet(xml::attribute, rename = "type")]
    target_type: String,
    #[facet(xml::attribute)]
    port: String,
}

// ── public API ─────────────────────────────────────────────

/// Generate libvirt domain XML from config.
///
/// Uses compact (single-line) output because facet-xml's pretty-printer
/// corrupts text nodes. Libvirt parses both forms identically.
pub fn generate_domain_xml(
    config: &Config,
    overlay_path: &Path,
    seed_path: &Path,
    mounts: &[ResolvedMount],
    drives: &[ResolvedDrive],
) -> String {
    let memory_backing = if mounts.is_empty() {
        None
    } else {
        Some(MemoryBacking {
            source: MemoryBackingSource {
                source_type: "memfd".into(),
            },
            access: MemoryBackingAccess {
                mode: "shared".into(),
            },
        })
    };

    let filesystems: Vec<Filesystem> = mounts
        .iter()
        .map(|m| Filesystem {
            fs_type: "mount".into(),
            accessmode: "passthrough".into(),
            driver: FsDriver {
                driver_type: "virtiofs".into(),
            },
            source: FsSource {
                dir: m.source.display().to_string(),
            },
            target: FsTarget {
                dir: m.tag.clone(),
            },
            readonly: if m.readonly { Some(Empty {}) } else { None },
        })
        .collect();

    let mut disks = vec![
        Disk {
            disk_type: "file".into(),
            device: "disk".into(),
            driver: DiskDriver {
                name: "qemu".into(),
                driver_type: "qcow2".into(),
            },
            source: DiskSource {
                file: overlay_path.display().to_string(),
            },
            target: DiskTarget {
                dev: "vda".into(),
                bus: "virtio".into(),
            },
            readonly: None,
        },
        Disk {
            disk_type: "file".into(),
            device: "cdrom".into(),
            driver: DiskDriver {
                name: "qemu".into(),
                driver_type: "raw".into(),
            },
            source: DiskSource {
                file: seed_path.display().to_string(),
            },
            target: DiskTarget {
                dev: "sda".into(),
                bus: "sata".into(),
            },
            readonly: Some(Empty {}),
        },
    ];

    // Extra drives (vdb, vdc, ...) from [drives] config
    for drive in drives {
        disks.push(Disk {
            disk_type: "file".into(),
            device: "disk".into(),
            driver: DiskDriver {
                name: "qemu".into(),
                driver_type: "qcow2".into(),
            },
            source: DiskSource {
                file: drive.path.display().to_string(),
            },
            target: DiskTarget {
                dev: drive.dev.clone(),
                bus: "virtio".into(),
            },
            readonly: None,
        });
    }

    let domain = Domain {
        domain_type: config.advanced.domain_type.clone(),
        name: config.name.clone(),
        memory: Memory {
            unit: "KiB".into(),
            value: config.resources.memory_mb * 1024,
        },
        vcpu: config.resources.cpus,
        os: Os {
            os_type: OsType {
                arch: "x86_64".into(),
                machine: config.advanced.machine.clone(),
                value: "hvm".into(),
            },
            boot: Boot { dev: "hd".into() },
        },
        memory_backing,
        features: Features {
            acpi: Empty {},
            apic: Empty {},
        },
        devices: Devices {
            disk: disks,
            filesystem: filesystems,
            interface: Interface {
                iface_type: "network".into(),
                source: InterfaceSource {
                    network: "default".into(),
                },
                model: InterfaceModel {
                    model_type: "virtio".into(),
                },
            },
            serial: Serial {
                serial_type: "pty".into(),
                target: SerialTarget {
                    port: "0".into(),
                },
            },
            console: Console {
                console_type: "pty".into(),
                target: ConsoleTarget {
                    target_type: "serial".into(),
                    port: "0".into(),
                },
            },
        },
    };

    facet_xml::to_string(&domain).expect("domain XML serialization should not fail")
}

/// Check if the generated XML differs from the saved XML on disk.
pub fn xml_has_changed(
    config: &Config,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::*;
    use std::path::PathBuf;

    fn test_config() -> Config {
        Config {
            name: "test-vm".into(),
            image: ImageConfig {
                base: "https://example.com/image.img".into(),
            },
            resources: ResourcesConfig {
                cpus: 2,
                memory_mb: 2048,
            },
            network: NetworkConfig::default(),
            provision: ProvisionConfig::default(),
            advanced: AdvancedConfig {
                libvirt_uri: "qemu:///system".into(),
                domain_type: "kvm".into(),
                machine: "q35".into(),
            },
            mounts: vec![],
            drives: std::collections::BTreeMap::new(),
        }
    }

    fn make_xml(config: &Config, mounts: &[ResolvedMount], drives: &[ResolvedDrive]) -> String {
        generate_domain_xml(
            config,
            &PathBuf::from("/tmp/overlay.qcow2"),
            &PathBuf::from("/tmp/seed.iso"),
            mounts,
            drives,
        )
    }

    #[test]
    fn xml_from_minimal_toml_has_defaults() {
        let toml = r#"
name = "test-vm"

[image]
base = "ubuntu.img"

[resources]
cpus = 1
memory_mb = 512
"#;
        let config: Config = facet_toml::from_str(toml).unwrap();
        let xml = make_xml(&config, &[], &[]);
        assert!(
            xml.contains(r#"type="kvm""#),
            "domain type should default to 'kvm', got:\n{xml}"
        );
        assert!(
            xml.contains(r#"machine="q35""#),
            "machine should default to 'q35', got:\n{xml}"
        );
    }

    #[test]
    fn xml_with_mounts_has_virtiofs() {
        let mounts = vec![
            ResolvedMount {
                source: PathBuf::from("/home/user/project"),
                target: "/mnt/project".into(),
                readonly: false,
                tag: "mnt_project".into(),
            },
            ResolvedMount {
                source: PathBuf::from("/data"),
                target: "/mnt/data".into(),
                readonly: true,
                tag: "mnt_data".into(),
            },
        ];
        let xml = make_xml(&test_config(), &mounts, &[]);
        assert!(xml.contains("<memoryBacking>"));
        assert!(xml.contains(r#"<driver type="virtiofs">"#));
        assert!(xml.contains(r#"<source dir="/home/user/project">"#));
        assert!(xml.contains(r#"<target dir="mnt_data">"#));
        assert!(xml.contains("<readonly>"));
    }

    #[test]
    fn xml_without_mounts_no_memory_backing() {
        let xml = make_xml(&test_config(), &[], &[]);
        assert!(!xml.contains("memoryBacking"));
        assert!(!xml.contains("virtiofs"));
    }

    #[test]
    fn xml_with_drives_has_extra_disks() {
        let drives = vec![
            ResolvedDrive {
                name: "data".into(),
                size: "20G".into(),
                path: PathBuf::from("/home/user/.local/share/rum/test-vm/drive-data.qcow2"),
                target: Some("/mnt/data".into()),
                dev: "vdb".into(),
            },
            ResolvedDrive {
                name: "scratch".into(),
                size: "50G".into(),
                path: PathBuf::from("/home/user/.local/share/rum/test-vm/drive-scratch.qcow2"),
                target: None,
                dev: "vdc".into(),
            },
        ];
        let xml = make_xml(&test_config(), &[], &drives);
        assert!(xml.contains(r#"dev="vdb""#));
        assert!(xml.contains(r#"dev="vdc""#));
        assert!(xml.contains("drive-data.qcow2"));
        assert!(xml.contains("drive-scratch.qcow2"));
    }
}
