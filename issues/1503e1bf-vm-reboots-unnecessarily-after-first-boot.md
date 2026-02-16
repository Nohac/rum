# VM reboots unnecessarily after first boot

**ID:** 1503e1bf | **Status:** Done | **Created:** 2026-02-15T20:47:30+01:00

On first boot, the VM completes initial setup and logs in via serial console correctly, but then appears to reboot/restart and logs in a second time. This is unnecessary and confusing.

## Likely causes

- **cloud-init reboot module** — cloud-init may trigger a reboot after applying certain config (e.g. package installs, kernel updates). Check if `power_state` or a reboot directive is being set.
- **`systemctl restart serial-getty@ttyS0`** in runcmd — we restart the serial getty for autologin, which disconnects and reconnects the console. This may look like a reboot even if the VM didn't actually restart.
- **cloud-init runs twice** — some cloud images run cloud-init in multiple stages (per-boot vs per-instance), which could re-trigger the getty restart.

## Investigation

1. Check `rum up` serial output carefully — is it an actual reboot (kernel messages again) or just a getty restart?
2. If getty restart: consider moving the autologin dropin to `write_files` only and letting the getty pick it up on next natural restart, or use `bootcmd` instead of `runcmd` to apply it earlier.
3. If actual reboot: check cloud-init logs inside the VM (`/var/log/cloud-init.log`) for reboot triggers.
