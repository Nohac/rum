use crate::error::RumError;
use super::backend_check::detect_backend;
use super::model::{WizardConfig, WizardStep};
use super::prompts::*;

pub(super) fn run_wizard() -> Result<WizardConfig, RumError> {
    println!();

    detect_backend()?;

    let mut image_url = String::new();
    let mut image_comment = None;
    let mut cpus = 2u32;
    let mut memory_mb = 2048u64;
    let mut disk = "20G".to_string();
    let mut hostname = String::new();
    let mut nat = true;
    let mut interfaces = Vec::new();
    let mut mounts = Vec::new();
    let mut drives = Vec::new();
    let mut filesystems = Vec::new();

    let mut step = WizardStep::OsImage;

    loop {
        match step {
            WizardStep::OsImage => match prompt_os_image() {
                Ok((url, comment)) => {
                    image_url = url;
                    image_comment = comment;
                    step = step.next();
                }
                Err(RumError::InitCancelled) => return Err(RumError::InitCancelled),
                Err(e) => return Err(e),
            },
            WizardStep::Resources => match prompt_resources() {
                Ok((c, m, d)) => {
                    cpus = c;
                    memory_mb = m;
                    disk = d;
                    step = step.next();
                }
                Err(RumError::InitCancelled) => step = step.prev(),
                Err(e) => return Err(e),
            },
            WizardStep::Hostname => match prompt_hostname() {
                Ok(h) => {
                    hostname = h;
                    step = step.next();
                }
                Err(RumError::InitCancelled) => step = step.prev(),
                Err(e) => return Err(e),
            },
            WizardStep::Network => match prompt_network() {
                Ok((n, ifaces)) => {
                    nat = n;
                    interfaces = ifaces;
                    step = step.next();
                }
                Err(RumError::InitCancelled) => step = step.prev(),
                Err(e) => return Err(e),
            },
            WizardStep::Mounts => match prompt_mounts() {
                Ok(m) => {
                    mounts = m;
                    step = step.next();
                }
                Err(RumError::InitCancelled) => step = step.prev(),
                Err(e) => return Err(e),
            },
            WizardStep::Storage => match prompt_storage() {
                Ok((d, fs)) => {
                    drives = d;
                    filesystems = fs;
                    step = step.next();
                }
                Err(RumError::InitCancelled) => step = step.prev(),
                Err(e) => return Err(e),
            },
            WizardStep::Done => break,
        }
    }

    Ok(WizardConfig {
        image_url,
        image_comment,
        cpus,
        memory_mb,
        disk,
        hostname,
        nat,
        interfaces,
        mounts,
        drives,
        filesystems,
    })
}
