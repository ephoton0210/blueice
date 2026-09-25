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

**Current leaf: C1.1.2.4.** Prove both module-root step modes and state
transitions through the public real-process debugger route.
Every item has an ID (`<section>.<item>[.<step>]`, e.g. `B4.2.2`); commit
messages and PLAN.md cite
these IDs, and a parent is checked only when all its steps are.

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

- [x] **A1.1** Require the exact document generation on every existing request;
  reject a stale create-node request before mutation.
  Script IPC v2 also rejects obsolete/repeated Hello messages, bounds frames
  and DOM name/text fields, and proves through a real core subprocess that a
  still-open socket cannot mutate a replacement document with its old target.
  This remains a proof-of-mechanism DOM channel, not yet a child VM binding.
- [x] **A1.2** Require a launcher-issued, per-core child capability at the script
  socket handshake; reject a foreign or predecessor child before dispatch.
  Script IPC v3 requires a fixed-shape 32-byte capability. The launcher also
  supplies its fresh per-core BlueJS child secret to that core's private script
  listener; core refuses an unpaired listener/token and checks the Hello
  before handing any request to the DOM session. Real core and supervised
  launcher subprocess tests reject malformed, foreign, and predecessor
  capabilities, including after a successor core reuses the same path. The
  listener is owner-only (`0600`), preserves occupied files and live sockets,
  and drops an incomplete unauthenticated handshake after two seconds.
- [x] **A1.3** Verify wrong-tab, stale-generation, and old-core denial through a
  real socket; denied requests must leave the DOM unchanged. A real core
  subprocess test loads two live documents, compares both tab DOM dumps
  before and after a cross-tab node write, replaces the first document and
  checks stale writes/creation, then reuses the socket path under a successor
  core and queues an old-capability write. Every denial preserves the exact
  visible DOM, while an authorized successor write changes it as a control.

#### A2. Serve DOM calls during script execution.

- [x] **A2.1** Let the session thread answer bounded calls from the executing child
  while it waits for that child's result; keep TabManager on that thread.
  The page-host reply reader owns only a cloned socket. The session thread
  pumps at most 64 exact-tab/generation calls per poll during document
  synchronization or debugger resume; wrong-target calls fail without DOM
  mutation. Transport, executor, resume, and dispatcher regressions cover the
  wait boundary. The child VM does not yet issue these calls itself.
- [x] **A2.2** Cap calls and wait time; never process unrelated frontend/debugger
  work in the nested wait. Each document synchronization or debugger resume
  has a 1,024-request total allowance and a single 60-second deadline shared
  by request write and child reply. The first excess request receives a
  structured error and ends that execution wait; a stalled or disconnected
  child poisons only its page-host connection. The nested callback routes
  only exact-document script requests and never enters the ordinary frontend,
  debugger, or compiler dispatch loop. Focused socket-pair tests cover prompt
  timeout/disconnect, and budget tests show excess calls leave DOM unchanged.
- [x] **A2.3** Prove a child script completes one synchronous DOM lookup through a
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

- [x] **A3.1** Bind each child request/reply to the exact live tab and document;
  provide no child fetch, resolver, or direct core-DOM reference. Script IPC
  v4 requires a monotonic per-connection call ID and an exact tab/generation
  target in every response; core rejects naked, nested, and out-of-order
  calls, and the child rejects mismatched or unwrapped replies and closes the
  stream. Real core-socket and supervised child HTTP tests exercise the
  route; the child profile exposes only a boolean lookup, with no fetch,
  resolver, direct document object, or raw node ID.
- [x] **A3.2** Revoke that route on reload, tab close, and core cutover; test that
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

- [x] **B1.1** Keep node wrappers and their collector-visible roots in the child;
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
- [x] **B1.2** Reject a wrapper after node removal, reload, or realm close. Script IPC
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

