use std::collections::BTreeMap;

use facet::Facet;

#[derive(Debug, Clone, Default, Facet)]
#[facet(default)]
pub struct MountConfig {
    pub source: String,
    pub target: String,
    #[facet(default)]
    pub readonly: bool,
    #[facet(default)]
    pub tag: String,
    #[facet(default)]
    pub default: bool,
}

#[derive(Debug, Clone, Default, Facet)]
#[facet(default)]
pub struct DriveConfig {
    pub size: String,
}

#[derive(Debug, Clone, Default, Facet)]
#[facet(default)]
pub struct FsEntryConfig {
    #[facet(default)]
    pub drive: String,
    #[facet(default)]
    pub drives: Vec<String>,
    #[facet(default)]
    pub target: String,
    pub mode: Option<String>,
    #[facet(default)]
    pub pool: String,
}

#[derive(Debug, Clone, Default, Facet)]
#[facet(default)]
pub struct PortForward {
    pub host: u16,
    pub guest: u16,
    #[facet(default = "127.0.0.1")]
    pub bind: String,
}

impl PortForward {
    pub fn bind_addr(&self) -> &str {
        if self.bind.is_empty() {
            "127.0.0.1"
        } else {
            &self.bind
        }
    }
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
    pub ssh: SshConfig,
    #[facet(default)]
    pub user: UserConfig,
    #[facet(default)]
    pub mounts: Vec<MountConfig>,
    #[facet(default)]
    pub drives: BTreeMap<String, DriveConfig>,
    #[facet(default)]
    pub fs: BTreeMap<String, Vec<FsEntryConfig>>,
    #[facet(default)]
    pub ports: Vec<PortForward>,
}

#[derive(Debug, Clone, Facet)]
pub struct ImageConfig {
    pub base: String,
}

#[derive(Debug, Clone, Facet)]
pub struct ResourcesConfig {
    pub cpus: u32,
    pub memory_mb: u64,
    #[facet(default = "20G")]
    pub disk: String,
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
    pub system: Option<ProvisionSystemConfig>,
    pub boot: Option<ProvisionBootConfig>,
}

#[derive(Debug, Clone, Facet)]
pub struct ProvisionSystemConfig {
    pub script: String,
}

#[derive(Debug, Clone, Facet)]
pub struct ProvisionBootConfig {
    pub script: String,
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
    #[facet(default)]
    pub autologin: bool,
}

impl Default for AdvancedConfig {
    fn default() -> Self {
        Self {
            libvirt_uri: "qemu:///system".into(),
            domain_type: "kvm".into(),
            machine: "q35".into(),
            autologin: false,
        }
    }
}

#[derive(Debug, Clone, Facet)]
#[facet(default)]
pub struct SshConfig {
    #[facet(default = "rum")]
    pub user: String,
    #[facet(default = "ssh")]
    pub command: String,
    #[facet(default)]
    pub interface: String,
    #[facet(default)]
    pub authorized_keys: Vec<String>,
}

impl Default for SshConfig {
    fn default() -> Self {
        Self {
            user: "rum".into(),
            command: "ssh".into(),
            interface: String::new(),
            authorized_keys: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Facet)]
#[facet(default)]
pub struct UserConfig {
    #[facet(default = "rum")]
    pub name: String,
    #[facet(default)]
    pub groups: Vec<String>,
}

impl Default for UserConfig {
    fn default() -> Self {
        Self {
            name: "rum".into(),
            groups: Vec::new(),
        }
    }
}
