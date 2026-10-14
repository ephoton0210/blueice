# BlueJS Browser-Runtime Gaps

[← Back to Phase 13 plan](PLAN.md)

**Status**: Analysis for a dedicated branch, to be cut from `main` after
PR #25 (`feature/test262-remaining-failures`) merges. Nothing here is
implemented on `feature/browsable-web-mvp`.

**2026-10-15 integration update:** `feature/bluejs-browser-integration-gaps`
has merged the product and BlueTS lines and completed the host-owned bounded
microtask checkpoint and session-driven timer increments. The owner-selected
File DOM profile installs callable-only timers, `queueMicrotask` and animation
frame APIs, with exact v12 typings and private v59 scheduling/render replies.
The frame entry remains separate from ordinary tasks. Real JavaScript and BlueTS counter
pages prove matching frame/Representation generations. M1d bounds page-owned
jobs, waiting reactions/requests and retained scheduling payloads with an
account shared by ShadowRealms. M1e charges that payload to the initiating
Core tab and refreshes it after task/render entries; broader host-buffer and
aggregate resource accounting remain open. M1f replaces permanent Promise roots
with live-owner tracing, roots work through publication and restores awaited
for-await cleanup. The public page regressions cover bounded managed heap and
native scheduling payload during repeated timer polls. M1g adds document-owned
registry cleanup tasks, automatic GC notifications at host checkpoints and a
shared 64-task browser/cleanup cap; the milestone record tracks final gates. See the
[`browser-integration milestone record`](BROWSER_INTEGRATION_TODO.md#milestone-record)
for current evidence and remaining task-ordering/resource work. The tables
below retain the original analysis baseline except for the dated M1f/M1h/M1i rows.

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
| Promise collectibility | M1f traces status/reaction edges through live heap owners and sweeps dead records/continuations. | Twelve lifecycle cases, 4,000 timer polls under a 1 MiB heap and 64 native-string payload polls cover bounded retention; the milestone records the full Test262 gate. |
| In-flight Promise registration | M1f roots queued/running jobs, outstanding module work and host waits; fixes publication roots in async setup, AsyncFromSync and `Array.fromAsync`. | Retained async invocations keep their Await frames alive; orphaned cycles can die. Existing allocation-ceiling scans verify each failure path. |
| `AsyncIteratorClose` opcode | M1f appends dedicated close/finish opcodes and uses awaited for-await loop finalizers. | Stream iteration/tee, pending cancellation under major GC and exact job ordering cover the behavior dropped in the merge. |
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
| Task sources / ordering across timers, network, UI events | M1i fairly arbitrates existing timer/browser groups within one bounded turn, and Core offers rendering after each selected task. Full logical source associations and network/UI ordering remain open before Fetch. |
| Microtask checkpoint owned by the host | M1h adds per-callback cleanup during native DOM dispatch, with one 256-job budget and preserved dispatch fuel/frame roots. Script-driven dispatch waits for its enclosing JavaScript; see the dated milestone record for acceptance. |
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