- [x] **B2.1** Implement document.getElementById and node.textContent get/set
  through A; a real page must render the changed text. The owner-selected
  profile installs receiver-checked `document` lookup and opaque VM-rooted
  node wrappers. Its `textContent` accessor makes generation-bound core
  get/set calls. BlueTS now parses and checks the exact interface method,
  direct member call, inferred non-null result, and text assignment against
  the shared generated profile/inventory; ordinary realms keep only copied
  snapshots. A real HTTP page executes checked BlueTS getter/setter work in
  document order, renders the changed text, rejects an invalid typed call,
  and rejects a removed wrapper.
- [x] **B2.2** Implement createElement, createTextNode, and appendChild; a real
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

- [x] **B3.1** Root and remove click listeners in the child VM; removed and
  old-document listeners must never run. The owner-selected event profile
  installs VM-rooted listeners, and child tests cover removal, GC, stale
  generation rejection, and successor-realm isolation.
- [x] **B3.2** Dispatch a core click before default navigation; a real listener
  must run, and preventDefault must suppress link navigation. A real
  launcher/core/child test covers coordinate Click and ActOn, JavaScript and
  BlueTS callbacks, synchronous DOM writes, and navigation suppression.
- [x] **B3.3** Complete the first event-loop semantics from the Phase 13 binding
  decision: a core-originated click enters a one-slot, generation-bound child
  task queue, and a 256-job microtask checkpoint finishes before the child
  returns its cancellation bit for core's default action. Ordinary listener
  exceptions do not discard `preventDefault()` or skip later listeners;
  resource failures still fail closed. VM, child, and real-process tests cover
  the exception, checkpoint, queue bound, and DOM mutation ordering. This is
  a synchronous one-task delivery boundary, not a general-purpose event loop.

#### B4. Publish only implemented host typings.

- [x] **B4.1** Generate the exact BlueTS declaration/capability profile only after
  its runtime installer exists; do not add broad lib.dom declarations. The
  owner-selected event-v1 profile has the exact 10-binding installer inventory
  and the BlueTS checker verifies the click callback type and literal name.
- [x] **B4.2** Enforce the event object's `readonly` type, target, and currentTarget
  qualifiers in BlueTS checking. Acceptance is a finite list, not "every
  expression form". Delivered forms (each has a test under
  `backend/bluets/src/checker/tests/readonly*.rs`): direct, chained, and
  static/computed member writes; compound, update, and `delete`; writes nested
  in calls and expressions; typed array/tuple/record/union indexing; inferred
  aliases; array/record literals and tuple/record spreads; `as`/`satisfies`;
  sequence, `=`, logical, and logical-assignment results. The child already
  exposes these properties as non-writable and non-configurable, so runtime is
  the backstop for anything below.
  - [x] **B4.2.1** Fail-closed policy: a write whose receiver inference cannot model
    (`Unknown`) is rejected with "cannot prove ... avoids readonly members"
    whenever a readonly-bearing binding is in scope; declared `any` is an
    explicit opt-out, and an unrelated opaque receiver stays allowed. Any
    form not on the delivered list is therefore rejected, not silently
    accepted.
    Checked record spreads with `any`, `unknown`, or non-record sources also
    fail closed with a type diagnostic; an event-v1 bridge test rejects an
    opaque holder spread before runtime. Transpile-only mode is unchanged.
  - [x] **B4.2.2** Verify the policy against the event-v1 bridge (public boundary):
    `pick(event).type = 'click'` in a registered click callback is rejected
    with a BlueTS diagnostic before an executable script is produced.
  - [x] **B4.2.3** Record known limitations (arithmetic/bitwise compound
    results, destructuring, closure capture) in PLAN.md instead of adding
    forms; require a concrete failing page before expanding the accepted
    expression list.
