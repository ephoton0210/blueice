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

**Latest milestone — M1p Promise rejection tracking (2026-10-15).**
The owner-selected File DOM v16 profile adds `PromiseRejectionEvent`,
`onunhandledrejection` and `onrejectionhandled` with checked listener typings.
Completed checkpoints queue bounded document tasks; early handlers suppress
notification, late handlers receive one follow-up event, and canceled reports
stay out of the copied console. Pending roots preserve values; outstanding
notifications remain weak and navigation discards old work. All 37 related
Rust targets plus fresh official MCP acceptance pass (3,235 cases; one existing
ignored), along with formatting and workspace/all-target Clippy/build.
All 102,926 pinned Test262 modes match M1o's accepted source/result inventory:
102,921 pass, one stale corpus, four exclusions, no new failure or timeout.
Original source positions and broader host-buffer/network/UI/default-on policy
remain open. See the
[M1p milestone record](#m1p--promise-rejection-tracking-and-global-events-2026-10-15)
and [rejection contract](HOST_PROMISE_REJECTIONS.md).

## A. Event loop and scheduling (P0)

The integrated branch has host-owned Promise checkpoints and session-driven
timers and a separate render entry for animation frames in the owner-selected
File DOM profile. General task-source ordering and complete queue/byte
accounting remain open; M1d now bounds the VM's document-owned scheduling
work and exposes its retained payload ledger.

- [x] Macrotask queue that drains microtasks after every task (per
      `research/js-bytecode-eventloop.md`). M1j selects existing logical
      document task sources through one bounded scheduler; real networking/
      UI source ingress remains in its separate ordering/hook items below.
- [x] **M1i: bounded document task turns.** One host entry shares a task
      budget between the existing timer and browser-work groups, checkpoints
      each delivery and returns the next runnable/deadline wake. Core selects
      one task per poll before offering rendering. Full logical task-source
      associations and network/UI ordering remain in the open item below.
- [x] **M1j: logical document task sources.** Callback timers and abort
      timeouts share one source and host clock. File reading, drag string
      callbacks and JavaScript engine cleanup have separate source identities,
      document enqueue order and fair selection across same-document realms.
      See [the source policy](HOST_TASK_SOURCES.md); real networking/UI ingress
      and host-wait hooks remain in their open items below.
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
- [x] Promise/task hooks that Phase 17's Fetch/XHR needs — M1k acceptance
      below covers the engine mechanism; request-service policy and private
      network IPC remain in E.
- [x] **M1k: owner-installed asynchronous operations.** Real VM-owned
      Promises, opaque completion capabilities, branded async methods and
      queued networking/user-interaction completions share document admission,
      checkpoints and cancellation. See [the host boundary](HOST_ASYNC_OPERATIONS.md).
- [ ] Task-source ordering between timers, network completions and UI events
      — M1j/M1k define logical source selection and queued service completions;
      Core's native UI source association and real request-service ingress
      remain open before `fetch`.
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
- [x] **M1h: native DOM callback cleanup.** Native click, selection, drag and
      asynchronous EventTarget callbacks checkpoint before the next listener,
      while dispatch state remains active. Synchronous script dispatch waits
      for its calling JavaScript to return. All callback cleanups in a native
      dispatch share the existing 256-job cap and retain its instruction fuel.
      Acceptance includes public ordering/resource regressions, dynamic-import
      GC pressure, checked BlueTS callbacks and the real Core selection path.
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

- [x] **M1p: host unhandled-rejection tracking**
      (`HostPromiseRejectionTracker`) and `unhandledrejection` /
      `rejectionhandled` events. Completed checkpoints queue bounded DOM-source
      tasks, preserving original values, cancellation, early/late handling,
      weak outstanding ownership and navigation isolation. Public VM/profile,
      real Core/MCP and all final gates pass; see the milestone record and
      [rejection contract](HOST_PROMISE_REJECTIONS.md).
- [x] **M1g: event-loop FinalizationRegistry cleanup.** Successful page host
      checkpoints discover GC-cleared cells and queue engine tasks, including
      collections during callbacks. Registry/cell ownership survives until
      delivery, so unregister still works before cleanup. One cell per task
      preserves the turn's callback bound and schedules remaining cells in
      later tasks; microtasks finish before the next cleanup callback.
- [x] **M1o: uncaught page exceptions.** File DOM v15 delivers cancelable
      global `ErrorEvent`s and legacy five-argument `window.onerror` calls;
      uncanceled errors reach the bounded Core/MCP console. Script, task,
      microtask, native callback and eventual page-module failures report
      without stopping peer callbacks or Core's session loop. Resource
      failures retain exact-document containment. See the milestone record.
- [x] **M1f: long-lived timer GC behavior.** Public page-runtime fixtures
      complete 4,000 Promise polls under a 1 MiB managed heap and 64 large
      native-string polls under the existing scheduling cap. These verify
      bounded retention between callbacks; broader host/RSS accounting remains
      in A's open resource item.

## C. Web platform globals (P1)

On `main`, none of these were found in production code (only `globalThis`
exists). Several exist on the product line, marked below.

- [x] **M1l: `window` / `self` / `top` / `parent`.** The owner-selected
      `core-script-dom-window-v13` profile installs top-level realm aliases
      with exact generated host-member types. Descriptors, replacement,
      tab/navigation isolation and real JavaScript/BlueTS DOM/timer use are
      covered; frames and a navigation-stable WindowProxy remain outside
      this increment. See [the global contract](WINDOW_GLOBALS.md).
- [x] `console` (log/warn/error/...) routed to the devtools/MCP surface —
      **M1m:** implemented the 19-method namespace, bounded
      copied output and Core/MCP drain with v14 typings. Initial public
      VM/profile, Core/MCP, real MCP and final Rust/Clippy/build gates pass;
      the complete pinned Test262 comparison matches M1l.
      See [the diagnostic contract](HOST_CONSOLE.md).
- [ ] `location`, `history`, `navigator`
- [x] `URL` **(product line, integrated)** — M1l pins the installed global.
- [ ] `URLSearchParams`
- [ ] `TextEncoder` / `TextDecoder`, `atob` / `btoa`
- [ ] `structuredClone`, `performance.now`
- [x] `AbortController` / `AbortSignal` **(product line, integrated)**
- [ ] `localStorage` / `sessionStorage` (policy + persistence design first)
- [x] `EventTarget` / `Event` / `CustomEvent` as general objects
      **(product line, integrated)**
- [x] `Blob` / `File` / `FileReader`, `ReadableStream`, `DOMException`
      **(product line, integrated)**
- [ ] `WritableStream` / `TransformStream` / `CompressionStream` (absent on
      both; needed by `fetch` request and response bodies)
- [ ] `crypto.getRandomValues` / `randomUUID`; entropy and clock injectable
      so tests are deterministic
- [x] **M1o: `reportError` and the global error-event shape.** The owner
      installs six matching v15 bindings; original values, trusted/synthetic
      event behavior, cancellation, recursion and navigation are covered.
      Older profiles remain immutable and reject these names. See B.
- [x] **M1p: global rejection-event surface.** File DOM v16 installs
      `PromiseRejectionEvent`, `onunhandledrejection` and `onrejectionhandled`.
      Independent real JavaScript/BlueTS inventories pin these three names;
      old-profile absence and checked listener/constructor controls pass. See B.
- [x] **M1l: global-inventory test.** Real Core/child JavaScript and checked
      BlueTS pages execute `typeof globalThis.<name>` for every section C
      name, pinning the installed set and remaining absences independently
      of the declaration generator. Existing product APIs above are now
      recorded as integrated; their behavior keeps its existing test suites.

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
      section E. M1k verifies that page-module top-level await resumes from a
      later external completion task through the public runtime.
- [ ] **Typed `fetch` remains absent.** M1m implements typed `console` and
      `window.console` in the exact File DOM v14 artifact; its public typing
      test and real mixed-language MCP acceptance pass, as do the final
      milestone gates. M1b already supplies typed scheduling functions.
- [x] **Ambient value queries and missing intermediate members (M1n).**
      Declaration-owned `typeof` queries retain complete ambient function
      signatures across declaration modules and respect lexical shadowing.
      Missing intermediate dot/indexed members fail checked use, including
      `window.console.log('missing')` against v13. Inferred declarations retain
      valid callable types. Public compiler/profile regressions and real
      Core/child refusal-before-mutation acceptance pass, with complete final
      gates recorded below; installed v13/v14 artifacts remain unchanged.
- [ ] **Original positions for uncaught errors and rejections.** M1o
      provides global exception events and copied Core/MCP diagnostics, but
      generated errors use empty/zero source metadata at this boundary.
      Mapping them back to BlueTS original positions remains open; only
      debugger pauses currently use the safe-point map. M1p implements the
      separate rejection tracker in B.

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

### M1p — Promise rejection tracking and global events (2026-10-15)

- [x] Reproduce missing notification through the real Core/child public boundary.
- [x] Track first rejection/handler transitions across public and internal awaits.
- [x] Queue both global events on the existing document task source.
- [x] Install `PromiseRejectionEvent` and handler attributes with v16 typings.
- [x] Verify early/late handlers, cancellation, GC, resource caps and navigation.
- [x] Pass real Core/MCP and the complete related Rust regression selection.
- [x] Pass final formatting checks.
- [x] Pass workspace/all-target Clippy with `-D warnings`.
- [x] Pass the final workspace/all-target production build.
- [x] Compare the complete pinned Test262 inventory against M1o.
- [x] Finalize evidence, documentation and a detailed English commit.

The owner-selected File DOM v16 surface and tracker are implemented; see
[the rejection contract](HOST_PROMISE_REJECTIONS.md). Its 61 bindings preserve
the v15 identity prefix and old artifact bytes. Both events use the existing
DOM-manipulation task source. Uncanceled copied diagnostics retain the private
v60 Core/MCP console route; no live Promise/reason crosses IPC. Original source
mapping and broader host-buffer/network/UI/default-on policy remain open.

**Verified red baseline and corrections:**

- The first real Core/child fixture expected an ignored rejection to invoke
  `onunhandledrejection` before a later timer published its result. It failed
  with `result:unreported` instead of `result:probe` (one failed, fourteen
  filtered; `/private/tmp/blueice-rejections-red.log`). The final fixture passes.
- The first VM run passed nine of eleven groups. Streams internal reader
  `closed` promises needed explicit handled flags at error initialization,
  error delivery and reader release. The adoption/combinator assertion also
  expected the wrong order: independent Node v26.7.0 confirms `0,2,1`, matching
  the existing queue. Evidence: `/private/tmp/blueice-rejections-vm.log` and
  `target/rejection-order-node-20261015.json`. The corrected host follow-up
  passed all 44 selected cases; subsequent controls expanded rejection coverage
  to thirteen groups, all green in the final complete selection.
- Initial official MCP acceptance observed only JavaScript output. Independent
  checked compilation confirmed that `Promise.reject` is refused because this
  page profile does not declare the `Promise` value. The corrected typed fixture
  uses `Blob.text().then(rejectTyped)` with a named typed callback. Profile
  regressions retain the undeclared-global refusal; both languages now reach
  the official MCP client. Failed evidence remains in
  `/private/tmp/blueice-rejections-mcp.log` and
  `/private/tmp/blueice-rejections-typed-probe.log`.

**Completed macOS acceptance:**

- All 37 selected targets (six libraries and 31 public integration targets):
  3,234 passed, one existing ignored, zero failed. Coverage includes thirteen
  rejection groups, existing error/console/scheduling/finalization behavior,
  Promise/async/generator/Streams GC and lifecycle controls, old/new profile
  checks, all fifteen real timer/page fixtures and Core/launcher/IPC/MCP.
- Fresh official MCP client → Core → child acceptance passes (one passed,
  eleven filtered; final replay 4.17s). It verifies JavaScript and checked
  BlueTS output, cancellation/early handling, exact native late-handler order,
  untrusted-content framing and replacement-document isolation.
- Final formatting, workspace/all-target Clippy with `-D warnings` (13m 41s)
  and workspace/all-target production build (15m 02s) pass.
- Complete pinned Test262: 102,921 passed, one stale corpus, four exclusions,
  zero new failures/timeouts, 437.384s. All 102,926 unique path/modes match
  M1o for source hashes, metadata, expectations, dispositions and result
  kind/phase. Only elapsed time, binary hashes and nondeterministic diagnostic
  text are excluded from the comparison. Frozen binary hashes match the run
  summary. No fresh coverage, full-workspace-test, reference-TypeScript or
  other-platform result is claimed.

Final evidence:

- Rust log: `/private/tmp/blueice-rejections-regressions.log`;
  audit: `target/rejections-rust-validation-20261015.json`;
  unchanged 23-file Rust manifest: `target/rejections-source-final-20261015.json`.
- Final MCP/format/Clippy/build logs:
  `/private/tmp/blueice-rejections-mcp-final.log`,
  `/private/tmp/blueice-rejections-format.log`,
  `/private/tmp/blueice-rejections-clippy.log`,
  `/private/tmp/blueice-rejections-build.log`.
- Full raw Test262 evidence: `target/test262-rejections-full-20261015/`
  (`summary.json`, `results.jsonl`, `baseline-comparison.json`);
  frozen binaries: `target/test262-rejections-bins-20261015/`.
- Unmodified committed summary:
  [test262-browser-rejections-macos-summary.json](test262-browser-rejections-macos-summary.json).

### M1o — Uncaught page exceptions and global error events (2026-10-15)

- [x] Reproduce missing reporting through the real Core/child public boundary.
- [x] Install owner-selected `ErrorEvent`, global EventTarget methods,
      `onerror` and `reportError`, with a new immutable typing profile.
- [x] Report script/task/microtask/native-callback exceptions exactly once.
- [x] Verify cancellation, recursive reporting, GC ownership and resource limits.
- [x] Verify bounded Core/MCP diagnostics and continued page/session work.
- [x] Pass the complete related Rust regression selection.
- [x] Pass final formatting checks.
- [x] Pass workspace/all-target Clippy with `-D warnings`.
- [x] Pass the final workspace/all-target production build.
- [x] Compare the complete pinned Test262 source/result inventory against M1n.
- [x] Finalize evidence, documentation and a detailed English commit.

The owner-selected File DOM v15 surface and shared reporter are implemented;
see [the error-reporting contract](HOST_ERROR_REPORTING.md). The prior v14
artifact and private v60 console transport remain unchanged. Original source
positions and general rejection tracking were separate open items at M1o;
M1p above completes the rejection tracker.

**Verified red baseline:** the real Core/child `reportError(new Error('probe'))`
fixture left its page showing `unreported`, instead of invoking `window.onerror`.
`/private/tmp/blueice-page-errors-red.log`: one failed, thirteen filtered.
The rebuilt fixture now passes.

**Completed macOS acceptance:**

- All 24 selected targets (six libraries and eighteen public integration
  targets): 3,101 passed, one existing ignored, zero failed. This includes
  1,200 BlueJS library cases; twelve public `host_errors` cases; profile
  compatibility/refusal controls; all fourteen real timer/page fixtures;
  forced GC, cleanup/navigation, asynchronous drag, module/TLA and caught
  dynamic-import controls; and Core/launcher/IPC/MCP regressions.
- The official MCP client → Core → child acceptance passed (one passed, ten
  filtered; final replay 4.65s). It covers JavaScript/BlueTS reports, canceled
  output, microtask/timer errors, untrusted framing, a throwing native click
  listener followed by a successful peer, and navigation replacement. The
  final-binary replay also passes; total acceptance is 3,102 passed and one
  existing ignored across 24 complete targets plus that focused MCP case.
- Final `cargo fmt --all -- --check` passed.
- `cargo clippy --workspace --all-targets -- -D warnings` passed in 14m 11s.
- `cargo build --workspace --all-targets` passed in 17m 11s.

The initial broad Rust build found a test-only `LinkedModule` fixture missing
its new one-time flag. Its initializer was corrected; the complete rerun
above passed. Logs: `/private/tmp/blueice-page-errors-{focused,host-followup,mcp,regressions,regressions-2,fmt-final,clippy}.log`.
Audit: `target/page-errors-rust-validation-20261015.json`, with the final Rust
source manifest `target/page-errors-source-final-20261015.json`; hashes remain
unchanged. No fresh coverage, complete workspace test suite or other-platform
execution is claimed.

**Final Test262 acceptance:** all 102,926 pinned modes match M1n's accepted
source/result inventory: 102,921 pass, one stale corpus, four exclusions,
no new failure or timeout (395.019s). Per-mode source hashes, metadata,
expectations, actual kind/phase and dispositions match; elapsed time and
nondeterministic diagnostic text are excluded from comparison. The runner's
exit 1 reflects the accepted stale/excluded baseline. Frozen production
adapter/worker hashes match the runner summary and remain unchanged.

- Raw run: `target/test262-page-errors-full-20261015/` (`summary.json`,
  `results.jsonl`, `baseline-comparison.json`).
- Frozen binaries: `target/test262-page-errors-bins-20261015/sha256.json`.
- Committed unmodified runner summary:
  [`test262-browser-error-reporting-macos-summary.json`](test262-browser-error-reporting-macos-summary.json).
- Final logs: `/private/tmp/blueice-page-errors-{workspace-build,mcp-final,test262}.log`.

### M1n — Checked ambient value queries and member chains (2026-10-15)

- [x] Reproduce both gaps through public compiler/page-profile interfaces.
- [x] Preserve ambient `typeof` signatures across declaration modules.
- [x] Respect lexical shadowing and declared callable overloads.
- [x] Reject absent intermediate members in checked member/call chains.
- [x] Pass the relevant compiler, bridge and real page regressions.
- [x] Pass final formatting checks.
- [x] Pass workspace/all-target Clippy with `-D warnings`.
- [x] Pass the final workspace/all-target build.
- [x] Compare the complete pinned Test262 inventory after the Rust gates.
- [x] Finalize evidence and the detailed English milestone commit.

This completes section H's existing compiler defect. No new host global or
profile version is added; installed v13/v14 declarations retain their bytes
while the checker enforces their actual surface.
See [the checker contract](HOST_TYPING_BOUNDARIES.md).

**Implemented behavior:** ambient function values preserve every overload and
generic relationship. Declaration-owned queries use module/offset identities
and the ordinary lexical model, so consumer shadows cannot change their
meaning. Resolved query types survive inferred declaration publication;
nameable public recursion remains valid and unresolved/private unnameable
queries fail closed. The existing closed declaration graph is unchanged.

Each postfix property prefix and indexed callee is checked before an absent
receiver can become `unknown`. Compatibility fixes preserve operator
precedence, type assertions, prefix updates, quoted keys, flow-only `never`,
specialized enum diagnostics and merged namespace/enum members. Compiler
fingerprints include `value-boundaries-v1`. File DOM v13/v14 declarations,
runtime inventories and private IPC are unchanged.

**Red baseline:** five ambient-query groups failed and the public v13 profile
accepted four absent intermediate-member chains; its four existing controls
passed (`/private/tmp/blueice-ambient-boundaries-red.log`). Final public cases
cover declaration ordering/identity, overloads/generics, lexical shadows,
qualified/missing queries, expansion limits, inferred `.d.ts` publication and
closed-graph controls. The page-profile matrix covers dot/indexed/nested and
short-circuit chains. A real Core/child fixture rejects three source variants
before an earlier DOM write can run. That fixture's attempted extra red run was
already green; it is positive acceptance, not the original failing baseline.

**Final Rust acceptance (macOS, 2026-10-15):**

- All 164 Cargo-enumerated compiler/bridge test targets: 1,248 passed,
  zero failed, 224 existing ignored, 723.136s. The executor ran every test
  executable from the successful final Cargo no-run build, using each package's
  working directory. Source/executable hashes remained unchanged.
- Six complete Core/launcher/IPC/MCP targets, including real timer and FileReader
  pages: 1,518 passed, zero failed and one existing ignored case.
- Separate official MCP client → Core → child console acceptance: one passed,
  nine filtered; this is additional focused acceptance, not another complete
  target. Total: 2,767 passed, 225 existing ignored, 170 complete targets plus
  that focused MCP case.
- Final `cargo fmt --all -- --check`,
  `cargo clippy --workspace --all-targets -- -D warnings` and
  `cargo build --workspace --all-targets` passed.

Audit: `target/host-typing-final-rust-20261015/{manifest,summary}.json` and
per-target logs; `target/host-typing-host-rust-validation-20261015.json`;
`/private/tmp/blueice-ambient-{final-targets,host-regressions,mcp-final}.log`.
The earlier exploratory all-target run had six failing cases in four targets
and a SIGKILL in `names`; the final stable-build run passes all affected targets
and matrices. Recorded TypeScript 5.9.3 verdicts were checked, not a fresh
reference compiler run. No fresh coverage, complete workspace test suite or
other-platform execution is claimed.

**Final Test262 acceptance:** the complete pinned inventory matches M1m:
102,921 pass / 102,926 modes, one stale corpus, four exclusions and no new
failure or timeout (426.973s). Per-mode source hashes, metadata, expected and
actual result kinds/phases and dispositions match; elapsed time and diagnostic
message text are excluded from comparison. The runner's exit 1 reflects the
accepted stale/excluded baseline, not a new failure. Frozen adapter/worker
hashes are unchanged from M1m.

- Raw run: `target/test262-host-typing-full-20261015/` (`summary.json`,
  `results.jsonl`, `baseline-comparison.json`).
- Frozen binaries: `target/test262-host-typing-bins-20261015/sha256.json`.
- Committed unmodified runner summary:
  [`test262-browser-host-typing-macos-summary.json`](test262-browser-host-typing-macos-summary.json).
- Raw log: `/private/tmp/blueice-host-typing-test262.log`.

### M1m — Page console and MCP diagnostics (2026-10-15)

- [x] Implement and verify the complete public console VM target (10 cases).
- [x] Generate v14 namespace/window typings and pass refusal controls.
- [x] Pass Core containment and MCP transport/untrusted-framing boundaries.
- [x] Pass the real official-MCP-client → Core → child acceptance.
- [x] Pass `cargo fmt --all -- --check`.
- [x] Finish the final related Rust regression batch (3,488 passed; 1 ignored;
      38 targets after rerunning the three affected targets).
- [x] Pass the final workspace/all-target production build.
- [x] Pass final workspace/all-target Clippy.
- [x] Compare the complete pinned Test262 inventory against M1l.
- [x] Finalize milestone documentation and the detailed English commit.

- **Delivered:** an owner-installed console namespace
  with logging/assertion/inspection/trace/group/count/timer methods; bounded
  copied records; native-byte accounting; exact File DOM v14 typings;
  private v60 document-bound drain; Core reply validation/containment; and
  MCP `get_console_messages` with untrusted-page-content framing.
- **Verified so far:** the new real Core/child fixture first failed on
  `console.log` before installation. Six initial public VM cases passed,
  including formatting/coercion, exception propagation, side-effect-free
  object previews, stateful methods and retained byte/count limits. The
  public typing case passed valid namespace/window calls and wrong-argument
  refusal controls. The first `cargo build --workspace --all-targets`
  passed after adding the frontend's new-message branch.
- **Review follow-ups implemented:** lossless
  UTF-16 count/timer keys, native instruction fuel for long unknown format
  specifiers, UTF-8 truncation that preserves a prefix, per-tab/navigation
  cleanup, malformed Core reply containment and MCP transport/framing tests.
  The focused batch passed four selected public VM cases, the typing case,
  four Core cases and two MCP cases. The complete ten-case VM target also
  passed unfiltered against that freshly built test binary, including native
  fuel, lossless labels and UTF-8 prefix controls. Logs:
  `/private/tmp/blueice-console-vm-complete.log` and
  `/private/tmp/blueice-console-boundaries.log`.
- **Real MCP acceptance passed:** official MCP client → Core → BlueJS
  child, mixed JavaScript/checked BlueTS, microtask/timer/native-click logs,
  draining and navigation isolation (one test, zero failures). Repeated
  against the final production build: passed in 4.71s. Final log:
  `/private/tmp/blueice-console-mcp-final.log` (initial run:
  `/private/tmp/blueice-console-mcp-e2e.log`).
- **Full pinned Test262 accepted:** 102,921 / 102,926 modes pass, with the
  unchanged one `stale_corpus` and four exclusions, zero failures/timeouts,
  in 394.494s. Every unique path/mode, source hash, expectation, flag and
  disposition/result kind/phase matches M1l; only elapsed time and diagnostic
  text are ignored per mode, and only binary hashes/elapsed time differ in
  the summary. Frozen executable hashes remain intact. The
  [unmodified machine summary](test262-browser-console-macos-summary.json)
  is committed; raw results and comparison remain in
  `target/test262-console-full-20261015/`. Full-run log:
  `/private/tmp/blueice-console-test262.log`.
- **Final Rust/build gates passed:** `cargo fmt --all -- --check`, workspace/
  all-target production build and workspace/all-target Clippy with
  `-D warnings`. Build/Clippy logs:
  `/private/tmp/blueice-console-final-build.log` and
  `/private/tmp/blueice-console-clippy-final.log`. Updated test sources also
  pass targeted Clippy in `/private/tmp/blueice-console-affected-clippy-final.log`.
- **Regression follow-up:** the broad batch exposed an older launcher test
  that expected zero native bytes after timer/frame cleanup. Console's fixed
  empty queue is now deliberately retained and charged. The test now measures
  the installed profile's baseline before admitting timer work and requires
  cleanup to return exactly to it; all ten launcher resource-accounting
  cases passed their focused rerun (`/private/tmp/blueice-console-ledger-rerun.log`). The broad
  batch also exposed the IPC test's old hard-coded v59 assertion (updated
  to v60) and a frame/Representation generation mismatch in the existing
  abort-timeout fixture during concurrent Clippy. All three affected targets
  passed complete reruns; the strict frame assertion was not relaxed. The
  remaining broad targets, including all 160 MCP library cases, passed.
  Complete affected-target rerun:
  `/private/tmp/blueice-console-affected-targets-final.log`.
