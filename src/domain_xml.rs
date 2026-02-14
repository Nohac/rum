use std::fmt::Write;
use std::path::Path;

use crate::config::{Config, ResolvedMount};

/// Generate libvirt domain XML from config.
pub fn generate_domain_xml(
    config: &Config,
    overlay_path: &Path,
    seed_path: &Path,
    mounts: &[ResolvedMount],
) -> String {
    let name = &config.name;
    let memory_kib = config.resources.memory_mb * 1024;
    let cpus = config.resources.cpus;
    let domain_type = &config.advanced.domain_type;
    let machine = &config.advanced.machine;
    let overlay = overlay_path.display();
    let seed = seed_path.display();

    let mut xml = String::new();
    writeln!(xml, "<domain type='{domain_type}'>").unwrap();
    writeln!(xml, "  <name>{name}</name>").unwrap();
    writeln!(xml, "  <memory unit='KiB'>{memory_kib}</memory>").unwrap();
    writeln!(xml, "  <vcpu>{cpus}</vcpu>").unwrap();
    writeln!(xml, "  <os>").unwrap();
    writeln!(xml, "    <type arch='x86_64' machine='{machine}'>hvm</type>").unwrap();
    writeln!(xml, "    <boot dev='hd'/>").unwrap();
    writeln!(xml, "  </os>").unwrap();

    if !mounts.is_empty() {
        writeln!(xml, "  <memoryBacking>").unwrap();
        writeln!(xml, "    <source type='memfd'/>").unwrap();
        writeln!(xml, "    <access mode='shared'/>").unwrap();
        writeln!(xml, "  </memoryBacking>").unwrap();
    }

    writeln!(xml, "  <features>").unwrap();
    writeln!(xml, "    <acpi/>").unwrap();
    writeln!(xml, "    <apic/>").unwrap();
    writeln!(xml, "  </features>").unwrap();
    writeln!(xml, "  <devices>").unwrap();
    writeln!(xml, "    <disk type='file' device='disk'>").unwrap();
    writeln!(xml, "      <driver name='qemu' type='qcow2'/>").unwrap();
    writeln!(xml, "      <source file='{overlay}'/>").unwrap();
    writeln!(xml, "      <target dev='vda' bus='virtio'/>").unwrap();
    writeln!(xml, "    </disk>").unwrap();
    writeln!(xml, "    <disk type='file' device='cdrom'>").unwrap();
    writeln!(xml, "      <driver name='qemu' type='raw'/>").unwrap();
    writeln!(xml, "      <source file='{seed}'/>").unwrap();
    writeln!(xml, "      <target dev='sda' bus='sata'/>").unwrap();
    writeln!(xml, "      <readonly/>").unwrap();
    writeln!(xml, "    </disk>").unwrap();

    for m in mounts {
        let source_dir = m.source.display();
        let tag = &m.tag;
        writeln!(xml, "    <filesystem type='mount' accessmode='passthrough'>").unwrap();
        writeln!(xml, "      <driver type='virtiofs'/>").unwrap();
        writeln!(xml, "      <source dir='{source_dir}'/>").unwrap();
        writeln!(xml, "      <target dir='{tag}'/>").unwrap();
        if m.readonly {
            writeln!(xml, "      <readonly/>").unwrap();
        }
        writeln!(xml, "    </filesystem>").unwrap();
    }

    writeln!(xml, "    <interface type='network'>").unwrap();
    writeln!(xml, "      <source network='default'/>").unwrap();
    writeln!(xml, "      <model type='virtio'/>").unwrap();
    writeln!(xml, "    </interface>").unwrap();
    writeln!(xml, "    <serial type='pty'>").unwrap();
    writeln!(xml, "      <target port='0'/>").unwrap();
    writeln!(xml, "    </serial>").unwrap();
    writeln!(xml, "    <console type='pty'>").unwrap();
    writeln!(xml, "      <target type='serial' port='0'/>").unwrap();
    writeln!(xml, "    </console>").unwrap();
    writeln!(xml, "  </devices>").unwrap();
    writeln!(xml, "</domain>").unwrap();

    xml
}

