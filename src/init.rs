use std::path::PathBuf;

use inquire::{Confirm, CustomType, Select, Text};
use inquire::validator::Validation;

use crate::config::sanitize_tag;
use crate::error::RumError;
use crate::registry;
use crate::util::parse_size;

// ── wizard state ─────────────────────────────────────────

struct WizardConfig {
    image_url: String,
    image_comment: Option<String>,
    cpus: u32,
    memory_mb: u64,
    disk: String,
    hostname: String,
    nat: bool,
    interfaces: Vec<WizardInterface>,
    mounts: Vec<WizardMount>,
    drives: Vec<WizardDrive>,
    filesystems: Vec<WizardFs>,
}

struct WizardInterface {
    network: String,
    ip: String,
}

struct WizardMount {
    source: String,
    target: String,
    readonly: bool,
    tag: String,
}

struct WizardDrive {
    name: String,
    size: String,
}

struct WizardFs {
    fs_type: String,
    drives: Vec<String>,
    mount_target: String,
    pool: String,
}

// ── public entry point ───────────────────────────────────

pub fn run(defaults: bool) -> Result<(), RumError> {
    let output_path = PathBuf::from("rum.toml");

    if output_path.exists() {
        if defaults {
            return Err(RumError::Validation {
                message: "rum.toml already exists (use interactive mode to overwrite)".into(),
            });
        }
        let overwrite = Confirm::new("rum.toml already exists. Overwrite?")
            .with_default(false)
            .prompt()
            .map_err(map_inquire_err)?;
        if !overwrite {
            println!("Cancelled.");
            return Ok(());
        }
    }

    let config = if defaults {
        default_config()
    } else {
        run_wizard()?
    };

    let toml = generate_toml(&config);
    std::fs::write(&output_path, &toml).map_err(|e| RumError::ConfigWrite {
        path: output_path.display().to_string(),
        source: e,
    })?;

    println!("Created rum.toml");
    println!("Run `rum up` to start the VM.");
    Ok(())
}

// ── defaults ─────────────────────────────────────────────

