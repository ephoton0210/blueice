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
They do not yet make a general TypeScript page executable or debuggable. The
bridge now has a limited, in-memory mapping from a single direct module's
top-level lowering spans to verified BlueJS safe points, plus an in-process
page host with two read-only document-oriented profiles. `blueice-core` can
now opt into one such profile for inline declarations, and expose only
source-free per-tab outcomes through its control plane. The opt-in JavaScript
host now also supports a bounded private debugger program-location inventory,
exact live safe-point validation, lifecycle-bound breakpoint configuration, and
an opt-in root-code-unit pause/resume seam. It can stop a pending classic
declaration at a verified non-entry root instruction while retaining its VM
root frame; it is not arbitrary interpreter suspension or stepping.
A launcher-supervised, capability-authenticated out-of-process BlueJS child
can execute caller-authorized inline JavaScript and explicit BlueTS declarations
in one bounded realm and DOM order. Its default core route rejects external
`src`; an explicitly selected immutable core-owned
`OutOfProcessPageScriptSourceAuthorizer` can instead admit a complete existing
typed JavaScript or BlueTS graph for the exact live declaration tuple. Core
copies that graph's canonical IDs, static edges, source bytes, and resolver
fingerprint to the child, which independently validates it and has no resolver
or source-loading fallback. For each core-verified document it also
installs immutable `blueiceDocumentText()` and
`blueiceDocumentOrigin()` copied snapshots: core validates their matching
1 MiB/4 KiB string contracts before serialization and the child repeats both
budgets plus canonical HTTP(S)-origin spelling before creating a realm. BlueTS
receives only the closed supplied graph and a child-fixed checked compiler
policy, then lowers directly into that realm without emitted-JavaScript
reparse. A shared generated and exact-inventory-verified
`core-script-document-context-v1` declaration admits calls only to those two
snapshot functions; page code cannot select, alter, or extend it. The core route keeps its
outcomes in the existing separate source-free JavaScript and BlueTS report
lanes, and closes child realms on navigation/tab removal. `blueice-launcher
--out-of-process-bluejs` creates that child/capability pair itself for each
core generation and reaps it on ordinary shutdown or cutover; the default
launcher leaves the mode disabled. An immutable core-owned startup authorizer
may additionally supply one exact closed external JavaScript or BlueTS graph;
the child still does not fetch, resolve, or fall back itself. Its private
page-host v5 transport permits source-free debugger location discovery plus
an exact bounded breakpoint-configuration table: after core verifies the
authenticated child owns the exact live
tab/document realm, it lists bounded child-private program/safe-point IDs and
revalidates one exact tuple. Core remints every public debugger program
handle/generation in a disjoint namespace and rejects any child reply whose
tab, document generation, private program, or safe point does not exactly
match that mapping; it checks exact child echoes for set/clear and revalidates
every listed private record before re-minting it. The child table is capped at
256 idempotent records and is discarded on realm replacement or close. The
ordinary child route has no pause/resume, stepping,
stack/scope/value/source/bytecode operation. When the same trusted core also
owns a debugger socket, it selects only the bounded root-classic lifecycle;
the child route also has a concrete but deliberately narrow
startup-only `HttpOutOfProcessPageScriptSourceAuthorizer`: core fixes a
same-document-origin or one canonical exact-origin rule, owner
URL-to-SHA-256 manifest, and module/depth/per-module/graph byte limits before
an executor exists. It admits only direct `200`, identity-encoded UTF-8
JavaScript/BlueTS MIME responses with matching bounded `Content-Length` and
integrity, disallows redirects, and parses only manifest-covered static edges.
Its verified private cache uses deterministic URL/integrity/language keys and
the finished graph fingerprint covers the whole policy. The page, child,
frontend, and MCP receive neither that authority nor its cache/manifest—only
a completed graph. The real core-to-child route can now also select one fixed
compiled `core-page-http-fixture-v1` profile at trusted startup: it permits
only one classic JavaScript path with one baked SHA-256 value below the live
document's canonical origin. The public launcher CLI has no switch for that
profile, and the private startup selector carries only its fixed name—not a
URL, manifest, resolver, source, path, or fetch setting. A real core
subprocess/HTTP/child-host regression proves that core builds the closed graph
before its private handoff. This is an integration fixture, not general
application-resource distribution. The remaining boundary has no general DOM
or event surface
beyond the copied strings, arbitrary debugger interruption/pause/runtime
control, or MCP project-registration path.

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
  dispatch without exposing a cross-thread DOM reference. The opt-in
  launcher-managed out-of-process BlueJS host is now wired through core; the
  JavaScript DOM bindings and host-wide source-fetch/cache accounting are
  still absent, so this prerequisite remains open. A core owner can now
  construct the in-process direct host with a
  caller-selected `DirectPageRealmOwner`, whose validated VM, realm,
  program-count, bytecode, and static-debug retention limits apply before a
  direct program executes. `DirectPageInlineExecutor` can receive the same
  owner for its explicitly enabled lifecycle seam, so inline declarations do
  not silently fall back to default resource limits. The host exposes only its
  live realm's per-tab
  program count, retained root-bytecode charge, and VM heap statistics;
  navigation/close releases old-realm charges before a successor is observed.
  This remains a per-host in-process policy, not a substitute for the process
  host's overall resource policy.

  `blueice-core --inline-bluets-profile <known-profile>` is an additional,
  explicitly configured process seam for the narrow inline executor. The core,
  not page content, selects the profile and fixed default compiler options.
  After a normal successful navigation, a client may send
  `GetBlueTsScriptReports` for its tab and receive a bounded ordered drain of
  tab/document generation, declaration ordinal/kind, and either `Executed` or
  a fixed rejection category. Reports contain no script source, diagnostics,
  runtime values, bytecode, or object handles; querying does not enable
  execution, and a core started without the flag rejects the query. A
  subprocess HTTP fixture now proves one classic and one module declaration
  execute under `core-script-document-text-v1`, while an invalid static call
  is reported source-free. This does not make the script-socket dispatcher a
  JavaScript binding or close the real out-of-process host acceptance.

  `blueice_engine::script::javascript::JavaScriptPageExecutor` now supplies
  the first bounded standard-JavaScript page-pipeline seam. It recognizes
  classic `<script>` declarations (missing/empty or common JavaScript MIME
  `type`) and `type="module"` separately from BlueTS, runs them only when its
  core owner explicitly synchronizes it, and uses the public `BlueJsPageRuntime`
  for an HTTP(S)-origin-bound realm per tab/document generation. Inline source
  receives a core-minted canonical identity and deterministic content hash;
  source is capped at 1 MiB per module, graphs at 128 modules, and BlueJS's
  fixed realm/program/bytecode limits apply before execution. Replacement,
  close, an unsupported origin, parse/compile/runtime failure, and an
  over-budget source leave source-free fixed outcome categories; one rejected
  declaration does not suppress a later declaration. External `src` fails
  closed by default. The only opt-in external authority is a core-owned
  `JavaScriptPageSourceAuthorizer`, which returns a closed graph of canonical
  source records plus every static `(from, specifier) -> canonical target`
  record; before compiling, the executor rewrites each static request to that
  canonical target and rejects a missing edge before admitting any graph
  program. It never fetches, performs URL/import-map lookup, or falls back to
  relative resolution. `blueice-core --inline-bluejs` is disabled by default,
  cannot be combined with the separate experimental BlueTS executor (so one
  page cannot receive two independent VMs), and exposes a tab-addressed drain
  of bounded source-free `GetBlueJsScriptReports` records; a normal session
  rejects that query without an enabled executor, so observation cannot enable
  JavaScript execution. A real subprocess
  HTTP fixture proves classic and module scripts execute in document order and
  that a parse rejection is redacted. A separate two-navigation subprocess
  fixture makes each document assert its own canonical origin snapshot and
  observes a new source-free execution report generation after both navigations;
  a stale first-document realm or callback would reject the second assertion.
  A two-tab session HTTP regression further proves that draining one tab's
  JavaScript reports neither exposes nor discards the other tab's report.
  A fixed one-byte bytecode realm budget unit fixture proves a compilation
  rejection retains neither a partial program nor a bytecode charge. This is an
  in-process,
  no-general-DOM-object-or-event-binding
  foundation. A separate launcher-owned process foundation now also exists:
  `blueice_ipc::page_host` defines a private v5 capability-authenticated
  launcher-to-child transport, and the `blueice-bluejs-host` child owns its
  own `BlueJsPageRuntime`, tab/document-generation table, program registry,
  and fixed realm/program/bytecode limits. A private frame is capped at
  12 MiB before payload allocation; a document is capped at 8 MiB source,
  each module at 1 MiB, and each graph at eight modules. It accepts only a complete
  caller-authorized document record containing canonical source IDs, exact
  source bytes and hashes, a non-empty resolver-policy fingerprint, and all
  static `(from, specifier) -> canonical-target` records. It recomputes each
  source hash, rejects duplicate/dangling/missing resolution records, and
  rewrites static ESM requests only to the supplied canonical targets; it
  never fetches, opens a URL/file, performs relative/import-map/package
  resolution, or receives DOM/IPC callbacks. The launcher creates an owner-
  only private socket plus per-spawn `/dev/urandom` capability token,
  authenticates before dispatch, supervises/reaps the child, and removes its
  socket on clean or forced teardown. The child returns bounded source-free
  reports, aggregate realm accounting, and the core-proxied location-only
  debugger inventory plus a 256-record exact breakpoint-configuration table;
  same-generation sync is
  idempotent, a stale generation cannot close or inspect a successor, and a
  missing module edge is rejected before any graph program is retained. Unit
  tests cover those rules, and a real launcher-spawned child regression proves
  the authenticated classic-plus-static-ESM route and clean shutdown.
  `blueice-launcher --out-of-process-bluejs` now creates a fresh child and
  private capability pair for each core generation, passes it only to that
  core's startup boundary, forwards loaded HTTP(S) inline declarations and,
  only when the startup-selected immutable core authorizer supplies a complete
  typed closed graph, external declarations through the core adapter, and
  reaps the child/socket on shutdown or cutover. Version 3 has no page-selected
  binding profile or capability field: core supplies
  exactly a copied document-text snapshot and canonical tuple-origin snapshot
  only after its existing `dom.document-text`/`dom.document-origin` pure
  contracts accept them. The child repeats the same 1 MiB/4 KiB byte limits
  and rejects a non-canonical or missing origin before it replaces a realm,
  then installs only immutable JavaScript callbacks
  `blueiceDocumentText()`/`blueiceDocumentOrigin()` through the restricted
  BlueJS registrar. Navigation, failed replacement, and close discard that VM
  and both captured strings. No DOM object, URL object, resolver, fetch,
  network, IPC, source/result value, or capability crosses that boundary. The
  launcher-to-core-to-child regression proves inline classic and module
  execution, explicit BlueTS direct lowering in the same realm and DOM order,
  source-free default/denied/invalid external-`src` rejection, immutable
  core-authorized JavaScript and BlueTS graphs, no static-edge fallback, no
  injected DOM/fetch binding, no endpoint reflection through the frontend
  broker, and both generations' cleanup. Its shared `blueice-bluets-bluejs` fixed typing
  artifact is generated from the exact two-callback runtime inventory and
  provides BlueTS only `blueiceDocumentText(): string` and
  `blueiceDocumentOrigin(): string`; unit and real-child regressions prove a
  typed call executes through direct lowering and `fetch` is a source-free
  rejection, while the unit regression also rejects invalid callback arity.
  The launcher-to-core-to-child regression now separately observes classic
  and ESM BlueTS execution reports in their shared realm and a wrong callback
  arity as the fixed source-free compilation-rejection category.
  It does not grant the child fetch, URL/import-map resolution, a filesystem,
  or external graph authority beyond the supplied graph. The concrete
  startup-selected child HTTP(S) authority now verifies a URL/SHA-256
  manifest, direct response policy, and a parsed closed static graph; the same
  core-owned implementation is the direct BlueTS external-source authorizer,
  not a duplicate loader. Its child process and direct core-session loopback
  regressions cover cache/integrity/MIME/redirect/cross-origin boundaries
  without source reflection. Core now caches each accepted child realm's
  source-free `(tab, document_generation)` program/bytecode/heap totals, then
  drops that record on replacement, tab removal, child error, transport loss,
  or a malformed/mismatched reply; it is never reflected to a page, frontend,
  debugger, or MCP client. A trusted launcher embedding can now select one
  immutable child-bootstrap envelope for realm count, programs/root-bytecode
  per realm, and VM managed heap per realm; both ordinary startup and cutover
  pass the same envelope to each fresh child before it binds its private
  socket. The public launcher CLI, page, frontend, core, and page-host IPC
  cannot inspect or widen it. This is not host-wide accounting or an RSS cap:
  VM heap accounting excludes allocator/Rust/source/registry/OS overhead and
  no aggregate child/fleet reservation exists. Those host-wide limits, native
  debugger attachment, and general JavaScript DOM-object/event binding remain
  open, so the prerequisite remains open.

  Acceptance: a page fixture can run a supported JavaScript classic script and
  module in its own realm; navigation/reload invalidates old program handles;
  an over-budget compilation fails without executing a partial program.

