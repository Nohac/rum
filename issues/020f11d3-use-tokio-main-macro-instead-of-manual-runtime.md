# Use tokio main macro instead of manual runtime

**ID:** 020f11d3 | **Status:** Done | **Created:** 2026-02-13T21:14:04+01:00

## Summary

`main.rs` manually constructs a Tokio runtime with `runtime::Builder::new_multi_thread()` but uses no custom settings. `#[tokio::main]` is cleaner and more idiomatic.

## Approach

Convert `main` to `async fn` with `#[tokio::main]`, move body out of `block_on` closure.

## Tasks

- [ ] Replace manual runtime with `#[tokio::main]`
