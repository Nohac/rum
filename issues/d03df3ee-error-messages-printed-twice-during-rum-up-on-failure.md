# Error messages printed twice during rum up on failure

**ID:** d03df3ee | **Status:** Open | **Created:** 2026-02-24T20:41:11+01:00

When `rum up` fails (e.g. "base image not found"), the error message appears twice in the output. Likely caused by both tracing and miette printing the same error.