- Full behavior and limits are recorded in [HOST_CONSOLE.md](HOST_CONSOLE.md).
  Console's MCP route is complete; global error/rejection delivery, source-map
  positions, real request-service ingress and broader host buffers remain open.

**Accepted Rust inventory:** 3,123 library cases, 352 public runtime/profile
cases and 13 real Core/child page cases pass after the affected-target rerun;
one existing launcher case remains ignored. The separate real MCP acceptance
also passes. The audit preserves the broad batch's exit 101 and the successful
three-target rerun, rather than presenting the first run as green:
`target/console-rust-validation-20261015.json`. Targeted Clippy for the updated
test sources and the final format check pass. Full pinned Test262 also passes
the comparison above. This is macOS acceptance; no fresh full-workspace test
suite, coverage or other-platform claim is made.

### M1l — Top-level global aliases and verified page inventory (2026-10-15)

- A new real Core/child counter fixture first reproduced rejected JavaScript
  execution when the page accessed missing `window`/`self`/`top`/`parent`.
  The owner-selected File DOM profile now installs four aliases of the
  intrinsic realm global. `window`/`top` are unforgeable getter properties;
  `self`/`parent` are replaceable accessors. All four validate receivers and
  retain intrinsic identity if the page changes its `globalThis` property.
- Public VM/page-runtime cases verify descriptors, strict/sloppy assignment,
  replacement/deletion, non-constructible accessors, global versus lexical
  bindings, collision preflight, non-extensibility, ShadowRealm absence,
  independent tabs and fresh navigation realms. This is a top-level host
  increment; nested contexts, cross-origin access and navigation-stable
  WindowProxy remain open. See [the contract](WINDOW_GLOBALS.md).
