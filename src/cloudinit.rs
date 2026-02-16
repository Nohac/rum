use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::Path;

use facet_value::{VArray, Value, value};

use crate::config::{BtrfsFs, ResolvedDrive, ResolvedFs, ResolvedMount, SimpleFs, ZfsFs};
use crate::error::RumError;
use crate::iso9660::{self, IsoFile};

/// Compute a short hash of the cloud-init inputs for cache-busting the seed ISO filename.
pub fn seed_hash(
    hostname: &str,
    provision_script: &str,
    packages: &[String],
    mounts: &[ResolvedMount],
    drives: &[ResolvedDrive],
    fs: &[ResolvedFs],
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
    for d in drives {
        d.name.hash(&mut hasher);
        d.size.hash(&mut hasher);
    }
    for f in fs {
        f.hash(&mut hasher);
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
    fs: &[ResolvedFs],
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
    let user_data = build_user_data(provision_script, packages, mounts, fs);
    // Network config v2 for cloud-init NoCloud datasource.
    // Note: no outer "network:" wrapper — the file IS the network config directly.
    let network_config =
        "version: 2\nethernets:\n  id0:\n    match:\n      name: \"en*\"\n    dhcp4: true\n";

    let iso = iso9660::build_iso(
        "CIDATA",
        &[
            IsoFile {
                name: "meta-data",
                data: meta_data.as_bytes(),
            },
            IsoFile {
                name: "user-data",
                data: user_data.as_bytes(),
            },
            IsoFile {
                name: "network-config",
                data: network_config.as_bytes(),
            },
        ],
    );

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

fn build_user_data(
    provision_script: &str,
    packages: &[String],
    mounts: &[ResolvedMount],
    fs: &[ResolvedFs],
) -> String {
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

    if !fs.is_empty() {
        let drive_script = build_drive_script(fs);
        write_files.push(value!({
            "path": "/var/lib/cloud/scripts/rum-drives.sh",
            "permissions": "0755",
            "content": (drive_script.as_str()),
        }));
    }

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
    runcmd.push(value!([
        "systemctl",
        "restart",
        "serial-getty@ttyS0.service"
    ]));

    // Create mount point directories before cloud-init processes mounts
    for m in mounts {
        runcmd.push(Value::from(VArray::from_iter([
            Value::from("mkdir"),
            Value::from("-p"),
            Value::from(m.target.as_str()),
        ])));
    }

    // Drive formatting/mounting runs before the provision script
    if !fs.is_empty() {
        runcmd.push(value!([
            "/bin/sh",
            "/var/lib/cloud/scripts/rum-drives.sh"
        ]));
    }

    if !provision_script.is_empty() {
        runcmd.push(value!([
            "/bin/bash",
            "/var/lib/cloud/scripts/rum-provision.sh"
        ]));
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

fn build_drive_script(fs: &[ResolvedFs]) -> String {
    use std::collections::BTreeSet;
    use std::fmt::Write;

    let mut script = String::from(
        "#!/usr/bin/env sh\nset -eu\n\n\
         . /etc/os-release\n\
         install_pkg() {\n\
         \x20 case \"$ID\" in\n\
         \x20   ubuntu|debian) DEBIAN_FRONTEND=noninteractive apt-get install -y \"$@\" ;;\n\
         \x20   arch)          pacman -S --noconfirm \"$@\" ;;\n\
         \x20   fedora)        dnf install -y \"$@\" ;;\n\
         \x20   alpine)        apk add \"$@\" ;;\n\
         \x20   *) echo \"rum: unsupported OS '$ID' for package install\" >&2; exit 1 ;;\n\
         \x20 esac\n\
         }\n\n",
    );

    // Collect needed filesystem types for tool checks
    let mut need_simple: BTreeSet<&str> = BTreeSet::new();
    let mut need_zfs = false;
    let mut need_btrfs = false;

    for entry in fs {
        match entry {
            ResolvedFs::Simple(s) => {
                need_simple.insert(&s.filesystem);
            }
            ResolvedFs::Zfs(_) => need_zfs = true,
            ResolvedFs::Btrfs(_) => need_btrfs = true,
        }
    }

    // Emit tool checks
    for fs_type in &need_simple {
        match *fs_type {
            "ext4" | "ext3" | "ext2" => {
                writeln!(
                    script,
                    "command -v mkfs.{fs_type} >/dev/null 2>&1 || install_pkg e2fsprogs"
                )
                .unwrap();
            }
            "xfs" => script.push_str("command -v mkfs.xfs >/dev/null 2>&1 || install_pkg xfsprogs\n"),
            "ntfs" => script.push_str("command -v mkfs.ntfs >/dev/null 2>&1 || install_pkg ntfs-3g\n"),
            "vfat" => script.push_str("command -v mkfs.vfat >/dev/null 2>&1 || install_pkg dosfstools\n"),
            _ => {
                writeln!(
                    script,
                    "command -v mkfs.{fs_type} >/dev/null 2>&1 || echo \"rum: mkfs.{fs_type} not found\" >&2"
                )
                .unwrap();
            }
        }
    }

    if need_btrfs {
        script.push_str("command -v mkfs.btrfs >/dev/null 2>&1 || install_pkg btrfs-progs\n");
    }

    if need_zfs {
        script.push_str(
            "command -v zpool >/dev/null 2>&1 || {\n\
             \x20 case \"$ID\" in\n\
             \x20   ubuntu|debian) install_pkg zfsutils-linux ;;\n\
             \x20   arch)          install_pkg zfs-utils ;;\n\
             \x20   fedora)        install_pkg zfs ;;\n\
             \x20   alpine)        install_pkg zfs ;;\n\
             \x20 esac\n\
             \x20 modprobe zfs\n\
             }\n",
        );
    }

    script.push('\n');

    // Per-filesystem setup blocks
    fn emit_simple(script: &mut String, s: &SimpleFs) {
        use std::fmt::Write;
        writeln!(
            script,
            "if ! blkid -o value -s TYPE {} >/dev/null 2>&1; then",
            s.dev
        )
        .unwrap();
        writeln!(script, "  mkfs.{} {}", s.filesystem, s.dev).unwrap();
        script.push_str("fi\n");
        writeln!(script, "mkdir -p {}", s.target).unwrap();
        writeln!(
            script,
            "grep -q '{}' /etc/fstab || echo '{} {} {} defaults,nofail 0 2' >> /etc/fstab",
            s.dev, s.dev, s.target, s.filesystem
        )
        .unwrap();
        script.push_str("mount -a\n\n");
    }

    fn emit_zfs(script: &mut String, z: &ZfsFs) {
        use std::fmt::Write;
        writeln!(
            script,
            "if ! zpool list {} >/dev/null 2>&1; then",
            z.pool
        )
        .unwrap();
        let mode_arg = if z.mode.is_empty() {
            String::new()
        } else {
            format!("{} ", z.mode)
        };
        writeln!(
            script,
            "  zpool create -o ashift=12 -O mountpoint={} {} {}{}",
            z.target,
            z.pool,
            mode_arg,
            z.devs.join(" ")
        )
        .unwrap();
        script.push_str("fi\n\n");
    }

    fn emit_btrfs(script: &mut String, b: &BtrfsFs) {
        use std::fmt::Write;
        let first_dev = &b.devs[0];
        writeln!(
            script,
            "if ! blkid -o value -s TYPE {} >/dev/null 2>&1; then",
            first_dev
        )
        .unwrap();
        let mode_arg = if b.mode == "single" {
            String::new()
        } else {
            format!("-d {} ", b.mode)
        };
        writeln!(script, "  mkfs.btrfs {}{}", mode_arg, b.devs.join(" ")).unwrap();
        script.push_str("fi\n");
        writeln!(script, "mkdir -p {}", b.target).unwrap();
        writeln!(
            script,
            "grep -q '{}' /etc/fstab || echo '{} {} btrfs defaults,nofail 0 0' >> /etc/fstab",
            first_dev, first_dev, b.target
        )
        .unwrap();
        script.push_str("mount -a\n\n");
    }

    for entry in fs {
        match entry {
            ResolvedFs::Simple(s) => emit_simple(&mut script, s),
            ResolvedFs::Zfs(z) => emit_zfs(&mut script, z),
            ResolvedFs::Btrfs(b) => emit_btrfs(&mut script, b),
        }
    }

    script
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_data_is_valid_cloud_config() {
        let ud = build_user_data("", &[], &[], &[]);
        assert!(ud.starts_with("#cloud-config\n"));
    }

    #[test]
    fn user_data_contains_user() {
        let ud = build_user_data("", &[], &[], &[]);
        assert!(ud.contains("name: rum"));
        assert!(ud.contains("lock_passwd: false"));
    }

    #[test]
    fn user_data_contains_packages() {
        let ud = build_user_data("", &["curl".into(), "git".into()], &[], &[]);
        assert!(ud.contains("packages:"));
        assert!(ud.contains("curl"));
        assert!(ud.contains("git"));
    }

    #[test]
    fn user_data_yaml_special_chars_in_packages() {
        let ud = build_user_data("", &["foo: bar".into()], &[], &[]);
        // YAML serializer should quote or escape the colon
        assert!(
            ud.contains("foo: bar") || ud.contains("'foo: bar'") || ud.contains("\"foo: bar\"")
        );
    }

    #[test]
    fn user_data_autologin_dropin() {
        let ud = build_user_data("", &[], &[], &[]);
        assert!(ud.contains("autologin.conf"));
        assert!(ud.contains("--autologin rum"));
        assert!(ud.contains("--keep-baud 115200,38400,9600"));
    }

    #[test]
    fn user_data_runcmd_restarts_getty() {
        let ud = build_user_data("", &[], &[], &[]);
        assert!(ud.contains("runcmd:"));
        assert!(ud.contains("daemon-reload"));
        assert!(ud.contains("serial-getty@ttyS0.service"));
    }

    #[test]
    fn user_data_provision_script() {
        let ud = build_user_data("echo hello\necho world", &[], &[], &[]);
        assert!(ud.contains("rum-provision.sh"));
        assert!(ud.contains("echo hello"));
        assert!(ud.contains("echo world"));
        assert!(ud.contains("runcmd:"));
    }

    #[test]
    fn user_data_runcmd_includes_provision_script() {
        let ud = build_user_data("echo hello", &[], &[], &[]);
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
        let ud = build_user_data("", &[], &mounts, &[]);
        assert!(ud.contains("mounts:"));
        assert!(ud.contains("mnt_project"));
        assert!(ud.contains("/mnt/project"));
        assert!(ud.contains("virtiofs"));
        assert!(ud.contains("nofail"));
        // Should also have mkdir in runcmd
        assert!(ud.contains("mkdir"));
    }

    #[test]
    fn drive_script_ext4() {
        let fs = vec![ResolvedFs::Simple(SimpleFs {
            filesystem: "ext4".into(),
            dev: "/dev/vdb".into(),
            target: "/mnt/data".into(),
        })];
        let script = build_drive_script(&fs);
        assert!(script.starts_with("#!/usr/bin/env sh"));
        assert!(script.contains("install_pkg"));
        assert!(script.contains("e2fsprogs"));
        assert!(script.contains("mkfs.ext4 /dev/vdb"));
        assert!(script.contains("mkdir -p /mnt/data"));
        assert!(script.contains("/dev/vdb /mnt/data ext4 defaults,nofail"));
        assert!(script.contains("blkid")); // idempotency guard
    }

    #[test]
    fn drive_script_zfs_mirror() {
        let fs = vec![ResolvedFs::Zfs(ZfsFs {
            pool: "logspool".into(),
            devs: vec!["/dev/vdc".into(), "/dev/vdd".into()],
            target: "/mnt/logs".into(),
            mode: "mirror".into(),
        })];
        let script = build_drive_script(&fs);
        assert!(script.contains("zfsutils-linux")); // ubuntu/debian package
        assert!(script.contains("modprobe zfs"));
        assert!(script.contains("zpool list logspool")); // idempotency guard
        assert!(script.contains("zpool create"));
        assert!(script.contains("mountpoint=/mnt/logs"));
        assert!(script.contains("mirror /dev/vdc /dev/vdd"));
    }

    #[test]
    fn drive_script_btrfs_raid1() {
        let fs = vec![ResolvedFs::Btrfs(BtrfsFs {
            devs: vec!["/dev/vde".into(), "/dev/vdf".into()],
            target: "/mnt/fast".into(),
            mode: "raid1".into(),
        })];
        let script = build_drive_script(&fs);
        assert!(script.contains("btrfs-progs"));
        assert!(script.contains("mkfs.btrfs -d raid1 /dev/vde /dev/vdf"));
        assert!(script.contains("mkdir -p /mnt/fast"));
        assert!(script.contains("/dev/vde /mnt/fast btrfs defaults,nofail"));
        assert!(script.contains("blkid")); // idempotency guard
    }

    #[test]
    fn user_data_with_fs_includes_drive_script() {
        let fs = vec![ResolvedFs::Simple(SimpleFs {
            filesystem: "ext4".into(),
            dev: "/dev/vdb".into(),
            target: "/mnt/data".into(),
        })];
        let ud = build_user_data("", &[], &[], &fs);
        assert!(ud.contains("rum-drives.sh"));
        assert!(ud.contains("/bin/sh"));
        assert!(ud.contains("mkfs.ext4"));
    }

    #[test]
    fn user_data_drive_script_before_provision() {
        let fs = vec![ResolvedFs::Simple(SimpleFs {
            filesystem: "ext4".into(),
            dev: "/dev/vdb".into(),
            target: "/mnt/data".into(),
        })];
        let ud = build_user_data("echo hello", &[], &[], &fs);
        let drives_pos = ud.find("rum-drives.sh").unwrap();
        let provision_pos = ud.find("rum-provision.sh").unwrap();
        assert!(
            drives_pos < provision_pos,
            "drive script should run before provision script"
        );
    }
}
