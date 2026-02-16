use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use facet::Facet;

use crate::error::RumError;
use crate::paths;

#[derive(Debug, Clone, Default, Facet)]
#[facet(default)]
pub struct MountConfig {
    pub source: String,
    pub target: String,
    #[facet(default)]
    pub readonly: bool,
    #[facet(default)]
    pub tag: String,
}

#[derive(Debug, Clone)]
pub struct ResolvedMount {
    pub source: PathBuf,
    pub target: String,
    pub readonly: bool,
    pub tag: String,
}

#[derive(Debug, Clone, Default, Facet)]
#[facet(default)]
pub struct DriveConfig {
    pub size: String,
    #[facet(default)]
    pub target: String,
}

#[derive(Debug, Clone)]
pub struct ResolvedDrive {
    pub name: String,
    pub size: String,
    pub path: PathBuf,
    pub target: Option<String>,
    pub dev: String,
}

#[derive(Debug, Clone, Facet)]
pub struct Config {
    pub name: String,
    pub image: ImageConfig,
    pub resources: ResourcesConfig,
    #[facet(default)]
    pub network: NetworkConfig,
    #[facet(default)]
    pub provision: ProvisionConfig,
    #[facet(default)]
    pub advanced: AdvancedConfig,
    #[facet(default)]
    pub mounts: Vec<MountConfig>,
    #[facet(default)]
    pub drives: BTreeMap<String, DriveConfig>,
}

#[derive(Debug, Clone, Facet)]
pub struct ImageConfig {
    pub base: String,
}

#[derive(Debug, Clone, Facet)]
pub struct ResourcesConfig {
    pub cpus: u32,
    pub memory_mb: u64,
}

#[derive(Debug, Clone, Default, Facet)]
#[facet(default)]
pub struct InterfaceConfig {
    pub network: String,
    #[facet(default)]
    pub ip: String,
}

#[derive(Debug, Clone, Facet)]
#[facet(default)]
pub struct NetworkConfig {
    #[facet(default = true)]
    pub nat: bool,
    #[facet(default)]
    pub hostname: String,
    #[facet(default = true)]
    pub wait_for_ip: bool,
    #[facet(default = 120)]
    pub ip_wait_timeout_s: u64,
    #[facet(default)]
    pub interfaces: Vec<InterfaceConfig>,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            nat: true,
            hostname: String::new(),
            wait_for_ip: true,
            ip_wait_timeout_s: 120,
            interfaces: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default, Facet)]
#[facet(default)]
pub struct ProvisionConfig {
    #[facet(default)]
    pub script: String,
    #[facet(default)]
    pub packages: Vec<String>,
}

#[derive(Debug, Clone, Facet)]
#[facet(default)]
pub struct AdvancedConfig {
    #[facet(default = "qemu:///system")]
    pub libvirt_uri: String,
    #[facet(default = "kvm")]
    pub domain_type: String,
    #[facet(default = "q35")]
    pub machine: String,
}

impl Default for AdvancedConfig {
    fn default() -> Self {
        Self {
            libvirt_uri: "qemu:///system".into(),
            domain_type: "kvm".into(),
            machine: "q35".into(),
        }
    }
}

impl Config {
    /// Resolved hostname — falls back to VM name if not set.
    pub fn hostname(&self) -> &str {
        if self.network.hostname.is_empty() {
            &self.name
        } else {
            &self.network.hostname
        }
    }

    /// Resolved libvirt URI.
    pub fn libvirt_uri(&self) -> &str {
        &self.advanced.libvirt_uri
    }

    fn validate(&self) -> Result<(), RumError> {
        let valid_name = !self.name.is_empty()
            && self.name.chars().next().unwrap().is_ascii_alphanumeric()
            && self
                .name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-');
        if !valid_name {
            return Err(RumError::Validation {
                message: format!(
                    "name must match [a-zA-Z0-9][a-zA-Z0-9._-]* (got '{}')",
                    self.name
                ),
            });
        }
        if self.resources.cpus < 1 {
            return Err(RumError::Validation {
                message: "cpus must be at least 1".into(),
            });
        }
        if self.resources.memory_mb < 256 {
            return Err(RumError::Validation {
                message: "memory_mb must be at least 256".into(),
            });
        }

        // Validate mounts
        for m in &self.mounts {
            if !m.target.starts_with('/') {
                return Err(RumError::Validation {
                    message: format!("mount target must be absolute (got '{}')", m.target),
                });
            }
        }

        // Check for duplicate tags (after auto-generation would apply, but we
        // can at least check explicitly set tags here)
        let explicit_tags: Vec<&str> = self
            .mounts
            .iter()
            .filter(|m| !m.tag.is_empty())
            .map(|m| m.tag.as_str())
            .collect();
        for (i, tag) in explicit_tags.iter().enumerate() {
            if explicit_tags[i + 1..].contains(tag) {
                return Err(RumError::Validation {
                    message: format!("duplicate mount tag '{tag}'"),
                });
            }
        }

        // Validate drives
        for (name, drive) in &self.drives {
            if drive.size.is_empty() {
                return Err(RumError::Validation {
                    message: format!("drive '{name}' must have a size"),
                });
            }
            if !drive.target.is_empty() && !drive.target.starts_with('/') {
                return Err(RumError::Validation {
                    message: format!(
                        "drive '{name}' target must be absolute (got '{}')",
                        drive.target
                    ),
                });
            }
        }

        // Validate network interfaces
        for iface in &self.network.interfaces {
            if iface.network.is_empty() {
                return Err(RumError::Validation {
                    message: "network interface must have a non-empty network name".into(),
                });
            }
        }

        Ok(())
    }

