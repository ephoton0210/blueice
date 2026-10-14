# BlueJS Browser-Runtime Gaps

[← Back to Phase 13 plan](PLAN.md)

**Status**: Analysis for a dedicated branch, to be cut from `main` after
PR #25 (`feature/test262-remaining-failures`) merges. Nothing here is
implemented on `feature/browsable-web-mvp`.

**2026-10-14 integration update:** `feature/bluejs-browser-integration-gaps`
has merged the product and BlueTS lines and completed the host-owned bounded
microtask checkpoint and session-driven timer increments. The owner-selected
File DOM profile installs callable-only timers, `queueMicrotask` and animation
frame APIs, with exact v12 typings and private v58 scheduling/render replies.
The frame entry remains separate from ordinary tasks. Real JavaScript and BlueTS counter
pages prove matching frame/Representation generations. M1d bounds page-owned
jobs, waiting reactions/requests and retained scheduling payloads with an
account shared by ShadowRealms; full host/Core accounting remains open. See the
[`browser-integration milestone record`](BROWSER_INTEGRATION_TODO.md#milestone-record)
for current evidence and remaining task-ordering/resource work; the tables below are
the original analysis baseline.

BlueJS is measured against ECMA-262/402 (Test262 99.995%). That says the
*language* is nearly complete; it says nothing about whether the engine is a
usable *page runtime*. This document is the inventory of what a script-driven
page needs from the engine and does not get today, so the work is planned once
instead of discovered one feature at a time.

## Method and its limits

Found by searching the Rust sources for each identifier (2026-10-14), then
reading the hits for the ambiguous ones. A name that appears only in a comment
or in the Test262 host (`vm/test262/cases.rs` defines its own `setTimeout`) is
counted as **absent**. This is a first pass: the first deliverable of the
branch is a *global-inventory test* that executes `typeof X` for every name
below in a page-host VM and pins the exact result, the same discipline
`DOM_EVENT_BINDINGS.md` uses. Where this document and that test disagree, the
test wins.

Two layers are deliberately kept apart, because the owner differs:

- **Engine** (`backend/bluejs`): language/runtime services every host needs.
- **Page host** (`backend/core/engine` + launcher): DOM, network, storage,
  anything that needs the document, an origin or the gatekeeper. The engine
  provides the *mechanism* (native-function kind, task queue, rooting), the
  host owns the *policy* and decides which profile installs it.

## A. Engine correctness and resource model

| Item | State | Why it matters |
|---|---|---|
| Promise collectibility | Every Promise is a root for the life of the VM (Test262 line). `promise_gc_lifecycle` has 4 tests asserting collectibility that fail. | Long-lived pages that poll or stream grow until the heap limit. Acceptance test: bounded memory in a long-running poll loop. |
| In-flight Promise registration | Not done. An `owned_edges` port was tried and reverted: it made the Test262 async/generator paths collect Promises that were still in flight (10 lib + several integration failures). | The prerequisite for the row above: every async/generator/host-wait path must register the Promise it is still using. |
| `AsyncIteratorClose` opcode | Dropped in the merge; Test262 line closes async iterators another way. | Re-check the product tests that depended on it (stream tee, `for await` cleanup); adopt or document equivalence. |
| Module dependency error order | `vm/modules.rs` uses unordered sets, so which equally-valid `SyntaxError` a cyclic graph reports is nondeterministic. | Pages with module graphs get run-to-run differences. Fix with an ordered set. |
| Per-document limits | Heap byte limit and fuel exist. No per-document cap on timers, pending tasks, queued microtasks, or host-held bytes. | A runaway page must degrade itself, never the session (Phase 27 decision 1). |

## B. Event loop and scheduling

Parked work: `wip/bluejs-web-timers` holds a complete, tested first slice
(`setTimeout`/`setInterval`/`clearTimeout`/`clearInterval`/`queueMicrotask`,
bounded pending count, argument forwarding, nested-timer clamping, a host
`next_timer_due()`), 11 tests passing, delivered by the existing
`run_browser_task_turn`. It is the starting point, not a finished design.

| Item | State |
|---|---|
| `setTimeout` family, `queueMicrotask` | Implemented on the WIP branch; absent on `main`. |
| `requestAnimationFrame` / `cancelAnimationFrame` | Absent. Needs a frame-tick source from the session, not wall time. |
| `requestIdleCallback` | Absent; low priority, may be a fixed-budget stub. |
| Task sources / ordering across timers, network, UI events | Single bounded queue today; no source priority. Decide before fetch lands. |
| Microtask checkpoint owned by the host | Present for browser-task turns; verify it also runs after each DOM event dispatch. |
| Host wake-up (`next_due`) | Only on the WIP branch; the launcher's `DragTasksFlushed` reply needs a matching field. |

## C. Web-platform globals

State is **absent** unless stated. "Host" means the page host owns it.

| Group | Names | Layer |
|---|---|---|
| Diagnostics | `console`, `reportError` | Engine mechanism, host sink (output must be bounded and untrusted-framed) |
| Encoding | `TextEncoder`, `TextDecoder`, `atob`, `btoa` | Engine |
| Cloning/identity | `structuredClone`, `crypto.getRandomValues`/`randomUUID`, `performance.now`/`timeOrigin` | Engine (clock and entropy must be injectable for tests) |
| URL | `URL` present (`vm/web_url.rs`); `URLSearchParams` absent | Engine |
| Cancellation | `AbortController`/`AbortSignal` present | Engine |
| Binary/files | `Blob`, `File`, `FileReader` present | Engine |
| Streams | `ReadableStream` present; `WritableStream`, `TransformStream`, `CompressionStream` absent | Engine |
| Events | `EventTarget`, `CustomEvent` present; `DOMException` present | Engine |
| Messaging | `MessageChannel`, `BroadcastChannel`, `Worker` | Host; Workers probably out of scope |
| Network | `fetch`, `Headers`, `Request`, `Response`, `FormData`, `XMLHttpRequest`, `WebSocket`, `EventSource` | Host (policy, gatekeeper, cookies); engine supplies Promise/stream plumbing |
| Storage | `localStorage`, `sessionStorage`, `indexedDB` | Host |
| Observers | `MutationObserver`, `IntersectionObserver`, `ResizeObserver` | Host; need DOM mutation records |

## D. Suggested order inside the branch

1. Global-inventory test (pin today's truth).
2. Promise collectibility with the long-running-poll memory test written first.
3. Land the timers slice; add `requestAnimationFrame`; settle the task-source
   order.
4. Encoding, `URLSearchParams`, `structuredClone`, `console`, `performance`,
   `crypto` (small, independent, all engine-side).
5. Writable/Transform streams (needed by `fetch` bodies).
6. Re-run the full Test262 inventory; a regression there blocks the merge.

Items in C marked *Host* are **not** part of this branch. They are Phase 27 M2
to M4 and use the mechanisms above.

## Non-goals

- Passing a web-platform-tests suite. Claims stay "this fixture works".
- Workers and SharedArrayBuffer-backed threading in page scripts.
- Replacing the capability-scoped, owner-selected profile model: a global
  existing in the engine does not make it visible to a page.
