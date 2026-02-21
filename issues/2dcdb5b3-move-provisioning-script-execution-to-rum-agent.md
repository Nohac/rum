# Move provisioning script execution to rum-agent

**ID:** 2dcdb5b3 | **Status:** Done | **Created:** 2026-02-21T00:41:13+01:00

## Summary

Move all provisioning script execution from cloud-init systemd services to the rum-agent. The agent becomes the single orchestrator of guest-side setup: drives, system provisioning, and boot scripts — all streamed back to the host as log events.

## Current state

Provisioning scripts are deployed as files via cloud-init `write_files` and executed via systemd services (`rum-system.service`, `rum-boot.service`) enabled in `runcmd`. The drive setup script (`rum-drives.sh`) also runs in `runcmd`. None of this output is visible to the host.

## Design

### Generic ordered script system

Every script is represented as:

```rust
struct ProvisionScript {
    name: String,
    content: String,
    order: u32,
    run_on: RunOn,  // System (first boot) | Boot (every boot)
}
```

The host builds the full list from config and internal scripts:

| Source | name | order | run_on |
|---|---|---|---|
| generated from `[[fs]]` | `drives` | 0 | System |
| `provision.system` | `system` | 100 | System |
| `provision.boot` | `boot` | 200 | Boot |

### RPC flow

1. Host connects to agent after `wait_for_agent()`
2. Host calls `agent.provision(scripts, log_tx)` with the full script list
3. Agent checks first-boot sentinel (`/var/lib/rum/.system-provisioned`)
   - First boot: run all scripts (System + Boot) sorted by order
   - Subsequent boot: run only Boot scripts sorted by order
4. Agent executes each script via `sh -c`, streaming stdout/stderr as `LogEvent`s
5. On successful first boot (all System scripts pass), agent creates the sentinel
6. Returns result indicating success or which script failed

### What stays in cloud-init

Cloud-init shrinks to the bare minimum:

- Create `rum` user with SSH keys
- Deploy agent binary + systemd service via `write_files`
- Start agent via `runcmd` (`systemctl enable --now rum-agent.service`)
- Autologin dropin (if enabled)
- Mount point `mkdir -p` and virtiofs `mounts:` entries

Scripts are no longer deployed as files via `write_files` — they go over RPC. The `rum-system.service`, `rum-boot.service`, and `rum-drives.sh` cloud-init artifacts are all removed.

### What changes in cloud-init ordering

Before: `mkdir` → `drives.sh` → `daemon-reload` → agent → system service → boot service
After: `mkdir` → `daemon-reload` → agent (agent handles drives + provisioning)

The agent service should start early (`After=local-fs.target`) so it can handle drive setup before anything else that might need the drives.

### Boot script on VM reboot

Because the agent is a systemd service that starts on every boot, it re-runs the provisioning flow automatically. On reboot the agent starts, connects don't apply (host may not be running `rum up`), but the agent checks the sentinel and runs Boot scripts independently. The host sees the output if it happens to be connected via log subscription.

This means the agent needs to be able to run provisioning autonomously (not just when the host pushes scripts). Two options:

**Option A: Scripts cached in guest** — On first `provision()` call, agent saves scripts to a well-known path (e.g. `/var/lib/rum/scripts/`). On subsequent boots, agent reads cached scripts and runs Boot-type ones automatically on startup.

**Option B: Scripts always pushed by host** — Agent only runs scripts when the host calls `provision()`. Boot scripts only run when `rum up` is active. Simpler, but boot scripts won't run on autonomous reboots.

Option A is recommended — it preserves the current behavior where boot scripts run on every VM boot regardless of host involvement.

### Agent-side changes

- New `provision()` RPC method: `async fn provision(&self, scripts: Vec<ProvisionScript>, output: Tx<LogEvent>) -> ProvisionResult`
- On startup: check for cached scripts, run Boot scripts if not first boot
- First-boot sentinel: `/var/lib/rum/.system-provisioned`
- Cache scripts to `/var/lib/rum/scripts/` for autonomous boot execution

### Host-side changes

- **`cloudinit.rs`**: Remove `rum-system.service`, `rum-boot.service`, `rum-drives.sh` from `write_files` and `runcmd`. Remove all provisioning-related systemd service constants. Keep agent binary deployment.
- **`libvirt.rs`**: After `wait_for_agent()`, call `agent.provision(scripts)` with the ordered script list. Stream output via existing log subscription.
- **`agent.rs`**: Add `provision()` RPC call, define `ProvisionScript` / `ProvisionResult` types in `rum-agent` lib.
- **`cloudinit.rs` seed_hash**: Still needs to include script content (affects whether seed needs regeneration for agent binary changes, but scripts themselves no longer affect the seed).
- **Autologin dropin**: Remove `After=rum-system.service rum-boot.service` — those services no longer exist.

### Error handling

- If a script fails (non-zero exit), stop execution and return the failure
- Host reports which script failed with its output
- `rum up` exits with error, VM stays running (user can fix and retry)

## Dependencies

- Split from: `a5c0d91a` (Docker-build UX)
- Blocks: `a5c0d91a` (UX needs agent log stream from provisioning)
- Existing infrastructure: agent `exec()` RPC, log subscription, vsock transport

## Tasks

- [ ] Define `ProvisionScript`, `RunOn`, `ProvisionResult` types in `rum-agent` lib
- [ ] Add `provision()` RPC method to agent trait + implementation
- [ ] Agent startup: load cached scripts, run Boot scripts autonomously
- [ ] Agent: first-boot sentinel check + creation
- [ ] Agent: cache scripts to `/var/lib/rum/scripts/` on first provision call
- [ ] Host: build ordered script list from config + generated drive script
- [ ] Host: call `agent.provision()` after `wait_for_agent()`
- [ ] Strip cloud-init of provisioning services and drive script
- [ ] Update seed_hash to exclude scripts (no longer in ISO)
- [ ] Update autologin dropin (remove After= for removed services)
- [ ] Update tests
