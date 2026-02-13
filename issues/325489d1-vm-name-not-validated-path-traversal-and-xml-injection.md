# VM name not validated - path traversal and XML injection

**ID:** 325489d1 | **Status:** Open | **Created:** 2026-02-13T21:18:40+01:00

## Summary

The `name` field in `rum.toml` is only checked for emptiness (`config.rs:96`). Names like `../../etc` cause path traversal in `paths::work_dir`, and names with XML special chars (`<`, `>`, `&`) produce malformed domain XML since `domain_xml.rs` uses `format!()` interpolation without escaping.

## Approach

Add name validation in `config.rs` — reject names that don't match `[a-zA-Z0-9][a-zA-Z0-9._-]*`. This catches both path traversal and XML injection at the config layer.

## Tasks

- [ ] Add regex/char validation for `config.name`
- [ ] Add tests for invalid names
