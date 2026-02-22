# VM snapshot and rollback support

**ID:** c48a9373 | **Status:** Open | **Created:** 2026-02-22T14:35:33+01:00

## Summary

Add qcow2 snapshot support so users can save and restore VM state at any point. This is a foundational feature that enables provisioning retry (595c06e8) and faster test iteration.

## Commands

```
rum snapshot create [name]    # create a named snapshot (default: auto-timestamped)
rum snapshot list              # list available snapshots
rum snapshot restore <name>    # roll back to a snapshot (VM must be stopped)
rum snapshot delete <name>     # remove a snapshot
```

## Implementation approach

### Internal snapshots (qcow2)

Use libvirt's `virDomainSnapshotCreateXML` for live/offline snapshots stored inside the qcow2 overlay. This is simpler than external snapshots and doesn't require managing extra files.

Key API:
- `Domain::snapshot_create_xml()` — create snapshot
- `Domain::snapshot_list_names()` — list snapshots
- `DomainSnapshot::revert_to()` — restore
- `DomainSnapshot::delete()` — remove

### Snapshot metadata

Store snapshot metadata (name, timestamp, description) in the snapshot XML. No extra files needed — libvirt tracks everything.

### CLI wiring

- Add `Snapshot` subcommand to `cli.rs` with `Create`, `List`, `Restore`, `Delete` variants
- Dispatch in `main.rs`
- Add `snapshot()` method to `Backend` trait or implement directly in libvirt backend

## Considerations

- Internal snapshots work with qcow2 overlays (which rum already uses)
- Live snapshots capture memory state too — useful for instant restore
- Offline snapshots are disk-only — faster, smaller
- Snapshots increase overlay file size — consider warning users about disk usage
- `rum destroy` should mention if snapshots exist before wiping
