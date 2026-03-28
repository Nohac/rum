use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::identity::{config_id, derive_name};
use super::runtime::*;
use super::schema::*;
use super::validate::{validate_config, validate_name};

fn valid_config() -> Config {
    Config {
        image: ImageConfig {
            base: "https://example.com/image.qcow2".into(),
        },
        resources: ResourcesConfig {
            cpus: 1,
            memory_mb: 512,
            disk: "20G".into(),
        },
        network: NetworkConfig::default(),
        provision: ProvisionConfig::default(),
        advanced: AdvancedConfig::default(),
        ssh: SshConfig::default(),
        user: UserConfig::default(),
        mounts: vec![],
        drives: BTreeMap::new(),
        fs: BTreeMap::new(),
        ports: vec![],
    }
}

/// Build a SystemConfig for testing (with fake path/id).
pub fn test_system_config() -> SystemConfig {
    SystemConfig {
        id: "deadbeef".into(),
        name: Some("test-vm".into()),
        config_path: PathBuf::from("/tmp/test-vm.rum.toml"),
        config: valid_config(),
    }
}

#[test]
fn derive_name_from_rum_toml() {
    assert_eq!(derive_name(Path::new("rum.toml")), None);
    assert_eq!(derive_name(Path::new("/some/path/rum.toml")), None);
}

#[test]
fn derive_name_from_prefixed_rum_toml() {
    assert_eq!(derive_name(Path::new("dev.rum.toml")), Some("dev".into()));
    assert_eq!(
        derive_name(Path::new("/some/path/staging.rum.toml")),
        Some("staging".into())
    );
}

#[test]
fn derive_name_from_other_toml() {
    // A file like `myvm.toml` (no .rum. infix) uses the full stem
    assert_eq!(derive_name(Path::new("myvm.toml")), Some("myvm".into()));
}

#[test]
fn config_id_is_deterministic() {
    let id1 = config_id(Path::new("/a/b/rum.toml"), None);
    let id2 = config_id(Path::new("/a/b/rum.toml"), None);
    assert_eq!(id1, id2);
    assert_eq!(id1.len(), 8);
}

#[test]
fn config_id_differs_by_name() {
    let id1 = config_id(Path::new("/a/b/rum.toml"), None);
    let id2 = config_id(Path::new("/a/b/dev.rum.toml"), Some("dev"));
    assert_ne!(id1, id2);
}

#[test]
fn config_id_differs_by_path() {
    let id1 = config_id(Path::new("/a/rum.toml"), None);
    let id2 = config_id(Path::new("/b/rum.toml"), None);
    assert_ne!(id1, id2);
}

#[test]
fn valid_names() {
    for name in ["myvm", "test-vm", "vm.dev", "VM_01", "a"] {
        validate_name(name).unwrap();
    }
}

#[test]
fn invalid_names() {
    for name in [
        "",
        "-bad",
        ".bad",
        "_bad",
        "../etc",
        "a/b",
        "vm<inject>",
        "vm&amp",
        "hello world",
    ] {
        assert!(
            validate_name(name).is_err(),
            "expected name '{}' to be rejected",
            name
        );
    }
}

#[test]
fn empty_interface_network_rejected() {
    let mut config = valid_config();
    config.network.interfaces = vec![InterfaceConfig {
        network: String::new(),
        ip: String::new(),
    }];
    assert!(validate_config(&config).is_err());
}

#[test]
fn valid_interface_config() {
    let mut config = valid_config();
    config.network.interfaces = vec![InterfaceConfig {
        network: "rum-hostonly".into(),
        ip: "192.168.50.10".into(),
    }];
    validate_config(&config).unwrap();
}

#[test]
fn parse_config_with_interfaces() {
    let toml = r#"
[image]
base = "ubuntu.img"

[resources]
cpus = 1
memory_mb = 512

[network]
nat = false

[[network.interfaces]]
network = "rum-hostonly"
ip = "192.168.50.10"

[[network.interfaces]]
network = "dev-net"
"#;
    let config: Config = facet_toml::from_str(toml).unwrap();
    assert!(!config.network.nat);
    assert_eq!(config.network.interfaces.len(), 2);
    assert_eq!(config.network.interfaces[0].network, "rum-hostonly");
    assert_eq!(config.network.interfaces[0].ip, "192.168.50.10");
    assert_eq!(config.network.interfaces[1].network, "dev-net");
    assert!(config.network.interfaces[1].ip.is_empty());
}

