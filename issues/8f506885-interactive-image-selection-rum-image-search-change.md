# Interactive image selection: rum image search/change

**ID:** 8f506885 | **Status:** Done | **Created:** 2026-02-21T14:10:34+01:00

**Depends on:** aaff7296 (image management commands)

Allow users to search for and switch to a different cloud image interactively, updating `rum.toml` automatically.

## Proposed CLI

```
rum image search <query>    # search available cloud images
rum image change             # interactive: search + select + update rum.toml
```

## Details

- **`rum image search <query>`** — search known cloud image sources (Ubuntu, Debian, Fedora, Arch, etc.) by distro name, version, or keyword. Show results with distro, version, arch, and URL.
- **`rum image change`** — interactive workflow:
  1. Prompt for search query (or show popular choices)
  2. Display matching images in a selectable list
  3. User picks one
  4. Update `[image] base = "..."` in `rum.toml`
  5. Optionally warn that `rum up --reset` is needed for the change to take effect

## Open questions

- Where to source image lists from? Could hardcode known URL patterns for major distros, or fetch a manifest/index. Hardcoded patterns are simpler and don't require network for the search step.
- Should `rum init` also offer this interactive selection instead of defaulting to Ubuntu Noble?
