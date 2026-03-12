use crate::registry;
use super::model::WizardConfig;

pub(super) fn default_config() -> WizardConfig {
    let presets = registry::preset_labels_and_urls();
    WizardConfig {
        image_url: presets[0].1.to_string(),
        image_comment: Some(presets[0].0.to_string()),
        cpus: 2,
        memory_mb: 2048,
        disk: "20G".into(),
        hostname: String::new(),
        nat: true,
        interfaces: vec![],
        mounts: vec![],
        drives: vec![],
        filesystems: vec![],
    }
}

pub(super) fn generate_toml(config: &WizardConfig) -> String {
    let mut out = String::new();

    // [image]
    if let Some(ref comment) = config.image_comment {
        out.push_str(&format!("# {comment}\n"));
    }
    out.push_str("[image]\n");
    out.push_str(&format!("base = \"{}\"\n", config.image_url));
    out.push('\n');

    // [resources]
    out.push_str("[resources]\n");
    out.push_str(&format!("cpus = {}\n", config.cpus));
    out.push_str(&format!("memory_mb = {}\n", config.memory_mb));
    out.push_str(&format!("disk = \"{}\"\n", config.disk));
    out.push('\n');

    // [network]
    let has_network = !config.nat || !config.hostname.is_empty() || !config.interfaces.is_empty();
    if has_network {
        out.push_str("[network]\n");
        if !config.nat {
            out.push_str("nat = false\n");
        }
        if !config.hostname.is_empty() {
            out.push_str(&format!("hostname = \"{}\"\n", config.hostname));
        }
        out.push('\n');

        for iface in &config.interfaces {
            out.push_str("[[network.interfaces]]\n");
            out.push_str(&format!("network = \"{}\"\n", iface.network));
            if !iface.ip.is_empty() {
                out.push_str(&format!("ip = \"{}\"\n", iface.ip));
            }
            out.push('\n');
        }
    }

    // [[mounts]]
    for m in &config.mounts {
        out.push_str("[[mounts]]\n");
        out.push_str(&format!("source = \"{}\"\n", m.source));
        out.push_str(&format!("target = \"{}\"\n", m.target));
        if m.readonly {
            out.push_str("readonly = true\n");
        }
        if !m.tag.is_empty() {
            out.push_str(&format!("tag = \"{}\"\n", m.tag));
        }
        out.push('\n');
    }

    // [drives.*]
    for d in &config.drives {
        out.push_str(&format!("[drives.{}]\n", d.name));
        out.push_str(&format!("size = \"{}\"\n", d.size));
        out.push('\n');
    }

    // [[fs.*]]
    for fs in &config.filesystems {
        out.push_str(&format!("[[fs.{}]]\n", fs.fs_type));
        if fs.drives.len() == 1 {
            out.push_str(&format!("drive = \"{}\"\n", fs.drives[0]));
        } else {
            let quoted: Vec<String> = fs.drives.iter().map(|d| format!("\"{d}\"")).collect();
            out.push_str(&format!("drives = [{}]\n", quoted.join(", ")));
        }
        out.push_str(&format!("target = \"{}\"\n", fs.mount_target));
        if !fs.pool.is_empty() {
            out.push_str(&format!("pool = \"{}\"\n", fs.pool));
        }
        out.push('\n');
    }

    // commented-out hints
    if config.mounts.is_empty() {
        out.push_str("# [[mounts]]\n");
        out.push_str("# source = \".\"\n");
        out.push_str("# target = \"/mnt/project\"\n");
        out.push('\n');
    }

    out.push_str("# [provision.system]\n");
    out.push_str("# script = \"apt-get update && apt-get install -y <packages>\"\n");
    out.push_str("#\n");
    out.push_str("# [provision.boot]\n");
    out.push_str("# script = \"echo booted\"\n");
    out.push_str("#\n");
    out.push_str("# [advanced]\n");
    out.push_str("# autologin = false\n");

    out
}

