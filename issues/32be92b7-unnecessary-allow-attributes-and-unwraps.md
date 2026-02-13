# Unnecessary allow attributes and unwraps

**ID:** 32be92b7 | **Status:** Done | **Created:** 2026-02-13T21:18:40+01:00

## Summary

Cleanup items across the codebase:
- `backend/mod.rs:6` — `#[allow(async_fn_in_trait)]` unnecessary in Rust 2024 edition
- `error.rs:1` — `#[allow(unused_assignments)]` too broad; should be scoped or removed
- `main.rs:25,14`, `image.rs:60` — bare `.unwrap()` calls should use `.expect()` with messages
- `overlay.rs:22-25` — `to_string_lossy()` for qemu-img paths; should pass `Path` directly as `OsStr` args

## Approach

Address each individually — remove stale allows, replace `.unwrap()` with `.expect()`, use `OsStr` args.

## Tasks

- [ ] Remove `#[allow(async_fn_in_trait)]`
- [ ] Scope or remove `#[allow(unused_assignments)]`
- [ ] Replace bare `.unwrap()` with `.expect()` messages
- [ ] Use `Path`/`OsStr` args in overlay.rs
