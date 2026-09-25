# Phase 18 — BlueTS / BlueTSC completion worklist

[← Plan](PLAN.md) · [integration contract](INTEGRATION_CONTRACT.md) · [test interface](TEST_INTERFACE.md) · [DOM/event decision](../phase-13-bluejs-engine/DOM_EVENT_BINDINGS.md)

The goal is a supported BlueTS page that runs, interacts, debugs, and enforces
its declared boundaries through the real BlueIce processes. [PLAN.md](PLAN.md)
holds design history and delivered evidence. This file holds the ordered work
and its acceptance checks; checked leaves show prerequisites for the next step.

Work one leaf at a time, in the order below. Finish its implementation and
public-boundary test, commit it, then check it off before starting the next
leaf. A design-only leaf needs a reviewable decision instead of a runtime test.
Headings show dependencies; they are not tasks to finish in one commit.
Keep design history in PLAN.md and defer capabilities or syntax that do not
close the current leaf.

**Current leaf: B4.2.** Enforce the event object's `readonly` type, target,
and currentTarget qualifiers across all supported BlueTS expression forms.

## Current boundary

The supervised child runs authorized JavaScript and supported BlueTS classic
and module graphs. Ordinary pages expose copied document text and origin,
with no general live DOM or event API. Separate owner-only HTTP profiles
expose boolean/opaque lookup probes or the exact typed `document` lookup and
`textContent` slice; neither grants creation or events. Classic-root
pause/resume and bounded BlueTS source-span stepping work; nested/module
debugging, stack, scope, and values do not. The
compiler/MCP route supports sealed projects and read-only queries, with no
client registration, build output, or write authority.

## P0 — interactive and debuggable pages

### A. Connect the child to the live core DOM (Phase 13).

#### A1. Authenticate and identify every DOM request.

- [x] Require the exact document generation on every existing request;
  reject a stale create-node request before mutation.
  Script IPC v2 also rejects obsolete/repeated Hello messages, bounds frames
  and DOM name/text fields, and proves through a real core subprocess that a
  still-open socket cannot mutate a replacement document with its old target.
  This remains a proof-of-mechanism DOM channel, not yet a child VM binding.
- [x] Require a launcher-issued, per-core child capability at the script
  socket handshake; reject a foreign or predecessor child before dispatch.
  Script IPC v3 requires a fixed-shape 32-byte capability. The launcher also
  supplies its fresh per-core BlueJS child secret to that core's private script
  listener; core refuses an unpaired listener/token and checks the Hello
  before handing any request to the DOM session. Real core and supervised
  launcher subprocess tests reject malformed, foreign, and predecessor
  capabilities, including after a successor core reuses the same path. The
  listener is owner-only (`0600`), preserves occupied files and live sockets,
  and drops an incomplete unauthenticated handshake after two seconds.
- [x] Verify wrong-tab, stale-generation, and old-core denial through a
  real socket; denied requests must leave the DOM unchanged. A real core
  subprocess test loads two live documents, compares both tab DOM dumps
  before and after a cross-tab node write, replaces the first document and
  checks stale writes/creation, then reuses the socket path under a successor
  core and queues an old-capability write. Every denial preserves the exact
  visible DOM, while an authorized successor write changes it as a control.

#### A2. Serve DOM calls during script execution.

- [x] Let the session thread answer bounded calls from the executing child
  while it waits for that child's result; keep TabManager on that thread.
  The page-host reply reader owns only a cloned socket. The session thread
  pumps at most 64 exact-tab/generation calls per poll during document
  synchronization or debugger resume; wrong-target calls fail without DOM
  mutation. Transport, executor, resume, and dispatcher regressions cover the
  wait boundary. The child VM does not yet issue these calls itself.
- [x] Cap calls and wait time; never process unrelated frontend/debugger
  work in the nested wait. Each document synchronization or debugger resume
  has a 1,024-request total allowance and a single 60-second deadline shared
  by request write and child reply. The first excess request receives a
  structured error and ends that execution wait; a stalled or disconnected
  child poisons only its page-host connection. The nested callback routes
  only exact-document script requests and never enters the ordinary frontend,
  debugger, or compiler dispatch loop. Focused socket-pair tests cover prompt
  timeout/disconnect, and budget tests show excess calls leave DOM unchanged.
