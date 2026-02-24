/// Concise markdown reference for AI coding agents managing rum VMs.
pub const SKILL_DOC: &str = r#"# rum — lightweight VM provisioning via libvirt

rum creates and manages single KVM virtual machines using declarative TOML config.
Linux-only. Requires libvirt + KVM + qemu.

## rum.toml Config Schema

### [image] (required)

| Field  | Type   | Default | Description                        |
|--------|--------|---------|------------------------------------|
| `base` | string | —       | Cloud image URL, local path, or shorthand (e.g. "ubuntu-24.04") |

### [resources] (required)

| Field       | Type   | Default | Description          |
|-------------|--------|---------|----------------------|
| `cpus`      | u32    | —       | Number of vCPUs      |
| `memory_mb` | u64    | —       | RAM in megabytes     |
| `disk`      | string | "20G"   | Root disk size       |

### [network]

| Field              | Type   | Default | Description                          |
|--------------------|--------|---------|--------------------------------------|
| `nat`              | bool   | true    | Attach default NAT network           |
| `hostname`         | string | —       | VM hostname (defaults to config name)|
| `wait_for_ip`      | bool   | true    | Wait for IP after boot               |
| `ip_wait_timeout_s`| u64    | 120     | Seconds to wait for IP               |

### [[network.interfaces]]

| Field     | Type   | Default | Description                     |
|-----------|--------|---------|---------------------------------|
| `network` | string | —       | Libvirt network name (required) |
| `ip`      | string | —       | Static IP address (optional)    |

### [provision.system]

| Field    | Type   | Description                        |
|----------|--------|------------------------------------|
| `script` | string | Shell script run once on first boot|

### [provision.boot]

| Field    | Type   | Description                     |
|----------|---------|---------------------------------|
| `script` | string | Shell script run on every boot  |

### [advanced]

| Field         | Type   | Default          | Description            |
|---------------|--------|------------------|------------------------|
| `libvirt_uri` | string | "qemu:///system" | Libvirt connection URI  |
| `domain_type` | string | "kvm"            | Domain type             |
| `machine`     | string | "q35"            | Machine type            |
| `autologin`   | bool   | false            | Auto-login on console   |

### [ssh]

| Field             | Type     | Default | Description                      |
|-------------------|----------|---------|----------------------------------|
| `user`            | string   | "rum"   | SSH username                     |
| `command`         | string   | "ssh"   | SSH client command               |
| `interface`       | string   | —       | Network interface for SSH IP     |
| `authorized_keys` | string[] | []      | Public keys injected into the VM |

### [user]

| Field    | Type     | Default | Description                |
|----------|----------|---------|----------------------------|
| `name`   | string   | "rum"   | Default user created in VM |
| `groups` | string[] | []      | Additional groups for user |

### [[mounts]]

| Field      | Type   | Default | Description                                    |
|------------|--------|---------|------------------------------------------------|
| `source`   | string | —       | Host path (".", "git", absolute, or relative)  |
| `target`   | string | —       | Guest mount point (absolute path)              |
| `readonly` | bool   | false   | Mount read-only                                |
| `tag`      | string | —       | virtiofs tag (auto-derived from target if omitted) |
| `inotify`  | bool   | false   | Enable inotify bridging                        |
| `default`  | bool   | false   | Mark as default mount (at most one)            |

### [drives.\<name\>]

| Field  | Type   | Description                 |
|--------|--------|-----------------------------|
| `size` | string | Disk size, e.g. "20G" (required) |

### [[fs.\<type\>]]

Filesystem provisioning on drives. Type is `ext4`, `xfs`, `zfs`, `btrfs`, etc.

| Field    | Type     | Description                                      |
|----------|----------|--------------------------------------------------|
| `drive`  | string   | Drive name (for simple fs: ext4, xfs, etc.)      |
| `drives` | string[] | Drive names (for zfs/btrfs)                      |
| `target` | string   | Mount point inside VM (required)                 |
| `mode`   | string   | zfs/btrfs mode (e.g. "mirror")                   |
| `pool`   | string   | zfs pool name (defaults to first drive name)     |

### [[ports]]

| Field   | Type   | Default     | Description         |
|---------|--------|-------------|---------------------|
| `host`  | u16    | —           | Host port (required)|
| `guest` | u16    | —           | Guest port (required)|
| `bind`  | string | "127.0.0.1" | Host bind address   |

## CLI Commands