- Added exact `core-script-dom-window-v13` typings with 51 runtime inventory
  entries, preserving all 47 inherited entries and the older artifacts.
  Window host members are generated from the existing declaration AST and
  signature text, retaining DOM/constructor/optional/rest/callback types.
  Both child installation and checked BlueTS preparation select and verify
  v13. Private protocol v59 has no new fields.
- Real JavaScript and checked BlueTS Core/child pages independently observe
  all 33 section C global names, plus 11 DOM/scheduling/messaging controls.
  Both the inventory and aliased DOM/timer cases must then produce three
  distinct counter frames and matching Representation generations. Existing
  integrated product globals are now checked in section C; missing APIs
  remain pinned as absent rather than inferred from source searches.
- Typing probes also exposed unresolved ambient value queries and unchecked
  missing intermediate members. Their concrete acceptance cases are now
  open section H work. The Window declaration projection preserves current
  host signatures without claiming these compiler gaps fixed or exposing a
  full `lib.dom.d.ts`.
- Final Rust acceptance: **3,097 tests passed** across 34 targets: 2,744 in
  five related libraries, 341 public runtime/allocation/profile cases and
  12 real Core/child fixtures. One existing launcher case remains ignored.
  Five-crate all-target Clippy, production-binary builds, formatting and
  whitespace checks passed. The accepted case audit is
  `target/window-globals-rust-validation-20261015.json`.
