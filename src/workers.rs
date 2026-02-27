//! Standalone worker functions for VM lifecycle operations.
//!
//! Each function is a self-contained async operation with clean inputs and
//! outputs. These are called by the event loop's `make_worker()` to execute
//! effects. No progress/UI coupling — that's the observer's job.

use std::path::{Path, PathBuf};

use crate::agent::AgentClient;
use crate::config::SystemConfig;
use crate::error::RumError;

/// Download or verify the base image. Returns path to cached image.
pub async fn ensure_image(
    base_url: &str,
    cache_dir: &Path,
) -> Result<PathBuf, RumError> {
    todo!("extract from backend/libvirt.rs image download section")
}

/// Create overlay, extra drives, seed ISO, domain XML, define domain,
/// ensure networks. Full artifact preparation.
pub async fn prepare_vm(
    sys_config: &SystemConfig,
    base_image: &Path,
) -> Result<(), RumError> {
    todo!("extract from backend/libvirt.rs prepare section")
}

/// Start the libvirt domain. Returns vsock CID.
pub async fn boot_vm(
    sys_config: &SystemConfig,
) -> Result<u32, RumError> {
    todo!("extract from backend/libvirt.rs boot section")
}

/// Wait for guest agent to become reachable.
pub async fn connect_agent(cid: u32) -> Result<AgentClient, RumError> {
    crate::agent::wait_for_agent(cid).await
}

/// Run provision scripts via the guest agent.
pub async fn run_provision(
    agent: &AgentClient,
    scripts: Vec<rum_agent::ProvisionScript>,
    logs_dir: &Path,
) -> Result<(), RumError> {
    todo!("wrap agent::run_provision without progress params")
}

/// ACPI shutdown with timeout, force-destroy fallback.
pub async fn shutdown_vm(
    sys_config: &SystemConfig,
) -> Result<(), RumError> {
    todo!("extract from backend/libvirt.rs shutdown_domain")
}

/// Force-destroy domain if running, undefine, remove artifacts.
pub async fn destroy_vm(
    sys_config: &SystemConfig,
) -> Result<(), RumError> {
    todo!("extract from main.rs destroy_cleanup")
}

/// Start log subscription + port forwards. Returns handles.
pub async fn start_services(
    cid: u32,
    sys_config: &SystemConfig,
) -> Result<crate::daemon::ServiceHandles, RumError> {
    todo!("extract from daemon.rs start_services")
}