- [x] **B4.3** Execute a supported BlueTS page through B2/B3; unsupported members
  must fail both static checking and JavaScript runtime access.
  - [x] **B4.3.1** Compile one click-handler page (getElementById, textContent,
    createElement/appendChild, addEventListener) through the event-v1 profile
    and run it in the real child; its execution report succeeds, and a core
    DOM dump contains the updated status and newly created text node.
  - [x] **B4.3.2** Reference `event.stopPropagation` in a registered click
    callback and `document.querySelector` at page root through the verified
    event-v1 bridge; both return checker diagnostics naming the member.
  - [x] **B4.3.3** Reach the same unsupported member through an `any` escape and assert a catchable runtime failure, never a crash or a silent `undefined` success. A real event-v1 HTTP page passes `document` and a core click event into checked BlueTS functions with explicit `any` parameters. Calling the absent `querySelector` and `stopPropagation` members raises `TypeError` caught by the page; later DOM writes and the click complete.
  - [x] **B4.3.4** Pin both failures in one paired test so static and runtime rejection cannot diverge. One launcher/core/child test checks named BlueTS diagnostics for both unsupported members against the verified event-v1 profile, compiles an explicit-`any` source using those same names, then runs it and asserts both catchable `TypeError` paths update the live DOM.

### C. Complete source debugging and metadata lifetime (Phases 17/18).

The exact classic-root pause/resume, instruction step, and bounded BlueTS
source-span step are delivered. Later control stays on the native debugger
channel with explicit owner/client grants.

#### C1. Extend execution control.

- [ ] **C1.1** Pause/resume and step a module root in a real page.
  - [x] **C1.1.1** Pause at the first module-root safe point and resume to completion.
    - [x] **C1.1.1.1** Specify the exact module-root safe point, graph and VM-state lifetime, and observable state transitions in PLAN.md. The entry evaluate-body boundary, pre-entry dependency effects, retained continuation, invalidation, source-free state sequence, and asynchronous/resource failure behavior are recorded there.
    - [x] **C1.1.1.2** Admit a checked ESM graph before execution and expose its exact entry program as `Pending` through the real debugger route. The child now attaches the graph and metadata at synchronization, registers the entry state, and advances the already attached graph once. An isolated-child two-module test inventories both programs and metadata before execution, while a real launcher/core/child debugger test observes the pending entry and later completion.
    - [x] **C1.1.1.3** Suspend BlueJS at the entry module's first evaluate-body root safe point and resume its retained graph to completion; cover the native VM boundary.
      - [x] **C1.1.1.3.1** Select the first verified root instruction at or after `module_evaluate_entry` for an exact live entry program; reject a classic or stale handle. BlueJS now selects the lowest root safe point in the evaluation body after checking exact realm ownership and module shape; a native regression checks its compiler entry offset and both rejection cases.
      - [x] **C1.1.1.3.2** Retain the linked graph and module context at that pre-instruction boundary; resume without replaying entry instructions or dependency effects. BlueJS now saves the entry frame and linked graph at its first evaluate-body instruction, roots the displaced frame through GC, and resumes the same module record. A two-module native regression checks the retained context and that both entry and dependency effects run exactly once.
      - [x] **C1.1.1.3.3** Verify native module pause/resume, error and asynchronous cleanup, and exclusion of concurrent realm execution. Native regressions cover a thrown entry, top-level await fulfillment and rejection, an async dependency before entry, retained error identity, and rejection of script/module/job entry points while paused. The continuation drains only the needed dependency jobs before entry pause and uses ordinary module-await jobs after resume.
    - [x] **C1.1.1.4** Wire the child/core debugger route and prove `Pending` → `Paused` → `Resuming` → `Completed` on a real BlueTS ESM page.
      - [x] **C1.1.1.4.1** Expose a generation-bound BlueJS page-runtime module-graph pause/resume pair, accepting only the exact entry evaluate-body safe point and retaining the graph's program identities. The page runtime validates every graph handle and the live entry generation, reserves linked module IDs, and routes pause/resume to the VM; a public-boundary regression covers wrong realm, wrong root point, concurrent execution, single evaluation, and navigation invalidation.
      - [x] **C1.1.1.4.2** Route the BlueTS ESM child queue through that pair, preserving exact arm/resume state and rejecting unrelated module or stale targets. The child now arms only the attached entry's first evaluate-body point, holds its queue head during `Paused`, resumes the retained graph on request, and still rejects module stepping. An isolated-child regression covers wrong entry and dependency targets, repeated advance, `Paused`/`Resuming`/`Completed`, and stale document rejection.
      - [x] **C1.1.1.4.3** Prove the public source-free state sequence and execution report on a real launcher/core/child BlueTS ESM document. A local-HTTP launcher integration test arms only a verified entry safe point through the public debugger socket, observes all four states, and checks the module's successful source-free script report and the loaded page DOM through the public browser socket.
  - [ ] **C1.1.2** Instruction step and BlueTS source-span step inside the module root.
    - [x] **C1.1.2.1** Step exactly one instruction in the retained module-root frame and return its actual verified successor or terminal state at the native VM boundary. BlueJS now re-parks the same entry frame after each root instruction and reports the real next offset, including backward loop hits; a native loop regression steps to terminal completion and proves the body result and continuation cleanup.
    - [x] **C1.1.2.2** Expose exact module-root instruction step through the page runtime and child scheduler without enabling dependency or nested-frame stepping. The page runtime forwards one native module step; the child accepts only its paused entry program, validates the returned successor as an exact live root safe point, and retains the queue head until resume. Boundary tests reject cross-tab, dependency and stale targets and prove a verified successor and unchanged pause on repeated advance.
    - [x] **C1.1.2.3** Reuse bounded BlueTS source-span stepping for a paused module root under its exact metadata/source receipts. The child now admits the armed BlueTS entry to the same 256-instruction source-span budget and metadata/source validation as classic scripts. A module regression rejects a wrong source ID, advances to the next bound source span, and resumes the same graph to completion; existing classic limit tests exercise the shared cap.
    - [ ] **C1.1.2.4** Prove both module-root step modes and state transitions through the public real-process debugger route.
  - [ ] **C1.1.3** Reject a pause/step request carrying a stale generation.