- [ ] **Expose minimal, truthful host bindings (Phases 2/13).** Define the
  first DOM/event binding surface and its policy/origin rules before declaring
  it in TypeScript. Do not expose a broad `lib.dom.d.ts` or add bindings merely
  to satisfy a checker fixture.

  Foundation delivered: `blueice_bluejs::Vm` now has a realm-local host
  callback ABI. An embedder can install an opaque global host object and
  non-constructable methods, backed by a VM-private callback registry. The
  boundary accepts and returns only `undefined`, `null`, booleans, numbers and
  `JsString`; object identities, `Symbol`, and `BigInt` fail before a callback
  can retain them outside the garbage collector. Callback failures become a
  host-controlled JavaScript `TypeError`, and installation rejects invalid or
  colliding global/member names. The ABI can install a global function as well
  as an object member, without exposing a VM or heap reference.

  The first actual binding is deliberately read-only and non-standard:
  `core-script-document-text-v1` declares exactly
  `blueiceDocumentText(): string`, with stable ID `dom.document-text`, runtime
  ID `global.blueiceDocumentText`, and `dom-read` capability. The direct page
  host accepts this non-empty profile only when it byte-for-byte matches the
  built-in typing artifact and runtime inventory. It snapshots the current
  document's recursive text into the realm-local callback; no DOM reference,
  node handle, mutable operation, event API, network API, or general
  `document` object enters BlueJS. A replacement document gets a replacement
  realm and snapshot. A non-empty profile with no matching installer fails
  closed before compilation/admission, and a realm cannot switch profiles.

  `core-script-document-context-v1` is the only broader composition currently
  accepted. It retains the same copied text snapshot and adds exactly
  `blueiceDocumentOrigin(): string` (`dom.document-origin`,
  `global.blueiceDocumentOrigin`, `dom-read`). The value is the core-derived,
  canonical tuple origin for the current document, never its path, query,
  fragment, URL object, or a capability-bearing DOM object. Navigation creates
  a new realm and therefore a new text/origin snapshot.

  The page runtime now exposes this substrate only through a temporary,
  realm-scoped registrar. It permits global-function/object/method registration
  but not VM execution, heap access, source inspection, or object inspection,
  and its callbacks disappear with navigation/reload/close when the realm VM
  is replaced. Direct-page tests prove the declared function returns the
  current document snapshot, rebinds after a same-origin document replacement,
  and rejects an uninstalled non-empty profile; an opt-in inline-executor
  lifecycle fixture executes the profile through discovered page declarations.
  Direct-page compilation enables the fingerprinted
  `require_declared_global_calls` policy, so a top-level direct call must be a
  local function or a supplied ambient host declaration. Consequently,
  `blueiceDocumentText()` and `blueiceDocumentOrigin()` under
  `core-script-empty-v1` fail with `UnknownName` before VM admission; a raw
  BlueJS realm with no registration rejects either global with `ReferenceError`.
  The declared profile also rejects an incorrect argument count statically.
  The bounded standard-JavaScript executor now installs those same exact
  `dom.document-text` and `dom.document-origin` bindings in every successfully
  admitted realm without exposing a `document` object. Before opening a realm,
  it validates the copied recursive text and core-derived canonical tuple origin
  against their identical core-selected reifiable string contracts; it then
  installs only zero-argument realm-local callbacks that capture those validated
  snapshots. A classic JavaScript page-level test proves the canonical origin
  value is returned and that a supplied argument becomes a host-controlled
  runtime failure; an oversized copied document fails before a JavaScript
  program or realm is retained. The `--inline-bluejs` subprocess fixture now
  invokes both bindings through ordinary HTML script execution. These remain
  immutable primitive snapshots, not a general DOM object, node handle, event,
  mutation,
  network, storage, or URL API.
  The first bindings do not yet satisfy the full DOM/event surface, so this
  item remains open.

  Acceptance: each initial binding has an implementation, capability policy,
  stable binding ID, and JavaScript page-level behavior test; an unimplemented
  API is absent and fails both static and runtime access tests.

