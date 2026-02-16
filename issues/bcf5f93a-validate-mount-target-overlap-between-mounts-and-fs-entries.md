# Validate mount target overlap between mounts and fs entries

**ID:** bcf5f93a | **Status:** Open | **Created:** 2026-02-16T21:12:48+01:00

Currently nothing prevents a virtiofs mount (`[[mounts]]`) and a filesystem entry (`[[fs.*]]`) from targeting the same mount point, or one being a subdirectory of the other. This would cause confusing runtime failures inside the VM.

### Validation rules

In `validate_config()`, collect all mount targets from both `config.mounts` and `config.fs` entries, then check:

1. **Exact duplicates** — no two entries (across mounts and fs) share the same target path
2. **Prefix overlap** — no target is a parent of another (e.g. `/mnt/data` and `/mnt/data/sub`), since mounting over an existing mount hides the underlying data

### Error messages

Should be user-friendly and identify both conflicting entries, e.g.:

- `"mount target '/mnt/data' is used by both [[mounts]] and [[fs.ext4]]"`
- `"mount target '/mnt/data/sub' overlaps with '/mnt/data' (from [[fs.ext4]])"`

### Files

- `src/config.rs`: add overlap checks in `validate_config()` after existing mount/fs validation
- `src/config.rs`: add unit tests for exact overlap and prefix overlap cases