- Final full-inventory acceptance against rebuilt, frozen production
  binaries: **102,921 / 102,926 pass** in **374.523 seconds**, one existing
  stale-corpus mode, four existing exclusions and **zero failures/timeouts**.
  Every mode's source/expectation/flags/disposition and actual error kind/phase
  matches accepted M1k; all summary settings match except executable hashes
  and elapsed time. Diagnostic text is not an acceptance key. Both frozen
  executable hashes remain intact. The
  [unmodified machine summary](test262-browser-window-globals-macos-summary.json)
  is committed; raw results/comparison remain under
  `target/test262-window-globals-full-20261015/`. No fresh workspace-wide or
  coverage claim is made; console, error/rejection reporting, real network/UI
  ingress and the remaining host surfaces stay open.

### M1k — Host Promise waits and queued service completions (2026-10-15)

- Public tests first failed because the page registrar/runtime had no
  asynchronous host-call or completion capability. Existing callbacks could
  return only synchronous primitive values.
- Added owner-installed Promise-returning functions and branded wrapper
  methods. The VM owns Promise creation, roots and normal rejection semantics;
  host callbacks receive only primitive arguments, private wrapper keys and
  opaque completion tokens. Cold standard-global collisions are refused
  through the canonical lazy-global inventory.
- Completions resolve/reject only in a selected document task. They support
  primitives, VM-created TypeErrors and VM-minted wrapper results with further
  async methods. Networking/user-interaction sources preserve completion
  arrival order and rotate with existing work. Microtask-only entry cannot
  settle a wait, and each selected completion finishes its checkpoint.
