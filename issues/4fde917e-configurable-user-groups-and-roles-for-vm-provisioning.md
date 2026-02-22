# Configurable user groups and roles for VM provisioning

**ID:** 4fde917e | **Status:** Open | **Created:** 2026-02-22T14:11:34+01:00

## Summary

Allow users to configure additional groups for the default `rum` user in `rum.toml`, so they can get passwordless access to Docker, KVM, etc. without manual `usermod` in provisioning scripts.

## Problem

Currently the `rum` user is created with only default groups. If you install Docker, you need to manually add the user to the `docker` group via a provisioning script (`usermod -aG docker rum`), and it only takes effect after re-login. This is a common pain point — most cloud VM users need the default user in `docker`, `kvm`, `video`, or other groups.

## Approach

1. **`src/config.rs`** — Add a `[user]` section to `Config`:
   ```toml
   [user]
   name = "rum"                     # default: "rum"
   groups = ["docker", "video"]     # additional groups (created if missing)
   ```
   Fields: `name` (String, default "rum"), `groups` (Vec<String>, default empty).

2. **`src/cloudinit.rs`** — In the cloud-config user-data, pass the configured groups:
   - Set `name` from config
   - Add `groups: docker,video,...` to the user block
   - Use `system_info.default_user.groups` or the `users` list depending on cloud-init conventions

3. **`src/init.rs`** — Optionally add a wizard step for groups (or just document it — wizard may be overkill for this).

## Notes

- Groups that don't exist on the guest should be auto-created (cloud-init handles this with the `groups` directive at the top level)
- Keep backward compatible — no `[user]` section means current behavior (user "rum", no extra groups)
- The `name` field also lets power users change the default username if they want
