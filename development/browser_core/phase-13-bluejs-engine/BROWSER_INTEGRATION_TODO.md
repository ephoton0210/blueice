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

**Integration progress (2026-10-16).** This checkout has merged the product
and BlueTS lines. The baseline descriptions below remain the original gap
inventory; the dated milestone records identify what has actually been
implemented and verified on `feature/bluejs-browser-integration-gaps`.

**Architecture priority — updated 2026-10-16.** Scheduling ownership,
timer/Promise collectibility, cancellation/navigation cleanup and per-document
resource containment take precedence over diagnostic-coordinate precision.
M1r through M1u close the reproduced native-pressure, discarded-realm,
aggregate managed-heap and agent-registry failures through real runtime/Core
boundaries. Next come complete native control/task-source/host-buffer
accounting and native UI/network ingress, then the long-running two-tab
acceptance. M1q's diagnostic refinements remain parked until these architectural
dependencies are closed. Its interrupted 191-target replay is incomplete and
does not constitute a milestone or a new acceptance gate.

M1s began with this public ownership failure:
discarded ShadowRealm instances become unreachable (`WeakRef.deref()` returns
undefined), but their VM tables retain global Promise results and their native
charge. A timer adding one such 400,000-character result per callback grows
from 800,912 to 8,005,232 native bytes and fails on callback 11. The accepted
realm-reclamation milestone below closes this failure; its original evidence
remains separate from the accepted replay.
Evidence: `target/orphan-realm-probe-source-20261016.json` and
`/private/tmp/blueice-orphan-realm-probe-20261016.log`.

**In progress — M1x scheduling capacity admission (2026-10-16).**
Five tiny-container regressions reproduce admission errors in timer, frame,
Promise-job and Promise-reaction storage. A fixed four-slot forecast permits
the ninth entry to be published before an eight-slot growth is refused;
it also rejects a timer that fits an existing slot. Admission now plans the
requested slot growth, reserves it explicitly and rechecks any allocator-
reported excess before publication. Timer argument/root forecasts exclude
the callback and delay from the argument vector. The initial four-container
checkpoint passes 100 selected cases. The browser/cleanup/rejection follow-up
adds twenty failing-before-fix groups, including FileReader requeue/cancellation
and timeout clock ordering. The reviewed scheduling vectors/deques now reserve
capacity before publication. Follow-up validation passes 161 selected cases,
formatting and offline workspace/all-target Clippy. Final integrated/resource
acceptance remains open.

**In progress — M1w document callback-timer counts (2026-10-16).**
Callback timer limits previously applied to each realm separately, while the
document ledger had no timer count. The shared account now admits at most
1,024 callback registrations across attached realms and exposes
`HostSchedulingUsage.pending_timers`. One-shots leave the count on delivery;
intervals remain until cleared. Document failure is terminal, while standalone
VMs keep their per-realm catchable `RangeError` policy. Seven unit groups pass
on small live queues, scalar admission and the finite default count.
Clock-hook reentry also reproduces reversed enqueue order; clock sampling now
precedes final admission and serial assignment. Current bounded validation
passes 94 cases, zero failures or ignored; the page-runtime default-count case
verifies refusal, a healthy neighboring tab and navigation recovery. Formatting
and offline workspace/all-target Clippy pass. Thirteen other public pressure/
limit cases remain filtered; integrated/full-source acceptance stays open.

**In progress — M1v active native execution payloads (2026-10-16).**
Two public regressions fail on M1u. Seventeen recursive frames retain at least
13,600,000 UTF-16 bytes in local variables or arguments, yet complete with
203,102 document-managed bytes and zero native bytes. Suspended continuations
already have payload accounting; active VM fields and displaced Rust-owned
caller frames need admission, release and shared document ownership. The first
implementation passes both original failures and the operand, cross-realm,
shared-owner and return/throw/eval release controls. Broader validation remains
incomplete, including native parameter-copy pressure and existing capacity
fixtures. GC-only roots now preserve object edges without copying primitives;
captured values already promoted into managed cells keep that ownership.
No accepted milestone is claimed yet. Red evidence:
`target/active-frame-red-source-20261016.json` and
`/private/tmp/blueice-active-frame-{probe,runtime-red}-20261016.log`.
The first expanded replay passes formatting/build/Clippy and 24 real-page
cases, but its async-return burst fails; it is not acceptance. Public follow-up
also exposes 8,000,000 stale bytes after a child frame fails. Cleanup now
reconciles released ownership without clearing failure. Async return tracing
finds accumulated callee completion records and caller pending returns copied
into child contexts; completion ownership and GC roots are being isolated
across ordinary, async and generator entries. All fourteen new public groups
and the five related scheduling/GC/realm suites pass. Parameter-environment
entry refusal now uses the common restoration path as well. An intermediate
405-executable and pinned Test262 replay was started. Its formatting,
workspace build/Clippy, fourteen public runtime groups, all 25 real-page cases
and 149 Core/child cases pass. Unchanged follow-up probes now refund the child
to its 400,168-byte registry baseline and complete all eighteen async-burst
polls with approximately 1.6 MB native usage. That replay was stopped after a
public pause-boundary probe retained 8,800,000 bytes at offset 88. A pause now
admits the preceding instruction's retained payload before handing it to the
host, without spending the paused instruction's fuel. All fifteen public,
seven page-runtime and twenty debugger controls pass, including the full root
offset matrix, near-cap pauses, refusal cleanup and one-unit fuel control.
A fresh complete replay remains required; prior partial runs are not accepted.
The latest replay was stopped following the owner's instruction to avoid
operations that could trigger cybersecurity controls. It completed 298 of the
405 selected Rust executables (4,525 passed, zero failed, 17 ignored), plus
formatting, workspace build/Clippy and test compilation. Test262 had not
started; no final input/artifact audit or acceptance summary was produced.
M1v remains unfinished. Do not repeat the reported probing or
seek elevated permissions to bypass security controls; continue with
repository-local static review and ordinary bounded unit checks.
Interrupted evidence: `target/active-frame-complete-source-20261016.json`,
`target/active-frame-complete-validation-20261016.json` and
`/private/tmp/blueice-active-frame-complete-acceptance-20261016.log`.
Subsequent static review and bounded unit tests close three call-entry gaps:
constructor arguments retain their lease during prototype getters and field
initializers; Proxy forwarding transfers one charge instead of multiplying it;
tail-position direct eval retains its source argument. Four new unit groups
and 82 related cases across eight public targets pass at the `6be49f4e2`
call-entry checkpoint without opening ports or running resource-exhaustion
probes. A further task-handoff correction removes duplicate timer, drag and
finalization input charges and releases consumed inputs before Promise
checkpoint jobs. The `deafa218a` task-handoff checkpoint passes seven unit
groups and 67 public timer/task/cleanup cases, with fifteen public pressure/
limit cases filtered out. The `0d0f09c0e` Promise/host-completion checkpoint passes
twelve handoff groups, four call-entry controls and 61 related public cases;
six host-async pressure/limit cases are filtered out. It also reconciles
continuation ownership after context restoration even when primitive sizes
are unchanged. These are implementation checkpoints, not final M1v acceptance;
see the record below.
Additional red evidence: `target/active-frame-refund-red-source-20261016.json`
and `target/active-async-burst-red-source-20261016.json`.
Pause evidence: `target/active-frame-pause-red-source-20261016.json`.

