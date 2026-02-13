# Provision script lines not properly YAML-escaped in cloud-init

**ID:** 5316c01f | **Status:** Done | **Created:** 2026-02-13T21:18:40+01:00

## Summary

`cloudinit.rs:60-63` inserts provision script lines and package names directly into cloud-config YAML without escaping. Lines containing colons, quotes, or brackets produce malformed YAML that cloud-init silently misparses or ignores.

## Approach

Properly quote `runcmd` entries. Simplest fix: wrap each line as `- ["bash", "-c", "<escaped-line>"]` instead of bare `- <line>`. Package names should also be quoted.

## Tasks

- [ ] YAML-escape or properly quote runcmd entries in `build_user_data`
- [ ] Quote package names
- [ ] Add a test with a script containing YAML-special characters
