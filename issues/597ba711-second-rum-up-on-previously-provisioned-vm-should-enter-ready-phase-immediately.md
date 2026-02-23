# Second rum up on previously-provisioned VM should enter Ready phase immediately

**ID:** 597ba711 | **Status:** Open | **Created:** 2026-02-24T00:14:14+01:00

Related: #0dca733d (provision.system should only run on first boot)

## Problem

After the `UpPhase` state machine refactor (#307566fa), a fresh `rum up` on a VM that was successfully created with `dom.create()` enters `NotReady` phase — even if the VM was previously fully provisioned and is being restarted. This means Ctrl+C during the agent-wait or provisioning steps will destroy+wipe a VM that was perfectly fine before.

The `NotReady` → destroy+wipe behavior should only apply to a truly first boot (overlay and seed just created, never provisioned). A VM that has been provisioned before and is being restarted should go straight to `Ready` after `dom.create()`, so Ctrl+C triggers ACPI shutdown instead of destroy.

## Current behavior

```
rum up         # first time: Preparing → NotReady → Ready (correct)
rum down
rum up         # second time: Preparing → NotReady → Ready (wrong — should skip NotReady)
```

On the second `rum up`, if Ctrl+C is pressed during agent wait, the VM is destroyed and artifacts wiped — losing a previously-provisioned VM.

## Expected behavior

The phase should distinguish "truly first boot" from "restarting a previously-provisioned VM." If the overlay already existed before `rum up` started (i.e., this is not the first boot), `dom.create()` should transition to `Ready` directly.

## Approach

Track whether the overlay existed at the start of `up()`. If it did, the VM has been provisioned before — use `Ready` after `dom.create()` instead of `NotReady`. This could be as simple as checking `overlay_path.exists()` before any reset/creation logic runs, and using that to decide the phase after step 5.
