# Ctrl+C during first boot should destroy VM state

**ID:** 5739a803 | **Status:** Done | **Created:** 2026-02-21T16:17:56+01:00

If Ctrl+C fires during first boot (while cloud-init is running — deploying agent, writing files, running provisioning scripts), the VM is shut down but its state is preserved. On next `rum up`, cloud-init won't re-run because it already marked itself as done, leaving the VM in a half-provisioned state.

When Ctrl+C happens during first boot (i.e. `just_started` is true and provisioning hasn't completed), the shutdown path should destroy the VM and wipe its artifacts (overlay, seed ISO) so the next `rum up` starts fresh. This is equivalent to `rum destroy` + `rum up`.

For subsequent boots (`just_started` is false), the current behavior (just shut down) is correct.