- [x] Prove a child script completes one synchronous DOM lookup through a
  real launcher/core/child HTTP fixture without deadlock. A trusted embedding
  owner selects the separate `blueiceTestHasElementById` proof profile; the
  launcher supplies its exact generation-private script socket and 64-hex
  capability to the supervised child. The child lazily authenticates, sends
  `GetElementById` with its executing tab/generation, and returns only a
  boolean to JavaScript, never a raw node ID. A real HTTP page tests both a
  present and absent ID in document order through launcher/core/child;
  ordinary child realms do not install the probe. This A2 boolean probe is
  separate from the later B1 opaque wrappers and B2.1 typed text profile.

#### A3. Preserve the child-to-core realm boundary.

- [x] Bind each child request/reply to the exact live tab and document;
  provide no child fetch, resolver, or direct core-DOM reference. Script IPC
  v4 requires a monotonic per-connection call ID and an exact tab/generation
  target in every response; core rejects naked, nested, and out-of-order
  calls, and the child rejects mismatched or unwrapped replies and closes the
  stream. Real core-socket and supervised child HTTP tests exercise the
  route; the child profile exposes only a boolean lookup, with no fetch,
  resolver, direct document object, or raw node ID.
- [x] Revoke that route on reload, tab close, and core cutover; test that
  an old reply cannot attach to a successor. Realm replacement and CloseRealm
  drop the VM-owned callback and its script socket. A child socket regression
  proves old streams reach EOF, a predecessor reply with the reused call ID
  is rejected by both replacement and reopened realms, and a fresh call can
  still succeed. A real launcher cutover regression proves the superseded
  child and script socket are removed before a successor with a distinct
  private endpoint serves, complementing the real core-socket denial of a
  predecessor capability.

### B. Install the first DOM and event profile (Phases 2/13).

Follow the [binding decision](../phase-13-bluejs-engine/DOM_EVENT_BINDINGS.md).
Keep per-tab VM, program, source, bytecode, and child-wide budgets.

#### B1. Give JavaScript safe node identity.

- [x] Keep node wrappers and their collector-visible roots in the child;
  never expose raw numeric node IDs to page code. A VM-owned host-object
  family converts exact `(tab, generation, node)` private keys into stable
  JavaScript objects, roots every wrapper and its private prototype for the
  realm lifetime, and caps the family at 4,096 wrappers. The primitive-only
  callback ABI still rejects JavaScript objects as Rust arguments. An
  owner-only `blueiceTestGetElementById` probe shares the authenticated DOM
  client with the prior boolean probe but returns a wrapper or `null`, never
  a numeric node ID. VM GC/identity tests and a real launcher/core/child HTTP
  fixture cover survival, repeat identity, an empty own-property surface,
  absence on miss, and lack of the probe in ordinary realms.
- [x] Reject a wrapper after node removal, reload, or realm close. Script IPC
  v5 adds an exact-document, read-only `ValidateNode` operation; core accepts
  allocated detached nodes but rejects reclaimed subtrees and stale/closed
  documents. VM-owned prototype methods resolve only exact wrappers in their
  originating family, reject forged receivers, and pass only a private key to
  the host. The owner-only `blueiceTestRequireLive` proof method revalidates
  through core on every call. VM, core dispatcher, isolated-child socket, and
  real launcher/core/child HTTP regressions cover the boundary. Reload and
  close discard the old VM and its rooted wrappers; neither can appear in a
  successor realm. General DOM text methods remain B2.

#### B2. Read and change live DOM text.

- [x] Implement document.getElementById and node.textContent get/set
  through A; a real page must render the changed text. The owner-selected
  profile installs receiver-checked `document` lookup and opaque VM-rooted
  node wrappers. Its `textContent` accessor makes generation-bound core
  get/set calls. BlueTS now parses and checks the exact interface method,
  direct member call, inferred non-null result, and text assignment against
  the shared generated profile/inventory; ordinary realms keep only copied
  snapshots. A real HTTP page executes checked BlueTS getter/setter work in
  document order, renders the changed text, rejects an invalid typed call,
  and rejects a removed wrapper.
- [x] Implement createElement, createTextNode, and appendChild; a real
  page must render the new subtree and reject a cross-document child. An
  owner-selected mutation profile installs the two factories and
  exact-wrapper append callback over generation-bound script IPC. Core
  recomputes styles when a detached element joins the tree. A real HTTP page
  executes JavaScript and checked BlueTS creation/append, verifies the live
  subtree and a changed rasterized frame, and rejects a forged child; invalid
  direct BlueTS calls fail checking. Script IPC v6 now mints process-unique,
  document-bound child handles instead of reusing raw per-document NodeIds.
  Two real HTTP pages in one core reproduce the former numeric collision,
  reject the foreign child through the authenticated script socket, preserve
  both DOMs, and leave the local detached child usable. VM tests also reject
  a same-family foreign document generation. BlueTS checks chained
  call-result receivers and rejects invalid arguments such as
  `document.getElementById('x')!.appendChild('wrong')`.