- [ ] **C1.2** Pause/step a nested frame and resume that same frame; reject stale
  or cross-tab targets.
  - [ ] **C1.2.1** Pause inside a nested call frame and step within that frame.
  - [ ] **C1.2.2** Resume that same frame; reject a stale frame identity after it returns.
  - [ ] **C1.2.3** Reject a cross-tab target and a target from a predecessor child.

#### C2. Inspect execution within fixed budgets.

- [ ] **C2.1** Return a bounded stack and scope for the paused frame.
  - [ ] **C2.1.1** Cap frame count and per-frame scope entries; report truncation explicitly.
  - [ ] **C2.1.2** Return stack frames with original BlueTS coordinates for nested and module frames.
- [ ] **C2.2** Return authorized values without guessed handles, excess depth,
  source text, or cross-realm references.
  - [ ] **C2.2.1** Return primitives and bounded plain data only; cap depth, length, and string bytes.
  - [ ] **C2.2.2** Refuse guessed handles, source text, and cross-realm references with a typed error.
- [ ] **C2.3** Report an exception at its original BlueTS source position.
  - [ ] **C2.3.1** Map a thrown error's position to the original BlueTS span for classic, module, and nested frames.

#### C3. Preserve original static metadata.

- [ ] **C3.1** Map nested/module locations, breakpoints, symbols, and type
  displays to the exact original source set; distinguish static types
  from runtime values. Partial: exact safe-point span replies now include
  bounded original UTF-16 coordinates for classic and module roots under the
  existing default-denied capability and same-stream source receipt; nested
  frame mapping and complete module execution control remain open.
  - [ ] **C3.1.1** Map nested-frame locations to the exact original source set.
  - [ ] **C3.1.2** Map breakpoints and symbols for module execution control.
  - [ ] **C3.1.3** Show static types separately from runtime values in every reply.
