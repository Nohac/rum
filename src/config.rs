use facet::Facet;
use std::path::Path;

use crate::error::RumError;

#[derive(Debug, Clone, Facet)]
pub struct Config {
    pub name: String,
    pub image: ImageConfig,
    pub resources: ResourcesConfig,
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

impl Config {
    fn validate(&self) -> Result<(), RumError> {
        if self.name.is_empty() {
            return Err(RumError::Validation {
                message: "name must not be empty".into(),
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

    let config: Config =
        facet_toml::from_str(&contents).map_err(|e| RumError::ConfigParse {
            path: path.display().to_string(),
            message: e.to_string(),
        })?;

    config.validate()?;
    Ok(config)
}
