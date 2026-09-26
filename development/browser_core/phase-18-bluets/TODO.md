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

**Current leaf: C3.1.3.2.3.** Join checked top-level BlueTS symbols/types to
exact structural BlueJS root slots and installed program generation; refuse
ambiguous, erased, mismatched, and cross-program joins.
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
debugger socket; original BlueTS frame coordinates and bounded runtime values
require separate owner/client grants and same-stream receipts. The
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
- [x] **C2.2** Return authorized values without guessed handles, excess depth,
  source text, or cross-realm references. The real public v38 socket gates exact
  plain-data previews, refuses target guesses and source-text probes with typed
  errors, and never returns a heap handle or partial value.
  - [x] **C2.2.1** Return primitives and bounded plain data only; cap depth, length, and string bytes. The native, private, core, and public routes now copy only exact paused plain data under independent owner/client grants and same-stream scope receipts; real Launcher-supervised classic/module and budget tests pass.
    - [x] **C2.2.1.1** Specify the exact paused-slot authority, lossless primitive and plain-data representation, side-effect exclusions, and hard resource budgets in PLAN.md; leave the wire unchanged. The selected active Scopes slot requires an owner value grant and same-stream receipt; previews are lossless tagged trees with 4/32/256/4,096 hard caps, no getter/proxy execution, and no reusable object handle. Public v36 remains unchanged.
    - [x] **C2.2.1.2** Read exact active root/nested binding primitives through a side-effect-free native VM snapshot, including cell-backed captures, with native regressions. Exact frame/safe-point/active-slot checks precede root, module, or nested binding reads; undefined/null/boolean/number bits/BigInt bytes/UTF-16 units are lossless, uninitialized and unsupported values refuse, and 4,096-byte primitive payloads are capped. Three native preview tests and Clippy pass; the 530-case BlueJS lib run has only the pre-existing native-stack environment test failure.
    - [x] **C2.2.1.3** Extend the native snapshot to bounded plain records and arrays without invoking accessors or proxy traps; reject unsupported shape, cycles, and every excess budget with native regressions. Heap-only descriptor traversal copies ordered own string keys and explicit array holes, rejects accessor/proxy/exotic/custom-prototype/symbol-key/cyclic shapes, and enforces depth 4, 32 entries, 256 nodes, and 4,096 aggregate payload bytes atomically. Three new native shape/budget tests pass; 532 BlueJS lib tests pass with only the pre-existing native-stack environment test excluded.
    - [x] **C2.2.1.4** Carry the exact bounded native preview through a versioned private page-host/child route and the core proxy; prove classic/module root and nested reads at that boundary.
      - [x] **C2.2.1.4.1** Define exact private target and bounded lossless preview value types, validate malicious depth/length/node/byte and mismatched-frame payloads, without adding wire variants yet. An exact tab/document/program/frame/safe-point/slot target and lossless tagged preview have independent shape/budget validators; duplicate record keys and impossible nested/root selection refuse. IPC v37 remains unchanged; all 103 IPC library tests and IPC Clippy pass.
      - [x] **C2.2.1.4.2** Expose the exact page-runtime slot read to the supervised child, add a complete versioned private request/reply, and test classic/module/nested dispatch and stale-target denial in the child. Private page-host v38 adds an exact GetDebuggerValueSnapshot/DebuggerValueSnapshot route; the child requires a live BlueTS attachment and retained queue-head pause, then remints only the validated bounded tree. Page-runtime, IPC socket, classic/module/nested child tests, all 103 IPC tests, 533 non-environmental BlueJS lib tests, and workspace Clippy pass. Launcher full lib has three independently reproducible failures in older source-breakpoint/source-step/socket tests; all five focused BlueTS child tests pass.
    - [x] **C2.2.1.4.3** Validate and remint the bounded child preview through the core proxy, with real Launcher-supervised classic/module root and nested regressions; then check off C2.2.1.4. Core re-reads the exact paused stack and active slot, binds the core frame to its child invocation, checks the entire echoed private target and preview budget, and returns a handle-free core-owned tree. A hostile child reply matrix rejects mismatched identity, excess bytes, and duplicate keys; Launcher-supervised classic/module tests read nested, caller-root, and resumed root bindings, reject forged/moved and successor-document targets, and keep public BoundedValues Planned. All 292 engine library tests and workspace Clippy pass.
    - [x] **C2.2.1.5** Add the owner-gated public same-stream scope-receipt request/reply, bump the debugger protocol with the complete route, and prove limits and stale-target denial on real Launcher-supervised BlueTS sockets; then check off C2.2.1. Public v37 and both real-socket leaves pass.
      - [x] **C2.2.1.5.1** Specify the independent owner/client value grant, exact scope receipt and pause-incarnation invalidation, bounded public tree, and refusal mapping before changing the wire. The grant is separate from static metadata; a core-local, per-stream receipt is bound to the active pause incarnation and every exact Scopes entry. The public protocol stays v36.
      - [x] **C2.2.1.5.2** Add the bounded core-local scope-receipt ledger and global pause-incarnation invalidation with focused core/IPC tests; keep the public wire and BoundedValues advertisement unchanged. Each stream stores at most 4,096 exact slot tuples under one core-owned pause incarnation; duplicate/malformed snapshots and atomic budget overflow refuse, and accepted execution controls invalidate receipts across streams. Two IPC receipt tests, the engine invalidation test, and workspace Clippy pass; v36 and BoundedValues Planned are unchanged.
      - [x] **C2.2.1.5.3** Add the independent owner/client value opt-in and complete public GetValue/Value route together with the debugger protocol bump; prove negotiation, policy, dispatch, malformed targets and complete preview limits at public boundaries. Public debugger v37 requires separate owner/client value grants, records only granted same-stream Scopes slots, and gates an exact live pause before returning a bounded handle-free tree; real Launcher-supervised value cases remain C2.2.1.5.4.
        - [x] **C2.2.1.5.3.1** Carry a default-denied owner `--debugger-bounded-values` policy through Launcher to core; require an explicit debugger endpoint, test CLI validation and propagation, but leave public negotiation and value reads unchanged. Launcher and core reject the flag without a debugger socket; Launcher forwards the independent opt-in to its spawned core, while v36 BoundedValues remains Planned. Focused core/Launcher CLI and launch-option tests pass.
        - [x] **C2.2.1.5.3.2** Add the separate client grant, bounded public target/reply, and complete receipt-gated core route in one debugger protocol bump; test handshake, dispatch, malformed targets, and budgets before checking off C2.2.1.5.3. The v37 Hello/HelloAck grant, GetValue/Value wire, owner-gated core listener, per-stream Scopes receipt, capability report, and native remint are complete; IPC/engine suites and real core grant negotiation pass.
          - [x] **C2.2.1.5.3.2.1** Define exact public target and independently validated lossless preview/snapshot types; test malformed identity, depth, length, nodes, bytes, and duplicate keys without new wire variants or a protocol bump. The public target binds program, core frame, index, safe point, and slot; the handle-free tagged tree enforces depth 4, length 32, nodes 256, and aggregate payload 4,096. All 107 IPC library tests and workspace Clippy pass; debugger v36 is unchanged.
          - [x] **C2.2.1.5.3.2.2** Add the separate client grant and complete owner/receipt/native-gated request/reply route, bump debugger v37 only with that route, and prove negotiation/dispatch/limits before checking off C2.2.1.5.3.2 and C2.2.1.5.3. Both subleaves below are complete.
            - [x] **C2.2.1.5.3.2.2.1** Build and test the core-internal value read: gate an exact same-stream scope receipt and pause incarnation, re-read the active scope, invoke the already validated child proxy, and remint an independently budgeted public snapshot. Do not add wire variants or alter v36 availability. Exact core-frame and slot routing, cross-stream/old-pause/forged-slot denial, moved-frame rejection, lossless payload, and all remint budgets pass two focused tests, all 295 engine library tests, and workspace Clippy.
            - [x] **C2.2.1.5.3.2.2.2** Bind the owner/client grant to Hello, wire the complete GetValue/Value dispatch and receipt recording, advertise only live granted BoundedValues, bump to v37, and prove negotiation, denial, and limits before checking off C2.2.1.5.3.2.2 and its parents. IPC 108/108 and engine 296/296 pass; a real core socket proves unrequested versus separately owner-granted HelloAck, the Launcher cutover endpoint passes, and workspace Clippy passes. Mock public dispatch proves missing/cross-stream/stale-pause denial, exact Scopes receipt, moved-frame rejection and lossless preview.
      - [x] **C2.2.1.5.4** Prove classic/module root and nested value reads, cross-stream and stale-pause denial, forged slots, and all preview budgets on real Launcher-supervised BlueTS sockets; then check off C2.2.1.5 and C2.2.1. Both focused real-process tests pass.
        - [x] **C2.2.1.5.4.1** Prove exact granted classic/module nested and caller-root reads, then resumed root reads, through the real public Launcher-supervised socket. A real public v37 socket with independent owner/client grant reads lossless numeric slots from each nested child, its waiting caller root, and its resumed root; both classic and module paths pass.
        - [x] **C2.2.1.5.4.2** Prove cross-stream and stale-pause denial, forged slots, and all preview budgets on real Launcher-supervised BlueTS sockets; then check off C2.2.1.5.4 and its parents. The classic/module fixture refuses a target on an unreceipted second stream, an ungranted client, a forged slot, and the old nested pause after resume. A second real BlueTS fixture reads four exact 4/32/256/4,096 cap values while refusing four one-over-cap parameters atomically; both focused tests and workspace Clippy pass.
  - [x] **C2.2.2** Refuse guessed handles, source text, and cross-realm references with a typed error. Public debugger v38's denial-only source-text probe and the receipt-gated GetValue target use fixed typed refusals without source or realm disclosure.
    - [x] **C2.2.2.1** Specify the public typed refusal matrix and non-disclosing validation order for guessed value targets, cross-realm references, unknown source-text requests, unsupported shapes, and budgets; decide whether the wire needs a version bump before implementation. PLAN.md selects generic typed `CapabilityUnavailable` for post-Hello source-text probes and unit unknown commands, keeps exact receipted GetValue targets and `InvalidTarget` before any realm lookup, retains typed `InvalidExecutionState` for native unsupported/over-budget values, and requires public v38. IPC's unit `Unknown` cannot parse a payload-bearing command, so v38 adds a denial-only `GetSourceText { program }` probe, never a positive source-read route.
    - [x] **C2.2.2.2** Implement the typed refusal mapping at the public debugger boundary with IPC/core/child regressions, preserving independent value grants and adding only a denial-only source-text probe. Public debugger v38 adds `GetSourceText { program }` only to return target-independent `CapabilityUnavailable`, and unit unknown commands get the same typed reply. Existing GetValue receipt checks reject guessed or cross-realm targets before child access. A raw probe preserves IPC framing; 109 IPC and 297 engine library tests, five focused private child tests, and workspace Clippy pass.
    - [x] **C2.2.2.3** Prove guessed handles, source-text attempts, cross-realm and unsupported-value refusal on real Launcher-supervised sockets; then check off C2.2.2 and C2.2. The classic/module v38 test refuses local/foreign source-text probes and unit unknown commands identically, keeps framing, and rejects guessed program and foreign-realm value targets as `InvalidTarget`; a separate real BlueTS nested function argument refuses without a partial preview. Both focused tests pass; the classic/module test once hit the previously documented pending-admission timing race and passed unchanged on rerun. The v38 exact-cap budget test and workspace Clippy pass.
