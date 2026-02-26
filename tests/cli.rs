use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use std::io::Write;

fn rum() -> assert_cmd::Command {
    cargo_bin_cmd!("rum").into()
}

fn write_test_config(dir: &tempfile::TempDir) -> std::path::PathBuf {
    let config_path = dir.path().join("rum.toml");
    let mut f = std::fs::File::create(&config_path).unwrap();
    write!(
        f,
        r#"
[image]
base = "ubuntu-24.04"

[resources]
cpus = 2
memory_mb = 2048
"#
    )
    .unwrap();
    config_path
}

#[test]
fn help_works() {
    rum()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Lightweight VM provisioning"));
}

#[test]
fn missing_config_shows_error() {
    rum()
        .args(["--config", "/nonexistent/rum.toml", "up"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("failed to load config"));
}

#[test]
fn status_nonexistent_vm() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = write_test_config(&dir);

    rum()
        .args(["--config", config_path.to_str().unwrap(), "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("not defined"));
}

#[test]
fn destroy_nonexistent_vm() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = write_test_config(&dir);

    rum()
        .args(["--config", config_path.to_str().unwrap(), "destroy"])
        .assert()
        .success()
        .stdout(predicate::str::contains("nothing to destroy"));
}

#[test]
fn config_with_optional_sections() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("rum.toml");
    let mut f = std::fs::File::create(&config_path).unwrap();
    write!(
        f,
        r#"
[image]
base = "ubuntu-24.04"

[resources]
cpus = 4
memory_mb = 4096

[network]
hostname = "myhost"
wait_for_ip = false

[provision.system]
script = "echo hello"

[provision.boot]
script = "echo booting"

[advanced]
libvirt_uri = "qemu:///session"
"#
    )
    .unwrap();

    // Should parse without error — status reports "not defined" for nonexistent VM
    rum()
        .args(["--config", config_path.to_str().unwrap(), "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("not defined"));
}

#[test]
fn config_with_mounts_section() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("rum.toml");
    let mut f = std::fs::File::create(&config_path).unwrap();
    write!(
        f,
        r#"
[image]
base = "ubuntu-24.04"

[resources]
cpus = 2
memory_mb = 2048

[[mounts]]
source = "."
target = "/mnt/project"

[[mounts]]
source = "."
target = "/mnt/data"
readonly = true
tag = "data"
"#
    )
    .unwrap();

    // Should parse without error — status reports "not defined" for nonexistent VM
    rum()
        .args(["--config", config_path.to_str().unwrap(), "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("not defined"));
}

#[test]
fn config_with_network_interfaces() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("rum.toml");
    let mut f = std::fs::File::create(&config_path).unwrap();
    write!(
        f,
        r#"
[image]
base = "ubuntu-24.04"

[resources]
cpus = 2
memory_mb = 2048

[network]
nat = true

[[network.interfaces]]
network = "rum-hostonly"
ip = "192.168.50.10"

[[network.interfaces]]
network = "dev-net"
"#
    )
    .unwrap();

    rum()
        .args(["--config", config_path.to_str().unwrap(), "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("not defined"));
}

#[test]
fn config_with_drives_section() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("rum.toml");
    let mut f = std::fs::File::create(&config_path).unwrap();
    write!(
        f,
        r#"
[image]
base = "ubuntu-24.04"

[resources]
cpus = 2
memory_mb = 2048

[drives.data]
size = "20G"

[drives.scratch]
size = "50G"
"#
    )
    .unwrap();

    rum()
        .args(["--config", config_path.to_str().unwrap(), "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("not defined"));
}

#[test]
fn config_with_fs_section() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("rum.toml");
    let mut f = std::fs::File::create(&config_path).unwrap();
    write!(
        f,
        r#"
[image]
base = "ubuntu-24.04"

[resources]
cpus = 2
memory_mb = 2048

[drives.data]
size = "20G"

[drives.logs1]
size = "50G"

[drives.logs2]
size = "50G"

[[fs.ext4]]
drive = "data"
target = "/mnt/data"

[[fs.zfs]]
drives = ["logs1", "logs2"]
target = "/mnt/logs"
mode = "mirror"
pool = "logspool"
"#
    )
    .unwrap();

    rum()
        .args(["--config", config_path.to_str().unwrap(), "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("not defined"));
}

#[test]
fn named_config_derives_name() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("dev.rum.toml");
    let mut f = std::fs::File::create(&config_path).unwrap();
    write!(
        f,
        r#"
[image]
base = "ubuntu-24.04"

[resources]
cpus = 1
memory_mb = 512
"#
    )
    .unwrap();

    // Status output should use the derived name "dev"
    rum()
        .args(["--config", config_path.to_str().unwrap(), "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("'dev'"));
}

#[test]
fn init_defaults_creates_config() {
    let dir = tempfile::tempdir().unwrap();

    rum()
        .arg("init")
        .arg("--defaults")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Created rum.toml"));

    let content = std::fs::read_to_string(dir.path().join("rum.toml")).unwrap();
    assert!(content.contains("[image]"));
    assert!(content.contains("[resources]"));
    assert!(content.contains("cpus = 2"));
    assert!(content.contains("memory_mb = 2048"));
    assert!(content.contains("cloud-images.ubuntu.com"));
}

#[test]
fn init_defaults_refuses_overwrite() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("rum.toml"), "existing").unwrap();

    rum()
        .arg("init")
        .arg("--defaults")
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("already exists"));
}

#[test]
fn init_interactive_requires_tty() {
    // `cargo test` runs with piped stdin — rum init without --defaults should
    // detect non-TTY and error with a helpful message.
    let dir = tempfile::tempdir().unwrap();

    rum()
        .arg("init")
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("requires a terminal"));
}

#[test]
fn init_help_shows_defaults_flag() {
    rum()
        .args(["init", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--defaults"));
}

#[test]
fn ssh_help_works() {
    rum()
        .args(["ssh", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Connect to the VM via SSH"));
}

#[test]
fn ssh_config_help_works() {
    rum()
        .args(["ssh-config", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Print OpenSSH config"));
}

#[test]
fn config_with_ssh_section() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("rum.toml");
    let mut f = std::fs::File::create(&config_path).unwrap();
    write!(
        f,
        r#"
[image]
base = "ubuntu-24.04"

[resources]
cpus = 2
memory_mb = 2048

[ssh]
user = "admin"
command = "kitten ssh"
authorized_keys = ["ssh-ed25519 AAAA... user@host"]
"#
    )
    .unwrap();

    rum()
        .args(["--config", config_path.to_str().unwrap(), "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("not defined"));
}

#[test]
fn image_search_help_works() {
    rum()
        .args(["image", "search", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Search cloud image registry"));
}

#[test]
fn skill_prints_reference() {
    rum()
        .arg("skill")
        .assert()
        .success()
        .stdout(predicate::str::contains("rum.toml"))
        .stdout(predicate::str::contains("[image]"));
}

#[test]
fn up_detach_flag_accepted() {
    rum()
        .args(["up", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--detach"));
}

#[test]
fn serve_command_hidden() {
    rum()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("serve").not());
}

#[test]
fn cp_help_works() {
    rum()
        .args(["cp", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Copy files between host and guest"));
}

#[test]
fn cp_no_colon_prefix_errors() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = write_test_config(&dir);

    rum()
        .args([
            "--config",
            config_path.to_str().unwrap(),
            "cp",
            "/tmp/a",
            "/tmp/b",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("neither path has a : prefix"));
}

#[test]
fn config_with_ports_section() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("rum.toml");
    let mut f = std::fs::File::create(&config_path).unwrap();
    write!(
        f,
        r#"
[image]
base = "ubuntu-24.04"

[resources]
cpus = 2
memory_mb = 2048

[[ports]]
host = 8080
guest = 80

[[ports]]
host = 5432
guest = 5432
bind = "0.0.0.0"
"#
    )
    .unwrap();

    rum()
        .args(["--config", config_path.to_str().unwrap(), "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("not defined"));
}
