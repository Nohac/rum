use std::path::PathBuf;

use crate::config::SystemConfig;
use crate::paths;

#[derive(Debug, Clone)]
pub struct MachineLayout {
    pub id: String,
    pub name: Option<String>,
    pub display_name: String,
    pub work_dir: PathBuf,
    pub overlay_path: PathBuf,
    pub xml_path: PathBuf,
    pub config_path_file: PathBuf,
    pub ssh_key_path: PathBuf,
    pub logs_dir: PathBuf,
    pub provisioned_marker: PathBuf,
}

impl MachineLayout {
    pub fn from_config(system: &SystemConfig) -> Self {
        let id = system.id.clone();
        let name = system.name.clone();
        let name_opt = system.name.as_deref();

        Self {
            id,
            name,
            display_name: system.display_name().to_string(),
            work_dir: paths::work_dir(&system.id, name_opt),
            overlay_path: paths::overlay_path(&system.id, name_opt),
            xml_path: paths::domain_xml_path(&system.id, name_opt),
            config_path_file: paths::config_path_file(&system.id, name_opt),
            ssh_key_path: paths::ssh_key_path(&system.id, name_opt),
            logs_dir: paths::logs_dir(&system.id, name_opt),
            provisioned_marker: paths::provisioned_marker(&system.id, name_opt),
        }
    }

    pub fn seed_path(&self, hash: &str) -> PathBuf {
        paths::seed_path(&self.id, self.name.as_deref(), hash)
    }
}
