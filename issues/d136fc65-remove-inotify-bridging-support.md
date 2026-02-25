# Remove inotify bridging support

**ID:** d136fc65 | **Status:** Done | **Created:** 2026-02-24T20:56:24+01:00

Remove the inotify bridging feature (`inotify = true` on `[[mounts]]`). With `rum cp` planned for file transfer and virtiofs mounts already handling shared directories, the SSH-based inotify bridge adds complexity (SSH key management, russh dependency, polling) for limited value.

## Scope

- Remove `src/watch.rs` module entirely
- Remove `inotify` field from mount config in `src/config.rs`
- Remove inotify bridge startup from `src/backend/libvirt.rs` (the `start_inotify_bridge` call)
- Remove `notify` and `russh` crate dependencies if they become unused
- Update `src/skill.rs`, `CLAUDE.md`, and any test fixtures that reference inotify
