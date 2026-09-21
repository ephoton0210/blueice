# Phase 18 — BlueTS / BlueTSC TODO

[← Phase 18 plan](PLAN.md) · [BlueTS ↔ BlueJS integration contract](INTEGRATION_CONTRACT.md) · [test interface](TEST_INTERFACE.md)

This is the prioritized completion worklist for making BlueTS a usable BlueIce
page language, rather than merely a standalone compiler. `PLAN.md` remains the
source of truth for scope and completed checklist items; update it and this
file together when an item closes. A check mark requires the stated acceptance
evidence, not just an API, design document, or unit test.

## Current boundary

The standalone front end, BlueTSC emitter, VM-independent debug metadata,
bounded contract validator, incremental cache, and the host-neutral direct
BlueTS-to-BlueJS structured-program bridge are complete for `blue-ts-0.1`.
They do not yet make a TypeScript page executable or debuggable. The bridge now
has a limited, in-memory mapping from a single direct module's top-level
lowering spans to verified BlueJS safe points, but it has no page host, DOM
bindings, live contract boundary, debugger IPC, or MCP project-registration
path.

The critical path is intentionally ordered below. Do not grow the TypeScript
syntax matrix while an earlier item prevents an already-supported program from
running safely through a real page.

```text
Phase 13 page-script host + initial bindings
  -> BlueJS source IDs / safe points + Phase 17 debugger IPC
  -> generated host typings + direct-page BlueTS integration
  -> source-level debugger, strict contracts, and MCP adapter
  -> real-process regression suite and carefully gated language growth
```

## P0 — prerequisite page runtime and source locations

These items are owned jointly with the listed phases. Phase 18 must consume
their public interfaces; it must not create a private host, debugger protocol,
or second module resolver to bypass them.

- [ ] **Provide a real BlueJS page-script host (Phase 13).** Wire
  `blueice_ipc::script` through `core` to a long-lived BlueJS host, with
  document/tab/realm lifecycle, origin/policy checks, per-tab compilation and
  memory accounting, and a JavaScript `<script>` fixture that executes through
  the normal page pipeline. The host must accept caller-authorized source and
  resolver records rather than granting BlueTS filesystem, network, DOM, or
  capability authority.

  Foundation delivered: `blueice_engine::script::handle_script_request` is the
  core-owned dispatcher for the existing narrow script IPC vocabulary. It
  scopes DOM lookup/mutation to one live tab, validates raw node IDs before
  mutation, relayouts after changes, and rejects stale/cross-tab handles after
  navigation. `blueice_bluejs::BlueJsPageRuntime` now also provides a
  host-neutral, in-process realm foundation: caller-owned opaque tab IDs and
  origin identities, one VM per tab, bounded root-bytecode/program retention,
  generation-owned program handles, and fail-closed invalidation on
  navigation/reload/close. It runs an already-authorized structured classic
  script or module, but it opens no URL and installs no DOM or IPC capability.
  `blueice-core --script-socket <path>` now binds the initial long-lived core
  listener: it requires `Hello` before a request, decodes the script protocol
  on a worker, and routes each request synchronously to the session thread that
  exclusively owns the live `TabManager`. This proves real-process DOM
  dispatch without exposing a cross-thread DOM reference. The launcher-managed
  out-of-process BlueJS host, authorized source/resolver transport,
  tab-memory accounting, JavaScript DOM bindings, and normal page-pipeline
  fixture are still absent, so this prerequisite remains open.

  Acceptance: a page fixture can run a supported JavaScript classic script and
  module in its own realm; navigation/reload invalidates old program handles;
  an over-budget compilation fails without executing a partial program.

- [ ] **Expose minimal, truthful host bindings (Phases 2/13).** Define the
  first DOM/event binding surface and its policy/origin rules before declaring
  it in TypeScript. Do not expose a broad `lib.dom.d.ts` or add bindings merely
  to satisfy a checker fixture.

  Acceptance: each initial binding has an implementation, capability policy,
  stable binding ID, and JavaScript page-level behavior test; an unimplemented
  API is absent and fails both static and runtime access tests.

