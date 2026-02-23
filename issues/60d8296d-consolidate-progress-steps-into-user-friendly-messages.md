# Consolidate progress steps into user-friendly messages

**ID:** 60d8296d | **Status:** Open | **Created:** 2026-02-23T23:31:52+01:00

Current `rum up` output has too many internal-detail steps that aren't useful to the user:

```
[2/10] — Disk overlay ready
[3/10] — Cloud-init seed ready
[4/10] ✓ Configuring domain...
[5/10] ✓ Starting VM...
[6/10] ✓ Waiting for agent...
```

Proposed changes:
- Combine overlay/seed/domain config into a single step or skip silently when cached
- Consolidate "Starting VM" + "Waiting for agent" into something like "Waiting for VM to be ready"
- Focus on user-meaningful milestones: downloading image, preparing VM, booting, provisioning, ready
