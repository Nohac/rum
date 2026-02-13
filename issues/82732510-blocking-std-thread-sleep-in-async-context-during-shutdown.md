# Blocking std thread sleep in async context during shutdown

**ID:** 82732510 | **Status:** Done | **Created:** 2026-02-13T21:18:40+01:00

## Summary

`backend/libvirt.rs:310-315` — `shutdown_domain` uses `std::thread::sleep` in a polling loop (up to 10s) while called from async context. This blocks the entire Tokio runtime thread.

## Approach

Replace `std::thread::sleep` with `tokio::time::sleep` and make `shutdown_domain` async.

## Tasks

- [ ] Make `shutdown_domain` async, use `tokio::time::sleep`