- [ ] **C3.2** On reload, close, cache eviction, or hibernation, invalidate or
  restore the same checked generation; never attach old metadata to a
  successor.
  - [ ] **C3.2.1** Invalidate metadata on reload and tab close.
  - [ ] **C3.2.2** Invalidate or restore the same checked generation on cache eviction and hibernation.
  - [ ] **C3.2.3** Assert a successor never receives the predecessor's metadata.

#### C4. Verify the public debugger route.

- [ ] **C4.1** Through launcher/core/child, test source pause, stepping, stack,
  scope, values, exception, and stale/unauthorized/over-budget rejection.
  - [ ] **C4.1.1** One real-process test per capability: pause, step, stack, scope, values, exception.
  - [ ] **C4.1.2** One rejection test per class: stale, unauthorized, over-budget.

### D. Close direct-page acceptance.

- [ ] **D.1** A real classic and ESM BlueTS fixture covers type-only import
  elision, authorized resolver identity, and original source mapping.
  - [ ] **D.1.1** Classic fixture: type-only import elision and original source mapping.
  - [ ] **D.1.2** ESM fixture: authorized resolver identity and original source mapping.
- [ ] **D.2** A real page exercises a declared contract and shows its bounded
  failure without exposing protected content.
  - [ ] **D.2.1** Trigger a contract failure on a real page and show its bounded report.
  - [ ] **D.2.2** Assert the report leaks no protected content.
- [ ] **D.3** Public-boundary tests cover multiple tabs, reload, policy isolation,
  and tab/child resource attribution; no required result depends only on a
  host-neutral unit test.
  - [ ] **D.3.1** Multiple tabs: separate results and attribution.
  - [ ] **D.3.2** Reload: no state carried across generations.
  - [ ] **D.3.3** Policy isolation between tabs and children.
  - [ ] **D.3.4** Tab/child resource attribution.

## P1 — strict contracts and compiler service

### E. Enforce live runtime contracts.

#### E1. Inventory only implemented boundaries.

- [x] **E1.1** Name and validate the copied document-text and origin results.
- [ ] **E1.2** For each new ingress/egress, record owner, source position when
  available, contract ID, limits, failure category, and capability.
  - [ ] **E1.2.1** Define the inventory record shape (owner, source position, contract ID, limits, failure category, capability).
  - [ ] **E1.2.2** Add a test that fails when a boundary lacks a record.
- [ ] **E1.3** Make strict-runtime reject a missing, unreifiable, or unchecked
  boundary unless a reviewed contract is authorized.
  - [ ] **E1.3.1** Reject a missing contract, an unreifiable type, and an unchecked boundary, each with its own diagnostic.
  - [ ] **E1.3.2** Allow the boundary only with a reviewed, authorized contract.

#### E2. Validate at the crossing.

- [ ] **E2.1** Run the pure bounded validator before data enters the VM; invoke
  no getter, proxy, page callback, or fetch during validation.
  - [ ] **E2.1.1** Validate before the value enters the VM.
  - [ ] **E2.1.2** Prove no getter, proxy trap, page callback, or fetch runs during validation.
- [ ] **E2.2** Attribute validation/cache cost to the initiating tab and report
  policy plus source position without protected content.
  - [ ] **E2.2.1** Charge cost to the initiating tab.
  - [ ] **E2.2.2** Report policy and source position without protected content.
- [ ] **E2.3** Test valid, malformed, cyclic, deep, oversized, and exhausting
  values at a live boundary.
  - [ ] **E2.3.1** One live-boundary test per value class: valid, malformed, cyclic, deep, oversized, exhausting.

#### E3. Preserve strict policy in emitted output.

