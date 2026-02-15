# Extra drives support

**ID:** 48d2425a | **Status:** Open | **Created:** 2026-02-15T14:31:21+01:00

Support attaching additional virtual disks to the VM beyond the root overlay and seed ISO.

## Config syntax

```toml
[[drives]]
size = "20G"
target = "/mnt/data"    # optional: auto-mount via cloud-init
format = "qcow2"        # default: qcow2

[[drives]]
size = "50G"
target = "/mnt/scratch"
```

## Approach

1. **Create disk images** — generate qcow2 images in the VM's data dir (`~/.local/share/rum/<name>/`)
2. **Domain XML** — add `<disk>` elements with virtio bus (vdb, vdc, etc.)
3. **Cloud-init** — use `disk_setup` + `fs_setup` + `mounts` modules to partition, format, and mount on first boot
4. **Idempotency** — on subsequent `rum up`, reuse existing disk images (don't reformat)

## Domain XML changes

Additional `<disk type="file" device="disk">` elements with auto-assigned device names (vdb, vdc, ...).

## Related

- `rum init` wizard (`e2b0661e`) includes an extra drives prompt
