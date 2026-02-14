use std::process::Stdio;

use indicatif::ProgressBar;
use virt::connect::Connect;
use virt::domain::Domain;
use virt::error as virt_error;
use virt::network::Network;

use crate::config::Config;
use crate::error::RumError;
use crate::{cloudinit, domain_xml, image, overlay, paths};

struct ConnGuard(Connect);

impl std::ops::Deref for ConnGuard {
    type Target = Connect;
    fn deref(&self) -> &Connect {
        &self.0
    }
}

impl Drop for ConnGuard {
    fn drop(&mut self) {
        self.0.close().ok();
    }
}

pub struct LibvirtBackend;

impl super::Backend for LibvirtBackend {
    async fn up(&self, config: &Config, reset: bool) -> Result<(), RumError> {
        let name = &config.name;
        let work = paths::work_dir(name);
        let overlay_path = paths::overlay_path(name);
        let seed_hash = cloudinit::seed_hash(
            config.hostname(),
            &config.provision.script,
            &config.provision.packages,
        );
        let seed_path = paths::seed_path(name, &seed_hash);
        let xml_path = paths::domain_xml_path(name);
        let cache = paths::cache_dir();

        let conn = connect(config)?;

        // --reset: stop, undefine, wipe artifacts
        if reset {
            tracing::info!(name, "resetting VM");
            if let Ok(dom) = Domain::lookup_by_name(&conn, name) {
                let _ = shutdown_domain(&dom).await;
                let _ = dom.undefine();
            }
            let _ = tokio::fs::remove_dir_all(&work).await;
        }

        // 1. Ensure base image
        println!("Ensuring base image...");
        let base = image::ensure_base_image(&config.image.base, &cache).await?;

        // 2. Create overlay if absent
        if !overlay_path.exists() {
            println!("Creating disk overlay...");
            overlay::create_overlay(&base, &overlay_path).await?;
        }

        // 3. Generate seed ISO if inputs changed (hash-keyed filename)
        if !seed_path.exists() {
            println!("Generating cloud-init seed...");
            // Remove old seed ISOs with different hashes
            if let Ok(mut entries) = tokio::fs::read_dir(&work).await {
                while let Ok(Some(entry)) = entries.next_entry().await {
                    let name = entry.file_name();
                    if let Some(s) = name.to_str() {
                        if s.starts_with("seed-") && s.ends_with(".iso") {
                            let _ = tokio::fs::remove_file(entry.path()).await;
                        }
                    }
                }
            }
            cloudinit::generate_seed_iso(
                &seed_path,
                config.hostname(),
                &config.provision.script,
                &config.provision.packages,
            )
            .await?;
        }

        // 4. Generate domain XML
        let xml = domain_xml::generate_domain_xml(config, &overlay_path, &seed_path);

        // 5. Define or redefine domain
        println!("Configuring VM...");
        let existing = Domain::lookup_by_name(&conn, name);
        match existing {
            Ok(dom) => {
                if domain_xml::xml_has_changed(config, &overlay_path, &seed_path, &xml_path) {
                    if is_running(&dom) {
                        return Err(RumError::RequiresRestart { name: name.clone() });
                    }
                    dom.undefine().map_err(|e| RumError::Libvirt {
                        message: format!("failed to undefine domain: {e}"),
                        hint: "check libvirt permissions".into(),
                    })?;
                    define_domain(&conn, &xml)?;
                    tracing::info!(name, "domain redefined with updated config");
                }
            }
            Err(_) => {
                define_domain(&conn, &xml)?;
                tracing::info!(name, "domain defined");
            }
        }

        // Save XML for future change detection
        tokio::fs::write(&xml_path, &xml)
            .await
            .map_err(|e| RumError::Io {
                context: format!("saving domain XML to {}", xml_path.display()),
                source: e,
            })?;

        // 6. Ensure default network is active
        println!("Checking network...");
        ensure_default_network(&conn)?;

        // 7. Start if not running
        let dom = Domain::lookup_by_name(&conn, name).map_err(|e| RumError::Libvirt {
            message: format!("domain lookup failed: {e}"),
            hint: "domain should have been defined above".into(),
        })?;

        if !is_running(&dom) {
            println!("Starting VM...");
            dom.create().map_err(|e| RumError::Libvirt {
                message: format!("failed to start domain: {e}"),
                hint: "check `virsh -c qemu:///system start` for details".into(),
            })?;
            tracing::info!(name, "VM started");
        } else {
            tracing::info!(name, "VM already running");
        }

        drop(conn);

        // 7. Attach serial console via virsh
        println!("Attaching console (press Ctrl+C or Ctrl+] to detach)...");
        let mut child = tokio::process::Command::new("virsh")
            .args(["-c", config.libvirt_uri(), "console", name])
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| RumError::Io {
                context: "running virsh console".into(),
                source: e,
            })?;

        tokio::select! {
            status = child.wait() => {
                match status {
                    Ok(s) if !s.success() => {
                        tracing::warn!("virsh console exited with {s}");
                    }
                    Err(e) => {
                        tracing::warn!("virsh console wait failed: {e}");
                    }
                    _ => {}
                }
            }
            _ = tokio::signal::ctrl_c() => {
                let _ = child.kill().await;
                println!("\nDetached from console. VM is still running.");
            }
        }

