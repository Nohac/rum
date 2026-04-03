# JSON observer should use roam serialization instead of manual format strings

**ID:** ff92be50 | **Status:** Open | **Created:** 2026-02-27T17:36:04+01:00

## Summary

`JsonObserver` currently emits JSON via hand-rolled `format!` strings with manual escaping.
This should use `facet_json` serialization with proper typed structs, or at minimum
`facet_value` for ad-hoc construction. Ideally dedicated output structs so the JSON schema
is explicit and stable.

## Current problem

`observer/json.rs` builds JSON with raw format strings:

```rust
fn json_transition(t: &Transition) -> String {
    format!(
        r#"{{"type":"transition","from":"{:?}","to":"{:?}","event":"{:?}"}}"#,
        t.old_state, t.new_state, t.event,
    )
}
```

Issues:
- Uses `Debug` formatting for enum variants (e.g. `ImageReady("/path")`) — brittle, not
  a stable serialization format
- Manual `replace('\\', "\\\\").replace('"', "\\\"")` escaping misses control characters
  (newlines, tabs, null bytes) that JSON requires escaping
- No schema — consumers must guess the JSON structure from reading source code
- `facet_json` is already a dependency but unused here

## Approach

Define dedicated output structs with `#[derive(Facet)]` and serialize via `facet_json::to_string()`.

```rust
use facet::Facet;

#[derive(Facet)]
#[repr(u8)]
enum JsonOutput {
    Transition(JsonTransition),
    Effect(JsonEffect),
}

#[derive(Facet)]
struct JsonTransition {
    from: String,    // VmState name
    to: String,      // VmState name
    event: String,   // Event name
}

#[derive(Facet)]
#[repr(u8)]
enum JsonEffect {
    Log { stream: String, data: String },
    Progress { stream: String, current: u64, total: u64 },
    Info { stream: String, data: String },
}
```

Then:
```rust
fn on_transition(&mut self, t: &Transition) -> ... {
    let output = JsonOutput::Transition(JsonTransition { ... });
    println!("{}", facet_json::to_string(&output));
}
```

## Prerequisites

`VmState` and `Event` don't currently derive `Facet`. Two options:
1. Add `Facet` derive to `VmState` and `Event` (preferred — they'll need it for roam
   transport anyway) and serialize them directly
2. Convert to strings in the output structs (simpler, but loses type info)

Both `VmState` (in `vm_state.rs`) and `Event` (in `flow/mod.rs`) need `#[repr(..)]` for
facet enum support.

## Files

- `src/observer/json.rs` — replace `json_transition()` and `json_effect()` with facet structs
- `src/vm_state.rs` — add `#[derive(Facet)]` + `#[repr(u8)]` to `VmState`
- `src/flow/mod.rs` — add `#[derive(Facet)]` + `#[repr(u8)]` to `Event` (and possibly `Effect`)
- `src/observer/mod.rs` — `Transition` may also benefit from `Facet` derive
