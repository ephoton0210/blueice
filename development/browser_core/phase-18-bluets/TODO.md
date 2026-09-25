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

**Current leaf: C2.2.1.** Return authorized primitives and bounded plain data
without exceeding depth, length, or string-byte limits.
Every item has an ID (`<section>.<item>[.<step>]`, e.g. `B4.2.2`); commit
messages and PLAN.md cite
these IDs, and a parent is checked only when all its steps are.

## Current boundary

The supervised child runs authorized JavaScript and supported BlueTS classic
and module graphs. Ordinary pages expose copied document text and origin,
with no general live DOM or event API. Separate owner-only HTTP profiles
expose boolean/opaque lookup probes or the exact typed `document` lookup and
`textContent` slice; neither grants creation or events. Classic-root
pause/resume and bounded BlueTS source-span stepping work; nested-frame
pause/step/resume now have distinct public identities and capabilities.
Bounded, source-free Stack and Scopes inspection is available on the opt-in
debugger socket; original BlueTS frame coordinates and runtime values remain
disabled. The
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

- [x] **C1.1** Pause/resume and step a module root in a real page.
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
  - [x] **C1.1.2** Instruction step and BlueTS source-span step inside the module root.
    - [x] **C1.1.2.1** Step exactly one instruction in the retained module-root frame and return its actual verified successor or terminal state at the native VM boundary. BlueJS now re-parks the same entry frame after each root instruction and reports the real next offset, including backward loop hits; a native loop regression steps to terminal completion and proves the body result and continuation cleanup.
    - [x] **C1.1.2.2** Expose exact module-root instruction step through the page runtime and child scheduler without enabling dependency or nested-frame stepping. The page runtime forwards one native module step; the child accepts only its paused entry program, validates the returned successor as an exact live root safe point, and retains the queue head until resume. Boundary tests reject cross-tab, dependency and stale targets and prove a verified successor and unchanged pause on repeated advance.
    - [x] **C1.1.2.3** Reuse bounded BlueTS source-span stepping for a paused module root under its exact metadata/source receipts. The child now admits the armed BlueTS entry to the same 256-instruction source-span budget and metadata/source validation as classic scripts. A module regression rejects a wrong source ID, advances to the next bound source span, and resumes the same graph to completion; existing classic limit tests exercise the shared cap.
    - [x] **C1.1.2.4** Prove both module-root step modes and state transitions through the public real-process debugger route. A launcher/core/child integration test pauses an admitted BlueTS ESM entry, requests one root instruction through the public socket, checks its inventoried successor, then requests a metadata-receipted source-span step and checks the distinct bound span. It resumes to `Completed` and verifies the module execution report; the full real-process debugger suite passes.
  - [x] **C1.1.3** Reject a pause/step request carrying a stale generation. After a real HTTP reload, the same debugger stream receives `StaleRealm` for its previous module's root-breakpoint arm, root-instruction step, and metadata-receipted BlueTS source-span step; the successor realm has a distinct generation and remains discoverable.
