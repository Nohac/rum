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

// ── SystemConfig ──────────────────────────────────────────

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
    pub fn resolve_drives(&self) -> Result<Vec<ResolvedDrive>, RumError> {
        let mut resolved = Vec::new();
        for (i, (name, drive)) in self.config.drives.iter().enumerate() {
            let dev = format!("vd{}", (b'b' + i as u8) as char);
            resolved.push(ResolvedDrive {
                name: name.clone(),
                size: drive.size.clone(),
                path: paths::drive_path(&self.id, self.name.as_deref(), name),
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
    pub fn resolve_mounts(&self) -> Result<Vec<ResolvedMount>, RumError> {
        let parent = self.config_path.parent().unwrap_or(Path::new("."));
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

        for m in &self.config.mounts {
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

// ── validation ────────────────────────────────────────────

fn validate_config(config: &Config) -> Result<(), RumError> {
    if config.resources.cpus < 1 {
        return Err(RumError::Validation {
            message: "cpus must be at least 1".into(),
        });
    }
    if config.resources.memory_mb < 256 {
        return Err(RumError::Validation {
            message: "memory_mb must be at least 256".into(),
        });
    }

    // Validate mounts
    for m in &config.mounts {
        if !m.target.starts_with('/') {
            return Err(RumError::Validation {
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
            return Err(RumError::Validation {
                message: format!("duplicate mount tag '{tag}'"),
            });
        }
    }

    // Validate drives
    for (name, drive) in &config.drives {
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
    for iface in &config.network.interfaces {
        if iface.network.is_empty() {
            return Err(RumError::Validation {
                message: "network interface must have a non-empty network name".into(),
            });
        }
    }

    Ok(())
}

fn validate_name(name: &str) -> Result<(), RumError> {
    let valid = !name.is_empty()
        && name.chars().next().unwrap().is_ascii_alphanumeric()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-');
    if !valid {
        return Err(RumError::Validation {
            message: format!("derived name must match [a-zA-Z0-9][a-zA-Z0-9._-]* (got '{name}')"),
        });
    }
    Ok(())
}

// ── helpers ───────────────────────────────────────────────

/// Generate a filesystem tag from a mount target path.
/// E.g. `/mnt/project` → `mnt_project`
fn sanitize_tag(target: &str) -> String {
    target.replace('/', "_").trim_start_matches('_').to_string()
}

/// Derive the VM name from the config filename.
/// `rum.toml` → None, `dev.rum.toml` → Some("dev")
fn derive_name(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?;
    if stem == "rum" {
        return None;
    }
    // For `dev.rum.toml`, file_stem gives `dev.rum`, we want `dev`
    // For `rum.toml`, file_stem gives `rum`, handled above
    let name = stem.strip_suffix(".rum").unwrap_or(stem);
    Some(name.to_string())
}

/// Compute an 8-hex-char ID from the canonicalized config path and optional name.
/// Including the name ensures `rum.toml` and `dev.rum.toml` in the same dir get
/// different IDs.
fn config_id(canonical_path: &Path, name: Option<&str>) -> String {
    let mut hash: u64 = 0xcbf29ce484222325; // FNV-1a offset basis
    for b in canonical_path.to_string_lossy().bytes() {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    if let Some(n) = name {
        for b in n.bytes() {
            hash ^= b as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    format!("{:08x}", hash as u32)
}

// ── public API ────────────────────────────────────────────

pub fn load_config(path: &Path) -> Result<SystemConfig, RumError> {
    let contents = std::fs::read_to_string(path).map_err(|source| RumError::ConfigLoad {
        path: path.display().to_string(),
        source,
    })?;

    let config: Config = facet_toml::from_str(&contents).map_err(|e| RumError::ConfigParse {
        path: path.display().to_string(),
        message: e.to_string(),
    })?;

    validate_config(&config)?;

    let canonical = path.canonicalize().map_err(|source| RumError::ConfigLoad {
        path: path.display().to_string(),
        source,
    })?;

    let name = derive_name(&canonical);
    if let Some(ref n) = name {
        validate_name(n)?;
    }

    let id = config_id(&canonical, name.as_deref());

    Ok(SystemConfig {
        id,
        name,
        config_path: canonical,
        config,
    })
}

#[cfg(test)]
pub mod tests {
    use super::*;

    fn valid_config() -> Config {
        Config {
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
}