- [x] **Give BlueJS generation-bound source identities and executable safe
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
  tested. This source-identity and safe-point prerequisite is complete.
  Complete nested-expression provenance, a process-owned page host, and
  debugger pause mechanics remain distinct open work below.

  Acceptance: a bytecode instruction can be named by `(code_unit, offset)` and
  verified by BlueJS; a program replacement invalidates its old IDs; malformed
  or stale IDs fail closed.

- [ ] **Implement the native debugger channel (Phase 17).** Add the versioned
  `blueice_ipc::debugger` path and native breakpoint, pause/resume, step,
  stack, scope, exception, and bounded-value operations. It must target one
  page realm and remain separate from script/DOM and network IPC.

  Foundation delivered: `blueice_ipc::debugger` now owns an independently
  framed v8 handshake, capability, bounded program-location vocabulary, and
  source-free root-frame execution-control vocabulary.
  Its page-realm, program, and safe-point identities include
  browser-context/tab/realm and program generations, reject zero placeholder
  handles, and require a host to validate an exact BlueJS instruction boundary
  rather than remap an offset. `ListPrograms`, `ListSafePoints`, and
  `ValidateSafePoint` are now real operations, not planned strings: they
  return only core-minted opaque program IDs and bounded `(code-unit, offset)`
  tuples, never canonical source IDs, source text, bytecode bytes, VM objects,
  or completion values. These additive request/reply and capability shapes
  advance the independent debugger protocol to v8, so a v1/v2/v3/v4/v5/v6/v7 peer fails its
  `Hello` negotiation rather than attempting to deserialize an incompatible
  capability report. `blueice_engine::debugger` routes post-handshake
  requests from a socket worker to the one session thread that owns live tabs;
  when that session selected `--inline-bluejs`, it resolves these opaque IDs
  through the current `JavaScriptPageExecutor` and BlueJS page-runtime registry.
  `blueice-core --debugger-socket <path>` binds that separate listener,
  negotiates `Hello` at its transport boundary, and validates the default
  browser-context ID, live tab, and exact private document generation before
  performing either discovery or location work.
  `ListPageRealms` first returns at most 128 currently loaded
  `(browser-context, tab, document-generation)` identities with no URL,
  source, program, bytecode, or runtime value; exceeding that cap fails with
  `ResourceLimit`. A stale realm returns `StaleRealm`; malformed/unknown
  targets return `InvalidTarget`; an obsolete program generation returns
  `StaleProgram`; and a non-boundary tuple returns `InvalidSafePoint`.
  `ProgramLocations` is `available` only for an enabled, live JavaScript page
  realm; it remains `planned` otherwise. v5 additionally exposes
  `ArmRootSafePointBreakpoint`, `GetExecutionState`, and `ResumeExecution`
  only when `--inline-bluejs` or the trusted out-of-process child route is
  paired with the private debugger socket.
  `ArmEntryBreakpoint` remains the zero-offset compatibility operation. The
  admission turn never immediately executes the new declaration; each
  handshaken bounded discovery/configuration request keeps it pending through
  one further session turn, so a peer can learn and arm an exact root-code-unit
  boundary. The out-of-process route caps those deferrals at 64 session turns
  per document, so discovery is not an execution lease. A successful
  `ResumeExecution` similarly exposes the source-free
  `Resuming` state for one bounded session turn before an idle scheduler turn
  resumes the retained frame; this is observability, not an execution lease.
  `ArmRootSafePointBreakpoint` starts a pending classic script and
  returns `Paused` only when BlueJS has reached that exact verified root
  instruction. Its continuation retains the root operand stack,
  bindings/cells, scopes, handler records, iterator records, completion state,
  and their GC roots; the paused VM rejects another execution until
  `ResumeExecution` completes the same frame. The reply contains no source,
  bytecode, stack, scope, object, or completion value. Modules, top-level
  await, child function code units, re-arming/loop hits, stepping, arbitrary
  nested-frame interruption, stack, scopes, exception policy, and bounded
  values remain planned. Unit coverage proves malformed-boundary rejection,
  same-document cross-tab rejection, realm replacement discarding a paused
  continuation, GC rooting of paused iterator records, and real BlueJS
  execution only after same-frame resume. A separate real `blueice-core`
  subprocess regression drives `Hello` through opaque discovery and the exact
  `Pending` → non-entry classic-root `Paused` → `Resuming` → `Completed`
  transition on the debugger socket. It proves cross-program and invalid-state
  resume failures leave a pending declaration pending; child code units,
  modules, repeated root arms, and stale-realm targets fail closed; and every
  raw framed debugger reply is checked not to reflect fixture
  source/completion data, a VM value, or a BlueJS opcode. The pre-existing
  `ArmEntryBreakpoint` subprocess route remains the v4 compatibility
  acceptance.

  The launcher-supervised child has a distinct v9 private page-host
  proxy for the public `ProgramLocations` and `BreakpointConfiguration`
  families. Core first
  resolves the public browser-context/tab/document generation, then requires
  its authenticated child to acknowledge that exact realm before advertising
  the capability. The child returns only its own bounded opaque program IDs
  and compiler-recorded `(code-unit, offset)` tuples; core remints disjoint
  public IDs and retains the exact private mapping. For `SetBreakpoint` and
  `ClearBreakpoint`, the child must echo the complete private tab/document/
  program/safe-point tuple; `ListBreakpoints` is capped and core independently
  revalidates every returned tuple before it re-mints the public reply. Any
  reply with another tab, document generation, program, or safe point is
  rejected. A core-owned debugger socket selects the otherwise default-off v7
  document lifecycle, which may defer document-order declarations for one
  turn, then arm only a pending
  classic program at an exact root-code-unit safe point. The child returns
  only `Pending`/`Paused`/`Resuming`/`Completed`; core validates the paused
  private tuple again before publishing its reminted public tuple, and only a
  later core-owned advance turn can resume the same root frame. It does not
  let polling a non-`Pending` state renew that hold; the OOP route additionally
  has the fixed 64-turn per-document discovery/configuration budget. It does
  not proxy `ArmEntryBreakpoint`, generic interruption, modules, child code
  units, re-arms/loop hits, stepping, stacks, scopes, exception policy, or
  values. Real Unix-socket child/core tests cover location discovery, exact
  validation, configuration lifecycle, non-entry root pause/resume, cross-tab
  rejection, malformed response rejection, and navigation-stale rejection
  without source, bytecode, or runtime-value leaks. `blueice-launcher
  --debugger-socket <absolute-path>` now owns an optional stable public `0600`
  debugger listener. It validates the public endpoint before child spawn,
  passes only a launcher-generated private debugger socket to each core
  generation, and shares the browser/compiler handoff gate. Each accepted
  debugger stream remains pinned to one private peer: a stream accepted for
  v1 fails closed when v1 ends rather than being retargeted to v2, while a new
  stream accepted after the committed cutover reaches v2. Realm, program, and
  safe-point handles therefore remain generation-bound.

  Acceptance for the delivered seam: a classic JS page fixture pauses at a
  verified root-code-unit safe point and resumes its same frame; realm
  replacement discards that continuation, and it cannot pause another tab or
  render transport. Stepping plus frame/value handles remain outside this
  capability and therefore have no acceptance claim yet.
  A separate launcher subprocess regression selects both
  `--out-of-process-bluejs` and `--debugger-socket`, drives the public
  debugger through `Pending` → non-entry-root-safe-point `Paused` →
  `Resuming` → `Completed`, then performs two local HTTP classic-script
  documents through only public sockets and proves that the original
  realm/program/safe-point tuple rejects as `StaleRealm` after reload.
  The test neither receives nor configures the private child endpoint or its
  capability.

