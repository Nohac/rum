# Base image download shows as skipped instead of checkmarked

**ID:** 3969de54 | **Status:** Open | **Created:** 2026-02-21T16:17:56+01:00

When a base image is downloaded for the first time, `image::ensure_base_image` shows its own progress bar. But the step progress shows it as skipped ("— Base image downloaded" in blue) instead of completed ("✓ Base image downloaded" in green).

The download step should use `progress.run()` so it shows a proper spinner/checkmark. The image download's own progress bar (reqwest streaming + indicatif) needs to integrate with or replace the step spinner.