- Pending waits and queued payloads use the existing reaction/request, task
  and byte limits. Navigation/close discard the work, invalidate tokens and
  notify service cancellation. Awaited frames remain live through collection;
  delivered waits release their roots. Host-owned I/O buffers outside these
  tables, Core's real network/UI ingress and full Fetch/XHR semantics remain
  open. The private v59 schema and installed v12 globals/typings are unchanged.
- Public acceptance also verifies 1,000 completed operations under a 1 MiB
  managed heap and module top-level await resuming only after a later host
  completion task. The latter closes H's scheduling-verification uncertainty
  while leaving the typed networking APIs open.
- Final review measured geometric spare capacity in `Result<Vec<_>>`
  argument conversion (33 inputs allocated 64 slots). Argument copies now
  allocate their admitted slot count explicitly. A 33-argument/partial-
  conversion public case verifies values and refusal before service start.
- Final Rust acceptance after that correction: **3,080 related tests passed**
  in the 30-target batch: 2,744 across five affected libraries, 326 public
  runtime/DOM/allocation cases and ten real Core/child fixtures. All 24 new
  async-host cases pass; one existing launcher case remains ignored.
  Five-crate all-target Clippy, formatting and whitespace checks passed.
  Production binaries were rebuilt; the accepted case audit is
  `target/host-async-tasks-rust-validation-20261015.json`.