## P0 — direct BlueTS page integration

- [x] **Generate and verify `lib.blueice.d.ts`.** Implement the
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
  The checked-in `core-script-empty-v1` fixture remains deliberately empty:
  core's narrow IPC dispatcher alone is not a BlueJS object binding. The
  checked-in `core-script-document-text-v1` fixture is the first non-empty
  artifact; it declares only `blueiceDocumentText(): string` and matches the
  direct host's `dom.document-text` snapshot binding. The checked-in
  `core-script-document-context-v1` artifact composes that same binding with
  `blueiceDocumentOrigin(): string`; the latter receives only the core's
  canonical tuple origin, never a URL or DOM object. No broad `document` or
  DOM node type is claimed. `GeneratedHostTypingsV1::verify_for_direct_compiler`
  now checks the selected manifest, exact declaration bytes, and complete
  runtime registration inventory before returning the one `.d.ts` source that
  a direct compiler may place in `CompilerOptions::ambient_declaration_modules`.
  BlueTS parses that declaration under its ordinary module/source limits,
  includes its exact bytes in the compiler fingerprint and static source
  metadata, exposes its declarations only as static ambient names, and emits
  no declaration code. Direct-page admission additionally enables the
  fingerprinted `require_declared_global_calls` policy, rejecting a direct
  call whose global function is neither local nor present in the verified
  ambient declaration root. The standalone `bluetsc` intentionally cannot set
  either host-only facility. Regression coverage proves the declared
  document-text/context bindings execute only in their matching profiles, the
  empty profile rejects both globals statically without admitting VM bytecode,
  an unconfigured BlueJS realm rejects either at runtime, and artifact
  mismatches execute nothing. New bindings must retain the same exact inventory,
  static-absence, and runtime-absence guarantees.

  Acceptance: a checked-in fixture generates byte-identical typing artifacts;
  every declared binding can be invoked in the matching host profile; an absent
  binding is rejected by both BlueTS and the host; a profile/schema mismatch
  executes nothing.

- [x] **Connect the direct bridge to the page loader.** Teach the page-script
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
  JavaScript-text reparse. `DirectPageScriptHost` performs the host-typing,
  script-kind, origin/policy/resolver/compiler-fingerprint, page-realm, and
  lifecycle-invalidation checks for its supplied closed request. Automatic
  production page loading remains a separate host responsibility.

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
  replacement realm. `blueice_engine::script::direct_page::DirectPageScriptHost`
  now gives a core/page-host caller one admission boundary: it requires an
  explicitly selected verified host profile, accepts only an
  `AuthorizedModuleLoader`, rejects caller-supplied ambient declarations and
  `transpile-only`, injects only the verified host declaration, and drives the
  owned realm admission/execution path for opted-in classic/module kinds. Its
  request now contains a typed core `TabId`, not an origin string or raw tab
  number: it reads the current `TabManager` page, derives a canonical HTTP(S)
  origin from its loaded URL, and tracks a core-private document generation.
  An execution therefore opens a realm only for a live document and recreates
  it on a same-origin replacement; `synchronize_tabs` also prunes a realm when
  its tab is gone. Blank and built-in pages fail closed until they receive an
  explicit origin policy. This remains an in-process core API, not the
  launcher-managed BlueJS process or an HTML script loader. The optional
  `run_session_with_script_requests_and_direct_page_host` entry point now
  accepts a core-owned host and synchronizes only realms that have already
  admitted a direct script after every frontend/session lifecycle batch. The
  parsed `Page` now also reports `application/x-blueice-typescript` and
  `application/x-blueice-typescript-module` declarations in document order,
  preserving either inline text or an external `src` without reinterpreting
  ordinary JavaScript or `text/typescript`. `DirectPageScriptHost::execute_inline`
  can now take one such inline declaration, mint its tab/document-generation/
  ordinal-scoped source identity, form exactly one closed source module, and
  submit it through verified direct admission. It rejects external `src`
  instead of fetching it, and an import remains unresolved without a future
  authorized graph loader. `DirectPageInlineExecutor` now adds a deliberately
  opt-in in-process page-pipeline seam: a core owner selects a known verified
  profile and compiler options once, the executor generates that profile's
  artifact itself, rejects `transpile-only` and caller-supplied ambient
  declarations, and runs each document's inline opted-in declarations once in
  document order after session lifecycle batches; a successfully fetched
  document runs them before its success reply and first frame. Each declaration has an
  independent result; without further authority, an external `src` is reported
  as a bounded source-free rejection and later declarations still run.
  `PageScriptSourceAuthorizer` is now the sole core-owned opt-in source seam:
  it receives a tab/document/ordinal plus raw document URL and `src`, and can
  return one already-authorized closed graph with a canonical entry and
  non-empty resolver fingerprint. The executor copies that exact fingerprint
  into compiler options and never fetches, resolves, or falls back on its own.
  Unit and real-session HTTP fixtures cover an authorized external ESM graph
  with a dependency. The explicit
  `run_session_with_script_requests_and_inline_page_executor` entry point has a
  regression fixture that fetches a real HTTP page through the normal core
  session pipeline and executes both an inline classic and inline module
  declaration. `blueice-core` retains that disabled default, but its explicit
  `--inline-bluets-profile <known-profile>` startup switch can construct the
  executor with a core-owned profile and default compiler policy. The source-
  free `GetBlueTsScriptReports` control-plane query drains only the addressed
  tab's bounded execution outcomes; it cannot enable the executor or expose
  page source, diagnostics, bytecode, or runtime values. The binary subprocess
  regression covers that opt-in mode across real HTTP navigation, a classic
  declaration, a module declaration, and a static rejection. The separate
  launcher-managed host now shares one BlueTS/JavaScript realm for its fixed
  snapshot profile and, only with an immutable core-owned startup authorizer,
  supplied closed external graphs. It also has only the core-proxied,
  source-free debugger location attachment described above: no debugger
  control or general DOM surface. The concrete narrowly scoped HTTP(S)
  fetch/cache/integrity policy is also the direct BlueTS external-source
  authorizer, so the in-process path does not duplicate network policy. There
  is still no general external graph policy, redirect/credential/import-map
  support, or deployment configuration. The closed-graph
  direct bridge
  integration is complete; those broader page-host responsibilities remain
  separate open prerequisites.

  Acceptance: one typed classic script and one typed ESM module graph execute
  in a real page with no generated `.js` input; parse/resolution/type/lowering
  failures execute neither the entry nor an affected dependent module; a
  runtime import cannot be re-resolved under a different host policy.