**Latest milestone — M1u agent-owned native registries (2026-10-16).**
Three public regressions reproduce uncharged `Symbol.for` storage through the
page runtime and the real Core child. One shared document-owned charge now
accounts for table capacity and retained UTF-16 keys/descriptions across
ShadowRealms, with admission before mutation. All nine public runtime groups
pass, including shared Promise/timer limits, repeated keys, child reclamation,
reentrant coercion, lossless UTF-16 and existing-policy attachment/refusal.
The original probe now returns `SchedulingLimit` with 8,000,412 committed
native bytes instead of retaining at least 25,600,000 bytes outside the ledger.
All 24 real-page and 149 Core/child cases pass, including copied totals,
release, uncatchable exhaustion, healthy-tab frames and navigation recovery.
Formatting and workspace/all-target build/Clippy pass. All 89 selected
Rust/Core/MCP executables pass 2,744 cases, zero failures and sixteen existing
ignored. Fresh complete Test262 matches all 102,926 modes with zero changed
source/result records and no new failure or timeout. See the
[M1u milestone record](#m1u--agent-owned-native-registries-2026-10-16).
Broader native control/service-buffer accounting remains open; M1q stays parked.

**Previous milestone — M1t document managed heaps (2026-10-16).**
Two public regressions fail on M1s: sixteen child heaps bypass a 1 MiB document
envelope, and child-host `heap_bytes` reports only 213,978 bytes despite at
least 12,800,000 bytes of retained child strings. A shared managed-storage
counter now charges allocation/growth, collection/shrink and VM release to
the creating document; public runtime stats expose the document total while
retaining root-collector diagnostics. All nine focused runtime groups pass,
including nested realms, six growth routes, shrink/release, queued cleanup,
two timer entries, caller garbage and a 1-versus-1,000-call GC control.
The same probe now refuses the 1 MiB envelope; with the default 16 MiB envelope
it reports 16,196,190 document bytes versus 217,502 root-heap bytes.
Workspace/all-target build and Clippy pass. All 23 real-page cases, 148 Core/
child cases and 321 launcher library cases pass, including copied aggregate
stats and managed-heap exhaustion with healthy-tab/navigation controls.
All 88 selected Rust/Core/MCP executables pass 2,733 cases, zero failures and
sixteen existing ignored, including all 1,205 BlueJS library cases and five
official MCP console cases. Fresh complete Test262 matches all 102,926 modes
with zero changed source/result records and no new failure or timeout. See the
[M1t milestone record](#m1t--aggregate-document-managed-heaps-2026-10-16).
Evidence: `target/document-heap-red-source-20261016.json` and
`/private/tmp/blueice-document-heap-{runtime,host}-red-20261016.log`.
After-fix evidence: `target/document-heap-after-probe-source-20261016.json`.

**Previous milestone — M1s realm ownership (2026-10-16).** Three
public regressions reproduce discarded timer realms, release after the last
wrapped-function owner is dropped, and an unreachable cross-heap wrapper
cycle. All three fail on M1r; the first fails on timer callback 11. The first
implementation traces instance/callable/task ownership at explicit VM
safepoints and releases foreign roots through deferred leases. The thirteen
page-GC groups, eighteen ShadowRealm cases and seventeen cleanup cases pass,
including queued-work/navigation controls and 64 polls through each timer
entry without observer scripts. The unified task entry initially still failed
on callback 19: strong traversal snapshots delayed release and inflated the
native-GC watermark. Weak traversal snapshots close that failure. All 22 real
Core page cases now pass, including JavaScript/direct BlueTS callbacks that
discard sixteen realms each and still publish three frames with matching
Representations. All 87 selected Rust/Core/MCP executables pass 2,721 cases,
zero failures and sixteen existing ignored; formatting and workspace/all-target
build/Clippy pass. Fresh complete Test262 matches the accepted baseline at all
102,926 modes with zero changed source/result records or new timeout. See the
[M1s milestone record](#m1s--shadowrealm-ownership-and-reclamation-2026-10-16).
Red evidence: `target/realm-ownership-red-source-20261016.json` and
`/private/tmp/blueice-realm-ownership-red-20261016.log`.

**Previous milestone — M1r native Promise pressure (2026-10-16).**
The first public timer/GC regression failed on its first callback: repeatedly
discarded 400,000-character Promise results exhausted the native scheduling
ledger before the callback's checkpoint. Managed-heap pressure alone does not
reclaim them. Rooted call-entry/return collection now passes the public native-
pressure/liveness controls and real JavaScript/BlueTS timer-burst pages while
retaining the document's hard limits. All 87 selected test executables pass
2,715 cases, zero failures and 16 existing ignored; formatting and workspace/
all-target build/Clippy pass. The accepted fresh full Test262 replay matches
M1p at all 102,926 modes with zero changed source/result records and no new
timeout. The first attempt's single wall-timeout and six passing isolated
controls remain separate evidence. See the
[M1r milestone record](#m1r--native-promise-pressure-during-timer-callbacks-2026-10-16).
Evidence: `/private/tmp/blueice-promise-native-pressure-red-20261016.log` and
`target/promise-native-pressure-red-source-20261016.json`. This checkpoint does
not close A's broader resource-accounting items or whole-realm reclamation.

**Previous host milestone — M1p Promise rejection tracking (2026-10-15).**
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

**Parked work — M1q original diagnostic source positions (in progress).**
The integrated lifecycle matrix has focused validation across all twenty
admitted producer/profile routes and normal/deferred execution. It retains
timer callbacks through a later script and observed major collection, then
checks delivery, cancellation or navigation, stale-generation refusal and
resource release. Focused runs pass all 480 lifecycle, forty map-budget and
eighty retained-filename cap/recovery contexts. The saved patch corrects ES5's
misclassification of `.catch(name)` as a catch declaration; its public emitted-
execution regression and actual catch-shadowing control pass. The final replay
was stopped at the user's priority change. Its five source/document files and
reversible patch are preserved in
`/private/tmp/blueice-parked-diagnostics-20261016/` and isolated from the current
architecture change. A verified durable Git stash also preserves those five
files: `c165c4ba5d0c8ef0fd23bca9d20b1e47ef98629a`. The gate count remains 6/10;
this checkpoint is not accepted.

The next operation-owner check reproduces 38 incorrect with/using admission
and with-reference locations across normal/aggressive GC. Operand and binding
owners now scope implicit conversion/resource/reference operations. Eleven
callback/body/cleanup controls and four resumed contexts after observed major
GC pass, alongside eleven AST/inventory/recursive-bytecode controls. Forty
classic-profile contexts, five added real-page contexts and four direct refusal
controls pass. The related 77-executable replay passes 2,358 cases, zero failures
and 16 existing ignored. Formatting and workspace/all-target build/Clippy pass;
fresh Test262 matches M1p across all 102,926 modes with zero changed modes or
new timeout. Six Rust inputs, 13,849 workspace/compiler inputs, seven runtimes
and all 77 selected test executable hashes remain fixed. The completed loop
and profile checkpoints below remain separate evidence; M1q stays at 6/10.

The operation-owner follow-up fixes 28 reproduced loop-protocol locations:
for-in, for-of and for-await acquisition/step failures retain the written right
operand. Ten callback controls, three resumed contexts after observed major GC,
five AST/bytecode controls, 21 deferred contexts and three added real-page
contexts pass. The related 70-executable Rust replay passes 2,303 cases, zero
failures and 16 existing ignored. Formatting and workspace/all-target build/
Clippy pass. The fresh complete Test262 comparison matches M1p at all 102,926
modes: 102,921 pass, one stale corpus and four exclusions, with zero changed
modes or new timeout. The completed profile checkpoint below remains separate
evidence. M1q stays at 6/10.

The producer/profile checkpoint passes 160 authenticated File DOM
normal/deferred throw/report/rejection/dynamic-code contexts, 28 generated-helper/
callback controls and eight real Core/child page contexts. Fixes remove duplicate
console declarations, preserve System top-level await, retain exact ES5 class
maps through helper insertion, terminate System class assignments and leave
generated CommonJS class publication unmapped. The fixed-input compiler/bridge/
launcher/MCP replay passes **1,843 cases, zero failures and 228 existing ignored**
across 189 test executables. Formatting and complete workspace/all-target build
and Clippy pass. Eighteen Rust inputs and 13,849 workspace/compiler inputs stay
fixed; all 189 test executable and seven runtime hashes are verified. The gate
count remains **6/10** pending the remaining operation and lifecycle acceptance.

The implicit-protocol checkpoint is implemented in `53a6d7f88`. Certified
operand locations survive heritage, spread, template conversion and delegated
iteration; AsyncFromSync preserves known and unavailable callback origins.
Diagnostic emitted-template maps certify unchanged interior positions within
the existing segment budget. Primitive `Proxy.ownKeys` results are rejected
before boxed-property reads. All fifty-six diagnostic groups, three public
own-key groups, ten source-map groups and thirty-six deferred contexts pass.

At that implicit-protocol checkpoint, the complete fixed-input Rust replay
passes **9,312 cases, zero failures and
233 existing ignored** across 687 result groups, including real Core/child
pages, all five official MCP console cases and doc tests. All 663 test
executables and seven runtime hashes remain fixed. Formatting and complete
workspace/all-target build and Clippy pass. BlueTS now follows the workspace's
raw-header HTTP feature policy; the downloader probe passes in the full suite.

**Six of ten final acceptance gates are complete.** The prior complete pinned
Test262 run matches M1p across all 102,926 modes: 102,921 pass, one stale corpus
and four exclusions, with no changed source/result inventory or new timeout.
Remaining expression/generated provenance,
producer/profile/module/debugger precision and the integrated queued-origin,
GC/navigation/resource matrix remain open. The detailed implementation and
acceptance checklist below separates completed checkpoints from the remaining
work. Unknown and generated-only locations stay explicitly unavailable;
section H remains unchecked. See the
[in-progress source-position contract](SOURCE_DIAGNOSTICS.md).

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
- [ ] **M1x: scheduling capacity admission.** Refuse container growth before
      publishing another task, reaction, registration or handle; admit appends
      that fit existing storage without a fictitious growth charge. Timer,
      frame, Promise-job and Promise-reaction regressions use at most nine
      entries and a scalar shared-owner model. The implementation checkpoint
      below now includes browser-task/cleanup/rejection producers, FileReader
      requeue/cancellation and timeout clock ordering; integrated acceptance
      and the wider native control/service-buffer gate remain open.
- [ ] **M1w: document callback-timer counts.** Root and attached child realms
      must share 1,024 pending callback registrations, expose aggregate counts,
      refund delivery/cancellation/document destruction and refuse before
      publishing a new registration. Preserve interval ownership, standalone
      policy, healthy-tab isolation and navigation recovery. Implementation
      and bounded count/lifecycle validation are recorded below; integrated
      acceptance, native control/service buffers and the broader per-document
      resource gate remain open.
- [ ] **Active native execution payloads.** Active locals, arguments and
      displaced caller frames must share document admission with retained
      Promise/timer/registry storage. Two public recursive-string probes exceed
      the 8 MiB envelope with at least 13.6 MB live while reporting zero native
      bytes. Preserve live execution values, release completed/unwound frames,
      and cover eval, constructor callbacks, Proxy forwarding, generator/async
      entry and same-document child calls.
      Evidence: `target/active-frame-red-source-20261016.json`; M1v is in progress.
- [x] **M1t: aggregate live-realm heap accounting.** Root and nested
      ShadowRealm heaps share one document envelope. Allocation and growth
      reserve managed bytes before committing storage; shrink, collection and
      heap release return their actual charge. Runtime `document_heap_bytes`
      and private child-host heap totals include all unique live heaps. Nine
      public groups, copied Core totals, real two-tab exhaustion/navigation
      and the full pinned Test262 inventory pass; see M1t below. Original
      evidence remains in `target/live-realm-accounting-probe-source-20261016.json`.
- [x] **M1u: agent-owned native registry payloads.** One document charge
      covers the shared `Symbol.for` table and retained UTF-16 key/description
      buffers across nested realms. Admission precedes live mutation; duplicate
      keys do not recharge, and child reclamation preserves registered identity.
      Promise/timer storage shares the cap. The original sixteen-key probe now
      fails with 8,000,412 committed native bytes; the three original runtime/
      Core failures, nine passing runtime groups, real two-tab/navigation/close
      controls and complete Test262 acceptance are recorded below. Original
      evidence: `target/agent-registry-probe-source-20261016.json`; after-fix
      evidence: `target/agent-registry-after-probe-source-20261016.json`.
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
M1s adds reclamation of discarded ShadowRealm VMs, including cross-heap wrapper
cycles. M1t adds aggregate document managed-heap admission; M1u charges shared
agent registries to the native account. Broader native control/service payload
accounting remains open in A.

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
- [x] **M1r: native pressure within callbacks.** Discarded Promise results
      can be collected before the callback's checkpoint, including before
      large async settlement. Live timer/reaction/await/WeakRef owners survive;
      a retained large result does not cause GC at every small call. Eight
      public page-runtime groups and real JavaScript/BlueTS burst pages pass,
      with the related Rust/Core/MCP and full pinned Test262 acceptance below.
- [x] **M1s: same-document realm reclamation.** Collect discarded ShadowRealm VMs
      and release their native Promise charge when no live instance, wrapped
      function or document work owns them. Acceptance: the failing public
      timer probe above continues with bounded native bytes after observed
      major collections; retained wrapped callbacks and queued work remain
      valid, and navigation/close release the whole document tree. Thirteen
      page-GC groups, eighteen ShadowRealm cases, seventeen cleanup cases,
      real JavaScript/BlueTS frames and full pinned Test262 pass; see M1s.

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
      provides global exception events and copied Core/MCP diagnostics; M1p
      adds the separate rejection tracker in B. M1q now verifies immutable
      source maps and original classic JavaScript root/block/function
      coordinates through real Core/child error handlers. Direct/emitted
      BlueTS, precise expressions/parameters and queued origins remain open.
      Known copied locations now pass the bounded private v61 Core/MCP route.
      See the detailed M1q implementation
      checklist and final gates below; unmapped code retains empty/zero
      metadata.

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

1. A/B ownership and GC: timer callbacks/arguments, Promise reactions and
   suspended work remain alive only as required, then release on completion,
   cancellation, navigation or close. Verify bounded long-running pages.
2. A resource containment and task-source integration: complete initiating-tab
   queue/native-buffer accounting and timer/network/UI ordering before adding
   more producers. Exhaustion must isolate the failing document.
3. Real Core/child long-running and two-tab acceptance for those contracts.
4. D/C platform surfaces, then E Fetch/XHR on the established scheduling and
   request-service boundary, followed by F ordinary-session acceptance.
5. Remaining M1q diagnostic-coordinate and generated-code precision.

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

### M1x — Scheduling capacity admission (implementation checkpoint, 2026-10-16)

Acceptance: calculate the requested slot-capacity delta before admission;
refuse before queue/owner publication or timer/frame identifier changes;
preserve successful appends into available slots; reconcile allocator-reported
capacity before publishing. Preserve callback rooting, document failure,
delivery, cancellation and Promise checkpoint semantics. This checkpoint
covers callback timers, animation frames, Promise jobs, pending Promise
reactions, browser/cleanup tasks and rejection notifications. Integrated
acceptance and broader native control/service storage remain open.

- [x] Reproduce five failures with ordinary unit tests using at most nine
      small live entries and a scalar model of another shared owner. The
      tests allocate no corresponding large buffer or large task queue.
- [x] Replace fixed four-slot forecasts with a shared growth plan and explicit
      reservation for timer/frame vectors, the Promise-job deque and each
      Promise record's reaction vector. Recheck unexpected reported spare
      capacity before publishing; release temporary timer/frame roots on
      refusal. Avoid a growth charge when existing slots suffice.
- [x] Forecast timer argument/root storage from arguments after callback and
      delay. Empty argument lists need one callback root, not two argument
      values plus three roots.
- [x] Pass eight capacity groups, 23 existing handoff/call-entry/timer-count
      groups and 69 public timer/frame/Promise controls; formatting and offline
      workspace/all-target Clippy pass with warnings denied.
- [x] Apply capacity admission to drag, timeout, FileReader, host completion,
      handled/unhandled rejection notifications, cleanup and pending-rejection
      queues before retaining new roots, serials or externally visible state.
- [x] Readmit FileReader's next stage after callbacks can consume its slot or
      document count; release the stage's root on refusal. Cancel matching
      reads in place without rebuilding other tasks into larger storage.
- [x] Sample abort-timeout clocks before final admission and serial assignment,
      preserving child work enqueued by an embedding clock callback.
- [x] Pass follow-up bounded regression/control checks, formatting and offline
      workspace/all-target Clippy with warnings denied; retain a separate
      source/executable/log receipt for this checkpoint.
- [ ] Complete integrated/full-source acceptance under the owner's security
      constraints. M1u remains the latest accepted milestone.

The original five regressions all fail before the change: timer, frame, job
and reaction containers already contain nine entries when the byte ledger
refuses growth; an available timer slot incorrectly returns SchedulingLimit.
Red log: `/private/tmp/blueice-scheduling-capacity-unit-red-20261016.log`.

Initial checkpoint (`f39d6385e`) validation: **100 passed, zero failed, zero ignored**.
All eight capacity groups pass, including fitting appends for all four
container families. Existing unit controls pass twelve payload-handoff, four
call-entry and seven timer-count groups. Public `document_timer_counts` (two),
`host_timers` (32), `host_animation_frames` (13), `promise_capabilities` (ten)
and `promise_gc_lifecycle` (twelve) pass. Six public resource/limit cases are
filtered; the finite timer/frame default-count controls still execute. No
large-payload or infinite-job test or out-of-process probe is added to this
selection. Formatting, `git diff --check` and offline workspace/all-target
Clippy with warnings denied pass.

Logs: `/private/tmp/blueice-scheduling-capacity-final-unit-20261016.log`,
`/private/tmp/blueice-scheduling-capacity-handoff-controls-20261016.log`,
`/private/tmp/blueice-scheduling-capacity-call-controls-20261016.log`,
`/private/tmp/blueice-scheduling-capacity-timer-controls-20261016.log`,
`/private/tmp/blueice-scheduling-capacity-public-controls-20261016.log` and
`/private/tmp/blueice-scheduling-capacity-clippy-20261016.log`.
Bounded source/executable/log receipt:
`target/scheduling-capacity-source-20261016.json`.
No fresh full Test262, real Core/MCP replay or coverage result is claimed.

**Browser/cleanup/rejection follow-up — bounded validation passed.** Twenty
bounded unit groups fail on the parent and pass with the
implementation. Twelve cover growth refusal and available slots for six browser
producers; four cover cleanup and pending-rejection storage. Two cover FileReader
callbacks consuming spare slots or the shared document count before requeue;
one covers cancellation without regrowing unrelated tasks and one covers clock
reentry ordering. Refusal preserves queued entries, capacity, root-registration
and serial counts, pending rejection ownership and initial FileReader state.
The requeue failures release the reader root instead of publishing a ninth task.
Fixtures use at most nine live entries with scalar shared-owner models; no
corresponding large payload or large task queue is allocated. The complete red
log is `/private/tmp/blueice-browser-task-capacity-expanded-red-20261016.log`.

Current-source bounded validation: **161 passed, zero failed, zero ignored**.
The twenty new groups and 36 existing unit controls pass: eight initial
capacity groups, twelve payload-handoff groups, four call-entry controls,
seven timer-count groups, two cleanup boundaries and three task-source
boundaries. Seven public selections pass `host_async_tasks` (18),
`host_finalization_tasks` (ten), `host_rejections` (twelve), `host_task_turns`
(27), `web_abort` (six), `web_abort_lifecycle` (seven) and `web_file_api` (25).
Fifteen other public pressure/limit cases remain filtered. Formatting,
`git diff --check` and offline workspace/all-target Clippy with warnings
denied pass.

Logs: `/private/tmp/blueice-browser-task-capacity-final-unit-20261016.log`,
`/private/tmp/blueice-browser-task-capacity-slot-controls-20261016.log`,
`/private/tmp/blueice-browser-task-capacity-handoff-controls-20261016.log`,
`/private/tmp/blueice-browser-task-capacity-call-controls-20261016.log`,
`/private/tmp/blueice-browser-task-capacity-timer-controls-20261016.log`,
`/private/tmp/blueice-browser-task-capacity-cleanup-controls-20261016.log`,
`/private/tmp/blueice-browser-task-capacity-source-controls-20261016.log`,
`/private/tmp/blueice-browser-task-capacity-public-controls-20261016.log` and
`/private/tmp/blueice-browser-task-capacity-clippy-20261016.log`.
Receipt: `target/browser-task-capacity-source-20261016.json`.
No fresh full Test262, real Core/MCP replay or coverage result is claimed.

### M1w — Document callback-timer counts (implementation checkpoint, 2026-10-16)

Acceptance: one shared document limit across root/child registrations;
accurate live counts through firing, self-cancellation and destruction;
terminal document refusal before another registration is published; healthy
tab isolation and navigation recovery; preserved standalone catchable limits.
Clock-hook work must precede the outer timer's final admission and enqueue
serial. Broader native control/service storage and final integrated acceptance
remain outside this count-only checkpoint.

- [x] Add `pending_timers` to shared scheduling addition, subtraction and
      admission; report aggregate callback registrations through runtime stats.
- [x] Admit one count before retaining timer roots or advancing registration
      state. Use document `SchedulingLimit` for bounded pages; preserve the
      host-neutral 1,024-per-realm `RangeError` and capacity recovery.
- [x] Refund firing one-shots and cancelled intervals; retain an interval's
      registration during delivery. Queued work keeps a discarded realm alive
      until delivery; dropping a whole document releases every scope's count.
- [x] Sample the host clock before final admission and serial assignment.
      A two-timer regression initially runs root before a child already queued
      by the clock hook; the corrected order is child then root.
- [x] Pass seven unit groups: five use small queues/scalar admission, one
      verifies clock-hook ordering and one checks the existing finite default
      count plus host-neutral recovery and policy attachment.
- [x] Pass public runtime refusal/isolation/navigation and related timer
      selections, formatting and offline workspace/all-target Clippy.
- [ ] Complete integrated/full-source acceptance under the owner's security
      constraints. M1u remains the latest accepted milestone.

The new public `document_timer_counts` target must be included in any fresh
full replay inventory; the historical 405-executable M1v selections predate it.
No private transport layout or byte-limit change is needed for these Rust
counter snapshots. Abort timeouts remain in the separate browser-task cap.

Current-source bounded validation: **94 passed, zero failed, zero ignored**.
The seven timer-count unit groups, twelve existing handoff groups and four
call-entry controls pass. The new complete `document_timer_counts` public
target passes two cases. Related public selections pass `host_timers` (32),
`host_task_turns` (27) and `host_finalization_tasks` (ten), filtering thirteen
other pressure/limit cases. The fixed 1,024-registration page test and timer
argument-count test run as ordinary bounded functional checks; no large
payload, infinite-job or out-of-process resource probe is used for this gate.
The standalone default-count test also verifies catchable refusal, clearing
and reuse, then terminal document refusal after policy attachment.

The initial API tests fail to compile on the parent because the shared count
and limit are absent. A separate two-registration clock regression fails with
`[root, child]` instead of `[child, root]` before clock sampling moves ahead of
final admission. Those logs are retained at
`/private/tmp/blueice-document-timer-count-unit-red-20261016.log` and
`/private/tmp/blueice-document-timer-clock-unit-red-20261016.log`.
Current logs:
`/private/tmp/blueice-document-timer-count-final-unit-20261016.log`,
`/private/tmp/blueice-document-timer-count-handoff-controls-20261016.log`,
`/private/tmp/blueice-document-timer-count-call-controls-20261016.log` and
`/private/tmp/blueice-document-timer-count-public-controls-20261016.log`.
Clippy: `/private/tmp/blueice-document-timer-count-clippy-20261016.log`.
Bounded receipt: `target/document-timer-count-source-20261016.json`.
No fresh full Test262, real Core/MCP replay or coverage result is claimed.

### M1v — Active execution ownership (implementation checkpoint, 2026-10-16)

- [x] Account for primitive payloads in active VM fields and displaced caller
      frames, sharing the document's Promise/timer/registry account. Preserve
      object roots without creating primitive copies solely for GC protection.
- [x] Transfer leases through suspended contexts and restore caller completion
      records on return, throw and entry refusal. Reconcile released ownership
      after a terminal resource failure without recovering the document.
- [x] Keep constructor input charged while prototype getters and base-class
      field initializers run; transfer ordinary/generator entry ownership only
      after VM fields take the arguments. Keep direct-eval source arguments
      charged in ordinary, spread and tail positions.
- [x] Transfer Proxy arguments through forwarding and trap invocation without
      charging moved buffers again; preserve the incoming charge during trap
      getters, including a throwing getter.
- [x] Pass four bounded call-entry unit groups and 82 related public cases
      across eight complete targets at `6be49f4e2`, zero failures or ignored
      cases.
- [x] Transfer timer, drag-string and finalization input charges to callback
      frames without retaining a duplicate through the Promise checkpoint.
      Preserve actual interval registration and named-parameter copies;
      consume ignored cleanup outcomes before checkpoint jobs run.
- [x] Pass the `deafa218a` bounded task-handoff selection: three new unit groups,
      four call-entry controls and 67 public timer/task/cleanup cases. Fifteen
      public pressure/limit cases are explicitly filtered out.
- [x] Review queued Promise-job and host-completion ownership. Transfer input
      leases into handlers, resumed frames and settled records; retain borrowed
      loader strings, targets and outcomes for their actual lifetime. Avoid
      primitive copies used solely as GC roots in species/thenable resolution.
- [x] Reconcile continuation/control ownership after context restoration even
      when primitive bytes are unchanged. Preserve the ordinary call/instruction
      fast path. Pass twelve handoff groups, four call-entry controls and
      61 related public cases; six host-async pressure/limit cases are filtered.
- [ ] Finish the final M1v regression selection with stable source/artifacts
      under the owner's security constraints.
- [ ] Compare a fresh complete pinned Test262 inventory with the accepted
      baseline after the final Rust checks.

The new tests use seven-character arguments and a twelve-character eval
source. Before these fixes, constructor field callbacks observe zero of the
argument's 14 bytes; transparent Proxy chains count 14, 28 and 42 bytes for
the same moved input. Tail eval loses its 24-byte source argument while the
strict caller's existing completion remains charged. The corrected tests also
cover normal/throwing prototype and field callbacks, trap getters, trapped
forwarding, ordinary/async/generator parameter entry and final lease release.

Call-entry checkpoint (`6be49f4e2`) validation is deliberately bounded:
`cargo test --offline -p blueice-bluejs --lib argument_charges` passes all four
groups; the eight public
targets are `async_from_sync_delegation`, `class_fields_define`,
`class_fields_evaluation`, `eval_var_environment_capture`,
`function_environments`, `generator_function_constructors`,
`generator_resume_with_objects` and `promise_capabilities`. Formatting and
offline workspace/all-target Clippy with warnings denied pass.
Logs: `/private/tmp/blueice-call-entry-final-unit-20261016.log` and
`/private/tmp/blueice-call-entry-related-units-20261016.log`; Clippy:
`/private/tmp/blueice-active-frame-call-entry-clippy-20261016.log`.
The bounded source/test receipt is
`target/active-frame-call-entry-unit-source-20261016.json`.

Task-handoff follow-up tests use tiny inputs. Before the correction, a timer
or finalization callback double-charges a 14-byte input as 28 bytes and keeps
14 stale bytes through the following Promise job. Drag delivery likewise
double-charges its allocated UTF-16 buffer. Input leases now end at dispatch;
interval registrations keep their actual retained copy until cancellation.
Seven timer variants cover one-shot/interval delivery, self-cancellation,
throws and real named-parameter copies. Drag and normal/throwing cleanup
callbacks also verify their following Promise checkpoint.

Task-handoff checkpoint (`deafa218a`) validation passes **74 cases, zero failures and zero
ignored**: `--lib payload_handoff` (three), `--lib argument_charges` (four),
`host_timers` (30), `host_task_turns` (27) and `host_finalization_tasks` (ten).
The three public targets filter out fifteen pressure/limit cases, so this is
not a complete target replay. The near-cap interval fixture now names its
callback parameter to require an actual additional VM-owned copy; that
resource-exhaustion fixture is updated statically and has not been rerun.
Formatting and offline workspace/all-target Clippy with warnings denied pass
on this follow-up source.
Logs: `/private/tmp/blueice-task-payload-handoff-final-unit-20261016.log`,
`/private/tmp/blueice-task-payload-handoff-call-controls-20261016.log` and
`/private/tmp/blueice-task-payload-handoff-public-controls-20261016.log`.
Clippy: `/private/tmp/blueice-task-payload-handoff-clippy-20261016.log`.
The bounded receipt is
`target/task-payload-handoff-source-20261016.json`.

Promise/host-completion follow-up: ordinary reaction and host-completion
callbacks originally count a 14-byte input as 42 rather than 28 bytes across
its two actual owners (settled result plus running arguments). Custom species
callbacks observe 56/70 instead of 42/42 bytes because the job retains a moved
input and resolution creates a primitive GC-root copy. Scoped job, target and
outcome leases now follow the actual owners; host completions release their
input lease before settlement. Dynamic import keeps borrowed loader strings
through module execution. Generator yield/delegate inputs are consumed before
successor requests or error resumption when they are no longer needed.

The await regression also exposes stale ownership publication: numeric input
does not change primitive bytes, so the old restoration fast path leaves a
removed continuation's control storage in the account. The observed string/
numeric difference is -667 rather than +28 bytes. Restoring a context now
reconciles all scheduling owners; ordinary instruction/call checks retain
their unchanged-payload fast path. Refusal/unwind reconciliation keeps the
document's terminal failure latched.

Promise/host-completion checkpoint (`0d0f09c0e`) validation passes **77 cases, zero failures and zero
ignored**: twelve `--lib payload_handoff` groups, four `--lib argument_charges`
controls and 61 public cases. Five targets run completely:
`async_from_sync_delegation` (two), `async_generator_ticks` (fourteen),
`async_yield_star_delegate_close` (seven), `dynamic_import` (ten) and
`promise_capabilities` (ten). `host_async_tasks` passes eighteen cases and
explicitly filters out six pressure/limit cases. All loaders/services in this
selection use in-memory fixtures; no external network ingress is exercised.
Formatting and offline workspace/all-target Clippy with warnings denied pass.
Logs: `/private/tmp/blueice-promise-payload-handoff-expanded-unit-20261016.log`,
`/private/tmp/blueice-promise-payload-handoff-call-controls-20261016.log` and
`/private/tmp/blueice-promise-payload-handoff-public-controls-20261016.log`.
Clippy: `/private/tmp/blueice-promise-payload-handoff-clippy-20261016.log`.
Original failures: `/private/tmp/blueice-promise-payload-handoff-unit-red-20261016.log`
and `/private/tmp/blueice-promise-await-publication-unit-red-20261016.log`.
Bounded receipt: `target/promise-payload-handoff-source-20261016.json`.

The interrupted 298/405-target replay predates the call-entry follow-up and
does not validate this final source. It remains partial evidence, not a final
gate. No new full Test262, real Core/MCP replay or coverage result is claimed.
M1u remains the latest accepted milestone; active control-table/vector
capacity and general host/service buffers remain separate work in A.

### M1u — Agent-owned native registries (2026-10-16)

- [x] Reproduce uncharged registry storage through two public runtime groups
      and the real Core child; preserve the original three failures.
- [x] Give the shared agent registry one document-owned native charge;
      admit table growth and retained key/description buffers before mutation.
- [x] Pass nine public groups covering duplicate keys, nested realms,
      child reclamation, shared Promise/timer limits, transactional refusal,
      terminal failure, healthy tabs/navigation, UTF-16 and reentrant coercion,
      host-neutral behavior and policy attachment to existing entries.
- [x] Verify copied Core totals, navigation/close release and the real
      two-tab registry-exhaustion page alongside existing lifecycle cases.
- [x] Pass formatting and workspace/all-target build/Clippy.
- [x] Complete the related Rust/Core/MCP replay and full pinned Test262.
- [x] Record accepted evidence and a detailed English milestone commit.

The shared registry owns its charge independently of individual VM scopes.
Committed bytes include entry capacity and stored key/description buffers;
prepared growth cannot change the live table before admission. Rejected
insertion preserves committed usage and leaves that document failed across
later entries. Existing unbounded VM storage is admitted before document
policy or rejection bindings are published; standalone VMs keep host-neutral
semantics. Runtime/Core native totals include the charge without changing the
private transport layout. See [the ownership contract](HOST_TASK_SOURCES.md#shared-agent-registries).

All 89 disjoint test executables pass 2,744 cases, zero failures and sixteen
existing ignored: BlueJS 1,945 across 50 targets (all 1,205 library cases),
bridge 300 across 35 targets, launcher 345 (321 library plus 24 real-page),
Core/child 149 and official MCP console 5. Formatting and workspace/all-target
build/Clippy pass. Sixteen Rust inputs, including both new files, 13,855
workspace/compiler inputs, 89 executable hashes and seven runtime hashes
remain fixed throughout validation.

Fresh complete Test262 takes 380.38s including validation/reporting
(runner 375.569s). All 102,926 modes match the accepted M1p
source/result inventory: 102,921 pass, one stale corpus and four exclusions.
Hashes, expectations, features/flags, status and actual kind/phase/reason have
zero changed records and no new failure or timeout. Accepted evidence:
`target/agent-registry-native-final-source-20261016.json`,
`target/agent-registry-native-final-validation-20261016.json` and
`target/test262-agent-registry-native-final-full-20261016/`.

The original three failures remain in
`target/agent-registry-red-source-20261016.json` and
`/private/tmp/blueice-agent-registry-{runtime,core}-red-20261016.log`.
Before/after public probe bindings remain in
`target/agent-registry-probe-source-20261016.json` and
`target/agent-registry-after-probe-source-20261016.json`.
The original validation inventory initialization was interrupted while its
formatting child was running, then replaced by a cached-inventory script;
that attempt is not an accepted gate. M1u closes only the registry gap in A.
Broader native control/service-buffer work remains open, and M1q stays parked.

### M1t — Aggregate document managed heaps (2026-10-16)

- [x] Reproduce bypass of a 1 MiB document envelope through sixteen live
      ShadowRealms and under-reporting through the public child-host stats.
- [x] Share managed-storage admission across all nested document heaps;
      charge each actual heap once and release storage on shrink, deletion,
      collection and heap destruction. Preserve root collector diagnostics.
- [x] Verify property/array/Map/Set/buffer allocation and resize refusal,
      terminal document failure, nested realms, pending cleanup, wrapped
      owners, shrink/release and 64 polls through each timer entry.
- [x] Reclaim caller garbage before borrowing a child for large growth;
      retain live owners and avoid repeated GC at 1,000 unchanged small calls.
- [x] Pass launcher/Core aggregate-stat transport and a real two-tab timer
      exhaustion/navigation fixture. All 23 real-page cases pass.
- [x] Pass formatting and workspace/all-target build/Clippy.
- [x] Complete the related Rust/Core/MCP replay and full pinned Test262.
- [x] Record final evidence and a detailed English milestone commit.

The root's immutable managed-heap limit now bounds all managed storage owned
by its document. `document_heap_bytes` exposes that total separately from
root `HeapStats`; child realm/child-wide heap reports carry the aggregate.
Existing child-wide reservations and Core's separate retained-metadata/
scheduling page-cache ledger remain distinct. Native scheduling, string and
per-heap limits are unchanged, and host-neutral VMs without a document scope
retain their per-heap policy. This closes managed storage, not the native
agent-registry/control/service payloads still open in A.

All 88 disjoint test executables pass 2,733 cases, zero failures and sixteen
existing ignored: BlueJS 1,936 across 49 targets (all 1,205 library cases),
bridge 300 across 35 targets, launcher 344, Core/child 148 and official MCP
console 5. Formatting and workspace/all-target build/Clippy pass. Twenty-eight
Rust inputs, 13,853 workspace/compiler inputs, 88 test executable hashes and
seven runtime hashes remain fixed.

The fresh complete Test262 replay takes 436.33s including validation/reporting
(runner 429.586s). All 102,926 modes match M1p's accepted source/result
inventory: 102,921 pass, one stale corpus and four exclusions. Source hashes,
expectations, features/flags, statuses and actual kind/phase/reason have zero
changes; no mode newly fails or times out. Accepted evidence is
`target/document-heap-source-20261016.json`,
`target/document-heap-validation-20261016.json` and
`target/test262-document-heap-full-20261016/`.

The before-fix runtime and child-host failures are preserved separately in
`target/document-heap-red-source-20261016.json` and
`/private/tmp/blueice-document-heap-{runtime,host}-red-20261016.log`.
The same public probe now refuses the 1 MiB envelope; under the default 16 MiB
it succeeds and reports 16,196,190 document bytes versus 217,502 root bytes.
Bindings are in `target/document-heap-after-probe-source-20261016.json`.
M1t closes A's aggregate managed-heap item. The separately reproduced native
agent-registry gap remains open in A and is the next architecture task; M1q
stays parked.

### M1s — ShadowRealm ownership and reclamation (2026-10-16)

- [x] Reproduce orphan VM/native-Promise retention through the public runtime:
      the timer fails at poll 11, dropping the last wrapped callable fails to
      release its charge, and an unreachable cross-heap wrapper cycle leaks.
- [x] Trace instance, wrapped-callable and document-work ownership at explicit
      execution/native-pressure safepoints. Release foreign target-root leases
      without reborrowing active VMs; defer agent tracing during active borrows.
- [x] Preserve pending Promise/cleanup work, retained wrapped callbacks,
      independent Test262 membrane owners and navigation cleanup. Use weak
      document-task traversal snapshots so collection can release orphan VMs
      before the native-pressure watermark is sampled.
- [x] Complete 64 polls through each timer API without intervening scripts,
      keeping native bytes below 4 MiB, plus the observed-GC WeakRef control.
- [x] Pass real JavaScript/BlueTS frame pages, related Rust/Core/MCP cases,
      formatting, workspace/all-target build/Clippy and full pinned Test262.
- [x] Record final evidence and a detailed English milestone commit.

The implementation makes a child's globals roots only after its VM has a live
instance, wrapped callable, queued work or independent membrane owner. Foreign
function pins become edges conditional on their facade; they cannot bootstrap
an otherwise dead realm/wrapper cycle into liveness. Deferred root releases
are drained before sweeping, with no JavaScript callback during collection.
The native and managed hard limits remain unchanged; aggregate child-heap and
service/control-buffer accounting remain in A's broader open resource item.

All 87 disjoint test executables pass 2,721 cases, zero failures and sixteen
existing ignored: BlueJS 1,927 across 48 targets (all 1,205 library cases),
bridge 300 across 35 targets, launcher 342, Core/child 147 and official MCP
console 5. The 22 real-page cases include JavaScript/direct BlueTS timers that
discard sixteen realms per callback and publish three frames with matching
Representations before clearing their interval. Formatting and full workspace/
all-target build/Clippy pass. Fifteen Rust inputs, 13,851 workspace/compiler
inputs, 87 test executable hashes and seven runtime hashes remain fixed.

Fresh complete Test262 takes 461.37s including validation/reporting (runner
453.844s). All 102,926 modes match M1p's accepted inventory: 102,921 pass, one
stale corpus and four exclusions. Source hashes, expectations, features/flags,
statuses and actual kind/phase/reason have zero changes; no mode newly fails
or times out. Accepted evidence is
`target/realm-ownership-final2-source-20261016.json`,
`target/realm-ownership-final2-validation-20261016.json` and
`target/test262-realm-ownership-final2-full-20261016/`.

The original three public failures are retained in
`target/realm-ownership-red-source-20261016.json` and
`/private/tmp/blueice-realm-ownership-red-20261016.log`. Earlier failed build/
page-fixture attempts remain in separate `realm-ownership` and
`realm-ownership-final` receipts; they are not accepted gates. M1s closes B's
discarded-realm ownership item. That checkpoint left live-realm managed-heap aggregation as its next task;
M1t now closes it. Agent-native/service work remains open; M1q stays parked.

### M1r — Native Promise pressure during timer callbacks (2026-10-16)

- [x] Reproduce document failure before the first timer callback completes:
      32 discarded 400,000-character Promise results exhaust native bytes
      while managed heap remains below its collection threshold.
- [x] Collect unreachable Promise/frame state at rooted call entry/return
      boundaries. The burst, large async-return and live-owner controls pass;
      one thousand small calls cause no more GC than a one-call control.
      Retained payload is reclaimed after its owner is dropped.
- [x] Complete final acceptance for the rooted execution boundary
      before native pressure falsely aborts the callback. Preserve live
      reactions, timer arguments, async frames and WeakRef job liveness.
- [x] Verify cancellation/navigation release and independent-document resource
      containment. All 37 timer and 13 scheduling-limit cases pass; live
      over-budget data still fails the existing policy. All 147 Core/child,
      320 launcher and 21 real-page cases pass, including the new JavaScript/
      BlueTS timer bursts and existing two-tab isolation/navigation fixtures.
- [x] Pass public runtime and real Core/child long-running page acceptance,
      formatting, workspace/all-target build/Clippy and pinned Test262.
- [x] Record final evidence and a detailed English milestone commit.

Native Promise results and suspended payloads can grow without crossing the
managed-heap collection trigger. Waiting until the host checkpoint falsely
aborts a timer that discards many results within one callback. Call entry
already protects the callee/receiver/arguments/new.target; the return boundary
additionally protects a fresh result only when collection runs. An adaptive
native-byte watermark reclaims dead state before new pressure reaches admission
without recollecting an unchanged live owner at every small call. The existing
8 MiB hard limit and terminal document-failure policy remain in force.

Eight public page-runtime groups pass, including the original first-callback
failure, the separately reproduced large async-return failure, live timer/
reaction/await/WeakRef roots, dropped-owner release and a one-call versus
1,000-call GC control. Existing 4,000-poll/1 MiB and large-native-string timer
regressions pass. New real JavaScript and direct BlueTS pages each finish
32-result bursts per callback and publish three distinct frames with matching
Representations; their cleared interval then stays quiet.

The fixed-input acceptance passes 2,715 cases, zero failures and 16 existing
ignored across 87 disjoint test executables: BlueJS 1,922 (48 targets, including
all 1,205 library cases), all 35 bridge targets 300, launcher 341, Core/child
147 and official MCP console 5. Formatting and full workspace/all-target
build/Clippy pass. Five Rust and 13,849 workspace/compiler inputs, all 87 test
executables and seven runtime hashes stay fixed. Evidence is
`target/promise-native-pressure-source-20261016.json` and
`target/promise-native-pressure-validation-20261016.json`; both before-fix
source manifests and public failure logs are retained separately.

The accepted fresh complete Test262 replay takes 467.82s including corpus
validation/reporting and matches M1p's 102,926-mode inventory exactly: 102,921
pass, one stale corpus and four exclusions, with zero changed source hashes,
expectations, features/flags, statuses or actual kind/phase/reason. No accepted
mode times out. The first full attempt's one deep-WeakMap wall-timeout remains
unmodified in `target/test262-promise-native-pressure-full-20261016/`; three
isolated unchanged-binary runs pass both modes in 2.86–3.27s. The separate
accepted full replay uses the same four jobs and deadlines and lives in
`target/test262-promise-native-pressure-repeat-full-20261016/`. Runner exit 1
reflects the five unchanged non-pass modes; the full comparison succeeds.

M1r closes this native-pressure checkpoint. Complete service-buffer/task-source
accounting and the independently reproduced discarded-ShadowRealm charge leak
remain open. Whole-realm ownership/reclamation is next, before parked M1q
diagnostic refinements; M1q stays at 6/10.

### M1q — Original diagnostic source positions (in progress)

**Progress updated 2026-10-16: implicit-protocol and emitted-template fixes
verified through full Rust/Core/MCP and pinned Test262;
6/10 final acceptance gates complete.** Completed
implementation steps and test evidence
are recorded separately below so partial progress is visible without closing
the full source-position requirement.

#### Verified implementation steps

- [x] Validate immutable source maps against exact source identities, live
      generations, code-unit ordinals and instruction boundaries; retain
      explicit gaps and reject malformed or overlapping ranges atomically.
- [x] Charge map capacities with installed code and roll back rejected page
      installation before execution.
- [x] Capture parser statement provenance and compiler instruction owners,
      including nested blocks/functions, repeated function bodies, Annex B
      wrappers, labels and directive normalization.
- [x] Report mapped throws and active `reportError` call sites, restore nested
      interpreter contexts, and keep unmapped `eval`/`Function` and callback
      origins unknown without borrowing an earlier root script's location.
- [x] Verify original JavaScript root/block/function coordinates through real
      Core/child `window.onerror` delivery.
- [x] Preserve source/debugger origins through synchronous finalizers, ignored
      iterator-close failures and yielded generator resumption across scripts;
      verify collection and suspended filename payload charges.
- [x] Carry bounded optional source records through private v61, exact-document
      Core validation and MCP's untrusted content frame; charge source names
      with console text and preserve unknown locations.
- [x] Retain native Promise rejection origins through reactions, thenables,
      settled/pending awaits, native forwarding and `finally` thunks. Verify
      32 source scenarios, cross-script/full-GC thunk retention and collection,
      filename payload charges and the document byte cap; real MCP verifies
      original JavaScript coordinates after `finally` and unavailable BlueTS.
- [x] Pass source-copy acceptance at `55af67038`: 2,117 passed, zero
      final failures, one existing ignored, across twelve targets.
- [x] Pass the latest current-source diagnostic/error/page replay: 55 passed,
      zero failed, across three complete targets.
- [x] Pass `cargo fmt --all -- --check` and focused Clippy for the BlueJS,
      BlueTS bridge, IPC, Core, launcher and MCP libraries with `-D warnings`.
- [x] Pass the Promise-origin follow-up at `97b021149`: 1,837 passed across
      twenty disjoint targets, one existing ignored; formatting and focused six-library Clippy.
- [x] Preserve FIFO async-generator request origins through queued throw/return,
      awaited finalizers, delegate rejection and abrupt replacement. Verify
      twelve source scenarios under two GC configurations, neighboring known/
      unknown origins, observable major-GC collection, source capacity charges
      and atomic heap refusal; real MCP reports inline line 5, column 25.
- [x] Pass the queued-generator follow-up: 1,841 passed across twenty disjoint
      targets, one existing ignored, formatting and focused six-library Clippy.
- [x] Retain certified module failure origins through synchronous/awaited
      dependency evaluation, async parent fan-out, cached dynamic imports and
      debugger resumption. Preserve argument-getter and explicitly unknown
      origins; keep idle loader referrers from suppressing host notifications.
- [x] Charge shared module origins and weak-ledger capacities before cache
      publication, including displaced graph owners; verify major-GC release,
      absence of added JavaScript roots and native-byte refusal.
- [x] Pass the final module-origin follow-up: 1,981 passed across thirty
      disjoint targets, one existing ignored, matched Core/host/Gatekeeper,
      formatting and focused six-library Clippy.
- [x] Preserve tested synchronous and asynchronous disposal origins through
      native pending completions, including suppression, caught noise, unknown
      sources, rejected awaits, module fan-out and queued-generator cleanup.
      The ten disposal scenarios pass under normal and aggressive GC; async
      stack and `await using` share one compiler loop and intrinsic suppression.
- [x] Verify suspended disposal filename charges, cross-script source
      retention, observed major GC and owner release after the host checkpoint.
- [x] Pass the final disposal follow-up: 2,041 passed across thirty-four
      disjoint targets, one existing ignored, matched runtime builds, formatting
      and focused six-library Clippy. Real MCP keeps inline line 5, column 25
      through generator/finally/async-stack/`await using` forwarding.
- [x] Capture tested expression and parameter provenance, including operator/
      constructor intermediates, defaults, destructuring, named/unnamed arrows,
      nested/tagged templates and CRLF/UTF-16 positions. Twenty-three source
      scenarios pass; five new real-page contexts preserve their own positions.
- [x] Append pattern slots in a source-only inventory while retaining debugger
      AST ordinals and the legacy statement-span API. Verify identical ordinary
      bytecode, bounded snapshot work, wide-list admission and lexer-state
      restoration; temporary arrow overlays preserve proven original owners.
- [x] Pass the final expression follow-up: 2,105 passed across forty-two
      disjoint targets, one existing ignored, matched runtime builds, formatting
      and focused Clippy. Real MCP now reports the RHS call's inline line 5,
      column 36, replacing the enclosing statement's column 25.
- [x] Clear old compilation artifacts after validation: remove 75,154 files
      from the main and `cov-round47` worktree targets, reducing their combined
      allocated size by 40.55 GiB. Preserve source edits, test reports, all
      thirty-two top-level validation manifests and the running MCP executable.
- [x] Preserve tested constructor/accessor clones, static-block copies and
      field initializer subtrees through scoped diagnostic overlays. All
      thirty-two class/decorator source scenarios and three unknown-source
      controls pass; ordinary syntax, code-unit counts and bytes stay identical.
- [x] Retain lowered class closure locations across later scripts and actual
      major GC in four scenarios. Pass 1,627 BlueJS cases over thirty-three
      complete targets, including 1,205 library cases and all thirty-eight
      diagnostic groups.
- [x] Pass the final class follow-up: 2,121 passed across thirty-seven disjoint
      targets, one existing ignored, formatting and focused library/integration
      Clippy. Six additional real-page source contexts pass; official MCP
      reports queued getter/field throws at 5:78 and 6:74. Fixed final runtime
      hashes remain identical before and after the cross-process replay.
- [x] Capture assignment-pattern provenance in a source-only suffix after
      the existing debugger AST and binding-pattern inventory. Preserve
      shorthand/for-head ranges, template rebasing and compiler clone aliases;
      legacy statement spans, syntax, code-unit counts and bytes stay intact.
- [x] Pass thirty-seven reference/assignment source scenarios, two explicitly
      unknown getter/Proxy controls and all forty-two diagnostic groups.
      Accept 3,000-target patterns and bound 40,000-target snapshot work without
      treating valid JavaScript as a syntax error.
- [x] Pass 1,571 current-source BlueJS cases across forty-one complete targets,
      including 1,205 library cases and all forty-two diagnostic groups.
- [x] Pass the final reference follow-up: 2,066 cases across forty-five
      disjoint targets, one existing ignored, six additional real-page contexts
      and all four official MCP console cases. Queued callee/nested-pattern
      locations are 3:27 and 6:27. Runtime/test hashes remain fixed throughout
      acceptance; formatting and focused library/integration Clippy pass.
- [x] Capture executable module/export diagnostic slots with the same bounded
      parser inventory as scripts. Eight public fixtures retain ordinary AST
      ordinals, instruction bytes and child code units; all forty-three
      diagnostic groups pass.
- [x] Bind emitted artifacts to authorized original IDs, hashes and exact
      source content. Batch exact mapping lookups validate the complete stream
      once and keep generated gaps unknown; preserve CR/CRLF and UTF-16 origins
      while publishing ECMAScript CR/LF/LS/PS coordinates.
- [x] Install original locations through normal and debugger-controlled
      emitted classic/module execution, JavaScript modules and deferred
      JavaScript classics. Verify dependency throws before/after `await`,
      unknown dynamic-code controls and atomic graph rollback for five source
      substitutions/malformed-map controls plus a code/map byte-budget refusal.
- [x] Fix BlueTS LS/PS lexical trivia and line-comment termination after a
      failing regression. Verify original byte spans through multibyte comments
      and CR, CRLF, LS and PS source cases.
- [x] Pass 2,227 cases across thirty disjoint target selections: 1,450 BlueJS,
      276 BlueTS, 167 bridge, 311 launcher, eighteen real-page and five official
      MCP console cases. Eleven existing cases remain ignored, including ten
      requiring the pinned TypeScript oracle. Six new real-page source contexts
      and MCP's emitted classic/module/async plus dynamic-unknown controls pass.
      Runtime/test hashes remain fixed during acceptance; formatting, seven
      library, changed-target and launcher test Clippy pass.
- [x] Share the source-only AST inventory and bounded instruction-range builder
      between original/emitted JavaScript and direct BlueTS. Preserve debugger
      ordinals and pattern namespaces; all forty-four diagnostic groups pass.
- [x] Derive initial direct page script/module locations from checked BlueTS
      declarations and the final lowered AST. Retain source identity, a bounded
      AST certificate and coordinates; discard temporary addresses/source text.
      Reject changed source identities, changed ASTs and code/map byte overruns
      before execution. Standalone debugger attachment behavior stays intact.
- [x] Pass 173 bridge cases, including original function/control-flow/class
      throws, constructor prologues, CR/CRLF/LS/PS coordinates, awaited dependency
      failures and a dynamic-code unknown control. Add the missing default-export
      snapshot lowering record without changing ordinary bytecode.
- [x] Pass the initial direct follow-up: 1,758 cases across six disjoint target
      selections, one existing ignored. Five new real-page contexts and three
      deferred debugger cases preserve direct `.ts` sources. Official MCP now
      verifies direct `reportError`/console at 1:1 and a Blob callback throw at
      1:44. Formatting, seven-library, changed-target and bridge/launcher test
      Clippy pass; runtime/test hashes remain fixed during final acceptance.
- [x] Capture direct-expression token origins in the existing parser and join
      them to the final AST only after exact expression equality. Preserve
      separate child order for repeated identifiers, constructor arguments,
      intermediate members, left-associative operators and erased assertions.
      Bound temporary capture bytes; opaque class/template/JSX descendants
      keep independent unavailable origins.
- [x] Verify twenty-two public expression failures and eighteen parser shapes,
      including default parameters, assignment/update references, object keys,
      array values and repeated operands. Ordinary ASTs, executable bytes and
      constants match; oversized capture is refused. Awaited/non-awaited
      dependencies retain their own expression sources; all 177 bridge cases
      pass, including the explicit opaque-template unknown control.
- [x] Pass the direct-expression follow-up: 1,762 cases across six disjoint
      targets, zero failures and one existing ignored. Four more real-page
      contexts, three more deferred debugger contexts and three queued direct
      MCP errors preserve original `.ts` operand/argument/key positions.
      Formatting and related library/integration/test Clippy pass; eleven Rust
      inputs, three Cargo inputs and runtime/test hashes remain fixed.
- [x] Preserve template substitution token offsets through embedded direct
      parsers while sharing the enclosing source-capture allocation charge.
      Match ordinary ASTs, bytes and constants in five template shapes;
      a 50,000-substitution expression cannot reset that budget per child.
- [x] Bind defined-field class-expression member bodies/defaults/initializers
      to checked class-expression evidence after exact replay equality.
      Verify fifteen template/class failure scenarios, original UTF-16 and
      CR/CRLF/LS/PS coordinates, dependency failures before/after await,
      constructor prologues and recursive class bodies. Preserve nested
      capture refusal and dynamic/assignment-field unavailable controls;
      all 182 bridge library cases pass.
- [x] Pass the embedded-source follow-up: 633 cases across thirty-nine disjoint
      targets, zero final failures and sixteen existing ignored. All bridge
      library/integration targets pass (253 cases), plus forty-four diagnostic
      groups, 312 launcher cases, nineteen real pages and five MCP console
      cases. Five more page contexts and two deferred debugger contexts pass;
      queued template/class MCP errors retain original 5:2/6:2. Formatting and
      related Clippy pass; final source/runtime/test identities are recorded.
- [x] Carry original classic JSX component-tag and embedded-expression tokens
      through the same bounded direct origin collector. Retain child order
      through generated calls/props, fragments, nested/attribute elements,
      spreads and templates; generated scaffolding keeps unavailable positions.
      Verify fourteen public failure forms, four unavailable controls and
      original factory/getter callback sources.
- [x] Verify six JSX AST/bytecode/constant equivalence shapes, ten UTF-16/line-
      terminator coordinate contexts, dependency failures before/after await
      and queued callback retention/navigation cancellation. A 50,000-child
      parser case shares its capture budget. Fix JSX LS/PS and comment trivia
      after a failing regression; twelve scanner cases retain original byte
      ranges without splitting UTF-8. All 192 bridge and 254 BlueTS library
      cases pass.
- [x] Pass the JSX follow-up: 937 cases across forty-seven disjoint targets,
      zero failures and twenty-two existing ignored. All bridge targets pass
      (263 cases); BlueTS library/seven JSX targets add 294, diagnostic groups
      add forty-four, launcher adds 312, real pages nineteen and MCP console
      five. Formatting, seven-library/integration Clippy and complete BlueTS/
      bridge/launcher test Clippy pass. Seven Rust inputs, four Cargo inputs,
      four runtime executables and forty-six test executables remain fixed.
- [x] Record class element paths, moved field assignments, pending key prefixes,
      inferred-class wrappers and actual constructor insertion offsets in the
      production lowerer. Join only after exact class replay. The 52 direct
      declaration/expression/field-mode cases pass; thirty-four missing origins
      were reproduced before the fix. Bound path capture independently of
      executable node slots; six shape/output controls and a 50,000-member
      capture refusal pass.
- [x] Verify moved initializer/key coordinates through twenty UTF-16/newline
      contexts, eight dependency cases before/after await, queued retention/
      navigation and a constructor prefix with hoisted computed bindings.
      Synthetic field-write errors keep unavailable positions.
- [x] Preserve each decorator expression's copied source through application
      validation. Keep source scalars in the existing private, heap-owned
      records; ordinary bytecode stays identical. Ten JavaScript scenarios
      pass under normal/aggressive GC, including stacked decorators, invalid
      returns and caught getter/callback noise; dynamic throws stay unknown.
- [x] Pass the class-path/application follow-up: 2,000 cases across forty-three
      disjoint targets, zero failures and sixteen existing ignored. BlueJS
      library/decorator/diagnostic targets pass 1,392 cases; all bridge targets
      pass 272; launcher, real pages and MCP add 336. Three new page contexts
      and two deferred debugger contexts pass. MCP keeps a queued key at 7:2
      and application validation at 2:2. Formatting and related Clippy pass;
      thirteen Rust inputs, three Cargo inputs and runtime/test hashes stay fixed.
- [x] Scope native/Proxy decorator dispatch and accessor-result reads to each
      saved expression. Restore the caller's throw metadata after successful
      calls, including tail-call transitions, so caught getter noise cannot
      impersonate a later native failure. Twenty-three JavaScript scenarios
      pass under normal/aggressive GC; three ordinary callback cases and two
      dynamic native/Proxy controls pass. All forty-seven diagnostic groups
      pass. Add six direct native/Proxy cases and eight dependency cases across
      both field modes and before/after await; all class-source groups pass.
- [x] Pass the native/Proxy follow-up: 2,302 cases across sixty-nine disjoint
      targets, zero failures and sixteen existing ignored. Thirty-one BlueJS
      targets pass 1,694 cases; all bridge targets pass 272; launcher, real pages
      and MCP add 336. Two new page and deferred debugger contexts pass; MCP
      keeps both new typed scripts at original 2:2. Formatting and related
      Clippy pass. Record source revisions and immutable executable hashes.
- [x] Record namespace wrapper/body paths and reference-rewrite contexts in the
      production lowerer, with a shared 8 MiB capture budget. Replay complete
      root namespace groups before binding original declarations and exported
      initializer values. Map computed enum initializer values separately from
      generated object writes. All forty-two previously missing positions pass
      across twenty-one shapes and both field modes, including dotted/nested/
      merged namespaces and function/enum merges.
- [x] Preserve intermediate qualified-name origins when rewritten tokens share
      the same source bounds. Track consumed token cursors rather than infer
      unchanged syntax from byte ranges. Eight chain controls pass; copied
      namespace receivers and generated export/enum writes remain unmapped,
      while the original namespace member read keeps its location.
- [x] Verify twelve unchanged AST/bytecode shapes, a 25,000-declaration path
      refusal and an oversized reference map with fewer than thirty-two
      executable slots, ten UTF-16/newline contexts, sixteen dependency cases before/
      after await and queued retention across actual major GC/navigation.
      Admit constant NaN/infinity/negative-zero enums using structural replay
      plus numeric bits; changed NaN payloads and zero signs fail bytecode/
      source certification atomically. All 213 bridge library cases pass.
- [x] Pass the namespace/enum follow-up: 667 cases across thirty-nine disjoint
      targets, zero failures and sixteen existing ignored. All thirty-five
      bridge targets pass 284 cases; forty-seven diagnostic groups, 312 launcher,
      nineteen real-page and five MCP console cases pass. Three new page and two
      deferred debugger contexts preserve 2:2; MCP verifies namespace and enum
      scripts at 2:2. Formatting and related Clippy pass. Twelve Rust inputs,
      three Cargo inputs, three runtime and thirty-nine test executable hashes
      stay fixed throughout final acceptance.
- [x] Replay checked flat variable patterns before binding their root, renamed/
      skipped/rest children and original default expressions. Bind written
      variable, parameter and catch names from checked token prefixes, including
      erased `this` parameters and constructor properties. Forty-four missing
      positions across twenty-two shapes/both field modes now pass; eight public
      instruction-owner checks, four AST/bytecode controls and four contextual
      keyword shorthand controls pass. Generated
      export bindings remain gaps, and existing unsupported pattern forms remain
      refused by the direct grammar.
- [x] Preserve parameters and bodies through anonymous default-function and
      default-class wrappers. Twenty-four dependency/default-export contexts
      before/after await and both field modes pass, alongside twenty UTF-16/
      newline contexts, callback/dynamic-source controls and queued retention
      across actual major GC/navigation. All 221 bridge library cases pass.
- [x] Reproduce and fix duplicate page-module errors and stalled ready siblings
      after an async dependency completes. Cache every catchable evaluation
      failure, including string limits; reject its static parents/waiters through
      the retained error and continue ready siblings. Standalone/page-owned
      source controls, sibling completion and language-limit/host-abort checks
      pass. Instruction-limit aborts still escape unchanged.
- [x] Add fingerprinted diagnostic copied-token map anchors for owner-selected
      emitted BlueTS. Preserve ordinary maps and generated JavaScript bytes;
      helper/scaffolding gaps remain unmapped. Exact dependency positions now
      pass for JavaScript, direct BlueTS and emitted BlueTS. Frozen artifact
      fixtures select the same map mode and retain full map/JavaScript equality.
- [x] Pass the binding/default-export and module-parent follow-up: 2,314 cases
      across fifty-nine disjoint targets, zero failures and twenty-nine existing
      ignored. BlueJS adds 1,398, BlueTS 287, all bridge targets 292, launcher
      313, real pages nineteen and MCP console five. Formatting and related
      Clippy pass. Record twenty-two Rust inputs, four Cargo inputs, three
      runtime and fifty-nine test executable hashes, with explicit source/test
      refinements and the unchanged earlier BlueJS cohort.
- [x] Attribute computed `ToPropertyKey` failures to the written expression,
      preserving the ordinary bytecode. Reproduce twenty-eight wrong locations
      across fourteen object/class/destructuring shapes and normal/aggressive
      GC; all now pass. Ten callback/native controls preserve known or explicitly
      unknown origins, including caught coercion noise and a revoked Proxy.
- [x] Preserve the final written class key through the sequence that runs pending
      erased field keys. Exact class replay and an already certified terminal
      expression establish the conversion origin; hidden binding/assignment
      nodes keep gaps. Reproduce both missing assignment-field-mode positions;
      sixteen direct contexts, three callback controls and twenty dependency
      contexts across both field modes and before/after await pass.
- [x] Pass eighteen deferred host contexts for object/class key conversion:
      JavaScript, direct BlueTS and emitted BlueTS each preserve original 2:2
      in classic, module and awaited-module execution.
- [x] Pass the computed-key follow-up: 2,070 cases across fifty-two disjoint
      targets, zero failures and sixteen existing ignored. Fourteen BlueJS
      targets pass 1,437 cases, all thirty-five bridge targets pass 295, launcher
      passes 314, real pages nineteen and MCP console five. Five added page
      contexts and two queued MCP keys retain original positions. Formatting,
      seven-library Clippy, bridge/launcher test Clippy and changed integration
      Clippy pass. Record seven Rust, six Cargo, three runtime and fifty-two
      test executable hashes with immutable validation inputs.
- [x] Reproduce and charge the VM's retained uncaught source filename until the
      next script clears it: the public long/short-name comparison changes from
      zero to the exact 4,095-byte difference. Count copied pending/uncaught
      locations, native decorator scopes and temporarily displaced Rust-owned
      provenance alongside the document's existing scheduling payload.
- [x] Reproduce three byte-cap bypasses in ErrorEvent snapshots, active catch
      handlers and iterator cleanup. Scope charges follow active handler/source
      ownership and release on every exit; admission refuses callback side
      effects after exhaustion, while short-name controls and navigation pass.
- [x] Reproduce five debugger source-accounting gaps: classic/module roots,
      nested classic/module calls and one intermediate caller. Paused filename
      copies are charged exactly once and released by resume. All fifty-two
      diagnostic and thirteen scheduling-limit groups pass.
- [x] Pass the native-payload follow-up: 2,193 cases across fifty-five disjoint
      targets, zero failures and sixteen existing ignored. Seventeen BlueJS
      targets pass 1,560 cases, all thirty-five bridge targets pass 295, launcher
      passes 314, real pages nineteen and MCP console five. Formatting,
      seven-library Clippy, bridge/launcher test Clippy and changed integration
      Clippy pass. Fifteen Rust and six Cargo inputs remain fixed throughout
      validation; record three runtime and fifty-five test executable hashes.
- [x] Reproduce missing emitted-template operand anchors. Diagnostic copy maps
      certify template character boundaries without changing emitted JavaScript
      or ordinary maps. Six template scenarios, segment-overflow refusal and
      existing generated-helper gaps pass in all ten public source-map groups.
- [x] Verify thirty-six deferred implicit-protocol contexts across JavaScript,
      direct BlueTS and emitted BlueTS: classic, module and awaited module
      execution each retain the original operand at line 2, column 2.
- [x] Pass formatting and complete workspace/all-target build and Clippy with
      warnings denied on the final fifteen Rust inputs. An equivalent heap-test
      initializer fixes the full-run Clippy warning without changing its limits.
- [x] Keep certified operands through implicit heritage, spread, template and
      delegated-iterator protocols. Forty-eight GC-varied source contexts and
      fourteen callback/unknown/forwarding controls pass in all fifty-six
      diagnostic groups, including native-entered AsyncFromSync continuations.
- [x] Reject primitive `Proxy.ownKeys` trap results before boxed-property reads.
      Three public regression groups pass 176 consumer/primitive/GC contexts,
      eight no-getter-side-effect checks and four valid-object/error controls.
- [x] Pass the complete fixed-input workspace replay: 9,312 passed, zero failed,
      233 existing ignored across 687 result groups, including doc tests, real
      Core/child pages, HTTP probes and all five official MCP console cases.
      All 663 test executables and seven runtimes retain their recorded hashes.
- [x] Compare all 102,926 pinned Test262 modes against M1p: identical source
      hashes, expectations, features/flags, statuses and actual kind/phase/reason.
      Results remain 102,921 pass, one stale corpus and four exclusions, with
      zero changed modes and no new failure or timeout. Runner exit 1 represents
      the five unchanged non-pass modes; the complete comparison succeeds.
- [x] Exercise twenty admitted producer/profile routes in normal and deferred
      execution: 160 throw/report/rejection/dynamic-code contexts preserve exact
      authored UTF-16 positions or explicit absence after a later script runs.
      Reuse File DOM's verified console declaration instead of adding duplicate
      classic, System or module-console ambient declarations.
- [x] Preserve System module awaits in async execute bodies, using existing
      lexical execution scopes. Four module-await and eight nested/text controls
      pass; ES5/ES2015 refuse atomically at the original await token.
- [x] Preserve ES5 class parameters, bodies, keys and heritage through exact
      relocation, with generated scaffolding unmapped. Public map validation
      reproduces and fixes malformed class/object-spread maps; constructor,
      derived/super, computed-key and accessor controls cover LF/CRLF and UTF-16.
- [x] Retain System class statement boundaries and leave generated CommonJS
      class-publication assignments unmapped. Public compile/parse/map coverage
      checks four System class shapes and both CommonJS/System publication.
      Thirteen source-map, four System-map and two System-runtime groups pass.
- [x] Distinguish authored callbacks and generated-helper gaps in twenty-eight
      normal/deferred emitted-profile contexts. Real Core/child pages exercise
      all six emitted profiles and both console selectors through window.onerror.
- [x] Pass the fixed-input profile replay: all 131 BlueTS, 35 bridge and 22
      launcher test executables plus the official MCP console selection pass
      1,843 cases, zero failures and 228 existing ignored. Formatting and full
      workspace/all-target build/Clippy pass; eighteen Rust and 13,849 workspace/
      compiler inputs, seven runtimes and all 189 selected test hashes stay fixed.
      Reuse the earlier Test262 result only after verifying identical adapter/
      worker/runner hashes, all 56,979 pinned corpus files and the saved inventory.
- [x] Keep the iterated operand through for-in, for-of and for-await acquisition
      and step operations. Reproduce 28 incorrect locations across normal and
      aggressive-GC configurations; ten callback/rejection/dynamic controls
      preserve independent origins. Five AST/inventory/recursive-bytecode
      comparisons preserve ordinary instructions and debugger identities.
- [x] Resume three loop protocols after later unrelated code and observed major
      collection, retaining native, authored-getter and unknown-getter origins.
      Twenty-one deferred JavaScript/emitted contexts and three added real-page
      contexts pass. Five direct BlueTS refusal controls preserve its existing
      excluded loop surface; emitted await fixtures retain original module markers.
- [x] Pass loop ownership acceptance: 2,303 cases across 70 disjoint executables,
      zero failures and 16 existing ignored. Full BlueJS library, related
      iteration/async/GC/resource targets, all 35 bridge targets, launcher library,
      real pages and official MCP console pass. Formatting and workspace/all-target
      build/Clippy pass on five fixed Rust inputs. Fresh Test262 matches M1p's
      complete 102,926-mode source/result inventory with zero changed modes.
- [x] Preserve with-object, disposable-initializer and iterated-binding owners
      through implicit conversion/resource admission. Reproduce 30 wrong native
      locations and eight wrong with-reference locations under normal/aggressive
      GC; typeof/delete references and with-var resolution/store use their
      written names. Eleven AST/inventory/recursive-bytecode controls preserve
      executable instructions, anonymous naming and debugger identities.
- [x] Preserve distinct known/unavailable getter/body/disposer origins in eleven
      admission/cleanup controls, including synchronous/asynchronous iterator
      close after a disposer throws. Four suspended resource contexts execute
      later code, observe major collection and retain their own resumed source.
      Forty authenticated classic-profile contexts, five added real Core/child
      contexts and four direct BlueTS refusal controls pass.
- [x] Pass admission/reference acceptance: 2,358 cases across 77 disjoint
      executables, zero failures and 16 existing ignored. Full BlueJS library,
      with/using/GC/resource targets, all 35 bridge targets, launcher library,
      real pages and official MCP console pass. Formatting and workspace/all-target
      build/Clippy pass on six fixed Rust inputs. Fresh complete Test262 matches
      M1p's 102,926-mode source/result inventory with zero changed modes.

#### Remaining implementation and verification

- [ ] Complete remaining expression-dispatch, decorator-application and
      compiler-generated provenance. Acceptance: audit the compiler's operation
      owners and lowering aliases against the source inventory; public failures
      retain the executing original expression or an explicit generated-only
      gap. Ordinary AST inventories and recursive instruction bytes stay equal.
- [ ] Complete remaining direct BlueTS expression and
      generated-helper provenance, remaining decorator application and class/key
      precision, plus emitted-profile/module/debugger
      precision and lifecycle acceptance. Acceptance: original callback throws
      and generated-helper failures are distinguished through JavaScript,
      direct BlueTS and all six admitted emitted profiles (`Standard`,
      `ModulePageConsole`, `Es5`, `System`, `LegacyDecorators`, and
      `LegacyDecoratorsWithMetadata`). Cover each profile's admitted classic/
      module and deferred execution paths; assert exact original filenames and
      UTF-16 coordinates, with unknown dynamic/helper bodies left unavailable.
- [ ] Complete the queued-origin/protocol matrix and integrated finalizer/
      disposal acceptance beyond the verified module and disposal cases.
      Acceptance: mapped throws, explicit reports and rejected Promises keep
      their own locations through microtasks, timers, native events and
      resumable/module work after a later script runs. Finalizers and disposal
      preserve the failure that escapes; known and unavailable callbacks do not
      borrow one another's locations.
- [ ] Complete retained-origin accounting and the integrated
      cancellation/navigation/GC/resource matrix across all source producers.
      Acceptance: observe actual major collection before retained callbacks;
      then verify exact sources, cancel/navigation cleanup and stale-generation
      refusal. Exercise map admission and retained-filename byte limits at the
      public producer boundary, prove atomic refusal and fresh-document
      recovery, and verify that released owners stop contributing native bytes.
- [ ] Revalidate related Rust/Core/MCP, workspace/all-target Clippy/build and
      the complete pinned Test262 inventory after the remaining source
      precision/lifecycle changes; finalize the milestone documentation.

#### Final acceptance gates

- [x] Reproduce missing source coordinates through the real Core/child boundary.
- [x] Bind certified source positions to exact live program generations.
- [ ] Preserve throw/reject/report origins across synchronous and queued work.
- [ ] Map JavaScript, direct BlueTS and compiler-emitted source positions.
- [x] Carry bounded copied locations through Core/MCP and retain unavailable cases.
- [ ] Verify cancellation, stale generations, navigation, GC and resource limits.
- [x] Pass the related Rust and real Core/MCP acceptance selections.
- [x] Pass formatting and workspace/all-target Clippy/build.
- [x] Compare the complete pinned Test262 inventory against M1p.
- [ ] Finalize evidence, documentation and a detailed English commit.

Initial inspection found that `report_host_error_value` deliberately wrote an
empty filename and zero line/column. The VM has a generation/code-unit/PC
throw sidecar for terminal debugger exceptions, while the current direct
BlueTS safe-point map may describe an enclosing function rather than its
throwing statement. Source-map support must extend the real reporting path
and preserve certified provenance rather than infer a nearby location.
The first real page regression failed before production changes: its global
handler observed `location::0:0` instead of the throwing inline script's
canonical identity and line 2, column 1 (one failed, fifteen filtered, 2.33s;
`/private/tmp/blueice-error-origins-red.log`). The implementation started
with immutable, owner-bound instruction/source metadata that follows compiled
closures rather than a mutable ambient filename or a permanent VM root.
The first three public registry groups pass: exact source identity, atomic
refusal of unknown units/operand offsets, explicit gaps, immutable old code
clones, retained owned-code maps and malformed/overlapping coordinate controls
(`/private/tmp/blueice-error-origin-maps.log`). The next public VM regressions
exercised mapped `ErrorEvent` fields and the active `reportError` call site
before that reporting connection was implemented. Both failed with `:0:0`
instead of the installed source location (three passed, two failed;
`/private/tmp/blueice-error-origin-vm-red.log`). The VM now captures mapped
throw sites and restores active source contexts after nested interpreters;
the follow-up passes all seven source-map/VM groups and all twelve existing
error-reporting groups (nineteen passed, zero failed;
`/private/tmp/blueice-error-origin-vm-followup.log`). This verifies source
identity/boundaries, metadata capacity charges, mapped throws, active reports,
unknown raw code and nested interpreter restoration. A further public control
for unmapped dynamic `eval`/`Function` code failed because it inherited the
mapped caller's position (`/private/tmp/blueice-error-origin-unknown-red.log`).
Separating diagnostic origin preservation from debugger fallback fixed this
case; it passes in the final replay below. Final host/provenance, accounting,
transport and conformance gates remain open.
The preceding owner follow-up passed all 39 cases across three complete targets:
eleven source-map/VM groups, twelve existing error groups and all sixteen real
Core/child page cases, including the original red fixture
(`/private/tmp/blueice-error-origins-owner-followup.log`). The page installer
charges immutable map bytes with code and rolls back failed metadata admission.
That prototype mapped classic JavaScript root statements only. Nested
statement support followed in the next implementation step; expression
provenance, direct/emitted BlueTS, modules, asynchronous origin retention and
copied Core/MCP locations remain required before M1q or section H can close.
The expanded real fixture confirmed a further gap: a block throw inherited
line 2, column 1 from its outer `if`, and a function throw remained unlocated;
both needed their own statement at line 3, column 3 (one failed, fifteen
filtered, 4.46s; `/private/tmp/blueice-error-origins-nested-red.log`). The
implementation then recorded parser statement provenance against compiler AST
instruction owners instead of accepting the enclosing statement/function.
The parser/compiler statement path now passes the expanded public/core rerun:
40 passed, zero failed, across all three targets
(`/private/tmp/blueice-error-origins-statements.log`). The real fixture verifies
line 3, column 3 inside both blocks and functions. Public compiler checks also
cover repeated function bodies, Annex B wrappers, labels and directive
normalization. The broader library/registry/lifecycle selection subsequently
passed 1,691 tests across five targets, with one existing ignored
(`/private/tmp/blueice-error-origins-foundation-regressions.log`). Focused
Clippy initially found two type-complexity warnings; named type aliases fixed
both, and the three-library rerun passed in 27.58s
(`/private/tmp/blueice-error-origins-foundation-clippy-fixed.log`). This alias-only
correction followed the broader run; the final current-source public replay
then passed all 40 cases across its three targets
(`/private/tmp/blueice-error-origins-statements-final.log`). All 27 recorded Rust
source hashes match that replay's manifest
(`target/source-diagnostics-foundation-source-20261015.json`). Formatting also
passed (`/private/tmp/blueice-error-origins-foundation-format.log`). The combined
checkpoint has 1,731 passing tests across eight distinct targets and one
existing ignored. Full workspace/all-target Clippy/build, final Core/MCP,
pinned Test262 and fresh coverage were not run for this checkpoint; their
previous M1p results are not substituted for M1q acceptance.

The cleanup follow-up first reproduced four incorrect origins among five
public cases (`/private/tmp/blueice-error-origins-cleanup-red.log`), plus a
real Core/child finalizer reporting empty/zero coordinates
(`/private/tmp/blueice-error-origins-cleanup-core-red.log`). Saving provenance
with pending finalizers fixed synchronous cleanup; expanded tests then exposed
a cross-script generator epoch collision (14 passed, one failed;
`/private/tmp/blueice-error-origins-cleanup-suspended-red.log`). A fresh
propagation epoch on resumption fixed the collision. The three-target replay
passed all 43 cases, including real page finalizer/iterator cleanup and
cross-script generator coordinates
(`/private/tmp/blueice-error-origins-cleanup-final.log`). The expanded current
source selection passed all 1,837 cases across twenty targets, with one
existing ignored and zero failures
(`/private/tmp/blueice-error-origins-cleanup-regressions.log`). This includes
the three libraries, all fifteen diagnostic groups, twenty debugger groups,
generator/async collection, completion, native Promise and scheduling limits,
and all sixteen real page cases. The source-coordinate page fixture verifies
six contexts, including finalizer/iterator cleanup and a generator resumed by
another script. All nineteen recorded Rust hashes match the complete run's
manifest (`target/source-diagnostics-cleanup-source-20261015.json`). Formatting
and focused three-library Clippy with `-D warnings` pass; Clippy completed in
1m 11s. See `target/source-diagnostics-cleanup-validation-20261015.json`.
Finalizer filename copies join generator managed bytes and async/module
continuation scheduling bytes. The broader M1q acceptance requirements above
remain open; this follow-up does not close section H.

The console-copy follow-up now reproduces the next boundary failure. Both
public VM tests lose the copied source object (two failed, fifteen filtered;
`/private/tmp/blueice-console-sources-vm-red.log`), and the real Core/child
console reply loses the known line 2, column 1 location (one failed, sixteen
filtered, 2.27s; `/private/tmp/blueice-console-sources-core-red.log`). Optional
source records, combined console-byte admission and independent Core/MCP
validation are the next implementation step; unknown locations must stay
absent.
The three public IPC groups also failed before implementation: valid source
data was discarded, eleven malformed source variants were accepted, and
filename bytes escaped the existing batch bound
(`/private/tmp/blueice-console-sources-ipc-red.log`). The first mapped-console
implementation passes these IPC groups and eighteen of nineteen VM groups,
but a new control exposes an unrelated unknown timer error inheriting the
previous script's filename (`/private/tmp/blueice-console-sources-foundation.log`).
Root reporting was then separated from callback reporting before the
complete Core/MCP rerun.
The corrected source run passes all nineteen VM groups and all three IPC
groups. The dedicated Core selection also passes both tests, including ten
malformed-reply variants and addressed-realm containment (818 unrelated tests
filtered). The first real-page/MCP attempts stop at startup: the sibling Core
binary still predates the private v61 change while the child is rebuilt.
The nine library/public targets pass (2,095 cases, one existing ignored), but
the seventeen real page cases and three official MCP cases require a matching
Core/child binary rebuild and rerun; their failed startup attempts are not
counted as acceptance. Evidence is in
`/private/tmp/blueice-console-sources-regressions.log`,
`/private/tmp/blueice-console-sources-core-validation.log` and
`/private/tmp/blueice-console-sources-mcp.log`.
After rebuilding the matching Core/BlueJS-host binaries (36.16s), all seventeen
page cases pass. Reusing the unchanged Cargo-built official MCP test executable
then passes all three console cases (nine filtered, 1.71s), including known
JavaScript positions, unavailable direct BlueTS positions, native events,
Promise/timer work, cancellation and navigation isolation. The final disjoint
selection has 2,117 passes across twelve targets and one existing ignored:
nine successful library/public targets from the original run, the successful
page rerun, the two-case Core selection and the three-case MCP selection.
The failed startup attempts remain recorded above and are superseded by the
matched-runtime acceptance. All thirteen recorded Rust hashes remain unchanged;
runtime/test executable hashes are in `target/console-sources-source-20261015.json`.
Formatting and focused six-library Clippy with `-D warnings` pass; Clippy
completed in 2m 48s. The complete evidence is recorded in
`target/console-sources-validation-20261015.json`. The full
M1q source-producer/queued-origin and final workspace/Test262 gates remain open.
See [the in-progress contract](SOURCE_DIAGNOSTICS.md) for the immutable ownership
and admission model and the remaining expression/typed/async/transport work.

The rejection-origin follow-up starts with nineteen public scenarios covering
native rejection, executor throws, missing handlers, thenables, adoption,
species, awaits, finalizers and async generators. Eighteen known origins are
missing; the unmapped-callback control correctly remains absent (one failing
test group, nineteen filtered, 0.03s;
`/private/tmp/blueice-promise-origins-red.log`). Promise records, queued
settlement jobs and native finalizer thunks must retain their own source
metadata before the queued-origin gate can close.

The Promise-origin follow-up now passes 32 forwarding, replacement, unknown,
thenable, species, combinator, await and async-generator source scenarios.
Promise records and settlement jobs retain native provenance separately from
rejection values. Entering JavaScript masks inherited native provenance;
rejected awaits publish fresh epochs. Weak native `finally` thunk metadata
survives a later script and full collection without rooting its owner.
Retained/job filename capacities are charged, and the document byte-cap
fixture accepts short filenames but refuses the same work with 8 KiB names.
The initial collectibility fixture used only minor GC; forcing major GC
correctly verifies release of promoted owners. All 23 diagnostic groups pass.
The official three-test MCP selection verifies original inline line 4, column
25 after `finally`, unavailable direct BlueTS metadata, cancellation, late
handlers and navigation. The first sandboxed socket attempts failed before
execution; the same compiled Launcher/browser/MCP executables pass with local
socket access. The final disjoint selection passes 1,837 tests over twenty
targets with one existing ignored; format and six-library Clippy also pass.
Source/runtime hashes and per-target results are recorded in
`target/promise-origins-source-20261015.json` and
`target/promise-origins-validation-20261015.json`. This is a completed
implementation step within M1q; the ten final gates remain at 3/10. Queued
async-generator request provenance, module failure fan-out, async-disposal
interactions and the remaining source producers still need full acceptance.

The next queued async-generator source regression initially loses four of
nine origins: a queued throw after await/yield, an awaited finalizer and a
return operand's throwing constructor getter
(`/private/tmp/blueice-generator-request-origins-red.log`). Retaining the
request origin fixes those nine cases. An expanded twelve-scenario follow-up
then exposes two `yield*` completion bugs: an awaited finalizer loses the
delegate's rejection origin, and an abrupt finalizer incorrectly keeps the
superseded delegate origin
(`/private/tmp/blueice-generator-request-origins-first.log`). The verified
implementation now retains request-owned immutable metadata,
replacement origins and fresh propagation epochs. Twelve source scenarios
pass under normal and aggressive GC (24 executions). A separate same-value
FIFO control preserves distinct known/unknown origins across scripts and
collection, then observes a major GC and verifies the generator is released.
Queue source capacities join managed heap accounting; an over-budget
replacement preserves the original request atomically. The first collection
fixture used only 100 allocations; its corrected release check requires
observed major collection under greater allocation pressure.

The real MCP regression fails with an absent source against the unchanged
prechange Core/host binaries, then passes inline line 5, column 25 after
queued `.throw()` and `finally` with fresh binaries. All three official MCP
console tests and seventeen real browser tests pass. The BlueJS selection
has fifteen successful targets plus the corrected complete diagnostic target
(26 passed); no failed diagnostic attempt is counted toward the final totals.
The first concurrent Launcher run had four Python helper startup/restart
failures; a serial run passed those four but hit another startup timeout.
After other validation finished, the identical Launcher executable passed all
307 cases with four test threads (one existing ignored, 2.87s).
The final disjoint selection passes 1,841 cases across twenty targets;
format and focused six-library Clippy pass. Seven Rust input hashes,
current/prechange runtime hashes and per-target results are recorded in
`target/generator-request-origins-source-20261015.json` and
`target/generator-request-origins-validation-20261015.json`. The complete
queued-origin matrix, module/disposal interactions, remaining source producers
and final workspace/Test262 gates stay open; M1q remains at 3/10 final gates.

The module-origin regression initially reports `:0:0` for dependency errors
and no dynamic-import rejection notifications with an installed idle loader
referrer (`/private/tmp/blueice-module-origins-red.log`). Cached module errors
now retain their native origin with the original rejection value. Async
parents share immutable metadata; cached replay republishes a fresh throw
epoch, and import waiters receive the dependency's source. Module awaits
restore the ambient execution state. Explicit module execution depth keeps
real execution separate from loader referrers and preserves debugger checkpoint
cleanup. A weak native ledger accounts for allocation and filename capacities
through displaced graph ownership, without adding JavaScript roots.

The final current-source selection passes all 1,487 BlueJS cases across
twenty-six targets, including 1,204 library and thirty diagnostic groups.
Bridge, Launcher and real Core/child page tests add 491 passes and one existing
ignored; all three official MCP console cases pass, for 1,981 passes over
thirty disjoint targets. Formatting, matched Core/host/Gatekeeper builds and
focused six-library Clippy pass. The first clean-cache MCP attempt lacked the
Gatekeeper executable and stopped during setup; rebuilding it resolves all
three failures. Final validation includes the last debugger resource-refusal
root cleanup. Thirteen Rust input hashes, three runtime hashes, test executable
hashes and per-target receipts are recorded in
`target/module-origins-source-20261015.json` and
`target/module-origins-validation-20261015.json`. Incremental caching and
dev/test debug info are disabled after the requested build-artifact cleanup.
These tests validate retention of owner-certified module maps; page-module
source production, the remaining await/disposal matrix and the broader final
acceptance gates remain open. M1q stays at 3/10.

The disposal-origin follow-up first reproduces eighteen missing/incorrect
sources in ten scenarios under normal and aggressive GC
(`/private/tmp/blueice-disposal-origins-red.log`). Generated JavaScript rethrows
misattribute rejected awaits and unknown code; the private async stack helper
loses known origins, and later successful synchronous disposers can erase an
earlier failure's source. The compiler now shares one awaited disposal loop
between declarations and async stacks. A native opcode folds caught failures
into the enclosing pending completion, retaining value identity and provenance
through suspension and intrinsic suppression. Catch/finalizer origins use the
existing frame field and its capacity accounting; no diagnostic property or
additional JavaScript root is introduced.

All thirty-two diagnostic groups pass, including twenty disposal-source
executions, module and queued-generator disposal, exact suspended filename
charges, cross-script major GC and owner release. The first owner assertion
collected in the same job that cleared references; the corrected test ends
that job and its checkpoint before observing collection. Resource tests also
verify that replacing the global `SuppressedError` constructor cannot intercept
intrinsic suppression. The unchanged old Core/host reproduce an absent source
through the new MCP chain; matching new binaries preserve inline line 5,
column 25. All three official MCP console cases and seventeen real page cases
pass. The final disjoint selection has 1,547 BlueJS passes across thirty
targets, 491 bridge/Launcher/page passes and three MCP passes: 2,041 passed
over thirty-four targets, one existing ignored. Formatting, six-library Clippy
and matched Core/host/Gatekeeper builds pass. Fifteen Rust hashes, old/new
runtime hashes and per-target receipts are in
`target/disposal-origins-source-20261015.json` and
`target/disposal-origins-validation-20261015.json`. M1q remains at 3/10;
remaining source producers, the complete protocol/integrated acceptance matrix
and final workspace/Test262 gates stay open.

The expression/parameter follow-up initially fails all nine source scenarios:
operands borrow statement coordinates, and parameter/arrow failures lack a
location (`/private/tmp/blueice-expression-origins-red.log`). The first producer
fix passes seven; source-only pattern slots and shared named/unnamed arrow
clone overlays resolve the remaining two. The expanded twenty-three-scenario
matrix covers intermediate operators/constructors, parameter binding/defaults,
template placeholders and callbacks, comments, repeated syntax and UTF-16/CRLF
positions. Diagnostic snapshots share existing `SourceText` buffers and charge
cumulative clone/span work before allocation, with an 8 MiB cap. Wide-list and
excessive-capture controls pass. Public debugger ordinals remain unchanged;
ordinary syntax and bytecode comparisons verify metadata-only compilation.

The final current-source selection passes 1,611 BlueJS cases across thirty-eight
targets, including 1,205 library cases and all thirty-five diagnostic groups.
Bridge, Launcher and real Core/child page tests add 491 passes and one existing
ignored; all three official MCP console cases pass. The unchanged old runtime
reports inline column 25; matching new Core/host report the actual RHS call
at line 5, column 36. The disjoint total is 2,105 passes across forty-two
targets. Formatting, matched Core/host/Gatekeeper builds and focused Clippy
pass. Initial library-test compilation found three private template-helper
calls using old signatures; the corrected complete selection passes. Twenty
Rust input hashes, old/new runtime hashes and per-target receipts are recorded
in `target/expression-origins-source-20261015.json` and
`target/expression-origins-validation-20261015.json`. Method/class/decorator and
other generated provenance, remaining source producers and integrated/final
workspace/Test262 gates stay open. M1q remains at 3/10 final acceptance gates.

The requested compilation cleanup runs after the verified selection. Source
hashes, test/runtime hashes and logs remain available; the corresponding old
binaries and caches are removed, apart from the MCP executable in use. The
cleanup verifies all twenty-two modified main-checkout files, both modified
worktree files and all thirty-two top-level report manifests without changes.
Its removal inventory and result are recorded in
`/private/tmp/blueice-compiled-cleanup-plan-20261015.json` and
`/private/tmp/blueice-compiled-cleanup-result-20261015.json`. This cleanup does
not rerun compilation or change any final acceptance gate.

The class-lowering follow-up reproduces fifteen missing or coarse locations in
eighteen public `ErrorEvent` scenarios before production changes
(`/private/tmp/blueice-class-origins-red.log`). Scoped overlays preserve cloned
constructor/accessor parameters and bodies, static-block statements and only
the original initializer subtree of a lowered field. Decorator grammar and
receiver-preserving compilation capture identifier/member owners. The expanded
thirty-two source cases, three generated/dynamic unknown controls and four
cross-script major-GC cases pass; ordinary syntax, code-unit counts and bytes
remain identical. No temporary AST address survives compilation.

The final selection passes 1,627 BlueJS cases across thirty-three targets,
including 1,205 library cases and all thirty-eight diagnostic groups. Bridge,
Launcher and real pages add 491 passes and one existing ignored; all three
official MCP console cases pass, including queued getter/field throws at
line/column 5:78 and 6:74. The disjoint total is 2,121 across thirty-seven
targets. Formatting and focused six-library/integration-target Clippy pass.
Cargo's test build relinks the child host, so an initial post-build hash guard
fails; both cross-process targets are replayed without rebuilding, verifying
identical final runtime hashes before and after all twenty acceptance cases.
Their replay receipts replace the initial ones in the total. Eight Rust input
hashes, runtime/test hashes and per-target receipts are retained in
`target/class-origins-source-20261015.json` and
`target/class-origins-validation-20261015.json`. A test-only change asserts
major GC in a separate successful script before invoking each retained class
closure; its complete thirty-eight-group replay passes and replaces the
earlier diagnostic receipt. Remaining source producers,
precision/queued/integrated acceptance and final workspace/Test262 gates stay
open; M1q remains at 3/10 final acceptance gates.

The reference/assignment-pattern follow-up reproduces twenty-seven incorrect
locations in thirty public source scenarios. The first fix passes twenty-nine;
scoping optional callees resolves the last one. The expanded thirty-seven-case
selection also exposes a logical `const` store, whose target owner is now
preserved. Assignment-pattern slots follow the unchanged public AST and binding
slots, including shorthand/for-head ranges, templates and compiler clones.
Prepared reference evaluation, value acquisition and the eventual store keep
their own owners without changing instruction order. Ordinary syntax, code-unit
counts and bytes remain unchanged; unknown getter/Proxy failures stay unknown.
A 3,000-target pattern is admitted and a valid 40,000-target pattern is refused
within the shared 8 MiB snapshot budget.

Old matched Core/child binaries reproduce a queued optional callee at 2:51
instead of 3:27 (`/private/tmp/blueice-reference-origins-mcp-red.log`); their
hashes are unchanged before and after reproduction. The final selection passes
1,571 BlueJS cases over forty-one targets, including 1,205 library cases and all
forty-two diagnostic groups. Bridge/Launcher/real pages add 491 passes and one
existing ignored; all four official MCP console cases pass, including queued
callee/nested-pattern positions 3:27 and 6:27. The disjoint total is 2,066 cases
across forty-five targets. Build all test artifacts first, then fix runtime/test
hashes before acceptance; they remain unchanged after execution. Formatting,
focused six-library Clippy and changed integration-target Clippy pass.

The initial parallel browser selection finds an existing latest-snapshot race:
`GetRepresentation` has advanced to generation 4 after frame 3 was received;
the isolated timer test passes. The test helper now matches each returned
counter/Representation to its corresponding observed frame after all three
arrive, retaining distinct unsolicited frames/pixels and the exact final count.
Only that test input changes. Its complete seventeen-case rerun and focused
Clippy pass; the other production/test inputs remain unchanged. Initial failed
and isolated receipts are retained without counting them toward the final total.
Eleven Rust input hashes, old/new runtime hashes, test hashes and receipts are in
`target/reference-origins-source-20261015.json` and
`target/reference-origins-validation-20261015.json`. Remaining source producers,
precision/queued/integrated acceptance and final workspace/Test262 gates stay
open; M1q remains at 3/10 final acceptance gates.

Build-artifact cleanup (2026-10-15, 21:58 Asia/Taipei) removes 8,971 compiled
files and Cargo cache entries, reducing the main `target` footprint by
6.06 GiB. The active MCP executable, test reports, coverage profiles and
worktree contents are retained. All fourteen modified source files and
forty-one top-level JSON reports keep their original SHA-256 hashes; all
130,680 retained files keep their recorded identities, sizes and modification
times. The inventory and verification receipt are
`/private/tmp/blueice-compiled-cleanup-request-20261015-215740-plan.json` and
`/private/tmp/blueice-compiled-cleanup-request-20261015-215740-result.json`.
No compilation or tests ran during cleanup. The emitted-source follow-up below
records the subsequent validation; M1q stays at 3/10 final gates.

The emitted/module/debugger follow-up first reproduces an uncaught emitted
function without a source record
(`/private/tmp/blueice-emitted-origins-red.log`). Compiler artifacts now retain
an authorized original ID/hash certificate; script/module diagnostic spans
select exact generated mapping segments in one bounded, fully validated pass.
Unmapped helper/token gaps stay unavailable. Original byte boundaries convert
the emitter's CR/LF/UTF-16 coordinates to ECMAScript CR/LF/LS/PS coordinates.
Both normal execution and debugger scheduling install the map atomically with
code, and graph failures discard earlier admitted modules and byte charges.
Eight module fixtures preserve ordinary AST/code shape; five source/map
substitution controls and a low-byte-budget graph refusal pass.

The expanded source matrix exposes LS/PS being tokenized as punctuation and
line comments swallowing subsequent declarations. The standalone lexer
regression fails before the fix
(`/private/tmp/blueice-emitted-origins-unicode-lexer-red.log`); character-boundary
trivia/comment scanning fixes both cases, including multibyte comments. CR,
CRLF, LS, PS and astral-column debugger cases pass. The real TLA fixture
explicitly exports its function, as required by the existing checked TypeScript
module policy; asynchronous observation is bounded and retains exact location
assertions. Its complete eighteen-case page replay passes.

The final disjoint selection passes 1,450 BlueJS cases across twenty-one targets
and 276 BlueTS cases across five targets. Bridge/launcher/real pages add 496
passes; all five official MCP console cases pass. The total is 2,227 passes
across thirty target selections, zero final failures and eleven existing
ignored (ten need pinned TypeScript 5.9.3). MCP verifies original `.ts` line 2,
column 2 for emitted classic, module and asynchronous throws while dynamic
`Function` code remains unavailable. Formatting, seven-library Clippy,
changed integration-target Clippy and launcher test Clippy pass. An equivalent
Clippy boolean correction affects only launcher; the final test-only fixture
correction is followed by complete host replay and browser Clippy. Unchanged
BlueJS/BlueTS input hashes justify retaining their earlier passing receipts.
Sixteen Rust inputs, two Cargo inputs, runtime/test hashes and superseded/final
receipts are retained in `target/emitted-origins-source-20261015.json` and
`target/emitted-origins-validation-20261015.json`. Runtime/test hashes remain
identical before and after the final host replay. Direct BlueTS, remaining
precision/emission-profile and integrated lifecycle/resource acceptance, full
workspace/all-target and pinned Test262 gates remain open; M1q stays at 3/10.

The initial direct-source follow-up reproduces a checked function throw without
a source record (`/private/tmp/blueice-direct-origins-statements-red.log`).
The bridge now derives a source-only coordinate table from checked BlueTS
declarations and their final lowered AST. Temporary addresses never survive
that traversal. A bounded private AST digest and original source identity
reject changed artifacts before execution; normal/debugger page installers
charge the common immutable range map with code. Source/AST substitutions and
a code/map budget refusal leave no program or byte charge behind. Existing
standalone debugger tests retain their caller-owned AST mutation behavior.

The expanded bridge cases cover twelve function/control-flow/class/constructor
source scenarios, CR/CRLF/LS/PS text, dependency failures before/after await and
an explicitly unknown dynamic function. The dynamic control stores its function
in a declared variable, respecting the existing direct-call subset. Adding a
default-export identifier snapshot's missing lowering record fixes an existing
body/provenance count mismatch without changing ordinary bytecode. All 173
bridge cases pass, including its complete existing debugger and lifecycle suite.

The final selection passes 1,205 BlueJS library cases and all forty-four
diagnostic groups; bridge/launcher/real pages add 504 passes, and all five
official MCP console cases pass. The disjoint total is 1,758 across six target
selections, zero final failures and one existing ignored. Five new real-page
contexts verify direct classic/module, class getter, `reportError` and awaited
module positions; three deferred debugger contexts retain their `.ts` origins.
MCP verifies direct `reportError`/console at original 1:1 and a Blob-text Promise
callback throw at 1:44. The last MCP failure was an obsolete assertion that
typed console source was unavailable; the complete five-case rerun and focused
MCP Clippy pass after correcting that test alone. Other source/runtime/test
hashes are unchanged. Formatting, seven-library Clippy, changed integration
Clippy and bridge/launcher test Clippy pass. The two successful BlueJS harnesses
from the initial coupled run retain their own passing receipts; its failed
bridge fixture is excluded and replaced by the complete final bridge replay.
Twelve Rust inputs, three Cargo inputs, runtime/test hashes and per-target
receipts are in `target/direct-origins-source-20261015.json` and
`target/direct-origins-validation-20261015.json`.

This is initial direct source binding. Deeper direct expressions/patterns,
namespace/enum/JSX and assignment-field lowering, other remaining precision,
integrated lifecycle/resource acceptance, full workspace/all-target and pinned
Test262 gates remain open; M1q stays at 3/10 final acceptance gates.

The direct-expression follow-up (2026-10-16) reproduces missing operand,
argument, computed-key and nested-reference positions in the public page
runtime (`/private/tmp/blueice-direct-expression-red.log`). An optional collector
in the existing direct parser now records ordered token evidence as expressions
are constructed. Precedence tiers reuse unchanged records; intermediate
binary/member nodes, constructor callees and shorthand values keep their own.
Checked token replay must equal the final expression before binding any child;
type erasure preserves original byte offsets. Temporary allocations have a
conservative 8 MiB charge, and inconsistent/oversized evidence refuses artifact
creation. Opaque template/class-expression/JSX descendants retain unavailable
origins instead of borrowing their parent's location.

Twenty-two public failure scenarios and eighteen parser shapes pass, preserving
ordinary ASTs, instruction bytes and constants. The wide-capture resource control
and an opaque-template unknown control pass. Awaited and non-awaited dependency
expressions use the dependency's original filename and coordinates. All 177
bridge cases and 312 launcher cases pass (one existing launcher test ignored).
Four additional real-page contexts cover operands, arguments, computed object
keys and awaited module expressions; three additional deferred debugger contexts
and three queued MCP direct errors preserve their own `.ts` sources.

The final selection passes 1,205 BlueJS library cases, forty-four diagnostic
groups, 177 bridge cases, 312 launcher cases, nineteen real-page cases and five
official MCP console cases: 1,762 passes across six disjoint targets, zero
failures and one existing ignored. Formatting, seven-library Clippy, changed
integration-target Clippy and bridge/launcher test Clippy pass with warnings
denied. Eleven Rust inputs, three Cargo inputs and runtime/test hashes stay
fixed throughout final acceptance. Per-target receipts and identities are in
`target/direct-expression-source-20261016.json` and
`target/direct-expression-validation-20261016.json`.

Remaining expression/pattern, opaque descendant, namespace/enum/JSX and
assignment-field provenance, integrated lifecycle/resource acceptance, full
workspace/all-target and pinned Test262 gates remain open. M1q remains at 3/10
final acceptance gates.

The embedded-source follow-up (2026-10-16) reproduces ten missing template/
class-expression origins at the public page boundary
(`/private/tmp/blueice-direct-nested-red.log`). The existing template parser now
passes the same optional capture through each original-offset substitution;
children share its pending roots and allocation charge. Five parser shapes
retain ordinary ASTs, instruction bytes and constants, while 50,000 small
substitutions cannot reset the capture budget. After exact replay equality,
defined-field class-expression bodies/defaults/initializers join their checked
original class evidence. Fifteen source scenarios cover constructors, synthetic
prologues, accessors, static blocks, nested classes and template initializers.
Ten additional coordinate contexts retain UTF-16 and CR/CRLF/LS/PS positions;
dependency templates/classes retain their own origins before and after await.
A child capture refusal cannot be cleared by a successful parent traversal.
Mapped templates/classes cannot lend positions to dynamic function bodies;
the separate assignment-field lowering remains explicitly unavailable.

All thirty-five bridge library/integration targets pass: 253 cases, fifteen
existing external TypeScript/Acorn oracle cases ignored. Forty-four BlueJS
diagnostic groups, 312 launcher cases, nineteen real-page cases and five MCP
console cases also pass, for 633 passes across thirty-nine disjoint targets,
zero final failures and sixteen existing ignored. Five new real-page contexts
cover templates, getters, constructors and awaited template/class modules; two
additional deferred debugger contexts pass. MCP retains queued template/class
origins at original 5:2 and 6:2 alongside its earlier reference positions.
Formatting, seven-library Clippy, changed integration Clippy and bridge/launcher
test Clippy pass with warnings denied.

The initial final-page replay had one invalid fixture: an awaited class file
without an import/export was rejected by BlueTS's existing external-module
check before execution. Adding `export` to that class variable alone corrects
the fixture. The complete nineteen-page replay, all five MCP console cases and
browser-target Clippy then pass; the failed receipt is excluded from the final
count and retained separately. Other source/runtime/test hashes are unchanged.
Nine Rust inputs, three Cargo inputs, three runtime executables and thirty-eight
test executables are recorded in `target/direct-nested-source-20261016.json`;
per-target and superseded receipts are retained in
`target/direct-nested-validation-20261016.json` and the corresponding
`direct-nested-initial-*` manifests. Final host replay keeps all identities
fixed. Remaining provenance, integrated lifecycle/resource and full workspace/
all-target/Test262 gates stay open; M1q remains at 3/10 final acceptance gates.

The classic JSX follow-up (2026-10-16) reproduces twelve missing original
expression locations and an incorrectly borrowed generated factory-call
position (`/private/tmp/blueice-direct-jsx-red.log`). The direct lowerer now
carries original embedded token groups and written component-name offsets
through its bounded capture. Generated factory/fragment references, props
objects, literals and invocations retain unavailable positions while carrying
their independently mapped children. Exact replay equality and child order
still govern the final AST join. Fourteen public failure forms, four unavailable
controls, real factory/getter callback bodies, dependency failures before/after
await and queued callback retention/navigation cancellation pass. Six JSX shapes
retain ordinary ASTs, bytes and constants; ten coordinate contexts retain UTF-16
and original line terminators. A 50,000-child parser case verifies the shared
capture budget independently of full-compiler admission.

The coordinate fixtures exposed JSX tag trivia that mishandled CR/LS/PS line
comments, LS/PS whitespace and UTF-8 block comments. A separate failing scanner
regression (`/private/tmp/blueice-jsx-trivia-red.log`) now passes twelve original-
range contexts. All thirty-five bridge targets pass (263 cases, fifteen existing
external-oracle cases ignored). BlueTS library and seven JSX targets pass 294
cases with six existing oracle cases ignored. Forty-four BlueJS diagnostic
groups, 312 launcher cases, nineteen real-page cases and five MCP console cases
also pass: 937 passes across forty-seven disjoint targets, zero failures and
twenty-two existing ignored. Formatting, seven-library Clippy, changed-target
Clippy and complete BlueTS/bridge/launcher test Clippy pass with warnings denied.

Seven Rust inputs, four Cargo inputs, four runtime executables and forty-six
test executables stay fixed throughout final acceptance; receipts are retained
in `target/direct-jsx-source-20261016.json` and
`target/direct-jsx-validation-20261016.json`. JSX execution here uses the explicit
classic-mode bridge API. The existing launcher page profile uses `jsx: None`,
so the Core/MCP selection proves the shared reporting path with its existing
JavaScript/BlueTS inputs. Remaining JSX helper, class/computed-key, pattern/
namespace/enum and assignment-field precision, integrated lifecycle/resource
acceptance and full workspace/all-target/Test262 gates stay open; M1q remains
at 3/10 final acceptance gates.

The class-path follow-up (2026-10-16) reproduces thirty-four missing origins
across 52 declaration/expression/field-mode scenarios
(`/private/tmp/blueice-direct-class-lowering-red.log`). The production lowerer
now optionally records the exact member paths it emits: moved instance/static
initializers, constructor prefix offsets including computed-binding hoisting,
pending key sequences and anonymous-class NamedEvaluation wrappers. Exact class
replay controls the join; temporary paths never survive the final coordinate
inventory. A separate 8 MiB charge bounds path vectors even when many members
have no executable expression slots. Six shape controls preserve ASTs, ordinary
bytes and constants, and a 50,000-member capture refuses excess metadata.
All 52 source scenarios, twenty UTF-16/newline contexts, eight dependency cases,
queued retention/navigation and the hoisted-prefix case pass. Generated field
writes remain explicitly unavailable.

Decorator validation additionally reported the `@` instead of its expression.
`PushDecorator` now owns that expression's diagnostic slot, and the private
heap-owned record keeps copied filename/line/column scalars with each function
and receiver. Native callable/result validation chooses the failing entry and
starts a fresh propagation epoch. Callback/getter throws retain their own
known/unknown origins; caught noise cannot replace a later validation failure.
Ordinary instruction bytes and code-unit layout remain unchanged. The scalars
use existing heap accounting/GC and add no JavaScript object roots or debugger
handles. Ten JavaScript scenarios pass under normal/aggressive GC; two added
dynamic decorator/getter controls remain unavailable.

Final acceptance passes 1,205 BlueJS library cases, 142 decorator cases and all
forty-five diagnostic groups. All thirty-five bridge targets pass 272 cases;
312 launcher cases, nineteen real-page cases and five MCP console cases also
pass. This is 2,000 passes across forty-three disjoint targets, zero failures
and sixteen existing ignored. Three additional page contexts cover field/method
keys and native decorator validation; two additional deferred debugger contexts
pass. MCP verifies a queued direct key at original 7:2 and a second typed
script's decorator validation at 2:2. Formatting, seven-library Clippy, changed
integration Clippy and bridge/launcher test Clippy pass with warnings denied.
Thirteen Rust inputs, three Cargo inputs, three runtime and thirty-eight host
test executable hashes remain fixed. Receipts are retained in
`target/direct-class-lowering-source-20261016.json` and
`target/direct-class-lowering-validation-20261016.json`.

Remaining native/proxy application, generated helper and namespace/enum/pattern
precision, integrated lifecycle/resource acceptance and full workspace/all-target/
Test262 gates stay open; M1q remains at 3/10 final acceptance gates.

The native/Proxy follow-up (2026-10-16) reproduces twelve failing shapes under
normal/aggressive GC (`/private/tmp/blueice-native-decorator-red-20261016.log`).
Native decorator dispatch and accessor-result reads now use the saved
expression's source scope. Successful calls restore the caller's pending
metadata, including tail-call transitions; escaping interpreted errors keep
their own known/unavailable origin. Twenty-three decorator scenarios, three
ordinary callback cases and two dynamic native/Proxy controls pass. Direct
BlueTS adds six script cases across both field modes and eight dependency cases
before/after await. No additional direct expression forms are required.

Final acceptance passes 2,302 cases across sixty-nine disjoint targets, zero
failures and sixteen existing ignored: 1,694 BlueJS, 272 bridge, 312 launcher,
nineteen real-page and five MCP console cases. All forty-seven diagnostic groups
pass. Two new real-page and deferred debugger contexts preserve original 2:2;
MCP verifies two additional typed scripts at that same location. Formatting,
seven-library Clippy and related integration/test Clippy pass with warnings
denied. The source receipt records nine Rust inputs, three Cargo inputs, three
runtime and sixty-nine test executable hashes. A host-only fixture correction
gives consecutive MCP scripts distinct class names before any host build; the
completed BlueJS inputs and executable hashes remain unchanged. Receipts are
`target/native-decorator-source-20261016.json` and
`target/native-decorator-validation-20261016.json`. Remaining dispatch/generated
precision, integrated lifecycle/resource and full workspace/all-target/Test262
gates remain open; M1q stays at 3/10 final acceptance gates.

The namespace/enum follow-up (2026-10-16) reproduces forty-two missing locations
across twenty-one shapes and both field modes
(`/private/tmp/blueice-namespace-origins-red-20261016.log`). Production paths now
identify original namespace declarations, exported initializer values, nested/
dotted wrappers and merge-dependent setup. Complete namespace replay and enum
evaluation history control the join. Generated receivers, object/export writes
and folded values stay unmapped. A shared 8 MiB capture budget includes path
vectors, reference-map node slack, string capacities and displaced copies.

Consumed token cursors distinguish new qualified-name members with overlapping
source bounds from unchanged grammar tiers. Twelve AST/bytecode controls and
eight qualified-name chains pass. A 25,000-declaration path capture and an
oversized reference map with fewer than thirty-two executable slots refuse
excess metadata. Ten UTF-16/newline contexts, sixteen dependency cases before/
after await, major-GC retention and navigation pass. Structural replay and
numeric bit checks admit NaN/infinity/negative-zero enums and atomically refuse
changed NaN payloads, zero signs or source certificates.

Final acceptance passes 667 cases over thirty-nine disjoint targets, zero
failures and sixteen existing ignored: 284 bridge (all thirty-five targets),
forty-seven diagnostic groups, 312 launcher, nineteen real-page and five MCP
console cases. Three additional page contexts and two deferred debugger
contexts keep original 2:2; MCP preserves namespace and enum sources in two
additional typed scripts. Formatting, seven-library Clippy and related test/
integration Clippy pass with warnings denied. Twelve Rust inputs, three Cargo
inputs, three runtime and thirty-nine test executable hashes stay fixed.
Final evidence is `target/namespace-origins-source-20261016.json` and
`target/namespace-origins-validation-20261016.json`; the initial green cohort
before reference-budget refinement is archived under `namespace-origins-initial`.
Remaining dispatch/pattern/generated precision, integrated lifecycle/resource
and full workspace/all-target/Test262 gates stay open; M1q remains at 3/10.

The binding/default-export follow-up (2026-10-16) reproduces forty-four missing
direct BlueTS positions across twenty-two supported shapes and both field
modes (`/private/tmp/blueice-pattern-origins-red-20261016.log`). Checked pattern
replay binds written declaration roots, renamed/elided/rest children and
defaults; exact token prefixes bind variable, parameter and catch names.
Anonymous default functions/classes retain their written parameters/bodies,
while generated export bindings stay unmapped. Eight instruction-owner checks,
four unchanged AST/bytecode controls, four contextual-keyword shorthand controls,
twenty UTF-16/newline contexts, twenty-four dependency/default-export contexts
before/after await, callback/unknown-source controls and actual major-GC/
navigation retention pass. All 221 bridge library cases pass.

Awaited dependencies also exposed duplicate page-owned module errors, skipped
ready siblings and uncached string-limit failures. Ready-parent settlement now
propagates an already recorded catchable failure to static ancestors/waiters,
continues ready siblings, and leaves uncatchable instruction exhaustion as a
host abort. Standalone and bounded-page tests preserve known/unknown callee
origins and report once across repeated checkpoints. The matching deferred
host fixture exposed an emitted-only gap at a copied pattern opening after
trivia. A separately fingerprinted `diagnostic_source_map` option adds exact
copied-token anchors within existing map budgets. The page owner enables it;
ordinary TypeScript-compatible maps and all emitted JavaScript bytes remain
unchanged. Generated helpers/scaffolding remain unmapped. Two frozen artifact
fixtures now select the same map mode and still compare entire maps and code.

Final acceptance passes 2,314 cases across fifty-nine disjoint targets, zero
failures and twenty-nine existing ignored: 1,398 BlueJS across thirteen targets,
287 BlueTS across eight, 292 bridge across all thirty-five, 313 launcher,
nineteen real-page and five official MCP console cases. MCP additionally keeps
the root/default typed binding errors at their original 2:2 and preserves
queued JavaScript positions 3:27 and 6:27. Formatting, seven-library Clippy,
bridge/launcher test Clippy and changed integration-target Clippy pass with
warnings denied. Receipts record twenty-two final Rust inputs, four Cargo
inputs, three runtime and fifty-nine test executable hashes. The completed
BlueJS cohort is retained because its source, Cargo inputs and thirteen test
executable hashes stay unchanged through the later compiler/host refinements;
each refinement and the launcher fixture replay is recorded explicitly.
Evidence is `target/pattern-origins-source-20261016.json` and
`target/pattern-origins-validation-20261016.json`. Remaining dispatch/generated
precision, emitted-profile and integrated lifecycle/resource acceptance, and
full workspace/all-target/Test262 gates stay open; M1q remains at 3/10.

The computed-key follow-up (2026-10-16) reproduces twenty-eight incorrect VM
positions across fourteen object/class/destructuring forms with normal and
aggressive GC (`/private/tmp/blueice-computed-key-red-20261016.log`). A computed
key's `ToPropertyKey` now keeps that expression's diagnostic owner rather
than the enclosing object/class/pattern owner. Native errors after a normally
returning coercion callback use the written key, while callback throws retain
their own known or explicitly unavailable origins. Ten native/callback controls
include caught noise, dynamic getters/methods, fallback conversion and revoked
proxies. The public mapper also verifies unchanged ordinary bytes recursively.

Direct assignment-field mode additionally lost the written key when pending
erased/initialized field keys preceded it in a generated sequence. Both missing
positions were reproduced after the general compiler fix
(`/private/tmp/blueice-computed-key-bridge-wrapper-red-20261016.log`). Complete
class replay proves which terminal expression supplies the converted key, so
the sequence root receives that expression's already certified origin. Hidden
binding and prefix assignment nodes remain unmapped. Sixteen direct contexts,
three callback controls, twenty dependency contexts across both field modes
before/after await and eighteen three-language deferred host contexts pass.

Final acceptance passes 2,070 cases across fifty-two disjoint targets, zero
failures and sixteen existing ignored: 1,437 BlueJS across fourteen targets,
295 bridge across all thirty-five, 314 launcher, nineteen real-page and five
official MCP console cases. Five added page contexts cover direct/emitted
classic and awaited module keys plus JavaScript modules. MCP keeps queued
object/class keys at original inline-9.ts 2:2 and 3:2, alongside all earlier
reference/binding positions. Formatting, seven-library Clippy, complete bridge/
launcher test Clippy and changed integration-target Clippy pass with warnings
denied. Seven Rust and six Cargo inputs remain fixed throughout validation;
three runtime and fifty-two test executable hashes are recorded and remain
unchanged through final bridge/host acceptance. The first deferred fixture's
missing module export was corrected before the complete selection; its initial
source/failed receipt is retained separately. Evidence is
`target/computed-key-origins-source-20261016.json` and
`target/computed-key-origins-validation-20261016.json`. Remaining dispatch/
generated precision, emitted-profile and integrated lifecycle/resource
acceptance, and full workspace/all-target/Test262 gates stay open; M1q remains
at 3/10 final acceptance gates.

The native diagnostic-payload follow-up (2026-10-16) reproduces a missing
4,095-byte long/short filename difference after a public VM throw
(`/private/tmp/blueice-diagnostic-retention-red-20261016.log`). Pending/uncaught
VM fields, owned native frames and paused debugger handlers now join the
document's existing scheduling ledger. Shared immutable maps retain their
installed-code charge. Scoped scalar counter leases keep source copies charged
while Rust owns them during callbacks, catch/finally execution, iterator/
disposer cleanup, nested interpreters and exception reporting; each lease
releases on every exit without rooting JavaScript or retaining source text.

Counting persistent fields alone still lets three near-cap fixtures run an
observable callback before refusing the work: ErrorEvent snapshots, active
catch and iterator cleanup
(`/private/tmp/blueice-diagnostic-retention-cap-red-20261016.log`; the partial
source snapshot is retained separately). The complete scope charges refuse
all three long-name cases before callback side effects. Short-name controls
pass, the uncatchable failure stays latched, and navigation admits fresh work.
Five paused debugger states separately reproduce zero bytes instead of one,
two or three filename copies: classic/module roots, nested classic/module
calls and an intermediate caller
(`/private/tmp/blueice-diagnostic-retention-debugger-red-20261016.log`). Their
charges now match exact ownership and disappear after resume. All fifty-two
diagnostic and thirteen scheduling-limit groups pass.

Final acceptance passes 2,193 cases across fifty-five disjoint targets, zero
failures and sixteen existing ignored: 1,560 BlueJS across seventeen targets,
295 bridge across all thirty-five, 314 launcher, nineteen real-page and five
official MCP console cases. The selection covers module/Promise, disposal/GC
and debugger boundaries plus existing direct/emitted source and unknown-origin
controls through real Core/child/MCP. Formatting, seven-library Clippy,
bridge/launcher test Clippy and changed integration-target Clippy pass with
warnings denied. Fifteen Rust and six Cargo inputs remain fixed throughout
validation; three runtime and fifty-five test executable hashes are recorded
and unchanged through final bridge/host acceptance. Evidence is
`target/diagnostic-retention-origins-source-20261016.json` and
`target/diagnostic-retention-origins-validation-20261016.json`. Remaining source
precision, emitted-profile and integrated lifecycle/resource acceptance, and
full workspace/all-target/Test262 gates stay open; M1q remains at 3/10.

The implicit-protocol follow-up (2026-10-16) reproduces forty-six incorrect
source coordinates across ordinary and aggressive-GC execution. Keeping the
operand owner through heritage, spread, template conversion and delegated
iteration fixes the expanded forty-eight-context matrix. Native-entered
generator/module bodies mask Promise-request overrides; AsyncFromSync preserves
fresh callback origins, including explicit absence, when turning throws into
rejected Promises. Fourteen callback, unknown-source and forwarding controls
pass alongside all fifty-six diagnostic groups.

Public own-key tests separately reproduce 144 accepted primitive trap results.
Requiring an object before reading `length` passes 176 primitive/consumer/GC
contexts, eight boxed-getter side-effect checks and four positive/error controls.
BlueTSC diagnostic maps also expose missing template-interior operand anchors;
unchanged copy boundaries now supply exact positions within the existing
segment cap. All ten public map groups and thirty-six three-language deferred
protocol contexts pass. Emitted JavaScript, ordinary maps and ordinary compiler
instruction bytes retain their established output.

The first complete Rust run records 9,180 passes, one failure and 233 ignored
before stopping at `blueice-net --test probe`. Cargo unifies BlueTS's default
ureq gzip feature with the ranged downloader, removing encoded response headers
before its identity check. The corrected BlueTS manifest follows Core/net's
existing no-transparent-decompression policy; all sixteen probe cases pass with
BlueTS and net selected together. The complete corrected replay passes 9,312
cases, zero failures and 233
existing ignored across 687 result groups, including doc tests and all five
MCP console cases. All 663 test executable and seven runtime hashes remain
fixed through acceptance. The before-fix source snapshot, failed receipts and
the final `cargo test --workspace --no-fail-fast` log are retained. The failed
and corrected commands remain separate validation receipts.
The fresh complete pinned Test262 run finishes in 355.87
seconds including corpus validation and reporting. All 102,926 unique
`(path, mode)` entries match M1p's source hashes, expected results, feature/flag
selection, status and actual kind/phase/reason. Results remain 102,921 pass,
one stale corpus and four exclusions; no new failure or timeout appears.
Runner exit 1 records the unchanged five non-pass modes; the complete baseline
comparison succeeds. Formatting, workspace/all-target build/Clippy and all
runtime/test hashes remain valid at this checkpoint. Evidence is
`target/implicit-protocol-origins-source-20261016.json` and
`target/implicit-protocol-origins-validation-20261016.json`, with the code
checkpoint in `target/implicit-protocol-origins-checkpoint-20261016.json`.
M1q now records 6/10 verified gates. Remaining producer/profile precision and
integrated queued-origin/lifecycle/resource acceptance still prevent section H
or the full M1q milestone from closing.

The requested build cleanup removes 766,331 old compiled files from the main
checkout and sixteen old agent worktrees, releasing 335.69 GiB. Sources,
worktree changes, corpora, reports and the live MCP executable are preserved;
all seventeen checkout states and the protected runtime hashes are verified.

The producer/profile follow-up (2026-10-16) reproduces thirty incorrect host
contexts across duplicate console admission, System await execution, ES5
class/object-spread maps and System class statement boundaries. Public compiler
lookup additionally exposes generated CommonJS class-publication text claiming
original class columns after System relocation. Generated publication now has
explicit gaps; validators continue checking the complete mapping stream.
The final matrix passes 160 source contexts and 28 helper/callback controls in
normal/deferred execution. Eight real Core/child contexts cover all six emitted
profiles and the classic/module console selectors. Thirteen public source-map,
four System-map and two System-runtime groups pass, including four retained
System class shapes and both CommonJS/System publication paths.

The complete affected selection passes 1,843 cases, zero failures and 228
existing ignored across 189 executables: BlueTS 1,030, bridge 298, launcher 510
and the five official MCP console cases. All workspace targets build and pass
Clippy with warnings denied; formatting passes. Eighteen Rust inputs and 13,849
workspace/compiler inputs remain fixed throughout validation. Seven runtime
and all 189 selected test executable hashes are checked before and after the
run. The failed broad replay is retained separately from the corrected replay.
Evidence is `target/source-profile-origins-source-20261016.json` and
`target/source-profile-origins-validation-20261016.json`.

No new Test262 execution is claimed for this compiler/host follow-up. Both
conformance runtime binaries and all eight runner inputs exactly match
`963c400a9`; the 56,979 pinned corpus files and saved summary/results hashes are
verified again. The unchanged 102,926-mode result remains 102,921 pass, one
stale corpus and four exclusions. Reuse evidence is
`target/source-profile-origins-conformance-reuse-20261016.json`. Remaining
operation-owner precision and the integrated queued-origin/lifecycle/resource
matrix keep M1q at 6/10 and section H open.

The loop-operation follow-up (2026-10-16) reproduces 28 incorrect positions
in for-in, for-of and for-await acquisition/step failures. Scoped emission
retains the written right operand through native iterator operations and
awaited steps; binding targets and bodies keep their own owners. Ten callback,
forwarded-rejection and dynamic-code controls preserve independent known or
unavailable locations. Three suspended loops execute later unrelated code,
observe major collection and retain the appropriate native/getter origin after
resumption. Five AST/inventory/recursive-bytecode controls preserve ordinary
instructions and debugger identities. Twenty-one deferred JavaScript/emitted
contexts and three added real Core/child contexts preserve original 2:2.
Five direct BlueTS refusal controls retain its explicitly excluded loop
surface. Emitted top-level-await fixtures retain TypeScript's required original
module marker; no grammar expansion is claimed.

The fixed-input acceptance passes 2,303 cases, zero failures and 16 existing
ignored across 70 disjoint executables: BlueJS 1,660, bridge 299, launcher 339
and five official MCP console cases. The complete 1,205-case BlueJS library and
all 61 diagnostic groups pass. Formatting and full workspace/all-target build/
Clippy pass. Five Rust and 13,849 workspace/compiler inputs, seven runtimes and
all 70 selected test hashes remain fixed. Evidence is
`target/loop-protocol-origins-source-20261016.json` and
`target/loop-protocol-origins-validation-20261016.json`; the before-fix source
manifest and failed public VM log remain separate evidence.

The fresh complete pinned Test262 run takes 357.72s including corpus validation
and reporting. Every `(path, mode)` source hash, expectation, feature/flag,
status and actual kind/phase/reason matches M1p across 102,926 modes. Results
remain 102,921 pass, one stale corpus and four exclusions, with no changed mode,
new failure or timeout. Runner exit 1 represents the five unchanged non-pass
modes; the complete comparison succeeds. Abrupt iterator cleanup and remaining
operation-owner/queued/lifecycle/resource acceptance keep M1q at 6/10 and
section H open.

The dynamic-scope/resource-admission follow-up (2026-10-16) reproduces 30 wrong
with/using admission locations plus eight wrong with-reference locations under
normal/aggressive GC. With-object conversion and disposable-method validation
retain their written operands; iterated resources retain their binding names.
Typeof/delete references and with-var resolution/store retain the written
reference or pattern. The existing pattern-owner scope is shared without
changing evaluation order, anonymous naming, executable instructions or
debugger identities; eleven public AST/inventory/recursive-bytecode controls pass.

Eleven callback/body/cleanup controls retain independent known/unavailable
sources, including synchronous/asynchronous iterator close after a disposer
throws. Four suspended resource contexts execute unrelated later code, observe
major collection and retain native/getter origins after resumption. Forty
authenticated File DOM classic-profile contexts, five added real Core/child
contexts and four direct BlueTS refusal controls pass. System's with fixture
uses its admitted non-external classic script; strict module wrappers continue
rejecting with. The direct/typed resource surface is not expanded.

The fixed-input selection passes 2,358 cases, zero failures and 16 existing
ignored across 77 disjoint executables: BlueJS 1,713, bridge 300, launcher 340
and five official MCP console cases. All 1,205 BlueJS library and 67 diagnostic
cases pass. Formatting and full workspace/all-target build/Clippy pass. Six
Rust and 13,849 workspace/compiler inputs, seven runtimes and all 77 selected
test hashes remain fixed. Admission and with-reference failure receipts are
retained separately. Final evidence is
`target/resource-admission-origins-source-20261016.json` and
`target/resource-admission-origins-validation-20261016.json`.

Fresh complete pinned Test262 takes 357.00s including corpus validation and
reporting. Every source hash, expected result, feature/flag, status and actual
kind/phase/reason matches M1p across 102,926 modes: 102,921 pass, one stale
corpus, four exclusions, zero changed modes and no new failure or timeout.
Runner exit 1 records the five unchanged non-pass modes; the comparison
succeeds. Remaining operation-owner and integrated queued/lifecycle/resource
acceptance keep M1q at 6/10 and section H open.

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
