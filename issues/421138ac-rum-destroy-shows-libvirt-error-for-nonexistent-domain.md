# rum destroy shows libvirt error for nonexistent domain

**ID:** 421138ac | **Status:** Open | **Created:** 2026-02-13T22:05:59+01:00

## Summary

Running `rum destroy` on a nonexistent VM prints a libvirt error to stderr before the success message:

```
libvirt: QEMU Driver error : Domain not found: no domain with matching name 'test-vm'
VM 'test-vm' destroyed.
```

The Rust code handles the `Err` from `Domain::lookup_by_name` silently (`if let Ok(dom)`), but the libvirt C library's default error handler prints to stderr before the Rust binding returns.

## Approach

Suppress libvirt's default stderr error handler. The `virt` crate may expose `Connect::set_error_func` or similar. Alternatively, register a no-op error callback after opening the connection.
