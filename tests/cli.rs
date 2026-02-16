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
        .stdout(predicate::str::contains("destroyed"));
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

[provision]
script = "echo hello"
packages = ["curl", "git"]

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
target = "/mnt/data"

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