- [x] **Publish the TS-to-safe-point map.** Combine BlueTS lowering provenance
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
  metadata record. `DirectProgramAttachment::breakpoint_at_or_after` now
  resolves a same-canonical-module UTF-8 byte position to its containing
  top-level span or nearest following span, returning that exact verified safe
  point or the span's explicit `Unbound`; it never remaps an unbound span to a
  later instruction. The retained-map bound-only counterpart has the same
  deterministic search policy. This published direct-map item is complete.
  Page-host map aggregation, nested-expression locations, host-request
  fingerprint checks, full breakpoint search policy, and debugger IPC remain
  separate open work.

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
  Its compiler-options hash includes the direct-page
  `require_declared_global_calls` policy, so static metadata checked under an
  empty or verified host profile cannot be mistaken for metadata compiled
  under the standalone policy.
  `DirectModuleGraph::attach_debug_in_page_realm` now derives a module-local
  static subset (one source, that module's symbols, and their referenced type
  IDs) for every graph generation and rolls back all retained records/programs
  if any module fails the limit or identity checks. `DirectPageRealmOwner`
  prunes the registry automatically after its navigation/reload and close
  operations. The isolated page-host child now applies that same bounded
  registry directly to its BlueTS classic and ESM graph admission, retaining
  only the exact child-local BlueJS program generation; replacement, binding
  setup failure, debugger inventory failure, and explicit close prune it
  before a successor can be observed. Private page-host protocol v9 may now
  enumerate at most one freshly child-minted opaque metadata handle for one
  exact live BlueTS program after its authenticated core requests it. That
  handle is in a namespace distinct from the child program ID and carries no
  source/module/name/type/span/contract/count/value payload; JavaScript,
  missing attachments, stale generations, replacement, and close produce no
  usable handle. No static record itself crosses page-host IPC, except the
  dependent bounded summary available only after the handle has been minted:
  it carries a fixed language label, compiler-options fingerprint, and four
  aggregate counts, never a source/module/name/type/span/symbol/contract
  record, bytecode, VM object, or value. Debugger v8 now has separately
  negotiated `OpaqueInventory`, dependent `OpaqueSummary`, and dependent
  `OpaqueSourceInventory` manifest capabilities: core policy defaults to an
  empty grant, an owner must opt in with
  `--debugger-static-metadata-inventory` and independently select
  `--debugger-static-metadata-summary` and/or
  `--debugger-static-metadata-source-inventory`; the socket peer must request
  the matching canonical capabilities in `Hello`, and the exact live child
  realm must advertise them before core remints at most one public opaque
  handle and returns its summary or only parent-bound compiler-minted source
  IDs for that program. The public handle is generation-bound
  and numerically disjoint from both child and public-program IDs; the summary
  validates its exact realm/program/handle generation and is not a
  dereference/read operation. Page-host v9 and debugger v8 additionally make
  the next, still non-dereferenceable layer explicit: `OpaqueSourceInventory`
  is a separate canonical manifest capability, requiring both the parent
  `OpaqueInventory` grant and
  `--debugger-static-metadata-source-inventory`. It returns only a bounded
  list of compiler-minted numeric `source_id` values returned only under the exact
  public metadata handle. The core validates the live realm/program/handle
  mapping, child reply tuple, fixed source limit, and duplicate IDs before
  returning that parent-bound list. It returns no module name/URL, source or
  content hash (including the non-cryptographic private transport fingerprint),
  source text, span, symbol, type, contract, bytecode, VM object, value, or
  source-record dereference. Summary and source-ID inventory may be enabled
  together only through their full canonical manifest; neither is inferred
  from the other or from protocol version. Real launcher-supervised public
  debugger coverage requests both, proves the source IDs remain payload-free,
  and proves reload makes the prior metadata parent and source IDs stale.
  Page-host v10/debugger v9 now add that separately authorized provenance
  policy: `OpaqueSourceProvenance` requires both the inventory and
  source-inventory grants, its own owner flag
  `--debugger-static-metadata-source-provenance`, a source ID actually
  returned by that stream's bounded source inventory, and the same exact live
  parent tuple. It discloses only a
  non-filesystem canonical module identity and compiler labeled SHA-256
  digest; it never carries source text, spans, names, types, symbols,
  contracts, bytecode, VM objects, values, or a source-read operation. The
  debugger now also stores a bounded per-stream receipt for each opaque parent
  handle returned by `ListStaticMetadata`: `DescribeStaticMetadata` and
  `ListStaticMetadataSources` fail closed until that exact handle was actually
  returned under the stream's negotiated session. This prevents a guessed
  handle from becoming either a summary or source-ID target-probing oracle;
  source provenance remains indirectly bound by its source receipt.
  Page-host v11/debugger v10 additionally expose only compiler-minted
  type-record IDs through the independently default-denied
  `OpaqueTypeInventory` capability and
  `--debugger-static-metadata-type-inventory` owner flag. IDs are bounded,
  parent-handle-bound, same-stream-receipted, and source/type-display-free;
  a later type-display operation must require those exact receipts rather
  than accepting a caller-guessed ID.
  Page-host v12/debugger v11 now exposes that next type-display layer through
  independently default-denied `OpaqueTypeDisplay` and the owner-only
  `--debugger-static-metadata-type-display` flag. It requires the parent
  inventory, type-ID inventory, exact same-stream type-ID receipt, live realm
  tuple, and child capability report before one bounded compiler-produced
  display may cross the public debugger socket. The fixed 4 KiB display cap
  rejects malformed child replies; displays can contain project-authored type
  identifiers but carry no source text, span, source/module/hash, symbol,
  contract, bytecode, VM object, value, or general record read. The real
  launcher-supervised integration test proves the complete opt-in flow and
  reload invalidates a prior type target.
  Page-host v13/debugger v12 now add independently default-denied
  `OpaqueSymbolInventory` and
  `--debugger-static-metadata-symbol-inventory`. It requires the parent
  inventory, live realm tuple, child capability report, and the exact
  same-stream parent-handle receipt before it returns bounded
  compiler-minted symbol IDs. IDs remain payload-free and are separately
  receipted at the fixed 65,536-symbol ceiling, so a future symbol-detail
  operation cannot turn a caller-guessed ID into a child probing target. No
  symbol name, source span, declared type, contract, bytecode, VM object,
  value, or general metadata record crosses this layer.
  Page-host v14/debugger v13 now add independently default-denied
  `OpaqueContractInventory` and
  `--debugger-static-metadata-contract-inventory`. It requires the parent
  inventory, live realm tuple, child capability report, and the exact
  same-stream parent-handle receipt before it returns bounded
  compiler-minted contract IDs. IDs remain payload-free and are separately
  receipted at the fixed 65,536-contract ceiling, so a future contract-plan
  or validation operation cannot turn a caller-guessed ID into a child
  probing target. No contract name, source span, plan, validation result,
  bytecode, VM object, value, or general metadata record crosses this layer.
  Page-host v15/debugger v14 now add independently default-denied
  `OpaqueSymbolDisplay` and the owner-only
  `--debugger-static-metadata-symbol-display` flag. It requires the parent
  inventory, symbol-ID inventory, exact same-stream symbol-ID receipt, live
  realm tuple, and child capability report before returning one
  compiler-produced display capped at 4 KiB. The core verifies that the child
  echoed the exact requested symbol ID before re-minting the public reply.
  A display may contain a project-authored identifier, but it carries no
  source text, source/module identity, span, type, contract, bytecode, VM
  object, value, or general static-record read. Named
  `DebuggerMetadataCapabilitySelection` now constructs the canonical owner
  manifest, so new capabilities no longer extend a positional boolean list.
  Page-host v16/debugger v15 now add independently default-denied
  `OpaqueContractDisplay` and the owner-only
  `--debugger-static-metadata-contract-display` flag. It requires the parent
  inventory, contract-ID inventory, exact same-stream contract-ID receipt,
  live realm tuple, and child capability report before returning one
  compiler-produced contract name capped at 4 KiB. Core verifies the child
  echoed the exact requested contract ID before re-minting the public reply.
  The display may contain a project-authored contract identifier, but carries
  no source text, source/module identity, span, contract plan, validation
  behavior/result, type, bytecode, VM object, value, or general static-record
  read. The real supervised-child test now retains an actual reifiable
  interface contract and proves negotiated disclosure plus reload staleness.
  Page-host v17/debugger v16 add the tenth independently default-denied
  `OpaqueContractValidation` capability and owner-only
  `--debugger-static-metadata-contract-validation` flag. It requires the
  parent inventory, contract-ID inventory, exact same-stream contract-ID
  receipt, live realm tuple, and child report. Core rejects inputs exceeding
  fixed depth (64), collection-entry (4,096), node (32,768), string/key
  (256 KiB), or finite-number policy before forwarding; the child repeats
  those limits before its pure `ContractPlan` validator runs. It echoes the
  exact contract ID before core re-mints only `valid: true|false`; input,
  plan, failure path, expected/observed category, source/span/type, bytecode,
  VM object, runtime value, and static-record read remain unavailable. The
  public supervised-child regression proves receipt rejection, valid/invalid
  results without detail reflection, and reload staleness.
  Page-host v18/debugger v17 add the eleventh independently default-denied
  `OpaqueLoweringSummary` capability and the owner-only
  `--debugger-static-metadata-lowering-summary` flag. It requires the exact
  same-stream parent metadata receipt, a live realm/program/metadata tuple,
  and a child capability report before it returns only the canonical direct
  safe-point-map ABI, canonical BlueJS program ABI, an aggregate lower-case
  source-set fingerprint, and a bounded safe-point count. Core rejects a
  child-controlled ABI label, malformed fingerprint, over-limit count, or any
  stale/mismatched child tuple before reminting the public reply. No source or
  module identity, source span, map entry, AST node, code-unit identity,
  bytecode offset, VM object, value, or general metadata read crosses this
  layer. The real launcher-supervised regression proves default denial,
  receipt rejection, canonical disclosure, and reload staleness.
  Core now rejects page-host realm
  accounting with zero ownership fields, the child conversion sentinels, or
  more programs than the fixed 256-declaration × 8-module document envelope
  before it can enter either the core accounting cache or debugger liveness
  path. The page host still must route
  cache/hibernation events through the same invariant and add source policy,
  diagnostics/contracts, stack locations, and runtime-value inspection before
  this item can close.

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

  Foundation delivered: the current inventory intentionally names only the two
  installed core host-to-script result boundaries: stable binding
  `dom.document-text` / contract
  `core-script-document-text-result-v1`, and `dom.document-origin` / contract
  `core-script-document-origin-result-v1`. Both are `dom-read`, return one
  copied primitive string snapshot, accept no data from script, and have no
  source span because they are core-created host values rather than lowered
  TypeScript calls. Both bindings are installed by the verified direct BlueTS
  host and the bounded standard-JavaScript executor. The catalog exposes their
  stable runtime binding IDs,
  direction, capability, contract IDs, and independent validation limits. It
  explicitly contains no speculative JSON, Fetch/XHR, URL/query, storage,
  messaging, foreign-module, extension, or DOM-object boundary.

  Acceptance: `strict-runtime` rejects a supported boundary with no reifiable
  contract, unreifiable type, or unchecked `any` unless an authorized reviewed
  contract is supplied; `checked` and `transpile-only` remain visibly distinct
  policies.

