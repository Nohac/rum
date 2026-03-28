use std::path::Path;

use crate::error::RumError;

use super::identity::{config_id, derive_name};
use super::runtime::SystemConfig;
use super::schema::Config;
use super::validate::{validate_config, validate_name};

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