- [x] **E3.1** Reject standalone strict-runtime build while its helper is absent.
- [ ] **E3.2** Emit a versioned helper and bind its identity to the artifact
  manifest; reject any target that erases a required check.
  - [ ] **E3.2.1** Emit a versioned helper file.
  - [ ] **E3.2.2** Bind the helper identity into the artifact manifest.
  - [ ] **E3.2.3** Reject any target that erases a required check.
- [ ] **E3.3** Make direct-page and emitted ESM reject the same malformed value;
  a weaker artifact must never claim strict-runtime.
  - [ ] **E3.3.1** Run one malformed value through both paths and assert the same rejection.
  - [ ] **E3.3.2** Assert a weaker artifact cannot claim strict-runtime.

### F. Finish registered-project compilation and MCP (Phase 12).

Keep check read-only and owner registration sealed before listeners.

#### F1. Authorize projects independently of query receipts.

- [ ] **F1.1** Pin canonical input/config/output roots and a closed source graph;
  client requests cannot add paths, resolver edges, options, or plugins.
  Partial: the owner-only startup catalog now rejects lexical path aliases,
  config/entry/source identities outside the declared project root, and
  output roots colliding with any catalog input or another output. Physical
  filesystem canonicalization and output-write authority remain open.
  - [ ] **F1.1.1** Physical filesystem canonicalization of input, config, and output roots.
  - [ ] **F1.1.2** Reject client-supplied paths, resolver edges, options, and plugins.
  - [ ] **F1.1.3** Output-write authority as a separate owner grant.
- [x] **F1.2** Admit only owner-exposed projects to each client inventory; deny a
  guessed or private project before reaching the compiler cache. Even a
  pre-populated core service now defaults to a private inventory until the
  owner explicitly exposes a registration. Direct adapter calls and accepted
  streams both reject private IDs. A real launcher/core/MCP test connects two
  distinct clients to one sealed catalog and verifies their separate receipts,
  owner-only inventory, private-ID rejection, and public-project checks.

#### F2. Produce bounded build artifacts.

- [ ] **F2.1** Bind results to project generation and fingerprint; cap artifact,
  diagnostic, and incremental-work-set responses.
  - [ ] **F2.1.1** Bind every result to project generation and fingerprint.
  - [ ] **F2.1.2** Cap artifact size, diagnostic count, and incremental-work-set size.
- [ ] **F2.2** Stage output atomically and emit nothing on compiler error; reject
  paths outside the authorized output root.
  - [ ] **F2.2.1** Write to a staging path and rename atomically.
  - [ ] **F2.2.2** Emit nothing on compiler error.
  - [ ] **F2.2.3** Reject output paths outside the authorized root.

#### F3. Complete the MCP adapter.

- [ ] **F3.1** Negotiate exact session/project/generation capabilities; keep
  pagination cursors one-shot and treat project strings as untrusted.
  - [ ] **F3.1.1** Negotiate exact session/project/generation capabilities.
  - [ ] **F3.1.2** Make pagination cursors one-shot.
  - [ ] **F3.1.3** Treat project strings as untrusted data.
- [ ] **F3.2** Expose build only with explicit output-write authority; a query
  receipt must never imply write access.
  - [ ] **F3.2.1** Hide build unless the owner granted output-write authority.
  - [ ] **F3.2.2** Assert a query receipt never authorizes a build.
- [ ] **F3.3** Through a real MCP client, inspect a diagnostic/type/contract
  failure and check/build an authorized project; reject stale, guessed,
  oversized, private, and unauthorized requests.
  - [ ] **F3.3.1** Inspect a diagnostic, a type, and a contract failure through a real MCP client.
  - [ ] **F3.3.2** Check and build an authorized project.
  - [ ] **F3.3.3** Reject stale, guessed, oversized, private, and unauthorized requests.

## Release gate

- [x] **R.1** Generated host typings, the direct BlueTS-to-BlueJS bridge, exact
  safe-point mapping, and the pinned TypeScript 5.9.3 CI oracle exist.

