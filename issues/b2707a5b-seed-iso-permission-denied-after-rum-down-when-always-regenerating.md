# seed ISO permission denied after rum down when always regenerating

**ID:** b2707a5b | **Status:** Done | **Created:** 2026-02-14T12:55:34+01:00

## Summary

After `rum down` + `rum up`, seed ISO regeneration fails with "Permission denied" because the existing file is owned by root (created during a previous `rum up` via `qemu:///system`). Regression from 0e2e325 which removed the `if !seed_path.exists()` guard.

## Approach

Remove the existing seed ISO before writing the new one in `cloudinit::generate_seed_iso()`. The `remove_file` can fail silently (file may not exist).
