use facet::Facet;
use std::path::Path;

use crate::error::RumError;

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

#[derive(Debug, Clone, Facet)]
#[facet(default)]
pub struct NetworkConfig {
    #[facet(default)]
    pub hostname: String,
    #[facet(default = true)]
    pub wait_for_ip: bool,
    #[facet(default = 120)]
    pub ip_wait_timeout_s: u64,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            hostname: String::new(),
            wait_for_ip: true,
            ip_wait_timeout_s: 120,
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
        Ok(())
    }
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
}