- [x] **C1.2** Pause/step a nested frame and resume that same frame; reject stale
  or cross-tab targets.
  - [x] **C1.2.1** Pause inside a nested call frame and step within that frame.
    - [x] **C1.2.1.1** Specify the first nested-frame pause/step boundary, exact invocation identity, continuation lifetime, and fail-closed unsupported cases in PLAN.md. The decision distinguishes static code-unit safe points from invocation handles, constrains the initial synchronous direct-call shape, requires GC-visible parent/child continuations and exact successor steps, and reserves protocol widening until the entire route is wired.
    - [x] **C1.2.1.2** Retain one synchronous nested interpreted call and its caller at a verified inner safe point in BlueJS; step exactly one instruction in that same invocation, preserving operands, handlers, GC roots, and caller effects.
      - [x] **C1.2.1.2.1** Stamp each installed root/closure bytecode with the exact pre-order code-unit ordinal used by the safe-point inventory, including precompiled BlueTS input; test duplicate-shaped closures and reinstallations. Registry installation now stamps the bytecode tree while collecting its safe-point inventory; bare compiled bytecode remains untagged. A regression checks distinct ordinals for identical closure bodies, nested descendants, cloned code, separate generations, and both structured and precompiled installs. All 507 non-environmental BlueJS library tests pass.
      - [x] **C1.2.1.2.2** Capture one direct synchronous caller/child invocation at a selected verified inner instruction without treating suspension as a catchable JavaScript error; keep both frames GC-visible and reject unsupported call shapes. A native seam now retains the child execution context and root call-site operands at the requested inner instruction, with a fresh invocation serial and GC-visible child edges. It rejects deeper calls and constructors before target-body execution, and an old closure with the same ordinal but another program generation cannot satisfy the request. The public route remains unavailable until C1.2.1.3; 511 non-environmental BlueJS library tests pass.
      - [x] **C1.2.1.2.3** Step the retained child by one actual instruction under an invocation serial, restore the same child/classic caller on a successor, and integrate one terminal child result without replaying its call site. Native regressions verify actual backward loop PCs, wrong and returned frame serials, exactly one caller effect after resume, the waiting caller's temporary operand across GC, and a child throw entering the caller's catch. Direct eval and tail-call targets fail closed; 514 non-environmental BlueJS library tests pass.
      - [x] **C1.2.1.2.4** Apply that native nested pause/step/rejoin to an entry-module root without losing the linked graph, module cells, or entry completion; cover dependency effects and graph cleanup.
        - [x] **C1.2.1.2.4.1** Capture an exact entry-module child invocation at a verified inner safe point, retaining the module-root caller and linked graph after dependencies run once. A native module-graph entry now arms the same program-generation/code-unit target, pauses the direct synchronous child, and preserves the entry root plus linked graph; a two-module regression observes one dependency effect, no child-body effect before pause, and rejection of another execution entry. The public debugger route remains unavailable.
        - [x] **C1.2.1.2.4.2** Step that module child under the same invocation serial and rejoin the retained module-root `Call` without repeating dependency or entry effects. Native stepping updates the saved module-root operands, PC, and remaining instruction budget; a two-module test validates each child successor, resumes the same graph, and observes exactly one dependency and entry effect. An unhandled child throw clears both debugger frames and records a module error instead of leaving a false paused state.
        - [x] **C1.2.1.2.4.3** Preserve module catch/error and asynchronous graph cleanup through nested completion; verify no detached continuation or false completion.
          - [x] **C1.2.1.2.4.3.1** Route a catchable nested-child throw into the retained module-root handler stack and resume that same graph through catch/finally without fabricating a completion. The VM restores the saved entry execution and linked records for completion resolution, then re-parks the handler successor in the same graph. A native regression catches the original thrown value, runs `finally` exactly once, and completes without a module error; the unhandled-throw cleanup regression remains green.
          - [x] **C1.2.1.2.4.3.2** Verify an asynchronous dependency and entry await still reach nested pause, step/rejoin, and graph completion exactly once. A tagged two-module native regression puts an await in the dependency before the nested entry pause and another in the entry after child return; it verifies all four dependency/entry/child/post-await effects occur once, both records evaluate, and all module continuations and Promise jobs drain.
          - [x] **C1.2.1.2.4.3.3** Exhaust a bounded nested step and prove child/root continuations are cleared, graph state is not falsely completed, and later realm execution remains possible. A 256-instruction native module loop reaches `InstructionLimit` under repeated same-frame steps; no debugger frame survives, the entry record is neither evaluated nor suspended, and a subsequent script executes normally.
    - [x] **C1.2.1.3** Add an opaque, generation-bound active-frame debugger identity and route nested pause/step through the page runtime, child, page-host IPC, and public debugger protocol without conflating it with a static code unit.
      - [x] **C1.2.1.3.1** Expose source-free nested pause/step through the page runtime with an active-frame identity bound to tab, installed program generation, code-unit ordinal, and nonzero invocation serial; revoke it on return, failure, or realm replacement. Classic and linked-module page-runtime seams now mint a frame only on an actual child pause, validate the full live identity on every step, preserve the graph and parent on return, and clear the identity after a returned frame, instruction-budget failure, or navigation. Three new page-runtime regressions pass; 522 non-environmental BlueJS library tests and workspace Clippy pass. The wire route remains unavailable until C1.2.1.3.2–3.
      - [x] **C1.2.1.3.2** Carry the nested breakpoint and active-frame step through the child scheduler and page-host IPC, preserving classic and linked-module ownership and rejecting stale or unsupported targets. The classic and BlueTS module private schedulers now retain actual invocation frames, and page-host v35 carries exact source-free nested arm/state/step across child and core. The existing public root-state route deliberately does not expose a child frame.
        - [x] **C1.2.1.3.2.1** Retain a verified classic child-frame target in the child-only deferred scheduler, pause and step its exact invocation, and keep root-only controls unavailable while that child is active. The child now holds an exact static target only for a pending classic declaration, then records a separate live frame on actual pause. It validates each successor, re-joins the original root on return, and rejects root controls while the child is active; unsupported deeper targets fail as rejected script execution. JavaScript and BlueTS classic regressions pass. The route remains child-private until C1.2.1.3.2.3; 99 launcher library tests pass with the known `/private/tmp` environment-only socket test excluded, and workspace Clippy passes.
        - [x] **C1.2.1.3.2.2** Apply the same private child scheduling to a BlueTS entry-module graph, including frame return and preserved module-root continuation. The child now arms a verified inner point on a pending BlueTS ESM entry, retains its linked dependency graph on actual pause, steps under the same frame identity, and rejoins the original module-root continuation on return. A two-module regression observes exactly one dependency and entry effect after completion; 100 launcher library tests pass with the known `/private/tmp` environment-only test excluded, and workspace Clippy passes. The control remains child-private until C1.2.1.3.2.3.
        - [x] **C1.2.1.3.2.3** Add versioned page-host IPC and the core child proxy for nested arm/state/step, with exact document and program identity checks; leave the public socket disabled until C1.2.1.3.3. The source-free private frame identity and v35 commands/state are wired end-to-end; a real child/core regression passes.
          - [x] **C1.2.1.3.2.3.1** Define a child-private source-free frame wire identity bound to tab, document generation, child program generation, code-unit ordinal, and nonzero invocation serial; test exact tuple validation and serialization without adding commands or raising the protocol version yet. `PageHostDebuggerFrame` now keeps every identity component separate from a static safe point, rejects zero/root placeholders, and matches only its own program and code unit. Its source-free JSON round-trip and exact-tuple regression pass; all 98 IPC library tests and workspace Clippy pass. No page-host command or protocol version was added yet.
          - [x] **C1.2.1.3.2.3.2** Add page-host arm/state/step commands, child dispatch, and core child proxy under a dedicated nested-frame capability; raise the page-host version only once this private route is end-to-end. Page-host v35 now serializes a separately gated active-frame state and command; the child compares the whole tab/document/program/code-unit/invocation tuple before scheduling one step, and core remaps the child program identity and validates every reply. The real-process core-child test, 99 IPC tests, 101 launcher tests (one known `/private/tmp` test excluded), 32 JavaScript-child tests, workspace Clippy and formatting pass. The engine test socket helper now uses the system temporary directory so it runs on Linux.
      - [x] **C1.2.1.3.3** Wire a distinct active-frame identity, capability, pause state, and step command through the public debugger protocol and core route; bump the public protocol version only when end-to-end behavior is available (page-host v35 was already bumped with its complete private route).
        - [x] **C1.2.1.3.3.1** Remint a core-owned, process-unique active-frame handle only from an actual child pause, keep the child serial private, and revoke the handle on return, realm replacement, or executor/child replacement; test same-frame stability and stale-handle rejection. The core proxy now keeps one exact child-frame association per live tab and issues a process-global nonzero handle only after a verified child pause. It retains that handle across same-frame pause/step observations, drops it on return or realm close/replacement, and refuses stale handles even when a successor executor remints the same numeric program ID. The real-child regression and all 32 JavaScript-child tests pass; workspace Clippy and formatting pass. No public protocol exposure yet.
        - [x] **C1.2.1.3.3.2** Add the distinct public frame type, capability, nested arm/state/step commands, core route, protocol-version bump, and real-process checks without aliasing root controls or exposing the child serial. Public debugger v32 carries a core-reminted frame handle, separate nested capability/requests/states, and exact per-program checks. A real child/core route test proves nested step and root-control separation; public-socket BlueTS acceptance follows in C1.2.1.4.
    - [x] **C1.2.1.4** Prove the nested pause and same-frame instruction successor on a real BlueTS page through the public debugger socket; keep unsupported call shapes unavailable. A launcher-supervised BlueTS classic page advertises the distinct nested capability, pauses on its inner function's first verified instruction, steps to another safe point under the same core-owned frame, then rejoins and completes its root once. Root stepping cannot alias the child, and a second real page rejects an armed deeper call shape without exposing a frame. All 12 real-process public debugger tests pass.
  - [x] **C1.2.2** Resume that same frame; reject a stale frame identity after it returns.
    - [x] **C1.2.2.1** Add exact-frame resume to the BlueJS VM and page runtime for classic and entry-module roots, preserving caller/graph completion and rejecting wrong or returned frames. The VM runs the retained child to return without a step suspension, then parks its original caller at the next root instruction. The page runtime requires its complete live frame tuple and revokes it on return. Classic serial/call-count and linked-module graph/identity regressions pass; 524 non-environmental BlueJS library tests pass.
    - [x] **C1.2.2.2** Route that resume through the child scheduler, page-host IPC, and core child proxy under the existing nested-frame capability; distinguish its requested state from one-instruction stepping. Private page-host v36 carries an exact frame resume command, reply, and `NestedResuming` state. The child admits only its actual paused frame and later returns to the original classic or BlueTS module root; the core accepts only its own live reminted handle and revokes it on return. Child classic/module regressions and a real BlueTS child/core socket regression pass. The public resume command remains disabled until C1.2.2.3.
    - [x] **C1.2.2.3** Add a separate public frame-resume command and reply, bump the public protocol only when it works over the socket, and prove exact-frame completion plus stale-handle denial in a real BlueTS page. Public debugger v33 distinguishes `ResumeNestedExecution`, `NestedResumeRequested`, and `NestedResuming` from root resume and nested step. A launcher-supervised BlueTS page steps and resumes its exact core-owned frame through the public socket, rejects wrong/root-alias handles while active, then rejects that frame after return; root completion and its BlueTS report remain correct. The complete 12-test public debugger suite passes.
  - [x] **C1.2.3** Reject a cross-tab target and a target from a predecessor child.
    - [x] **C1.2.3.1** Prove a live frame from one tab cannot step or resume another live tab through the public debugger socket, even when both tabs have the same BlueTS program shape. A real Launcher test opens two live BlueTS pages with the same direct inner function, pauses each on its own core-owned frame, and forges both cross-tab frame/program combinations. Both public controls reject both forgeries without consuming either original frame; the original handles still resume their own pages.
    - [x] **C1.2.3.2** Prove a frame from a predecessor supervised child cannot control a successor after Launcher cutover, including when tab/document/program numeric identities are replayed; harden core-minted handle identity if the public regression exposes a collision. The real cutover regression exposed that both cores minted `frame_handle = 1` for otherwise identical second-generation BlueTS realms/programs. Public debugger v34 now binds each monotonic handle to a core-instance identity containing the process ID and 96 OS-random bits; failure to obtain entropy fails closed. The predecessor's public step/resume requests are denied after cutover, while the successor remains paused and its own handle resumes normally.