#[test]
fn display_name_uses_name_when_present() {
    let sc = test_system_config();
    assert_eq!(sc.display_name(), "test-vm");
}

#[test]
fn display_name_falls_back_to_id() {
    let mut sc = test_system_config();
    sc.name = None;
    assert_eq!(sc.display_name(), "deadbeef");
}

#[test]
fn hostname_falls_back_to_display_name() {
    let sc = test_system_config();
    assert_eq!(sc.hostname(), "test-vm");
}

#[test]
fn hostname_uses_explicit_value() {
    let mut sc = test_system_config();
    sc.config.network.hostname = "custom-host".into();
    assert_eq!(sc.hostname(), "custom-host");
}

#[test]
fn parse_config_with_fs_ext4() {
    let toml = r#"
[image]
base = "ubuntu.img"

[resources]
cpus = 1
memory_mb = 512

[drives.data]
size = "20G"

[[fs.ext4]]
drive = "data"
target = "/mnt/data"
"#;
    let config: Config = facet_toml::from_str(toml).unwrap();
    validate_config(&config).unwrap();
    assert_eq!(config.fs.len(), 1);
    assert_eq!(config.fs["ext4"][0].drive, "data");
    assert_eq!(config.fs["ext4"][0].target, "/mnt/data");
}

#[test]
fn parse_config_with_fs_zfs() {
    let toml = r#"
[image]
base = "ubuntu.img"

[resources]
cpus = 1
memory_mb = 512

[drives.logs1]
size = "50G"

[drives.logs2]
size = "50G"

[[fs.zfs]]
drives = ["logs1", "logs2"]
target = "/mnt/logs"
mode = "mirror"
pool = "logspool"
"#;
    let config: Config = facet_toml::from_str(toml).unwrap();
    validate_config(&config).unwrap();
    assert_eq!(config.fs["zfs"][0].drives, vec!["logs1", "logs2"]);
    assert_eq!(config.fs["zfs"][0].mode.as_deref(), Some("mirror"));
    assert_eq!(config.fs["zfs"][0].pool, "logspool");
}

#[test]
fn fs_missing_target_rejected() {
    let mut config = valid_config();
    config.drives.insert("d".into(), DriveConfig { size: "10G".into() });
    config.fs.insert(
        "ext4".into(),
        vec![FsEntryConfig {
            drive: "d".into(),
            target: String::new(),
            ..Default::default()
        }],
    );
    assert!(validate_config(&config).is_err());
}

#[test]
fn fs_nonexistent_drive_rejected() {
    let mut config = valid_config();
    config.fs.insert(
        "ext4".into(),
        vec![FsEntryConfig {
            drive: "nonexistent".into(),
            target: "/mnt/data".into(),
            ..Default::default()
        }],
    );
    assert!(validate_config(&config).is_err());
}

#[test]
fn fs_duplicate_drive_rejected() {
    let mut config = valid_config();
    config.drives.insert("d".into(), DriveConfig { size: "10G".into() });
    config.fs.insert(
        "ext4".into(),
        vec![
            FsEntryConfig {
                drive: "d".into(),
                target: "/mnt/a".into(),
                ..Default::default()
            },
            FsEntryConfig {
                drive: "d".into(),
                target: "/mnt/b".into(),
                ..Default::default()
            },
        ],
    );
    assert!(validate_config(&config).is_err());
}

#[test]
fn fs_simple_with_drives_rejected() {
    let mut config = valid_config();
    config.drives.insert("d".into(), DriveConfig { size: "10G".into() });
    config.fs.insert(
        "ext4".into(),
        vec![FsEntryConfig {
            drives: vec!["d".into()],
            target: "/mnt/data".into(),
            ..Default::default()
        }],
    );
    assert!(validate_config(&config).is_err());
}

#[test]
fn fs_zfs_with_drive_rejected() {
    let mut config = valid_config();
    config.drives.insert("d".into(), DriveConfig { size: "10G".into() });
    config.fs.insert(
        "zfs".into(),
        vec![FsEntryConfig {
            drive: "d".into(),
            target: "/mnt/data".into(),
            ..Default::default()
        }],
    );
    assert!(validate_config(&config).is_err());
}

