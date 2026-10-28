# BlueJS browser-integration TODO

BlueJS is a near-complete ECMAScript engine (Test262: 102,921 / 102,926 modes
pass). What keeps it from running real web pages is the **browser host layer**
around it. This file is the **current work list**. The full history (every
dated progress note, per-milestone checkpoint, verification count and evidence
path, formerly this file's 13,939 lines) is kept verbatim in
[`BROWSER_INTEGRATION_MILESTONES.md`](BROWSER_INTEGRATION_MILESTONES.md); item
names below (M1a–M1z, M0a, M1q …) refer to it.

Related: [`PLAN.md`](PLAN.md), [`DOM_EVENT_BINDINGS.md`](DOM_EVENT_BINDINGS.md),
[`../research/js-bytecode-eventloop.md`](../research/js-bytecode-eventloop.md),
[`../phase-17-automation-devtools-and-ajax/PLAN.md`](../phase-17-automation-devtools-and-ajax/PLAN.md),
[`../phase-27-browsable-web-mvp/PLAN.md`](../phase-27-browsable-web-mvp/PLAN.md).

**Legend.** **P0** blocks everything else, **P1** needed for ordinary sites,
**P2** compatibility/polish. *Engine* = mechanism in `backend/bluejs`; *Host* =
policy owned by `backend/core` / launcher (origin, gatekeeper, cookies, which
profile installs it). A global existing in the engine never makes it visible to
a page: the owner-selected profile does. An item is only schedulable with an
acceptance condition (section I): a test that fails today through the real
public interface, not a checked box. *(product line)* = done on the product
line and integrated here; do not re-implement.

## Rules

- **Commit rule.** Commit only after an existing item *and its stated
  acceptance conditions* are complete. A helper fix, an implementation
  checkpoint or a larger selected-test count is not completion. Keep unfinished
  work in the working tree, record its real status and keep failing evidence.
  Do not commit partial follow-ups, and do not add features to stand in for
  resolving recorded failures. Earlier checkpoint commits are historical
  records, not completed items.
- **Validation constraint.** Use offline static checks and fixed, finite
  unit/functional cases with tiny in-memory fixtures and numeric admission
  controls (not represented large allocations). No resource-exhaustion or
  infinite-execution probes, no network/service/listener/Core/MCP replay, no
  privileged validation or process killing. Broader acceptance stays open under
  this constraint. After a cybersecurity warning or approval rejection, inspect
  existing local evidence first and do not rephrase or reroute the operation;
  record affected checks as pending. Push and process inspection stay excluded.
- **Architecture priority.** Scheduling ownership, timer/Promise
  collectibility, cancellation/navigation cleanup and per-document resource
  containment come before diagnostic-coordinate precision. Timer/Promise roots
  span callbacks and realms, so wrong ownership either keeps a discarded
  document alive or releases pending work. M1q's diagnostic refinements are
  parked until these dependencies close.

## Current focus: native ownership and GC lifecycle audit (uncommitted)

Goal: every native allocation a document's scripts can cause (queues, roots,
snapshots, argument/text copies, registries, caches) is admitted against that
document's budget before publication, charged to its *original* owner, and
refunded exactly on completion, refusal, cancellation or navigation. Work lives
in the uncommitted tree on `feature/bluejs-browser-integration-gaps`
(`*_storage.rs` files and `tests/fixtures/*`).

Accepted so far: M1a–M1u (and the items marked done below). **Not accepted:**
M1v–M1z, below, until the whole `blueice-bluejs` suite is green.

