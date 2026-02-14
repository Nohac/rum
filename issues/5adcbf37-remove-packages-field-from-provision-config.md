# Remove packages field from provision config

**ID:** 5adcbf37 | **Status:** Open | **Created:** 2026-02-14T16:05:47+01:00

The `packages` field in `[provision]` is unreliable — it assumes a specific package manager and breaks when the user switches distros (e.g. apt vs dnf vs pacman). Users should install packages via the provision script instead, which gives them full control.

## Approach

1. Remove `packages: Vec<String>` from `ProvisionConfig` in `src/config.rs`
2. Remove package-related logic from `build_user_data()` in `src/cloudinit.rs` (the `packages:` cloud-init key)
3. Remove `packages` from `seed_hash()` inputs
4. Update tests that reference packages
5. Update example `rum.toml` — move `packages = ["curl"]` into the script
6. Consider printing a deprecation warning if `packages` key is present in TOML (facet may just ignore unknown keys)
