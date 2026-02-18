# Support default/workdir mount for automatic working directory

**ID:** 40f35133 | **Status:** Open | **Created:** 2026-02-14T16:05:47+01:00

When a mount is marked as the default/workdir, the user's shell should automatically `cd` into that directory on login (via serial console or SSH).

## Config syntax

```toml
[[mounts]]
source = "."
target = "/mnt/project"
default = true           # sets this as the login working directory
```

Only one mount may have `default = true`. Validation should reject multiple defaults.

## Approach

1. Add `default: bool` field to `MountConfig` (default: `false`)
2. Validate at most one mount has `default = true`
3. In cloud-init user-data, if a default mount exists:
   - Set the `rum` user's home dir or add a `.bashrc`/`.profile` snippet that does `cd /mnt/project`
     (this could cause a lot of extra files to be written to the mounted dir, should not be preferred)
   - Alternatively, use `write_files` to drop a profile.d script: `/etc/profile.d/rum-workdir.sh` containing `cd /mnt/project` (preferred solution)
4. The profile.d approach is simplest and works for both serial console and SSH sessions