        Ok(())
    }

    async fn down(&self, config: &Config) -> Result<(), RumError> {
        let name = &config.name;
        let conn = connect(config)?;

        let dom = Domain::lookup_by_name(&conn, name)
            .map_err(|_| RumError::DomainNotFound { name: name.clone() })?;

        if !is_running(&dom) {
            println!("VM '{name}' is not running.");
            return Ok(());
        }

        // ACPI shutdown
        tracing::info!(name, "sending ACPI shutdown");
        dom.shutdown().map_err(|e| RumError::Libvirt {
            message: format!("shutdown failed: {e}"),
            hint: "VM may not have ACPI support".into(),
        })?;

        // Wait up to 30s for shutdown
        let spinner = ProgressBar::new_spinner();
        spinner.set_message(format!("Waiting for VM '{name}' to shut down..."));
        spinner.enable_steady_tick(std::time::Duration::from_millis(120));
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            if !is_running(&dom) {
                spinner.finish_with_message(format!("VM '{name}' stopped."));
                return Ok(());
            }
            if tokio::time::Instant::now() >= deadline {
                spinner.finish_and_clear();
                break;
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            spinner.tick();
        }

        // Force stop
        tracing::warn!(name, "ACPI shutdown timed out, force stopping");
        dom.destroy().map_err(|e| RumError::Libvirt {
            message: format!("force stop failed: {e}"),
            hint: "check libvirt permissions".into(),
        })?;
        println!("VM '{name}' force stopped.");

        Ok(())
    }

    async fn destroy(&self, config: &Config, purge: bool) -> Result<(), RumError> {
        let name = &config.name;
        let conn = connect(config)?;

        if let Ok(dom) = Domain::lookup_by_name(&conn, name) {
            if is_running(&dom) {
                tracing::info!(name, "stopping VM before destroy");
                let _ = dom.destroy();
            }
            dom.undefine().map_err(|e| RumError::Libvirt {
                message: format!("failed to undefine domain: {e}"),
                hint: "check libvirt permissions".into(),
            })?;
            tracing::info!(name, "domain undefined");
        }

        // Remove work dir
        let work = paths::work_dir(name);
        if work.exists() {
            tokio::fs::remove_dir_all(&work)
                .await
                .map_err(|e| RumError::Io {
                    context: format!("removing {}", work.display()),
                    source: e,
                })?;
            tracing::info!(path = %work.display(), "removed work directory");
        }

        // Purge: remove cached base image
        if purge && let Some(filename) = config.image.base.rsplit('/').next() {
            let cached = paths::cache_dir().join(filename);
            if cached.exists() {
                tokio::fs::remove_file(&cached)
                    .await
                    .map_err(|e| RumError::Io {
                        context: format!("removing cached image {}", cached.display()),
                        source: e,
                    })?;
                tracing::info!(path = %cached.display(), "removed cached base image");
            }
        }

        println!("VM '{name}' destroyed.");
        Ok(())
    }

    async fn status(&self, config: &Config) -> Result<(), RumError> {
        let name = &config.name;
        let conn = connect(config)?;

        match Domain::lookup_by_name(&conn, name) {
            Ok(dom) => {
                let state = if is_running(&dom) {
                    "running"
                } else {
                    "stopped"
                };
                println!("VM '{name}': {state}");

                if is_running(&dom) {
                    // Try to get IP from DHCP leases
                    match dom
                        .interface_addresses(virt::sys::VIR_DOMAIN_INTERFACE_ADDRESSES_SRC_LEASE, 0)
                    {
                        Ok(ifaces) => {
                            for iface in &ifaces {
                                for addr in &iface.addrs {
                                    println!("  IP: {}", addr.addr);
                                }
                            }
                        }
                        Err(_) => {
                            println!("  IP: unknown (DHCP lease not yet available)");
                        }
                    }
                }
            }
            Err(_) => {
                println!("VM '{name}': not defined");
            }
        }

        Ok(())
    }
}

fn connect(config: &Config) -> Result<ConnGuard, RumError> {
    // Suppress libvirt's default error handler that prints to stderr.
    // This installs a no-op callback so errors are only surfaced through
    // Rust's Result types, not printed to stderr by the C library.
    virt_error::clear_error_callback();

    Connect::open(Some(config.libvirt_uri()))
        .map(ConnGuard)
        .map_err(|e| RumError::Libvirt {
            message: format!("failed to connect to libvirt: {e}"),
            hint: format!(
                "ensure libvirtd is running and you have access to {}",
                config.libvirt_uri()
            ),
        })
}

fn define_domain(conn: &Connect, xml: &str) -> Result<Domain, RumError> {
    Domain::define_xml(conn, xml).map_err(|e| RumError::Libvirt {
        message: format!("failed to define domain: {e}"),
        hint: "check the generated domain XML for errors".into(),
    })
}

fn ensure_default_network(conn: &Connect) -> Result<(), RumError> {
    let net = Network::lookup_by_name(conn, "default").map_err(|_| RumError::Libvirt {
        message: "default network not found".into(),
        hint: "run `sudo virsh net-define /usr/share/libvirt/networks/default.xml && sudo virsh net-start default`".into(),
    })?;

    if !net.is_active().unwrap_or(false) {
        tracing::info!("starting inactive default network");
        net.create().map_err(|e| RumError::Libvirt {
            message: format!("failed to start default network: {e}"),
            hint: "try `sudo virsh net-start default`".into(),
        })?;
    }

    Ok(())
}

fn is_running(dom: &Domain) -> bool {
    dom.is_active().unwrap_or(false)
}

async fn shutdown_domain(dom: &Domain) -> Result<(), RumError> {
    if !is_running(dom) {
        return Ok(());
    }
    dom.shutdown().map_err(|e| RumError::Libvirt {
        message: format!("shutdown failed: {e}"),
        hint: "VM may not support ACPI shutdown".into(),
    })?;

    // Brief wait for shutdown
    for _ in 0..10 {
        if !is_running(dom) {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }

    // Force
    dom.destroy().map_err(|e| RumError::Libvirt {
        message: format!("force stop failed: {e}"),
        hint: "check libvirt permissions".into(),
    })?;
    Ok(())
}
