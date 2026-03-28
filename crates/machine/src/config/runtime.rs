use std::path::{Path, PathBuf};

use crate::error::Error;
use crate::paths;

use super::identity::sanitize_tag;
use super::schema::*;

#[derive(Debug, Clone)]
pub struct ResolvedMount {
    pub source: PathBuf,
    pub target: String,
    pub readonly: bool,
    pub tag: String,
    pub default: bool,
}

#[derive(Debug, Clone)]
pub struct ResolvedDrive {
    pub name: String,
    pub size: String,
    pub path: PathBuf,
    pub dev: String,
}

#[derive(Debug, Clone, Hash)]
pub enum ResolvedFs {
    Zfs(ZfsFs),
    Btrfs(BtrfsFs),
    Simple(SimpleFs),
}

#[derive(Debug, Clone, Hash)]
pub struct ZfsFs {
    pub pool: String,
    pub devs: Vec<String>,
    pub target: String,
    pub mode: Option<String>,
}

#[derive(Debug, Clone, Hash)]
pub struct BtrfsFs {
    pub devs: Vec<String>,
    pub target: String,
    pub mode: Option<String>,
}

#[derive(Debug, Clone, Hash)]
pub struct SimpleFs {
    pub filesystem: String,
    pub dev: String,
    pub target: String,
}

/// Resolved runtime config combining the parsed TOML with path-derived identity.
#[derive(Debug, Clone)]
pub struct SystemConfig {
    /// 8-hex-char hash of canonicalized config path + name.
    pub id: String,
    /// Derived from filename: `dev.rum.toml` → Some("dev"), `rum.toml` → None.
    pub name: Option<String>,
    /// Canonicalized path to the config file.
    pub config_path: PathBuf,
    /// Parsed TOML config.
    pub config: Config,
}

