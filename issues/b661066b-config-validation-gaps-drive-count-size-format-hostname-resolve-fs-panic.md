# Config validation gaps: drive count, size format, hostname, resolve_fs panic

**ID:** b661066b | **Status:** Open | **Created:** 2026-02-18T18:44:35+01:00

Several missing validations in `src/config.rs` that cause confusing runtime failures instead of clear config errors.

## Items

1. **Drive count limit** — `resolve_drives()` uses `(b'b' + i as u8) as char` for device names. More than 24 drives goes past 'z', producing invalid device names. Validate `drives.len() <= 24` in `validate_config()`.

2. **Drive size format** — `validate_config()` only checks `size.is_empty()`. Invalid formats like `"20X"` or `"abc"` pass validation and fail later in `qcow2::create_qcow2()`. Pre-validate with `parse_size()` during config validation.

3. **Hostname format** — No validation that an explicit hostname is valid for Linux (alphanumerics, hyphens, dots, length <= 253). Special characters or spaces would cause cloud-init issues.

4. **`resolve_fs()` returns Vec, not Result** — Does `drive_map[name]` which panics if the drive name isn't found. Validation catches this first, but the function signature is unsound. Should return `Result<Vec<ResolvedFs>, RumError>`.

## Files

- `src/config.rs`: `validate_config()`, `resolve_drives()`, `resolve_fs()`
- Update callers of `resolve_fs()` to handle `Result`
