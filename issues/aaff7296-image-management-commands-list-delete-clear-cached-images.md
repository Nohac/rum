# Image management commands: list, delete, clear cached images

**ID:** aaff7296 | **Status:** Done | **Created:** 2026-02-21T14:06:27+01:00

Base images are cached under `~/.cache/rum/images/` but there's no way to manage them other than manually poking around in that directory.

## Proposed CLI

```
rum image list          # list cached images with size and last-used date
rum image delete <name> # delete a specific cached image by filename
rum image clear         # delete all cached images
```

## Details

- `rum image list` — scan `~/.cache/rum/images/`, show filename, file size (human-readable), and mtime. Show total disk usage at the bottom.
- `rum image delete <name>` — delete a single image by filename. Error if not found.
- `rum image clear` — delete all files in the cache directory. Confirm with count and total size before deleting.

## Files

- `src/cli.rs` — add `image` subcommand with `list`, `delete`, `clear` sub-subcommands
- `src/main.rs` — wire up the new commands
- `src/image.rs` — add list/delete/clear functions operating on the cache directory
- `src/paths.rs` — `cache_dir()` already exists, should be sufficient
