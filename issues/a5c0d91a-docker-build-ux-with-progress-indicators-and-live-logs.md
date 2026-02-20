# Docker-build UX with progress indicators and live logs

**ID:** a5c0d91a | **Status:** Open | **Created:** 2026-02-20T22:51:40+01:00

## Summary

Replace the current `println!` step output with a docker-build-like UX showing numbered steps, spinners, live log output, and completion indicators. Two phases of boot visibility: TTY forwarding for early boot, then agent log streaming once the agent is ready.

## Current state

```
Ensuring base image...
Creating disk overlay...
Generating cloud-init seed...
Configuring VM...
Checking network...
Starting VM...
Waiting for agent...
Forwarding 127.0.0.1:8080 → guest:80
VM is running. Press Ctrl+C to stop...
```

No live output during boot, no script output, no indication of what's happening inside the VM.

## Target UX

```
[1/7] ✓ Base image cached
[2/7] ✓ Overlay created
[3/7] ✓ Cloud-init seed generated
[4/7] ✓ Domain configured
[5/7] ⠋ Booting VM...
        [    1.234567] kernel: EXT4-fs (vda1): mounted filesystem
        cloud-init: running module apt_configure
        cloud-init: running module write_files
[6/7] ⠋ Running system provisioning...
        + apt-get update
        Hit:1 http://archive.ubuntu.com/ubuntu jammy InRelease
        + apt-get install -y postgresql
        Setting up postgresql-14 ...
[7/7] ✓ Agent ready
      → Forwarding 127.0.0.1:8080 → guest:80
      → Forwarding 127.0.0.1:5432 → guest:5432
VM is running. Press Ctrl+C to stop.
```

When a step completes, its log lines collapse to a single checkmark line. The active step shows a spinner with scrolling log output underneath (last N lines, similar to docker build).

## Design

### Two-phase boot visibility

**Phase 1: TTY forwarding (pre-agent)**
- After `dom.create()`, connect to the libvirt serial console (was previously implemented, can be brought back)
- Display kernel and cloud-init output under the "Booting VM" step
- This covers everything from BIOS → kernel → cloud-init → agent start

**Phase 2: Agent log stream (post-agent)**
- Once `wait_for_agent()` succeeds, switch from TTY to the structured agent log stream (issue `042ee8d2`)
- Agent-driven script execution sends stdout/stderr as log events
- Display under "Running system provisioning" / "Running boot script" steps

### Terminal rendering

Use `indicatif::MultiProgress` with custom draw targets:

- Each step is a `ProgressBar` with a spinner or checkmark prefix
- Log lines are rendered as additional bars below the active step (ring buffer of last ~10 lines)
- On step completion: remove log lines, update step bar to checkmark
- `console` crate for terminal width detection and truncation

### Step sequence

1. Base image (download with progress bar if needed, or instant "cached")
2. Overlay creation
3. Seed ISO generation
4. Domain configuration (define/redefine)
5. VM boot (TTY output: kernel → cloud-init)
6. System provisioning (agent exec, if configured)
7. Boot script (agent exec, if configured)
8. Ready (show port forwards, IP address)

Steps 6-7 only appear if provisioning scripts are configured.

### Quiet/verbose modes

- Default: collapsed steps with live logs under active step
- `--verbose` / `-v`: don't collapse completed step logs
- `--quiet` / `-q`: no log output, just step completion lines

## Dependencies

- Depends on: agent tracing/log forwarding (`042ee8d2`) for phase 2
- Phase 1 (TTY forwarding) can be implemented independently
- Blocks: better experience for integration test lib (`6215eef7`)

## Tasks

- [ ] Bring back libvirt serial console TTY forwarding (read-only, for boot logs)
- [ ] Build step-based progress renderer with `indicatif::MultiProgress`
- [ ] Integrate TTY output as log lines under the boot step
- [ ] Integrate agent log stream under provisioning steps (after `042ee8d2`)
- [ ] Add `--verbose` / `--quiet` flags
- [ ] Handle terminal resize and non-TTY output (CI: plain line output)
