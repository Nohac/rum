# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**rum** is a lightweight CLI tool (Rust) for provisioning and running single VM instances via libvirt. It uses declarative TOML config (`rum.toml`) to manage VMs with cloud images, cloud-init provisioning, and serial console access. The full specification is in `spec.md`.

CLI binary name: `rum`. Implemented commands: `up`, `down`, `destroy`, `status`.

## Build Commands

```bash
cargo build            # debug build
cargo build --release  # release build
cargo test             # run all tests
cargo test <name>      # run a single test by name
cargo clippy           # lint
cargo fmt              # format code
```

## System Requirements

- `libvirt-dev` (build-time, for virt crate C bindings)
- `qemu-img` (runtime, for overlay creation)
- `libvirt` + KVM (runtime)

No external tools needed for cloud-init ISO generation (uses `hadris-iso` crate).

## Libraries

- **Serialization**: `facet` + `facet-toml` (NOT serde)
- **Arg parsing**: `clap`
- **Async runtime**: `tokio`
- **Error handling**: `miette` with pretty formatting
- **Progress/busy indicators**: `indicatif`
- **Libvirt bindings**: `virt` crate (links against libvirt C library)
- **Image download**: `reqwest` with streaming + `futures-util`
- **ISO generation**: `hadris-iso` (pure Rust ISO 9660, no external tools)
- **Path helpers**: `dirs` (XDG directories)

## Architecture

Rust 2024 edition. Code is split into focused modules — avoid monolithic files.

### Module Map

- **`src/config.rs`** — TOML config parsing/validation via facet. Structs: `Config`, `ImageConfig`, `ResourcesConfig`, `NetworkConfig`, `ProvisionConfig`, `AdvancedConfig`. Note: facet requires both `#[facet(default)]` on the struct AND a manual `impl Default` for structs with non-zero defaults.
- **`src/paths.rs`** — XDG path helpers (`~/.cache/rum/images/`, `~/.local/share/rum/<name>/`)
- **`src/image.rs`** — Base image download/caching with reqwest streaming + indicatif progress bar
- **`src/overlay.rs`** — qcow2 overlay creation (shells out to `qemu-img`)
- **`src/cloudinit.rs`** — NoCloud seed ISO generation via `hadris-iso` (FAT or ISO 9660 with volume label "CIDATA"). Creates default `rum` user with password `rum`.
- **`src/domain_xml.rs`** — Libvirt domain XML generation from config (KVM, virtio disk, SATA CDROM, NAT network, serial console)
- **`src/backend/mod.rs`** — `Backend` trait with async methods (up, down, destroy, status)
- **`src/backend/libvirt.rs`** — Full libvirt implementation: image download, overlay/seed creation, domain define/redefine/start, serial console via `virsh console`, ACPI shutdown with timeout, destroy with purge, auto-starts default network if inactive
- **`src/cli.rs`** — Clap CLI definition
- **`src/error.rs`** — `RumError` enum with miette diagnostics and actionable hints
- **`src/main.rs`** — Entry point, wires CLI to backend

## Testing

- Focus on integration tests over unit tests
- Only unit test highly complex logic
- Keep tests short and concise
- Unit tests: domain XML generation, cloud-init user-data content, ISO validity
- Integration tests (in `tests/cli.rs`): CLI help, config validation, status/destroy of nonexistent VMs, config with all optional sections

## Key Design Decisions

- Host OS is Linux-only; requires KVM and libvirt with qemu driver
- Config format is TOML (`rum.toml`), NOT YAML
- Default libvirt URI: `qemu:///system`
- Artifacts per VM stored under `~/.local/share/rum/<name>/` (overlay, seed ISO, domain XML)
- Base images cached under `~/.cache/rum/images/`
- `rum up` is idempotent: reuses existing domain/artifacts, redefines if config changed (errors if running with changed config)
- `--reset` flag wipes overlay + seed to force fresh first boot
- `rum up` attaches serial console via `virsh console` after boot
- Default network is auto-started if defined but inactive

## Not Yet Implemented

- SSH key injection, readiness polling, `rum ssh` command
- virtiofs/9p mounts, inotify bridging
- `rum logs`
- UEFI boot, bridge networking, sha256 image verification