- [ ] **Enforce contract plans at live boundaries.** Invoke the existing pure,
  bounded validator on data crossing each declared boundary, with no getter,
  proxy, user callback, fetch, or capability acquisition during validation.
  Attribute validator/cache/debug allocations and failures to the initiating
  tab and include the lowered behavior in the gatekeeper summary.

  Foundation delivered: before a document-text or document-origin snapshot can
  be captured in a direct realm callback, or either immutable snapshot in the
  bounded JavaScript executor, the core constructs its exact
  reifiable string `ContractPlan` and validates the copied value with
  core-selected `ValidationLimits`. The default text budget is the validator's
  one-mebibyte string limit and the canonical-origin budget is 4 KiB; a page
  request cannot loosen either. A violation rejects profile installation before
  BlueTS/BlueJS program admission, retains no program/bytecode/debug record,
  and becomes a source-free inline-executor result. The JavaScript executor
  likewise validates both snapshots before opening a realm or admitting any
  script, and rejects an oversized copied document or a canonical origin when
  its core-selected origin budget is tightened below the tuple string size.
  Both rejection fixtures retain no JavaScript realm. This covers only immutable
  primitive host results; it
  does not yet provide declared boundary source
  spans, strict-runtime coverage checks, validator/cache/debug allocation
  attribution, diagnostics retention, gatekeeper summaries, or any mutable or
  foreign-data boundary. A `blueice-core` subprocess regression now serves an
  HTTP document whose copied text exceeds the fixed one-mebibyte budget and
  verifies normal navigation still completes while the inline declaration is
  rejected before BlueTS/BlueJS admission. The only frontend observation is
  the fixed `host binding contract rejected the page script` category; neither
  the oversized content nor compiler/runtime diagnostics cross the IPC reply.

  Acceptance: malformed, recursive, cyclic, deep, oversized, and
  resource-exhausting values fail at a bounded path and budget; valid data
  reaches BlueJS without repeated local checks; failure diagnostics identify
  the policy and original source location without leaking protected content.

