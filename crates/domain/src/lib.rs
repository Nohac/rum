pub mod domain_xml;
pub mod network_xml;

use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct ResolvedMount {
    pub source: PathBuf,
    pub target: String,
    pub readonly: bool,
    pub tag: String,
}

#[derive(Debug, Clone)]
pub struct ResolvedDrive {
    pub path: PathBuf,
    pub dev: String,
}

#[derive(Debug, Clone)]
pub struct InterfaceConfig {
    pub network: String,
}

#[derive(Debug, Clone)]
pub struct DomainConfig {
    pub id: String,
    pub name: String,
    pub domain_type: String,
    pub machine: String,
    pub memory_mb: u64,
    pub cpus: u32,
    pub nat: bool,
    pub interfaces: Vec<InterfaceConfig>,
}

pub use domain_xml::{generate_domain_xml, generate_mac, parse_vsock_cid, xml_has_changed};
pub use network_xml::{derive_subnet, generate_network_xml, prefixed_name};
