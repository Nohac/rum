//! Domain XML generation from config.

use std::path::Path;

use crate::{DomainConfig, ResolvedDrive, ResolvedMount, prefixed_name};

use super::model::*;
use super::support::generate_mac;

/// Generate libvirt domain XML from config.
///
/// Uses compact (single-line) output because facet-xml's pretty-printer
/// corrupts text nodes. Libvirt parses both forms identically.
pub fn generate_domain_xml(
    config: &DomainConfig,
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
            target: FsTarget { dir: m.tag.clone() },
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

    // Build network interfaces
    let mut interfaces = Vec::new();

    if config.nat {
        interfaces.push(Interface {
            iface_type: "network".into(),
            mac: None,
            source: InterfaceSource {
                network: "default".into(),
            },
            model: InterfaceModel {
                model_type: "virtio".into(),
            },
        });
    }

    let display = &config.name;
    for (i, iface_cfg) in config.interfaces.iter().enumerate() {
        let libvirt_name = prefixed_name(&config.id, &iface_cfg.network);
        interfaces.push(Interface {
            iface_type: "network".into(),
            mac: Some(InterfaceMac {
                address: generate_mac(display, i),
            }),
            source: InterfaceSource {
                network: libvirt_name,
            },
            model: InterfaceModel {
                model_type: "virtio".into(),
            },
        });
    }

    let domain = Domain {
        domain_type: config.domain_type.clone(),
        name: config.name.clone(),
        memory: Memory {
            unit: "KiB".into(),
            value: config.memory_mb * 1024,
        },
        vcpu: config.cpus,
        os: Os {
            os_type: OsType {
                arch: "x86_64".into(),
                machine: config.machine.clone(),
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
            interface: interfaces,
            serial: Serial {
                serial_type: "pty".into(),
                target: SerialTarget { port: "0".into() },
            },
            console: Console {
                console_type: "pty".into(),
                target: ConsoleTarget {
                    target_type: "serial".into(),
                    port: "0".into(),
                },
            },
            vsock: Vsock {
                model: "virtio".into(),
                cid: VsockCid {
                    auto: "yes".into(),
                },
            },
        },
    };

    facet_xml::to_string(&domain).expect("domain XML serialization should not fail")
}
