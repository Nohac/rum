use crate::config::SystemConfig;
use crate::error::RumError;
use crate::progress::OutputMode;

pub async fn run_provision(
    sys_config: &SystemConfig,
    system: bool,
    boot: bool,
    mode: OutputMode,
) -> Result<(), RumError> {
    let cid = crate::backend::libvirt::get_vsock_cid(sys_config)?;

    let config = &sys_config.config;
    let drives = sys_config.resolve_drives()?;
    let resolved_fs = sys_config.resolve_fs(&drives)?;
    let mut provision_scripts = Vec::new();

    if !resolved_fs.is_empty() {
        provision_scripts.push(crate::agent::ProvisionScript {
            name: "rum-drives".into(),
            title: "Setting up drives and filesystems".into(),
            content: crate::cloudinit::build_drive_script(&resolved_fs),
            order: 0,
            run_on: crate::agent::RunOn::System,
        });
    }
    if let Some(ref sys) = config.provision.system {
        provision_scripts.push(crate::agent::ProvisionScript {
            name: "rum-system".into(),
            title: "Running system provisioning".into(),
            content: sys.script.clone(),
            order: 1,
            run_on: crate::agent::RunOn::System,
        });
    }
    if let Some(ref boot_cfg) = config.provision.boot {
        provision_scripts.push(crate::agent::ProvisionScript {
            name: "rum-boot".into(),
            title: "Running boot provisioning".into(),
            content: boot_cfg.script.clone(),
            order: 2,
            run_on: crate::agent::RunOn::Boot,
        });
    }

    // Filter by flags: --system = only System, --boot = only Boot, neither = all
    let scripts: Vec<_> = if system && !boot {
        provision_scripts
            .into_iter()
            .filter(|s| matches!(s.run_on, crate::agent::RunOn::System))
            .collect()
    } else if boot && !system {
        provision_scripts
            .into_iter()
            .filter(|s| matches!(s.run_on, crate::agent::RunOn::Boot))
            .collect()
    } else {
        provision_scripts
    };

    if scripts.is_empty() {
        println!("No provisioning scripts to run.");
        return Ok(());
    }

    let agent = crate::agent::wait_for_agent(cid).await?;
    let logs_dir = crate::paths::logs_dir(&sys_config.id, sys_config.name.as_deref());
    std::fs::create_dir_all(&logs_dir).ok();

    let total_steps = scripts.len();
    let mut progress = crate::progress::StepProgress::new(total_steps, mode);
    crate::agent::run_provision(&agent, scripts, &mut progress, &logs_dir).await?;

    Ok(())
}
