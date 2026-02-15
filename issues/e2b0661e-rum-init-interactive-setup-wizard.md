# rum init interactive setup wizard

**ID:** e2b0661e | **Status:** Open | **Created:** 2026-02-15T14:29:22+01:00

`rum init` — an interactive setup wizard that generates a `rum.toml` in the current directory, getting users up and running quickly.

## Wizard steps

1. **Backend detection** — detect available hypervisors (libvirt/KVM, VirtualBox, etc.) and default to what's found. Let user confirm or override.
2. **OS selection** — list commonly used cloud images (Ubuntu LTS, Fedora, Debian, Arch, Alpine, etc.) with pre-filled image URLs. Include an option to open a link for finding more images (e.g. cloud-images.ubuntu.com).
3. **Resources** — ask for CPUs and memory (sensible defaults like 2 CPUs, 2 GB RAM).
4. **Workspace mounts** — ask if the user wants to mount the current directory into the VM (default yes for dev workflows). Allow adding additional mounts.
5. **Port forwarding** — ask which ports to forward (common presets: SSH/22, HTTP/8080, etc.) or skip.
6. **Extra drives** — ask if additional disks are needed (size, mount point).
7. **Write `rum.toml`** — generate the config file with all selections, including helpful comments.

## Notes

- Should work non-interactively too (e.g. `rum init --defaults` for zero-prompt setup)
- Port forwarding and extra drives are not yet implemented in rum — this issue covers the wizard UX; the underlying features are separate work
- Consider using `dialoguer` or `inquire` crate for the interactive prompts
