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

**Integration progress (2026-10-14).** This checkout has merged the product
and BlueTS lines. The baseline descriptions below remain the original gap
inventory; the dated milestone records identify what has actually been
implemented and verified on `feature/bluejs-browser-integration-gaps`.

## A. Event loop and scheduling (P0)

The integrated branch has host-owned Promise checkpoints and session-driven
timers in the owner-selected File DOM profile. General task-source ordering,
animation frames and complete queue/byte accounting remain open.

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
- [ ] `requestAnimationFrame` / `cancelAnimationFrame`
- [ ] `MessageChannel` / `postMessage`
- [ ] Timers and queued tasks are dropped on navigation (old document
      generation), bounded per tab, and accounted to the initiating tab
      — timer lifetime/count/argument bounds are covered by M1b; complete
      queued-task and host-byte accounting is still required.
- [x] **M1b: core / child timer ownership.** The child realm roots callbacks
      and arguments; the host injects a document-local monotonic clock. Core
      transports only exact document identities and relative wake-up delays.
- [ ] Promise/task hooks that Phase 17's Fetch/XHR needs
- [ ] Task-source ordering between timers, network completions and UI events
      (one bounded queue today, no source priority); decide before `fetch`
- [ ] Per-document caps on pending timers, queued tasks, queued microtasks and
      host-held bytes, so a runaway page degrades only itself
- [x] **M1b: timer wake-up.** `DragTasksFlushed.next_wake_in_ms` carries the
      next runnable delay. Core validates its identity/range, clears it on
      replacement/close, and shortens its ordinary polling timeout for an
      earlier timer. Paused debugger realms retain timers without busy wakes.
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
`FinalizationRegistry` (cleanup runs as a queued job after collection).
**Resource behavior is not.** On the Test262 line every Promise is a GC root
for the life of the VM.

- [ ] **Promise collectibility (P1).** An unreachable settled Promise, or a
      Promise/result cycle, must be collectible so a long-lived page that
      polls or streams has bounded memory. Product-line tests
      `promise_gc_lifecycle` (4 assertions) already express this and fail on
      the Test262 rooting. Prerequisite: every async-function, generator,
      `Array.fromAsync` and host-wait path registers the Promise it is still
      using. A port of the product line's `owned_edges` onto the Test262
      rooting was tried and reverted: it made 10 lib allocation-failure tests
      and several integration targets fail (`invalid Promise receiver`,
      `Heap(InvalidObject)`). Acceptance: a long-running poll loop's managed
      heap stays bounded, plus the full Test262 inventory unchanged.
- [ ] **`AsyncIteratorClose`** opcode was dropped in the merge in favor of the
      Test262 line's closing; re-check stream tee and `for await` early-exit
      cleanup against the product-line tests that depended on it.

- [ ] Host unhandled-rejection tracking (`HostPromiseRejectionTracker`) and
      the `unhandledrejection` / `rejectionhandled` events
- [ ] Drive FinalizationRegistry cleanup jobs from the event loop, not only
      from a manual job drain (*verify*)
- [ ] Error reporting for uncaught exceptions in tasks (`window.onerror`
      shape), without aborting `core`'s session loop
- [ ] Heap/GC budget behavior for long-lived pages with timers (*verify*:
      limits were designed for run-to-completion scripts)

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
