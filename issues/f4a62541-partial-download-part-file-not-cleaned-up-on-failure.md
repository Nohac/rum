# Partial download part file not cleaned up on failure

**ID:** f4a62541 | **Status:** Done | **Created:** 2026-02-13T21:18:40+01:00

## Summary

`image.rs` downloads to a `.part` temp file then renames on success. If the download fails mid-stream, the `.part` file is left behind. Repeated failures accumulate stale partial downloads.

## Approach

Delete the `.part` file in the error path. Optionally check for and remove stale `.part` files at download start.

## Tasks

- [ ] Clean up `.part` file on download error
