# Handle Ctrl-C gracefully during rum up

**ID:** d63b8a3b | **Status:** Done | **Created:** 2026-02-13T21:14:04+01:00

## Summary

After `rum up` attaches `virsh console`, Ctrl+C is passed through to the guest. There's no way to cleanly exit — the only option is `rum destroy` from another terminal. `Ctrl+]` detaches but leaves the VM running with no feedback.

## Approach

Spawn `virsh console` as a killable child process. Set up a Ctrl+C handler that kills the console process and performs graceful ACPI shutdown (reusing `down` logic). Must handle terminal raw mode cleanup.

## Tasks

- [ ] Spawn `virsh console` with a killable handle (not `.status().await`)
- [ ] Set up Ctrl+C handler to kill console + shut down VM
- [ ] Ensure terminal state is restored on exit
