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

## A. Event loop and scheduling (P0)

The engine has Promise jobs (`run_promise_jobs`, `run_promise_jobs_bounded`)
but no host event loop.

- [ ] Macrotask queue that drains microtasks after every task (per
      `research/js-bytecode-eventloop.md`)
- [ ] `setTimeout` / `clearTimeout` / `setInterval` / `clearInterval`
      (today only a Test262-harness stub in `vm/test262/harness.rs`)
- [ ] `queueMicrotask`
- [ ] Render-pass entry point: run scheduled script before layout/paint,
      checkpoint microtasks afterwards
- [ ] `requestAnimationFrame` / `cancelAnimationFrame`
- [ ] `MessageChannel` / `postMessage`
- [ ] Timers and queued tasks are dropped on navigation (old document
      generation), bounded per tab, and accounted to the initiating tab
- [ ] Decide where timers live across the core / child-process boundary
- [ ] Promise/task hooks that Phase 17's Fetch/XHR needs
- [ ] Task-source ordering between timers, network completions and UI events
      (one bounded queue today, no source priority); decide before `fetch`
- [ ] Per-document caps on pending timers, queued tasks, queued microtasks and
      host-held bytes, so a runaway page degrades only itself
- [ ] Host wake-up: the engine reports the next due time so the session tick
      sleeps correctly (the launcher's `DragTasksFlushed` reply needs a
      matching field)
- [ ] Microtask checkpoint also runs after every DOM event dispatch, not only
      after browser-task turns (*verify*)
- [ ] *Parallel work*: an uncommitted `backend/bluejs/tests/host_timers.rs`
      (page-runtime timers with a host-supplied clock) exists in the
      `feature/bluejs-browser-integration-gaps` worktree, and a tested
      first slice sits on the local `wip/bluejs-web-timers` branch of the
      `feature/browsable-web-mvp` worktree. Reconcile before either lands.

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

- [ ] Wire `blueice_ipc::script` into a real BlueJS host process end to end
      (PLAN.md checklist item still open)
- [ ] End-to-end smoke test: a real `<script>` fixture through the full
      pipeline
- [ ] *Verify*: `<script src>`, `defer`/`async`, `type=module`, inline
      handlers (`onclick=`), execution order vs. parsing
- [ ] `bluejs` shell (REPL + batch) and the full Node differential harness
- [ ] `bluejs_run` / `bluejs_analyze` MCP tools (with Phase 12)
- [ ] AI-facing capability summary and Phase 7 gatekeeper script hooks
- [ ] **Default-on script execution** is a reviewed decision, not a flag
      flip: today scripts run only behind `--inline-bluejs` or the page-host
      options, so a normal launcher session never executes one. The core
      selects the supported profile; the page never does.
- [ ] A script DOM change must publish a frame in the same render pass, with
      the same `generation` on the `Representation`. *Fixed on
      `feature/browsable-web-mvp` (`a54ddceea`)*: relayout without a frame
      push left human and AI observers stale until an unrelated request.

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
- [ ] **`async` / `await` is checkable but not useful on a page.** The engine
      runs Promise jobs and module top-level await, and the debugger can
      pause across module-await continuations, but there is no timer,
      `fetch` or task source (sections A, E), so a typed async function can
      only await values that resolve from microtasks.
- [ ] **No `console`, `setTimeout`, `fetch`** for typed code: the commonest
      TypeScript snippets cannot be written against the supported profile.
- [ ] **Uncaught errors and rejections** cannot be mapped back to BlueTS
      original positions until the host reports them (section B); today
      only debugger pauses use the safe-point map (*verify* for thrown
      exceptions and `Error.stack`).

### BlueJS capabilities BlueTS should use or still needs

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
