# Provision types: system (first boot) and boot (every boot)

**ID:** 000f5d08 | **Status:** Done | **Created:** 2026-02-16T21:19:42+01:00

Inspired by Lima's provision types. Currently `[provision]` has a single `script` field that runs via cloud-init `runcmd` (first boot only). Split into two types:

- **`[provision.system]`** — runs once on first boot (current behavior). For package installation, system setup, user config, etc.
- **`[provision.boot]`** — runs on every boot. For starting services, refreshing state, dev environment setup, etc.

### Config

```toml
[provision.system]
script = "apt-get install -y build-essential curl git"

[provision.boot]
script = "systemctl start my-dev-server"
```

Drop the `packages` field entirely — package installation belongs in the system script. This simplifies the config and avoids a separate cloud-init code path.

### Implementation

- `[provision.system]` script → cloud-init `runcmd` (runs once per instance, current behavior)
- `[provision.boot]` script → a systemd service (`rum-boot.service`) with `Type=oneshot` + `RemainAfterExit=yes` written via `write_files`, enabled via `runcmd`. This runs after the system is fully up (unlike `bootcmd` which runs early before networking).

### Files

- `src/config.rs`: replace `ProvisionConfig { script, packages }` with nested `system`/`boot` sub-structs (script only). Remove `packages` field.
- `src/cloudinit.rs`: remove packages handling, generate boot script as a systemd unit via `write_files`, enable it via `runcmd`
- Tests for both provision types
