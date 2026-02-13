use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use std::io::Write;

fn rum() -> assert_cmd::Command {
    cargo_bin_cmd!("rum").into()
}

#[test]
fn help_works() {
    rum().arg("--help").assert().success().stdout(
        predicate::str::contains("Lightweight VM provisioning"),
    );
}

#[test]
fn up_with_valid_config_returns_not_implemented() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("rum.toml");
    let mut f = std::fs::File::create(&config_path).unwrap();
    write!(
        f,
        r#"
name = "test-vm"

[image]
base = "ubuntu-24.04"

[resources]
cpus = 2
memory_mb = 2048
"#
    )
    .unwrap();

    rum()
        .args(["--config", config_path.to_str().unwrap(), "up"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not yet implemented"));
}

#[test]
fn down_with_valid_config_returns_not_implemented() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("rum.toml");
    let mut f = std::fs::File::create(&config_path).unwrap();
    write!(
        f,
        r#"
name = "test-vm"

[image]
base = "ubuntu-24.04"

[resources]
cpus = 2
memory_mb = 2048
"#
    )
    .unwrap();

    rum()
        .args(["--config", config_path.to_str().unwrap(), "down"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not yet implemented"));
}

#[test]
fn validation_rejects_empty_name() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("rum.toml");
    let mut f = std::fs::File::create(&config_path).unwrap();
    write!(
        f,
        r#"
name = ""

[image]
base = "ubuntu-24.04"

[resources]
cpus = 2
memory_mb = 2048
"#
    )
    .unwrap();

    rum()
        .args(["--config", config_path.to_str().unwrap(), "up"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("name must not be empty"));
}

#[test]
fn missing_config_shows_error() {
    rum()
        .args(["--config", "/nonexistent/rum.toml", "up"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("failed to load config"));
}
