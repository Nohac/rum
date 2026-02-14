use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::Path;

use facet_value::{VArray, Value, value};

use crate::config::ResolvedMount;
use crate::error::RumError;
use crate::iso9660::{self, IsoFile};

/// Compute a short hash of the cloud-init inputs for cache-busting the seed ISO filename.
pub fn seed_hash(
    hostname: &str,
    provision_script: &str,
    packages: &[String],
    mounts: &[ResolvedMount],
) -> String {
    let mut hasher = DefaultHasher::new();
    hostname.hash(&mut hasher);
    provision_script.hash(&mut hasher);
    packages.hash(&mut hasher);
    for m in mounts {
        m.tag.hash(&mut hasher);
        m.target.hash(&mut hasher);
        m.readonly.hash(&mut hasher);
    }
    format!("{:016x}", hasher.finish())
}

/// Generate a cloud-init NoCloud seed ISO (ISO 9660 with volume label "CIDATA").
pub async fn generate_seed_iso(
    seed_path: &Path,
    hostname: &str,
    provision_script: &str,
    packages: &[String],
    mounts: &[ResolvedMount],
) -> Result<(), RumError> {
    if let Some(parent) = seed_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| RumError::Io {
                context: format!("creating directory {}", parent.display()),
                source: e,
            })?;
    }

    let meta_data = format!("instance-id: {hostname}\nlocal-hostname: {hostname}\n");
    let user_data = build_user_data(provision_script, packages, mounts);
    // Network config v2 for cloud-init NoCloud datasource.
    // Note: no outer "network:" wrapper — the file IS the network config directly.
    let network_config = "version: 2\nethernets:\n  id0:\n    match:\n      name: \"en*\"\n    dhcp4: true\n";

    let iso = iso9660::build_iso("CIDATA", &[
        IsoFile { name: "meta-data", data: meta_data.as_bytes() },
        IsoFile { name: "user-data", data: user_data.as_bytes() },
        IsoFile { name: "network-config", data: network_config.as_bytes() },
    ]);

    tokio::fs::write(seed_path, &iso)
        .await
        .map_err(|e| RumError::Io {
            context: format!("writing seed ISO to {}", seed_path.display()),
            source: e,
        })?;

    tracing::info!(path = %seed_path.display(), "generated cloud-init seed ISO");
    Ok(())
}

const AUTOLOGIN_DROPIN: &str = "\
[Service]
ExecStart=
ExecStart=-/sbin/agetty --autologin rum --noclear --keep-baud 115200,38400,9600 %I $TERM
";

