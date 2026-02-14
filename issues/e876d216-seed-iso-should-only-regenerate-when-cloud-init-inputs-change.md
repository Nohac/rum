# seed ISO should only regenerate when cloud-init inputs change

**ID:** e876d216 | **Status:** Done | **Created:** 2026-02-14T12:57:23+01:00

## Summary

The seed ISO is currently always regenerated on every `rum up` (since 0e2e325). This is wasteful and causes a permission-denied error (b2707a5b) when the existing file is root-owned. The previous behavior (skip if exists) missed config changes. Neither extreme is right.

## Approach

Hash the cloud-init inputs (hostname, provision script, packages) and embed the hash in the seed ISO filename: `seed-<hash>.iso`. If the file exists, skip generation entirely. If not, generate it and remove any old `seed-*.iso` files.

- Hash inputs with a short hash (e.g. first 8 chars of a SHA-256 or simple hash)
- `paths::seed_path` takes the hash and returns `~/.local/share/rum/<name>/seed-<hash>.iso`
- In `up`: compute hash, check if `seed-<hash>.iso` exists, generate only if missing
- Clean up old `seed-*.iso` files when generating a new one (config changed)
- Update `domain_xml.rs` to use the new seed path
- Supersedes the blunt fix in b2707a5b