impl SystemConfig {
    /// User-facing display name: the derived name if present, otherwise the id.
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.id)
    }

    /// Resolved hostname — falls back to display_name if not set.
    pub fn hostname(&self) -> &str {
        if self.config.network.hostname.is_empty() {
            self.display_name()
        } else {
            &self.config.network.hostname
        }
    }

    /// Resolved libvirt URI.
    pub fn libvirt_uri(&self) -> &str {
        &self.config.advanced.libvirt_uri
    }

    /// Resolve drive configs into paths and device names.
    ///
    /// BTreeMap iteration is sorted by key, so device names are assigned
    /// in alphabetical order of drive names: first drive → vdb, second → vdc, etc.
    /// (vda is reserved for the root overlay disk.)
    pub fn resolve_drives(&self) -> Result<Vec<ResolvedDrive>, Error> {
        let mut resolved = Vec::new();
        for (i, (name, drive)) in self.config.drives.iter().enumerate() {
            let dev = format!("vd{}", (b'b' + i as u8) as char);
            resolved.push(ResolvedDrive {
                name: name.clone(),
                size: drive.size.clone(),
                path: paths::drive_path(&self.id, self.name.as_deref(), name),
                dev,
            });
        }
        Ok(resolved)
    }

    /// Resolve mount sources relative to the config file path.
    pub fn resolve_mounts(&self) -> Result<Vec<ResolvedMount>, Error> {
        let parent = self.config_path.parent().unwrap_or(Path::new("."));
        let parent = if parent.as_os_str().is_empty() {
            Path::new(".")
        } else {
            parent
        };
        let config_dir = parent.canonicalize().map_err(|e| Error::Io {
            context: format!("canonicalizing config dir {}", parent.display()),
            source: e,
        })?;

        let default_count = self.config.mounts.iter().filter(|m| m.default).count();
        if default_count > 1 {
            return Err(Error::Validation {
                message: "at most one mount may have default = true".into(),
            });
        }

        let mut resolved = Vec::new();
        let mut seen_tags = std::collections::HashSet::new();

        for m in &self.config.mounts {
            let source = match m.source.as_str() {
                "." => config_dir.clone(),
                "git" => {
                    let output = std::process::Command::new("git")
                        .args(["rev-parse", "--show-toplevel"])
                        .current_dir(&config_dir)
                        .output()
                        .map_err(|e| Error::GitRepoDetection {
                            message: format!("failed to run git: {e}"),
                        })?;
                    if !output.status.success() {
                        return Err(Error::GitRepoDetection {
                            message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
                        });
                    }
                    PathBuf::from(String::from_utf8_lossy(&output.stdout).trim())
                }
                other => {
                    let p = Path::new(other);
                    if p.is_absolute() {
                        p.to_path_buf()
                    } else {
                        config_dir.join(p)
                    }
                }
            };

            if !source.is_dir() {
                return Err(Error::MountSourceNotFound {
                    path: source.display().to_string(),
                });
            }

            let tag = if m.tag.is_empty() {
                sanitize_tag(&m.target)
            } else {
                m.tag.clone()
            };

            if !seen_tags.insert(tag.clone()) {
                return Err(Error::Validation {
                    message: format!("duplicate mount tag '{tag}'"),
                });
            }

            resolved.push(ResolvedMount {
                source,
                target: m.target.clone(),
                readonly: m.readonly,
                tag,
                default: m.default,
            });
        }

        Ok(resolved)
    }

    /// Resolve filesystem entries by mapping drive names to device paths.
    ///
    /// Must be called after `resolve_drives()` — uses the resolved drives
    /// to look up device names (vdb, vdc, ...).
    pub fn resolve_fs(&self, drives: &[ResolvedDrive]) -> Result<Vec<ResolvedFs>, Error> {
        let drive_map: std::collections::HashMap<&str, &str> = drives
            .iter()
            .map(|d| (d.name.as_str(), d.dev.as_str()))
            .collect();

        let mut resolved = Vec::new();
        for (fs_type, entries) in &self.config.fs {
            for entry in entries {
                match fs_type.as_str() {
                    "zfs" => {
                        let mut devs = Vec::new();
                        for name in &entry.drives {
                            let dev = drive_map.get(name.as_str()).ok_or_else(|| {
                                Error::Validation {
                                    message: format!(
                                        "fs entry references unknown drive '{name}'"
                                    ),
                                }
                            })?;
                            devs.push(format!("/dev/{dev}"));
                        }
                        let pool = if entry.pool.is_empty() {
                            entry.drives[0].clone()
                        } else {
                            entry.pool.clone()
                        };
                        resolved.push(ResolvedFs::Zfs(ZfsFs {
                            pool,
                            devs,
                            target: entry.target.clone(),
                            mode: entry.mode.clone(),
                        }));
                    }
                    "btrfs" => {
                        let mut devs = Vec::new();
                        for name in &entry.drives {
                            let dev = drive_map.get(name.as_str()).ok_or_else(|| {
                                Error::Validation {
                                    message: format!(
                                        "fs entry references unknown drive '{name}'"
                                    ),
                                }
                            })?;
                            devs.push(format!("/dev/{dev}"));
                        }
                        resolved.push(ResolvedFs::Btrfs(BtrfsFs {
                            devs,
                            target: entry.target.clone(),
                            mode: entry.mode.clone(),
                        }));
                    }
                    _ => {
                        let dev_name =
                            drive_map.get(entry.drive.as_str()).ok_or_else(|| {
                                Error::Validation {
                                    message: format!(
                                        "fs entry references unknown drive '{}'",
                                        entry.drive
                                    ),
                                }
                            })?;
                        let dev = format!("/dev/{dev_name}");
                        resolved.push(ResolvedFs::Simple(SimpleFs {
                            filesystem: fs_type.clone(),
                            dev,
                            target: entry.target.clone(),
                        }));
                    }
                }
            }
        }
        Ok(resolved)
    }
}