| Gate | What it requires | State |
|---|---|---|
| M1v active native execution payloads | Active locals/arguments/displaced caller frames share document admission; completed/unwound frames release | implemented in tree; some conservation tests red |
| M1w document callback-timer counts | 1,024 shared pending callbacks across root + child realms; refund on delivery/cancel/destroy | implemented; integrated acceptance open |
| M1x scheduling capacity admission | Refuse growth before publishing a task/reaction/registration/handle; appends that fit add no charge | implemented; integrated acceptance open |
| M1y async module dependency ownership | Charge both directions of waiting graph edges; batch admission before publication | open |
| M1z active execution control capacity | Operand/binding/scope/eval/completion containers in the budget across call/eval/generator/async transfers | many producers done; remaining producers and integrated acceptance open |

**Working-tree test state (2026-10-20).** `cargo test -p blueice-bluejs`:
7,593 passed, 0 failed (lib 4,136, integration, doc-tests); `cargo clippy -p
blueice-bluejs --all-targets -- -D warnings` and `cargo fmt --check` are clean.
Not yet run on this tree: the full pinned Test262 inventory, workspace-wide
tests and coverage. One lib run in five showed a single failure that was not
captured and did not recur in three further runs (likely a timing test under
load); treat the suite as green but not yet proven stable. HEAD itself was red
(49 lib failures, mostly a worker binary missing in a fresh worktree, plus
accounting tests), so these fixes are not all attributable to this audit.

Causes found, so they are not rediscovered:

1. *Baseline cost:* `AsyncGeneratorControl` grew and sat inline in every heap
   object; it is boxed now (`Vm::new` minimum budget is 512 bytes again).
2. *Stale test budgets:* temporary `RealmCallRoots`, retained source text,
   installed program code and owned promise-record/table storage are charged to
   the document account. Fixtures that budget only the old owners must add
   them, or (better) assert conservation by rerunning the same program, or by
   comparing against an unlimited-room twin. A fresh bounded realm owes 96 bytes
   of root-table storage, not zero. Compare runs with equal-length source.
3. *Real defects fixed:* `parseInt`/`parseFloat` now validate the entry owner
   before conversion and charge their UTF-16/UTF-8 copies; console previews and
   traces borrow `name`/`message` allocation-free (`Heap::own_string_property_ref`);
   `prepare_gc_trace` judges terminal cleanup *before* binding (a newly
   over-budget document was treated as cleanup and let through);
   `run_due_host_timers` restores fuel on refusal; `finish_async_delegation`
   no longer panics when the completed value does not fit back into the frame
   (`Heap::try_set_generator_state` returns the unpublished state; the
   delegation is restored atomically); `import.source()` rejects an
   unresolvable specifier like `import()` instead of raising a hard error (this
   one already failed at HEAD); an absent ShadowRealm ancestor reports the
   boundary's own error.
4. *Post-checkpoint GC* triggers on a document's **own** scheduling usage.
5. A result is rooted only until the next execution (which collects first), and
   an unreferenced child realm is reclaimed: fixtures must root what they keep.
   A document VM keeps `WeakRef`/`deref` targets until its owner's checkpoint.

The assertions that had been loosened to fit the new ownership model are exact
again, rebuilt on twin references instead of enumerating owners: the ShadowRealm
module-context name test (twin of two name lengths, two copies), the `page_*`
child-realm and failed-frame residues (twin runs with equal-length sources, equal
refusal depth), and the shared agent-registry charges (twin key sizes). Remaining
intentional behaviour changes: a refused host-neutral registry attachment and an
over-cap set of child heaps now surface as terminal `SchedulingLimit`.
`diagnostic_sources` origin-copy counts are exact for shared provenance (0, 0, 0,
1, 1). Two fixtures were rebuilt around the new transient-copy cost of timer
registration (`host_scheduling_limits`).

Committed 2026-10-20 at the owner's request as a green but **unaccepted**
checkpoint (7,593 passed, 0 failed, clippy/fmt clean). M1v–M1z stay open.
The intermittent lib failure is identified and fixed: 2 of 40 full runs (and 5 of
300 single runs) failed `thunk_origin_storage_nested_guard_defers_compaction_until_outer_completion`,
which pinned `HashMap::capacity() == 14` after deletions. hashbrown's insertion
headroom then depends on tombstone placement, i.e. on the random hash seed. The
test now asserts the allocation ledger is unchanged and capacity is not the
compacted size (0 of 1,000 single runs fail; `dependency_storage`,
`disposal_map_storage` and `await_registration` showed 0 of 200 each). Rule:
never pin a RandomState map's `capacity()` after removals.