- Final full-inventory acceptance against the rebuilt, frozen binaries:
  **102,921 / 102,926 pass** in **413.223 seconds**, one existing stale-corpus
  mode, four existing exclusions and **zero failures/timeouts**. All source/
  mode hashes, dispositions, expectations, feature flags, error kinds/phases
  and runner settings match M1j; diagnostic text and elapsed time are not
  acceptance keys. Both frozen executable hashes stayed intact. The
  [unmodified machine summary](test262-browser-host-async-macos-summary.json)
  is committed; raw results and the comparison remain under
  `target/test262-host-async-tasks-accepted-full-20261015/`. No fresh
  workspace-wide or coverage claim is made; broader M1 ingress, host-buffer
  and reporting requirements remain open.

### M1j — Logical document task sources and shared timeout clocks (2026-10-15)

- Red-first public tests exposed a later abort timeout overtaking an earlier
  callback timer, and abort deadlines ignoring the page runtime's manual
  clock. These were consequences of scheduling by storage group and keeping
  page abort timeouts on an unrelated Instant clock.
- One logical selector now covers JavaScript engine cleanup, file reading,
  timers and drag string callbacks. Same-source ordering uses document-wide
  enqueue serials across the root and its same-document ShadowRealms; timer
  order uses deadline then serial. Cross-source rotation preserves the
  64-delivery total cap and the production one-task Core boundary. Legacy
  browser-only turns reuse the selector without running callback timers.
