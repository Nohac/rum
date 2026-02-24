# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Directory Layout

The working directory may be **`rum-base/`** (parent) or **`rum-base/rum/`** (the actual git repo + Cargo project). Check which one you're in:

- **`rum-base/`** — contains the git repo (`rum/`) and worktrees (`w1/`–`w5/`). `CLAUDE.md` and `.claude/` are symlinked from `rum/`. **Cargo and git commands must be run inside `rum/`** (or the appropriate worktree).
- **`rum-base/rum/`** — the actual git repository with `Cargo.toml`, `src/`, `issues/`, etc.
- **`rum-base/w1/`–`w5/`** — git worktrees for parallel development. Each is a full checkout of the repo.

If your working directory is `rum-base/`, always `cd rum` (or `cd` into the appropriate worktree) before running `cargo`, `git`, or file-relative commands. Source files are at `rum/src/...`, not `src/...`.

## Project Overview

**rum** is a lightweight CLI tool (Rust) for provisioning and running single VM instances via libvirt. It uses declarative TOML config (`rum.toml`) to manage VMs with cloud images, cloud-init provisioning, and serial console access. The full specification is in `spec.md`.

CLI binary name: `rum`. Implemented commands: `up`, `down`, `destroy`, `status`, `ssh`, `ssh-config`, `exec`, `provision`, `log`, `init`, `image`, `skill`, `dump-iso`.

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

No external tools needed for cloud-init ISO generation (pure Rust `iso9660` module).

## Libraries

- **Serialization**: `facet` + `facet-toml` (NOT serde)
- **Arg parsing**: `clap`
- **Async runtime**: `tokio`
- **Error handling**: `miette` with pretty formatting
- **Progress/busy indicators**: `indicatif`
- **Libvirt bindings**: `virt` crate (links against libvirt C library)
- **Image download**: `reqwest` with streaming + `futures-util`
- **ISO generation**: `src/iso9660.rs` (minimal pure Rust ISO 9660 + Rock Ridge)
- **Path helpers**: `dirs` (XDG directories)

## Architecture

Rust 2024 edition. Code is split into focused modules — avoid monolithic files.

### Module Map

- **`src/config.rs`** — TOML config parsing/validation via facet. Structs: `Config`, `ImageConfig`, `ResourcesConfig`, `NetworkConfig`, `ProvisionConfig`, `AdvancedConfig`. Note: facet requires both `#[facet(default)]` on the struct AND a manual `impl Default` for structs with non-zero defaults.
- **`src/paths.rs`** — XDG path helpers (`~/.cache/rum/images/`, `~/.local/share/rum/<name>/`)
- **`src/image.rs`** — Base image download/caching with reqwest streaming + indicatif progress bar
- **`src/overlay.rs`** — qcow2 overlay creation (shells out to `qemu-img`)
- **`src/cloudinit.rs`** — NoCloud seed ISO generation (ISO 9660 with volume label "CIDATA"). Creates default `rum` user with password `rum`.
- **`src/iso9660.rs`** — Minimal pure-Rust ISO 9660 generator with Rock Ridge extensions (SUSP/RRIP). Supports flat file layout only — exactly what cloud-init seed images need.
- **`src/domain_xml.rs`** — Libvirt domain XML generation from config (KVM, virtio disk, SATA CDROM, NAT network, serial console)
- **`src/backend/mod.rs`** — `Backend` trait with async methods (up, down, destroy, status)
- **`src/backend/libvirt.rs`** — Full libvirt implementation: image download, overlay/seed creation, domain define/redefine/start, serial console via `virsh console`, ACPI shutdown with timeout, destroy, auto-starts default network if inactive
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

## Git Workflow

- **Never use `git -C`** — `cd` into the directory first, then run git commands. `git -C` cannot be whitelisted in permission settings
- Keep a **linear history** — no merge commits
- Use `git cherry-pick` to integrate feature branches onto main (preferred over merge)
- Rebase feature branches onto `main` before merging if they've diverged
- One focused commit per fix/feature with a conventional commit message (e.g. `fix:`, `feat:`, `chore:`)

## Parallel Worktree Workflow

For batching multiple fixes, use git worktrees with parallel agents:

1. **Group issues by file overlap** — issues touching the same files go in the same batch (sequential), non-overlapping issues run in parallel
2. **Create worktrees**: `cd rum && git worktree add ../wN -b fix/issue-slug`
3. **Launch `general-purpose` agents** (NOT `Bash` agents — those lack Write/Edit tools). Each agent makes changes, runs `cargo build && cargo test`, and updates the issue status. Agents must NOT run git commands — commit from the main context
4. **Commit from main context** after agents complete
5. **Cherry-pick onto main**: `git cherry-pick <hash>` for linear history. Resolve conflicts manually if branches touch the same files
6. **Reuse worktrees** between batches — switch branches with `git checkout -b fix/next-issue main` rather than removing/recreating
7. **Commit new issue files before launching agents** so worktrees have access to them
8. **Verify after each batch**: `cargo build && cargo test && cargo clippy` on main

## Issues

Tracked as markdown files in `issues/`. Create new issues with:

```bash
scripts/create-issue.sh <issue title>
```

This generates `issues/<id>-<slug>.md` with a header:

```markdown
# Issue title

**ID:** <8-char-hex> | **Status:** Open | **Created:** <iso-timestamp>
```

Below the header, write free-form markdown describing the issue — summary, approach, tasks, whatever is relevant. Keep it concise. Set **Status** to `Done` when resolved. Find open issues with:

```bash
rg 'Status:.*Open' issues/
```

## Not Yet Implemented

- SSH key injection, readiness polling, `rum ssh` command
- virtiofs/9p mounts, inotify bridging
- `rum logs`
- UEFI boot, bridge networking, sha256 image verification