- [x] **C2.3** Report an exception at its original BlueTS source position. Public debugger v39 returns only core-reminted, original compiler-bound locations after independent owner/client grants and same-stream source receipts; classic/module nested and refusal routes pass real Launcher-supervised sockets.
  - [x] **C2.3.1** Map a thrown error's position to the original BlueTS span for classic, module, and nested frames. Native uncaught origin, exact child attachment, private route, public authorization, and real-socket positive/negative proofs are complete.
    - [x] **C2.3.1.1** Specify uncaught throw-site capture across nested calls and catches, exact compiler-bound mapping with no nearest-span fallback, source-free child/core retention, and a separately granted public location request/reply before changing code. PLAN.md binds the VM sidecar to debugger generation/code-unit/instruction offset, maps only exact live BlueTS attachment records, keeps frontend reports source-free, and reserves existing `OpaqueSafePointSpan` owner/client plus same-stream source receipt for the future public location; private/public version bumps occur only with complete routes.
    - [x] **C2.3.1.2** Capture the exact originating BlueJS debugger code-unit generation/ordinal and instruction offset for an uncaught catchable exception; clear caught/superseded sites and prove classic, module, and nested VM behavior without a wire change. Native source-free sidecar preserves child origins through classic/module propagation, clears catches, and replaces superseded throws; direct/nested/catch/finally debugger VM tests, workspace Clippy, and format checking pass.
      - [x] **C2.3.1.2.1** Add a source-free, generation-bound native throw-site record for exact classic root instructions; expose it only after uncaught catchable failure, clear it on catch/new execution/normal completion, and test direct throw plus caught and successor cases. `VmDebuggerThrowSite` records only program generation, code-unit ordinal, and instruction offset. Interpreter completion stamps an exact catchable throw; catch entry clears the transient site; outer completion publishes it only for an uncaught error and clears it on the next execution. The focused test, all 31 debugger VM tests, and workspace Clippy pass; no wire changed.
      - [x] **C2.3.1.2.2** Preserve the original child site through nested propagation and module evaluation, replace it for a new throw, and prove nested/classic/module/catch/finally cases; then check off C2.3.1.2. A per-execution throw epoch distinguishes fresh throws from caller propagation; module completion publishes only uncaught catchable sites. All 34 native debugger VM tests, workspace Clippy, and format checking pass; no wire changed.
    - [x] **C2.3.1.3** Map the retained site only through its exact live BlueTS safe-point attachment in the supervised child, add a versioned private location route, and prove classic/module/nested mapping, unbound refusal, and replacement expiry. Child-only snapshots bind exact classic/module/dependency sites and reject absent/unbound/stale targets; private page-host v39 and a core-only transport wrapper complete the route. Focused child, IPC socket, real supervised-child, core transport, and workspace Clippy checks pass.
      - [x] **C2.3.1.3.1** Expose the VM's most recent source-free uncaught site at the page-runtime boundary only when its installed generation belongs to the exact tab-owned program; prove classic/module/nested results, later execution overwrite, and realm invalidation without changing IPC. The page-runtime getter checks tab ownership and installed generation before returning the source-free sidecar; focused classic nested/module root/module nested and overwrite/cross-realm/discard/navigation tests pass.
      - [x] **C2.3.1.3.2** In the supervised child, capture each terminal debugger-controlled execution before another program runs, match its exact live BlueTS attachment and safe-point map, retain only a bounded private location under its document/program identity, and prove classic/module/nested, no-native-site/unbound, and replacement expiry without adding a wire route. Terminal classic/module/dependency execution snapshots only exact verified entries; normal BlueTS/ordinary JS and unbound offsets retain no location, and navigation removes the record. Ten focused private-child tests and workspace Clippy pass; current BlueTS direct lowering does not accept a top-level `throw` or `try/catch`, so caught-throw socket proof remains C2.3.1.5 after a supported fixture is available.
      - [x] **C2.3.1.3.3** Add the complete page-host v39 private exception-location request/reply and core transport wrapper, with typed exact-identity/stale refusals and IPC plus real-child route tests; then check off C2.3.1.3. The route echoes document/program/metadata, returns only the exact source-free safe point and bounded original span, and refuses pending, wrong program/metadata, or stale documents without partial location. All 14 page-host IPC tests, a real Launcher-supervised child socket test, the core wrapper socket test, focused child tests, and workspace Clippy pass; no public debugger command changed.
    - [x] **C2.3.1.4** Remint the exception location through core behind the existing independent safe-point-span owner/client grant and same-stream source receipt; add the complete public request/reply and protocol bump with typed denial tests. Public debugger v39 accepts only a receipted `source` under the independent grant, revalidates the exact native site and source binding, and returns a distinct core-minted `ExceptionLocation`; all 110 IPC and 300 engine library tests plus workspace Clippy pass.
      - [x] **C2.3.1.4.1** Decide the source-only public request/reply, core-private target/result, receipt/grant validation order, and typed no-location/refusal matrix before implementation; leave public debugger v38 unchanged in this design-only leaf. PLAN.md fixes the source-only request, distinct source/safe-point/span reply, independent `OpaqueSafePointSpan` grant before source-receipt validation, `CapabilityUnavailable` for absent grant, `InvalidTarget` for missing/forged receipts or mismatched locations, and `InvalidExecutionState` for pending/normal completion; no public wire changed.
      - [x] **C2.3.1.4.2** Add a default-denied core location API and an out-of-process adapter that revalidates the live child program/metadata/source/safe point and remints only a bounded source-free result; prove malformed, cross-program, stale, and normal-completion child replies without a public wire change. The adapter rechecks the private exception tuple, exact safe point and compiler-bound span before returning numeric core identities; a focused malformed-reply/default-deny test and all 299 engine library tests pass. No public debugger wire changed.
      - [x] **C2.3.1.4.3** Add the complete public debugger v39 `DescribeExceptionLocation { source }` request/reply behind the existing independent `OpaqueSafePointSpan` owner/client grant and same-stream source receipt, with IPC/core typed denial tests; then check off C2.3.1.4. The public request chooses only a prior source ID; absent grant returns `CapabilityUnavailable`, missing/forged receipts or mismatched positions return `InvalidTarget`, and no terminal uncaught site returns `InvalidExecutionState`, never a partial position. All 110 IPC and 300 engine library tests plus workspace Clippy pass.
    - [x] **C2.3.1.5** Prove original classic/module/nested UTF-8 and UTF-16 exception spans, missing grants/receipts, caught errors, and stale generations on real Launcher-supervised debugger sockets; then check off C2.3.1 and C2.3. Four focused real-socket tests prove the positive path and all refusal classes; a direct bridge regression proves the caught fixture evaluates to the caught value, with workspace Clippy and format checks passing.
      - [x] **C2.3.1.5.1** On real Launcher-supervised public debugger sockets, prove a receipted classic and module nested BlueTS throw returns only the original compiler-bound UTF-8 byte span and UTF-16 coordinates, with the exact code-unit safe point and no error/source text. A real Launcher/core/child socket test passes for classic and module nested throws: each returns the original function span, exact first child code unit in public inventory, and UTF-16 columns distinct from UTF-8 bytes across an astral prefix; the extra module source ID is refused without a partial location.
      - [x] **C2.3.1.5.2** On real public sockets, prove absent owner/client grant, missing/cross-stream source receipt, normal or caught throw, and replaced document/program generations return typed no-partial-location refusals; then check off C2.3.1.5, C2.3.1, and C2.3. Absent grant returns `CapabilityUnavailable`; missing or cross-stream receipt returns `InvalidTarget`; normal/caught execution returns `InvalidExecutionState`; old document sources return `StaleRealm`. Three focused Launcher socket tests, the checked caught-value bridge test, workspace Clippy, and format checks pass.

