# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**rum** is a lightweight CLI tool (Rust) for provisioning and running single VM instances via libvirt. It uses declarative YAML config (`rum.yaml`) to manage VMs with cloud images, virtiofs mounts, cloud-init provisioning, and SSH access. The full specification is in `spec.md`.

CLI binary name: `rum`. Commands: `up`, `down`, `destroy`, `status`, `ssh`, `logs`.

## Build Commands

```bash
cargo build            # debug build
cargo build --release  # release build
cargo test             # run all tests
cargo test <name>      # run a single test by name
cargo clippy           # lint
cargo fmt              # format code
```

## Libraries

- **Serialization**: `facet` (NOT serde)
- **Arg parsing**: `clap`
- **Async runtime**: `tokio`
- **Error handling**: `miette` with pretty formatting; track source locations for config errors
- **Progress/busy indicators**: `indicatif`

## Architecture

The project is in early scaffolding stage (Rust 2024 edition). Event-driven architecture with state machines (Rust enums) where appropriate. Code is split into focused modules — avoid monolithic files.

- **Config layer**: YAML config parsing/validation via facet
- **Libvirt integration**: Domain XML generation, VM lifecycle (define, start, stop, undefine) via virt crate
- **Image management**: Base image download/caching, qcow2 overlay creation (shells out to `qemu-img`)
- **Cloud-init**: NoCloud seed ISO generation (meta-data, user-data)
- **Networking**: IP discovery via libvirt DHCP leases / domifaddr
- **SSH**: Key generation, readiness polling, connection proxy (invokes openssh)
- **Mounts**: virtiofs (preferred) with 9p fallback
- **Inotify bridge** (optional): Host file watcher (`notify` crate) forwarding events to guest agent over SSH

## Testing

- Focus on integration tests over unit tests
- Only unit test highly complex logic
- Keep tests short and concise

## Key Design Decisions from Spec

- Host OS is Linux-only; requires KVM and libvirt with qemu driver
- Default libvirt URI: `qemu:///system`
- Artifacts per VM stored under `~/.local/share/rum/<name>/` (overlay, seed ISO, domain XML, state, keys)
- Base images cached under `~/.cache/rum/images/`
- `rum up` is idempotent: reuses existing domain/artifacts, redefines if config changed (requires restart if running)
- `--reset` flag wipes overlay + seed to force fresh first boot
