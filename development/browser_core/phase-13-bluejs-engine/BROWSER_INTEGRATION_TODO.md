# BlueJS browser-integration TODO

BlueJS is a near-complete ECMAScript engine (Test262: 102,921 / 102,926 modes
pass). What keeps it from running real web pages is the **browser host
layer** around it. This list was compiled from the code and design docs on
2026-10-14 by inspection; it was not validated by running real pages, so items
marked *verify* need a check before they are scheduled.

Related design docs: [`PLAN.md`](PLAN.md), [`DOM_EVENT_BINDINGS.md`](DOM_EVENT_BINDINGS.md),
[`../research/js-bytecode-eventloop.md`](../research/js-bytecode-eventloop.md),
[`../phase-17-automation-devtools-and-ajax/PLAN.md`](../phase-17-automation-devtools-and-ajax/PLAN.md).

Priority: **P0** blocks everything else, **P1** needed for ordinary sites,
**P2** compatibility/polish.

**Baseline.** This list describes `main` (the Test262 line of PR #25). The
product line (`feature/phase-7-6-8-9-followup`, carried by
`feature/browsable-web-mvp`) already has host objects `main` lacks: `URL`,
`AbortController`/`AbortSignal`, `Blob`/`File`/`FileReader`, `ReadableStream`,
`EventTarget`/`CustomEvent`, `DOMException`, three owner-selected DOM profiles
and a bounded browser-task queue. Items marked **(product line)** are done
there and become done here when the two lines meet. Do not re-implement them.

**Each item needs an acceptance condition** before it is scheduled, in the
sense of the Phase 13 Definition of Done: a test that fails today through the
real public interface, not a checked box. Section I lists the shared ones.

**Layer.** *Engine* = mechanism in `backend/bluejs` (a native-function kind,
a task queue, rooting). *Host* = policy owned by `backend/core` / launcher
(origin, gatekeeper, cookies, which profile installs it). A global existing in
the engine never makes it visible to a page: the owner-selected profile does.

**Integration progress (2026-10-15).** This checkout has merged the product
and BlueTS lines. The baseline descriptions below remain the original gap
inventory; the dated milestone records identify what has actually been
implemented and verified on `feature/bluejs-browser-integration-gaps`.

## A. Event loop and scheduling (P0)

The integrated branch has host-owned Promise checkpoints and session-driven
timers and a separate render entry for animation frames in the owner-selected
File DOM profile. General task-source ordering and complete queue/byte
accounting remain open; M1d now bounds the VM's document-owned scheduling
work and exposes its retained payload ledger.

- [ ] Macrotask queue that drains microtasks after every task (per
      `research/js-bytecode-eventloop.md`)
- [x] **M1a: bounded host microtask checkpoints.** Both page executors finish
      each completed script turn, including throws and completed debugger
      declarations, before executing the next declaration. A throwing job
      does not skip later jobs. Script/browser/timer checkpoints stop after
      1,024 jobs; the existing native-event checkpoint keeps its 256-job cap.
      Page-module top-level-await and page-created ShadowRealm drains also
      obey the bound. See the
      milestone record below.
- [x] **M1b: `setTimeout` / `clearTimeout` / `setInterval` / `clearInterval`.**
      Installed only by the owner-selected File DOM profile, with matching
      `core-script-dom-timers-v11` typings. Callable callbacks remain in the
      realm; string evaluation is deliberately unsupported. Signed Web IDL
      delay/ID conversion and the nested 4 ms floor are covered.
- [x] **M1b: `queueMicrotask`.** Uses the realm job queue and the bounded,
      exception-tolerant host checkpoint; absent from older page profiles.
- [x] **M1b: timer render-pass entry.** Core's ordinary session tick runs the
      child's due callbacks and checkpoints before publishing dirty layout/
      paint. Real JavaScript and BlueTS counter pages prove three unsolicited,
      distinct frames and matching Representation generations/text.
- [x] **M1c: `requestAnimationFrame` / `cancelAnimationFrame`.** The exact
      `core-script-dom-animation-frame-v12` artifact adds both callable APIs.
      Core's software render cadence drives a separate, document-bound entry
      before dirty frame publication. Callback snapshots preserve order,
      cancellation and one shared timestamp; newly registered callbacks wait
      for another frame. Each callback and render exit checkpoint microtasks.
- [ ] `MessageChannel` / `postMessage`
- [ ] Timers and queued tasks are dropped on navigation (old document
      generation), bounded per tab, and accounted to the initiating tab
      — timer/frame lifetime and count bounds are covered by M1b/M1c; complete
      queued-task and host-byte accounting is still required.
- [x] **M1b: core / child timer ownership.** The child realm roots callbacks
      and arguments; the host injects a document-local monotonic clock. Core
      transports only exact document identities and relative wake-up delays.
- [ ] Promise/task hooks that Phase 17's Fetch/XHR needs
- [ ] Task-source ordering between timers, network completions and UI events
      (one bounded queue today, no source priority); decide before `fetch`
- [ ] Per-document caps on pending timers, queued tasks, queued microtasks and
      host-held bytes, so a runaway page degrades only itself
- [x] **M1d: document scheduling admission.** Page VMs and their ShadowRealms
      share a 1,024-job, 1,024-waiting-reaction/request and 8 MiB owned-payload
      budget. Promise settlement, thenables, async functions/generators,
      module await and finalizer jobs use one admission path. Resource
      exhaustion is an uncatchable document failure; navigation replaces it.
      The public page runtime exposes counts and bytes. M1e propagates these
      bytes to Core; complete task-source/host-buffer accounting remains in
      the unchecked item above.
- [x] **M1e: Core scheduling-payload accounting.** Private v59 realm stats
      and task/frame replies carry source-free owned scheduling bytes. Core
      charges the initiating document's tab cache, refreshes after successful
      task/render entries and removes the charge on replacement/close or
      realm revocation. Excess scheduling bytes and invalid combined totals
      fail the existing exact-document containment path without replay.
- [x] **M1g: document browser-task admission and cleanup scheduling.** A
      shared 64-task cap includes browser tasks and pending registry cleanup
      tasks throughout the document's ShadowRealms. Public scheduling stats
      expose the aggregate pending count. Cleanup has its own FIFO task
      source and alternates with ready browser work within a bounded turn;
      general timer/network/UI source ordering remains open.
- [x] **M1b: timer wake-up.** `DragTasksFlushed.next_wake_in_ms` carries the
      next runnable delay. Core validates its identity/range, clears it on
      replacement/close, and shortens its ordinary polling timeout for an
      earlier timer. Paused debugger realms retain timers without busy wakes.
      M1c adds frame wake-ups to that deadline selection; repeated task polls
      preserve an already requested frame's deadline, and paused/empty frame
      queues clear it.
- [ ] Microtask checkpoint also runs after every DOM event dispatch, not only
      after browser-task turns (*verify*)
- [x] **M1b: reconcile the parked timer slice.** Retained the public
      page-runtime host-supplied clock mechanism, including explicit manual
      time for deterministic tests. Adopted the parked slice's 32-argument
      bound and nested-delay behavior, plus a 64-callback turn bound. The
      separate `wip/bluejs-web-timers` implementation is not merged wholesale;
      pending host-byte/queue budgets remain in the open item above.

## B. Promise, GC and error reporting (P0/P1)

Spec shape is done: `PromiseCapability` records, `WeakRef`,
`FinalizationRegistry` (page cleanup runs as a queued engine task after collection).
M1f replaces permanent Promise roots with live-owner tracing and explicit
ownership for queued/running jobs, module work and host waits.

- [x] **M1f: Promise collectibility (P1).** Settled Promises, result/reaction
      cycles and orphaned async invocations can be collected; a retained async
      invocation keeps its suspended frame and awaited Promise alive. The
      public page timer regression completes 4,000 polls with a 1 MiB heap.
      Successful host checkpoints collect under scheduling-payload pressure
      so dead native string payloads do not accumulate to the 8 MiB cap.
      The 12 lifecycle cases and four public page/cleanup cases cover both
      collectibility and live work. Acceptance also requires the unchanged
      full Test262 inventory; see the milestone record for final gates.
- [x] **M1f: awaited `AsyncIteratorClose`.** Dedicated loop-finalizer opcodes
      await `return()` exactly once, validate its result and preserve the
      incoming throw over a return failure. Stream cancellation remains
      pending until its Promise settles, including major-GC pressure. Stream
      iteration and tee regressions cover the previously dropped behavior.

- [ ] Host unhandled-rejection tracking (`HostPromiseRejectionTracker`) and
      the `unhandledrejection` / `rejectionhandled` events
- [x] **M1g: event-loop FinalizationRegistry cleanup.** Successful page host
      checkpoints discover GC-cleared cells and queue engine tasks, including
      collections during callbacks. Registry/cell ownership survives until
      delivery, so unregister still works before cleanup. One cell per task
      preserves the turn's callback bound and schedules remaining cells in
      later tasks; microtasks finish before the next cleanup callback.
- [ ] Error reporting for uncaught exceptions in tasks (`window.onerror`
      shape), without aborting `core`'s session loop
- [x] **M1f: long-lived timer GC behavior.** Public page-runtime fixtures
      complete 4,000 Promise polls under a 1 MiB managed heap and 64 large
      native-string polls under the existing scheduling cap. These verify
      bounded retention between callbacks; broader host/RSS accounting remains
      in A's open resource item.

## C. Web platform globals (P1)

On `main`, none of these were found in production code (only `globalThis`
exists). Several exist on the product line, marked below.

- [ ] `window` / `self` / `top` / `parent` aliasing of the global
- [ ] `console` (log/warn/error/...) routed to the devtools/MCP surface
- [ ] `location`, `history`, `navigator`
- [ ] `URL` **(product line)**; `URLSearchParams` absent on both
- [ ] `TextEncoder` / `TextDecoder`, `atob` / `btoa`
- [ ] `structuredClone`, `performance.now`
- [ ] `AbortController` / `AbortSignal` **(product line)**
- [ ] `localStorage` / `sessionStorage` (policy + persistence design first)
- [ ] `EventTarget` / `Event` / `CustomEvent` as general objects
      **(product line)**
- [ ] `Blob` / `File` / `FileReader`, `ReadableStream`, `DOMException`
      **(product line)**
- [ ] `WritableStream` / `TransformStream` / `CompressionStream` (absent on
      both; needed by `fetch` request and response bodies)
- [ ] `crypto.getRandomValues` / `randomUUID`; entropy and clock injectable
      so tests are deterministic
- [ ] `reportError` and the global error-event shape (with B)
- [ ] A global-inventory test: execute `typeof X` for every name in this
      section in a page-host VM and pin the exact set, so this list cannot
      drift from the code again

## D. DOM bindings (P1)

Bound today: `document.getElementById`, `createElement`, `createTextNode`,
`textContent`, `appendChild`, and `click` add/removeEventListener.

- [ ] `querySelector` / `querySelectorAll`, `getElementsBy*`
- [ ] Attributes (`getAttribute`, `setAttribute`, `dataset`), `id`,
      `className`, `classList`
- [ ] Inline `style` and `getComputedStyle`
- [ ] Tree mutation: `insertBefore`, `removeChild`, `replaceChild`, `remove`,
      `innerHTML` / `outerHTML`, `cloneNode`
- [ ] Traversal: `parentNode`, `children`, `firstChild`, `nextSibling`, ...
- [ ] Events: `input`, `change`, `submit`, keyboard, focus, capture/bubble,
      `stopPropagation`, `DOMContentLoaded`, `load`
- [ ] Form state: `value`, `checked`, `FormData`
- [ ] Layout reads: `getBoundingClientRect`, `offset*`, `scroll*`
- [ ] `MutationObserver`, `IntersectionObserver`, `ResizeObserver`
- [ ] Generated `lib.dom.d.ts` kept in lockstep with each installed binding

## E. Networking (P1, Phase 17)

- [ ] `fetch`, `Request`, `Response`, `Headers` over `core`'s request service
- [ ] `XMLHttpRequest` including event ordering
- [ ] `FormData`; urlencoded and JSON request bodies
- [ ] CORS (fail closed on unsupported policy), cache, cancellation via
      `AbortSignal`
- [ ] **Cookie jar** in the request service: `Secure`, `HttpOnly`,
      `SameSite`, `Set-Cookie` round trip on form POST. Nothing exists today;
      without it there is no login or session.
- [ ] **Gatekeeper review for script-initiated requests**: the same two-stage,
      fail-closed review as navigation; a script request is a typed request
      to `blueice-net`, never a raw socket
- [ ] **Private-network URL policy** (loopback, link-local, RFC 1918): an
      open gap in the gatekeeper rule base, and a precondition for letting a
      page issue requests at all
- [ ] Response bodies are page-derived data: untrusted-framed on the MCP
      surface, byte and time limits enforced, a request-lifecycle record
      readable like a download's
- [ ] Server-Sent Events; WebSocket (Phase 20)
- [ ] Depends on section A

## F. Page script loading and integration (P0/P1)

- [x] Wire `blueice_ipc::script` into a real BlueJS host process end to end.
      M1b's owner-selected File DOM fixtures use the supervised child and
      Core's pumped exact-document DOM requests; broader APIs remain scoped
      by their own open items.
- [x] End-to-end smoke test: real JavaScript and BlueTS `<script>` fixtures
      through HTTP navigation, the isolated host, DOM mutation and rendering.
- [ ] *Verify*: `<script src>`, `defer`/`async`, `type=module`, inline
      handlers (`onclick=`), execution order vs. parsing
- [ ] `bluejs` shell (REPL + batch) and the full Node differential harness
- [ ] `bluejs_run` / `bluejs_analyze` MCP tools (with Phase 12)
- [ ] AI-facing capability summary and Phase 7 gatekeeper script hooks
- [ ] **Default-on script execution** is a reviewed decision, not a flag
      flip: today scripts run only behind `--inline-bluejs` or the page-host
      options, so a normal launcher session never executes one. The core
      selects the supported profile; the page never does.
- [x] A script DOM change publishes a frame in the same render pass, with
      the same `generation` on the `Representation`. M1b's timer counter
      fixtures verify the integrated `a54ddceea` frame-push behavior through
      the real Core/child path without a frontend request between ticks.

## G. Engine limitations (P2)

- [ ] No JIT and no performance measurements against Node/Chromium
- [ ] `regress` deviations: `(?i:...)` modifier groups, others listed in
      `TEST262_ANALYSIS_REPORT.md`
- [ ] Intl formatter constructors still partly open (`INTL_CONFORMANCE.md`)
- [ ] Module static-error discovery uses unordered `HashMap`/`HashSet`, so
      which `SyntaxError` a cyclic graph reports is nondeterministic
- [ ] Parser/compiler nesting and native data recursion are not covered by
      the native-stack call-depth guard
- [ ] Windows native-stack query branch compiled but never run; 1 MiB
      main-thread stack untested
- [ ] General reverse-buffer-mirror for cross-realm TypedArrays not built

## H. BlueTS / BlueTSC dependencies on BlueJS

BlueTSC itself is a Rust compiler and does not run on BlueJS; BlueJS only
executes its output, through `backend/bluets-bluejs` (the sole crate that
depends on both). So most remaining BlueTS work is compiler-side
([`../phase-18-bluets/TODO.md`](../phase-18-bluets/TODO.md): the open leaves
are class checking/emit, enums, namespaces, JSX, decorators, CommonJS and
package resolution). The items below are the places where a BlueTS feature is
blocked or constrained by a BlueJS or browser-host gap. They were compiled
on 2026-10-14 from `INTEGRATION_CONTRACT.md`, the Phase 18 TODO and the
bridge's lowering code, not by running BlueTS pages; *verify* marks inference.

### Blocked by the host layer gaps above

- [ ] **Typed host API stays tiny.** `lib.blueice.d.ts` is generated from
      implemented bindings only, so typed BlueTS code can reach exactly
      `document.getElementById`, `textContent`, and `click` listeners until
      sections C and D grow. Every binding added there needs its typings
      entry in the same change (the "absent API fails typed use" rule).
- [ ] **Typed asynchronous web APIs remain incomplete.** M1b gives the File
      DOM profile typed timers and `queueMicrotask`; network waits still need
      section E, and general page-module host-wait scheduling needs verification.
- [ ] **No `console` or `fetch`** for typed page code. `setTimeout` and the
      other M1b scheduling functions are now in the exact File DOM artifact.
- [ ] **Uncaught errors and rejections** cannot be mapped back to BlueTS
      original positions until the host reports them (section B); today
      only debugger pauses use the safe-point map (*verify* for thrown
      exceptions and `Error.stack`).

### BlueJS capabilities BlueTS should use or still needs

- [x] **Merged host-profile compatibility (M0a).** Interface inheritance
      compares complete method overload sets, preserving the generic fallback
      alongside typed event overloads. Nested functions and keyword-named
      members such as stream iterator `.return()` lower through the native
      BlueJS AST again; the named-function parser retains its 64-level limit.
      Acceptance: the unchanged live stream-profile preparation fixture,
      native closure/event/iterator execution tests, and 17 cases recorded
      with TypeScript 5.9.3. See the milestone record.
- [ ] **Native decorators.** BlueJS implements Stage 3 decorators
      (`Symbol.metadata`, `addInitializer`, auto-accessors), which matches
      TypeScript 5.x standard decorators; J.5.3 should lower to them
      directly rather than emitting a helper. Legacy
      `experimentalDecorators` still need an emitted `__decorate` helper.
- [ ] **Class lowering to BlueJS class AST (J.3.1.5).** The engine has full
      ClassDefinitionEvaluation (private names, static blocks, derived
      constructors), so no engine gap is expected; the direct bridge today
      rejects class members with "class members and runtime semantics are not
      installed yet" - that is bridge work.
- [ ] **CommonJS and `import()`.** BlueJS has ESM, dynamic-import jobs,
      `import defer` and `import.source()`, but no `require` / `module` /
      `exports` host objects and no page-level `HostLoadImportedModule`
      resolver wiring. J.4.1 must either emit a helper or the host must
      provide them; `import.meta` properties are also host-defined
      (*verify* what the page runtime installs).
- [ ] **Runtime contract helper is string-only.** `runtime_helper_v1.mjs`
      validates reviewed primitive-string contracts. Strict boundaries over
      numbers, records and arrays (Phase 18 E) need a richer helper and a
      defined structured value format across the child-to-core boundary.
- [ ] **Source positions for runtime errors.** BlueTSC emits source maps and
      static debug metadata; BlueJS exposes bytecode safe points but a
      frame-position API for thrown errors is unconfirmed (*verify*).
- [ ] **Debugger coverage.** Classic-root and nested-frame pause/step/resume
      exist. Missing or unconfirmed for TypeScript sources: breakpoints
      set by line, conditional breakpoints, pause-on-exception, stepping
      across `await` inside ordinary async functions (Phase 17).
- [ ] **ES library typings vs. engine.** BlueJS implements ES2026, Intl and
      Temporal; TypeScript 5.9.3's bundled `lib.es*.d.ts` does not declare
      Temporal. Add a conformance check that every global BlueTS declares
      exists in BlueJS and, separately, decide how engine-only features
      (Temporal, `import defer`, Immutable ArrayBuffer) are typed.
- [ ] **Resource limits as TypeScript semantics.** Instruction fuel, managed
      heap budget and native-stack guard raise `RangeError`/limit errors
      that TypeScript code cannot model; document them in the BlueTS
      runtime contract.

### Suggested joint order

1. Event loop, timers and `console` (A, C) - unlocks typed async code.
2. Unhandled-rejection / uncaught-error reporting with source-map lookup
   (B + BlueTS provenance).
3. DOM growth (D) with typings generated in the same change.
4. Class, enum, namespace lowering (J.3) and native decorators (J.5.3).
5. CommonJS / `import()` host support (J.4) and the richer contract helper.

## Suggested order

1. A (event loop + timers) - everything async depends on it
2. `console` + unhandled-rejection reporting (B, C) - cheap, unblocks debugging
3. D (DOM surface) and C (globals) - needed to run ordinary page scripts
4. E (fetch/XHR) - needs A
5. F (end-to-end smoke test, shell) - validates the whole stack

## I. Ownership and acceptance

| Area | Layer | Acceptance (must fail today, through the real interface) |
|---|---|---|
| Host microtask checkpoints | Engine mechanism, host script cleanup | Two page script declarations observe the first declaration's Promise work before the second starts, including after a throw and with debugger scheduling; runaway jobs return a resource failure through the public page runtime |
| Timers, `queueMicrotask`, `requestAnimationFrame` | Engine mechanism, host clock | A counter page updating by `setInterval` yields N frames with strictly increasing `generation`, and each matching `Representation` agrees |
| Promise collectibility | Engine | Bounded managed heap over a long poll loop; `promise_gc_lifecycle`; full Test262 inventory unchanged |
| Platform globals | Engine, installed by profile | The global-inventory test in C |
| DOM read/write, events | Host (page host profiles) | Fetch JSON, build a list with `innerHTML`, click and type to update it: frame pixels, `get_page_representation` and an MCP client all agree |
| `fetch` / XHR / cookies | Host, over `blueice-net` + gatekeeper | A first-party fixture GET/POST with a `Set-Cookie` round trip; a request to a private address is refused with a visible diagnostic |
| Default-on scripts | Host | The same fixture works from an ordinary launcher session with no extra flags |

Mapping to the product phase: Phase 27 (`phase-27-browsable-web-mvp/PLAN.md`)
M1 is sections A and B here, M2 is D, M3 is E, M4 is E's cookie jar and F's
default-on decision, M5 is the acceptance fixture above. Section H is
unaffected by that order except that every binding added in C or D must update
the generated typings in the same change.

## Milestone record

### M1g — Document-owned FinalizationRegistry tasks (2026-10-15)

- Public red-first page tests exposed cleanup executing inside the Promise
  checkpoint, unregister losing already-extracted cells and ShadowRealm
  cleanup running synchronously before control returned to the page.
  Separate resource/turn regressions caught callbacks continuing with queued
  microtasks after exhaustion and an entire registry bypassing the turn's
  callback budget.
- Page VMs keep one pending task/root per registry and leave cells in the
  managed heap until delivery. Each task consumes one cleared cell; the
  remaining cells stay cancellable and can be queued again. Callback throws
  preserve remaining work and complete the normal checkpoint; resource
  failures stop immediately. Active holdings keep explicit GC roots and their
  native payload is charged while the callback/checkpoint executes.
- Cleanup tasks use a document-wide serial/FIFO across ShadowRealms. Browser
  turns traverse same-account realms without recursive Rust calls and refresh
  after callbacks create new realms. Ready ordinary work and cleanup alternate
  within the same finite task budget. Foreign-document records are excluded;
  debugger pauses preserve queued work and navigation drops the old tree.
- Added a shared 64-entry browser/cleanup-task count to scheduling admission
  and public snapshots. Existing browser producers reserve their count before
  publication. A GC-maintained ready-registry/cell index avoids scanning the
  full heap at each microtask and repeatedly shifting or scanning live cells
  while consuming cleanup. Collection removes dead registry index entries.
- References: [HTML cleanup tasks](https://html.spec.whatwg.org/multipage/webappapis.html#hostenqueuefinalizationregistrycleanupjob),
  [ECMAScript cleanup-cell selection](https://tc39.es/ecma262/multipage/executable-code-and-execution-contexts.html#sec-cleanup-finalization-registry).
  Standalone language VMs retain their existing manual job policy; installed
  v12 globals/typings and the private v59 wire schema remain unchanged.
- The real typed-page fixture exposed missing standard `WeakRef` and
  `FinalizationRegistry` declarations. Added original, edition-selected
  `ecma-weakrefs.v3.d.ts` and advanced the standard-library identity to v3;
  symbol targets are selected only from ES2023, following the
  [TypeScript edition split](https://github.com/microsoft/TypeScript/blob/v5.9.3/src/lib/es2023.collection.d.ts).
  Preserved prior declaration files and the owner-selected page profile.
  Direct lowering erases parser-recorded call/constructor type-argument
  ranges while retaining original runtime token positions. Typed cleanup,
  nested generic closers, invalid target/holding types and edition selection
  have focused regression coverage. Page fixtures use named callbacks in the
  current direct bridge subset.
- The unfiltered pinned Test262 inventory completed in 492.221 seconds:
  **102,921 / 102,926 modes**, with one pre-existing stale baseline entry and
  four excluded modes, **zero failures/timeouts**. All 102,926 mode paths,
  source hashes and dispositions match M1f; the inventory hash remains
  `331ed129027852eb95d1742dd0377027ed504e5fb70a499eadae3f143d709c2f`.
  The corpus, adapter and disposition policy are unchanged. See the checked-in
  [machine summary](test262-browser-finalization-macos-summary.json).
- Final macOS gates: **2,962 related tests passed**: 2,739 tests across the
  five affected libraries, 199 public runtime/allocation cases, 16 standard
  library/edition cases and eight rebuilt real Core/child fixtures. One
  existing launcher case and four optional compiler-oracle cases remain
  ignored. This includes 16 new public page cleanup cases, five new
  ownership/index contracts and all 41 cases in the three Promise allocation
  targets. Socket-dependent tests ran with local socket access.
- Real JavaScript and BlueTS pages each produced three distinct unsolicited
  frames with matching post-cleanup-microtask Representation text/generations.
  A 65-registry task overflow failed only its document, without replay, while
  another tab kept rendering and the failed tab accepted navigation. Existing
  timer, animation-frame and FileReader fixtures remain green. All-target
  Clippy for the five affected crates and workspace formatting passed.
- General task-source ordering, complete host-buffer/aggregate accounting and
  rejection/error events remain open. Full workspace and coverage gates are
  not claimed by M1g.

### M1f — Collectible Promise state and awaited iterator cleanup (2026-10-15)

- Red-first public evidence: four original Promise lifecycle cases failed
  under permanent rooting, and a 1 MiB page timer failed at poll 743. A
  stream's pending cancellation Promise also let `for await` leave early.
  Added ownership tests for retained/orphaned async invocations, a 4,000-poll
  page regression, native-string payload polling and exact one-job cleanup
  ordering. The latter verifies that closing adds one Await continuation
  without an extra `.then` reaction turn.
- Promise status/reaction records now contribute edges only when their heap
  owner is live. Suspended frames are traced through their awaited Promise;
  retained invocation/generator owners keep that Await alive. Queued and
  running jobs, outstanding module imports/evaluation and host async waits
  retain their explicit roots. Collection sweeps dead records/continuations
  and releases oversized Promise-map capacity. Active job roots survive
  dynamic-import evaluation resetting the ordinary operand stack.
- Allocation-pressure checks exposed missing prerequisite roots during
  async-call binding setup, AsyncFromSync wrapper allocation, resolving
  function creation and `Array.fromAsync` error/close processing. Added roots
  through publication and release on each exit. A throwing host timer no
  longer discards later wait completions or leaks their root registrations.
- Successful outer host checkpoints clear WeakRef keep-alive state and run
  major collection when this VM's scheduling payload reaches 2 MiB. This
  supplies pressure for native Promise strings outside managed heap bytes;
  the public payload fixture completes 64 polls of 400,000-character strings.
  Nested module/call contexts retain their current-job WeakRef guarantee.
  Existing 1,024-job/reaction and 8 MiB admission limits remain in force.
- Appended `AsyncIteratorClose`/`AsyncIteratorCloseFinish` bytecodes without
  changing existing opcode IDs. For-await loop finalizers retain the iterator
  across suspension, handle AsyncFromSync return semantics and preserve the
  incoming completion. A rejected `next()` does not spuriously call return;
  an incoming throw wins over a failing return after its Await completes.
  The v12 installed profile and private v59 protocol remain unchanged.
- Final macOS gates: 2,732 affected library tests passed (one existing
  launcher case ignored), 190 focused runtime/allocation regressions passed
  and six rebuilt real Core/child timer/frame/FileReader fixtures passed.
  All-target Clippy for the five affected crates and formatting passed.
- The unfiltered pinned Test262 inventory completed in 468.631 seconds:
  **102,921 passed / 102,926 scheduled**, with the same one `stale_corpus`
  and four `excluded` modes as the baseline, and zero failures/timeouts.
  Snapshot, group/feature counts and all path/mode/source hashes match the
  pinned inventory. See the
  [machine summary](test262-browser-integration-gc-macos-summary.json).
  The runner retains exit 1 for those five declared non-passing modes.
  The first run under concurrent compilation timed out in two `toSpliced`
  modes; both passed in isolation and in the final full run with unchanged
  deadlines, resource allowances, harnesses and fixture sources.
- The broader BlueJS sweep completed all 353 targets: 4,392 passed, four
  pre-fix allocation failures and four existing ignored cases. Those failures
  are fixed; all 41 cases in the three affected targets pass on re-run, with
  no additional failures in the completed sweep. The original sweep retains
  exit 101 for its pre-fix results. Full workspace and coverage gates are not
  claimed. Complete task-source/host-buffer accounting, rejection/error events
  and broader browser compatibility remain open.

### M1e — Scheduling payload in Core's per-tab ledger (2026-10-14)

- Added a read-only scheduling snapshot to public page realm stats. It
  includes the document's ShadowRealms and agrees with the mutable admission
  usage API without changing the account or exposing scheduled values.
- Advanced the private page-host protocol to v59. `PageHostRealmStats`
  requires `scheduling_payload_bytes`; task and frame acknowledgements carry
  the updated aggregate after callbacks/checkpoints. Connected peers still
  require the exact protocol version. The private byte envelope matches the
  VM's 8 MiB cap and checks static + deferred + scheduling sums with checked
  arithmetic; the established heap/bytecode ledgers retain their meanings.
- Core's retained page-cache total now includes scheduling ownership for
  the exact live tab/document. Valid task/frame replies refresh that field
  before publishing scheduling wake state. An absent realm, excess byte count
  or invalid combined total rejects the reply and uses existing realm-only
  revocation, clears its accounting/wakes and preserves the admission identity
  so the document scripts are not replayed. Replacement/close release the old
  generation's charge without changing another tab's.
- Red-first real-child evidence: a globally retained Promise with an 800 KB
  string previously contributed zero bytes to Core's tab ledger. The two-tab
  fixture now checks distinct 800/600 KB charges, replacement release and
  independent close. A real VM File DOM fixture verifies pending timer bytes
  in task acknowledgements and their release when a frame callback clears the
  timer. Core fixtures cover independent task/frame refresh and oversized
  acknowledgements; IPC tests cover mandatory stats fields, byte ceilings,
  combined-total overflow and scalar round trips over actual sockets.
- Final macOS verification: 2,696 affected library tests passed (one existing
  launcher case ignored), plus 12 scheduling-limit, 37 timer and 14 frame
  runtime tests. Rebuilt v59 Core/launcher binaries passed the five real
  timer/frame/two-tab fixtures and the real FileReader fixture; the real-child
  Core ledger regression also passed after its final assertion updates.
  All-target Clippy for the five crates and the revised launcher fixture,
  formatting and working/staged diff checks passed. Full workspace, coverage
  and unfiltered Test262 gates are not claimed. Broader host-buffer accounting,
  aggregate process policy, task-source ordering, Promise collectibility and
  error-event reporting remain open.

### M1d — Document-owned scheduling admission (2026-10-14)

- Added shared document admission for at most 1,024 queued microtasks and
  1,024 pending reactions, module-import waiters and async-generator requests.
  The page VM and its ShadowRealms contribute to one account; standalone
  ECMAScript VMs retain their host-neutral policy. Every Promise/thenable,
  async/await, async-generator, dynamic-import, finalizer and `queueMicrotask`
  job producer now uses the same gate. Async frames preflight their captured
  state before publication. The existing 1,024-timer, 64-frame and 64-browser-
  task bounds remain in force.
- Added an 8 MiB scheduling-payload cap and public page-runtime usage API.
  It counts owned container slot capacities, copied strings/Symbol
  descriptions/logical BigInt limbs, Promise record/status/reaction storage,
  timer arguments/roots, browser task strings, await frame buffers, import
  waiter storage and a module continuation's owned outer bytecode copy.
  Managed heap objects and shared installed code have separate ledgers.
  Allocator/control-table overhead, spare BigInt limb capacity and RSS are
  outside this metric; broader host buffers and Core tab-cache propagation
  remain follow-up work.
- Running jobs and timer/browser callbacks retain their payload charge.
  One-shot timer arguments move into the running task instead of being
  copied; interval argument copies must fit before allocation. Clear/fire,
  cancellation and empty queues update accounting and release capacity where
  the container no longer owns work. Parent usage is synchronized before
  entering a ShadowRealm so child admission cannot use stale parent charges.
- Scheduling exhaustion latches `SchedulingLimit` for the document. Page
  `try/catch`, Promise combinators and the ShadowRealm error membrane cannot
  turn it into successful execution. Later script/task/render entries refuse
  the failed document; replacement/close discards the account. Infallible
  Promise settlement continues internal bookkeeping without a Rust panic,
  while the execution boundary returns the resource failure.
- Red-first regressions established that six distinct producers and owned
  timer/Promise strings previously escaped pending-work/payload bounds.
  Twelve public page-runtime tests now cover eight producer forms, native
  combinator fanout, captured async arguments, reaction-to-job transfer,
  running interval/one-shot payloads, release, shared ShadowRealm admission,
  uncaught failure, exact 1,024-entry admission and fresh-document admission,
  plus a bare-VM control.
- The real HTTP/Core/supervised-child two-tab fixture overflows one timer's
  microtask admission even inside `try/catch`, preserves its last DOM mutation,
  and observes three matching unsolicited frames/Representations from the
  healthy tab. The failed tab subsequently navigates to a successor document.
- Final macOS verification: 2,691 affected library tests passed (one existing
  launcher case ignored), plus 12 scheduling-limit, 37 timer and 14 frame
  runtime tests. Rebuilt Core/launcher binaries passed the five real
  timer/frame/two-tab fixtures and the real FileReader fixture. All-target
  Clippy for BlueJS, bridge, Core, IPC and launcher passed with `-D warnings`;
  formatting and working/staged diff checks passed. Full workspace, coverage
  and unfiltered Test262 gates are not claimed. M1 remains in progress for
  task-source ordering, full host accounting, Promise collectibility and
  error-event reporting.

### M1c — Animation-frame render entry and continued frame delivery (2026-10-14)

- Added VM-rooted `requestAnimationFrame` / `cancelAnimationFrame` with a
  separate public page-runtime render entry. Ordinary browser/timer turns
  cannot invoke frame callbacks. One render opportunity snapshots handles in
  registration order, allows cancellation of later callbacks (including by a
  microtask), and uses a shared fractional document timestamp. Registrations
  made by callbacks or their jobs wait for the next opportunity. Callback and
  render-exit checkpoints are bounded and continue after page exceptions.
- Bounded the pending callback map at 64 entries and ran its complete
  snapshot, retaining roots through invocation and releasing them on cancel
  or completion. Navigation/close discard the map. Paused debugger realms
  retain callbacks without advertising runnable frame work. Required-argument
  function lengths, unsigned-long cancellation and finite monotonic host
  timestamps are checked at the public VM/page-runtime boundaries.
- Added the exact 47-entry `core-script-dom-animation-frame-v12` typing
  artifact as a superset of v11; older artifacts remain unchanged. The latest
  owner-selected File DOM installer and BlueTS admission use its matching
  inventory. Wrong callbacks/handles fail typed admission; old profiles remain
  statically and dynamically absent. No page selects its profile or clock.
- Advanced the private protocol to v58 with exact-document
  `RunAnimationFrame` / `AnimationFrameRendered` and a runnable-frame flag in
  task replies. Core owns a 16.667 ms software cadence (the research permits
  timer-driven render opportunities for the MVP), retains deadlines across
  ordinary task polls, and selects the earliest timer/frame wake. Its separate
  render call runs after task scheduling and before dirty frame publication,
  pumping exact-document DOM calls while the child executes. Invalid tab,
  generation or timer-delay replies revoke only the realm without replaying
  the document scripts. Native vsync and broader visibility policy remain
  later host work; this is not a guaranteed display-refresh-rate claim.
- Red-first acceptance: the real rAF fixture failed at page script execution
  before installation. JavaScript and BlueTS fixtures now update their counter
  through rAF and a queued microtask, producing distinct unsolicited frames
  and matching Representation generation/text. A separate continuous loop
  re-registers directly from its frame callback with no further timers or
  frontend work, proving that returned pending-frame state keeps scheduling.
  The fixture retains asynchronous frames while awaiting correlated replies.
- Final macOS verification: 1,191 BlueJS and 164 bridge library tests,
  14 frame-runtime tests, 37 timer tests, four frame/timer/iteration typing
  tests, Core's 815, IPC's 216 and launcher's 305 library tests passed (one
  launcher case ignored), plus four real timer/frame tests and the real
  FileReader fixture. The waiting-closure case checks that an actual collection
  occurred. Core/launcher binaries were rebuilt at private protocol v58 before
  real-process validation. All-target Clippy for the five affected crates
  passed with `-D warnings`; formatting and staged/working diff checks passed.
  Full workspace, coverage and unfiltered Test262 gates are not claimed.
- General task-source ordering, pending-task/job and host-byte budgets,
  Promise collectibility, error-event reporting and default-on execution are
  still required; M1 as a whole remains in progress.

### M1b — Session-driven page timers and frame acceptance (2026-10-14)

- Installed callable-only `setTimeout`, `setInterval`, their shared-ID clear
  functions and `queueMicrotask` in the latest owner-selected File DOM profile.
  The exact 45-entry `core-script-dom-timers-v11` artifact adds five globals
  without changing v1–v10 declaration bytes/inventories. Typed valid calls,
  incorrect callbacks/delays/IDs, old-profile absence and absent animation
  frames are checked at the public bridge; child inventory tests pin runtime
  presence and copied-snapshot absence. No page gains a profile selector.
- Retained deterministic explicit host time and added an injected monotonic
  clock sampled at scheduling and interval rearming. Callbacks and object
  arguments remain collector-rooted in their realm. Reconciled 32 extra
  arguments and the nested 4 ms floor; added a 64-callback turn limit retaining
  unfinished timers. The realm's existing 1,024-timer cap remains in force.
  Signed Web IDL long conversion handles delay/ID overflow and infinities;
  strict callbacks receive the realm global, and function lengths match
  required argument counts (the clear functions have an optional ID). Late
  intervals run once and rearm after the callback/checkpoint, respecting
  self-clear.
- Drove due timers after bounded browser tasks on Core's ordinary session
  tick, preserving checkpoint ownership and same-turn dirty frame publication.
  A paused debugger realm retains timers and advertises no runnable wake-up
  until it resumes. Resource failures remain contained by the existing
  exact-realm revocation path, which keeps Core alive without script replay.
- Advanced the private Core/child protocol to v57 for
  `DragTasksFlushed.next_wake_in_ms`. Only relative delays cross the boundary.
  Core validates tab/document identity and the signed-long delay range, samples
  its deadline before transport waits, discards wakes on replacement/close,
  and shortens its 25 ms service poll for earlier timers. Timer-free/paused
  replies clear wakes. Socket timeout updates retain best-effort disconnect
  handling; a deterministic EINVAL regression covers the new polling path.
- Acceptance was red first: the real counter page was rejected at execution,
  and five public VM cases failed for conversion, nesting, turn/argument bounds
  and callback `this`. Both real JavaScript and BlueTS counter pages now
  publish three different unsolicited RGBA frames with strictly increasing
  generation and exactly matching Representation generation/text. Self-clear
  stops later frames; navigation from a running interval leaves its successor
  quiet. The existing real selected-file/FileReader/stream fixture also passes.
- Verification on macOS: 1,191 BlueJS library tests, 37 `host_timers`, 164
  bridge library tests, three timer/iteration typing tests, 812 Core library
  tests, 215 IPC library tests and 305 launcher library tests passed (one
  launcher case ignored), plus two real timer tests and one real FileReader
  test. Rebuilt Core and launcher binaries before the final host run so both
  ends use protocol v57. Historical debugger wire tests now enforce their
  minimum revision rather than retaining an obsolete exact v53 assertion.
  All-target Clippy for the five affected crates passed with `-D warnings`;
  `cargo fmt --all -- --check` and `git diff --check` passed.
- `requestAnimationFrame`, general task-source ordering, pending-job/task and
  host-byte caps, Promise collectibility, error-event reporting and default-on
  execution remain open. This completes the timer/frame vertical slice, not
  M1 as a whole. The full workspace, coverage and unfiltered Test262 gates are
  not claimed by this increment.

### M1a — Host microtask checkpoint ownership and bounds (2026-10-14)

- Kept raw classic `execute_program` separate from host script cleanup. This
  preserves pending jobs and debugger ownership while the in-process and
  isolated page executors checkpoint between declarations, even after throws.
- Added public `run_microtask_checkpoint`, returning the count of uncaught
  job exceptions. All host checkpoints continue after page exceptions and
  return resource failures for unfinished chains. Error-event/console delivery
  is still section B work; the current hosts do not yet publish those counts.
- Enabled bounded implicit Promise drains only in page realms, including
  replacement realms and their ShadowRealms. Standalone ECMAScript/Test262 VMs
  retain their explicit drain behavior. Routed single page-module execution
  through the module-graph evaluator so top-level await uses its continuation machinery rather than
  reaching the synchronous interpreter's `unreachable!` branch.
- Acceptance regressions reproduced missing between-script cleanup in both
  hosts, throwing-job starvation, unbounded async-module graph draining, and
  the single-module top-level-await panic before the fixes.
- Verification: 29 `host_timers`, seven `page_runtime_coverage`, 25
  `web_file_api`, ten `web_node_events`, and 1,191 BlueJS library tests passed;
  both host cleanup regressions pass for normal/debugger scheduling and
  success/throw cases. Core's full library run passed 808 cases and failed its
  unchanged transport fixture (49 mocked replies for 50 requests). Launcher's
  run passed 304 cases, ignored one, and failed its unchanged BlueTS stream
  profile preparation fixture. These two integration-verification gaps remain
  open; this is not a green workspace-suite claim. Local socket tests require
  an environment that permits TCP/Unix socket bind.
- Final focused reruns passed all 29 host checkpoint/timer cases, 14
  ShadowRealm library cases and both host script-cleanup regressions.
  `cargo fmt --all -- --check` and Clippy for the affected crates' library/test
  targets with `-D warnings` passed.
- Next: reconcile the parked timer implementation's argument/nesting limits,
  install timers with a versioned generated typing profile, deliver them on
  the core session tick across the child boundary, propagate the next wake-up,
  and prove the counter-page frame/Representation acceptance in section I.
  `requestAnimationFrame`, task-source ordering, queue/byte caps and Promise
  collectibility remain required work; M1 as a whole is not complete.

### M0a — Restore merged host-profile compatibility (2026-10-14)

- Fixed the Core transport fixture's stale 49-reply count: it now services
  all 50 public debugger-wrapper requests, including the final pumped turn.
- Compared each inherited interface property against the complete declared
  overload set, once per property name. The fallback signature can satisfy
  the parent while specialized callbacks remain available for call checking.
  Lookup and relation checks retain their expansion budgets; incompatible
  overloads, returns, callback parameters and optional overrides remain errors.
- Recorded eight strict accept/reject cases from the locally available,
  version-verified TypeScript 5.9.3 compiler, and checked the public BlueTS
  compiler against those verdicts and TS2430 error codes.
- Fixed weak-record compatibility across inherited option dictionaries:
  overlap is tested against the complete record intersection rather than each
  parent separately, retaining every duplicated property constraint. Nine
  further TypeScript 5.9.3 verdict controls cover valid partial dictionaries,
  empty dictionaries, incompatible fields and missing overlap. This restores
  checked native `DragEvent` initialization without changing its declarations.
- Restored rejection of prototype-only host values and their named/local
  aliases used as constructors under the declared-global page-profile policy,
  preserving checked real constructor aliases.
  A heritage lookup that exceeds its expansion budget now reports a resource
  error instead of silently accepting an unchecked override. Updated the old
  overlapping event-tag refusal to TypeScript's confirmed compatible-callback
  verdict, retaining all wrong-callback/tag/option rejection controls.
- Restored product-line direct lowering that the BlueTS merge had dropped:
  nested function declarations (including method/try/while bodies), keyword
  property names after `.`, and the parser's 64-level named-function cap.
  Existing regressions prove hoisting, closure capture, lexical isolation,
  event callback execution and native stream iterator cancellation. The depth
  regression accepts 63/64 and rejects 65/70 nested declarations with a
  resource diagnostic. Generated host declaration bytes and inventories keep
  their existing versions and supported bindings.
- The two M1a host-suite failures are now resolved: Core's 809 library tests
  and launcher's 305 library tests passed (one launcher test ignored), as did
  252 BlueTS and 164 bridge library tests and the focused host-profile cohort.
  This proves the integration regressions fixed here; session-driven timers,
  frame acceptance and the remaining items in this TODO are still required.
- Final complete BlueTS/bridge library and integration run: 1,222 passed,
  zero failed, 224 optional oracle cases ignored, across 158 test targets.
  Clippy for BlueTS, bridge, core and launcher library/test targets passed
  with `-D warnings`; formatting and `git diff --check` passed. The locally
  recorded 17 native controls ran against version-verified TypeScript 5.9.3;
  ignored external-oracle suites and the full workspace/Test262 gates are not
  claimed by this increment.