#[test]
fn resolve_fs_simple() {
    let mut sc = test_system_config();
    sc.config.drives.insert("data".into(), DriveConfig { size: "20G".into() });
    sc.config.fs.insert(
        "ext4".into(),
        vec![FsEntryConfig {
            drive: "data".into(),
            target: "/mnt/data".into(),
            ..Default::default()
        }],
    );
    let drives = sc.resolve_drives().unwrap();
    let fs = sc.resolve_fs(&drives).unwrap();
    assert_eq!(fs.len(), 1);
    match &fs[0] {
        ResolvedFs::Simple(s) => {
            assert_eq!(s.filesystem, "ext4");
            assert_eq!(s.dev, "/dev/vdb");
            assert_eq!(s.target, "/mnt/data");
        }
        _ => panic!("expected Simple"),
    }
}

#[test]
fn resolve_fs_zfs() {
    let mut sc = test_system_config();
    sc.config
        .drives
        .insert("logs1".into(), DriveConfig { size: "50G".into() });
    sc.config
        .drives
        .insert("logs2".into(), DriveConfig { size: "50G".into() });
    sc.config.fs.insert(
        "zfs".into(),
        vec![FsEntryConfig {
            drives: vec!["logs1".into(), "logs2".into()],
            target: "/mnt/logs".into(),
            mode: Some("mirror".into()),
            ..Default::default()
        }],
    );
    let drives = sc.resolve_drives().unwrap();
    let fs = sc.resolve_fs(&drives).unwrap();
    assert_eq!(fs.len(), 1);
    match &fs[0] {
        ResolvedFs::Zfs(z) => {
            assert_eq!(z.pool, "logs1"); // defaults to first drive name
            assert_eq!(z.devs.len(), 2);
            assert_eq!(z.mode.as_deref(), Some("mirror"));
            assert_eq!(z.target, "/mnt/logs");
        }
        _ => panic!("expected Zfs"),
    }
}

#[test]
fn parse_config_with_provision_system_and_boot() {
    let toml = r#"
[image]
base = "ubuntu.img"

[resources]
cpus = 1
memory_mb = 512

[provision.system]
script = "echo system"

[provision.boot]
script = "echo boot"
"#;
    let config: Config = facet_toml::from_str(toml).unwrap();
    let system = config.provision.system.as_ref().unwrap();
    assert_eq!(system.script, "echo system");
    let boot = config.provision.boot.as_ref().unwrap();
    assert_eq!(boot.script, "echo boot");
}

#[test]
fn parse_config_provision_absent_is_none() {
    let toml = r#"
[image]
base = "ubuntu.img"

[resources]
cpus = 1
memory_mb = 512
"#;
    let config: Config = facet_toml::from_str(toml).unwrap();
    assert!(config.provision.system.is_none());
    assert!(config.provision.boot.is_none());
}

#[test]
fn mount_target_exact_overlap_rejected() {
    let mut config = valid_config();
    config.mounts = vec![MountConfig {
        source: "/tmp".into(),
        target: "/mnt/data".into(),
        ..Default::default()
    }];
    config.drives.insert("d".into(), DriveConfig { size: "10G".into() });
    config.fs.insert(
        "ext4".into(),
        vec![FsEntryConfig {
            drive: "d".into(),
            target: "/mnt/data".into(),
            ..Default::default()
        }],
    );
    let err = validate_config(&config).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("/mnt/data"), "error should mention the target path: {msg}");
    assert!(msg.contains("[[mounts]]"), "error should mention [[mounts]]: {msg}");
    assert!(msg.contains("[[fs.ext4]]"), "error should mention [[fs.ext4]]: {msg}");
}

#[test]
fn mount_target_prefix_overlap_rejected() {
    let mut config = valid_config();
    config.mounts = vec![MountConfig {
        source: "/tmp".into(),
        target: "/mnt/data".into(),
        ..Default::default()
    }];
    config.drives.insert("d".into(), DriveConfig { size: "10G".into() });
    config.fs.insert(
        "ext4".into(),
        vec![FsEntryConfig {
            drive: "d".into(),
            target: "/mnt/data/sub".into(),
            ..Default::default()
        }],
    );
    let err = validate_config(&config).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("overlaps"), "error should mention overlap: {msg}");
    assert!(msg.contains("/mnt/data/sub"), "error should mention the child path: {msg}");
    assert!(msg.contains("/mnt/data"), "error should mention the parent path: {msg}");
}