#### C2. Inspect execution within fixed budgets.

- [x] **C2.1** Return a bounded stack and scope for the paused frame.
  - [x] **C2.1.1** Cap frame count and per-frame scope entries; report truncation explicitly.
    - [x] **C2.1.1.1** Specify the source-free first stack/scope snapshot, exact paused-frame target, frame/entry budgets, truncation semantics, and default-denied transport boundary. PLAN.md fixes child-to-parent frame order, opaque active lexical slot/depth entries without names or values, caller-requested limits no larger than hard caps, and explicit stack/per-frame truncation. No public capability is advertised until the full route exists.
    - [x] **C2.1.1.2** Capture that bounded stack/scope snapshot from the retained BlueJS classic and module continuations through the page runtime; test nested/root frame order, active-scope exclusion, both truncation flags, and no execution or getter effects. The VM snapshots only its parked classic/module continuation and exact child serial into source-free code-unit offsets plus active lexical slot/depth entries, with hard 64-frame/256-entry caps and explicit truncation. Page runtime binds the request to a live tab, installed program generation, and exact nested frame. Tests cover child-first and root-only order in classic/module paths, inactive block exclusion, limit rejection, stale-frame denial, and a getter whose effects occur only on actual resume.
    - [x] **C2.1.1.3** Carry the exact paused snapshot through child scheduling, page-host IPC, and the core child proxy with generation/active-frame validation; keep the public stack/scope capability disabled.
      - [x] **C2.1.1.3.1** Define the bounded source-free private page-host request/reply, bump its version, and implement child-side exact paused root/nested snapshot validation with focused tests. Private page-host v37 carries a source-free, bounded request/reply; the child admits only a live document's queue-head paused root or exact nested invocation, validates the installed program and positive native/protocol budgets, and rejects stale documents, wrong frames, and in-flight stepping. IPC round-trip and child scheduler tests pass; no public capability changes.
      - [x] **C2.1.1.3.2** Route the private snapshot through the core child proxy, validating every echoed identity, active frame, limit, and source-free entry before returning a core-facing snapshot. The core checks the current paused state and its own core-to-child frame association before the request, then validates the full child echo, child-first/root-only shape, exact top offset, independent truncation and entry budgets, lexical depth order, and every returned safe point. The core-facing result has no private program/frame IDs. A real child route, malformed-payload matrix, and mismatched-reply identity test cover the proxy; no public request/capability is added.
      - [x] **C2.1.1.3.3** Prove the private route over a real child with stale document, wrong program/frame, both truncation flags, and no public capability; then check off C2.1.1.3. A real BlueTS child-host socket test reaches a paused function with at least two active lexical slots and confirms both truncation flags, exact child/root order, repeatable read-only capture, invalid budgets, wrong frame/program and successor-document denial. A real Launcher/core/host debugger socket test continues to report Stack and Scopes as Planned while nested control remains available; public protocol and commands are unchanged.
    - [x] **C2.1.1.4** Expose separately gated public stack/scope requests and bounded replies, bump the public protocol only with the complete route, and prove limits/truncation on a real BlueTS debugger socket.
      - [x] **C2.1.1.4.1** Specify distinct public Stack and Scopes gates, exact paused-target and per-frame scope selection, source-free reply shapes, budgets, and protocol-bump condition; do not change the wire in this design leaf. PLAN.md defines separate Stack (frame locations and stack truncation) and Scopes (one exact currently paused frame's active slots/depths and scope truncation) operations, with hard 64/256 limits and an expected safe point for stale scope selection. Both are default-denied outside the owner-selected debugger route; source, value, and BlueTS coordinates remain out of scope. No wire or protocol version changes in this leaf.
      - [x] **C2.1.1.4.2** Implement both gated public requests/replies through the core proxy, bump the debugger protocol with the complete route, and prove limits, truncation, stale-target denial, and capability states on a real Launcher-supervised BlueTS debugger socket; then check off C2.1.1.4 and C2.1.1. Public debugger v35 has independent GetStack/Stack and GetScopes/Scopes operations: Stack returns only child-first/root-only exact safe points and stack truncation, while Scopes returns one exact active frame's opaque lexical slots/depths and scope truncation. Positive 64-frame/256-entry caps, expected-safe-point matching, and exact core-instance frame identity fail closed. Real BlueTS socket tests cover both flags, wrong limits, stale/moved frames, root return/completion, and predecessor-frame rejection after core cutover. No source, value, or original BlueTS coordinate crosses this seam.
  - [x] **C2.1.2** Return stack frames with original BlueTS coordinates for nested and module frames.
    - [x] **C2.1.2.1** Specify the separately authorized, exact-stack BlueTS coordinate result and per-frame source receipts, including unbound safe-point failure and no source text or guessed nearest mapping; keep the base Stack source-free. PLAN.md fixes a separate batched coordinate operation that re-reads the exact ordered paused stack, requires same-stream OpaqueSafePointSpan plus metadata/source receipts for each frame under one live BlueTS attachment, and returns only existing half-open original byte/UTF-16 spans. An unbound or mismatched frame fails the whole batch; v35 Stack remains source-free and the wire is unchanged in this design leaf.
    - [x] **C2.1.2.2** Verify that the retained direct-lowering map resolves actual nested classic and module child/root stack safe points through the child/core exact-span route; repair any code-unit or module mapping gaps. BlueJS now records compiler-owned root statement instruction ranges and direct function-child indices. The retained map binds exactly those root instructions and their corresponding child code-unit instructions to original BlueTS statement/declaration spans, leaving root Halt unbound. Direct classic/module and real child/core paused-stack tests verify root Call/child entry, UTF-8 bytes, and UTF-16 columns after a non-BMP prefix.
    - [x] **C2.1.2.3** Add the receipt-gated public stack-coordinate request/reply, bump the debugger protocol only with the complete route, and prove nested/module frames, stale stack and source receipts, and bound/unbound behavior on real BlueTS debugger sockets; then check off C2.1.2 and C2.1. Public debugger v36 returns a separately gated, all-or-nothing coordinate batch while the base Stack remains source-free. Classic/module child/root spans, stale frames, missing/cross-stream receipts, wrong sources, and a genuinely paused unbound Halt are covered by real Launcher sockets.
      - [x] **C2.1.2.3.1** Define bounded target/reply value types with exact ordered frame and per-frame same-parent source identities; test malformed and mismatched values without adding wire variants or changing v35. The request carries the complete prior Stack snapshot and one source per frame; the reply carries that stack and ordered exact spans. Value validation enforces 1–64 frames, root/nested shape, same program/metadata parent, equal lengths, span order, and original coordinate well-formedness. IPC v35 remains unchanged.
      - [x] **C2.1.2.3.2** Install the complete receipt-gated core batch operation, add the request/reply wire and protocol bump together, and prove dispatch with focused core/IPC tests. Public debugger v36 adds GetStackCoordinates/StackCoordinates. Core checks the explicit span grant and each same-stream receipt, re-reads the entire paused Stack snapshot, and returns only all verified child spans; changed stacks or any unbound frame fail without partial coordinates. IPC round-trip, core mock dispatch, and workspace Clippy pass.
      - [x] **C2.1.2.3.3** Prove nested classic/module frames, stale stack/source receipts, and bound/unbound batch behavior on real Launcher-supervised BlueTS debugger sockets; then check off C2.1.2.3, C2.1.2, and C2.1. The v36 real-process tests verify exact original UTF-8/UTF-16 ranges after a non-BMP prefix, no partial reply for a truly paused unbound Halt, moved-frame denial, and both guessed and predecessor-stream source receipt denial. Core maps the private unbound result to public InvalidTarget. The 16-case debugger suite had one known pending-admission timing failure that passed in isolation; all other cases passed.
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
