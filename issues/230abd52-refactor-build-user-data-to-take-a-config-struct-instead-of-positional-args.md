# Refactor build_user_data to take a config struct instead of positional args

**ID:** 230abd52 | **Status:** Open | **Created:** 2026-02-20T20:04:36+01:00

`build_user_data()`, `seed_hash()`, and `generate_seed_iso()` in `src/cloudinit.rs` take 7-9 positional arguments. This makes call sites fragile and hard to read — every new feature (e.g. `has_agent`) adds another bool to the end.

**Approach:** Create a `SeedConfig` struct with `#[derive(Default)]` containing all the inputs. Callers construct it with struct update syntax (`SeedConfig { hostname, ..Default::default() }`). Also simplifies the `#[allow(clippy::too_many_arguments)]` suppressions.

**Affected functions:**
- `seed_hash()`
- `generate_seed_iso()`
- `build_user_data()` (internal)
