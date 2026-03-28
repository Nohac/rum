#[cfg(test)]
mod tests {
    use crate::{
        DomainConfig, InterfaceConfig, ResolvedDrive, ResolvedMount, network_xml,
        generate_domain_xml, generate_mac, parse_vsock_cid,
    };
    use std::path::PathBuf;

    fn test_domain_config() -> DomainConfig {
        DomainConfig {
            id: "aabbccdd".into(),
            name: "test-vm".into(),
            domain_type: "kvm".into(),
            machine: "q35".into(),
            memory_mb: 512,
            cpus: 1,
            nat: true,
            interfaces: Vec::new(),
        }
    }

    fn make_xml(config: &DomainConfig, mounts: &[ResolvedMount], drives: &[ResolvedDrive]) -> String {
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
        let xml = make_xml(&test_domain_config(), &[], &[]);
        assert!(
            xml.contains(r#"type="kvm""#),
            "domain type should default to 'kvm', got:\n{xml}"
        );
        assert!(
            xml.contains(r#"machine="q35""#),
            "machine should default to 'q35', got:\n{xml}"
        );
        assert!(
            xml.contains(r#"<vsock model="virtio">"#),
            "should have vsock device, got:\n{xml}"
        );
        assert!(
            xml.contains(r#"auto="yes""#),
            "vsock CID should be auto-assigned, got:\n{xml}"
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
        let xml = make_xml(&test_domain_config(), &mounts, &[]);
        assert!(xml.contains("<memoryBacking>"));
        assert!(xml.contains(r#"<driver type="virtiofs">"#));
        assert!(xml.contains(r#"<source dir="/home/user/project">"#));
        assert!(xml.contains(r#"<target dir="mnt_data">"#));
        assert!(xml.contains("<readonly>"));
    }

    #[test]
    fn xml_without_mounts_no_memory_backing() {
        let xml = make_xml(&test_domain_config(), &[], &[]);
        assert!(!xml.contains("memoryBacking"));
        assert!(!xml.contains("virtiofs"));
    }

    #[test]
    fn xml_with_drives_has_extra_disks() {
        let drives = vec![
            ResolvedDrive {
                path: PathBuf::from("/home/user/.local/share/rum/test-vm/drive-data.qcow2"),
                dev: "vdb".into(),
            },
            ResolvedDrive {
                path: PathBuf::from("/home/user/.local/share/rum/test-vm/drive-scratch.qcow2"),
                dev: "vdc".into(),
            },
        ];
        let xml = make_xml(&test_domain_config(), &[], &drives);
        assert!(xml.contains(r#"dev="vdb""#));
        assert!(xml.contains(r#"dev="vdc""#));
        assert!(xml.contains("drive-data.qcow2"));
        assert!(xml.contains("drive-scratch.qcow2"));
    }

    #[test]
    fn xml_default_config_has_single_nat_interface() {
        let xml = make_xml(&test_domain_config(), &[], &[]);
        assert!(xml.contains(r#"<source network="default">"#));
        // No explicit MAC on NAT interface
        assert!(!xml.contains("<mac"));
    }

    #[test]
    fn xml_nat_plus_extra_nic() {
        let mut config = test_domain_config();
        config.interfaces = vec![InterfaceConfig {
            network: "hostonly".into(),
        }];
        let xml = make_xml(&config, &[], &[]);
        let expected_net = network_xml::prefixed_name(&config.id, "hostonly");
        // NAT interface
        assert!(xml.contains(r#"<source network="default">"#));
        // Extra interface with MAC and prefixed network name
        assert!(
            xml.contains(&format!(r#"<source network="{expected_net}">"#)),
            "expected prefixed network name '{expected_net}' in:\n{xml}"
        );
        assert!(xml.contains("<mac"));
        assert!(xml.contains("52:54:00:"));
    }

    #[test]
    fn xml_no_nat_with_extra_nic() {
        let mut config = test_domain_config();
        config.nat = false;
        config.interfaces = vec![InterfaceConfig {
            network: "isolated".into(),
        }];
        let xml = make_xml(&config, &[], &[]);
        let expected_net = network_xml::prefixed_name(&config.id, "isolated");
        assert!(!xml.contains(r#"<source network="default">"#));
        assert!(
            xml.contains(&format!(r#"<source network="{expected_net}">"#)),
            "expected prefixed network name '{expected_net}' in:\n{xml}"
        );
    }

    #[test]
    fn xml_no_networking() {
        let mut config = test_domain_config();
        config.nat = false;
        let xml = make_xml(&config, &[], &[]);
        assert!(!xml.contains("<interface"));
        assert!(!xml.contains(r#"network="default""#));
    }

    #[test]
    fn generate_mac_is_deterministic() {
        let mac1 = generate_mac("test-vm", 0);
        let mac2 = generate_mac("test-vm", 0);
        assert_eq!(mac1, mac2);
        assert!(mac1.starts_with("52:54:00:"));
    }

    #[test]
    fn generate_mac_differs_by_index() {
        let mac0 = generate_mac("test-vm", 0);
        let mac1 = generate_mac("test-vm", 1);
        assert_ne!(mac0, mac1);
    }

    #[test]
    fn parse_vsock_cid_from_live_xml() {
        let xml = r#"<domain type="kvm">
  <name>test-vm</name>
  <devices>
    <vsock model="virtio">
      <cid auto="yes" address="3"/>
      <alias name="vsock0"/>
    </vsock>
  </devices>
</domain>"#;
        assert_eq!(parse_vsock_cid(xml), Some(3));
    }

    #[test]
    fn parse_vsock_cid_no_address() {
        let xml = r#"<domain type="kvm">
  <devices>
    <vsock model="virtio">
      <cid auto="yes"/>
    </vsock>
  </devices>
</domain>"#;
        assert_eq!(parse_vsock_cid(xml), None);
    }

    #[test]
    fn parse_vsock_cid_no_vsock_section() {
        let xml = r#"<domain type="kvm"><name>test</name></domain>"#;
        assert_eq!(parse_vsock_cid(xml), None);
    }
}