fn build_user_data(provision_script: &str, packages: &[String], mounts: &[ResolvedMount]) -> String {
    let user = value!({
        "name": "rum",
        "plain_text_passwd": "rum",
        "lock_passwd": false,
        "shell": "/bin/bash",
        "sudo": "ALL=(ALL) NOPASSWD:ALL",
    });

    let autologin_file = value!({
        "path": "/etc/systemd/system/serial-getty@ttyS0.service.d/autologin.conf",
        "content": (AUTOLOGIN_DROPIN),
    });

    let mut write_files = VArray::new();
    write_files.push(autologin_file);

    if !provision_script.is_empty() {
        write_files.push(value!({
            "path": "/var/lib/cloud/scripts/rum-provision.sh",
            "permissions": "0755",
            "content": (provision_script),
        }));
    }

    // runcmd runs late (after write_files), so the autologin dropin is already
    // on disk by the time we reload + restart the getty.
    let mut runcmd = VArray::new();
    runcmd.push(value!(["systemctl", "daemon-reload"]));
    runcmd.push(value!(["systemctl", "restart", "serial-getty@ttyS0.service"]));

    // Create mount point directories before cloud-init processes mounts
    for m in mounts {
        runcmd.push(Value::from(VArray::from_iter([
            Value::from("mkdir"),
            Value::from("-p"),
            Value::from(m.target.as_str()),
        ])));
    }

    if !provision_script.is_empty() {
        runcmd.push(value!(["/bin/bash", "/var/lib/cloud/scripts/rum-provision.sh"]));
    }

    let mut config = value!({
        "users": [user],
        "write_files": (Value::from(write_files)),
        "runcmd": (Value::from(runcmd)),
    });

    // Add virtiofs mount entries
    if !mounts.is_empty() {
        let mut mount_entries = VArray::new();
        for m in mounts {
            let entry = VArray::from_iter([
                Value::from(m.tag.as_str()),
                Value::from(m.target.as_str()),
                Value::from("virtiofs"),
                Value::from("defaults,nofail"),
                Value::from("0"),
                Value::from("0"),
            ]);
            mount_entries.push(Value::from(entry));
        }
        if let Some(obj) = config.as_object_mut() {
            obj.insert("mounts", Value::from(mount_entries));
        }
    }

    if !packages.is_empty() {
        let mut pkg_array = VArray::new();
        for pkg in packages {
            pkg_array.push(Value::from(pkg.as_str()));
        }
        if let Some(obj) = config.as_object_mut() {
            obj.insert("packages", Value::from(pkg_array));
        }
    }

    let yaml = facet_yaml::to_string(&config).expect("valid YAML serialization");
    // Strip the "---\n" YAML document separator — cloud-init expects #cloud-config
    // as the first line, and some versions choke on a document separator after it.
    let yaml = yaml.strip_prefix("---\n").unwrap_or(&yaml);
    format!("#cloud-config\n{yaml}")
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_data_is_valid_cloud_config() {
        let ud = build_user_data("", &[], &[]);
        assert!(ud.starts_with("#cloud-config\n"));
    }

    #[test]
    fn user_data_contains_user() {
        let ud = build_user_data("", &[], &[]);
        assert!(ud.contains("name: rum"));
        assert!(ud.contains("lock_passwd: false"));
    }

    #[test]
    fn user_data_contains_packages() {
        let ud = build_user_data("", &["curl".into(), "git".into()], &[]);
        assert!(ud.contains("packages:"));
        assert!(ud.contains("curl"));
        assert!(ud.contains("git"));
    }

    #[test]
    fn user_data_yaml_special_chars_in_packages() {
        let ud = build_user_data("", &["foo: bar".into()], &[]);
        // YAML serializer should quote or escape the colon
        assert!(ud.contains("foo: bar") || ud.contains("'foo: bar'") || ud.contains("\"foo: bar\""));
    }

    #[test]
    fn user_data_autologin_dropin() {
        let ud = build_user_data("", &[], &[]);
        assert!(ud.contains("autologin.conf"));
        assert!(ud.contains("--autologin rum"));
        assert!(ud.contains("--keep-baud 115200,38400,9600"));
    }

    #[test]
    fn user_data_runcmd_restarts_getty() {
        let ud = build_user_data("", &[], &[]);
        assert!(ud.contains("runcmd:"));
        assert!(ud.contains("daemon-reload"));
        assert!(ud.contains("serial-getty@ttyS0.service"));
    }

    #[test]
    fn user_data_provision_script() {
        let ud = build_user_data("echo hello\necho world", &[], &[]);
        assert!(ud.contains("rum-provision.sh"));
        assert!(ud.contains("echo hello"));
        assert!(ud.contains("echo world"));
        assert!(ud.contains("runcmd:"));
    }

    #[test]
    fn user_data_runcmd_includes_provision_script() {
        let ud = build_user_data("echo hello", &[], &[]);
        assert!(ud.contains("rum-provision.sh"));
    }

    #[test]
    fn user_data_contains_virtiofs_mounts() {
        let mounts = vec![ResolvedMount {
            source: std::path::PathBuf::from("/home/user/project"),
            target: "/mnt/project".into(),
            readonly: false,
            tag: "mnt_project".into(),
        }];
        let ud = build_user_data("", &[], &mounts);
        assert!(ud.contains("mounts:"));
        assert!(ud.contains("mnt_project"));
        assert!(ud.contains("/mnt/project"));
        assert!(ud.contains("virtiofs"));
        assert!(ud.contains("nofail"));
        // Should also have mkdir in runcmd
        assert!(ud.contains("mkdir"));
    }
}
