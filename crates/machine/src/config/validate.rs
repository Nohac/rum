use crate::error::Error;

use super::schema::*;

pub(super) fn validate_config(config: &Config) -> Result<(), Error> {
    if config.resources.cpus < 1 {
        return Err(Error::Validation {
            message: "cpus must be at least 1".into(),
        });
    }
    if config.resources.memory_mb < 256 {
        return Err(Error::Validation {
            message: "memory_mb must be at least 256".into(),
        });
    }
    if !config.resources.disk.is_empty() {
        crate::util::parse_size(&config.resources.disk)?;
    }

    // Validate mounts
    for m in &config.mounts {
        if !m.target.starts_with('/') {
            return Err(Error::Validation {
                message: format!("mount target must be absolute (got '{}')", m.target),
            });
        }
    }

    // Check for duplicate tags
    let explicit_tags: Vec<&str> = config
        .mounts
        .iter()
        .filter(|m| !m.tag.is_empty())
        .map(|m| m.tag.as_str())
        .collect();
    for (i, tag) in explicit_tags.iter().enumerate() {
        if explicit_tags[i + 1..].contains(tag) {
            return Err(Error::Validation {
                message: format!("duplicate mount tag '{tag}'"),
            });
        }
    }

    // Validate drives
    if config.drives.len() > 24 {
        return Err(Error::Validation {
            message: format!("too many drives (max 24, got {})", config.drives.len()),
        });
    }
    for (name, drive) in &config.drives {
        if drive.size.is_empty() {
            return Err(Error::Validation {
                message: format!("drive '{name}' must have a size"),
            });
        }
        crate::util::parse_size(&drive.size)?;
    }

    // Validate filesystem entries
    let mut used_drives = std::collections::HashSet::new();
    for (fs_type, entries) in &config.fs {
        for (idx, entry) in entries.iter().enumerate() {
            let label = format!("fs.{fs_type}[{idx}]");

            if entry.target.is_empty() {
                return Err(Error::Validation {
                    message: format!("{label}: target is required"),
                });
            }
            if !entry.target.starts_with('/') {
                return Err(Error::Validation {
                    message: format!("{label}: target must be absolute (got '{}')", entry.target),
                });
            }

            match fs_type.as_str() {
                "zfs" => {
                    if entry.drives.is_empty() {
                        return Err(Error::Validation {
                            message: format!("{label}: zfs requires 'drives' (list of drive names)"),
                        });
                    }
                    if !entry.drive.is_empty() {
                        return Err(Error::Validation {
                            message: format!("{label}: zfs uses 'drives', not 'drive'"),
                        });
                    }
                    for d in &entry.drives {
                        if !config.drives.contains_key(d) {
                            return Err(Error::Validation {
                                message: format!("{label}: drive '{d}' not found in [drives]"),
                            });
                        }
                        if !used_drives.insert(d.as_str()) {
                            return Err(Error::Validation {
                                message: format!(
                                    "{label}: drive '{d}' is already used by another fs entry"
                                ),
                            });
                        }
                    }
                }
                "btrfs" => {
                    if entry.drives.is_empty() {
                        return Err(Error::Validation {
                            message: format!(
                                "{label}: btrfs requires 'drives' (list of drive names)"
                            ),
                        });
                    }
                    if !entry.drive.is_empty() {
                        return Err(Error::Validation {
                            message: format!("{label}: btrfs uses 'drives', not 'drive'"),
                        });
                    }
                    if !entry.pool.is_empty() {
                        return Err(Error::Validation {
                            message: format!("{label}: 'pool' is only valid for zfs"),
                        });
                    }
                    for d in &entry.drives {
                        if !config.drives.contains_key(d) {
                            return Err(Error::Validation {
                                message: format!("{label}: drive '{d}' not found in [drives]"),
                            });
                        }
                        if !used_drives.insert(d.as_str()) {
                            return Err(Error::Validation {
                                message: format!(
                                    "{label}: drive '{d}' is already used by another fs entry"
                                ),
                            });
                        }
                    }
                }
                _ => {
                    if entry.drive.is_empty() {
                        return Err(Error::Validation {
                            message: format!(
                                "{label}: '{fs_type}' requires 'drive' (single drive name)"
                            ),
                        });
                    }
                    if !entry.drives.is_empty() {
                        return Err(Error::Validation {
                            message: format!("{label}: '{fs_type}' uses 'drive', not 'drives'"),
                        });
                    }
                    if entry.mode.is_some() {
                        return Err(Error::Validation {
                            message: format!("{label}: 'mode' is only valid for zfs/btrfs"),
                        });
                    }
                    if !entry.pool.is_empty() {
                        return Err(Error::Validation {
                            message: format!("{label}: 'pool' is only valid for zfs"),
                        });
                    }
                    if !config.drives.contains_key(&entry.drive) {
                        return Err(Error::Validation {
                            message: format!(
                                "{label}: drive '{}' not found in [drives]",
                                entry.drive
                            ),
                        });
                    }
                    if !used_drives.insert(entry.drive.as_str()) {
                        return Err(Error::Validation {
                            message: format!(
                                "{label}: drive '{}' is already used by another fs entry",
                                entry.drive
                            ),
                        });
                    }
                }
            }
        }
    }

    // Validate mount target overlap between [[mounts]] and [[fs.*]] entries
    {
        // Collect all (target, source_label) pairs
        let mut targets: Vec<(&str, String)> = Vec::new();
        for m in &config.mounts {
            targets.push((&m.target, "[[mounts]]".into()));
        }
        for (fs_type, entries) in &config.fs {
            for entry in entries {
                targets.push((&entry.target, format!("[[fs.{fs_type}]]")));
            }
        }

        // Check for exact duplicates
        for i in 0..targets.len() {
            for j in (i + 1)..targets.len() {
                if targets[i].0 == targets[j].0 {
                    return Err(Error::Validation {
                        message: format!(
                            "mount target '{}' is used by both {} and {}",
                            targets[i].0, targets[i].1, targets[j].1
                        ),
                    });
                }
            }
        }

        // Check for prefix overlap (parent/child mount points)
        for i in 0..targets.len() {
            for j in 0..targets.len() {
                if i == j {
                    continue;
                }
                let parent = targets[i].0;
                let child = targets[j].0;
                // Check if parent is a prefix of child with a '/' boundary
                if child.len() > parent.len()
                    && child.starts_with(parent)
                    && child.as_bytes()[parent.len()] == b'/'
                {
                    return Err(Error::Validation {
                        message: format!(
                            "mount target '{}' overlaps with '{}' (from {})",
                            child, parent, targets[i].1
                        ),
                    });
                }
            }
        }
    }

    // Validate hostname
    if !config.network.hostname.is_empty() {
        let h = &config.network.hostname;
        if h.len() > 253 {
            return Err(Error::Validation {
                message: format!(
                    "invalid hostname '{}': must contain only alphanumerics, hyphens, and dots",
                    h
                ),
            });
        }
        let valid = h
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
            && !h.starts_with('-')
            && !h.starts_with('.')
            && !h.ends_with('-')
            && !h.ends_with('.');
        if !valid {
            return Err(Error::Validation {
                message: format!(
                    "invalid hostname '{}': must contain only alphanumerics, hyphens, and dots",
                    h
                ),
            });
        }
    }

    // Validate network interfaces
    for iface in &config.network.interfaces {
        if iface.network.is_empty() {
            return Err(Error::Validation {
                message: "network interface must have a non-empty network name".into(),
            });
        }
    }

    // Validate port forwards
    for (i, pf) in config.ports.iter().enumerate() {
        if pf.host == 0 {
            return Err(Error::Validation {
                message: format!("ports[{i}]: host port must be > 0"),
            });
        }
        if pf.guest == 0 {
            return Err(Error::Validation {
                message: format!("ports[{i}]: guest port must be > 0"),
            });
        }
        // Check for duplicate host port + bind combinations
        for j in (i + 1)..config.ports.len() {
            if pf.host == config.ports[j].host && pf.bind_addr() == config.ports[j].bind_addr() {
                return Err(Error::Validation {
                    message: format!(
                        "duplicate port forward: host port {} on {}",
                        pf.host,
                        pf.bind_addr()
                    ),
                });
            }
        }
    }

    Ok(())
}

pub(super) fn validate_name(name: &str) -> Result<(), Error> {
    let valid = !name.is_empty()
        && name.chars().next().unwrap().is_ascii_alphanumeric()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-');
    if !valid {
        return Err(Error::Validation {
            message: format!("derived name must match [a-zA-Z0-9][a-zA-Z0-9._-]* (got '{name}')"),
        });
    }
    Ok(())
}
