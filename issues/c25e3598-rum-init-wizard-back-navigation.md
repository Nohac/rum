# rum init wizard back navigation

**ID:** c25e3598 | **Status:** Done | **Created:** 2026-02-17T23:25:32+01:00

Allow users to go back to a previous step during `rum init` to change their answer. The `inquire` crate doesn't support this natively — each prompt is independent with no multi-step navigation. Needs a custom state machine loop around the wizard steps (prompt → store → allow back on request → re-prompt).
