# Wait for boot completion instead of showing full serial log

**ID:** a0168603 | **Status:** Done | **Created:** 2026-02-16T23:32:26+01:00

After `rum up`, the CLI currently attaches `virsh console` and streams the full serial log. Instead, the CLI should show a progress/spinner and wait for boot to complete, then drop into a shell or report success.

- Detect when provisioning services (rum-system, rum-boot) have finished — e.g. watch for a sentinel in serial output or poll via libvirt guest agent
- Show a spinner/progress indicator during boot
- Related: 05a5d038 (keyboard shortcuts during boot wait)