- [ ] **Give BlueJS generation-bound source identities and executable safe
  points (Phases 13/17).** Publish tested AST node IDs, code-unit IDs,
  instruction-safe-point enumeration, and validation APIs for the page/module
  AST surface. IDs must be validated against the exact generated program and
  never guessed from a bytecode offset.

  Foundation delivered: `bluejs-program-debug-v1` now gives a host an opaque,
  monotonically generated program handle, immutable canonical-module/source-hash
  identity, deterministic root-first executable AST/code-unit IDs,
  instruction-boundary enumeration, and fail-closed validation. The
  host-neutral direct bridge installs its structured program and checked source
  identity without a JS-text round trip, pairing each top-level lowering span
  with its verified generated AST statement. The BlueJS compiler records the
  exact root-statement instruction start (or an explicit unbound result), and
  the registry resolves that AST statement only to this verified safe point.
  Replacement, navigation-style invalidation, and malformed offsets are
  tested. It deliberately does not yet provide complete nested-expression
  provenance, a page host, or debugger pause mechanics, so this prerequisite
  remains open.

  Acceptance: a bytecode instruction can be named by `(code_unit, offset)` and
  verified by BlueJS; a program replacement invalidates its old IDs; malformed
  or stale IDs fail closed.

- [ ] **Implement the native debugger channel (Phase 17).** Add the versioned
  `blueice_ipc::debugger` path and native breakpoint, pause/resume, step,
  stack, scope, exception, and bounded-value operations. It must target one
  page realm and remain separate from script/DOM and network IPC.

  Acceptance: a JS page fixture pauses at a verified safe point, supports the
  declared stepping subset, rejects stale frame/value handles after
  navigation/resume, and does not pause another tab or render transport.

## P0 — direct BlueTS page integration

- [ ] **Generate and verify `lib.blueice.d.ts`.** Implement the
  host-adjacent `HostTypeSurfaceV1` schema and deterministic generator for
  `lib.blueice.d.ts` and `lib.blueice.manifest.json`. Sort by stable binding ID,
  normalize output, include the host API/profile/schema identities, and make
  the direct-page compiler reject unavailable profiles, schema mismatches, or
  declaration-byte mismatches.

  Foundation delivered: `blueice_engine::script::host_typings` now owns
  `HostTypeSurfaceV1`, deterministic declaration/manifest generation, a
  profile catalog, and a runtime-binding inventory derived from the same
  schema. Generated artifacts have sorted binding IDs, normalized LF
  declarations, fixed-order JSON, and schema/declaration hashes; a supplied
  manifest or source with a wrong profile, identity, schema, binding inventory,
  or declaration bytes is rejected without fallback. The generated artifact
  also validates a host's runtime registrations as an order-independent but
  exact inventory, rejecting missing, duplicate, extra, or drifted bindings.
  The checked-in `core-script-empty-v1` fixture is deliberately empty: core
  has an IPC dispatcher but no BlueJS DOM globals, so declaring `document`
  would be dishonest. `GeneratedHostTypingsV1::verify_for_direct_compiler`
  now checks the selected manifest, exact declaration bytes, and complete
  runtime registration inventory before returning the one `.d.ts` source that
  a direct compiler may place in `CompilerOptions::ambient_declaration_modules`.
  BlueTS parses that declaration under its ordinary module/source limits,
  includes its exact bytes in the compiler fingerprint and static source
  metadata, exposes its declarations only as static ambient names, and emits
  no declaration code. The standalone `bluetsc` intentionally cannot set this
  host-only option. Actual bindings, their matching BlueJS installation, and
  page-host request adoption remain required before this item can close.

  Acceptance: a checked-in fixture generates byte-identical typing artifacts;
  every declared binding can be invoked in the matching host profile; an absent
  binding is rejected by both BlueTS and the host; a profile/schema mismatch
  executes nothing.

