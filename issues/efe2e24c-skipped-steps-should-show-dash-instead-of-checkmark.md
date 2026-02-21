# Skipped steps should show dash instead of checkmark

**ID:** efe2e24c | **Status:** Done | **Created:** 2026-02-21T13:54:19+01:00

Steps that don't need to run (cached, skipped, already done) currently show a green `✓` checkmark, same as steps that actually executed. This makes it hard to tell what actually happened.

Change `StepProgress::skip()` to show a dash `—` instead of `✓`:

```
[3/8] — Cloud-init seed ready
[7/8] — Running system provisioning (skipped)
```

**File:** `src/progress.rs` — update `skip()` method to use `—` (em dash) instead of `✓`, and blue instead of green.
