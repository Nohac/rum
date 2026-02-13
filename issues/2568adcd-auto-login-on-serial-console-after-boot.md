# Auto-login on serial console after boot

**ID:** 2568adcd | **Status:** Open | **Created:** 2026-02-13T21:14:04+01:00

## Summary

After `rum up` boots the VM, the user hits a login prompt and must manually type `rum`/`rum`. Should auto-login so users land directly in a shell.

## Approach

Configure auto-login on `ttyS0` via cloud-init — add a systemd drop-in override for `serial-getty@ttyS0.service` with `agetty --autologin rum`.

## Tasks

- [ ] Add cloud-init config to enable autologin on ttyS0
- [ ] Test that `rum up` drops directly into a shell after boot