Measured on the committed tree (2026-10-21): full pinned Test262 inventory
102,921 pass / 0 fail / 1 `stale_corpus` / 4 `excluded` of 102,926 modes; workspace
tests and `clippy --workspace -D warnings` green. The workspace run found a real
defect the bluejs suite had missed: `HostCodeCache::prune` (and `grow`/`shrink`)
ledgered its tables by `HashMap::capacity()`, which drops with tombstones and
rises again with no reallocation, so `release_to` could be asked for more than it
held and panic the page child ("cleanup cannot acquire native storage",
about half of `real_mcp_keeps_queued_reference_and_nested_assignment_sources`
runs). The cache now records each table's real slot count; a 200-seed
regression test fails on the old code. Coverage was run with Homebrew `llvm@22`
(`LLVM_COV`/`LLVM_PROFDATA`, same LLVM as rustc) and, under `zsh`, with
`NO_BG_NICE` (a background job's +5 niceness fails the three launcher priority
tests).
Workspace coverage gate on that tree (`cargo llvm-cov --workspace`, `--fail-under-lines 90`,
no failing test): lines 93.50% (293,263 / 313,664), functions 93.15%
(24,497 / 26,298), regions 92.97% (449,576 / 483,596).

Real Core/child two-tab long-running acceptance (2026-10-22, 600 s,
`BLUEJS_ACCEPT_SECS=600 cargo test -p blueice-mcp-server --test core_process
long_running -- --ignored --nocapture`, `backend/mcp-server/tests/core_process/long_running.rs`):
a counter tab (`setInterval`) and a Promise/timer churn tab on one launcher-managed
Core. Tab A's `generation` and counter advanced together at every 2 s sample
(7,190 frames, strictly increasing); tab B's churn advanced to 32,300 ticks; resident
memory of Core plus the BlueJS child stayed at 81.2 → 81.9 MiB (warm baseline
81,856 KiB, steady-state peak 81,920 KiB). Tab B then exhausted its own document
budget with tab A still advancing, recovered on navigation, and closing it left
the total at 89,984 KiB (+8 MiB over the warm baseline, within the 32 MiB bound;
the cause, allocator retention versus a retained document, is not established).

Frame/pixel comparison (2026-10-27, `long_running.rs`): a second direct client
of the shared Core stands in for a human's frontend. Connected before any tab
renders, it maps every announced frame at once and pairs a tab's
`GetRepresentation` with the frame of the generation that representation
reports. In a 300 s run, 147 samples of tab A matched the frame/representation
generation and were pixel-identical to an independent `blueice-engine` render of
exactly the counter text the representation showed ("count K"); tab B's frame
equalled a fresh render of its page at every sample, and again after it
recovered; the two tabs' pixels differed. (A frame is announced by broadcast, not
with the request id; the MCP `screenshot` tool maps only its cached reply frame,
so it cannot catch a page that re-renders every 50 ms.)
Windowed frontend (2026-10-27, `frontend_window_shows_the_cores_frame`, opt-in,
needs a display): the real `blueice-frontend` binary joins the same launcher-owned
Core (`--socket`, `--url` the counter page) with `BLUEICE_FRONTEND_SNAPSHOT` set,
which writes the window's actual `0x00RRGGBB` buffer to a PNG. In a run on macOS
27.0, 20 of 20 window pictures (1600x1200, taken ~0.7 s apart) were pixel-identical,
below the 34-pixel tab strip, to an independent engine render of "count K" for a
K within a few ticks of the Core's newest counter value, so the window shows
the Core's frame, not a separate render. Not covered: a person looking at the
window, and the second tab (the window shows one selected tab and the test
sends it no input).