- [ ] **Preserve strict contracts in BlueTSC output.** Emit or import the
  deterministic versioned contract helper required by a `strict-runtime`
  build, record its identity in output metadata, and reject targets that would
  erase a required validation.

  Safety foundation delivered: standalone `bluetsc build` now fails closed for
  `strict-runtime` rather than publishing an artifact/manifest that merely
  names a stronger policy without its versioned runtime boundary helper.
  `bluetsc check` may still validate the static policy, but cannot claim an
  emitted enforcement boundary. Direct-page contracts remain a separately
  core-owned route. The helper, its output identity, and equivalent direct-page
  / emitted-ESM malformed-boundary behavior are still required before this item
  can close.

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

  Foundation delivered: `blueice_engine::compiler_service` now provides the
  core-owned in-memory `RegisteredProjectCompilerService`. Registration accepts
  one closed `AuthorizedModuleLoader`, fixed `CompilerOptions`, and the
  core-selected canonical project/config/output identities exactly once. Its
  opaque project and generation handles leave later check/build callers no
  path, resolver, plugin, source-graph, or compiler-option parameter to
  extend. `check` returns capped diagnostics, observable incremental-cache
  sets, a successful artifact fingerprint, and source-text-free static debug
  metadata; static type/symbol lookup requires the exact latest generation.
  Successful static metadata also retains core-minted source-hash provenance
  identities and only those local, non-generic declarations whose existing
  `ContractPlan` lowering is exact; imported, generic, erased, or otherwise
  unreifiable types deliberately have no contract ID.
  `build` returns only bounded in-memory `BuildOutput` and preserves no output
  on compiler errors, so this layer performs no filesystem writes. Registration,
  static metadata, and build-response limits fail closed.

  IPC foundation delivered: `blueice_ipc::compiler` defines a separately
  versioned, one-mebibyte-framed native compiler query protocol, and
  `blueice_engine::compiler_ipc::CompilerServiceIpcAdapter` owns the mapping
  to the registered service. Its `DescribeProject`, `Check`,
  `GetStaticType`, `GetStaticSymbol`, `ListStaticMetadata`, `GetStaticProvenance`,
  `GetStaticContract`, and `ValidateStaticContract` requests carry only opaque
  core-minted project/generation/metadata handles. `Check` returns capped work
  sets, diagnostics, fingerprints, and metadata counts; every static query is
  exact-generation-bound. `ListStaticMetadata` turns those counts into capped
  pages of opaque source/type/symbol/contract IDs, so a client can discover a
  `debug_get_symbol` ID before following its returned provenance/contract IDs
  rather than guessing an ordinal. The public MCP adapter now treats those
  pages as capability receipts: it rejects a guessed, wrong-category, or
  never-inventoried type/symbol/source/contract ID before forwarding a
  dereference or validation to core. A later successful check revokes that
  project's old receipts, and a fixed per-session receipt budget prevents
  cross-project inventory from accumulating without bound. A core-minted cursor is one-shot, bound to
  exactly one generation and collection, capped by service and response policy,
  invalidated on a later check, and rejects malformed, replayed, stale, or
  mismatched uses without falling back to another page. Provenance returns only
  a static module identity and
  content hash, never text. Contract reads expose a bounded static summary;
  validation accepts a bounded data-only tree under immutable core-selected
  limits and never echoes it. Canonical project/config/output roots are never
  returned, no source or emitted artifact crosses this protocol, and malformed
  handles, stale generations, v1 negotiation, oversized frames, over-budget
  fields, and invalid contract values fail closed. A worker may only forward
  decoded requests over the bounded channel; the adapter owner alone mutates
  the incremental compiler cache. There is no IPC registration/update request,
  filesystem loader, resolver/plugin/options extension or output write. At
  this adapter layer the launcher can distribute only its one compiled-in
  closed fixture profile through an explicitly selected local endpoint; it
  does not provide general project catalog/distribution or project update.
  Explicit artifact-write elevation remains a separate required capability.

  Core lifecycle foundation delivered: a trusted core startup owner now builds
  `CoreCompilerProjectCatalog`, registers complete closed projects through its
  Rust-only `register_startup_project` seam, then consumes it with `seal`
  before the listener/session begins. The sealed
  `CoreCompilerServiceSession` has no registration/update API; its bounded
  request receiver is the only route from an independently handshaken
  `blueice-core --compiler-socket` worker to the mutable adapter/cache on the
  core session thread. The reference process admits only the compiled-in
  `core-closed-fixture-v1` integration profile, rather than accepting a
  project path, source text, source graph, resolver, plugin, compiler option,
  output root, or write flag from its command line or socket. Real-process
  coverage proves rejected pre-`Hello` traffic cannot reach the catalog and a
  handshaken opaque `DescribeProject`/`Check` receives source-free,
  generation-bound metadata from the core-registered closed fixture. Its v5
  process regression also proves v1/v2/v3 rejection, a fresh core-minted
  per-accepted-stream session attestation and exact fixed query-only capability
  manifest, plus exact source-hash
  provenance/contract lookup and redacted invalid-data validation. The
  launcher now has one explicit `--compiler-mcp-socket <absolute-path>` seam:
  it validates and owns that stable public `0600` endpoint before spawning a
  child, passes only the fixed `core-closed-fixture-v1` profile and a fresh
  launcher-generated private compiler socket to each core, and reaps
  socket-only state on final shutdown. A live/non-socket public endpoint fails
  closed. During cutover it starts and health-checks v2's private sealed
  listener before changing the public relay for future accepts. Each accepted
  compiler connection remains pinned to the one private core peer selected at
  accept time; after v1 ends it fails closed rather than being retargeted to
  v2, so opaque cursors cannot cross catalog generations. Real launcher/core
  and paired MCP `tools/call` coverage verifies those lifetime rules as well
  as the fixed profile. General catalog distribution/authorization and all
  update/write capabilities remain deliberately open.

  MCP read foundation delivered: `blueice-mcp-server` now has a distinct
  `CompilerConnection` client and explicit
  `BlueIceMcpServer::connect_with_compiler_socket` and paired
  `connect_with_core_and_compiler_sockets` construction paths. The paired
  path attaches both adapters to one already-running core and never falls
  back to an unrelated browser process. After
  the separate compiler `Hello` negotiation, `bluetsc_describe_project`, `bluetsc_check`,
  `bluetsc_list_diagnostics`, `debug_list_static_metadata`, `debug_get_type`, `debug_get_symbol`,
  `debug_get_provenance`, `debug_get_contract`, and
  `debug_validate_contract` forward only opaque project/generation/metadata
  handles to the core service. `bluetsc_describe_project` accepts only a
  previously known opaque project handle and returns that handle plus its
  canonical entry-module identity; it cannot enumerate a catalog or reveal
  source/project/config/output roots. Contract validation accepts only bounded JSON
  data (not JavaScript values or JSON-inexpressible `undefined`) and never
  echoes it. Their JSON output is source-text-free and wraps project-controlled
  diagnostic prose, identifiers, and static displays as untrusted data. The
  ordinary browser-only `BlueIceMcpServer::spawn` path has no compiler
  connection: these tools report a fixed unavailable result and cannot
  manufacture a local registration or invoke BlueTSC. An end-to-end fixture
  now proves through the actual MCP `tools/call` boundary that the paired
  connection reaches a sealed core-owned catalog only through its
  worker-to-session hand-off after a separate compiler `Hello`, pages every
  source/type/symbol/contract ID collection, follows every ID through its
  exact query, and rejects cross-kind, replayed, and stale cursors without
  source text. It cannot use the connection to re-open startup registration.
  `bluetsc_session_capabilities` truthfully reports whether the compiler
  adapter is attached and, when it is, returns the exact opaque receipt minted
  by the core listener for that accepted relay stream together with the complete
  core-authored v2 manifest of nine read-only operations. MCP accepts neither
  a missing, malformed, subset, reordered, nor locally derived manifest. Every
  compiler tool echoes and requires that receipt;
  static queries additionally require the exact generation first observed by
  `bluetsc_check` under the same receipt and the individual ID first returned
  from the matching `debug_list_static_metadata` category. A malformed core
  attestation, mismatched/unobserved generation, guessed/wrong-category ID,
  or exhausted receipt budget fails source-free, and receipt/session state is
  never retargeted on launcher cutover.
  Compiler IPC v5 now adds `ListDiagnostics` and MCP's
  `bluetsc_list_diagnostics` as a ninth read-only operation. A successful
  `bluetsc_check` retains at most 4,096 source-free diagnostics for that exact
  generation while its immediate reply remains capped; the MCP tool requires
  the same session receipt and observed generation, then accepts only a
  core-minted one-shot diagnostic cursor. A page contains only code, severity,
  canonical module identity, byte range, and untrusted compiler prose. It
  never returns source text or a source-read path, and malformed/replayed
  cursors or a later check's stale generation fail closed. The real MCP/core
  regression exercises receipt mismatch, unobserved generation, and stale
  diagnostic-page rejection; service and adapter coverage exercise
  continuation/replay and response bounds.
  There is still
  no general launcher-owned catalog distribution or authorization beyond the
  one fixed closed profile, no remote registration/update/source/filesystem/
  resolver/plugin/options authority, no `bluetsc_build`, no artifact/
  declaration/source-map/source response, and no output write or elevation.
  The delivered `bluetsc_session_capabilities` receipt/generation gate covers
  this adapter's current connection lifecycle, including core-side
  per-accepted-stream attestation and exact capability manifest. Lowering/bytecode provenance, broader MCP
  negotiation, and the full MCP tool set remain open.

  Acceptance: `check` performs no writes; `build` keeps BlueTSC's atomic
  no-emit-on-error guarantee; responses are generation/fingerprint bound,
  capped, redacted where needed, and reject stale project state.

