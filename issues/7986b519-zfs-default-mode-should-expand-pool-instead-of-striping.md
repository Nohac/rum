# ZFS default mode should expand pool instead of striping

**ID:** 7986b519 | **Status:** Open | **Created:** 2026-02-16T21:12:44+01:00

When no `mode` is specified for a `[[fs.zfs]]` entry, the default should create an expanding pool — each drive is added as a separate top-level vdev so the pool's usable capacity is the sum of all drives. Currently the default omits the vdev keyword which results in a stripe (same effect, but the intent should be explicit).

In `zpool create` terms, adding drives without a vdev keyword (`zpool create pool /dev/vdb /dev/vdc`) already does this — each disk is its own vdev and the pool stripes across them. So the current generated script is technically correct, but the `mode` field semantics and documentation should make "expand" the named default rather than calling it "stripe".

### Changes

- `src/config.rs` `resolve_fs()`: when `mode` is empty for zfs, set it to `""` (no vdev keyword) — this is already the behavior, just clarify in comments/docs that the default is an expanding pool
- `rum.toml` example comment: change "stripe (default)" to something like "expand (default) — pool size is sum of all drives"
- Consider whether to accept `mode = "expand"` as an alias that maps to no vdev keyword