- Callback timers and page abort timeouts share the sampled document clock.
  Interval and file-stage continuations receive fresh serials. Timer-source
  snapshots also defer callback-created abort timeouts until another turn.
  Standalone abort embeddings retain their prior Instant clock policy.
- [Source associations, clock behavior and future ingress requirements](HOST_TASK_SOURCES.md)
  document the implementation policy and standards references. Real network/
  UI source integration, Promise host-wait hooks, broader host-buffer policy
  and error/rejection reporting remain open.
- Combined macOS Rust gates: **3,056 related tests passed**: 2,744 across
  the five affected libraries, 302 public runtime/DOM/allocation cases and ten
  real Core/child fixtures. One existing launcher case remains ignored. The
  initial broad batch used the pre-correction recorder helper and typed arrow
  fixture; the three corrected realm cases and all ten final Core cases
  passed separately. Five-crate all-target Clippy, final BlueJS test-target
  Clippy, formatting and whitespace checks passed. The case-by-case accepted
  Rust audit is `target/host-task-sources-rust-validation-20261015.json`.
- The complete pinned Test262 inventory finished in **462.012 seconds**:
  **102,921 / 102,926 pass**, one existing stale-corpus mode, four existing
  exclusions and **zero failures/timeouts**. All source/mode hashes,
  dispositions, expectations, feature flags, error kinds/phases and runner
  settings match M1i; diagnostic text and elapsed time are not acceptance
  keys. Both frozen executable hashes stayed intact. The
  [unmodified machine summary](test262-browser-task-sources-macos-summary.json)
  is committed; raw results and the comparison remain under
  `target/test262-host-task-sources-full-20261015/`. This milestone adds no
  fresh workspace-wide or coverage claim; M1 remains open for the ingress,
  host-buffer and reporting requirements above.

### M1i — Bounded document task turns and render opportunities (2026-10-15)

- The real public Core fixture failed before implementation: three ready
  timers ran in one child batch, and the first observed Representation already
  contained `tick:3`. The child previously drained up to 64 browser tasks and
  then 64 timers before returning to Core.
- Added the source-free `run_task_turn`/`BlueJsTaskTurn` page boundary. Timer
  and browser work share a maximum 64-delivery turn and alternate when both
  groups are ready. Browser cleanup/ordinary fairness and same-document
  ShadowRealm ownership remain in the existing document scheduler. Timer
  snapshots retain due/creation order, cancellation and next-turn rearming;
  browser tasks retain their ready FIFO order. Every delivery checkpoints.
