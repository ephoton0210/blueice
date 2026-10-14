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

## B. Promise, GC and error reporting (P0/P1)

Engine side is done: spec-shaped `PromiseCapability`, `WeakRef`,
`FinalizationRegistry` (cleanup runs as a queued job after collection).

- [ ] Host unhandled-rejection tracking (`HostPromiseRejectionTracker`) and
      the `unhandledrejection` / `rejectionhandled` events
- [ ] Drive FinalizationRegistry cleanup jobs from the event loop, not only
      from a manual job drain (*verify*)
- [ ] Error reporting for uncaught exceptions in tasks (`window.onerror`
      shape), without aborting `core`'s session loop
- [ ] Heap/GC budget behavior for long-lived pages with timers (*verify*:
      limits were designed for run-to-completion scripts)

## C. Web platform globals (P1)

None of these were found in production code (only `globalThis` exists).

- [ ] `window` / `self` / `top` / `parent` aliasing of the global
- [ ] `console` (log/warn/error/...) routed to the devtools/MCP surface
- [ ] `location`, `history`, `navigator`
- [ ] `URL`, `URLSearchParams`
- [ ] `TextEncoder` / `TextDecoder`, `atob` / `btoa`
- [ ] `structuredClone`, `performance.now`
- [ ] `AbortController` / `AbortSignal`
- [ ] `localStorage` / `sessionStorage` (policy + persistence design first)
- [ ] `EventTarget` / `Event` / `CustomEvent` as general objects

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
- [ ] CORS, credentials/cookies, cache, cancellation
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

## Suggested order

1. A (event loop + timers) - everything async depends on it
2. `console` + unhandled-rejection reporting (B, C) - cheap, unblocks debugging
3. D (DOM surface) and C (globals) - needed to run ordinary page scripts
4. E (fetch/XHR) - needs A
5. F (end-to-end smoke test, shell) - validates the whole stack