Stability soak (2026-10-22/23): 1,000 consecutive full `blueice-bluejs` lib runs
(4,137 tests each, 3 workers in parallel for load) on the tree at commit
`152ba271f`: 1,000 pass, 0 fail. It found, and the fixes at `01f14bd57`
and `152ba271f` closed, every flake seen so far: all were `HashMap::capacity()`
drifting with tombstones under the random hash seed (a test pinned it, a
fixture shaped headroom with the wrong accessor, and one real ledger bug in
`HostCodeCache`). Rule for any shared test helper: stress every caller
(200+ runs each) before committing a change to it. Scope: lib tests only;
integration targets were run once (workspace) plus the focused stress runs
above.

Remaining before M1v–M1z can be called accepted: a person looking at the window
(and switching to the second tab), and an owner decision to accept.

## A. Event loop and scheduling (P0)

Done (see milestone record): macrotask queue draining microtasks (M1a/M1i/M1j);
`setTimeout`/`setInterval`/`queueMicrotask` and timer render-pass entry (M1b);
`requestAnimationFrame` (M1c); core/child timer ownership and wake-ups (M1b);
document scheduling admission and Core payload accounting (M1d/M1e);
browser-task admission and cleanup scheduling (M1g); native DOM callback
cleanup (M1h); owner-installed async operations (M1k); aggregate live-realm
heap, agent registry accounting (M1t/M1u).

Open:

- [ ] `MessageChannel` / `postMessage`.
- [ ] Timers and queued tasks dropped on navigation, bounded per tab, and
      accounted to the initiating tab — timer/frame lifetime and counts are
      covered; complete queued-task and host-byte accounting is not.
- [ ] Task-source ordering between timers, network completions and UI events
      (Core's native UI source association and real request-service ingress are
      needed before `fetch`).
- [ ] Per-document caps on pending timers, tasks, microtasks and host-held
      bytes so a runaway page degrades only itself (M1w–M1z above).

## B. Promise, GC and error reporting (P0/P1)

Done: Promise collectibility and awaited `AsyncIteratorClose` (M1f); host
unhandled-rejection tracking and events (M1p); FinalizationRegistry cleanup as
engine tasks (M1g); uncaught page exceptions as `ErrorEvent`/`onerror` (M1o);
native pressure inside callbacks (M1r); discarded ShadowRealm reclamation (M1s).

Open: broader native control/service payload accounting (A, M1v–M1z).

## C. Web platform globals (P1)

Done: `window`/`self`/`top`/`parent` (M1l); `console` (M1m); `URL`,
`AbortController`/`AbortSignal`, `EventTarget`/`Event`/`CustomEvent`, `Blob`/
`File`/`FileReader`, `ReadableStream`, `DOMException` *(product line)*;
`reportError` (M1o); rejection-event globals (M1p); global-inventory test (M1l).

Open:

- [ ] `location`, `history`, `navigator`
- [ ] `URLSearchParams`
- [ ] `TextEncoder` / `TextDecoder`, `atob` / `btoa`
- [ ] `structuredClone`, `performance.now`
- [ ] `localStorage` / `sessionStorage` (policy + persistence design first)
- [ ] `WritableStream` / `TransformStream` / `CompressionStream` (needed by
      `fetch` bodies)
- [ ] `crypto.getRandomValues` / `randomUUID`; injectable entropy and clock

## D. DOM bindings (P1)

Bound today: `document.getElementById`, `createElement`, `createTextNode`,
`textContent`, `appendChild`, and `click` listeners.

- [ ] `querySelector` / `querySelectorAll`, `getElementsBy*`
- [ ] Attributes (`getAttribute`, `setAttribute`, `dataset`), `id`,
      `className`, `classList`