- [ ] **Connect the direct bridge to the page loader.** Teach the page-script
  host to recognize only the opted-in BlueIce TypeScript script kinds, pass its
  authorized canonical modules and resolver fingerprint to `blueice-bluets`,
  and submit the resulting `BlueJsProgramV1` to BlueJS without an emitted-JS
  text round trip. Preserve canonical module IDs, source hashes, ordering,
  policy, language version, and all compiler-limit fingerprints.

  Foundation delivered: `blueice_bluets::AuthorizedModuleLoader` represents a
  closed host-supplied graph of canonical module records and exact
  `(from-module, specifier) -> canonical-target` records. It validates duplicate
  and dangling records before compilation, performs no filesystem/URL/import-map
  lookup, and rejects an absent edge rather than falling back to relative
  resolution. The direct BlueTS-to-BlueJS graph bridge preserves these canonical
  targets through module requests and executes the supplied graph without a
  JavaScript-text reparse. A page host still must validate the host-typing
  manifest, select an opted-in script kind, bind the request's origin/policy/
  resolver and compiler fingerprints, submit to the page realm, and invalidate
  it at lifecycle boundaries, so this item remains open.

  The first classic-script realm seam is now available as
  `DirectScript::attach_in_page_realm` (or its static-metadata variant): it
  submits the existing `BlueJsProgramV1` to the owned `BlueJsPageRuntime`,
  checks that the resulting live generation retains the artifact's exact source
  identity and bytecode, then attaches lowering provenance and verified safe
  points. A provenance or metadata failure discards the just-installed program
  and returns its bytecode charge before it can execute. `DirectModuleGraph`
  now applies the same all-or-cleaned-up admission rule to every closed runtime
  ESM module and executes only those attached canonical module IDs in the tab
  realm; navigation makes its handles unusable. `DirectPageRealmOwner` now
  combines either seam with its static metadata registry and prunes invalid
  records after its own navigation/reload or close operation. A realm reserves
  a canonical ESM ID after its first execution attempt, so no later artifact
  can reuse BlueJS's linked module cells until navigation/reload creates a
  replacement realm. A core/page-host caller must still adopt this owner (or
  preserve the same invariant), enforce host typings, and drive actual page
  lifecycle events.

  Acceptance: one typed classic script and one typed ESM module graph execute
  in a real page with no generated `.js` input; parse/resolution/type/lowering
  failures execute neither the entry nor an affected dependent module; a
  runtime import cannot be re-resolved under a different host policy.

- [ ] **Publish the TS-to-safe-point map.** Combine BlueTS lowering provenance
  with BlueJS's verified safe points into the generation-bound
  `bluejs-safe-point-map-v1` format defined by the integration contract.
  Preserve UTF-8 byte spans at this boundary and require explicit conversion
  from BlueTSC Source Map v3's UTF-16 columns.

  Foundation delivered: the direct bridge now publishes an in-memory
  `bluejs-safe-point-map-v1` for a single directly attached source module. It
  retains the program ABI/generation, compiler-options fingerprint, canonical
  source-set hash, UTF-8 lowering span, and lowering kind. Its bound entries
  are deterministically sorted, unique by instruction tuple, and revalidated
  against the live BlueJS generation; a no-output top-level statement remains
  explicitly unbound in the attachment rather than being remapped. The map is
  intentionally limited to direct top-level lowering spans. Page-realm ESM
  admission now gives every closed runtime module its own exact map and static
  metadata record, but page loader integration, host-owned multi-module
  lifetime ownership, nested-expression locations, host-request fingerprint
  checks, breakpoint search policy, and debugger IPC are still absent, so this
  item remains open.

  Acceptance: entries are deterministic, sorted, unique and validated against
  BlueJS code units; a TS breakpoint binds to the nearest permitted following
  safe point or returns an explicit unbound result; wrong generation, ABI,
  fingerprint, source set, code unit, or offset is rejected without heuristic
  remapping.

