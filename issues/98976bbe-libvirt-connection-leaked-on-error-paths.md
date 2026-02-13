# Libvirt connection leaked on error paths

**ID:** 98976bbe | **Status:** Done | **Created:** 2026-02-13T21:18:40+01:00

## Summary

All backend methods call `conn.close().ok()` on the happy path, but early error returns (e.g. `RequiresRestart`, undefine failure) skip the close. The `virt` crate's `Connect` may not close on drop, leaking the connection.

## Approach

Create a small RAII wrapper (`struct ConnGuard(Connect)`) that calls `close()` on `Drop`. Use it in all backend methods.

## Tasks

- [ ] Add `ConnGuard` with `Drop` impl that closes the connection
- [ ] Use it in all backend methods