- [ ] Inline `style` and `getComputedStyle`
- [ ] Tree mutation: `insertBefore`, `removeChild`, `replaceChild`, `remove`,
      `innerHTML` / `outerHTML`, `cloneNode`
- [ ] Traversal: `parentNode`, `children`, `firstChild`, `nextSibling`, …
- [ ] Events: `input`, `change`, `submit`, keyboard, focus, capture/bubble,
      `stopPropagation`, `DOMContentLoaded`, `load`
- [ ] Form state: `value`, `checked`, `FormData`
- [ ] Layout reads: `getBoundingClientRect`, `offset*`, `scroll*`
- [ ] `MutationObserver`, `IntersectionObserver`, `ResizeObserver`
- [ ] Generated `lib.dom.d.ts` kept in lockstep with each installed binding

## E. Networking (P1, Phase 17; depends on A)

- [ ] `fetch`, `Request`, `Response`, `Headers` over `core`'s request service
- [ ] `XMLHttpRequest` including event ordering
- [ ] `FormData`; urlencoded and JSON request bodies
- [ ] CORS (fail closed on unsupported policy), cache, `AbortSignal` cancellation
- [ ] **Cookie jar** in the request service (`Secure`, `HttpOnly`, `SameSite`,
      `Set-Cookie` round trip on form POST). Nothing exists today; without it
      there is no login or session.
- [ ] **Gatekeeper review for script-initiated requests**: same two-stage,
      fail-closed review as navigation; a typed request to `blueice-net`, never
      a raw socket
- [ ] **Private-network URL policy** (loopback, link-local, RFC 1918): open gap
      in the gatekeeper rule base and a precondition for any page request
- [ ] Response bodies are untrusted page data: framed as such on MCP, byte and
      time limits, a readable request-lifecycle record
- [ ] Server-Sent Events; WebSocket (Phase 20)

## F. Page script loading and integration (P0/P1)

Done: `blueice_ipc::script` wired to a real BlueJS host process; end-to-end
smoke test for JavaScript and BlueTS `<script>` fixtures; a script DOM change
publishes a frame in the same render pass with the same `generation`.

- [ ] *Verify*: `<script src>`, `defer`/`async`, `type=module`, inline handlers
      (`onclick=`), execution order vs. parsing
- [ ] `bluejs` shell (REPL + batch) and the full Node differential harness
- [ ] `bluejs_run` / `bluejs_analyze` MCP tools (with Phase 12)
- [ ] AI-facing capability summary and Phase 7 gatekeeper script hooks
- [ ] **Default-on script execution** is a reviewed decision, not a flag flip:
      scripts run only behind `--inline-bluejs` or page-host options today. The
      core selects the supported profile; the page never does.

## G. Engine limitations (P2)

- [ ] No JIT; no performance measurements against Node/Chromium
- [ ] `regress` deviations: `(?i:...)` modifier groups, others in
      `TEST262_ANALYSIS_REPORT.md`
- [ ] Intl formatter constructors partly open (`INTL_CONFORMANCE.md`)
- [ ] Module static-error discovery uses unordered `HashMap`/`HashSet`: which
      `SyntaxError` a cyclic graph reports is nondeterministic
- [ ] Parser/compiler nesting and native data recursion are outside the
      native-stack call-depth guard
- [ ] Windows native-stack query branch never run; 1 MiB main-thread stack
      untested
- [ ] General reverse-buffer-mirror for cross-realm TypedArrays not built

## H. BlueTS / BlueTSC dependencies on BlueJS

BlueTSC is a Rust compiler that does not run on BlueJS; BlueJS only executes
its output via `backend/bluets-bluejs`. Most remaining BlueTS work is
compiler-side ([`../phase-18-bluets/TODO.md`](../phase-18-bluets/TODO.md)).
These items are where BlueTS is blocked or constrained by a BlueJS/host gap.