// ── tests ────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::model::{WizardDrive, WizardFs, WizardInterface, WizardMount};

    #[test]
    fn generate_toml_default_round_trips() {
        let config = default_config();
        let toml = generate_toml(&config);

        let presets = registry::preset_labels_and_urls();
        // Must parse back as a valid rum Config
        let parsed: crate::config::Config = facet_toml::from_str(&toml).unwrap();
        assert_eq!(parsed.image.base, presets[0].1);
        assert_eq!(parsed.resources.cpus, 2);
        assert_eq!(parsed.resources.memory_mb, 2048);
    }

    #[test]
    fn generate_toml_with_hostname() {
        let config = WizardConfig {
            hostname: "devbox".into(),
            ..default_config()
        };
        let toml = generate_toml(&config);
        assert!(toml.contains("[network]\n"));
        assert!(toml.contains("hostname = \"devbox\""));
    }

    #[test]
    fn generate_toml_no_hostname_omits_network() {
        let config = default_config();
        let toml = generate_toml(&config);
        assert!(!toml.contains("[network]"));
    }

    #[test]
    fn generate_toml_with_mounts() {
        let config = WizardConfig {
            mounts: vec![WizardMount {
                source: ".".into(),
                target: "/mnt/project".into(),
                readonly: false,
                tag: "project".into(),
            }],
            ..default_config()
        };
        let toml = generate_toml(&config);
        assert!(toml.contains("[[mounts]]"));
        assert!(toml.contains("source = \".\""));
        assert!(toml.contains("target = \"/mnt/project\""));
        assert!(toml.contains("tag = \"project\""));
        // Should not have commented-out mount hint
        assert!(!toml.contains("# [[mounts]]"));
    }

    #[test]
    fn generate_toml_with_drives_and_fs() {
        let config = WizardConfig {
            drives: vec![WizardDrive {
                name: "data".into(),
                size: "10G".into(),
            }],
            filesystems: vec![WizardFs {
                fs_type: "ext4".into(),
                drives: vec!["data".into()],
                mount_target: "/mnt/data".into(),
                pool: String::new(),
            }],
            ..default_config()
        };
        let toml = generate_toml(&config);
        assert!(toml.contains("[drives.data]"));
        assert!(toml.contains("size = \"10G\""));
        assert!(toml.contains("[[fs.ext4]]"));
        assert!(toml.contains("drive = \"data\""));
        assert!(toml.contains("target = \"/mnt/data\""));

        // Round-trip
        let parsed: crate::config::Config = facet_toml::from_str(&toml).unwrap();
        assert!(parsed.drives.contains_key("data"));
    }

    #[test]
    fn generate_toml_zfs_multi_drive() {
        let config = WizardConfig {
            drives: vec![
                WizardDrive { name: "log1".into(), size: "50G".into() },
                WizardDrive { name: "log2".into(), size: "50G".into() },
            ],
            filesystems: vec![WizardFs {
                fs_type: "zfs".into(),
                drives: vec!["log1".into(), "log2".into()],
                mount_target: "/mnt/logs".into(),
                pool: "logspool".into(),
            }],
            ..default_config()
        };
        let toml = generate_toml(&config);
        assert!(toml.contains("[drives.log1]"));
        assert!(toml.contains("[drives.log2]"));
        assert!(toml.contains("[[fs.zfs]]"));
        assert!(toml.contains(r#"drives = ["log1", "log2"]"#));
        assert!(toml.contains("pool = \"logspool\""));

        // Round-trip
        let parsed: crate::config::Config = facet_toml::from_str(&toml).unwrap();
        assert_eq!(parsed.fs["zfs"][0].drives, vec!["log1", "log2"]);
    }

    #[test]
    fn generate_toml_readonly_mount() {
        let config = WizardConfig {
            mounts: vec![WizardMount {
                source: "/host/path".into(),
                target: "/mnt/shared".into(),
                readonly: true,
                tag: "shared".into(),
            }],
            ..default_config()
        };
        let toml = generate_toml(&config);
        assert!(toml.contains("readonly = true"));
    }

    #[test]
    fn generate_toml_nat_disabled() {
        let config = WizardConfig {
            nat: false,
            ..default_config()
        };
        let toml = generate_toml(&config);
        assert!(toml.contains("[network]\n"));
        assert!(toml.contains("nat = false"));
    }

    #[test]
    fn generate_toml_with_interfaces() {
        let config = WizardConfig {
            interfaces: vec![
                WizardInterface {
                    network: "rum-hostonly".into(),
                    ip: "192.168.50.10".into(),
                },
                WizardInterface {
                    network: "dev-net".into(),
                    ip: String::new(),
                },
            ],
            ..default_config()
        };
        let toml = generate_toml(&config);
        assert!(toml.contains("[[network.interfaces]]"));
        assert!(toml.contains("network = \"rum-hostonly\""));
        assert!(toml.contains("ip = \"192.168.50.10\""));
        assert!(toml.contains("network = \"dev-net\""));
        // Second interface has no IP — should not emit ip line
        let dev_net_section = toml.split("network = \"dev-net\"").nth(1).unwrap();
        assert!(!dev_net_section.starts_with("\nip ="));

        // Round-trip
        let parsed: crate::config::Config = facet_toml::from_str(&toml).unwrap();
        assert_eq!(parsed.network.interfaces.len(), 2);
    }

    #[test]
    fn generate_toml_raw_drive_no_fs() {
        let config = WizardConfig {
            drives: vec![WizardDrive {
                name: "raw".into(),
                size: "50G".into(),
            }],
            ..default_config()
        };
        let toml = generate_toml(&config);
        assert!(toml.contains("[drives.raw]"));
        assert!(toml.contains("size = \"50G\""));
        assert!(!toml.contains("[[fs."));
    }
}