#### B3. Deliver a real click.

- [x] Root and remove click listeners in the child VM; removed and
  old-document listeners must never run. The owner-selected event profile
  installs VM-rooted listeners, and child tests cover removal, GC, stale
  generation rejection, and successor-realm isolation.
- [x] Dispatch a core click before default navigation; a real listener
  must run, and preventDefault must suppress link navigation. A real
  launcher/core/child test covers coordinate Click and ActOn, JavaScript and
  BlueTS callbacks, synchronous DOM writes, and navigation suppression.
- [x] Complete the first event-loop semantics from the Phase 13 binding
  decision: a core-originated click enters a one-slot, generation-bound child
  task queue, and a 256-job microtask checkpoint finishes before the child
  returns its cancellation bit for core's default action. Ordinary listener
  exceptions do not discard `preventDefault()` or skip later listeners;
  resource failures still fail closed. VM, child, and real-process tests cover
  the exception, checkpoint, queue bound, and DOM mutation ordering. This is
  a synchronous one-task delivery boundary, not a general-purpose event loop.

#### B4. Publish only implemented host typings.

- [x] Generate the exact BlueTS declaration/capability profile only after
  its runtime installer exists; do not add broad lib.dom declarations. The
  owner-selected event-v1 profile has the exact 10-binding installer inventory
  and the BlueTS checker verifies the click callback type and literal name.
- [ ] Enforce the event object's `readonly` type, target, and currentTarget
  qualifiers in BlueTS checking. The parser now retains `readonly` for
  interface and record fields, including inherited/generic lookup, and the
  checker rejects direct dot-property assignment, compound assignment,
  update, and deletion. Exact unescaped string-key computed writes share
  that check; dynamic or escaped keys fail closed when the receiver has any
  readonly member, including through generic inheritance. The child already
  exposes these properties as non-writable and non-configurable. The checker
  now infers parenthesized, chained dot/static-bracket, and direct-call result
  receivers before testing the final field. A separately bounded scan catches
  readonly writes nested in calls, assignment chains, arithmetic/boolean
  expressions, and prefix/postfix updates or deletion without charging plain
  assignments against generic-expansion fuel. Dynamically computed receiver
  chains and opaque/unmodeled expression forms remain outside this static
  check, so do not claim full qualifier enforcement yet.
- [ ] Execute a supported BlueTS page through B2/B3; unsupported members
  must fail both static checking and JavaScript runtime access.

### C. Complete source debugging and metadata lifetime (Phases 17/18).

The exact classic-root pause/resume, instruction step, and bounded BlueTS
source-span step are delivered. Later control stays on the native debugger
channel with explicit owner/client grants.

#### C1. Extend execution control.

- [ ] Pause/resume and step a module root in a real page.
- [ ] Pause/step a nested frame and resume that same frame; reject stale
  or cross-tab targets.

#### C2. Inspect execution within fixed budgets.

- [ ] Return a bounded stack and scope for the paused frame.
- [ ] Return authorized values without guessed handles, excess depth,
  source text, or cross-realm references.
- [ ] Report an exception at its original BlueTS source position.

#### C3. Preserve original static metadata.

- [ ] Map nested/module locations, breakpoints, symbols, and type
  displays to the exact original source set; distinguish static types
  from runtime values. Partial: exact safe-point span replies now include
  bounded original UTF-16 coordinates for classic and module roots under the
  existing default-denied capability and same-stream source receipt; nested
  frame mapping and complete module execution control remain open.
- [ ] On reload, close, cache eviction, or hibernation, invalidate or
  restore the same checked generation; never attach old metadata to a
  successor.

#### C4. Verify the public debugger route.

- [ ] Through launcher/core/child, test source pause, stepping, stack,
  scope, values, exception, and stale/unauthorized/over-budget rejection.

### D. Close direct-page acceptance.

- [ ] A real classic and ESM BlueTS fixture covers type-only import
  elision, authorized resolver identity, and original source mapping.
- [ ] A real page exercises a declared contract and shows its bounded
  failure without exposing protected content.
- [ ] Public-boundary tests cover multiple tabs, reload, policy isolation,
  and tab/child resource attribution; no required result depends only on a
  host-neutral unit test.

## P1 — strict contracts and compiler service

### E. Enforce live runtime contracts.