#[test]
fn mount_target_no_false_prefix_overlap() {
    // /mnt/data and /mnt/database should NOT be flagged as overlapping
    let mut config = valid_config();
    config.mounts = vec![MountConfig {
        source: "/tmp".into(),
        target: "/mnt/data".into(),
        ..Default::default()
    }];
    config.drives.insert("d".into(), DriveConfig { size: "10G".into() });
    config.fs.insert(
        "ext4".into(),
        vec![FsEntryConfig {
            drive: "d".into(),
            target: "/mnt/database".into(),
            ..Default::default()
        }],
    );
    validate_config(&config).unwrap();
}

#[test]
fn mount_target_non_overlapping_passes() {
    let mut config = valid_config();
    config.mounts = vec![MountConfig {
        source: "/tmp".into(),
        target: "/mnt/shared".into(),
        ..Default::default()
    }];
    config.drives.insert("d".into(), DriveConfig { size: "10G".into() });
    config.fs.insert(
        "ext4".into(),
        vec![FsEntryConfig {
            drive: "d".into(),
            target: "/mnt/data".into(),
            ..Default::default()
        }],
    );
    validate_config(&config).unwrap();
}

#[test]
fn drive_count_exceeding_24_rejected() {
    let mut config = valid_config();
    for i in 0..25 {
        config
            .drives
            .insert(format!("d{i}"), DriveConfig { size: "1G".into() });
    }
    let err = validate_config(&config).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("too many drives"), "expected drive count error: {msg}");
    assert!(msg.contains("25"), "error should mention count: {msg}");
}

#[test]
fn invalid_drive_size_format_rejected() {
    let mut config = valid_config();
    config
        .drives
        .insert("bad".into(), DriveConfig { size: "20X".into() });
    assert!(validate_config(&config).is_err());
}

#[test]
fn invalid_hostname_rejected() {
    for hostname in ["-bad", "bad-", ".bad", "bad.", "hello world", "a@b", "a/b"] {
        let mut config = valid_config();
        config.network.hostname = hostname.into();
        assert!(
            validate_config(&config).is_err(),
            "expected hostname '{}' to be rejected",
            hostname
        );
    }
}

#[test]
fn valid_hostname_passes() {
    for hostname in ["myvm", "my-vm", "vm.example.com", "a", "VM-01"] {
        let mut config = valid_config();
        config.network.hostname = hostname.into();
        validate_config(&config).unwrap();
    }
}

#[test]
fn parse_config_with_ports() {
    let toml = r#"
[image]
base = "ubuntu.img"

[resources]
cpus = 1
memory_mb = 512

[[ports]]
host = 8080
guest = 80

[[ports]]
host = 5432
guest = 5432
bind = "0.0.0.0"
"#;
    let config: Config = facet_toml::from_str(toml).unwrap();
    validate_config(&config).unwrap();
    assert_eq!(config.ports.len(), 2);
    assert_eq!(config.ports[0].host, 8080);
    assert_eq!(config.ports[0].guest, 80);
    assert_eq!(config.ports[0].bind_addr(), "127.0.0.1");
    assert_eq!(config.ports[1].host, 5432);
    assert_eq!(config.ports[1].guest, 5432);
    assert_eq!(config.ports[1].bind_addr(), "0.0.0.0");
}

#[test]
fn port_forward_zero_host_rejected() {
    let mut config = valid_config();
    config.ports = vec![PortForward {
        host: 0,
        guest: 80,
        ..Default::default()
    }];
    assert!(validate_config(&config).is_err());
}

#[test]
fn port_forward_zero_guest_rejected() {
    let mut config = valid_config();
    config.ports = vec![PortForward {
        host: 8080,
        guest: 0,
        ..Default::default()
    }];
    assert!(validate_config(&config).is_err());
}

#[test]
fn port_forward_duplicate_host_rejected() {
    let mut config = valid_config();
    config.ports = vec![
        PortForward {
            host: 8080,
            guest: 80,
            ..Default::default()
        },
        PortForward {
            host: 8080,
            guest: 443,
            ..Default::default()
        },
    ];
    assert!(validate_config(&config).is_err());
}

#[test]
fn port_forward_same_host_different_bind_ok() {
    let mut config = valid_config();
    config.ports = vec![
        PortForward {
            host: 8080,
            guest: 80,
            bind: "127.0.0.1".into(),
        },
        PortForward {
            host: 8080,
            guest: 443,
            bind: "0.0.0.0".into(),
        },
    ];
    validate_config(&config).unwrap();
}
