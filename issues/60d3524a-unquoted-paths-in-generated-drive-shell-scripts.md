# Unquoted paths in generated drive shell scripts

**ID:** 60d3524a | **Status:** Done | **Created:** 2026-02-18T18:44:34+01:00

`build_drive_script()` in `src/cloudinit.rs` interpolates mount targets, pool names, and device paths directly into shell commands without quoting. This breaks for paths containing spaces (e.g. `/mnt/my data`).

## Affected commands

- `mkdir -p {target}` — splits on spaces
- `grep -q '{target}' /etc/fstab` — breaks quoting if target contains single quotes
- `echo '... {target} ...' >> /etc/fstab` — same
- `zpool create -O mountpoint={target} {pool} ...` — splits on spaces
- `mkfs.btrfs ... {devs}` — device list unquoted

## Fix

Quote all interpolated paths in the generated shell script. Use double quotes around variables/paths to handle spaces. For single-quote contexts (grep/echo), escape embedded single quotes.

## Files

- `src/cloudinit.rs`: `build_drive_script()` (~lines 340-410)
- Add test with mount target containing a space