```
rum up [--reset] [-d]     Create/start VM. --reset forces fresh boot. -d detaches after provisioning.
rum down                  Graceful ACPI shutdown (via daemon).
rum destroy [--purge]     Force-stop VM + daemon, undefine domain, remove artifacts. --purge removes cached image.
rum status                Show VM state, IP addresses, and daemon status.
rum exec <command>        Run a command inside the VM via vsock agent. Streams stdout/stderr, returns exit code.
rum ssh [args...]         SSH into VM (passes extra args to ssh).
rum ssh-config            Print OpenSSH config block for the VM.
rum init [--defaults]     Create rum.toml in current directory. --defaults skips prompts.
rum image list            List cached base images.
rum image delete <name>   Delete a specific cached image.
rum image clear           Delete all cached images.
rum image search [query]  Search cloud image registry, update rum.toml.
rum provision [--system] [--boot]  Re-run provisioning scripts on a running VM via vsock agent.
rum log [--failed|--all|--rum]  View provisioning and runtime logs.
rum skill                 Print this reference document.
```

## Architecture: Daemon and Agent

`rum up` spawns a background **daemon** (via `roam`) that manages the VM lifecycle. The daemon stays running after `rum up` exits and handles `rum down`, `rum status`, `rum ssh-config`, etc. via IPC.

A **vsock agent** binary is embedded in rum and deployed into the VM at first boot via the cloud-init seed ISO. The agent runs as a systemd service inside the guest and communicates with the host over virtio-vsock (no network required). `rum exec` uses this agent to run commands inside the VM.

**Detached mode** (`rum up -d`): provisions the VM and exits immediately — no serial console is attached. The daemon continues running in the background. This is the mode to use for programmatic/agent workflows. When stdin is not a terminal, detached mode is used automatically.

## Common Workflows

**Create and start a VM (interactive):**
1. Create `rum.toml` with image and resources (or run `rum init --defaults`)
2. Run `rum up` — downloads image, creates overlay, boots VM, attaches console
3. Ctrl-] to detach from console (daemon keeps running)

**Create and start a VM (programmatic):**
1. Create `rum.toml`
2. `rum up -d` — provisions and exits, daemon runs in background
3. `rum exec <command>` — run commands inside the VM
4. `rum down` or `rum destroy` when done

**Re-run provisioning scripts (no reboot):**
`rum provision` — re-runs all scripts. `--system` or `--boot` to filter by type.

**Re-provision from scratch:**
`rum up --reset` — wipes overlay and seed ISO, forces fresh first boot

**Run commands inside the VM:**
`rum exec "apt-get update && apt-get install -y curl"` — runs via vsock agent, streams output, returns exit code

**SSH into a running VM:**
`rum ssh` or `rum ssh -- -L 8080:localhost:80` (with port forwarding)

**Tear down completely:**
`rum destroy --purge` — force-stops VM and daemon, removes domain, artifacts, and cached base image

**Named configs (multiple VMs in one directory):**
Name the file `dev.rum.toml` — the VM gets name "dev". Use `rum -c dev.rum.toml up`.

## Full Lifecycle (for AI agents)

An AI agent can manage a VM entirely through CLI commands:

```sh
# 1. Create config
rum init --defaults        # or write rum.toml directly

# 2. Boot VM in background
rum up -d                  # provisions, starts daemon, exits

# 3. Check readiness
rum status                 # shows state, IPs, daemon status

# 4. Run commands inside VM
rum exec "whoami"          # single command
rum exec "cd /mnt/project && make test"  # compound command

# 5. Inspect logs on failure
rum log                    # latest script log
rum log --failed           # latest failed script log
rum log --rum              # rum's own debug log

# 6. Tear down
rum destroy                # stop VM + daemon, remove artifacts
```

All commands are idempotent and non-interactive. `rum exec` returns the command's exit code, so agents can check `$?` for success/failure. Output from `rum exec` streams to stdout/stderr in real time.

## Constraints

- Linux-only (KVM + libvirt + qemu required)
- Config format is TOML, not YAML
- Default user is `rum` with password `rum`
- Base images cached in `~/.cache/rum/images/`
- VM artifacts stored in `~/.local/share/rum/<id>/`
- `rum up` is idempotent — safe to run repeatedly

## Example Configs

**Minimal:**
```toml
[image]
base = "ubuntu-24.04"

[resources]
cpus = 2
memory_mb = 2048
```

**With provisioning and mounts:**
```toml
[image]
base = "ubuntu-24.04"

[resources]
cpus = 4
memory_mb = 4096
disk = "40G"

[network]
hostname = "devbox"

[[mounts]]
source = "."
target = "/mnt/project"

[provision.system]
script = """
apt-get update && apt-get install -y build-essential
"""

[ssh]
authorized_keys = ["ssh-ed25519 AAAA... user@host"]

[[ports]]
host = 8080
guest = 80
```

**With drives and filesystems:**
```toml
[image]
base = "ubuntu-24.04"

[resources]
cpus = 2
memory_mb = 2048

[drives.data]
size = "50G"

[[fs.ext4]]
drive = "data"
target = "/mnt/data"
```
"#;