#### E1. Inventory only implemented boundaries.

- [x] Name and validate the copied document-text and origin results.
- [ ] For each new ingress/egress, record owner, source position when
  available, contract ID, limits, failure category, and capability.
- [ ] Make strict-runtime reject a missing, unreifiable, or unchecked
  boundary unless a reviewed contract is authorized.

#### E2. Validate at the crossing.

- [ ] Run the pure bounded validator before data enters the VM; invoke
  no getter, proxy, page callback, or fetch during validation.
- [ ] Attribute validation/cache cost to the initiating tab and report
  policy plus source position without protected content.
- [ ] Test valid, malformed, cyclic, deep, oversized, and exhausting
  values at a live boundary.

#### E3. Preserve strict policy in emitted output.

- [x] Reject standalone strict-runtime build while its helper is absent.
- [ ] Emit a versioned helper and bind its identity to the artifact
  manifest; reject any target that erases a required check.
- [ ] Make direct-page and emitted ESM reject the same malformed value;
  a weaker artifact must never claim strict-runtime.

### F. Finish registered-project compilation and MCP (Phase 12).

Keep check read-only and owner registration sealed before listeners.

#### F1. Authorize projects independently of query receipts.

- [ ] Pin canonical input/config/output roots and a closed source graph;
  client requests cannot add paths, resolver edges, options, or plugins.
  Partial: the owner-only startup catalog now rejects lexical path aliases,
  config/entry/source identities outside the declared project root, and
  output roots colliding with any catalog input or another output. Physical
  filesystem canonicalization and output-write authority remain open.
- [x] Admit only owner-exposed projects to each client inventory; deny a
  guessed or private project before reaching the compiler cache. Even a
  pre-populated core service now defaults to a private inventory until the
  owner explicitly exposes a registration. Direct adapter calls and accepted
  streams both reject private IDs. A real launcher/core/MCP test connects two
  distinct clients to one sealed catalog and verifies their separate receipts,
  owner-only inventory, private-ID rejection, and public-project checks.

#### F2. Produce bounded build artifacts.

- [ ] Bind results to project generation and fingerprint; cap artifact,
  diagnostic, and incremental-work-set responses.
- [ ] Stage output atomically and emit nothing on compiler error; reject
  paths outside the authorized output root.

#### F3. Complete the MCP adapter.

- [ ] Negotiate exact session/project/generation capabilities; keep
  pagination cursors one-shot and treat project strings as untrusted.
- [ ] Expose build only with explicit output-write authority; a query
  receipt must never imply write access.
- [ ] Through a real MCP client, inspect a diagnostic/type/contract
  failure and check/build an authorized project; reject stale, guessed,
  oversized, private, and unauthorized requests.

## Release gate

- [x] Generated host typings, the direct BlueTS-to-BlueJS bridge, exact
  safe-point mapping, and the pinned TypeScript 5.9.3 CI oracle exist.

### G. Verify closure without exclusions.

- [ ] Run workspace tests and applicable real-process suites.
- [ ] Pass formatting and all-target Clippy with warnings as errors.
- [ ] Pass the pinned oracle and workspace coverage gate in CI; no
  ignored required test or suppressed warning may substitute for evidence.

## P2 — compatibility after the page gate

### H. Grow control flow and function semantics.

- [ ] Define one supported loop form, then test checker, direct execution,
  safe points, and oracle before selecting another.
- [ ] Define try/catch/finally with matching BlueJS execution and safe points.
- [ ] Define control-flow narrowing and return paths through checker, emitter,
  direct execution, and oracle.
- [ ] Define callback/method overload resolution with the same four gates.

### I. Grow expressions one form at a time.

- [ ] Choose the next single form from optional calls/chaining, templates,
  object methods/accessors, member constructors, iterable spread, or
  structural/union/any operands; state its unsupported behavior.
- [ ] For that form, test parser/checker/emitter and direct runtime without
  reparsing emitted JavaScript.
- [ ] Verify its provenance, debugger behavior, contracts, and TypeScript
  oracle evidence before choosing another form.

### J. Decide large features only when requested.

- [ ] For each proposed class/enum/decorator/namespace/JSX/CommonJS or
  package/remote-declaration feature, record its runtime lowering and
  host authority impact before adding it to the implementation backlog.
- [ ] Require a debugger map, contract policy, and conformance plan for
  each accepted proposal; defer features without that evidence.

Phase 18 does not close general Phase 13 ECMAScript conformance, all Phase 17
automation/AJAX, or unrelated Phase 12 MCP families.