fn default_config() -> WizardConfig {
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

// ── wizard step navigation ───────────────────────────────

enum WizardStep {
    OsImage,
    Resources,
    Hostname,
    Network,
    Mounts,
    Storage,
    Done,
}

impl WizardStep {
    fn next(&self) -> Self {
        match self {
            Self::OsImage => Self::Resources,
            Self::Resources => Self::Hostname,
            Self::Hostname => Self::Network,
            Self::Network => Self::Mounts,
            Self::Mounts => Self::Storage,
            Self::Storage | Self::Done => Self::Done,
        }
    }

    fn prev(&self) -> Self {
        match self {
            Self::OsImage => Self::OsImage,
            Self::Resources => Self::OsImage,
            Self::Hostname => Self::Resources,
            Self::Network => Self::Hostname,
            Self::Mounts => Self::Network,
            Self::Storage => Self::Mounts,
            Self::Done => Self::Storage,
        }
    }
}

// ── interactive wizard ───────────────────────────────────

fn run_wizard() -> Result<WizardConfig, RumError> {
    println!();

    detect_backend()?;

    let mut image_url = String::new();
    let mut image_comment = None;
    let mut cpus = 2u32;
    let mut memory_mb = 2048u64;
    let mut disk = "20G".to_string();
    let mut hostname = String::new();
    let mut nat = true;
    let mut interfaces = Vec::new();
    let mut mounts = Vec::new();
    let mut drives = Vec::new();
    let mut filesystems = Vec::new();

    let mut step = WizardStep::OsImage;

    loop {
        match step {
            WizardStep::OsImage => match prompt_os_image() {
                Ok((url, comment)) => {
                    image_url = url;
                    image_comment = comment;
                    step = step.next();
                }
                Err(RumError::InitCancelled) => return Err(RumError::InitCancelled),
                Err(e) => return Err(e),
            },
            WizardStep::Resources => match prompt_resources() {
                Ok((c, m, d)) => {
                    cpus = c;
                    memory_mb = m;
                    disk = d;
                    step = step.next();
                }
                Err(RumError::InitCancelled) => step = step.prev(),
                Err(e) => return Err(e),
            },
            WizardStep::Hostname => match prompt_hostname() {
                Ok(h) => {
                    hostname = h;
                    step = step.next();
                }
                Err(RumError::InitCancelled) => step = step.prev(),
                Err(e) => return Err(e),
            },
            WizardStep::Network => match prompt_network() {
                Ok((n, ifaces)) => {
                    nat = n;
                    interfaces = ifaces;
                    step = step.next();
                }
                Err(RumError::InitCancelled) => step = step.prev(),
                Err(e) => return Err(e),
            },
            WizardStep::Mounts => match prompt_mounts() {
                Ok(m) => {
                    mounts = m;
                    step = step.next();
                }
                Err(RumError::InitCancelled) => step = step.prev(),
                Err(e) => return Err(e),
            },
            WizardStep::Storage => match prompt_storage() {
                Ok((d, fs)) => {
                    drives = d;
                    filesystems = fs;
                    step = step.next();
                }
                Err(RumError::InitCancelled) => step = step.prev(),
                Err(e) => return Err(e),
            },
            WizardStep::Done => break,
        }
    }

    Ok(WizardConfig {
        image_url,
        image_comment,
        cpus,
        memory_mb,
        disk,
        hostname,
        nat,
        interfaces,
        mounts,
        drives,
        filesystems,
    })
}

// ── wizard steps ─────────────────────────────────────────

fn detect_backend() -> Result<(), RumError> {
    let kvm_available = std::path::Path::new("/dev/kvm").exists();

    let libvirt_available = {
        virt::error::clear_error_callback();
        virt::connect::Connect::open(Some("qemu:///system"))
            .map(|mut c| {
                let _ = c.close();
            })
            .is_ok()
    };

    if kvm_available && libvirt_available {
        println!("  Detected: KVM + libvirt (qemu:///system)");
    } else {
        if !kvm_available {
            println!("  Warning: KVM not available (/dev/kvm not found)");
        }
        if !libvirt_available {
            println!("  Warning: Cannot connect to libvirt (qemu:///system)");
        }
        let proceed = Confirm::new("Continue anyway?")
            .with_default(false)
            .prompt()
            .map_err(map_inquire_err)?;
        if !proceed {
            return Err(RumError::InitCancelled);
        }
    }

    println!();
    Ok(())
}

fn prompt_os_image() -> Result<(String, Option<String>), RumError> {
    let presets = registry::preset_labels_and_urls();
    let mut labels: Vec<String> = presets.iter().map(|(label, _)| label.to_string()).collect();
    labels.push("Custom URL".to_string());

    let choice = Select::new("OS image:", labels)
        .with_help_message("Choose a cloud image or enter a custom URL")
        .prompt()
        .map_err(map_inquire_err)?;

    if choice == "Custom URL" {
        let url = Text::new("Image URL:")
            .with_validator(|input: &str| {
                if input.starts_with("http://") || input.starts_with("https://") {
                    Ok(Validation::Valid)
                } else {
                    Ok(Validation::Invalid(
                        "URL must start with http:// or https://".into(),
                    ))
                }
            })
            .prompt()
            .map_err(map_inquire_err)?;
        Ok((url, None))
    } else {
        let (label, url) = presets.iter().find(|(l, _)| *l == choice).unwrap();
        Ok((url.to_string(), Some(label.to_string())))
    }
}

fn prompt_resources() -> Result<(u32, u64, String), RumError> {
    let cpus: u32 = CustomType::new("CPUs:")
        .with_default(2)
        .with_help_message("Number of virtual CPUs (minimum 1)")
        .with_error_message("Please enter a valid number")
        .with_validator(|val: &u32| {
            if *val >= 1 {
                Ok(Validation::Valid)
            } else {
                Ok(Validation::Invalid("Must be at least 1".into()))
            }
        })
        .prompt()
        .map_err(map_inquire_err)?;

    let memory_input = Text::new("Memory:")
        .with_default("2G")
        .with_help_message("e.g. '2G', '512M', '4096M'")
        .with_validator(|input: &str| match parse_size(input) {
            Ok(bytes) => {
                let mb = bytes / (1024 * 1024);
                if mb >= 256 {
                    Ok(Validation::Valid)
                } else {
                    Ok(Validation::Invalid("Must be at least 256M".into()))
                }
            }
            Err(e) => Ok(Validation::Invalid(format!("{e}").into())),
        })
        .prompt()
        .map_err(map_inquire_err)?;

    let memory_mb = parse_size(&memory_input).unwrap() / (1024 * 1024);

    let disk = Text::new("Root disk size:")
        .with_default("20G")
        .with_help_message("e.g. '20G', '50G' — cloud image root partition grows to this size")
        .with_validator(|input: &str| match parse_size(input) {
            Ok(_) => Ok(Validation::Valid),
            Err(e) => Ok(Validation::Invalid(format!("{e}").into())),
        })
        .prompt()
        .map_err(map_inquire_err)?;

    Ok((cpus, memory_mb, disk))
}

fn prompt_hostname() -> Result<String, RumError> {
    Text::new("Hostname:")
        .with_help_message("Leave empty to use the VM name (derived from config filename)")
        .prompt()
        .map_err(map_inquire_err)
}

fn prompt_network() -> Result<(bool, Vec<WizardInterface>), RumError> {
    let nat = Confirm::new("Enable NAT networking?")
        .with_default(true)
        .with_help_message("Gives the VM internet access via the default libvirt network")
        .prompt()
        .map_err(map_inquire_err)?;

    let mut interfaces = Vec::new();

    loop {
        let add = Confirm::new("Add a host-only network interface?")
            .with_default(false)
            .with_help_message("Private network between host and VM")
            .prompt()
            .map_err(map_inquire_err)?;

        if !add {
            break;
        }

        let network = Text::new("  Network name:")
            .with_default("rum-hostonly")
            .with_help_message("Libvirt network name (created automatically if it doesn't exist)")
            .with_validator(|input: &str| {
                if input.is_empty() {
                    Ok(Validation::Invalid("Network name cannot be empty".into()))
                } else {
                    Ok(Validation::Valid)
                }
            })
            .prompt()
            .map_err(map_inquire_err)?;

        let ip = Text::new("  Static IP (optional):")
            .with_help_message("e.g. '192.168.50.10' — leave empty for DHCP")
            .prompt()
            .map_err(map_inquire_err)?;

        interfaces.push(WizardInterface { network, ip });
    }

    Ok((nat, interfaces))
}

fn prompt_mounts() -> Result<Vec<WizardMount>, RumError> {
    let mut mounts = Vec::new();

    let mount_cwd = Confirm::new("Mount current directory into the VM?")
        .with_default(true)
        .with_help_message("Mounts the working directory at /mnt/project via virtiofs")
        .prompt()
        .map_err(map_inquire_err)?;

    if mount_cwd {
        mounts.push(WizardMount {
            source: ".".to_string(),
            target: "/mnt/project".to_string(),
            readonly: false,
            tag: "project".to_string(),
        });
    }

    loop {
        let add_more = Confirm::new("Add another mount?")
            .with_default(false)
            .prompt()
            .map_err(map_inquire_err)?;

        if !add_more {
            break;
        }

        let source = Text::new("  Source path:")
            .with_help_message(r#""." = config dir, "git" = repo root, or absolute path"#)
            .prompt()
            .map_err(map_inquire_err)?;

        let target = Text::new("  Target path in VM:")
            .with_validator(|input: &str| {
                if input.starts_with('/') {
                    Ok(Validation::Valid)
                } else {
                    Ok(Validation::Invalid("Must be an absolute path".into()))
                }
            })
            .prompt()
            .map_err(map_inquire_err)?;

        let readonly = Confirm::new("  Read-only?")
            .with_default(false)
            .prompt()
            .map_err(map_inquire_err)?;

        let default_tag = sanitize_tag(&target);
        let tag = Text::new("  Tag:")
            .with_default(&default_tag)
            .with_help_message("Unique identifier for this mount")
            .prompt()
            .map_err(map_inquire_err)?;

        mounts.push(WizardMount {
            source,
            target,
            readonly,
            tag,
        });
    }

    Ok(mounts)
}

fn prompt_storage() -> Result<(Vec<WizardDrive>, Vec<WizardFs>), RumError> {
    let mut drives = Vec::new();
    let mut filesystems = Vec::new();

    loop {
        let add = Confirm::new("Add a filesystem/drive?")
            .with_default(false)
            .with_help_message("Additional virtual disks with optional auto-format and mount")
            .prompt()
            .map_err(map_inquire_err)?;

        if !add {
            break;
        }

        let fs_options = vec!["ext4", "xfs", "zfs", "btrfs", "none (raw drive)"];
        let fs_choice = Select::new("  Filesystem type:", fs_options)
            .prompt()
            .map_err(map_inquire_err)?;

        let multi_drive = matches!(fs_choice, "zfs" | "btrfs");

        // Collect drive(s) for this filesystem
        let mut fs_drive_names = Vec::new();
        loop {
            let drive = prompt_drive(&drives)?;
            fs_drive_names.push(drive.name.clone());
            drives.push(drive);

            if !multi_drive {
                break;
            }

            let another = Confirm::new("  Add another drive to this filesystem?")
                .with_default(false)
                .prompt()
                .map_err(map_inquire_err)?;

            if !another {
                break;
            }
        }

        if fs_choice == "none (raw drive)" {
            continue;
        }

        let default_target = format!("/mnt/{}", fs_drive_names[0]);
        let mount_target = Text::new("  Mount point:")
            .with_default(&default_target)
            .with_validator(|input: &str| {
                if input.starts_with('/') {
                    Ok(Validation::Valid)
                } else {
                    Ok(Validation::Invalid("Must be an absolute path".into()))
                }
            })
            .prompt()
            .map_err(map_inquire_err)?;

        let pool = if fs_choice == "zfs" {
            Text::new("  ZFS pool name:")
                .with_default(&format!("{}pool", fs_drive_names[0]))
                .prompt()
                .map_err(map_inquire_err)?
        } else {
            String::new()
        };

        filesystems.push(WizardFs {
            fs_type: fs_choice.to_string(),
            drives: fs_drive_names,
            mount_target,
            pool,
        });
    }

    Ok((drives, filesystems))
}

fn prompt_drive(existing: &[WizardDrive]) -> Result<WizardDrive, RumError> {
    let existing_names: Vec<String> = existing.iter().map(|d| d.name.clone()).collect();

    let name = Text::new("  Drive name:")
        .with_help_message("e.g. 'data', 'scratch'")
        .with_validator(move |input: &str| {
            if input.is_empty() {
                Ok(Validation::Invalid("Name cannot be empty".into()))
            } else if !input
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            {
                Ok(Validation::Invalid(
                    "Use only alphanumeric, dash, or underscore".into(),
                ))
            } else if existing_names.iter().any(|n| n == input) {
                Ok(Validation::Invalid("Drive name already used".into()))
            } else {
                Ok(Validation::Valid)
            }
        })
        .prompt()
        .map_err(map_inquire_err)?;

    let size = Text::new("  Size:")
        .with_default("10G")
        .with_help_message("e.g. '10G', '500M', '1T'")
        .with_validator(|input: &str| match parse_size(input) {
            Ok(_) => Ok(Validation::Valid),
            Err(e) => Ok(Validation::Invalid(format!("{e}").into())),
        })
        .prompt()
        .map_err(map_inquire_err)?;

    Ok(WizardDrive { name, size })
}

// ── TOML generation ──────────────────────────────────────

fn generate_toml(config: &WizardConfig) -> String {
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

// ── error mapping ────────────────────────────────────────

fn map_inquire_err(e: inquire::InquireError) -> RumError {
    match e {
        inquire::InquireError::OperationCanceled | inquire::InquireError::OperationInterrupted => {
            RumError::InitCancelled
        }
        other => RumError::Validation {
            message: format!("prompt error: {other}"),
        },
    }
}

// ── tests ────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

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
