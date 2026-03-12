use inquire::{Confirm, CustomType, Select, Text};
use inquire::validator::Validation;

use crate::config::sanitize_tag;
use crate::error::RumError;
use crate::registry;
use crate::util::parse_size;
use super::errors::map_inquire_err;
use super::model::{WizardDrive, WizardFs, WizardInterface, WizardMount};

pub(super) fn prompt_os_image() -> Result<(String, Option<String>), RumError> {
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

pub(super) fn prompt_resources() -> Result<(u32, u64, String), RumError> {
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

pub(super) fn prompt_hostname() -> Result<String, RumError> {
    Text::new("Hostname:")
        .with_help_message("Leave empty to use the VM name (derived from config filename)")
        .prompt()
        .map_err(map_inquire_err)
}

pub(super) fn prompt_network() -> Result<(bool, Vec<WizardInterface>), RumError> {
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

pub(super) fn prompt_mounts() -> Result<Vec<WizardMount>, RumError> {
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

pub(super) fn prompt_storage() -> Result<(Vec<WizardDrive>, Vec<WizardFs>), RumError> {
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
