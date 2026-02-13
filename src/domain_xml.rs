use std::path::Path;

use crate::config::Config;

/// Generate libvirt domain XML from config.
pub fn generate_domain_xml(config: &Config, overlay_path: &Path, seed_path: &Path) -> String {
    let name = &config.name;
    let memory_kib = config.resources.memory_mb * 1024;
    let cpus = config.resources.cpus;
    let domain_type = &config.advanced.domain_type;
    let machine = &config.advanced.machine;
    let overlay = overlay_path.display();
    let seed = seed_path.display();

    format!(
        r#"<domain type='{domain_type}'>
  <name>{name}</name>
  <memory unit='KiB'>{memory_kib}</memory>
  <vcpu>{cpus}</vcpu>
  <os>
    <type arch='x86_64' machine='{machine}'>hvm</type>
    <boot dev='hd'/>
  </os>
  <features>
    <acpi/>
    <apic/>
  </features>
  <devices>
    <disk type='file' device='disk'>
      <driver name='qemu' type='qcow2'/>
      <source file='{overlay}'/>
      <target dev='vda' bus='virtio'/>
    </disk>
    <disk type='file' device='cdrom'>
      <driver name='qemu' type='raw'/>
      <source file='{seed}'/>
      <target dev='sda' bus='sata'/>
      <readonly/>
    </disk>
    <interface type='network'>
      <source network='default'/>
      <model type='virtio'/>
    </interface>
    <serial type='pty'>
      <target port='0'/>
    </serial>
    <console type='pty'>
      <target type='serial' port='0'/>
    </console>
  </devices>
</domain>
"#
    )
}

/// Check if the generated XML differs from the saved XML on disk.
pub fn xml_has_changed(
    config: &Config,
    overlay_path: &Path,
    seed_path: &Path,
    existing_xml_path: &Path,
) -> bool {
    let new_xml = generate_domain_xml(config, overlay_path, seed_path);
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
        }
    }

    #[test]
    fn xml_contains_vm_name() {
        let config = test_config();
        let xml = generate_domain_xml(
            &config,
            &PathBuf::from("/tmp/overlay.qcow2"),
            &PathBuf::from("/tmp/seed.iso"),
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
}
