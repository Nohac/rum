# System provisioning runs on every boot instead of first boot only

**ID:** 0dca733d | **Status:** Done | **Created:** 2026-02-23T23:31:53+01:00

`[provision.system]` scripts run on every `rum up`, not just the first boot. The `just_started` guard in `libvirt.rs` should prevent this, but the system provision `run_on: RunOn::System` is meant to run only on first boot.

Likely cause: `just_started` is true whenever `dom.create()` is called (VM was stopped), which happens on every `rum up` after `rum down`. Need a separate "first provisioned" marker — e.g. a file in the work dir that records whether system provisioning has already completed.
