# Extra drives support

**ID:** 48d2425a | **Status:** Done | **Created:** 2026-02-15T14:31:21+01:00

Support attaching additional virtual disks to the VM beyond the root overlay and seed ISO.

## Implementation

- Config uses `BTreeMap<String, DriveConfig>` — key is drive name, duplicates impossible
- Drive images created as qcow2 via pure Rust `src/qcow2.rs` module (no external tools)
- Drives attached as virtio disks (vdb, vdc, ...) in domain XML
- Idempotent: existing drive images reused on subsequent `rum up`

```toml
[drives.data]
size = "20G"
target = "/mnt/data"    # optional: for future auto-mount

[drives.scratch]
size = "50G"
```

## Not yet implemented

- Auto-formatting and mounting (disk_setup/fs_setup in cloud-init) — deferred for ext4 + ZFS support

## Related

- `rum init` wizard (`e2b0661e`) includes an extra drives prompt