#### C3. Preserve original static metadata.

- [ ] **C3.1** Map nested/module locations, breakpoints, symbols, and type
  displays to the exact original source set; distinguish static types
  from runtime values. Partial: classic/module root, same-program nested, and
  two-source linked dependency/caller stacks have exact receipt-bound original
  spans and original source/symbol breakpoint control; static/runtime display
  separation remains open.
  - [x] **C3.1.1** Map nested-frame locations to the exact original source set. Same-program classic/module and distinct-program dependency/caller frames now bind to their own original sources on real Launcher sockets.
    - [x] **C3.1.1.1** Verify real classic and module same-program nested frame stacks bind both ordered safe points to exact original UTF-8/UTF-16 spans, and make a wrong-but-receipted classic source a mandatory typed refusal. The existing real Launcher v36 test passes for both script kinds; classic now requires a second source ID and an `InvalidTarget` reply when that ID is substituted for the caller frame.
    - [x] **C3.1.1.2** Map and prove a genuinely multi-source module dependency nested frame without conflating entry/dependency program or source identities.
      - [x] **C3.1.1.2.1** Decide the paused cross-program stack, per-frame metadata/source receipt, grant, and stale/refusal contract before changing the public wire; keep v39 unchanged in this design leaf. PLAN.md fixes a separate linked-module pause/stack/coordinate family: entry and dependency programs remain distinct opaque identities, each frame carries its own installed generation, and each source must have its own same-stream metadata/source receipt under `OpaqueSafePointSpan`. Malformed/mismatched targets, missing grants/receipts, moved continuations, and stale realms have typed no-partial refusals; v39 is unchanged.
      - [x] **C3.1.1.2.2** Implement exact child/core and public mapping for the decided cross-program nested-frame source set, with unit and IPC denial coverage.
        - [x] **C3.1.1.2.2.1** Retain each paused frame's installed program generation in BlueJS, accept only a dependency safe point in the entry's live closed module graph, and prove dependency-child/entry-caller identity plus stale and wrong-generation rejection without IPC changes. Native debugger tests prove distinct frame generations and refusal of wrong serials, stale generations, unrelated graph members, and the old same-program stack route; private/public v39 remain unchanged.
        - [x] **C3.1.1.2.2.2** Add a complete private page-host linked-module pause/stack/resume and per-frame-source route, with child tests and a private protocol bump only when the route is complete.
          - [x] **C3.1.1.2.2.2.1** Bind the native linked pause to a host-neutral page realm and exact entry/dependency handles, retain a distinct linked frame, and prove stack/resume and stale/cross-realm denial without IPC changes. The host-neutral runtime rejects absent/unreachable dependencies, moved or wrong linked frames, cross-realm reads, and navigation-stale handles while preserving the separate same-program API.
          - [x] **C3.1.1.2.2.2.2** Retain distinct entry/dependency child programs and complete source/metadata mapping in the page host's linked pause state, without changing the private wire yet.
            - [x] **C3.1.1.2.2.2.2.1** Add child-local linked-module arm/pause/stack/resume scheduling, preserve both exact child program identities, and refuse stale/wrong-frame state; keep v39 unchanged. A child module test proves the real dependency call, distinct entry/dependency stack programs, denial of wrong entry or document, old v39 state refusal, exact linked resume, and stale frame expiry after return.
            - [x] **C3.1.1.2.2.2.2.2** Map both retained linked frames to their independent child metadata/source attachments and reject a swapped or unbound source without a partial result; then check off C3.1.1.2.2.2.2. The child rereads the complete live stack and validates each frame against its own metadata and source; tests prove distinct provenance, swapped metadata refusal, either unbound source refusal, and mismatched-stack refusal.
          - [x] **C3.1.1.2.2.2.3** Add complete private linked arm/stack/resume and per-frame-source requests/replies, child denial tests, and bump the private protocol only when all routes are complete; then check off C3.1.1.2.2.2. Private v40 exposes a distinct linked state/frame/stack family, exact arm prevalidation, and all-or-nothing per-frame span mapping; 111 IPC tests and focused child tests cover round trips and denials while public v39 remains unchanged.
        - [x] **C3.1.1.2.2.3** Remint linked frames and per-program safe points in core, add the complete public linked-module request/reply family behind the existing span grant and independent per-frame receipts, bump public debugger protocol only with the route, and prove IPC/core denial and exact-span behavior; then check off C3.1.1.2.2.
          - [x] **C3.1.1.2.2.3.1** Add a strict core-side child adapter for the complete private linked arm/state/stack/span/resume family, retaining both child program identities and rejecting malformed or moved private replies; keep public v39 unchanged. Core transport exposes each private request and a strict adapter checks full echoes, generation-bound frame shape, complete stack, both source IDs and spans, and exact resume; fake-child tests reject forged/moved/partial replies.
          - [x] **C3.1.1.2.2.3.2** Remint opaque linked frames and per-frame safe points in core with exact live document/program identity, and stage independent metadata/source receipt plus span-grant checks without public wire changes. Core retains a complete child-first stack, remints two distinct process-bound frame handles, maps each safe point to its own public program, and denies unauthorized/swapped/missing metadata or source access before the child; document replacement and program rediscovery invalidate the association.
          - [x] **C3.1.1.2.2.3.3** Add the complete public linked request/reply family, enforce the existing span grant and both independent receipts on every coordinate read, bump public debugger protocol only with the route, and prove real IPC/core no-partial denials and exact spans; then check off C3.1.1.2.2.3 and C3.1.1.2.2.
            - [x] **C3.1.1.2.2.3.3.1** Define source-free public linked frame/stack and receipt-bound two-source coordinate data shapes with strict per-frame validation; add no callable public route and keep v39. Fixed two-frame shapes reject swapped roles, duplicate frame handles, cross-program source attachments, and reordered spans; IPC round-trip and workspace Clippy pass.
            - [x] **C3.1.1.2.2.3.3.2** Connect complete core-facing linked arm/state/stack/resume and coordinate operations to the private child adapter, retaining per-frame public identities without public wire exposure.
              - [x] **C3.1.1.2.2.3.3.2.1** Add the core-facing linked model, distinct capability probe, and exact linked arm operation with both live public program identities; no public wire exposure. Core checks exact live entry/dependency mappings and a verified dependency safe point before the private graph-validation arm; fake-child tests cover valid, same-program, stale-document, and forged-acknowledgement paths.
              - [x] **C3.1.1.2.2.3.3.2.2** Expose paused/resuming state, complete stack, exact resume, and grant/receipt-bound two-source spans through the core-facing trait; then check off C3.1.1.2.2.3.3.2.
                - [x] **C3.1.1.2.2.3.3.2.2.1** Expose exact pending/paused/resuming/completed linked state and complete two-frame stack with stable core identities and private state/stack revalidation. Core accepts only exact private lifecycle replies, captures the complete child-first stack before publishing a pause, preserves handles across scope-budget changes, and rejects moved or unregistered frames without partial state.
                - [x] **C3.1.1.2.2.3.3.2.2.2** Expose exact linked resume and grant/receipt-bound two-source spans through the core-facing trait; then check off C3.1.1.2.2.3.3.2.2 and C3.1.1.2.2.3.3.2. Core revalidates both live program mappings before resuming, demands the complete previously returned public stack for span reads, checks both authorization/receipt sets and metadata attachments before child access, and relies on the private child to atomically re-read the full paused stack and both original spans.
            - [x] **C3.1.1.2.2.3.3.3** Add complete public linked requests/replies and dispatcher grant/receipt checks, real IPC/core no-partial tests and exact-span coverage, then bump public debugger protocol only with the complete route and check off the parent items.
              - [x] **C3.1.1.2.2.3.3.3.1** Validate the public linked-arm target and pending/paused/resuming/completed lifecycle state shapes without a callable route; keep v39. Wire data shapes reject same-program/root/cross-realm arms and malformed paused or resuming states; IPC serialization and workspace Clippy pass.
              - [x] **C3.1.1.2.2.3.3.3.2** Add the complete linked request/reply family, core dispatcher authorization and receipt checks, real IPC/core exact-span and no-partial tests, then bump public protocol only with the finished route and check off parent items. Public v40 has all five operations, two per-program receipt checks, real socket round trips, and core/child exact-span and no-partial denial tests.
      - [x] **C3.1.1.2.3** Prove entry/dependency original spans, distinct source IDs, swapped/unreceipted source refusal, and stale graph expiry on real Launcher-supervised sockets; then check off C3.1.1.2 and C3.1.1. The public socket test loads a manifest-authorized two-file BlueTS graph through HTTP, checks exact original slices and UTF-16 columns for both frames, refuses a missing second receipt and swapped handles, and rejects old graph targets after HTTP reload.
  - [x] **C3.1.2** Map breakpoints and symbols for module execution control. Entry root and linked dependency source/symbol workflows now use separate receipt-bound original positions and exact safe-point spans, generation/graph-checked arms, and real Launcher public-socket denial/stale tests.
    - [x] **C3.1.2.1** Decide the exact entry/dependency program, source/symbol receipt, and safe-point arm contract for module execution control before changing implementation; preserve public debugger v40 in this design leaf. PLAN.md fixes per-program metadata/source/symbol receipt identity, explicit unbound resolution, distinct root-versus-linked arm ownership, and arm-time generation/graph revalidation; whether a new atomic source-arm request is necessary stays with the implementation leaf.
    - [x] **C3.1.2.2** Prove existing source-position resolution and symbol locations bind to their own original entry/dependency source and exact verified safe points; preserve explicit unbound results and reject cross-program, swapped, stale, and missing-receipt targets. A bridge regression found that the retained resolver indexed only root provenance, so a dependency function's original position selected the root; it now also consults the already-verified nested safe-point map, preferring the most specific overlapping span and child entry. The real Launcher test verifies both programs' original declaration ranges and bound safe points, explicit source-end unbound results, absent source/symbol receipts, cross-program symbol/source refusal, and reload-stale targets. Public debugger remains v40.
    - [x] **C3.1.2.3** Connect the verified source/symbol mapping to module-root and linked-dependency breakpoint control, with atomic revalidation and no program/source aliasing.
      - [x] **C3.1.2.3.1** Prove a separately receipted original module-entry root position atomically arms only that entry's pending root frame; unbound or dependency child positions must refuse without starting the declaration. A real Launcher public-socket module test verifies missing source receipt refusal, child-entry and source-end refusal while the module stays pending, exact root safe-point arm, pause, resume, and completion. The existing atomic source-arm route serves classic and module roots without a protocol bump.
      - [x] **C3.1.2.3.2** Connect a receipted dependency source position to the distinct live entry's linked arm, rechecking the exact generation, verified child safe point, and closed graph at arm time; add core/child denial coverage and change the public wire only if the existing two-step route cannot preserve the contract. The real Launcher socket first resolves the dependency's own receipted original position to its verified child safe point, then arms the distinct entry; same-program, wrong-generation, and moved-pause arms refuse, while the existing child graph validator remains the final authority. Immutable generation-bound source maps and arm-time safe-point/graph checks preserve this two-step composition without a v41 wire.
      - [x] **C3.1.2.3.3** Compose a separately receipted module symbol declaration range with source resolution and the appropriate root/linked control route; preserve explicit unbound/type-only results and no cross-program aliasing, then check off C3.1.2.3.
        - [x] **C3.1.2.3.3.1** Decide the separately granted symbol-kind and declaration-location evidence required before a symbol can be treated as a candidate executable breakpoint; specify type-only/import/unbound refusal without changing debugger v40. PLAN.md requires independent display, location, source-breakpoint, and exact safe-point-span grants/receipts; only `Variable` or `Function` with a same-source bound span inside the declaration may compose with root/linked arm. `Interface`, `TypeAlias`, `Import`, unbound, and later non-overlapping spans refuse. No new wire authority is added.
        - [x] **C3.1.2.3.3.2** Prove a receipted executable entry-module symbol resolves its original declaration position to the exact root safe point and arms that module only; type-only and unbound symbols must not arm. A v40 data-only helper accepts only executable symbol kinds with same-program/source, declaration-start binding, and separately receipted exact span contained in the declaration; IPC tests reject type-only/import, unbound, moved, and wrong-source candidates. A real Launcher module test reads `rootValue` and `Shape` displays/locations under separate grants, binds only the variable's original declaration to its root span, leaves the type-only candidate and unbound candidate unarmed, then pauses/resumes the exact module root. All 114 IPC tests and workspace Clippy pass.
        - [x] **C3.1.2.3.3.3** Prove a receipted dependency function symbol resolves to its own verified child entry and arms only the distinct linked entry, with swapped/cross-program and stale refusal; then check off C3.1.2.3.3 and C3.1.2.3. The real Launcher test now composes the dependency's `Function` display, original declaration range, source binding, and exact contained child span before arming its distinct entry. Same-program/wrong-generation/moved arms, cross-program symbol/source pairing, and stale reload targets refuse without a partial source or frame. No new wire is needed.
    - [x] **C3.1.2.4** Prove module entry/dependency source and symbol breakpoint behavior, typed denials, and stale expiry through real Launcher-supervised public sockets; then check off C3.1.2. The entry test proves granted root-variable control, type-only/unbound refusal, and no-grant or fresh-stream no-receipt denials while the module remains live; the two-file test proves dependency function linked control, cross-program/moved denials, and both programs' source/symbol expiry after HTTP reload. Both focused real-socket tests and workspace Clippy pass.
  - [ ] **C3.1.3** Show static types separately from runtime values in every relevant debugger reply.
    - [x] **C3.1.3.1** Decide the checked compiler-symbol to active BlueJS lexical-slot relation, per-frame/program generation identity, and separate static/runtime grant and pause-receipt contract; keep debugger v40 unchanged in this design leaf. PLAN.md rules out name/source-span/runtime-shape guesses, keeps `Value` independent, and requires a new separately granted static-only scope relation over a same-stream Scopes receipt and internally checked pause incarnation. Missing, ambiguous, erased, moved, or stale mappings refuse rather than inventing a runtime type proof.
    - [ ] **C3.1.3.2** Retain only verified, generation-bound BlueTS symbol/type to BlueJS lexical-slot provenance for supported declarations in the direct bridge and child; reject unbound, shadowed, erased, and cross-program guesses with unit tests, without public wire changes.
      - [x] **C3.1.3.2.1** Retain compiler-owned root declaration to lexical-slot evidence for supported single-identifier variables and functions; explicitly leave other root statements unbound, with BlueJS unit tests. BlueJS records root-scope slots in statement order, leaves blocks/expressions/destructuring/multiple declarators unbound, and exposes duplicate `var` slot collisions for later join refusal. BlueJS 579 library tests, bridge 97 tests, focused Clippy, and formatting pass; no public wire change.
      - [x] **C3.1.3.2.2** Prove BlueTS has no compiler-minted SymbolId for function parameters or nested/local declarations, and that captured bindings do not appear as active child `Scopes` slots; explicitly exclude both from the first static-scope join with focused tests. BlueTS debug-info and real paused BlueJS child-snapshot tests pass; first join is root-code-unit-only, and a still-active parent frame requires its own exact receipt.
      - [ ] **C3.1.3.2.3** Join checked top-level BlueTS symbols/types to exact structural BlueJS root declaration slots and installed code-unit generation in the direct bridge; reject ambiguous, erased, mismatched, and cross-program joins in bridge tests.
      - [ ] **C3.1.3.2.4** Retain and revalidate that joined slot map in the child against the live program generation, with no-partial stale/moved/forged denials and no public wire changes.
    - [ ] **C3.1.3.3** Add a strict child/core static scope-symbol/type relation for an exact paused slot, never a runtime value or type display, with no-partial stale/moved/forged child denials; split the private route before implementation if needed.
    - [ ] **C3.1.3.4** Add an independently granted public static scope relation request/reply with exact Scopes and metadata/type receipts; retain the existing separate bounded `Value` reply, bump public protocol only when the complete route and IPC/core denials pass.
    - [ ] **C3.1.3.5** Prove a static type display and independently authorized runtime preview remain distinct, correctly matched to the same paused classic/module/linked slot, and expire after step/reload/cutover on real Launcher sockets; then check off C3.1.3 and C3.1.
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
