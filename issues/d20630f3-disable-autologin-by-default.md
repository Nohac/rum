# Disable autologin by default

**ID:** d20630f3 | **Status:** Open | **Created:** 2026-02-16T23:32:25+01:00

Currently the autologin dropin is always written, causing the `rum` user to be logged in automatically on the serial console. This should be opt-in rather than the default.

- Add `autologin = true/false` to the config (default: `false`)
- Only write the autologin dropin to write_files when enabled
- The After= ordering on rum-system/rum-boot services should still apply when autologin is enabled
