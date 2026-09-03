# Phase 9 — BlueIce Extension Protocol

[← Back to plan](../BROWSER_CORE_PLAN.md)

**Status**: Not started

## Objective

Turn the process-isolated `extension` architecture (already settled: separate OS process, IPC boundary, no in-process sandboxing — plan §1) into an actual documented, versioned protocol third parties can build against: a manifest format, a permission model, and a defined API surface. Plan §4 already excludes the extension *ecosystem* from MVP scope — this phase is that ecosystem, once the MVP core exists for it to extend.

## Design sketch

**Capability list — a concrete starting draft**, to give the manifest/permission model something real to reference rather than staying abstract:

- `dom:read` — query the Phase 1 AI-facing representation, read-only
- `dom:write` — mutate specific nodes (content-script-style injection)
- `network:observe` — read request/response metadata
- `network:intercept` — modify or block requests
- `ui:inject` — ask `frontend` to render extension-provided UI (toolbar button, popup)
- `storage` — a scoped key-value store, one per extension

**Manifest sketch**: a declarative file (`extension.toml` or `.json`) naming `name`, `version`, `blueice_api_version` (for the protocol-versioning/compatibility story), a `capabilities` list drawn from the set above, and an `entry_point` (path to the extension's WASM module or native binary, per whichever implementation-format decision below).

**Wire protocol shape**: since `extension` already runs in its own OS process (settled, plan §1), the natural transport is a length-prefixed framed protocol over a Unix domain socket / named pipe, with messages defined via a schema (tying back to `backend/ipc`'s still-open format decision — protobuf/Cap'n Proto are the leading candidates there too). Capability checks are enforced by `core` against what that extension's manifest declared at install time — checked on `core`'s side of the boundary, not trusted from the extension process itself, since not trusting that process is the entire point of the isolation.

## Open questions (blocking real design)

- **What can an extension actually do?** A concrete capability list is needed before a manifest/permission model means anything: DOM access (read-only vs. mutating, and against which representation — raw DOM or the Phase 1 AI-facing representation), network request interception, UI injection via `frontend`, background/persistent behavior vs. request-scoped only.
- **Manifest/permission model**: declared capabilities upfront at install time (Chrome/Firefox `manifest.json`-style) vs. runtime capability grants (prompt-on-use) vs. both.
- **Extension implementation format**: OS-process isolation (plan §1) protects `core` regardless of what runs inside an extension process, but that's a separate question from what language/format an extension is authored in — native Rust dylib (fastest, but couples an extension to `core`'s ABI across versions, worse compatibility story) vs. WASM (portable, independently sandboxable as defense-in-depth on top of process isolation, more restricted capability surface). Not decided; process isolation being settled doesn't settle this.
- **Versioning**: how the protocol itself evolves without breaking installed extensions — semver on the protocol, capability negotiation at connect time, or something else.

## Checklist

- [ ] Define the extension capability list (blocks the manifest/permission model)
- [ ] Design the manifest/permission model
- [ ] Decide the extension implementation format (native dylib vs. WASM vs. both)
- [ ] Design the protocol versioning/compatibility story
- [ ] Specify the actual wire protocol in the `ipc` crate (`backend/ipc`), replacing its current placeholder
- [ ] Build a minimal reference extension against the spec to validate it end to end