- [ ] **Attach `BlueTsDebugInfo` to page lifetime safely.** Retain static
  types, symbols, diagnostics, source hashes, provenance, and contract plans
  only for the corresponding live program generation. Enforce source/privacy
  policy and bounded retention; static types must always be distinguished from
  a BlueJS runtime value.

  Foundation delivered: direct scripts/modules now preserve their compiler
  produced `BlueTsDebugInfo`, and `DirectDebugRegistry` attaches it only after
  validating the exact live BlueJS generation, safe-point map, language
  version, compiler-options fingerprint, and canonical source hash set. It
  retains no source text or runtime values, bounds programs/sources/symbols/
  types, rejects mismatched or over-limit attachments before exposing metadata,
  and can prune records after the owning BlueJS generation is invalidated.
  `DirectModuleGraph::attach_debug_in_page_realm` now derives a module-local
  static subset (one source, that module's symbols, and their referenced type
  IDs) for every graph generation and rolls back all retained records/programs
  if any module fails the limit or identity checks. `DirectPageRealmOwner`
  prunes the registry automatically after its navigation/reload and close
  operations. The page host still must route all actual lifecycle/cache/
  hibernation events through that owner (or an equivalent invariant) and add
  source policy, diagnostics/contracts, debugger IPC, stack locations, and
  runtime-value inspection before this item can close.

  Acceptance: TS breakpoints, stack locations, scopes, symbol navigation, and
  static type display point to original source; navigation, reload, cache
  eviction, and hibernation make old metadata unavailable rather than
  misattributing it to a successor page.

## P1 — strict runtime contracts

- [ ] **Define the first live boundary inventory.** For every initially
  available ingress/egress (starting with any implemented JSON/Fetch/XHR
  surface, then URL/query, storage, `postMessage`, foreign modules, and
  extension/native IPC), record the boundary owner, source span, contract ID,
  validation limits, failure schema, and policy requirements. Do not claim a
  boundary before its underlying host API exists.

  Acceptance: `strict-runtime` rejects a supported boundary with no reifiable
  contract, unreifiable type, or unchecked `any` unless an authorized reviewed
  contract is supplied; `checked` and `transpile-only` remain visibly distinct
  policies.

- [ ] **Enforce contract plans at live boundaries.** Invoke the existing pure,
  bounded validator on data crossing each declared boundary, with no getter,
  proxy, user callback, fetch, or capability acquisition during validation.
  Attribute validator/cache/debug allocations and failures to the initiating
  tab and include the lowered behavior in the gatekeeper summary.

  Acceptance: malformed, recursive, cyclic, deep, oversized, and
  resource-exhausting values fail at a bounded path and budget; valid data
  reaches BlueJS without repeated local checks; failure diagnostics identify
  the policy and original source location without leaking protected content.

- [ ] **Preserve strict contracts in BlueTSC output.** Emit or import the
  deterministic versioned contract helper required by a `strict-runtime`
  build, record its identity in output metadata, and reject targets that would
  erase a required validation.

  Acceptance: direct-page and emitted-ESM executions reject the same malformed
  fixture at the same declared boundary; an artifact built with a weaker policy
  cannot be presented as strict-runtime output.

## P1 — native compiler and MCP integration

- [ ] **Define the native registered-project compiler service.** Expose
  check/build, diagnostics, artifacts, incremental-cache state, static type,
  symbol, contract, and provenance queries through the core-owned compiler and
  debugger capability layer. Registration must pin canonical project/config/
  output roots; a client may not extend them using a path, import map, plugin,
  or compiler option.

  Acceptance: `check` performs no writes; `build` keeps BlueTSC's atomic
  no-emit-on-error guarantee; responses are generation/fingerprint bound,
  capped, redacted where needed, and reject stale project state.

- [ ] **Implement the negotiated MCP BlueTS/BlueTSC adapter (Phase 12).** Add
  `debug_get_type`, `debug_get_symbol`, `debug_get_contract`,
  `debug_validate_contract`, `bluetsc_check`, and `bluetsc_build` as adapters
  over the native service—not shell endpoints or a second compiler. Follow the
  documented capability negotiation, session generations, authorization,
  pagination, untrusted-content handling, and explicit build-write elevation.

  Acceptance: a real MCP client can inspect a page TS diagnostic/static type/
  contract failure and run check/build for an authorized registered project;
  arbitrary paths, stale handles, oversized artifacts, unauthorized control,
  and untrusted page strings fail safely and observably.

