# rum init interactive setup wizard

**ID:** e2b0661e | **Status:** Open | **Created:** 2026-02-15T14:29:22+01:00

`rum init` — an interactive setup wizard that generates a `rum.toml` in the current directory, getting users up and running quickly.

## Wizard steps

1. **Backend detection** — detect available hypervisors (libvirt/KVM, VirtualBox, etc.) and default to what's found. Let user confirm or override.
2. **OS selection** — list commonly used cloud images (Ubuntu LTS, Fedora, Debian, Arch, Alpine, etc.) with pre-filled image URLs. Include an option to open a link for finding more images (e.g. cloud-images.ubuntu.com).
3. **Resources** — ask for CPUs and memory (sensible defaults like 2 CPUs, 2 GB RAM).
4. **Workspace mounts** — ask if the user wants to mount the current directory into the VM (default yes for dev workflows). Allow adding additional mounts.
5. **Port forwarding** — (not implemented - skip this and move to a separate issue when this is done): ask which ports to forward (common presets: SSH/22, HTTP/8080, etc.) or skip.
6. **Extra drives** — ask if additional disks are needed, with size and filesystem type/mount point (uses the `[drives.*]` + `[[fs.*]]` config sections).
7. **Provisioning** — ask for a system script (first boot) and/or boot script (every boot), using the `[provision.system]` / `[provision.boot]` sections.
8. **Write `rum.toml`** — generate the config file with all selections, including helpful comments.

## Current state

Already implemented in rum:
- Workspace mounts (`[[mounts]]`)
- Extra drives (`[drives.*]`) with auto-format/mount (`[[fs.*]]`) supporting ext4, xfs, ZFS, btrfs
- Dual-NIC networking (NAT disable/enable + host-only interfaces)

Not yet implemented (wizard should still ask, but note in generated config):
- Provision types (this should not be part of the initial wizzard) (`[provision.system]` / `[provision.boot]`) — currently flat `[provision]` with `script`

## Interactive prompts

Use [inquire](https://github.com/mikaelmello/inquire) for the interactive TUI prompts (Select, Text, Confirm, etc.).

## Notes

- Should work non-interactively too (e.g. `rum init --defaults` for zero-prompt setup)