/// Check if the generated XML differs from the saved XML on disk.
pub fn xml_has_changed(
    config: &Config,
    overlay_path: &Path,
    seed_path: &Path,
    mounts: &[ResolvedMount],
    existing_xml_path: &Path,
) -> bool {
    let new_xml = generate_domain_xml(config, overlay_path, seed_path, mounts);
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
        }
    }

    #[test]
    fn xml_contains_vm_name() {
        let config = test_config();
        let xml = generate_domain_xml(
            &config,
            &PathBuf::from("/tmp/overlay.qcow2"),
            &PathBuf::from("/tmp/seed.iso"),
            &[],
        );
        assert!(xml.contains("<name>test-vm</name>"));
    }

    #[test]
    fn xml_contains_resources() {
        let config = test_config();
        let xml = generate_domain_xml(
            &config,
            &PathBuf::from("/tmp/overlay.qcow2"),
            &PathBuf::from("/tmp/seed.iso"),
            &[],
        );
        assert!(xml.contains("<memory unit='KiB'>2097152</memory>"));
        assert!(xml.contains("<vcpu>2</vcpu>"));
    }

    #[test]
    fn xml_contains_serial_console() {
        let config = test_config();
        let xml = generate_domain_xml(
            &config,
            &PathBuf::from("/tmp/overlay.qcow2"),
            &PathBuf::from("/tmp/seed.iso"),
            &[],
        );
        assert!(xml.contains("<serial type='pty'>"));
        assert!(xml.contains("<console type='pty'>"));
    }

    #[test]
    fn xml_contains_devices() {
        let config = test_config();
        let xml = generate_domain_xml(
            &config,
            &PathBuf::from("/tmp/overlay.qcow2"),
            &PathBuf::from("/tmp/seed.iso"),
            &[],
        );
        assert!(xml.contains("bus='virtio'"));
        assert!(xml.contains("bus='sata'"));
        assert!(xml.contains("<source network='default'/>"));
    }

    #[test]
    fn xml_from_minimal_toml_has_domain_type() {
        let toml = r#"
name = "test-vm"

[image]
base = "ubuntu.img"

[resources]
cpus = 1
memory_mb = 512
"#;
        let config: Config = facet_toml::from_str(toml).unwrap();
        let xml = generate_domain_xml(
            &config,
            &PathBuf::from("/tmp/overlay.qcow2"),
            &PathBuf::from("/tmp/seed.iso"),
            &[],
        );
        assert!(
            xml.contains("type='kvm'"),
            "domain type should default to 'kvm', got:\n{xml}"
        );
        assert!(
            xml.contains("machine='q35'"),
            "machine should default to 'q35', got:\n{xml}"
        );
    }

    #[test]
    fn xml_with_mounts_has_virtiofs() {
        let config = test_config();
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
        let xml = generate_domain_xml(
            &config,
            &PathBuf::from("/tmp/overlay.qcow2"),
            &PathBuf::from("/tmp/seed.iso"),
            &mounts,
        );
        assert!(xml.contains("<memoryBacking>"));
        assert!(xml.contains("<source type='memfd'/>"));
        assert!(xml.contains("<access mode='shared'/>"));
        assert!(xml.contains("<driver type='virtiofs'/>"));
        assert!(xml.contains("<source dir='/home/user/project'/>"));
        assert!(xml.contains("<target dir='mnt_project'/>"));
        assert!(xml.contains("<source dir='/data'/>"));
        assert!(xml.contains("<target dir='mnt_data'/>"));
        // readonly mount should have <readonly/> inside filesystem
        assert!(xml.contains("<readonly/>"));
    }

    #[test]
    fn xml_without_mounts_no_memory_backing() {
        let config = test_config();
        let xml = generate_domain_xml(
            &config,
            &PathBuf::from("/tmp/overlay.qcow2"),
            &PathBuf::from("/tmp/seed.iso"),
            &[],
        );
        assert!(!xml.contains("<memoryBacking>"));
        assert!(!xml.contains("virtiofs"));
    }
}