- The child selects one task per ordinary Core poll, permitting rendering and
  frontend service before another task. Render callbacks keep their separate
  entry. Both task and frame replies report ready browser work as an immediate
  wake, include future abort deadlines and preserve timer wakes. Future live
  abort work neither consumes task budget nor blocks a ready read; dead weak
  entries can be reaped immediately. Long abort deadlines use bounded private
  wake chunks while the VM retains the true deadline. The v59 schema/range
  validation and v12 host globals/typings are unchanged.
- This defines fair arbitration between the existing implementation groups;
  general logical task-source associations and network/UI source policy still
  need the open ordering work before Fetch. The design follows the
  [two-entry-point decision](../research/js-bytecode-eventloop.md) and
  [HTML task selection/checkpoint model](https://html.spec.whatwg.org/multipage/webappapis.html#event-loop-processing-model).
- The JavaScript/BlueTS burst fixture now requires three distinct PNG frames
  and a matching post-microtask Representation after each timer. The existing
  FileReader fixture initially assumed navigation drained every read task;
  it now waits for Core frame notifications before observing later task
  results, retaining all data, cancellation and native-event assertions.
- Final macOS gates: **3,042 related tests passed**: 2,741 across the five
  affected libraries, 292 public runtime/DOM/allocation cases and nine rebuilt
  real Core/child fixtures; one existing launcher case remains ignored.
  Five-crate all-target Clippy, the updated FileReader fixture's Clippy and
  formatting passed. The pinned full Test262 inventory completed in
  **450.159 seconds**: **102,921 / 102,926 pass**, one existing stale-corpus
  mode, four existing exclusions and **zero failures/timeouts**. All source/
  mode hashes, dispositions, expectations, feature flags, error kinds/phases
  and runner settings match M1h; diagnostic text and elapsed times are not
  acceptance keys. Both frozen binaries retained their recorded hashes.
  The [unmodified machine summary](test262-browser-task-turns-macos-summary.json)
  records the gates; raw results and the comparison remain under
  `target/test262-host-task-turns-full-20261015/`. This is focused affected-
  crate validation, with no new workspace-wide or coverage claim. M1 remains
  incomplete for general ordering, broader host-buffer policy and error/
  rejection reporting.

### M1h — Native DOM callback microtask cleanup (2026-10-15)

- Public red-first tests exposed whole-dispatch cleanup: a first listener's
  jobs ran after the next listener and could not remove it, stop bubbling or
  cancel the native default action while dispatch was still active. Nine of
  the first eleven cases failed, including FileReader abort timing and passive
  callback state. A separate dynamic-import/major-GC case then exposed a stale
  event handle when module execution replaced the dispatch's operand stack.
- Page native click, selection and drag entries now own one cleanup scope.
  Native FileReader and timeout-driven EventTarget dispatch use the same
  mechanism. Each callback cleans up after its JavaScript stack is empty,
  before resetting passive state or selecting the next listener; script-driven
  dispatch remains synchronous within its enclosing native call. Throwing
  callbacks and jobs do not skip ordinary cleanup or later listeners. Resource
  failure stops immediately, before later jobs/listeners or a default action.
- All callback checkpoints and the native dispatch exit share the existing
  256-job budget. Empty checkpoints consume no jobs, an exact 256-job drain
  succeeds, and cleanup cannot refill the dispatch's instruction fuel. The
  displaced execution frame and callback completion keep independent GC roots
  during import/module work; normal and failed checkpoints restore that frame
  and release temporary ownership. Cleanup only queues registry tasks for a
  later browser turn.
- References: [Web IDL callback operations](https://webidl.spec.whatwg.org/#call-a-user-objects-operation),
  [HTML script cleanup](https://html.spec.whatwg.org/multipage/webappapis.html#clean-up-after-running-script),
  [DOM listener invocation](https://dom.spec.whatwg.org/#concept-event-listener-inner-invoke).
  Standalone language VMs retain their existing manual host policy. The v12
  installed globals/typings and private v59 wire schema remain unchanged.
- Final macOS gates: **3,022 related tests passed**: 2,740 across the five
  affected libraries, 274 public runtime/DOM/allocation cases and eight rebuilt
  real Core/child fixtures; one existing launcher case remains ignored. This
  includes 20 new public event cases and checked BlueTS callback ordering.
  Corrected the transport fixture's old whole-dispatch expectation: its first
  callback's microtask cannot see a later listener in that click; the second
  click removes the canceler and the third is uncanceled. The fresh launcher
  test binary passed all 306 cases after that correction. The real selection
  fixture exposes the first callback's microtask state from the next listener.
  All-target Clippy for the five affected crates and workspace formatting pass.
- An initial full Test262 run overlapping compiler/allocation work hit two
  Date wall deadlines. The same binary hashes and per-case limits passed the
  four affected modes in 51.740 seconds. The subsequent unfiltered four-worker
  run completed in **455.744 seconds: 102,921 / 102,926 modes**, one existing
  stale-corpus entry, four exclusions and **zero failures/timeouts**. All
  102,926 mode paths, source hashes and dispositions, plus runner/corpus,
  per-case budgets and group/feature results, match M1g. Frozen adapter/regex
  worker bytes were verified before and after the run. See the raw
  [machine summary](test262-browser-event-checkpoints-macos-summary.json).
- General task-source ordering, broader host-buffer accounting and unhandled
  rejection/error reporting remain open; M1 as a whole is not complete. Full
  workspace and coverage gates are not claimed by M1h.

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