- [ ] **Complete the negotiated MCP BlueTS/BlueTSC adapter (Phase 12).**
  `bluetsc_describe_project`, `bluetsc_check`, `bluetsc_list_diagnostics`, `debug_list_static_metadata`, `debug_get_type`, `debug_get_symbol`,
  `debug_get_provenance`, `debug_get_contract`, and
  `debug_validate_contract` now adapt the native query service, not shell
  endpoints or a second compiler. Still add `bluetsc_build` only alongside an
  explicit output-write capability, then complete core-owned capability/session
  negotiation across adapters, authorization, broader result pagination,
  untrusted-content handling, and build-write elevation. Static metadata ID
  pagination is already bounded and generation-bound; its returned JSON is
  framed as untrusted project-controlled metadata.

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

  Foundation delivered: `backend/core/engine/tests/core_binary.rs` now starts
  the compiled `blueice-core` binary with an explicit
  `--inline-bluets-profile core-script-document-text-v1`, serves an actual HTTP
  document, and verifies the framed frontend IPC sequence for a classic
  declaration, module declaration, and rejected typed call. It observes only
  source-free outcome records after navigation. A second fixture opens and
  navigates a second tab in that same core process, drains each tab's reports
  through tab-addressed IPC, and proves the first drain neither leaks nor
  discards the second tab's record. A third fixture replaces one tab's document
  through two real navigations and observes one newly executed report at each
  distinct document generation. A fourth fixture drives a document-text
  contract violation through the same process path and observes only its fixed
  source-free rejection category. Full stale-handle invalidation, debugger,
  contracts beyond the two immutable snapshots, resource/policy isolation, and
  additional multi-tab cases remain required before this item can close.

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

  Local gate evidence (2026-09-21): following the checked-in reference README,
  the gitignored Test262 corpus was checked out at pinned revision
  `72faf8ec1445c55149615e8b35187830783aba1a`. With that required input present,
  `cargo test --workspace`, `cargo fmt --all -- --check`, and
  `cargo clippy --workspace --all-targets -- -D warnings` all passed. The
  focused engine library and real `blueice-core` subprocess suites also pass.
  No BlueJS warning allow, test exclusion, or corpus-free fallback was added.
  The local ignored TypeScript-oracle test remains intentionally dependent on
  `BLUEICE_BLUETSC_ORACLE`; its pinned 5.9.3 execution is still enforced by the
  required CI job, so this release item remains open pending the complete CI
  evidence rather than being marked complete from a local proxy.

  Latest local repeat (2026-09-23): after merging the remaining Test262
  failures and the current page-host/debugger/compiler integration work,
  `cargo test --workspace`, `cargo fmt --all -- --check`, and
  `cargo clippy --workspace --all-targets -- -D warnings` passed again. The
  TypeScript oracle remains CI-owned and is not claimed as locally executed
  without its explicitly provisioned `BLUEICE_BLUETSC_ORACLE` input.

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

  Foundation delivered: explicit functions whose return annotation excludes
  `undefined` now require every recognized `return`/`throw` or braced
  `if`/`else if`/`else` path to terminate; a bare return is diagnosed as
  `undefined`. Opaque control flow, loops, exception handlers, narrowing, and
  callback/method resolution remain open.

- [ ] **Broaden expression/runtime lowering deliberately.** Assess optional
  chaining/calls, nested templates, object methods/accessors, array holes,
  member constructors, general iterable spread, and structural/union/`any`
  operand rules one at a time. Each item must define rejection/fallback rules;
  there is no emitted-JS reparsing fallback.

  Foundation delivered: the direct bridge preserves holes in a non-spread
  array literal as BlueJS AST holes, with parser/checker/emitter/direct-runtime
  and TypeScript-oracle coverage. A literal that combines a hole and spread
  remains explicitly rejected because the current BlueJS spread construction
  path would materialize the hole as an `undefined` property. All other items
  in this compatibility-growth task remain open.

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
