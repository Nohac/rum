# Add rum provision command to re-run provisioning scripts on a running VM

**ID:** 403b7fe3 | **Status:** Done | **Created:** 2026-02-24T00:45:37+01:00

## Summary

Add a `rum provision` command that re-runs provisioning scripts on a running VM via the vsock agent. Currently, re-running provisioning requires `rum destroy && rum up` or `rum up --reset`, which reboots the entire VM. `rum provision` would re-read `rum.toml` from disk and execute the scripts directly — no reboot, no snapshot, no overlay wipe.

## Usage

```
rum provision             # re-run all provisioning scripts (system + boot)
rum provision --system    # re-run only [provision.system] script
rum provision --boot      # re-run only [provision.boot] script
```

## Behavior

1. Re-reads `rum.toml` from disk (picks up script edits)
2. Connects to the vsock agent on the running VM
3. Runs the selected provisioning script(s) via agent exec
4. Streams output to stdout/stderr, returns exit code

No rollback, no snapshot — just runs the script. The user is responsible for idempotency of their scripts. This is the simple, low-ceremony option for iterating on provisioning.

## Related

- **0dca733d** — Bug: system provisioning runs on every boot (first-boot-only semantics)
- **595c06e8** — `rum retry` with snapshot rollback (heavier, depends on snapshot support)
