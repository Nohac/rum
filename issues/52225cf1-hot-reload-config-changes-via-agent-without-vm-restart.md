# Hot reload config changes via agent without VM restart

**ID:** 52225cf1 | **Status:** Open | **Created:** 2026-02-22T14:38:56+01:00

## Summary

With the rum-agent running inside the VM, many config changes can be applied live without a full `rum destroy && rum up` cycle. Detect what changed in `rum.toml`, apply what's possible via agent RPC, and tell the user what requires a restart.

## Hot-reloadable vs restart-required

### Hot-reloadable (apply via agent RPC)

| Config section | What changes | How to apply |
|---|---|---|
| `[provision.boot]` | Boot script content | Re-run via `agent.provision()` — this is what boot scripts are for |
| `[provision.system]` | System script content | Re-run via `agent.provision()` (user must opt in — destructive) |
| `[[ports]]` | Port forwards | Stop/start TCP listeners on host side — no guest changes needed |
| `[user]` | Groups | `usermod -aG` via `agent.exec()` |
| `[network.hostname]` | Hostname | `hostnamectl set-hostname` via `agent.exec()` |

### Restart-required (need `rum down && rum up` or `rum up --reset`)

| Config section | Why |
|---|---|
| `[image]` | Different base image — need fresh overlay |
| `[resources]` | CPUs/memory — libvirt domain XML change, requires redefine + restart |
| `[[mounts]]` | virtiofs — domain XML change (new filesystem devices) |
| `[drives]` / `[fs]` | Block devices — domain XML change |
| `[advanced]` | Machine type, libvirt URI — fundamental domain config |
| `[network.nat]`, `[[network.interfaces]]` | NIC topology — domain XML change |

## User experience

### `rum reload`

```
rum reload              # diff config, apply hot-reloadable changes
rum reload --dry-run    # show what would change without applying
```

Output:
```
  ✓ Updated port forwards (added 3000→3000, removed 5432→5432)
  ✓ Re-ran boot provisioning script
  ⚠ resources.memory_mb changed (1024 → 2048) — requires restart (`rum down && rum up`)
```

### Automatic reload during `rum up`

When `rum up` finds the VM already running and config has changed, instead of erroring with `RequiresRestart`, it could:
1. Check if all changes are hot-reloadable
2. If yes: apply them live, print what was updated
3. If no: show which changes need a restart, suggest `rum down && rum up`

### File watching (stretch goal)

Watch `rum.toml` for changes while `rum up` is running and auto-reload hot-reloadable values. Combined with the inotify bridge for mounts, this gives a fully live development loop.

## Implementation

### Config diffing

Add a `diff_config(old: &Config, new: &Config) -> ConfigDiff` function that returns:
```rust
struct ConfigDiff {
    port_forwards: PortDiff,        // added/removed/changed
    provision_boot: bool,            // script content changed
    provision_system: bool,          // script content changed
    user_groups: GroupsDiff,         // added groups
    hostname: Option<String>,        // new hostname
    requires_restart: Vec<String>,   // list of fields that can't be hot-reloaded
}
```

### Agent-side changes

May need new RPC methods:
- `set_hostname(name: String)` — or just use `exec("hostnamectl set-hostname ...")`
- `add_user_groups(user: String, groups: Vec<String>)` — or just use `exec("usermod -aG ...")`

Port forwards are host-only — no agent involvement needed.

### Host-side port forward management

The port forward handles are already `JoinHandle<()>` — abort old ones, start new ones. Need to extract the port forward setup into a reusable function that can be called during reload.

## Considerations

- Hot reload of provisioning scripts is powerful but potentially destructive — system scripts especially. Consider requiring `--force` for system script re-run.
- Boot scripts are designed to run on every boot, so re-running them is safe by convention.
- Config diffing should be structural (compare parsed Config structs), not textual.
- The `RequiresRestart` error in `rum up` already detects config-changed-while-running — this feature replaces that error with a smarter response.
