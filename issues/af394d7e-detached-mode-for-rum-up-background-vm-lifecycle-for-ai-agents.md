# Detached mode for rum up (background VM lifecycle for AI agents)

**ID:** af394d7e | **Status:** Open | **Created:** 2026-02-22T16:20:41+01:00

## Problem

`rum up` currently blocks forever — it provisions the VM, then enters a "Press Ctrl+C to stop..." loop that keeps the process alive for log streaming, port forwarding, and inotify bridging. This is fine for interactive use but makes rum unusable for AI agents: an agent can't run `rum up`, wait for it to finish, then run `rum exec` or `rum ssh` as separate steps.

For an agent workflow to work, every command must start, do its job, and exit.

## Current `rum up` responsibilities after boot

These things keep the process alive:
1. **Port forwards** — vsock-based TCP tunnels (`start_port_forwards`)
2. **Log subscription** — streams guest logs to host (`start_log_subscription`)
3. **Inotify bridge** — syncs filesystem events to guest (`start_inotify_bridge`)
4. **Ctrl+C handler** — graceful shutdown on interrupt
5. **Domain state polling** — detects external `rum down`/`rum destroy`

## Architectural challenge

rum was designed as a foreground orchestrator — several features require a persistent host-side process:

- **Port forwards** use vsock between host and guest. Both sides are needed: the guest agent and a host-side TCP listener. If rum exits, the host listener dies and port forwards stop.
- **Inotify bridge** watches host filesystem events and relays them over SSH to the guest. Requires a host process.
- **Log subscription** streams guest logs to the host terminal.

Simply adding `--detach` and exiting after boot would break port forwards and inotify. These aren't optional nice-to-haves — port forwards are how users access services in the VM.

## Approach: Self-spawn + Unix socket

`rum up -d` provisions the VM, then spawns itself as a background process to manage port forwards / inotify / logs. A Unix socket in the state folder provides IPC between the background process and subsequent CLI invocations.

### How it works

1. **`rum up -d`** (foreground, exits):
   - Provisions VM (image, overlay, seed, domain, start, wait for agent, run scripts)
   - Spawns `rum up --serve <id>` as a detached child process (double-fork or `Command::new(std::env::current_exe())`)
   - Prints status, exits 0

2. **`rum up --serve <id>`** (background, long-running):
   - Creates `~/.local/share/rum/<id>/rum.sock` (Unix domain socket)
   - Starts port forwards, inotify bridge, log subscription
   - Listens on the socket for commands from other rum invocations
   - Removes socket on clean shutdown

3. **`rum down`** / **`rum destroy`** / **`rum status`**:
   - Check for `rum.sock` in the state folder
   - If socket exists: send shutdown command over it (for down/destroy), or query status
   - If socket gone: process died, fall through to direct libvirt operations

4. **`rum up`** (no `-d` flag): current behavior unchanged — provisions and blocks in foreground

### Socket protocol

Keep it minimal — a simple line-based protocol or even just "presence = alive":

- `rum down` → sends `shutdown` over socket → background process does graceful ACPI shutdown, cleans up, exits
- `rum status` → connects to socket to confirm background process is alive, then queries libvirt as usual
- `rum destroy` → sends `destroy` over socket → background process force-stops VM, removes artifacts, exits

### Staleness handling

- If the socket file exists but nobody is listening (connect fails): background process crashed. Remove stale socket, proceed with direct libvirt operations.
- Background process writes its PID to `rum.pid` as a secondary check.

### State folder layout

```
~/.local/share/rum/<id>/
├── overlay.qcow2
├── seed-<hash>.iso
├── domain.xml
├── config_path
├── ssh_key / ssh_key.pub
├── rum.sock          ← new: Unix socket for IPC
├── rum.pid           ← new: background process PID
└── logs/
```

### What changes

- **`src/backend/libvirt.rs`**: split the post-boot loop (port forwards, inotify, log streaming, ctrl-c wait) into a separate function that `--serve` mode calls
- **`src/cli.rs`**: add `--detach` / `-d` flag to `Up`, add hidden `--serve` internal flag
- **`src/main.rs`**: handle `-d` (spawn background process after provisioning) and `--serve` (enter background loop)
- **New `src/daemon.rs`**: Unix socket listener, IPC protocol, self-spawn logic
- **`rum down` / `rum destroy`**: check for socket, send commands before falling through to libvirt

### Non-interactive default

When stdin is not a TTY (Plain mode), `rum up` could default to detach behavior — provision and exit. This makes `echo "" | rum up` and agent-driven `rum up` work out of the box without needing `-d`.

## Scope estimate

This is a medium-sized change:
- Socket IPC and self-spawn: new module, ~150-200 lines
- Refactoring the post-boot loop out of `libvirt.rs`: mostly moving code
- CLI changes: small
- Integration with `down`/`destroy`/`status`: small per command
