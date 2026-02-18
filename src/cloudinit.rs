use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::Path;

use facet_value::{VArray, Value, value};

use crate::config::{BtrfsFs, ResolvedDrive, ResolvedFs, ResolvedMount, SimpleFs, ZfsFs};
use crate::error::RumError;
use crate::iso9660::{self, IsoFile};

/// Compute a short hash of the cloud-init inputs for cache-busting the seed ISO filename.
#[allow(clippy::too_many_arguments)]
pub fn seed_hash(
    hostname: &str,
    system_script: Option<&str>,
    boot_script: Option<&str>,
    mounts: &[ResolvedMount],
    drives: &[ResolvedDrive],
    fs: &[ResolvedFs],
    autologin: bool,
    ssh_keys: &[String],
) -> String {
    let mut hasher = DefaultHasher::new();
    hostname.hash(&mut hasher);
    system_script.hash(&mut hasher);
    boot_script.hash(&mut hasher);
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
    autologin.hash(&mut hasher);
    for k in ssh_keys {
        k.hash(&mut hasher);
    }
    format!("{:016x}", hasher.finish())
}

/// Generate a cloud-init NoCloud seed ISO (ISO 9660 with volume label "CIDATA").
#[allow(clippy::too_many_arguments)]
pub async fn generate_seed_iso(
    seed_path: &Path,
    hostname: &str,
    system_script: Option<&str>,
    boot_script: Option<&str>,
    mounts: &[ResolvedMount],
    fs: &[ResolvedFs],
    autologin: bool,
    ssh_keys: &[String],
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
    let user_data = build_user_data(system_script, boot_script, mounts, fs, autologin, ssh_keys);
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
[Unit]
After=rum-system.service rum-boot.service

[Service]
ExecStart=
ExecStart=-/sbin/agetty --autologin rum --noclear --keep-baud 115200,38400,9600 %I $TERM
";

const RUM_SYSTEM_SERVICE: &str = "\
[Unit]
Description=rum system provisioning (first boot)
After=network-online.target
Wants=network-online.target
ConditionPathExists=!/var/lib/rum/.system-provisioned

[Service]
Type=oneshot
RemainAfterExit=yes
ExecStart=/bin/bash /var/lib/cloud/scripts/rum-system.sh
ExecStartPost=/bin/mkdir -p /var/lib/rum
ExecStartPost=/bin/touch /var/lib/rum/.system-provisioned

[Install]
WantedBy=multi-user.target
";

const RUM_BOOT_SERVICE: &str = "\
[Unit]
Description=rum boot provisioning
After=network-online.target rum-system.service
Wants=network-online.target

[Service]
Type=oneshot
RemainAfterExit=yes
ExecStart=/bin/bash /var/lib/cloud/scripts/rum-boot.sh

[Install]
WantedBy=multi-user.target
";

fn build_user_data(
    system_script: Option<&str>,
    boot_script: Option<&str>,
    mounts: &[ResolvedMount],
    fs: &[ResolvedFs],
    autologin: bool,
    ssh_keys: &[String],
) -> String {
    let mut user = value!({
        "name": "rum",
        "plain_text_passwd": "rum",
        "lock_passwd": false,
        "shell": "/bin/bash",
        "sudo": "ALL=(ALL) NOPASSWD:ALL",
    });

    if !ssh_keys.is_empty() {
        let keys_array = VArray::from_iter(ssh_keys.iter().map(|k| Value::from(k.as_str())));
        if let Some(obj) = user.as_object_mut() {
            obj.insert("ssh_authorized_keys", Value::from(keys_array));
        }
    }

    let mut write_files = VArray::new();

    if !fs.is_empty() {
        let drive_script = build_drive_script(fs);
        write_files.push(value!({
            "path": "/var/lib/cloud/scripts/rum-drives.sh",
            "permissions": "0755",
            "content": (drive_script.as_str()),
        }));
    }

    if let Some(script) = system_script {
        write_files.push(value!({
            "path": "/var/lib/cloud/scripts/rum-system.sh",
            "permissions": "0755",
            "content": (script),
        }));
        write_files.push(value!({
            "path": "/etc/systemd/system/rum-system.service",
            "content": (RUM_SYSTEM_SERVICE),
        }));
    }

    if let Some(script) = boot_script {
        write_files.push(value!({
            "path": "/var/lib/cloud/scripts/rum-boot.sh",
            "permissions": "0755",
            "content": (script),
        }));
        write_files.push(value!({
            "path": "/etc/systemd/system/rum-boot.service",
            "content": (RUM_BOOT_SERVICE),
        }));
    }

    if autologin {
        write_files.push(value!({
            "path": "/etc/systemd/system/serial-getty@ttyS0.service.d/autologin.conf",
            "content": (AUTOLOGIN_DROPIN),
        }));
    }

    let mut runcmd = VArray::new();

    // Create mount point directories before cloud-init processes mounts
    for m in mounts {
        runcmd.push(Value::from(VArray::from_iter([
            Value::from("mkdir"),
            Value::from("-p"),
            Value::from(m.target.as_str()),
        ])));
    }

    // Drive formatting/mounting runs before provisioning services
    if !fs.is_empty() {
        runcmd.push(value!([
            "/bin/sh",
            "/var/lib/cloud/scripts/rum-drives.sh"
        ]));
    }

    // Reload systemd so it picks up the service unit files from write_files,
    // then enable+start each provisioning service (blocks until complete).
    // No deadlock: these services don't have After=cloud-final.service.
    if system_script.is_some() || boot_script.is_some() {
        runcmd.push(value!(["systemctl", "daemon-reload"]));
    }
    if system_script.is_some() {
        runcmd.push(value!(["systemctl", "enable", "--now", "rum-system.service"]));
    }
    if boot_script.is_some() {
        runcmd.push(value!(["systemctl", "enable", "--now", "rum-boot.service"]));
    }

    // Reload and restart getty to pick up the autologin dropin from write_files.
    // The dropin has After=rum-system.service rum-boot.service so on subsequent
    // boots systemd won't start the getty until provisioning completes.
    // Non-existent or skipped services (ConditionPathExists) satisfy After=
    // immediately.
    if autologin {
        runcmd.push(value!(["systemctl", "daemon-reload"]));
        runcmd.push(value!([
            "systemctl",
            "restart",
            "serial-getty@ttyS0.service"
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
            "if ! blkid -o value -s TYPE \"{}\" >/dev/null 2>&1; then",
            s.dev
        )
        .unwrap();
        writeln!(script, "  mkfs.{} \"{}\"", s.filesystem, s.dev).unwrap();
        script.push_str("fi\n");
        writeln!(script, "mkdir -p \"{}\"", s.target).unwrap();
        writeln!(
            script,
            "grep -q \"{}\" /etc/fstab || echo \"{} {} {} defaults,nofail 0 2\" >> /etc/fstab",
            s.dev, s.dev, s.target, s.filesystem
        )
        .unwrap();
        script.push_str("mount -a\n\n");
    }

    fn emit_zfs(script: &mut String, z: &ZfsFs) {
        use std::fmt::Write;
        writeln!(
            script,
            "if ! zpool list \"{}\" >/dev/null 2>&1; then",
            z.pool
        )
        .unwrap();
        let mode_arg = match z.mode.as_deref() {
            Some(m) => format!("{m} "),
            None => String::new(),
        };
        let quoted_devs: Vec<String> = z.devs.iter().map(|d| format!("\"{d}\"")).collect();
        writeln!(
            script,
            "  zpool create -o ashift=12 -O mountpoint=\"{}\" \"{}\" {}{}",
            z.target,
            z.pool,
            mode_arg,
            quoted_devs.join(" ")
        )
        .unwrap();
        script.push_str("fi\n\n");
    }

    fn emit_btrfs(script: &mut String, b: &BtrfsFs) {
        use std::fmt::Write;
        let first_dev = &b.devs[0];
        writeln!(
            script,
            "if ! blkid -o value -s TYPE \"{}\" >/dev/null 2>&1; then",
            first_dev
        )
        .unwrap();
        let mode_arg = match b.mode.as_deref() {
            Some(m) => format!("-d {m} "),
            None => String::new(),
        };
        let quoted_devs: Vec<String> = b.devs.iter().map(|d| format!("\"{d}\"")).collect();
        writeln!(script, "  mkfs.btrfs {}{}", mode_arg, quoted_devs.join(" ")).unwrap();
        script.push_str("fi\n");
        writeln!(script, "mkdir -p \"{}\"", b.target).unwrap();
        writeln!(
            script,
            "grep -q \"{}\" /etc/fstab || echo \"{} {} btrfs defaults,nofail 0 0\" >> /etc/fstab",
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
        let ud = build_user_data(None, None, &[], &[], false, &[]);
        assert!(ud.starts_with("#cloud-config\n"));
    }

    #[test]
    fn user_data_contains_user() {
        let ud = build_user_data(None, None, &[], &[], false, &[]);
        assert!(ud.contains("name: rum"));
        assert!(ud.contains("lock_passwd: false"));
    }

    #[test]
    fn user_data_autologin_dropin_in_write_files() {
        let ud = build_user_data(None, None, &[], &[], true, &[]);
        let write_files = &ud[ud.find("write_files:").unwrap()..ud.find("runcmd:").unwrap()];
        assert!(write_files.contains("autologin.conf"));
        assert!(write_files.contains("--autologin rum"));
        assert!(write_files.contains("--keep-baud 115200,38400,9600"));
        assert!(write_files.contains("%I"));
        assert!(write_files.contains("$TERM"));
    }

    #[test]
    fn user_data_autologin_absent_when_disabled() {
        let ud = build_user_data(None, None, &[], &[], false, &[]);
        assert!(!ud.contains("autologin.conf"));
        assert!(!ud.contains("--autologin rum"));
        assert!(!ud.contains("serial-getty@ttyS0.service"));
    }

    #[test]
    fn user_data_runcmd_restarts_getty() {
        let ud = build_user_data(None, None, &[], &[], true, &[]);
        assert!(ud.contains("runcmd:"));
        assert!(ud.contains("daemon-reload"));
        assert!(ud.contains("serial-getty@ttyS0.service"));
    }

    #[test]
    fn user_data_system_script() {
        let ud = build_user_data(Some("echo hello\necho world"), None, &[], &[], false, &[]);
        assert!(ud.contains("rum-system.sh"));
        assert!(ud.contains("echo hello"));
        assert!(ud.contains("echo world"));
        assert!(ud.contains("rum-system.service"));
        assert!(ud.contains("runcmd:"));
    }

    #[test]
    fn user_data_runcmd_enables_system_service() {
        let ud = build_user_data(Some("echo hello"), None, &[], &[], false, &[]);
        let runcmd = &ud[ud.find("runcmd:").unwrap()..];
        assert!(runcmd.contains("enable"));
        assert!(runcmd.contains("--now"));
        assert!(runcmd.contains("rum-system.service"));
    }

    #[test]
    fn user_data_boot_script() {
        let ud = build_user_data(None, Some("echo booting"), &[], &[], false, &[]);
        assert!(ud.contains("rum-boot.sh"));
        assert!(ud.contains("echo booting"));
        assert!(ud.contains("rum-boot.service"));
        assert!(ud.contains("systemctl"));
        assert!(ud.contains("enable"));
    }

    #[test]
    fn user_data_boot_script_absent_when_none() {
        let ud = build_user_data(None, None, &[], &[], false, &[]);
        assert!(!ud.contains("rum-boot.sh"));
        assert!(!ud.contains("/etc/systemd/system/rum-boot.service"));
    }

    #[test]
    fn user_data_system_before_boot() {
        let ud = build_user_data(Some("echo system"), Some("echo boot"), &[], &[], false, &[]);
        let runcmd = &ud[ud.find("runcmd:").unwrap()..];
        let system_pos = runcmd.find("rum-system.service").unwrap();
        let boot_pos = runcmd.find("rum-boot.service").unwrap();
        assert!(
            system_pos < boot_pos,
            "system service should be enabled before boot service in runcmd"
        );
    }

    #[test]
    fn user_data_boot_service_content() {
        let ud = build_user_data(None, Some("echo boot"), &[], &[], false, &[]);
        assert!(ud.contains("Type=oneshot"));
        assert!(ud.contains("RemainAfterExit=yes"));
        assert!(ud.contains("ExecStart=/bin/bash /var/lib/cloud/scripts/rum-boot.sh"));
        assert!(ud.contains("After=network-online.target rum-system.service"));
    }

    #[test]
    fn user_data_system_service_content() {
        let ud = build_user_data(Some("echo sys"), None, &[], &[], false, &[]);
        assert!(ud.contains("ConditionPathExists=!/var/lib/rum/.system-provisioned"));
        assert!(ud.contains("ExecStart=/bin/bash /var/lib/cloud/scripts/rum-system.sh"));
        assert!(ud.contains("ExecStartPost=/bin/touch /var/lib/rum/.system-provisioned"));
    }

    #[test]
    fn user_data_system_service_in_write_files() {
        let ud = build_user_data(Some("echo sys"), None, &[], &[], false, &[]);
        let write_files = &ud[ud.find("write_files:").unwrap()..ud.find("runcmd:").unwrap()];
        assert!(write_files.contains("rum-system.service"));
    }

    #[test]
    fn user_data_contains_virtiofs_mounts() {
        let mounts = vec![ResolvedMount {
            source: std::path::PathBuf::from("/home/user/project"),
            target: "/mnt/project".into(),
            readonly: false,
            tag: "mnt_project".into(),
        }];
        let ud = build_user_data(None, None, &mounts, &[], false, &[]);
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
        assert!(script.contains("mkfs.ext4 \"/dev/vdb\""));
        assert!(script.contains("mkdir -p \"/mnt/data\""));
        assert!(script.contains("/dev/vdb /mnt/data ext4 defaults,nofail"));
        assert!(script.contains("blkid")); // idempotency guard
    }

    #[test]
    fn drive_script_zfs_mirror() {
        let fs = vec![ResolvedFs::Zfs(ZfsFs {
            pool: "logspool".into(),
            devs: vec!["/dev/vdc".into(), "/dev/vdd".into()],
            target: "/mnt/logs".into(),
            mode: Some("mirror".into()),
        })];
        let script = build_drive_script(&fs);
        assert!(script.contains("zfsutils-linux")); // ubuntu/debian package
        assert!(script.contains("modprobe zfs"));
        assert!(script.contains("zpool list \"logspool\"")); // idempotency guard
        assert!(script.contains("zpool create"));
        assert!(script.contains("mountpoint=\"/mnt/logs\""));
        assert!(script.contains("mirror \"/dev/vdc\" \"/dev/vdd\""));
    }

    #[test]
    fn drive_script_btrfs_raid1() {
        let fs = vec![ResolvedFs::Btrfs(BtrfsFs {
            devs: vec!["/dev/vde".into(), "/dev/vdf".into()],
            target: "/mnt/fast".into(),
            mode: Some("raid1".into()),
        })];
        let script = build_drive_script(&fs);
        assert!(script.contains("btrfs-progs"));
        assert!(script.contains("mkfs.btrfs -d raid1 \"/dev/vde\" \"/dev/vdf\""));
        assert!(script.contains("mkdir -p \"/mnt/fast\""));
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
        let ud = build_user_data(None, None, &[], &fs, false, &[]);
        assert!(ud.contains("rum-drives.sh"));
        assert!(ud.contains("/bin/sh"));
        assert!(ud.contains("mkfs.ext4"));
    }

    #[test]
    fn user_data_drive_script_before_system() {
        let fs = vec![ResolvedFs::Simple(SimpleFs {
            filesystem: "ext4".into(),
            dev: "/dev/vdb".into(),
            target: "/mnt/data".into(),
        })];
        let ud = build_user_data(Some("echo hello"), None, &[], &fs, false, &[]);
        let runcmd = &ud[ud.find("runcmd:").unwrap()..];
        let drives_pos = runcmd.find("rum-drives.sh").unwrap();
        let system_pos = runcmd.find("rum-system.service").unwrap();
        assert!(
            drives_pos < system_pos,
            "drive script should run before system service in runcmd"
        );
    }

    #[test]
    fn user_data_autologin_waits_for_provisioning_services() {
        let ud = build_user_data(None, None, &[], &[], true, &[]);
        assert!(ud.contains("After=rum-system.service rum-boot.service"));
    }

    #[test]
    fn drive_script_quotes_paths_with_spaces() {
        let fs = vec![
            ResolvedFs::Simple(SimpleFs {
                filesystem: "ext4".into(),
                dev: "/dev/vdb".into(),
                target: "/mnt/my data".into(),
            }),
            ResolvedFs::Zfs(ZfsFs {
                pool: "my pool".into(),
                devs: vec!["/dev/vdc".into()],
                target: "/mnt/zfs store".into(),
                mode: None,
            }),
            ResolvedFs::Btrfs(BtrfsFs {
                devs: vec!["/dev/vde".into(), "/dev/vdf".into()],
                target: "/mnt/bt data".into(),
                mode: None,
            }),
        ];
        let script = build_drive_script(&fs);

        // ext4: all paths must be double-quoted
        assert!(script.contains("mkdir -p \"/mnt/my data\""));
        assert!(script.contains("mkfs.ext4 \"/dev/vdb\""));
        assert!(script.contains("grep -q \"/dev/vdb\" /etc/fstab"));
        assert!(script.contains("/dev/vdb /mnt/my data ext4 defaults,nofail"));

        // zfs: pool name, mountpoint, and devices must be quoted
        assert!(script.contains("zpool list \"my pool\""));
        assert!(script.contains("mountpoint=\"/mnt/zfs store\""));
        assert!(script.contains("\"my pool\""));
        assert!(script.contains("\"/dev/vdc\""));

        // btrfs: target and devices must be quoted
        assert!(script.contains("mkdir -p \"/mnt/bt data\""));
        assert!(script.contains("\"/dev/vde\" \"/dev/vdf\""));
        assert!(script.contains("/dev/vde /mnt/bt data btrfs defaults,nofail"));
    }

    #[test]
    fn user_data_getty_restart_after_provisioning() {
        let ud = build_user_data(Some("echo sys"), Some("echo boot"), &[], &[], true, &[]);
        let runcmd = &ud[ud.find("runcmd:").unwrap()..];
        let system_pos = runcmd.find("rum-system.service").unwrap();
        let boot_pos = runcmd.find("rum-boot.service").unwrap();
        let getty_pos = runcmd.find("serial-getty@ttyS0.service").unwrap();
        assert!(
            getty_pos > system_pos && getty_pos > boot_pos,
            "getty restart should be after all provisioning services"
        );
    }

    #[test]
    fn user_data_with_ssh_keys() {
        let keys = vec![
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAITest auto-generated".to_string(),
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAITest extra-key".to_string(),
        ];
        let ud = build_user_data(None, None, &[], &[], false, &keys);
        assert!(ud.contains("ssh_authorized_keys:"));
        assert!(ud.contains("ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAITest auto-generated"));
        assert!(ud.contains("ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAITest extra-key"));
    }

    #[test]
    fn user_data_without_ssh_keys_omits_authorized_keys() {
        let ud = build_user_data(None, None, &[], &[], false, &[]);
        assert!(!ud.contains("ssh_authorized_keys"));
    }
}