### G. Verify closure without exclusions.

- [ ] **G.1** Run workspace tests and applicable real-process suites.
  - [ ] **G.1.1** `cargo test --workspace` passes.
  - [ ] **G.1.2** Real-process suites (launcher, core, MCP, debugger) pass.
- [ ] **G.2** Pass formatting and all-target Clippy with warnings as errors.
  - [ ] **G.2.1** `cargo fmt --check` passes.
  - [ ] **G.2.2** `cargo clippy --workspace --all-targets -- -D warnings` passes.
- [ ] **G.3** Pass the pinned oracle and workspace coverage gate in CI; no
  ignored required test or suppressed warning may substitute for evidence.
  - [ ] **G.3.1** Pinned TypeScript oracle passes in CI.
  - [ ] **G.3.2** Workspace coverage gate passes in CI.
  - [ ] **G.3.3** No ignored required test or suppressed warning substitutes for evidence.

## P2 — compatibility after the page gate

### H. Grow control flow and function semantics.

- [ ] **H.1** Define one supported loop form, then test checker, direct execution,
  safe points, and oracle before selecting another.
  - [ ] **H.1.1** Pick one loop form and state its unsupported neighbors.
  - [ ] **H.1.2** Test checker, direct execution, safe points, and oracle for it before choosing another.
- [ ] **H.2** Define try/catch/finally with matching BlueJS execution and safe points.
  - [ ] **H.2.1** Define supported catch-binding and finally semantics.
  - [ ] **H.2.2** Test matching BlueJS execution and safe points.
- [ ] **H.3** Define control-flow narrowing and return paths through checker, emitter,
  direct execution, and oracle.
  - [ ] **H.3.1** Define narrowing forms and return-path analysis.
  - [ ] **H.3.2** Test checker, emitter, direct execution, and oracle.
- [ ] **H.4** Define callback/method overload resolution with the same four gates.
  - [ ] **H.4.1** Define resolution rules and the ambiguous-call diagnostic.
  - [ ] **H.4.2** Test with the same four gates: checker, emitter, direct execution, oracle.

### I. Grow expressions one form at a time.

- [ ] **I.1** Choose the next single form from optional calls/chaining, templates,
  object methods/accessors, member constructors, iterable spread, or
  structural/union/any operands; state its unsupported behavior.
  - [ ] **I.1.1** Record the chosen form and its unsupported behavior.
- [ ] **I.2** For that form, test parser/checker/emitter and direct runtime without
  reparsing emitted JavaScript.
  - [ ] **I.2.1** Parser test.
  - [ ] **I.2.2** Checker test.
  - [ ] **I.2.3** Emitter test.
  - [ ] **I.2.4** Direct-runtime test without reparsing emitted JavaScript.
- [ ] **I.3** Verify its provenance, debugger behavior, contracts, and TypeScript
  oracle evidence before choosing another form.
  - [ ] **I.3.1** Provenance and debugger behavior verified.
  - [ ] **I.3.2** Contract behavior and TypeScript oracle evidence verified.

### J. Decide large features only when requested.

- [ ] **J.1** For each proposed class/enum/decorator/namespace/JSX/CommonJS or
  package/remote-declaration feature, record its runtime lowering and
  host authority impact before adding it to the implementation backlog.
  - [ ] **J.1.1** Record runtime lowering per proposal.
  - [ ] **J.1.2** Record host authority impact per proposal.
- [ ] **J.2** Require a debugger map, contract policy, and conformance plan for
  each accepted proposal; defer features without that evidence.
  - [ ] **J.2.1** Require a debugger map, a contract policy, and a conformance plan per accepted proposal.
  - [ ] **J.2.2** Defer proposals lacking that evidence.

Phase 18 does not close general Phase 13 ECMAScript conformance, all Phase 17
automation/AJAX, or unrelated Phase 12 MCP families.
