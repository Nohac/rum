# Boot scripts skipped on subsequent boots

**ID:** 1fe02889 | **Status:** Done | **Created:** 2026-02-27T17:26:26+01:00

## Summary

`provision.boot` scripts should run on every boot (first boot AND subsequent boots after
`rum down` / `rum up`). Currently the new flow system passes a flat `scripts: Vec<String>`
to `select_flow()`, but has no mechanism to distinguish system scripts from boot scripts.
This means either all scripts run on reboot (wrong) or boot scripts are silently skipped.

## Bug details

The old code in `backend/libvirt.rs` correctly handled this with:

```rust
// Old code — filters system scripts on subsequent boots
let scripts_to_run: Vec<_> = if previously_provisioned {
    provision_scripts.into_iter()
        .filter(|s| !matches!(s.run_on, rum_agent::RunOn::System))
        .collect()
} else {
    provision_scripts
};
```

The new flow system in `flow/mod.rs` lost this distinction:

```rust
pub fn select_flow(command: &FlowCommand, state: &VmState, scripts: Vec<String>) -> ... {
    FlowCommand::Up => match state {
        Virgin | ImageCached | Prepared | PartialBoot => FirstBootFlow::new(scripts),  // all scripts
        Provisioned => RebootFlow::new(scripts),  // same scripts param — caller must filter
    }
}
```

The problem: `select_flow` receives a single flat `scripts` list. The caller would need to
pre-filter boot-only scripts before calling `select_flow` for the Provisioned case, but:
- The caller doesn't know which flow will be selected (that's `select_flow`'s job)
- The flow structs (`FirstBootFlow.scripts`, `RebootFlow.boot_scripts`) are both `Vec<String>`
  with no system/boot distinction

## Expected behavior

- **First boot** (`rum up` from Virgin): run `provision.system` then `provision.boot`
- **Subsequent boot** (`rum up` from Provisioned): run only `provision.boot`
- **Manual provision** (`rum provision`): run scripts based on `--system`/`--boot` flags

## Approach

Change `select_flow` to accept the full `ProvisionConfig` (or separate system/boot script
lists) instead of a flat `Vec<String>`:

```rust
pub fn select_flow(
    command: &FlowCommand,
    state: &VmState,
    system_scripts: Vec<String>,
    boot_scripts: Vec<String>,
) -> Result<Box<dyn Flow>, RumError> {
    FlowCommand::Up => match state {
        Virgin | ... => FirstBootFlow::new(system_scripts, boot_scripts), // runs both
        Provisioned => RebootFlow::new(boot_scripts),                     // boot only
    }
}
```

Then `FirstBootFlow` sequences system scripts first, then boot scripts.

## Files

- `src/flow/mod.rs` — `select_flow()` signature and `FlowCommand`
- `src/flow/first_boot.rs` — `FirstBootFlow` needs separate system/boot script fields
- `src/flow/reboot.rs` — `RebootFlow` already has `boot_scripts` (correct)
- `src/flow/reprovision.rs` — may need system/boot split for `--system`/`--boot` flags
- Caller of `select_flow` (currently not wired — will be in daemon.rs event loop integration)