Done: merged host-profile compatibility (M0a); ambient value queries and
missing-intermediate-member checking (M1n); typed timers/`queueMicrotask`
(M1b) and typed `console` (M1m).

Blocked by host gaps (C, D, E):

- [ ] **Typed host API stays tiny.** `lib.blueice.d.ts` is generated from
      implemented bindings only; every binding added in C/D needs its typings
      entry in the same change.
- [ ] **Typed asynchronous web APIs / `fetch`** remain incomplete/absent until E.
- [ ] **Original positions for uncaught errors and rejections** (M1q): classic
      JavaScript root/block/function coordinates and copied known locations are
      verified; direct/emitted BlueTS, precise expressions/parameters and queued
      origins remain open.

BlueJS capabilities BlueTS should use or still needs:

- [ ] Native decorators (Stage 3, matches TS 5.x; legacy
      `experimentalDecorators` still need an emitted `__decorate` helper)
- [ ] Class lowering to the BlueJS class AST (bridge work; no engine gap)
- [ ] CommonJS and `import()` host support (`require`/`module`/`exports`,
      page-level `HostLoadImportedModule`, host-defined `import.meta`)
- [ ] Runtime contract helper is string-only; strict boundaries over numbers,
      records and arrays need a richer helper and a structured value format
- [ ] Source positions for runtime errors (frame-position API *verify*)
- [ ] Debugger coverage for TypeScript sources (line/conditional breakpoints,
      pause-on-exception, stepping across `await`)
- [ ] ES library typings vs. engine (Temporal etc. are not in TS 5.9.3 libs):
      conformance check plus a decision on typing engine-only features
- [ ] Resource limits as TypeScript semantics (fuel, heap budget, stack guard
      raise `RangeError`/limit errors): document in the BlueTS runtime contract

## Suggested order

1. A/B ownership and GC: native audit above to green; timer callbacks/
   arguments, Promise reactions and suspended work live only as required.
2. Resource containment and task-source integration: complete initiating-tab
   queue/native-buffer accounting and timer/network/UI ordering before adding
   producers; exhaustion isolates the failing document.
3. Real Core/child long-running and two-tab acceptance for those contracts.
4. C/D platform surfaces, then E Fetch/XHR on the established scheduling and
   request-service boundary, then F ordinary-session acceptance.
5. M1q diagnostic precision; then H joint items (event loop/console → rejection
   reporting → DOM typings → classes/decorators → CommonJS/`import()`).

## I. Ownership and acceptance

| Area | Layer | Acceptance (must fail today, through the real interface) |
|---|---|---|
| Host microtask checkpoints | Engine, host script cleanup | Two page script declarations observe the first one's Promise work before the second starts, including after a throw and with debugger scheduling; runaway jobs return a resource failure through the public page runtime |
| Timers, `queueMicrotask`, `requestAnimationFrame` | Engine, host clock | A counter page updating by `setInterval` yields N frames with strictly increasing `generation`; each matching `Representation` agrees |
| Promise collectibility | Engine | Bounded managed heap over a long poll loop; `promise_gc_lifecycle`; full Test262 inventory unchanged |
| Platform globals | Engine, installed by profile | The global-inventory test in C |
| DOM read/write, events | Host (page profiles) | Fetch JSON, build a list with `innerHTML`, click and type to update it: frame pixels, `get_page_representation` and an MCP client all agree |
| `fetch` / XHR / cookies | Host, over `blueice-net` + gatekeeper | A first-party fixture GET/POST with a `Set-Cookie` round trip; a request to a private address is refused with a visible diagnostic |
| Default-on scripts | Host | The same fixture works from an ordinary launcher session with no extra flags |

Mapping to Phase 27 (`phase-27-browsable-web-mvp/PLAN.md`): M1 = sections A and
B, M2 = D, M3 = E, M4 = E's cookie jar and F's default-on decision, M5 = the
acceptance fixture above. Every binding added in C or D must update the
generated typings in the same change.