    /// Resolve drive configs into paths and device names.
    ///
    /// BTreeMap iteration is sorted by key, so device names are assigned
    /// in alphabetical order of drive names: first drive → vdb, second → vdc, etc.
    /// (vda is reserved for the root overlay disk.)
    pub fn resolve_drives(&self) -> Result<Vec<ResolvedDrive>, RumError> {
        let mut resolved = Vec::new();
        for (i, (name, drive)) in self.drives.iter().enumerate() {
            let dev = format!("vd{}", (b'b' + i as u8) as char);
            resolved.push(ResolvedDrive {
                name: name.clone(),
                size: drive.size.clone(),
                path: paths::drive_path(&self.name, name),
                target: if drive.target.is_empty() {
                    None
                } else {
                    Some(drive.target.clone())
                },
                dev,
            });
        }
        Ok(resolved)
    }

    /// Resolve mount sources relative to the config file path.
    pub fn resolve_mounts(&self, config_path: &Path) -> Result<Vec<ResolvedMount>, RumError> {
        let parent = config_path.parent().unwrap_or(Path::new("."));
        let parent = if parent.as_os_str().is_empty() {
            Path::new(".")
        } else {
            parent
        };
        let config_dir = parent.canonicalize().map_err(|e| RumError::Io {
            context: format!("canonicalizing config dir {}", parent.display()),
            source: e,
        })?;

        let mut resolved = Vec::new();
        let mut seen_tags = std::collections::HashSet::new();

        for m in &self.mounts {
            let source = match m.source.as_str() {
                "." => config_dir.clone(),
                "git" => {
                    let output = std::process::Command::new("git")
                        .args(["rev-parse", "--show-toplevel"])
                        .current_dir(&config_dir)
                        .output()
                        .map_err(|e| RumError::GitRepoDetection {
                            message: format!("failed to run git: {e}"),
                        })?;
                    if !output.status.success() {
                        return Err(RumError::GitRepoDetection {
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
                return Err(RumError::MountSourceNotFound {
                    path: source.display().to_string(),
                });
            }

            let tag = if m.tag.is_empty() {
                sanitize_tag(&m.target)
            } else {
                m.tag.clone()
            };

            if !seen_tags.insert(tag.clone()) {
                return Err(RumError::Validation {
                    message: format!("duplicate mount tag '{tag}'"),
                });
            }

            resolved.push(ResolvedMount {
                source,
                target: m.target.clone(),
                readonly: m.readonly,
                tag,
            });
        }

        Ok(resolved)
    }
}

/// Generate a filesystem tag from a mount target path.
/// E.g. `/mnt/project` → `mnt_project`
fn sanitize_tag(target: &str) -> String {
    target.replace('/', "_").trim_start_matches('_').to_string()
}

pub fn load_config(path: &Path) -> Result<Config, RumError> {
    let contents = std::fs::read_to_string(path).map_err(|source| RumError::ConfigLoad {
        path: path.display().to_string(),
        source,
    })?;

    let config: Config = facet_toml::from_str(&contents).map_err(|e| RumError::ConfigParse {
        path: path.display().to_string(),
        message: e.to_string(),
    })?;

    config.validate()?;
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_config(name: &str) -> Config {
        Config {
            name: name.into(),
            image: ImageConfig {
                base: "https://example.com/image.qcow2".into(),
            },
            resources: ResourcesConfig {
                cpus: 1,
                memory_mb: 512,
            },
            network: NetworkConfig::default(),
            provision: ProvisionConfig::default(),
            advanced: AdvancedConfig::default(),
            mounts: vec![],
            drives: BTreeMap::new(),
        }
    }

    #[test]
    fn valid_names() {
        for name in ["myvm", "test-vm", "vm.dev", "VM_01", "a"] {
            valid_config(name).validate().unwrap();
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
                valid_config(name).validate().is_err(),
                "expected name '{}' to be rejected",
                name
            );
        }
    }

    #[test]
    fn empty_interface_network_rejected() {
        let mut config = valid_config("test");
        config.network.interfaces = vec![InterfaceConfig {
            network: String::new(),
            ip: String::new(),
        }];
        assert!(config.validate().is_err());
    }

    #[test]
    fn valid_interface_config() {
        let mut config = valid_config("test");
        config.network.interfaces = vec![InterfaceConfig {
            network: "rum-hostonly".into(),
            ip: "192.168.50.10".into(),
        }];
        config.validate().unwrap();
    }

    #[test]
    fn parse_config_with_interfaces() {
        let toml = r#"
name = "test-vm"

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
}