## P1 — evidence and release gate

- [ ] **Add real-process direct-page regression coverage.** Cover classic and
  ESM TypeScript pages, compile failures, type-only import elision, resolver
  identity, source-map/provenance/safe-point mapping, breakpoints, stepping,
  exception locations, contracts, tab resource attribution, policy isolation,
  navigation/reload invalidation, and multi-tab isolation.

  Acceptance: the tests exercise `core`, IPC, BlueJS host, BlueTS, and the
  public debugger/MCP boundary as applicable; no assertion is satisfied solely
  by a host-neutral unit test.

- [x] **Make the TypeScript 5.9.3 compatibility oracle a reproducible CI
  gate.** The `typescript-oracle` CI job provisions the exact pinned compiler
  with `npm exec --package typescript@5.9.3`, passes it explicitly through
  `BLUEICE_BLUETSC_ORACLE`, and runs the ignored oracle test. The test verifies
  the compiler version before execution, then runs the supported fixture
  matrix with accepted and rejected cases checked by diagnostic
  code/count/source line. Node/BlueJS behavioral differentials remain limited
  to features that the direct bridge executes.

  Evidence: `.github/workflows/ci.yml` makes `typescript-oracle` a dependency
  of the required `ci-gate`; the job cannot pass by silently skipping the
  ignored test.

  Verified acceptance: the oracle job is selected by the normal CI triggers,
  runs the otherwise-ignored test explicitly, and is required for the final
  CI gate. Fixtures identify parser/checker/emitter/direct-lowering/runtime/
  source-location/declaration expectations in the test's case matrix.

- [ ] **Restore the relevant quality gates before declaring Phase 18 ready.**
  Run formatting, `clippy -D warnings`, focused crate tests, the applicable
  real-process suite, and the oracle job. Resolve or separately track blocking
  warnings/formatting failures in BlueJS dependencies rather than hiding them
  with an allow in BlueTS.

  Acceptance: Phase 18's required CI jobs are green with no ignored required
  oracle/integration test and no suppressed cross-crate warning that masks a
  release gate.

## P2 — compatibility growth, only after the direct-page gate

Every new feature must be an explicit `blue-ts-*` matrix change. It needs a
parser/binder/checker/emitter test, a direct-bridge/BlueJS runtime decision, a
provenance and debugger expectation, contract behavior at any new boundary,
and external-oracle evidence. A syntax form that happens to erase must remain
rejected until this evidence exists.

- [ ] **Control flow and function semantics.** Specify and implement the
  supported subset of loops, `try`/`catch`/`finally`, control-flow narrowing,
  return-path analysis, and callback/method overload resolution, gated on the
  matching BlueJS semantics and safe-point behavior.

- [ ] **Broaden expression/runtime lowering deliberately.** Assess optional
  chaining/calls, nested templates, object methods/accessors, array holes,
  member constructors, general iterable spread, and structural/union/`any`
  operand rules one at a time. Each item must define rejection/fallback rules;
  there is no emitted-JS reparsing fallback.

- [ ] **Make separate product decisions for large TypeScript features.**
  Classes, enums, decorators, namespaces, parameter properties, CommonJS,
  generic arrow functions, TSX/JSX, custom transformers, package-manager
  resolution, remote declarations, and full web-platform typings are not
  automatic backlog items. Propose each only with its runtime lowering, host
  capability/policy impact, debugger mapping, contract behavior, and
  differential/conformance plan.

## Out of scope for closure

Phase 18 does not close Phase 13's remaining general ECMAScript conformance,
Phase 17's complete automation/AJAX/SOAP product, or Phase 12's unrelated MCP
families. Those phases own their broader backlogs. Their listed prerequisites
above are tracked here only because they gate an honest BlueTS direct-page
claim.
