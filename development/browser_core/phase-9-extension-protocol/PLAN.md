# Phase 9 — BlueIce Extension Protocol

[← Back to plan](../BROWSER_CORE_PLAN.md)

**Status**: In progress (capability list, manifest model, and implementation format decided; wire protocol and versioning still open)

## Objective

Turn the process-isolated `extension` architecture (already settled: separate OS process, IPC boundary, no in-process sandboxing — plan §1) into an actual documented, versioned protocol third parties can build against: a manifest format, a permission model, and a defined API surface. Plan §4 already excludes the extension *ecosystem* from MVP scope — this phase is that ecosystem, once the MVP core exists for it to extend.

## Design sketch

**Capability list, refined against real prior art by [`research/extension-architecture.md`](../research/extension-architecture.md)** (which studied Chromium's Manifest V3 and Firefox's WebExtensions capability catalogs directly):

- `dom:read` — query the Phase 1 AI-facing representation, read-only
- `dom:write` — mutate specific nodes (content-script-style injection)
- `network:observe` — read request/response metadata
- `network:intercept` — modify or block requests (research note: Chromium's own move away from blocking `webRequest` toward `declarativeNetRequest` — the browser evaluating declarative rules itself rather than an extension parsing untrusted network data in a privileged process — is worth weighing for this capability specifically, not just copying the older blocking-callback model)
- `ui:inject` — ask `frontend` to render extension-provided UI (toolbar button, popup)
- `storage` — a scoped key-value store, one per extension

**Manifest model — a three-tier structure, borrowed from both reference engines' converged design**: `declared` (granted at install, matches Chromium/Firefox's persistent host-permission model), `optional` (available but not granted until requested, matches both engines' `optional_permissions`), and `runtime-ephemeral` (gesture-triggered, non-persistent — the research found Chromium's `activeTab` is exactly this shape, worth a BlueIce equivalent rather than only the binary declared/optional split). Manifest file: a declarative file (`extension.toml` or `.json`) naming `name`, `version`, `blueice_api_version` (for the protocol-versioning/compatibility story), the three-tier `capabilities` structure above, and an `entry_point` (path to the extension's WASM module, per the implementation-format decision below).

**Wire protocol shape**: since `extension` already runs in its own OS process (settled, plan §1), the natural transport is a length-prefixed framed protocol over a Unix domain socket / named pipe, with messages defined via a schema (tying back to `backend/ipc`'s still-open format decision — protobuf/Cap'n Proto are the leading candidates there too). Capability checks are enforced by `core` against what that extension's manifest declared at install time — checked on `core`'s side of the boundary, not trusted from the extension process itself, since not trusting that process is the entire point of the isolation. This is also where Phase 7's `GatekeeperClearance` capability-token pattern needs its server-side counterpart: `core`'s handler for each extension-initiated request must itself reject an unauthorized capability use, not rely solely on the extension process behaving.

**Extension implementation format — decided: WASM.** `research/extension-architecture.md` found both Chromium and Firefox independently carve `'wasm-unsafe-eval'` out of their otherwise-tightened MV3 Content-Security-Policy defaults, while continuing to restrict plain JS `eval` — and neither gives extension code *any* path to native code except as a separate, explicitly-consented native-messaging peer process. Two independent production extension systems converging on the same WASM-over-native stance is a strong, source-grounded signal, not just WASM's general reputation for sandboxability. Native dylibs are dropped from consideration.

## Open questions (blocking real design)

- ~~What can an extension actually do?~~ — **draft resolved above**, refined against Chromium/Firefox's real capability catalogs; still needs review for BlueIce-specific gaps neither reference engine has (e.g. anything touching the Phase 1 AI-facing representation specifically).
- ~~Manifest/permission model~~ — **resolved to a three-tier model** (declared/optional/runtime-ephemeral) above, borrowed from both reference engines' converged design.
- ~~Extension implementation format~~ — **resolved: WASM**, see design sketch above.
- **Versioning**: how the protocol itself evolves without breaking installed extensions — semver on the protocol, capability negotiation at connect time, or something else. Still open; the research didn't cover this specifically.
- **Server-side capability enforcement**: per the enforcement-mechanism note above, `core`'s request handlers need their own capability checks, not just Phase 7's client-side `GatekeeperClearance` token — needs designing together with Phase 7.

## Checklist

- [x] Define the extension capability list — draft above, refined against `research/extension-architecture.md`
- [x] Design the manifest/permission model — three-tier (declared/optional/runtime-ephemeral)
- [x] Decide the extension implementation format — WASM
- [ ] Design the protocol versioning/compatibility story
- [ ] Specify the actual wire protocol in the `ipc` crate (`backend/ipc`), replacing its current placeholder, including server-side capability-check enforcement (with Phase 7)
- [ ] Build a minimal reference extension against the spec to validate it end to end
