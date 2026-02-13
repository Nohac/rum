# rum up exits instead of showing auto-login shell

**ID:** 017fa41d | **Status:** Done | **Created:** 2026-02-13T22:05:59+01:00

## Summary

After `rum up` completes, it exits immediately instead of showing an auto-login shell. The autologin cloud-init drop-in was added but only takes effect on fresh VMs. Existing VMs with a previously-generated seed ISO don't get the new autologin config because `rum up` skips seed generation when the ISO already exists (`if !seed_path.exists()`).

## Approach

This may be a "works on fresh VM" issue — need to verify with `rum up --reset`. If so, this is expected behavior (seed ISO is only generated once). Document this or detect stale seed ISOs.
