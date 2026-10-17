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

**Validation constraint — reaffirmed 2026-10-18.** Continue architecture work
with offline static checks and fixed, finite unit/functional cases. Use tiny
in-memory fixtures and numerical admission controls rather than represented
large allocations. Do not resume resource-exhaustion/infinite-execution probes,
network/service/listener/Core/MCP replay, privileged validation or process
killing. Broader acceptance items below stay open under this constraint.
After a cybersecurity warning or automatic approval rejection, inspect existing
local evidence first. Do not rephrase, reroute or repeat the affected operation
to bypass review. Record affected checks and publication as pending while the
operation is being identified.

M1s began with this public ownership failure:
discarded ShadowRealm instances become unreachable (`WeakRef.deref()` returns
undefined), but their VM tables retain global Promise results and their native
charge. A timer adding one such 400,000-character result per callback grows
from 800,912 to 8,005,232 native bytes and fails on callback 11. The accepted
realm-reclamation milestone below closes this failure; its original evidence
remains separate from the accepted replay.
Evidence: `target/orphan-realm-probe-source-20261016.json` and
`/private/tmp/blueice-orphan-realm-probe-20261016.log`.

**In progress — M1z active execution control storage (2026-10-16).**
The eval environment producer follow-up (2026-10-18) has an implementation
checkpoint. Borrowed
selection scratch, snapshot vectors, copied names and dynamic capture tables
now acquire native admission before construction. Both original refusal cases
and all fourteen expanded cases pass. Final validation passes **176 distinct
selected tests** across twelve executable targets and thirty-eight fixed
selections. Formatting, diff checks and offline workspace/all-target Clippy pass,
with source/artifact and local commit audits. Broader acceptance remains open.

The direct/indirect eval storage follow-up (2026-10-18) is active. Apply retained
input/source/code ownership to eval, including its compiler environment snapshots
and capture handoff. Preserve strictness, lexical/global variable environments,
super/private-name access and non-string fast paths. Four corrected old-code
cases fail; twenty expanded cases pass. Final related controls pass **162 distinct
selected tests** in thirty-seven fixed selections across twelve executable
targets. Formatting, diff checks and offline workspace/all-target Clippy pass,
with source/artifact and local commit audits. Broader acceptance remains open.

The dynamic Function storage follow-up (2026-10-18) is active. Extend native
input/source/code ownership to all four Function constructor kinds, including
coercion-time string buffers, independent grammar checks and code retained by
timer/Promise callbacks. Preserve exact synthesized source and global scope;
three original failures and 22 passing new cases are recorded. Final related
controls pass **132 distinct selected tests** across eight executable targets and
twenty-six fixed selections. Formatting, diff checks and offline workspace/
all-target Clippy pass, with source/artifact and local commit audits. Broader
acceptance remains open under the existing cybersecurity constraint.

The shared child-code storage follow-up (2026-10-18) is active. Dynamic modules'
source leases and child-code headers/buffers now have independent owners.
Metadata edits admit coexisting copies while preserving nested sharing and
host-compiled reuse. Three old-code cases fail; the initial expanded fifteen and
nineteen source-lifetime controls pass. The final seventeen new cases and related
controls pass **103 distinct selected tests**, across five executable targets and
twenty-one fixed selections. Formatting, diff checks and offline workspace/
all-target Clippy pass, with source/artifact and local commit audits. Other
program storage and integrated acceptance stay open.

The compiled dynamic-module source follow-up (2026-10-18) transfers source
leases from parsed ASTs into independently shared runtime code. Child code and
queued timer/Promise callbacks retain their source after root-code/VM teardown;
distinct template-fragment sources release independently. Nineteen fixed tiny
cases pass. Final related controls pass **80 distinct selected tests** in
fourteen selections across five executable targets. A callback type alias
addresses the archived pre-lint finding; final formatting and offline workspace/
all-target Clippy pass. Source/artifact and local commit audits record the
checkpoint. Broader compiled-program storage and integrated acceptance remain
open. The cybersecurity constraint above continues to apply, and the rejected
push is not retried.

The dynamic module compilation-input follow-up (2026-10-18) now borrows module
bodies and owns returned AST buffers through root-code preparation. Parser input
minimums and source-deduplication scratch are admitted, including separate
template-fragment source allocations. The corrected focused run passes **22/22**;
final related controls pass **60 distinct selected tests**, across five executable
targets and twelve selections. Formatting, diff checks and offline workspace/
all-target Clippy pass, with source/artifact and commit audits. Complete parser/
compiler producer preallocation and shared compiled-program storage remain open.

The source-phase import admission follow-up (2026-10-17) now resolves source
requests through admitted native name storage and returns latched settlement/
thenable-job failure through its fallible entry. Three old-code cases reproduce
early allocation and a successful return after document failure. The first
expanded run passes **14/14**. Final focused validation passes **105 distinct
selected tests**, across four executable targets and nine selections, including
the preserved test-reference resolver. Formatting, diff checks and offline
workspace/all-target Clippy pass, with source/artifact and commit audits. The
earlier Clippy dead-code failure and source state are archived. No broad replay
is claimed; compilation, producer storage and integrated acceptance remain open.

The ImportCall argument-storage follow-up (2026-10-17) now admits incoming native
payloads and temporary-root capacity before Promise creation, moves input values
without operand copies, and retains converted UTF-8 specifiers and attribute-key
snapshots across option getters and Proxy traps. Three old-code cases reproduce
two pre-refusal allocations, changed stack capacity and five missing callback
bytes. The first expanded run passes **14/14**; focused related controls pass
**137 distinct tests**, across five executable targets and 14 selections.
Formatting, diff checks and offline workspace/all-target Clippy pass, with
source/artifact and commit audits. No broad control replay is claimed. Dynamic
compilation, generic coercion/own-key producer preallocation and integrated
acceptance remain open; M1u remains the latest accepted milestone.

The module completion/rejection notification-storage follow-up (2026-10-17) now
owns independently admitted notification work. Four old-code cases reproduce one
queue allocation at zero native headroom and dependency-edge removal before
refusal. Notification queues now
admit complete parent batches; detached reverse vectors transfer their existing
VM charge to independent name/iterator owners. The first focused run passes
**26/26**. The initial fixed controls pass 1892 cases and fail one legacy
observation of the obsolete ambient counter. The corrected observation passes
**16/16**, preserving its 153-byte minimum. Reusing 170 unaffected passing
selections with nine unchanged Rust-input hashes yields evidence for **1893
distinct selected tests**, across 50 executable targets and 171 selections.
Formatting, diff checks and offline workspace/all-target Clippy pass. Both
failed observations and their source/artifact provenance are preserved; the
broad runner is not replayed after the cybersecurity warning. Dynamic source
compilation, other task/native/heap buffers and wider integrated acceptance
remain open. M1u remains the latest accepted milestone.

The iterative module-dependency evaluation follow-up (2026-10-17) now owns
explicit dependency evaluation frames.
Three fixed public graphs reproduce 3/4/3 simultaneously active dependency
evaluation calls. An admitted explicit DFS controller now retains request
iterators, waiting targets and pending dependency names independently of the VM
while JavaScript runs. The first focused run passes **24/24**. Twenty-four new
groups and all 1843 prior selected controls pass: **1867 total**, across 50
executables and 170 fixed selections. Formatting, diff checks and offline
workspace/all-target Clippy pass; source/artifact and commit audits record the
checkpoint. Legitimate JavaScript-driven reentry retains the existing execution-
context mechanism; dynamic source compilation, other native/heap/task buffers
and integrated acceptance remain open. M1u remains the latest accepted milestone.

The iterative namespace-construction follow-up (2026-10-17) now owns explicit
construction frames and removes recursive namespace calls.
Three fixed public module graphs reproduce 3/4/2 simultaneous namespace host
calls rather than one. Construction now uses admitted movable frames and
independently owned export payloads, preserving DFS order, placeholders and
transactional rollback. The first focused run passes **21/21**. Twenty-one new
groups and all 1822 prior selected controls pass: **1843 total**, across 50
executables and 169 fixed selections. Formatting, diff checks and offline
workspace/all-target Clippy pass; source/artifact and commit audits record the
checkpoint. Module-body evaluation recursion, dynamic source compilation, other
native/heap buffers and integrated acceptance remain open. M1u remains the latest
accepted milestone.

The synthetic Bytecode preparation follow-up (2026-10-17) now owns metadata
through registry adoption.
Four old-code cases reproduce six metadata allocations and seven allocations
for a full record with only value-owner headroom, plus 562 unreported retained
metadata bytes both before and after VM destruction. Metadata now owns complete
binding/scope/export storage before value conversion and transfers its native
owner into the registry. The completed focused run passes **26/26**. Twenty-six
new groups and all 1796 prior selected controls pass: **1822 total**, across 50
executables and 168 fixed selections. Formatting, diff checks and offline
workspace/all-target Clippy pass; source/artifact and commit audits record the
checkpoint. Dynamic source compilation, other native/heap buffers, recursion and
integrated acceptance remain open. M1u remains the latest accepted milestone.

The static synthetic-request preparation follow-up (2026-10-17) now owns the
complete request batch through registration.
Four old-code cases reproduce 1/6 allocations before zero-headroom admission,
154 unreported retained target bytes and the same missing ownership after VM
destruction. Canonical module-name scratch, complete target backing and resolved
keys now retain native owners through record registration. The completed focused
run passes **22/22**. Twenty-two new groups, one additional finite public rollback
case and all 1773 prior selected controls pass: **1796 total**, across 50
executables and 167 fixed selections. Formatting, diff checks and offline
workspace/all-target Clippy pass; source/artifact and commit audits record the
checkpoint. Synthetic Bytecode metadata construction, other request/cache/native
buffers, recursion and integrated acceptance remain open. M1u remains the latest
accepted milestone.

The source/import-meta identity-cache follow-up (2026-10-17) now owns retained
cache storage and prepares complete publication.
Four old-code cases reproduce one allocation on an import-meta hit, four on a
zero-headroom cold call, and missing retained table/key charges. Four identity/
root tables now own retained backing and key capacities; publication prepares
complete table/name storage before managed allocation. Import-meta lookup
borrows its name, and source allocation uses the heap's protected prototype
argument. The first expanded run passes **23/23**. Twenty-three new groups,
two additional finite public identity cases and all 1748 prior selected controls
pass: **1773 total**, across 50 executables and 165 fixed selections. Formatting,
diff checks and offline workspace/all-target Clippy pass; source/artifact and
commit audits record the checkpoint. Synthetic request producers, other caches/
buffers, recursion and integrated acceptance remain open. M1u remains the latest
accepted milestone.

The module-binding preparation follow-up (2026-10-17) now owns import and
namespace staging. Four
pre-admission observations reproduce one allocation each for import slots,
aliases and namespace export backing at zero native headroom, and an uncharged
retained namespace payload. These original producer expressions were extracted
without adding admission before the red run. Import-slot/alias batches and
namespace UTF-16/output storage now have native owners; namespace preparation
precedes placeholder publication. The first completed expanded run passes
**25/25**. Twenty-five new groups and all 1723 prior selected controls pass:
**1748 total**, across 49 executables and 162 fixed selections. Formatting, diff
checks and offline workspace/all-target Clippy pass; source/artifact and commit
audits record the checkpoint. Other request/cache/heap buffers, namespace/module-
body recursion and integrated acceptance remain open. M1u remains the latest
accepted milestone.

The namespace managed-payload follow-up (2026-10-17) now retains payload
capacity and protects incoming cells. Five tiny
old-code cases reproduce 42 rather than 384 retained bytes, 42 residual bytes
after namespace collection, incorrect empty-backing replacement, and a panic
when completion GC reclaims incoming unrooted binding cells. Heap adoption now
charges namespace vector/name capacity, updates per-object and aggregate bytes,
and protects incoming cells. The first expanded run passes **13/13**. Thirteen
new groups, three additional finite heap cases and all 1707 prior selected
controls pass: **1723 total**, across 49 executables and 161 fixed selections.
Formatting, diff checks and offline workspace/all-target Clippy pass; source/
artifact and commit audits record the checkpoint. Native binding-list/import-
alias staging and wider acceptance remain open. M1u is the latest accepted
milestone.

The export-resolution storage follow-up (2026-10-17) now owns iterative query
and result storage. Five old-code cases reproduce four local-query allocations,
eight indirect-query
allocations before zero-headroom refusal, and uncharged namespace/deferred/source
result names. `ResolveExport` now borrows canonical keys or retains admitted
target ownership, with explicit traversal frames and query-wide cycle markers.
The first expanded run passes **25/25**. Twenty-five new groups, four additional
finite public cases and all 1678 prior controls pass: **1707 total**, across 49
executables and 157 fixed selections. Formatting, diff checks and offline
workspace/all-target Clippy pass; source/artifact and commit audits record the
checkpoint. Binding-list/import-alias staging, namespace/module-body recursion,
other synthetic/source request producers and wider acceptance remain open. M1u
remains the latest accepted milestone.

The export-name traversal follow-up (2026-10-17) now admits iterative name
collection. Three tiny namespace cases reproduce 2/4/9 extra allocations after
cache preparation uses
all remaining native headroom. `GetExportedNames` now uses admitted iterative
walks and borrows canonical module/export names instead of copying Strings into
recursive hash/tree sets. All three old-code failures now pass. Nineteen new
groups and all 1659 prior selected controls pass: **1678 total**, across 47
executables and 152 fixed selections. Formatting, diff checks and offline
workspace/all-target Clippy pass; source/artifact and commit audits bind the
evidence to this checkpoint. `ResolveExport`, binding-list staging, namespace/
module-body recursion and wider acceptance remain open. M1u remains the latest
accepted milestone.

The namespace-cache ownership follow-up (2026-10-17) now retains allocation
ledgers for five persistent cache/root/reverse-map layers and pre-admits complete
publication storage. Three old-code cases failed and now pass.
Publication marks replace allocation during rollback, including rollback across
the full linking transaction. Twenty-two new groups, four additional legacy
boundaries and all 1633 prior selected controls pass: **1659 total**, across 47
executables and 151 fixed selections. Formatting, diff checks and offline
workspace/all-target Clippy pass. Source/artifact, staged and committed audits
tie the checkpoint to the twelve unchanged Rust inputs and final documentation.
The cybersecurity constraint remains in force; rejected push stays pending.
Export traversal/staging and wider
integrated acceptance remain open; M1u stays the latest accepted milestone.

The module-walk storage follow-up (2026-10-17) now admits complete
visited tables, explicit DFS/loading stacks and ordered SCC candidate storage.
It borrows canonical registry keys and reuses scratch within one operation,
preserving source/error order, cycle semantics and resource propagation. Three
fixed old-code runtime observations failed and now pass without allocation.
Twenty-three new groups, four additional legacy boundaries and all 1606 prior
selected controls pass: **1633 total**, across 47 executables and 146 fixed
selections. Formatting and offline workspace/all-target Clippy pass. Linking/
export producers, module-evaluation recursion, allocator/error-message overhead,
shared programs and integrated acceptance remain open; M1u stays the latest
accepted milestone.

The module-name producer follow-up (2026-10-17) now admits request
canonicalization buffers, evaluation/frontier vectors and pending-name copies
before allocation, and retains independent ownership across dependency execution
and result handoff. Fixed eager/deferred runtime boundary observations failed
with 6/9 allocation attempts and now pass without allocation. Nineteen new groups
and all 1587 prior selected controls pass: **1606 total**, across 47 executables
and 141 fixed selections. Formatting and offline workspace/all-target Clippy
pass. DFS/visited/SCC scratch, readiness/loading/linking producers, full module
caches, shared programs, other native/host buffers and integrated acceptance
remain open; M1u stays the latest accepted milestone.

The module-dependency batch-storage follow-up (2026-10-17) now prepares
deduplication/input/name storage and complete forward/reverse/set/vector
candidates before either graph direction changes. Three tiny old-code cases
failed and now pass. Twenty new groups and all 1567 prior selected controls
pass: **1587 total**, across 47 executables and 140 fixed selections. Retained
allocation ownership across deletion and admitted optional reclamation are
implemented. Formatting and offline workspace/all-target Clippy pass. Wider
pending-name/frontier/DFS/walk producers, full module caches, shared programs,
other native/host buffers and integrated acceptance stay open; M1u stays the
latest accepted milestone.

The template-control preparation follow-up (2026-10-17) now independently leases
the full table and operand candidates before active stack backing changes. Three
finite old-code cases failed and now pass. Sixteen new groups and all 1551 prior
selected controls pass: **1567 total**, across 47 executables and 139 fixed
selections. Formatting and offline workspace/all-target Clippy pass. Earlier
template results retain their historical scope. Wider string/input/origin
producers, shared site/program ownership, other native buffers and integrated
acceptance remain open; M1u stays the latest accepted milestone.

The disposal-map retained-storage follow-up (2026-10-17) now retains both weak
brand tables' native backing and prepares complete constructor/move replacement
storage before managed instance allocation. Four tiny old-code cases failed and
now pass. Twenty-two new groups and all 1529 prior selected controls pass:
**1551 total**, across 47 executables and 138 fixed selections. Formatting and
offline workspace/all-target Clippy pass. Other input/origin producers, retained
maps, heap/host buffers, shared programs and integrated acceptance remain open;
M1u stays the latest accepted milestone.

The continuation-pruning scratch follow-up (2026-10-17) now marks stored frames
in place instead of allocating two unadmitted serial-ID HashSets after heap GC.
Three tiny old-code observations failed with 1/1/2 allocation attempts and now
pass without scratch. Sixteen new groups and all 1513 prior selected controls
pass: **1529 total**, across 47 executables and 137 fixed selections. Formatting
and offline workspace/all-target Clippy pass. Other input/origin producers,
retained maps, heap/host buffers, shared programs and integrated acceptance
remain open; M1u stays the latest accepted milestone.

The module-referrer retained-storage follow-up (2026-10-17) now retains weak
hash backing and independently leases closure/call preparations after VM
destruction. Finite cases reproduced lost detached charges, missing reclamation,
transient double charging and stale candidates overwriting larger tables.
Twenty-three new groups and all 1490 prior selected controls pass: **1513 total**,
across 47 executables and 136 fixed selections. Formatting and offline workspace/
all-target Clippy pass. Other input/origin producers, retained maps, heap/host
buffers, shared programs and integrated acceptance remain open; M1u stays the
latest accepted milestone.

The finally-thunk provenance storage follow-up (2026-10-17) now retains hash
allocation ownership across deletion and stages complete table/source storage
before managed thunk-state creation. Three finite cases reproduced guarded GC
shrinkage, premature entry publication and an unadmitted thrower source copy.
The first implementation run fixed those behaviors; a fixture's broad heap
snapshot included legitimate earlier callback/PromiseResolve allocations and is
now checked separately at the native table boundary. Twenty-four new groups and
all 1466 prior selected controls pass: **1490 total**, across 47 executables and
135 fixed selections. Formatting and offline workspace/all-target Clippy pass.
Other input/origin
producers, other retained maps, heap/host buffers, shared programs and integrated
acceptance remain open; M1u stays the latest accepted milestone.

The continuation await preparation follow-up (2026-10-17) now prepares module,
async-function and generator await work before changing handled state. Three
corrected finite refusal cases failed before implementation and now pass.
Generator status/source clones use the same pre-admitted input path as frame
awaits. Frame/queue commitment precedes handled updates and checked notification
cleanup; native/heap/fuel refusals propagate without conversion to page throws.
Twenty-four new groups and all 1442 prior selected controls pass: **1466 total**,
across 47 executables and 134 fixed selections. Formatting and offline
workspace/all-target Clippy pass.
Other input/origin producers, provenance/other maps, heap/host buffers, shared
programs and integrated acceptance remain open; M1u stays the latest accepted
milestone.

The Promise Then registration follow-up (2026-10-17) now admits pending and
settled work before changing handled state or rejection-notification ownership.
Three tiny refusal reproductions failed before the fix and now pass. Selected
incoming fields and retained value/source copies have independent leases before
copying; registration reuses the existing complete queue preparation boundary.
Twenty-two new groups and all 1420 prior selected controls pass: **1442 total**,
across 47 executables and 133 fixed selections. Formatting and offline
workspace/all-target Clippy pass.
Other await/generator handled-state/input/origin producers, provenance/other
maps, heap/host buffers, shared programs and integrated acceptance remain open;
M1u stays the latest accepted milestone.

The Promise settlement fan-out follow-up (2026-10-17) prepares all reaction-job
copies and complete queue capacity before changing a pending record. Three tiny
refusal reproductions failed before the fix and now pass. Whole-batch refusal
retains the pending status and every reaction while latching the document abort;
native resolvers do not panic. Twenty-two new groups and all 1398 prior selected
controls pass: **1420 total**, across 47 executables and 132 fixed selections.
Formatting and offline workspace/all-target Clippy pass.
Already-settled reaction/origin producers, provenance/other maps, heap/host
buffers, shared programs and integrated acceptance remain open; M1u stays the
latest accepted milestone.

The import-delivery ownership follow-up (2026-10-17) prepares complete native
snapshots and target/namespace/reason roots before consuming waiters or advancing
dependencies. Three finite refusal cases reproduced the old behavior. All
nineteen focused groups now pass, including collection during a namespace getter
and preservation of unattempted targets after a committed error. These groups
and all 1379 prior selected controls pass: **1398 total**, across 47 executables
and 131 fixed selections. Formatting and offline workspace/all-target Clippy
pass. Common Promise settlement/reaction
scratch, provenance/other maps, heap/host buffers, shared programs and integrated
acceptance remain open; M1u stays the latest accepted milestone.

The import-registration storage follow-up (2026-10-17) now prepares complete
eager/deferred native storage before publication. Two finite cases reproduced
net-only vector/table admission; all twenty new finite groups pass. Import maps
and dependency sets retain their allocation cost and use admitted optional
compaction. Twenty groups and all 1359 prior selected controls pass: **1379 total**,
across 47 executables and 130 fixed selections. Formatting and offline workspace/
all-target Clippy pass. That checkpoint left import-delivery rooting/scratch for
the follow-up above; provenance/other maps, heap/host buffers, shared programs
and integrated acceptance remain open.

The host-wait/continuation map follow-up (2026-10-17) now extends allocation
ledgers to external waits and async/module continuations. Last resumption and
orphan collection release empty backing; a shared guard protects all four staged
scheduling tables during managed allocation. Twenty finite groups and all 1339
prior selected controls pass: **1359 total**, across 47 executables and 129 fixed
selections. Formatting and offline workspace/all-target Clippy pass. Other retained
maps, event/dispatch scratch, heap/host buffers, shared programs and integrated
acceptance remain open; M1u stays the latest accepted milestone.

The Promise retained-allocation follow-up (2026-10-17) now retains allocation
cost across deletion and prepares optional GC compaction under a native lease.
Its initial finite reproduction failed before the fix. Eighteen finite groups
and all 1321 prior selected controls pass: **1339 total**, across 47 executables
and 128 fixed selections. Formatting and offline workspace/all-target Clippy
pass. Other retained maps, event/dispatch scratch,
heap/host buffers, shared programs and integrated acceptance remain open.

The rejection-batch entry follow-up (2026-10-17) prepares native notification
snapshots before removing their rooted task. The two original refusal cases
failed before the fix; seventeen focused finite groups, four additional exact
public rejection cases and all 1300 prior selected controls pass: **1321 total**.
Formatting and offline workspace/all-target Clippy pass.
Managed event allocation/dispatch scratch, retained-map ownership,
other heap/host buffers, shared programs and integrated acceptance remain open.

The browser-task entry checkpoint (2026-10-17) now peeks queued work and prepares
starting fuel/drag argument capacity before removal. Twenty-eight finite
groups pass: active refusal preserves queued state/context, stale cleanup needs
no host instruction, and future timeouts keep their deadline/serial. Public turn
reproductions also pass. Four exact finite public cases and all 1268 prior
selected controls pass: **1300 total**, with formatting and offline workspace/
all-target Clippy. Per-event/batch staging, retained-map ownership, other heap/
host buffers, shared programs and integrated acceptance remain open; M1u stays
the latest accepted milestone.

The animation-frame entry checkpoint (2026-10-17) now admits render handle
snapshots and timestamp argument storage to the document budget before
callback removal. Starting-fuel/native refusal preserves the registration,
roots, ambient fuel and timestamp; committed dispatch/reporting errors release
their roots. Twenty-five finite groups pass, including exact allowances,
argument handoff, checkpoints and GC. Five exact finite public frame cases and
all 1238 prior selected controls also pass: **1268 total**, with formatting and
offline workspace/all-target Clippy. Other browser-task/event entry scratch,
retained-map ownership, heap/host buffers, shared programs and integrated
acceptance remain open; M1u stays the latest accepted milestone.

The browser-task preparation checkpoint (2026-10-17) now admits complete queue
replacements and incoming payload, stages roots and FileReader updates, and
publishes task flags/state only after preparation succeeds. Twenty-seven
finite groups pass, covering managed/root/native/serial refusal without consuming
an enqueue serial or live queue capacity, plus error order, nested state copies,
GC and cancellation. Four exact finite public read/timeout cases and all 1207
prior selected controls also pass: **1238 total**, with formatting and offline
workspace/all-target Clippy. Callback-entry scratch, retained-map ownership,
other heap/host buffers, shared programs and integrated acceptance remain open;
M1u stays the latest accepted milestone.

The rooted scheduling registration checkpoint (2026-10-17) now admits
animation-frame, cleanup-task and pending-rejection producers'
complete coexisting replacements and root storage before publishing capacity,
handles, roots or enqueue serials. Twenty-four finite groups pass, covering
preparation refusal, shared-owner refunds and existing GC/cancellation behavior.
Seven exact finite public frame/rejection cases and all 1176 prior controls also
pass: **1207 total**, with formatting and offline workspace/all-target Clippy.
Complete browser-task queue/root/state preparation, callback-entry scratch,
retained-map ownership, other buffers, shared programs and integrated acceptance
remain open; M1u stays the latest accepted milestone.

The host-wait registration checkpoint (2026-10-17) now admits complete detached
wait/Promise table candidates and leased argument buffers before capability
publication. Root IDs are preflighted before managed allocation; root storage
precedes token serial acquisition as the final fallible preparation step.
Twenty-four finite in-memory cases pass, including GC, refusal/refunds,
callback lifetime, rejection order, completion and cancellation. All 1152 prior
selected controls also pass: **1176 total**, with formatting and offline
workspace/all-target Clippy. Other control tables/buffers, retained-map ownership,
producer clones, shared programs and integrated acceptance remain open; M1u stays
the latest accepted milestone.

The Promise record checkpoint (2026-10-17) now keeps complete native table
candidates detached through managed/trace admission and publishes them only
after object creation. GC pruning preserves reusable slots; failure refunds
candidates and restores cache reclamation policy. Twenty-three new finite cases
and all 1129 prior controls pass: **1152 total**, with formatting and offline
workspace/all-target Clippy. Host-wait/other table preallocation, retained-map
ownership, producer clones, other buffers, shared programs and integrated gates
remain open; M1u stays the latest accepted milestone.

The Promise queue checkpoint (2026-10-17) now admits complete job/reaction
replacements and incoming payload before publication. Async/module await stages
queue and continuation-map storage together, preserves capacities/serials on
refusal and reconciles rejected-frame refunds for sibling realms. Twenty-eight
new finite cases and all 1101 prior controls pass: **1129 total**, with formatting
and offline workspace/all-target Clippy. Other Promise-record/wait-table and
retained hash-allocation ownership, producer clones, other buffers, shared
programs and wider integrated gates remain open; M1u stays the latest accepted
milestone.

The timer registration checkpoint (2026-10-17) now admits complete coexisting
table replacement, argument/root vectors and root-table batch storage before
publishing timer/root IDs or task serials. Refusal preserves old registrations,
capacities and counters; spare slots admit only newly owned inputs. Twenty-four
new finite cases and all 1077 prior controls pass: **1101 total**, with formatting
and offline workspace/all-target Clippy. Other managed/native scheduling and
heap/host buffers, shared programs and wider integrated gates remain open; M1u
stays the latest accepted milestone.

The timer callback-entry checkpoint (2026-10-17) now validates starting fuel
before one-shot removal or interval-copy admission, retains interval inputs
through self-cancellation/GC and transfers argument capacity into dispatch once.
Twenty-one new finite entry cases, six finite public timer cases and all 1050
prior controls pass: **1077 total**, with formatting and offline workspace/all-
target Clippy. Timer registration staging, other managed/native heap/host buffers,
broader report/event scratch, shared programs and wider integrated gates remain
open; M1u stays the latest accepted milestone.

The finalization cell backing checkpoint (2026-10-17) now charges actual slots
in the existing managed heap ledger, admits coexisting replacement storage
before mutation and refunds only released allocation/payload. Twenty-four new
finite offline cases and all 1026 prior controls pass: **1050 total**, with
formatting and offline workspace/all-target Clippy. Document sharing and cleanup
ownership remain intact. Other managed/native heap/host buffers, broader report/
event scratch, shared programs and wider integrated gates remain open; M1u stays
the latest accepted milestone.

The finalization callback-entry checkpoint (2026-10-17) now prepares roots,
payload, argument storage and starting fuel before consuming its queued task or
cell. Refusal preserves cleanup ownership and ambient context; committed calls
hand ownership into dispatch once. Twenty-two new finite cases, four finite
public cleanup cases and all 1000 prior controls pass: **1026 total**, with
formatting and offline workspace/all-target Clippy. Cell backing, other heap/
host buffers, broader report/event scratch, shared programs and wider gates
remain open; M1u stays the latest accepted milestone.

The finalization storage checkpoint (2026-10-17) now admits ready-index growth
before weak clearing/sweeping and page registry snapshots before task publication.
Standalone cleanup moves holdings directly into traced jobs without bulk scratch
copies. Twenty-five new finite cases, three existing index cases and all 972
prior controls pass: **1000 total**, with formatting and offline workspace/all-
target Clippy. Cell backing/callback-entry preparation, other heap buffers,
broader report/event scratch, shared programs and wider gates remain open; M1u
stays the latest accepted milestone.

The host-turn snapshot checkpoint (2026-10-17) now admits realm discovery and
due callback/abort/per-realm snapshot storage. Native leases survive callbacks
and Rust-held lifetimes while weak handles allow discarded realms to disappear.
Twenty-two new finite cases and all 950 prior controls pass: **972 total**,
with formatting and offline workspace/all-target Clippy. Finalization cleanup
index/job snapshots, other heap buffers, broader report/event scratch, shared
programs and wider gates remain open; M1u stays the latest accepted milestone.

The foreign callable pin checkpoint (2026-10-17) now admits shared native
headers and registry slots before root publication. Dropping handles performs
no allocation; safepoints reclaim released registrations and native ownership,
including terminal cleanup and Rust-held lifetimes. Twenty new finite cases
and all 930 prior controls pass: **950 total**, with formatting and offline
workspace/all-target Clippy. Other heap buffers, broader report/event scratch,
shared programs and wider gates remain open; M1u stays the latest accepted
milestone.

The persistent root-table checkpoint (2026-10-17) now admits replacement hash
capacity before root-ID publication and keeps bootstrap/Rust-held heaps charged.
Root and collector caches bind together; refusal preserves registrations and
terminal cleanup refunds released storage. Twenty-two new finite cases and all
908 prior controls pass: **930 total**, with formatting and offline workspace/
all-target Clippy. Other heap tables, deferred pin queues, broader report/event
scratch, shared programs and wider gates remain open. M1u stays the latest
accepted milestone.

The pending module error reporting checkpoint (2026-10-17) now admits snapshot
slots/names and roots every object value before consuming flags. Batches survive
graph release and callback GC; interrupted delivery restores only unattempted
records with matching names/error owners. Twenty-one new cases, twelve public
error-reporting controls and all 875 prior selected cases pass: **908 total**,
with formatting and offline workspace/all-target Clippy. Broader report/event
scratch, shared programs, managed-container buffers and wider gates remain open;
M1u stays the latest accepted milestone.

The cached module value checkpoint (2026-10-17) now admits completion/error
primitive copies before publication and keeps native ownership across installed,
parked and Rust-held records. Async parents share one immutable error value and
owned origin; weak discovery retains only header ownership after payload release.
Twenty-one new finite cases, one existing origin control and all 853 prior
selected cases pass: **875 total**, with formatting and offline workspace/all-
target Clippy. Shared programs, other native values, managed-container backing
buffers and wider gates remain open; M1u stays the latest accepted milestone.

The collector workspace checkpoint (2026-10-17) precedes this follow-up.
Work/mark/waiter storage retains an admitted owner before collection; borrowed
visitors and a deduplicated queue avoid intermediate vectors and unplanned
hash growth. Returned realm marks retain ownership outside the heap. Twenty-one
new cases and all 853 selected cases pass, together with formatting and offline
workspace/all-target Clippy. Remaining managed-container backing buffers,
retained native values, shared programs and wider gates remain open; M1u stays
the latest accepted milestone.

The realm collection metadata checkpoint (2026-10-17) precedes the collector.
Its admitted flat plan covers discovery, snapshots, owner lookup, ignored pins
and wrapper targets. Eighteen finite cases and all 832 selected cases passed;
see the dated record for its original failures and source/artifact audit.

The safepoint trace batch checkpoint (2026-10-17) precedes this follow-up.
Three tiny root/edge refusal cases initially entered heap work at a full scalar
quota; all three now pass. Borrowed visitors replace intermediate reference
vectors, and reusable flat root/owner-edge batches retain admitted capacity
through cached, Rust-held and heap-held ownership. Joint replacement precedes
heap effects; retained capacity remains reusable during terminal cleanup.
All seventeen finite lifecycle cases and all 814 fixed selected cases pass,
along with formatting and offline workspace/all-target Clippy. Collector
work/mark sets, realm traversal, retained native values, shared programs and wider
gates remain open; M1u stays the latest accepted milestone.

The temporary execution root capacity checkpoint (2026-10-17) precedes this follow-up.
Four tiny native-pressure/callback/job/iterator cases failed before admission
was moved ahead of vector growth and job removal; all four now pass. Seventeen
finite ownership/restoration/GC cases pass. Every temporary execution-root
append now uses admitted capacity; detached replacement budgets simultaneous
old/new storage. All 797 fixed selected cases pass across 38 executables and
69 selections, together with formatting and offline workspace/all-target Clippy.
Other GC scratch vectors, retained native values, shared programs and wider gates
remain open; M1u stays the latest accepted milestone.

The displaced execution rooting checkpoint (2026-10-17) precedes this follow-up.
Four initial refusal/restoration cases and a separate public ShadowRealm
fatal-error case failed before the fix. All fourteen new finite lifecycle cases
now pass. Saved-edge enumeration borrows the frame; temporary registered roots
own admitted segments and roll back only their new suffix. Queued jobs, deferred
namespaces and reentrant realms restore displaced owners before returning errors.
All 780 fixed selected cases pass across 38 executables and 68 selections,
together with formatting and offline workspace/all-target Clippy. General
trace-vector growth admission, retained native values, shared programs and wider
gates remain open; M1u stays the latest accepted milestone.

The module root storage checkpoint (2026-10-17) precedes this follow-up.
Four tiny native-admission, registration-error and cached-completion cases
initially failed and now pass. Module roots use admitted segments/reservations
that splice without allocation during cleanup. Synchronous object completions
now retain a persistent root before their cached value is published, surviving
later script collection. All fourteen finite lifecycle groups and all 766 fixed
selected cases pass, along with formatting and offline workspace/all-target
Clippy. Retained native value payloads, shared programs and wider gates remain
open; M1u stays the latest accepted milestone.

The module record-table checkpoint (2026-10-17) precedes this follow-up.
Three tiny retained/Rust-held/admission cases initially failed and now pass.
Record tables now own their admitted slots and canonical key buffers; growth
reserves a detached replacement while old storage coexists. Name selection
borrows immutable registry keys with a leased vector. All thirteen finite groups
and all 752 fixed selected cases pass, together with formatting and offline
workspace/all-target Clippy. Root-list merging, retained record values and
shared programs remain open; M1u stays the latest accepted milestone.

The linked module cell checkpoint (2026-10-17) precedes this follow-up.
Three tiny completed/pending/admission cases initially failed and now pass.
Linked tables reserve capacity before managed cells are created; a separate
realm counter follows their leases while graphs are stored, parked or held in
Rust. Entry/completion transfers preserve existing admission, and await/debugger
copies are staged before execution-state capture. All thirteen finite lifecycle
groups and all 739 fixed selected cases pass, along with formatting and offline
workspace/all-target Clippy. Wider graph metadata and shared programs remain
open; M1u stays the latest accepted milestone.

The module continuation checkpoint (2026-10-17) precedes this follow-up.
Three tiny await/debugger probes initially failed: suspended frames copied root
Bytecode and did not retain the installed registry's charge. They now share an
immutable executable snapshot and stage admitted identifier copies before frame
capture. Thirteen finite ownership/lifecycle groups pass, including replaced
loaders, repeated await with GC, debugger transitions, late bounded activation,
rejection and terminal cleanup. All 726 fixed selected cases, formatting and
offline workspace/all-target Clippy pass. Shared child/source programs, linked
cell maps and integrated acceptance remain open. M1u stays the latest accepted
milestone.

The immutable host module-source checkpoint (2026-10-17) precedes this follow-up.
Three small native/runtime probes reproduce uncharged raw JavaScript/JSON/text/
bytes/source-phase data, replacement without admission and independent ShadowRealm
buffer copies. All three now pass with immutable shared data, one document charge
and refusal preserving the previous snapshot. All thirteen expanded lifecycle/
budget/typed-import groups and offline workspace/all-target Clippy pass. All 641
selected cases and formatting pass; the source/artifact audit is recorded below.
The later compiled-registry checkpoint passes 713 selected cases (record below).
Shared programs and wider code/cache/continuation ownership remain open work.

The module context-name checkpoint (2026-10-17) addresses these failures. Module
entry copies names after replacing caller fields; displaced names are unleased,
and debugger pauses clone the ambient allocation into both VM and continuation.
Three small module/callback/debugger probes fail initially and now pass after
staging new names and moving ambient ownership across pause/resume. A separate
ShadowRealm probe reproduces a Promise capability created before name refusal;
that preparation now precedes capability creation. Eleven expanded groups pass;
all 628 selected cases, formatting and offline workspace/all-target Clippy pass.
The source/artifact audit is recorded below. Source registries and continuation
code/cell copies remain open; M1u stays the latest accepted milestone.

The module closure-referrer checkpoint (2026-10-17) addresses these failures.
Four small accounting/runtime/collection probes reproduced uncharged table/name
storage, dead names after GC, creation without admission and a call-copy refusal
after instruction consumption. All four now pass with weak-key reclamation,
staged publication and early call-name preparation. Expanded controls are in
place and all twelve groups pass, along with offline workspace/all-target
Clippy. All 617 selected cases and formatting pass; the source/artifact audit is
recorded below. Other module-name/code/cache/native/integrated work remains
open; M1u stays the latest accepted milestone.

The realm template-cache checkpoint (2026-10-17) addresses these failures.
Three finite async/generator/cache-entry probes reproduce stale template
identity after resumption and allocation without native admission (zero passed,
three failed). One realm now owns the cache across all frame changes and stages
new publication before heap effects. Thirteen expanded controls pass; the final
bounded selection passes all 605 cases, with formatting and offline workspace/
all-target Clippy passing. The source/artifact audit is recorded below; wider
native/integrated gates remain open and M1u stays the latest accepted milestone.

The generator parameter-entry checkpoint (2026-10-17) addresses two
runtime-entry regressions that reproduced generator/environment
allocation before native refusal; both pass after joint stack/environment
preparation before generator creation. Expanded exact-budget, forwarded-owner,
caller-root/collection and real parameter/eval controls pass all nine groups.
All 591 selected cases, formatting and offline workspace/all-target Clippy pass;
the source/artifact audit is recorded below. Other association copies and wider
native/integrated gates remain open; M1u remains the latest accepted milestone.

The outgoing suspension checkpoint (2026-10-17) jointly admits stack/completion
targets before allocation or frame mutation and retains scoped ownership until
adoption. Fourteen finite groups pass, including a real debugger refusal that
previously left a nested continuation behind. Terminal root cleanup now drops
those frames and releases their result roots. All 582 selected cases, formatting
and offline workspace/all-target Clippy pass; the source/artifact audit is
recorded below. Wider native/integrated gates remain open; M1u remains the
latest accepted milestone.

The generator resume checkpoint (2026-10-17) prepares stack/completion/module
storage and incoming native ownership while saved state stays in the heap.
Both initial state-consumption regressions and ten expanded controls pass.
Extracted buffers remain charged through adoption; pending roots use a borrowed
iterator. All 568 selected cases, formatting and offline workspace/all-target
Clippy pass; the source/artifact audit is recorded below. Wider native/integrated
gates remain open;
M1u remains the latest accepted milestone.

The realm-global record checkpoint counts persistent table/key capacity and
prepares script/eval records and initial values before declaration effects.
Deleted-global recreation refuses before heap/root effects, while ShadowRealm
reentry retains extracted names/roots and reserved restoration slots. Eval and
nested-script return reconcile after caller restoration and temporary release.
Fourteen expanded groups and all 545 selected cases pass, with formatting,
offline workspace/all-target Clippy and the source/artifact audit recorded below.
Other association copies, conversion preallocation, wider native storage and
integrated acceptance remain open; M1u remains the latest accepted milestone.

The function-eval publication checkpoint jointly prepares names, shadowed-cell
lists and declaration/alias tables before heap effects. Two tiny regressions
reproduce new declarations and deleted-variable recreation succeeding without
native admission; both now pass. Eleven expanded groups and the full 496-case
bounded selection pass, together with formatting and offline workspace/all-target
Clippy. The source/artifact audit is recorded below. Other association copies,
conversion preallocation and integrated acceptance
remain open; this does not close the wider M1z gate.

The dynamic environment entry/recovery checkpoint addresses two
finite groups that reproduce a constructor getter and async result capability
published before environment-storage refusal. Both now pass with joint stack/
recovery preparation before entry effects and context replacement. Necessary
copies retain scoped ownership; synchronous forwarding creates no recovery
copy. Eight expanded groups and the full 485-case bounded selection pass with
formatting and offline workspace/all-target Clippy. This implementation
checkpoint is recorded below; dynamic binding publication, other association
copies, conversion preallocation and integrated acceptance remain open.

The generator storage checkpoint addresses two finite two-yield groups that
reproduced maps losing 31 reserved slots across resume; both now pass.
Suspension/resume retain the actual maps rather than converting them to/from
vectors. Managed state counts their table capacity and shared binding edges;
Rust-held state carries a scoped native charge through admission and heap
adoption. Ten expanded groups and the full 477-case bounded selection pass,
along with formatting and offline workspace/all-target Clippy. This checkpoint
is recorded below; stack/completion conversion preallocation, dynamic binding
work and integrated acceptance remain open.

The captured/dynamic frame-map checkpoint addresses allocation after constructor
prototype callbacks and generator state extraction. Two finite 48-capture groups
reproduce both effects and now pass with scoped, admitted table preparation.
Call, eval and generator entry share the path; generator resume admits incoming
dynamic name/shadowed-buffer ownership before taking state. Expanded controls
pass all ten groups; the full bounded selection passes 467 cases, formatting
and offline workspace/all-target Clippy. This implementation checkpoint is
recorded below; other transient storage and integrated acceptance remain open.

The scope-entry checkpoint prevents bindings from being cleared and alias cells
from being published before scope storage is admitted. Joint admission now precedes
lexical mutation; all ten expanded groups pass. Generator scope-list staging is admitted
before taking saved state and retains a scoped native charge. Expanded finite
validation passes 457 cases, formatting and offline workspace/all-target Clippy.
This implementation checkpoint is recorded below; other container/map/transient
storage and integrated acceptance remain open.

The frame-binding admission follow-up reproduces root allocation before refusal
and constructor field effects before frame admission. Shared binding/name
staging now admits before allocation and keeps a scoped charge through entry;
all eight expanded groups pass. Script, call, eval, nested script, module and
generator entry use this path; module cells and generator state remain in place
until admission succeeds. All 445 selected cases, formatting and offline
workspace/all-target Clippy pass. The implementation checkpoint is recorded
below; broader preallocation/staging and integrated acceptance remain open.

The call-entry follow-up reproduces six storage/accounting failures before the
fix. Twelve bounded groups now pass for reserved argument capacity, constructor
callbacks, bound/Proxy forwarding, host conversion, parameter handoff, tail-call
buffer reuse and managed generator container capacity. All 436 selected cases,
formatting and offline workspace/all-target Clippy pass. This implementation
checkpoint is recorded below; wider staging, preallocation, module-cache/
service storage and integrated acceptance remain open.

The disposal follow-up addresses two ownership failures: interleaved generator/
async frames share one resource stack and dispose the wrong frame's entries,
while disposal side tables root unreachable resources and cycles forever.
Five initial groups reproduce both failures and missing capacity accounting.
Fifteen expanded groups now pass with frame-owned resource/mark storage, native
admission, conditional GC edges and peer-ledger reconciliation. All 403 selected
cases, formatting and offline workspace/all-target Clippy pass. The checkpoint
is recorded below; wider entry/transient staging, other preallocation and
integrated acceptance remain open.

The interpreter-record follow-up now carries allocation charges with active,
Rust-held and suspended owners and admits record growth before reserve and
publication. Fourteen finite groups cover callbacks, exact spare slots,
IteratorClose, async/generator handoffs, debugger pause/resume, VM destruction
and cross-realm ledger release. Its 387-case checkpoint is preserved below;
the following disposal checkpoint addresses resource ownership. Wider entry/
transient staging, other preallocation and integrated gates remain open.

The active native ledger now includes VM operand, binding, scope, eval and
completion containers alongside primitive payloads. Scoped charges follow
displaced caller allocations across callbacks; ordinary calls forward eval
maps without unnecessary recovery copies. Suspension/debugger edits update
both saved charges, and finished root scripts return their frame storage.
Eleven new groups, 58 debugger cases, 38 public semantic cases and the prior
193 ownership controls pass: 300 cases, formatting and offline workspace/
all-target Clippy. This is an implementation checkpoint. Wider entry/transient
staging, other preallocation and integrated acceptance
remain open; M1u remains the latest accepted milestone.

**In progress — M1y asynchronous module dependency ownership (2026-10-16).**
Static asynchronous evaluation previously kept forward dependency sets and
reverse parent vectors outside the native ledger. Both directions now account
for table/slot/name capacity, admit new edge batches before publication and
release obsolete links, including parents of still-running siblings. Scoped
charges cover traversal/frontier storage and collected dependency names across
later JavaScript callbacks; deferred frontiers no longer linger in a realm
result cache. Sixteen small-graph regression groups and the selected controls
pass: 193 cases, formatting and offline workspace/all-target Clippy. This is an
implementation checkpoint; integrated acceptance remains open and M1u remains
the latest accepted milestone.

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

The Promise-table follow-up now checks Promise record growth, paired host wait/
Promise tables and async/module continuation growth together with their
resumption work. Twelve of fifteen initial tiny-owner groups fail on the parent;
all fifteen pass with joint admission. Script refusal and early argument-lease
refund controls also pass. That checkpoint passes 148 selected cases, formatting
and offline workspace/all-target Clippy. No fresh integrated acceptance is
claimed.

The import-waiter follow-up now covers ordinary/deferred import publication,
shared waiting counts and release after fulfillment or rejection. Eight of ten
small-graph tests fail on the parent; the two available-slot controls pass.
Prepublication admission, reclamation and all twelve expanded groups now pass,
including partial-dependency shrink and a deterministic namespace-root refusal.
The full selected checkpoint passes 177 cases, formatting and offline
workspace/all-target Clippy. Final integrated acceptance remains open.

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
- [ ] **M1y: asynchronous module dependency ownership.** Charge both directions
      of waiting graph edges, including actual table/set/vector slots and name
      buffers. Admit a parent's complete batch before publishing either
      direction or marking it suspended; duplicate edges add no charge. Release
      obsolete sibling links on rejection and preserve source/BFS ordering.
      Cover Rust-held traversal/frontier storage across JavaScript callbacks
      and return it on every exit. See the implementation record below.
- [ ] **M1z: active execution control capacity.** Active and displaced operand,
      binding, scope, eval and completion containers must enter the document
      budget alongside primitive payloads. Admission/release tests must follow
      real call/eval/generator/async ownership transfers, avoid charging maps
      forwarded into the callee twice, and preserve suspension/debugger state.
      VM-field, interpreter-record, disposal, call-entry, frame-binding,
      scope-entry/restoration and frame-map checkpoints below pass their bounded
      selections. Generator storage, dynamic environments/bindings, realm-global
      records, incoming/outgoing snapshot buffers and parameter-entry roots now
      have bounded implementation checkpoints. Realm template-cache ownership
      now joins module records, sources, roots, safepoint batches, realm metadata
      and collector workspace ownership. Cached completion/error values, admitted
      error-reporting batches, persistent root tables, foreign callable pins,
      host-turn realm/timer/abort snapshots and finalization index/snapshot/direct
      cleanup plus callback-entry preparation and managed cell-vector backing
      now have bounded checkpoints. Timer callback preparation, interval GC
      protection and argument handoff now join complete timer registration/root-
      batch staging plus admitted Promise job/reaction replacements and atomic
      await queue/map publication. Detached Promise record creation now preserves
      admitted slots through GC and refusal. Host waits now prepare arguments,
      joint tables and root storage before token/capability publication; all
      1176 selected controls passed that checkpoint. Rooted frame/cleanup/
      rejection registration now prepares complete queue and root storage
      before capacity/handle/serial publication. Complete browser-task queue/
      payload/root preparation and staged FileReader state now join those
      checkpoints. Render handle snapshots and animation-frame argument/fuel
      preparation now preserve unattempted callbacks and clean up committed
      dispatch/report failures. Browser-task activity classification, starting
      fuel and drag argument admission now precede task consumption, with stale
      cleanup and future deferral preserved; all 1300 selected controls pass.
      Import delivery now prepares native snapshots and a complete temporary
      root batch before dependency progress or consumption, retaining queued
      targets after an earlier committed attempt fails. Common settlement fan-out
      now stages complete copies/queue growth before status publication.
      Then registration prepares native input/queues before handled-state changes.
      Continuation awaits now share admitted status/queue preparation and publish
      handled state after frame/work ownership. Finally provenance retains hash
      backing cost and prepares table/source state across GC. Other input/origin producers
      remain open. Eval compiler environment producers now preflight borrowed
      selection scratch, copied names and returned snapshot backing; fourteen
      fixed semantics/lifetime/refusal cases pass. Shared programs, other
      managed-container and scheduling backing buffers,
      retained-map ownership, wider native storage and integrated acceptance
      remain open.
- [ ] **M1x: scheduling capacity admission.** Refuse container growth before
      publishing another task, reaction, registration or handle; admit appends
      that fit existing storage without a fictitious growth charge. Timer,
      frame, Promise-job and Promise-reaction regressions use at most nine
      entries and a scalar shared-owner model. The implementation checkpoint
      below now includes browser-task/cleanup/rejection producers, FileReader
      requeue/cancellation, timeout clock ordering and joint Promise/await-table
      admission and import-waiter publication/reclamation. Those implementation
      checkpoints pass their bounded selections; integrated acceptance and the
      wider native control/service-buffer gate remain open.
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

### M1z — Active execution control storage (implementation checkpoint, 2026-10-16)

Acceptance: active/displaced operand, argument, binding, scope, eval, completion
and associated name buffers share the document native budget. Count actual
reported container capacity, include nested allocations, and keep caller-held
owners charged while callees execute. Forwarded eval environments have one
owner; independent backup copies have their own charge. Suspension, resumption,
debugger edits and unwind must preserve and reconcile those owners. Release
finished execution storage without changing persistent JavaScript state.

- [x] Reproduce missing capacity, callback ownership and finished-root release
      with finite functional tests and scalar admission controls.
- [x] Implement active/displaced VM-field control ownership and suspension handoff.
- [x] Verify duplicate-free eval forwarding, refusal/unwind, async/generator
      and debugger boundaries, plus relevant prior ownership controls.
- [x] Pass formatting and offline workspace/all-target Clippy, update this
      record and record the source/artifact audit with the checkpoint commit.
- [ ] Complete integrated/full-source acceptance under the owner's security
      constraints. Broader module-cache/service and transient accounting remain
      open; M1u remains the latest accepted milestone.

The initial active-field implementation did not close interpreter-local
iterator/handler buffers. The follow-up below addresses those record owners.
The disposal follow-up below addresses resource owners. Wider call-entry/
transient staging and admission before every other active-container allocation
remain open alongside the integrated gate;
safepoint accounting alone is not a preallocation guarantee.

The initial five regression groups fail before implementation and pass with
active/displaced control charges. They check eighteen independent containers,
five nested/name allocations, script entry refusal before callbacks, actual
call/eval/generator/async ownership and finished-root release. Red log:
`/private/tmp/blueice-execution-control-red-20261016.log`.
A separate forwarding regression observes 3,664 bytes in an unnecessary
synchronous-call recovery copy; a normal call now forwards its eval environment
without creating that copy. Red log:
`/private/tmp/blueice-execution-control-forwarding-red-20261016.log`.
Expanded groups cover suspended stack capacity, async splits, deterministic
abort reconciliation, persistent template identity and bounded debugger frames.
The bounded debugger group independently reproduces a returned string entering
a still-paused module parent without updating its primitive lease (12 charged
bytes versus 22 actually owned). Stack edits now refresh both saved charges.
Red log: `/private/tmp/blueice-execution-control-debugger-red-20261016.log`.

The first control replay also exposed three outdated capacity fixtures: a new
script's frame could consume the exact spare-slot budget before reaching timer/
frame admission, or before a FileReader callback completed. Timer/frame fixtures
now invoke the actual native producer after preparing the callable, preserving
their exact slot/root admission assertions. FileReader uses eight tasks for the
count boundary and 32 for the byte boundary so that successor queue growth is
larger than the finite callback's own frame; it still verifies refusal only
after the callback's tasks have been published and the reader root is released.

VM-field checkpoint (`fe8703359`) validation:
**300 passed, zero failed, zero ignored** across
sixteen test executables. Eleven new groups cover the native capacity and
ownership contracts; 58 existing debugger cases preserve script/module pause,
step, resume, catch/finally and object-root behavior. Seven public semantic
targets pass 38 cases: `generator_resume_with_objects` (three),
`parameter_eval_scope` (ten), `eval_var_environment_capture` (six),
`eval_var_deletion` (seven), `eval_var_redeclaration` (four),
`with_eval_inner_scope` (two) and `yield_template_context` (six).
The previous 193 ownership cases pass on this source: sixteen dependency groups,
twelve import-waiter groups, 75 scheduling/handoff/restoration controls and 90
public async/import/Promise cases. Six other host-async pressure/limit cases
remain filtered under the owner's security constraint. The tests are finite
and offline; the abstract budget owners allocate no represented large buffer.

Formatting, `git diff --check` and offline workspace/all-target Clippy with
warnings denied pass. Logs use `/private/tmp/blueice-execution-control-` with
`{capacity,debugger,semantics,final-unit,waiter-controls,promise-controls,slot-controls,browser-controls,handoff-controls,call-controls,timer-controls,await-refusal-controls,continuation-controls,cleanup-controls,source-controls,public-controls,fmt,clippy}-20261016.log`.
Receipt: `target/execution-control-source-20261016.json`.
No fresh full Test262, real Core/MCP replay or coverage result is claimed.

#### Interpreter record ownership follow-up (`9741452ae` implementation checkpoint)

Acceptance: native iterator/handler allocation charges follow their actual
owner through active execution, nested callbacks, IteratorClose, async/module
await, generator managed storage and debugger pause/resume. Capacity and native
iterator payloads are charged once; diagnostic origin ownership stays on its
existing path. Admit new records before reserve/publication, preserve available
slots, release storage on completion/error, and retain valid GC roots and close
ordering across every transfer.

- [x] Reproduce five missing-ownership/admission groups with finite scripts and
      a scalar shared-budget owner; no represented large buffer is allocated.
- [x] Carry charges with iterator/handler containers across all interpreter
      exits, continuation owners and cleanup paths; admit record growth first.
- [x] Validate refusal, spare capacity, nested callbacks, collection, cleanup
      ordering, async/generator and debugger ownership and release.
- [x] Pass bounded controls, formatting and offline workspace/all-target
      Clippy; record the source/artifact audit with the checkpoint commit.

Initial red log: `/private/tmp/blueice-interpreter-records-red-20261016.log`
(zero passed, five failed). Broader entry staging, other active-container
preallocation, full module-cache/service and integrated gates remain open.

The expanded shared-ledger control independently reproduces a stale contribution
after direct interpreter return: a peer realm observes 976 bytes while the
owner's current snapshot recomputes 464 bytes. The missing 512-byte release is
the exited handler allocation. Every interpreter exit now reconciles ownership
after restoring the outer diagnostic context and dropping its temporary charge.
Evidence: `/private/tmp/blueice-interpreter-records-reconcile-red-20261016.log`.

Final bounded validation: **387 passed, zero failed, zero ignored** across 21
test executables. Fourteen new groups pass. Six additional unit controls pass:
two closure exits, one module exit, one generator exit, one real async-generator
close and one nested iterator close/first-error/GC group. Five additional public
targets pass 67 cases: `generator_delegation` (eleven), `resource_management`
(35), `resource_management_gc` (thirteen), `for_of_return_iterator_close` (five)
and `async_iterator_dispose` (three). The previous 300-case VM-field selection
also passes on this source. Six other host-async pressure/limit cases remain
filtered under the owner's security constraint. New scripts use fixed small
records and bounded job steps; scalar owners allocate no represented large
buffer. No physical resource-exhaustion or network-listener probe is run.

Formatting, `git diff --check` and offline workspace/all-target Clippy with
warnings denied pass. Logs use
`/private/tmp/blueice-interpreter-records-` with
`{records,closure-exits,module-exits,generator-exits,generator-close,iterator-close,record-semantics,capacity,debugger,semantics,final-unit,waiter-controls,promise-controls,slot-controls,browser-controls,handoff-controls,call-controls,timer-controls,await-refusal-controls,continuation-controls,cleanup-controls,source-controls,public-controls,fmt,clippy}-20261016.log`.
Receipt: `target/interpreter-records-source-20261016.json`.
Disposal/resource temporaries, call-entry staging, other active-container
preallocation, full module-cache/service storage and integrated acceptance
remain open. No fresh full Test262, real Core/MCP replay or coverage result is
claimed; M1u remains the latest accepted milestone.

#### Disposal ownership follow-up (implementation checkpoint)

Acceptance: each active/suspended call, eval, generator and module frame owns
its declaration resources and disposal scope marks. Interleaving resumes the
correct resources in the correct order. Saved disposer methods remain GC-live
across nested calls, suspension and debugger pauses. Native record/mark/table
capacity and adopted payloads share document admission; growth refuses before
publication, existing slots avoid invented growth and move refuses before
changing the source state. Managed generator storage owns its transferred
buffers. Side-table resources are conditional edges from their stack object,
so unreachable stacks and callback cycles release resources without running
disposers. Collection and frame cleanup reconcile the shared ledger.

- [x] Reproduce five failures with finite scripts and small native allocations.
- [x] Move declaration resources/marks with frame suspension and restoration;
      include managed generator capacity and GC references.
- [x] Charge/admit native resource records, marks, stack tables and temporary
      entry vectors; retain popped resources and pending error payloads across
      synchronous disposal callbacks.
- [x] Replace unconditional side-table resource roots with owner edges; prune
      dead capability rows and reconcile collected storage for peer realms.
- [x] Pass bounded controls, formatting and offline workspace/all-target
      Clippy; record the source/artifact audit with the checkpoint commit.
- [ ] Complete the wider entry-staging, other preallocation, module-cache/
      service-buffer and integrated gates. M1u remains the latest accepted
      milestone; no full-source acceptance is claimed here.

Initial red log: `/private/tmp/blueice-disposal-ownership-red-20261016.log`
(zero passed, five failed). Interleaved generator cleanup produces `b,a`
instead of `a,b`; the first resumed async frame disposes `b` instead of `a`;
unreachable adopted objects stay reachable through side-table roots; active
resource/mark and side-table capacity deltas report zero.
The first five groups pass after the ownership implementation. Fifteen expanded
groups additionally cover refusal/exact spare slots, adopted primitive payload,
constructor/move atomicity, nursery-one collection of ephemeral disposer
methods, nested calls/eval/generator entry, sibling module awaits, debugger
pause/resume, live-stack retention/disposal release, VM destruction and shared
ledger release after table collection.

Final bounded validation: **403 passed, zero failed, zero ignored** across 21
test executables. The fifteen new groups, one strengthened heap generator
reference/capacity group and the previous 387-case ownership selection pass.
The heap group checks all three resource object edges and the actual resource/
mark capacity delta in managed state. The previous selection includes sixty-
seven public disposal/delegation/IteratorClose cases, async/import/Promise GC,
58 debugger cases and the earlier native admission/ownership controls.
Six other host-async pressure/limit cases remain filtered under the owner's
security constraint. New graphs/resources and job loops are fixed and small;
scalar budget owners allocate no represented large buffer.

Formatting, `git diff --check` and offline workspace/all-target Clippy with
warnings denied pass. Logs use `/private/tmp/blueice-disposal-ownership-` with
`{resources,heap-resources,records,closure-exits,module-exits,generator-exits,generator-close,iterator-close,record-semantics,capacity,debugger,semantics,final-unit,waiter-controls,promise-controls,slot-controls,browser-controls,handoff-controls,call-controls,timer-controls,await-refusal-controls,continuation-controls,cleanup-controls,source-controls,public-controls,fmt,clippy}-20261016.log`.
Receipt: `target/disposal-ownership-source-20261016.json`.
Wider entry/transient staging, other active-container preallocation, full
module-cache/service storage and integrated acceptance remain open. No fresh
full Test262, real Core/MCP replay or coverage result is claimed; M1u remains
the latest accepted milestone.

#### Call-entry and generator storage follow-up (implementation checkpoint)

Acceptance: incoming argument vectors retain their actual capacity charge
through prototype getters, field initializers and native callbacks. Bound
flattening admits its one final buffer before allocation; closure metadata and
host argument conversions retain their own native charges. Ordinary, generator
and Proxy handoffs transfer ownership without duplicate charges. Managed
generator states account for their boxed state and each retained container's
actual capacity and payload, independently of their GC reference list.

- [x] Reproduce six failures with tiny vectors and finite callbacks; retain
      the initial red log.
- [x] Add paired call-entry payload/control leases, admitted bound/host
      conversion storage and closure metadata ownership.
- [x] Reuse the tail-call argument carrier and avoid duplicate generator
      parameter-entry copies; account for all saved generator containers.
- [x] Pass twelve focused native/heap groups, including refusal before callback
      and single-owner handoffs.
- [x] Pass the selected finite semantic/GC controls, formatting and Clippy;
      record the source/artifact audit with the checkpoint commit.
- [ ] Complete wider transient staging, other preallocation, full module-cache/
      service storage and integrated acceptance. M1u remains the latest
      accepted milestone; no full Test262/Core/MCP/coverage gate is claimed.

Initial red log: `/private/tmp/blueice-call-entry-storage-red-20261016.log`
(zero passed, six failed). Focused expanded log:
`/private/tmp/blueice-call-entry-storage-expanded-20261016.log`
(twelve passed, zero failed/ignored).

Call entry now retains separate primitive and control leases. Captures and
with-environments are admitted before copying; borrowed metadata inspection
avoids an unused captured-environment clone. Receiver replacement releases the
old primitive owner. Bound dispatch admits wrapper-ID storage and one final
argument buffer, then releases its lookup vector before the target runs. Host
conversion admits its native vector and copied strings before conversion and
keeps them charged throughout the callback. Tail calls consume the carrier
prefix in place. Every call-entry exit reconciles released native ownership.

Parameter initialization uses a rooted running generator placeholder while
the original inputs retain native ownership. Saved managed state charges its
fixed state box, actual retained vector capacities, dynamic-binding storage and
value/completion payloads; GC references remain reachability edges. The new
heap cases check reserved start-state vectors and all thirteen suspended
container capacities.

Final bounded validation: **436 passed, zero failed, zero ignored** across 26
test executables. This includes twelve new groups, one existing heap restoration
case, eleven cross-realm/generator-constructor/async-GC cases, eight bound-call
cases, one finite tail-call semantic matrix and the prior 403 ownership controls.
The six previous host-async pressure cases and the larger retained-bind
allocation case remain filtered. Deep-recursion tail suites are not run.
New vectors/graphs and callback/job loops are fixed and small; scalar budget
owners allocate no corresponding large buffer.

Formatting, `git diff --check` and offline workspace/all-target Clippy with
warnings denied pass. Logs use `/private/tmp/blueice-call-entry-storage-` with
`{entry,heap-restoration,entry-semantics,bound-semantics,tail-semantics,resources,heap-resources,records,closure-exits,module-exits,generator-exits,generator-close,iterator-close,record-semantics,capacity,debugger,semantics,final-unit,waiter-controls,promise-controls,slot-controls,browser-controls,handoff-controls,call-controls,timer-controls,await-refusal-controls,continuation-controls,cleanup-controls,source-controls,public-controls,fmt,clippy}-20261016.log`.
Receipt: `target/call-entry-storage-source-20261016.json`.
Other transient staging, remaining active-container preallocation, full
module-cache/service storage and integrated acceptance remain open. No fresh
full Test262, real Core/MCP replay or coverage result is claimed; M1u remains
the latest accepted milestone.

#### Frame-binding admission follow-up (implementation checkpoint)

Acceptance: binding-value slots, metadata slots/names and function variable-
scope lexical names are admitted before allocating a new execution frame.
Actual staged capacity retains a native owner until transfer into VM fields.
Script, ordinary/async call, generator parameter/resume, eval, nested script
and module entry use this same path. Refusal precedes constructor field/default
parameter effects, preserves a generator's saved state, and leaves module input
cells and the caller context intact.

- [x] Reproduce root allocation after refusal and constructor field effects
      before a rejected frame; both initial groups pass after implementation.
- [x] Share staged binding/name admission across execution entries, retaining
      actual-capacity charges through callbacks and field transfer.
- [x] Make module entry fallible before taking input cells; prepare generator
      metadata/bindings before taking its saved state.
- [x] Pass expanded refusal/exact-budget/ownership and related finite semantic/
      GC controls, formatting and Clippy; record the source/artifact audit with
      the checkpoint commit.
- [ ] Complete other container growth, map/scope restoration and temporary
      staging, full module-cache/service storage and integrated acceptance.
      M1u remains the latest accepted milestone.

Initial red log: `/private/tmp/blueice-frame-binding-red-20261016.log`
(zero passed, two failed). Root binding capacity changes from zero to four
despite refusal; a constructor with 48 parameters executes its field callback
before its binding frame exceeds a scalar shared-owner budget. The fixed
two-case log is `/private/tmp/blueice-frame-binding-green-20261016.log`.

Eight expanded groups pass: exact-budget and one-byte refusal for script,
function and resumed-generator staging; storage ownership through callbacks
and detached-part lifetime; eval/nested caller preservation; module input cell/
context preservation; initial/suspended generator state preservation; and
default initializer refusal for ordinary/generator/async/async-generator calls
before publishing a result capability. The constructor checks 48 small formal
names and one field callback. Scalar peer owners allocate no represented large
buffer. Simple generator creation still defers its binding frame to first
resume; suspended resume reuses its saved bindings.

Final bounded validation: **445 passed, zero failed, zero ignored** across 26
test executables. The eight new groups, an existing module scope/cell cleanup
contract and the previous 436 finite ownership/semantic/GC/debugger cases pass.
The seven previous pressure/large-allocation exclusions remain filtered;
deep-recursion tail suites and network/service replays are not run.

Formatting, `git diff --check` and offline workspace/all-target Clippy with
warnings denied pass. Logs use `/private/tmp/blueice-frame-binding-` with
`{frames,module-entry,entry,heap-restoration,entry-semantics,bound-semantics,tail-semantics,resources,heap-resources,records,closure-exits,module-exits,generator-exits,generator-close,iterator-close,record-semantics,capacity,debugger,semantics,final-unit,waiter-controls,promise-controls,slot-controls,browser-controls,handoff-controls,call-controls,timer-controls,await-refusal-controls,continuation-controls,cleanup-controls,source-controls,public-controls,fmt,clippy}-20261016.log`.
Receipt: `target/frame-binding-source-20261016.json`.
Other container growth, map/scope restoration, wider transient staging, full
module-cache/service storage and integrated acceptance remain open. No fresh
full Test262, real Core/MCP replay or coverage result is claimed; M1u remains
the latest accepted milestone.

#### Scope-entry and restoration admission follow-up (implementation checkpoint)

Acceptance: scope ID/slot-list vectors, a new scope's slot buffer and alias-cell
table growth pass joint admission before bindings or cell aliases change.
Reuse existing vector/table slots, including cells removed by the same scope.
Preserve eval-over-script alias priority and module outer cells. Generator
scope-list restoration admits/clones its new nested buffers before taking
saved state, retains native ownership through transfer and reconciles temporary
owners on every direct generator-resume exit.

- [x] Reproduce binding reset and alias publication despite refused scope
      growth; both initial groups pass after implementation.
- [x] Route interpreter and module-body scope entry through joint admission;
      check minimum map growth before preparing actual table capacity.
- [x] Prepare/charge generator scope-list restoration before state extraction;
      reconcile released staging through a common resume wrapper.
- [x] Pass expanded capacity/alias/module/generator/peer controls and the
      relevant finite semantic/GC/debugger selection, formatting and Clippy;
      record the source/artifact audit with the checkpoint commit.
- [ ] Complete other container growth, captured/dynamic-map restoration,
      per-iteration cell staging, wider transient/module-cache/service storage
      and integrated acceptance. M1u remains the latest accepted milestone.

Initial red log: `/private/tmp/blueice-scope-admission-red-20261016.log`
(zero passed, two failed). Two existing binding values become `None`, and
global cell aliases enter the active table before admission fails. The first
fixed log is `/private/tmp/blueice-scope-admission-green-20261016.log`.

Ten expanded groups cover joint refusal before either scope vector reserves,
exact spare-vector storage, actual table rounding before alias publication,
table slots released by the same scope, eval-over-script alias priority,
module cell retention and generator nested-buffer ownership. A generator test
leaves room for binding staging but one byte too little for restored scope
lists; refusal preserves the saved scopes and managed bytes, drops the staged
bindings, and a second realm observes the exact shared-ledger refund.

Final bounded validation: **457 passed, zero failed, zero ignored** across 27
test executables. The ten new groups, two finite public variable/loop scope
matrices and the previous 445 ownership/semantic/GC/debugger cases pass.
New scopes contain two to four bindings, and the generator saves two active
scopes. Scalar budget owners allocate no represented large buffer. The seven
previous pressure/large-allocation exclusions remain filtered; deep-recursion
tail suites and network/service replays are not run.

Formatting, `git diff --check` and offline workspace/all-target Clippy with
warnings denied pass. Logs use `/private/tmp/blueice-scope-admission-` with
`{scopes,scope-functions,scope-loops,frames,module-entry,entry,heap-restoration,entry-semantics,bound-semantics,tail-semantics,resources,heap-resources,records,closure-exits,module-exits,generator-exits,generator-close,iterator-close,record-semantics,capacity,debugger,semantics,final-unit,waiter-controls,promise-controls,slot-controls,browser-controls,handoff-controls,call-controls,timer-controls,await-refusal-controls,continuation-controls,cleanup-controls,source-controls,public-controls,fmt,clippy}-20261016.log`.
Receipt: `target/scope-admission-source-20261016.json`.
Other container growth, captured/dynamic-map restoration, per-iteration cell
staging, wider transient/module-cache/service storage and integrated acceptance
remain open. No fresh full Test262, real Core/MCP replay or coverage result is
claimed; M1u remains the latest accepted milestone.

#### Frame-map admission follow-up (implementation checkpoint)

Acceptance: capture-cell table reconstruction forecasts minimum storage before
reserve and admits reported capacity before publication or constructor
prototype/field/default effects. Ordinary,
async, generator parameter/start/resume and eval entry share the path. Restored
dynamic tables include actual table slots, moved name capacities and nested
shadowed-cell buffers in admission before saved-state extraction. Prepared
tables carry scoped native ownership through callbacks and detached-part
transfer, then move entries without reallocation or primitive buffer copies.
Refusal preserves saved state and reconciles staged ownership for peer realms.

- [x] Reproduce constructor prototype getter effects and a lost generator
      Start state after captured-table refusal; both initial groups now pass.
- [x] Prepare/admit actual table capacity before call/eval context mutation;
      preserve simple-generator deferred creation.
- [x] Forecast generator table/name/shadowed storage before taking saved
      state; adopt entries and buffers by move into scoped native owners.
- [x] Pass expanded exact-capacity, buffer-transfer, saved-state/peer and GC
      controls plus the related finite semantic/debugger selection, formatting,
      Clippy; record the source/artifact audit with the checkpoint commit.
- [ ] Complete other container growth and table-planning scratch, outgoing
      map-to-vector suspension,
      dynamic environment backups/publication and per-iteration staging,
      wider module-cache/service storage and integrated acceptance. M1u
      remains the latest accepted milestone.

Initial red log: `/private/tmp/blueice-frame-map-red-20261016.log`
(zero passed, two failed). A constructor prototype getter executes before its
capture table is refused; a generator's refused first resume changes its Start
state to Done. Both scripts capture only 48 numeric bindings; abstract peer
budgets allocate no corresponding large buffer. Initial fixed log:
`/private/tmp/blueice-frame-map-green-20261016.log`.

Ten expanded groups pass. They verify allocated table capacity during native
callbacks and detached-part lifetime, exact/one-byte admission for capture and
dynamic tables, adoption without another reserve, and pointer-preserving moves
of a 64-byte name allocation and a nine-slot shadowed-cell buffer. A suspended
generator refuses one byte below its complete table/adopted-buffer requirement
without changing saved state or managed bytes; a peer realm observes the exact
staging refund. Tiny-nursery (one and sixteen) and explicit major collection
preserve the restored dynamic cell and its returned value. Capture refusal
precedes default initializers and Promise capability publication for all four
ordinary/generator/async/async-generator entry kinds.

Final bounded validation: **467 passed, zero failed, zero ignored** across 27
test executables. The ten new groups and the previous 457 finite ownership,
semantic, GC and debugger controls pass. Scalar peer budgets allocate no
represented large buffer. The seven previous pressure/large-allocation cases
remain filtered; deep-recursion tail suites and network/service replays are
not run.

Formatting, `git diff --check` and offline workspace/all-target Clippy with
warnings denied pass. Logs use `/private/tmp/blueice-frame-map-` with
`{maps,scopes,scope-functions,scope-loops,frames,module-entry,entry,heap-restoration,entry-semantics,bound-semantics,tail-semantics,resources,heap-resources,records,closure-exits,module-exits,generator-exits,generator-close,iterator-close,record-semantics,capacity,debugger,semantics,final-unit,waiter-controls,promise-controls,slot-controls,browser-controls,handoff-controls,call-controls,timer-controls,await-refusal-controls,continuation-controls,cleanup-controls,source-controls,public-controls,fmt,clippy}-20261016.log`.
Receipt: `target/frame-map-source-20261016.json`.
Other container growth/table-planning scratch, outgoing map-to-vector
suspension, dynamic environment backups/publication, per-iteration staging,
wider module-cache/service storage and integrated acceptance remain open.
No fresh full Test262, real Core/MCP replay or coverage result is claimed;
M1u remains the latest accepted milestone.

#### Generator storage ownership follow-up (implementation checkpoint)

Acceptance: generator suspension/resume retains the same owned cell/dynamic
tables, including spare capacity, bucket allocations and saved cell edges.
Parameter entry, ordinary yield and async yield-after-await use the same map
representation. Managed accounting includes actual map/name/shadowed capacity;
resume admits the existing tables before taking state without allocating
replacements. Rust-held outgoing state retains native buffer ownership until
the heap adopts it, releasing that lease before post-collection reconciliation.
Native refusal closes the abandoned frame and refunds staging; managed refusal
also releases native ownership. Preserve parameter entry and terminal cleanup.

- [x] Reproduce two ordinary two-yield capacity losses; both initial groups
      now pass with directly moved tables.
- [x] Share dynamic binding records with the heap; preserve managed capacity
      and GC references for both tables and shadowed cells.
- [x] Replace all three outgoing table conversions and suspended-resume table
      reconstruction with ownership moves; keep Start capture preparation.
- [x] Carry outgoing state leases through admission and heap handoff; release
      inside the heap operation before collection bookkeeping.
- [x] Pass expanded allocation/lease/refusal/GC/async controls and related
      finite semantics/debugger checks, formatting and Clippy; record the
      source/artifact audit with the checkpoint commit.
- [ ] Complete stack/completion conversion preallocation, dynamic environment
      backups/publication, other container/transient/module-cache/service
      storage and integrated acceptance. M1u remains the latest accepted
      milestone.

Initial red log: `/private/tmp/blueice-generator-storage-red-20261016.log`
(zero passed, two failed). A reserved empty cell vector and dynamic vector each
lose all 31 spare slots after an ordinary resume/yield. Fixed initial log:
`/private/tmp/blueice-generator-storage-green-20261016.log`.

Ten expanded groups pass. They preserve the actual bucket addresses of nonempty
tables across yield and major collection, check native ownership through a
callback/drop and native-to-managed byte transfer, and verify native/managed
refusal cleanup with exact peer refunds. GC checks retain dynamic cell values
and distinct shadowed cells under nursery one/sixteen and major collection.
An async await/yield round trip preserves both reserved tables. Synchronous and
asynchronous generator parameter entry finish their native handoff before returning.
The native/managed accounting fixtures use small fixed allocations, including
128 argument slots for one modeled 4 KiB heap refusal.

Final bounded validation: **477 passed, zero failed, zero ignored** across 27
test executables. The ten new groups and the previous 467 ownership/semantic/
GC/debugger cases pass. Existing heap capacity tests now measure eleven vectors
and two maps; the previous frame-map fixture checks adoption of existing full
table storage rather than reconstruction. Scalar peer budgets allocate no
represented large buffer. The seven previous pressure/large-allocation
exclusions remain filtered; deep-recursion tail suites and network/service
replays are not run.

Formatting, `git diff --check` and offline workspace/all-target Clippy with
warnings denied pass. Logs use `/private/tmp/blueice-generator-storage-` with
`{storage,maps,scopes,scope-functions,scope-loops,frames,module-entry,entry,heap-restoration,entry-semantics,bound-semantics,tail-semantics,resources,heap-resources,records,closure-exits,module-exits,generator-exits,generator-close,iterator-close,record-semantics,capacity,debugger,semantics,final-unit,waiter-controls,promise-controls,slot-controls,browser-controls,handoff-controls,call-controls,timer-controls,await-refusal-controls,continuation-controls,cleanup-controls,source-controls,public-controls,fmt,clippy}-20261016.log`.
Receipt: `target/generator-storage-source-20261016.json`.
Stack/completion conversion preallocation, dynamic environment backups/
publication, other container/transient/module-cache/service storage and
integrated acceptance remain open. No fresh full Test262, real Core/MCP replay
or coverage result is claimed; M1u remains the latest accepted milestone.

#### Call-environment admission follow-up (implementation checkpoint)

Acceptance: jointly admit forwarded dynamic-environment stack growth and required
recovery copies while the caller context is intact. Refusal precedes constructor
prototype getters, async result-capability publication and context replacement.
Include outer/current map slots, copied names/shadowed buffers and module-name
copies; recheck reported capacity before returning the prepared owner. Existing
stack slots add no growth. Ordinary synchronous calls retain forwarding without
recovery copies. Async/debugger handoff keeps snapshot ownership through field
transfer and restores the same environments/module association on all exits.

- [x] Reproduce a getter running and an async capability published before
      environment refusal; both initial groups pass after implementation.
- [x] Prepare stack capacity and required recovery copies before call effects;
      preserve synchronous forwarding and generator entry behavior.
- [x] Carry actual recovery capacity through callbacks/detached transfers and
      reconcile refusal without changing caller environment contents.
- [x] Pass expanded exact-budget, no-copy, module, recovery, peer and await/GC
      controls plus related finite semantics/debugger checks, formatting and
      Clippy; record the source/artifact audit with the checkpoint commit.
- [ ] Complete dynamic binding publication, other module-association copies,
      stack/completion conversions,
      other container/transient/module-cache/service storage and integrated
      acceptance. M1u remains the latest accepted milestone.

Initial red log: `/private/tmp/blueice-environment-admission-red-20261016.log`
(zero passed, two failed). A constructor's native prototype getter runs before
the forwarded environment vector grows; an async result Promise increases the
table from one to two entries before recovery-copy admission fails. Initial
fixed log: `/private/tmp/blueice-environment-admission-green-20261016.log`.

Eight expanded groups pass. Exact-budget/one-byte controls cover growing and
spare stack slots, populated and reserved-empty tables and both recovery modes.
Synchronous spare-slot preparation consumes no new native bytes and preserves
original buffers. Copied name/shadowed buffers use their actual clone capacity;
detached recovery fields stay charged during a native callback. Refusal preserves
caller contents and a peer observes the exact contribution. Module-name storage
joins admission, and await/resume restores environments and module association
under nursery one/sixteen with at most 32 job turns.

Final bounded validation: **485 passed, zero failed, zero ignored** across 27
test executables. The eight new groups and the prior 477 ownership/semantic/
GC/debugger cases pass, including the existing synchronous no-backup contract.
Fixtures use tiny maps/vectors and scalar peer budgets with no represented
large-buffer allocation. The seven prior pressure/large-allocation exclusions
remain filtered; deep-recursion tail suites and network/service replays are
not run.

Formatting, `git diff --check` and offline workspace/all-target Clippy with
warnings denied pass. Logs use `/private/tmp/blueice-environment-admission-` with
`{environments,storage,maps,scopes,scope-functions,scope-loops,frames,module-entry,entry,heap-restoration,entry-semantics,bound-semantics,tail-semantics,resources,heap-resources,records,closure-exits,module-exits,generator-exits,generator-close,iterator-close,record-semantics,capacity,debugger,semantics,final-unit,waiter-controls,promise-controls,slot-controls,browser-controls,handoff-controls,call-controls,timer-controls,await-refusal-controls,continuation-controls,cleanup-controls,source-controls,public-controls,fmt,clippy}-20261016.log`.
Receipt: `target/environment-admission-source-20261016.json`.
Dynamic binding publication, other association copies, stack/completion
conversions, other container/transient/module-cache/service storage and
integrated acceptance remain open. No fresh full Test262, real Core/MCP replay
or coverage result is claimed; M1u remains the latest accepted milestone.

#### Function-eval binding publication follow-up (implementation checkpoint)

Acceptance: prepare fresh function-eval declarations, reused capture aliases
and deleted-variable recreation before changing cells, environment properties
or native maps. Include copied names, shadowed-cell buffers, both tables' actual
replacement capacity and temporary root slots. Preserve spare slots and no-op
aliases. Keep staged storage charged and object edges rooted across collection;
heap refusal must roll back environment publication and leave a recreated cell
deleted. Reconcile temporary ownership on all exits without clearing failure.

- [x] Reproduce two finite native refusals incorrectly succeeding before
      admission; both initial regressions pass after implementation.
- [x] Jointly prepare native declaration/alias records before cell creation or
      recreation, then publish the tables after all heap operations succeed.
- [x] Roll back partial environment publication and recreated values, including
      restoring a prior ordinary environment cell after a latched heap failure.
- [x] Pass eleven expanded exact-budget, spare-capacity, capture-alias, GC,
      heap-rollback, recreation and peer-refund groups.
- [x] Pass the related bounded selection, formatting and offline workspace/
      all-target Clippy; record the source/artifact audit with the checkpoint.
- [x] Complete global-binding publication in the realm-global checkpoint below.
- [ ] Complete other association copies,
      stack/completion conversion preallocation, wider native storage and
      integrated acceptance. M1u remains the latest accepted milestone.

Initial red log: `/private/tmp/blueice-dynamic-binding-red-20261016.log`
(zero passed, two failed). A two-variable eval batch and a deleted-variable
initializer both return success despite a full scalar scheduling account.
The eleven expanded groups pass in
`/private/tmp/blueice-dynamic-binding-expanded-20261016.log`.

Fixtures use tiny ordinary cells, short names/values and scalar peer budgets
without allocating represented large buffers. Nursery one/sixteen and major
collection preserve original/shadowed cells, new cells, parameter-environment
edges and an object-valued recreated binding. Heap rollback uses only two
ordinary cells and one short environment property of admitted headroom.

Final bounded validation: **496 passed, zero failed, zero ignored** across 27
test executables. The eleven new groups and the prior 485 ownership/semantic/
GC/debugger cases pass. The seven prior pressure/large-allocation exclusions
remain filtered; deep-recursion tail suites and network/service replays are
not run. Formatting, `git diff --check` and offline workspace/all-target Clippy
with warnings denied pass.

Logs use `/private/tmp/blueice-dynamic-binding-` with selection names from
`controls-20261016.log`; the runner is `controls-20261016.py`. The final new-group
log is `bindings-20261016.log`; `fmt-20261016.log` and `clippy-20261016.log` record
the static checks. Receipt: `target/dynamic-binding-source-20261016.json`.
Other association copies, stack/completion
conversions, wider native storage and integrated acceptance remain open.
No fresh full Test262, real Core/MCP replay or coverage result is claimed;
M1u remains the latest accepted milestone.

#### Realm-global binding ownership follow-up (implementation checkpoint)

Acceptance: count persistent global table/key capacity independently of script
frames. Prepare script/global-eval binding and slot-alias records and copied
initial values before declaration cells/properties; admit actual replacement
capacity while old tables coexist. Recreate a deleted global only after native
admission/root registration and undo tentative cell values on property failure.
Preserve accessor, descriptor, lexical-conflict and abrupt-script TDZ semantics.
Keep extracted lexical records charged/rooted through ShadowRealm reentry,
reserve restoration slots during inner publications and release replaced roots.
Reconcile eval/nested-script owners after the caller context is restored.

- [x] Reproduce uncharged persistent storage and script/eval/recreation effects
      before native refusal; all three initial finite regressions pass.
- [x] Prepare tables, names/aliases and initial property-value copies; count
      persistent capacity and reuse existing aliases/accessors without copies.
- [x] Move suspended lexical names/root ownership under a lease, reserve return
      slots and remove cleanup name-list copies; release replaced binding roots.
- [x] Reproduce stale eval ownership (729 versus 363 bytes) and reconcile after
      eval/nested-script context restoration and lease release.
- [x] Pass fourteen expanded GC/refusal/capacity/semantic groups and the final
      bounded selection, formatting and Clippy; record the source/artifact audit.
- [ ] Complete other association copies, stack/completion conversion
      preallocation, wider native storage and integrated acceptance.
      M1u remains the latest accepted milestone.

Initial logs: `/private/tmp/blueice-global-binding-red-20261016.log` (zero passed,
three failed) and `restoration-red-20261016.log` under the same prefix (zero
passed, one failed). The first shows a new 200-byte global record adding zero
native bytes. The latter observes the stale account immediately after eval
returns, before another admission or explicit snapshot refresh can hide it.

Fourteen expanded groups pass, including exact-budget/one-byte refusal with
growing/spare tables, no-copy aliases/accessors, initial-value refusal before
any cell, persistent name capacity, nested evaluation accounting and required
abrupt-script TDZ state. Nursery one/sixteen cover real ShadowRealm reentry;
native refusal after lexical extraction still restores without allocation or
clearing failure. Major collection proves overwritten and failed-recreation
cells are collectible after their final real owner releases them. Heap/root
refusals use small ordinary properties and registration counters. Staging
ownership updates by byte deltas instead of rescanning the batch for each row.

Final bounded validation: **545 passed, zero failed, zero ignored** across 33
test executables. This includes fourteen new groups, 34 public global-property/
ShadowRealm cases, one existing creation-boundary group and the prior 496
ownership/semantic/GC/debugger cases. Four existing frame/argument-release
checks now preserve the realm-global charge while still requiring temporary
control/payload owners to be zero. The seven prior pressure/large-allocation
exclusions remain filtered; deep-recursion tail suites and network/service
replays are not run. Formatting, `git diff --check` and offline workspace/
all-target Clippy with warnings denied pass.

Logs use `/private/tmp/blueice-global-binding-` with selection names listed in
`controls-20261016.log`; the runner is `controls-20261016.py`. Final new-group
and public semantic logs are `globals-20261016.log` and
`global-semantics-20261016.log`; `fmt-20261016.log` and `clippy-20261016.log`
record the static checks. Receipt: `target/global-binding-source-20261016.json`.
This is an implementation checkpoint. Other association copies, stack/completion
conversions, broader container/transient/module-cache/service storage and
integrated acceptance remain open. No fresh full Test262, real Core/MCP replay
or coverage result is claimed; M1u remains the latest accepted milestone.

#### Generator resume storage follow-up (implementation checkpoint, 2026-10-17)

Acceptance: admit the incoming frame's native contribution, caller/environment/
resource/pending roots, initial operand/result stack slots, typed completion
conversion storage and module-name copy before extracting heap state. Admit
minimum storage before allocation and reported capacity before extraction.
Refusal preserves saved state and caller contents. Keep Rust-held buffers
leased through field/record adoption, transfer completion values without a
second allocation, and preserve module-referrer semantics and terminal failure.

- [x] Reproduce two finite failures that consume the saved frame/pending return
      before native refusal; both initial regressions pass after implementation.
- [x] Prepare stack/completion/module storage and incoming native ownership
      before extraction; avoid allocating resume storage for Done/Running.
- [x] Keep detached buffers charged until active fields/typed records adopt
      them; append borrowed pending roots without an intermediate vector.
- [x] Pass ten expanded capacity/refusal/ownership/GC controls and related
      completion/finally semantics, formatting and Clippy; record the source/
      artifact audit with the checkpoint commit.
- [x] Complete outgoing stack/completion snapshot preallocation in the
      outgoing suspension checkpoint below (2026-10-17).
- [x] Complete generator parameter-entry root growth in the parameter-entry
      checkpoint below (2026-10-17).
- [ ] Complete other association copies,
      wider native storage and integrated acceptance.
      M1u remains the latest accepted milestone.

Initial red log: `/private/tmp/blueice-generator-resume-red-20261016.log`
(zero passed, two failed). Both calls return SchedulingLimit, but the original
resumable frame is already consumed. Initial fixed log:
`/private/tmp/blueice-generator-resume-development-20261016.log` (two passed).

Ten expanded groups pass. Exact-budget/one-byte controls cover growing/spare
stack slots and both referrer/caller module copies. Caller-root preparation
covers bindings, cells, arguments, environments, resources, tail operands and
saved completions. Detached completion/state buffers retain their native lease;
the restored generator completes its real return protocol unchanged. Nursery
one/sixteen and major collection retain pending returned objects, including an
async finally round trip with at most 64 job turns. Refusal preserves caller
contents/state and exactly refunds staged owners to a peer without clearing
failure. Done/Running preparation adds no resume allocation.

Final bounded validation: **568 passed, zero failed, zero ignored** across 35
test executables. The ten new groups, three completion protocol/stack-guard
checks and ten public finally cases join the previous 545 ownership/semantic/
GC/debugger cases. Existing dynamic-import cases verify generator and async-
generator module referrers across resumption. Fixtures use tiny frames/short
values and scalar peer budgets without represented large-buffer allocation.
The seven prior pressure/large-allocation exclusions remain filtered;
deep-recursion tail suites and network/service replays are not run.

Formatting, `git diff --check` and offline workspace/all-target Clippy with
warnings denied pass. Logs use `/private/tmp/blueice-generator-resume-` with
selection names listed in `controls-20261016.log`; the runner is
`controls-20261016.py`. Final new-group/completion/finally logs are
`{resume,completion,finally}-20261016.log`; static checks use
`{fmt,clippy}-20261016.log`. These artifacts started on October 16 and are
retained under their original names for the October 17 checkpoint.
Receipt: `target/generator-resume-source-20261016.json`.
Other association copies, wider native storage and integrated acceptance remain
open; outgoing snapshots and parameter-entry roots are recorded in the
follow-ups below.
No fresh full Test262, real Core/MCP replay or coverage result is claimed;
M1u remains the latest accepted milestone.

#### Outgoing suspension storage follow-up (implementation checkpoint, 2026-10-17)

Acceptance: jointly admit stack suffix and generator-completion conversion
buffers before allocation or frame mutation. Check actual capacities before
capture and retain native ownership, including detached value payloads, until
the enclosing generator state or suspended execution adopts them. Move whole
stacks without copying. Refusal must restore caller ownership and preserve
iterator cleanup without publishing an unusable continuation.

- [x] Reproduce two finite native-boundary failures: preparation succeeds at a
      full budget and detached buffers report zero rather than 104 native bytes.
- [x] Stage generator entry/yield, async yield/await and nested debugger stack
      captures; convert completion values into admitted storage without another
      allocation and count error-message capacity across heap adoption.
- [x] Return the caller prefix immediately to the active VM and retain the
      detached suffix/completion lease through the enclosing owner's adoption.
- [x] Reproduce a public debugger refusal retaining a nested continuation;
      discard the terminal root's saved descendants and release result roots.
- [x] Pass fourteen expanded finite capacity/ownership/refusal/GC groups.
- [x] Pass the bounded regression/static checks and record the source/artifact
      audit with the checkpoint commit.
- [x] Complete parameter-entry root growth in the following checkpoint.
- [ ] Complete other association copies, wider
      native storage and integrated acceptance. M1u remains the latest accepted
      milestone.

Initial native-boundary red log:
`/private/tmp/blueice-suspension-storage-red-20261017.log` (zero passed, two
failed). These probes exercise the extracted storage boundary before admission
was implemented; they do not claim a historical public end-to-end failure.
The separate public debugger red log is
`/private/tmp/blueice-suspension-storage-debugger-refusal-20261017.log`
(zero passed, one failed): the API returns SchedulingLimit but still retains a
nested frame when the parent's stack capture is refused.

The fourteen expanded groups cover joint exact-budget/one-byte admission,
empty suffix/completion storage, whole-stack pointer/capacity preservation,
detached native charges and peer refunds, full error capacity through native/
managed handoff, caller-prefix ownership and major-GC object liveness/release.
Valid generator/async/debugger entry points cover refusal cleanup, unpublished
continuations, frame serial behavior and latched failure. A separate terminal
cleanup check proves a saved result root no longer pins its object. Fixtures
use tiny frames, short values, 32 numeric operands and scalar peer-budget owners
without allocating any represented large buffer.

Final bounded validation: **582 passed, zero failed, zero ignored** across 35
test executables. The fourteen new groups join the previous 568 selected
ownership/semantic/GC/debugger cases. The seven prior pressure/large-allocation
exclusions remain filtered; deep-recursion tail suites and network/service
replays are not run. Formatting, `git diff --check` and offline workspace/
all-target Clippy with warnings denied pass.

Logs use `/private/tmp/blueice-suspension-storage-` with selection names listed
in `controls-run-20261017.log`; the fixed offline runner is
`controls-20261017.py`. Final new-group and static-check logs are
`{snapshot,fmt,clippy}-20261017.log`.
Receipt: `target/suspension-storage-source-20261017.json` records source/log/
executable hashes and staged/committed blob checks. No fresh full Test262, real
Core/MCP replay or coverage result is claimed. This remains an implementation
checkpoint; the wider M1z and integrated gates stay open.

#### Generator parameter-entry root follow-up (implementation checkpoint, 2026-10-17)

Acceptance: before creating a generator with parameter expressions, jointly
admit and reserve caller/inherited GC-root stack slots and optional parameter-
eval environment growth. Include outer receiver/generator roots and the
temporary environment-construction root. Check reported capacities before
creating the generator or replacing caller fields. Keep forwarded capacity in
its existing call-entry owner until active fields adopt it; refusal preserves
caller contents, releases temporary owners and keeps terminal failure latched.

- [x] Reproduce two small runtime-entry failures: root refusal follows an
      880-byte generator allocation, and parameter-environment refusal follows
      1,276 additional managed bytes. Prewarm lazy `globalThis` to isolate these
      effects. Both initial regressions now pass for ordinary/async generators.
- [x] Jointly admit minimum stack/environment capacity before reservation and
      actual capacity before generator creation; include the temporary root.
- [x] Extend the existing call-entry lease for forwarded environment capacity
      and release it only on active-field adoption.
- [x] Preserve the complete displaced caller root set, including current/outer
      dynamic and shadowed cells, without copying primitive buffers for GC.
- [x] Pass nine expanded capacity/ownership/refusal/collection/parameter groups.
- [x] Pass the fixed bounded regression/static checks and record the source/
      artifact audit with the checkpoint commit.
- [ ] Complete other association copies, broader native/container/module-cache/
      service storage and integrated acceptance. M1u remains the latest accepted
      milestone.

Initial red log: `/private/tmp/blueice-generator-entry-red-20261017.log`
(zero passed, two failed); initial fixed log:
`/private/tmp/blueice-generator-entry-development-20261017.log` (two passed).
Both runtime entry points already returned SchedulingLimit before this change,
but had allocated objects before refusing. The root-growth case uses 48 tiny
object-valued caller slots; the environment case uses a short default eval and
a spare operand stack. Scalar peer budgets represent no physical large buffer.

Nine expanded groups pass. Joint exact-budget/one-byte controls cover growing/
spare stack and forwarded environment capacity, primitive/object receivers and
optional parameter eval. Spare capacity copies no primitive environment roots.
Forwarded capacity has one charge across VM adoption. Twenty-one distinct tiny
objects verify all displaced edges survive major collection and become
collectible after the last owner releases them. Real captured-with/default-eval
parameters run for ordinary/async generators with nursery sizes one/sixteen;
only the existing explicit GC hook is installed, without agent helpers, and
async validation admits at most sixteen job turns. Parameter throws restore
the caller stack, bindings and environment; refusal preserves peer usage and
terminal failure.

Final bounded validation: **591 passed, zero failed, zero ignored** across 35
test executables. The nine new groups join the previous 582 selected ownership/
semantic/GC/debugger cases. The same seven pressure/large-allocation cases stay
filtered. Formatting, `git diff --check` and offline workspace/all-target Clippy
with warnings denied pass.

Logs use `/private/tmp/blueice-generator-entry-` with selection names listed in
`controls-run-20261017.log`; the fixed offline runner is
`controls-20261017.py`. Final new-group and static-check logs are
`{generator-entry,fmt,clippy}-20261017.log`.
Receipt: `target/generator-entry-source-20261017.json` records source/log/
executable hashes and staged/committed blob checks. No deep-recursion tail
suite, network/service replay, fresh full Test262, real Core/MCP replay or
coverage result is claimed; the wider M1z and integrated gates stay open.

#### Realm template-cache ownership follow-up (implementation checkpoint, 2026-10-17)

Acceptance: a realm observes the same tagged-template object for one compiled
site across active/suspended async, generator, debugger and module frames.
An older frame must see sites cached after it suspended. Count one realm table
independently of execution leases; do not copy or merge it on context changes.
Admit full staging and root capacity before a miss allocates arrays, preserve
prior entries on refusal and trace cached objects without permanent root pins.

- [x] Reproduce two real async identity failures and a cache miss succeeding
      at a full native budget (zero passed, three failed).
- [x] Keep one realm-owned table and native contribution; remove table copies,
      merges and ownership transfers from every suspended execution path.
- [x] Jointly admit minimum/actual detached table and operand-root capacity
      before heap effects; move existing entries only on successful publication.
- [x] Build final raw/cooked arrays directly with frozen properties, avoiding
      temporary value/key vectors; trace cache values without permanent roots.
- [x] Pass thirteen expanded capacity/refusal/identity/GC controls and replace
      the obsolete permanent-root refusal check with traced liveness/release.
- [x] Pass the fixed bounded regression/static checks and record the source/
      artifact audit with the checkpoint commit.
- [ ] Complete module-association/code copies, wider native/container/module-
      cache/service storage and integrated acceptance. M1u remains the latest
      accepted milestone.

A later finite audit found narrower staging gaps: the detached table used a
VM-local counter and operand growth admitted only its net delta. The
[template-control preparation follow-up](#template-control-preparation-follow-up-implementation-checkpoint-2026-10-17)
records their independent full-candidate correction. The 605-case result below
is historical evidence and does not prove that stronger boundary.

Initial red log: `/private/tmp/blueice-realm-template-red-20261017.log`
(zero passed, three failed). The public async function/generator cases suspend
before another job creates a template at the same site; resumption returns a
different object. The native cache-entry probe succeeds despite a full scalar
budget. These use short strings, one Promise gate and at most sixteen job turns.

Thirteen expanded groups pass. They cover ordinary/async-generator and imported
module resumption, rejection/repeated await, realm separation, cache ownership
through displaced frames, full/spare exact budgets and one-byte refusal, cache
hits/spare insertion at a full budget, string/heap failure cleanup and frozen
raw/cooked descriptors including invalid cooked escapes. Nursery one/sixteen
and major collection validate reachability. The updated permanent-root check
also proves cache removal releases the objects when no other owner remains.
No agent helper, listener, network/service replay or physical exhaustion probe
is used; scalar budgets allocate no represented large buffer.

Final bounded validation: **605 passed, zero failed, zero ignored** across 35
test executables. Thirteen new groups and the updated permanent-root boundary
join the prior 591 selected ownership/semantic/GC/debugger cases. The same seven
pressure/large-allocation cases remain filtered. Formatting, `git diff --check`
and offline workspace/all-target Clippy with warnings denied pass.

Logs use `/private/tmp/blueice-realm-template-` with selection names listed in
`controls-run-20261017.log`; the fixed offline runner is `controls-20261017.py`.
Final new-group/root-boundary and static logs are
`{template,root-ownership,fmt,clippy}-20261017.log`.
Receipt: `target/realm-template-source-20261017.json` records source/log/
executable hashes and staged/committed blob checks. No fresh full Test262,
real Core/MCP replay or coverage result is claimed; this remains an
implementation checkpoint with the wider M1z/integrated gates open.

#### Closure module-referrer ownership follow-up (implementation checkpoint, 2026-10-17)

Acceptance: count actual closure-association table/name capacity as document
native storage; records disappear when their closure is collected without
rooting the key. Admit complete creation staging before prototype/capture/closure
effects, preserve any spare publication slot across GC, and release staging on
unwind. Admit a call's independent referrer copy before instruction consumption,
call roots or caller-context replacement. Move each name to its owner and
restore the caller's original allocation on every return path.

- [x] Reproduce four finite accounting/GC/runtime-entry failures; all four pass.
- [x] Count table/name capacity, prune dead weak-key records at collection and
      return empty table storage after the last pending publication releases it.
- [x] Jointly admit fallible name/table reservation and actual reported capacity
      before closure creation; retain a scoped lease and GC publication guard.
- [x] Admit/lease call-name copies before caller effects and preserve the
      allocation-free absent-association path and associated empty-name behavior.
- [x] Pass twelve expanded capacity/refusal/lifetime/import-origin groups.
- [x] Pass the fixed bounded regression/static checks and record the source/
      artifact audit with the checkpoint commit.
- [ ] Complete other module-name/code/cache/service copies and integrated
      native/resource acceptance. M1u remains the latest accepted milestone.

Initial red log: `/private/tmp/blueice-module-referrer-red-20261017.log`
(zero passed, four failed). The table/name probe reports zero instead of 184
bytes; GC leaves a name after collecting its closure; a refused call consumes
an instruction; a full-budget Closure instruction still publishes its object.
Initial fixed log: `/private/tmp/blueice-module-referrer-development-20261017.log`
(four passed). Scalar peer owners allocate no represented large buffer.

Twelve expanded groups pass. They cover full/spare exact-budget and one-byte
refusal, independent copy leases through module installation, preserving a
prepared spare slot when nursery one/sixteen and major collection remove the
last old weak key, heap-failure unwind, absent/empty names, caller allocation
restoration after return/throw, and collection refund after terminal failure.
A surviving factory child imports from its original source after the caller's
loader context changes, using one in-memory module and at most sixteen job
turns. Fixtures use short names, tiny functions and fixed finite inputs.

Final bounded validation: **617 passed, zero failed, zero ignored** across 35
test executables. The twelve new groups join the prior 605 selected ownership/
semantic/GC/debugger cases. The same seven pressure/large-allocation cases remain
filtered. Formatting, `git diff --check` and offline workspace/all-target Clippy
with warnings denied pass.

Logs use `/private/tmp/blueice-module-referrer-` with selection names listed in
`controls-run-20261017.log`; the fixed offline runner is `controls-20261017.py`.
Final new-group and static logs are `{referrers,fmt,clippy}-20261017.log`.
Receipt: `target/module-referrer-source-20261017.json` records source/log/
executable hashes and staged/committed blob checks. No network/service replay,
fresh full Test262, real Core/MCP replay or coverage is claimed; the wider
M1z/native/integrated gates stay open.

#### Module context-name ownership follow-up (implementation checkpoint, 2026-10-17)

Acceptance: prepare module context-name storage before replacing caller fields
or entering its frame. Keep displaced caller names charged through callbacks,
return/throw/await and debugger ownership changes. A pause retains the original
ambient allocation in the VM; resume, step, nested throw and abort move it via a
scoped owner without cloning. ShadowRealm inheritance must refuse before Promise
capability creation and hand the admitted name to the child without double
counting. Every refusal restores or preserves its caller and refunds staging.

- [x] Reproduce three finite module/callback/debugger failures and a separate
      ShadowRealm capability-before-name-refusal failure; all now pass.
- [x] Admit minimum/actual fallible name storage before module frame entry and
      lease the caller allocation while code executes; reconcile all exits.
- [x] Remove the continuation's ambient-name backup; move one allocation through
      pause/resume/step/nested throw/abort and permit cleanup after failure.
- [x] Prepare inherited names after required argument checks and before Promise
      creation; transfer the buffer and reconcile parent/child contributions.
- [x] Pass eleven expanded ownership/refusal/budget/module/debugger/realm groups.
- [x] Pass the fixed bounded regression/static checks and record the source/
      artifact audit with the checkpoint commit.
- [x] Complete immutable raw host-source registry ownership in the following
      checkpoint; compiled realm-specific registries remain separate.
- [ ] Complete compiled-registry, continuation identifier/code/cell-map and other
      module-cache/service storage plus integrated acceptance. M1u remains the
      latest accepted milestone.

Initial red log: `/private/tmp/blueice-module-context-red-20261017.log`
(zero passed, three failed): instantiation refusal clears the caller operand,
a module callback reports zero native bytes for its 64-byte displaced caller
name, and pause substitutes a cloned ambient allocation. Initial fixed log:
`/private/tmp/blueice-module-context-development-20261017.log` (three passed).
The separate `shadow-red-20261017.log` under the same prefix reports zero passed,
one failed: refused inheritance has already increased Promise records from one
to two. New name preparation precedes that capability and later registry copies.

Eleven expanded groups pass. Exact-budget/one-byte controls validate new names;
full-budget moves preserve absent, empty and reserved-capacity ambient names.
Module return/throw/await and debugger pause/step/nested catch/terminal abort
restore original pointers/capacity and release temporary owners. Later frame
refusal refunds preparation before another account read can refresh it. One
in-memory ShadowRealm import preserves its source and callable export while
the shared ledger equals the sum of parent/child owners. Inputs are fixed tiny
modules, short names and scalar budgets without represented large buffers.

Final bounded validation: **628 passed, zero failed, zero ignored** across 35
test executables. The eleven new groups join the prior 617 selected ownership/
semantic/GC/debugger cases. The same seven pressure/large-allocation cases remain
filtered. Formatting, `git diff --check` and offline workspace/all-target Clippy
with warnings denied pass.

Logs use `/private/tmp/blueice-module-context-` with selection names listed in
`controls-run-20261017.log`; the fixed offline runner is `controls-20261017.py`.
Final new-group and static logs are `{contexts,fmt,clippy}-20261017.log`.
Receipt: `target/module-context-source-20261017.json` records source/log/
executable hashes and staged/committed blob checks. No network/service replay,
fresh full Test262, real Core/MCP replay or coverage is claimed; the wider
M1z/native/integrated gates stay open.

#### Immutable host module-source follow-up (implementation checkpoint, 2026-10-17)

Acceptance: raw JavaScript/JSON/text/bytes/source-phase tables retain one
immutable snapshot and one document charge for their actual table, key/data
capacities and owned header. Attached realms share those buffers; replacement
preserves earlier snapshots until their final owner releases them. Host refusal
retains its old snapshot and latches terminal failure. Bounded activation must
attach existing data once and cleanup must remain possible after failure.

- [x] Reproduce three finite uncharged-source, replacement and ShadowRealm-copy
      failures; all three initial probes pass after implementation.
- [x] Share immutable raw data with one document charge, preserving empty
      reserved allocations and isolating different document accounts.
- [x] Admit full replacement ownership before publication. Source-phase names
      deduplicate without another table, admit minimum/actual fallible set
      storage and charge input-vector coexistence until its strings move.
- [x] Attach previously installed data when bounded mode begins, preserve
      snapshots across parent replacement and release on final realm/owner drop.
- [x] Pass thirteen expanded ownership/refusal/activation/realm/typed-import
      groups and offline workspace/all-target Clippy.
- [x] Pass the fixed bounded regression/static checks and record the source/
      artifact audit with the checkpoint commit.
- [x] Complete shared derived synthetic values and outer conversion/binding
      handoffs in the synthetic export ownership follow-up below.
- [ ] Complete compiled-registry/code, transient parse/compile/conversion,
      continuation/cache/service storage and integrated acceptance. M1u remains
      the latest accepted milestone.

Initial red log: `/private/tmp/blueice-module-source-red-20261017.log`
(zero passed, three failed): source buffers add no document charge, full-budget
replacement discards the old table, and a ShadowRealm receives a different raw
source allocation. Initial fixed log:
`/private/tmp/blueice-module-source-development-20261017.log` (three passed).
The existing void host setters preserve their signatures; refusal keeps prior
data and is reported as document scheduling failure at execution/admission.

Thirteen expanded groups pass. Exact-budget/one-byte controls cover all five
source kinds and source-phase vector/set coexistence. Full-budget snapshot
sharing adds no charge; old data survives parent replacement and final drop
refunds it. Empty reserved tables, deduplication, late/repeated bounded activation,
terminal cleanup and cross-document rejection preserve their ledger invariants.
Real ShadowRealm imports share original raw buffers and discarded last realms
release them. One fixed import batch checks JSON/text/bytes/source-phase values
within at most 32 job turns. Fixtures use tiny tables, short strings/bytes and
scalar peer budgets without represented large-buffer allocations.

Final bounded validation: **641 passed, zero failed, zero ignored** across 35
test executables. The thirteen new groups join the prior 628 selected ownership/
semantic/GC/debugger cases. The same seven pressure/large-allocation cases remain
filtered. Formatting, `git diff --check` and offline workspace/all-target Clippy
with warnings denied pass.

Logs use `/private/tmp/blueice-module-source-` with selection names listed in
`controls-run-20261017.log`; the fixed offline runner is `controls-20261017.py`.
Final new-group and static logs are `{sources,fmt,clippy}-20261017.log`.
Receipt: `target/module-source-source-20261017.json` records source/log/
executable hashes and staged/committed blob checks. No network/service replay,
fresh full Test262, real Core/MCP replay or coverage is claimed; the wider
M1z/native/integrated gates stay open.

#### Synthetic module resource-refusal follow-up (implementation checkpoint, 2026-10-17)

Acceptance: a refused synthetic-value root registration returns the original
heap error without publishing a module; JSON conversion preserves host resource
abort types and configured string-limit identity, while malformed JSON remains
a module-resolution failure.

- [x] Reproduce finite root, instruction and string refusal cases with tiny
      in-memory sources and no network or resource-exhaustion workloads.
- [x] Propagate resource failures and pass eleven finite root lifetime,
      caller/graph preservation, cleanup, idempotency and primitive-value groups.
- [x] Finish the fixed bounded regression/static checks, including ordinary
      typed imports, and record the source/artifact evidence.
- [x] Update the checkpoint evidence and commit with a detailed English message.
- [ ] Complete compiled/transient storage and wider integrated acceptance.
      M1u remains the latest accepted milestone.

Initial red log: `/private/tmp/blueice-synthetic-boundary-red-20261017.log`
(zero passed, three failed): refused root registration panics, and JSON
conversion rewrites instruction/string failures into module-resolution errors.
The implementation now propagates root errors before record publication and
wraps only malformed JSON's SyntaxError. The eleven expanded groups pass in
`/private/tmp/blueice-synthetic-boundary-development-20261017.log`. Refusal
preserves the caller's existing modules, roots and operand prefix. Fresh-graph
preparation releases earlier synthesis roots, while failed extension preserves
an existing graph's roots/cells. Object exports survive major GC until root
release and then collect; primitive exports and repeated synthesis need no new
object roots. Graph instruction refusal precedes module evaluation. Inputs are
tiny in-memory sources and deterministic scalar limits, without physical
resource-exhaustion or network/service workloads.

Final bounded validation: **652 passed, zero failed, zero ignored** across 35
test executables and 53 fixed selections. The eleven new groups join the prior
641 selected ownership/semantic/GC/debugger cases. The same seven pressure/
large-allocation cases remain filtered. Formatting, `git diff --check` and
offline workspace/all-target Clippy with warnings denied pass. Clippy's initial
test-helper initialization warning was fixed before the final regression run.

Logs use `/private/tmp/blueice-synthetic-boundary-`; selection names are listed
in `controls-run-20261017.log` and the fixed offline runner is
`controls-20261017.py`. Final new-group and static logs are
`{synthetic,fmt,clippy}-20261017.log`. Receipt:
`target/synthetic-boundary-source-20261017.json` records source/log/executable
hashes and staged/committed blob checks. No fresh full Test262, real Core/MCP
replay, coverage or integrated acceptance is claimed. Transient conversion,
compiled registries/caches and the wider native/integrated gates stay open.

#### Synthetic export ownership follow-up (implementation checkpoint, 2026-10-17)

Acceptance: synthetic exports share immutable derived values across Bytecode
snapshots, with one document charge following their final owner. Admit outer
text/JSON-input/bytes conversion and binding-copy buffers, attach preloaded
values on bounded activation and reject adoption into a different document
without replacing existing data.
Realm-local object exports retain their existing GC roots and rebuild policy.

- [x] Reproduce finite retained-charge, full-budget conversion and snapshot-copy
      failures with short text modules.
- [x] Implement shared export ownership and bounded conversion/handoff leases;
      validate activation, refusal, realm sharing, GC and final-drop cleanup.
- [x] Run the fixed offline regression/static checks, update evidence and commit
      with a detailed English message.
- [x] Replace JSON module coercion/parser/source-map staging with the admitted
      direct-source AST checkpoint below.
- [ ] Complete compiled tables/keys/code, general JSON runtime accounting,
      other continuation/cache/service buffers and integrated
      acceptance. M1u remains the latest accepted milestone.

Initial red log: `/private/tmp/blueice-synthetic-value-red-20261017.log`
(zero passed, three failed): a retained UTF-16 export adds no native bytes,
full-budget synthesis succeeds, and cloned Bytecode owns a different export
buffer. Initial fixed log: `development-20261017.log` under the same prefix
(three passed). Fifteen expanded groups pass in `expanded-20261017.log`.

One immutable derived-value owner charges its header and actual primitive
capacity once. Bytecode snapshots and attached realms share its buffers; final
drop refunds the charge even after terminal failure. Activation, host setters,
borrowed graphs and binding installation attach existing owners once. A foreign
document cannot adopt them or replace the host's previous registry/referrer.
Object exports retain realm-local GC roots and are filtered before inheritance.

Text conversion uses admitted fallible UTF-16 storage. JSON reserves its derived
output independently of its scoped UTF-16 input. Bytes copies and independent
binding copies retain their leases until release inside managed heap adoption;
the temporary bytes-buffer operand root is admitted before prototype/buffer
allocation. Raw JSON/text/bytes snapshots are borrowed, and dynamic JavaScript
compilation no longer clones its raw UTF-8 input. Inner parser/conversion and
compiled storage remain open rather than being covered by these outer leases.

Exact-budget/one-byte controls cover Unicode/empty text, scoped inputs and
binding copies. Refused root storage produces no managed bytes allocation;
refused heap adoption refunds its copy. Preloaded activation, cross-document
rejection, root refusal and malformed JSON release temporary owners. A real
ShadowRealm shares the text buffer, omits the parent's object export, and
releases its final derived owner when discarded. One real JSON/text/bytes import
batch finishes within at most 32 job turns and retains values through major GC.
Inputs are tiny local modules and scalar peer budgets with no represented
large-buffer allocation or physical resource-exhaustion workload.

Final bounded validation: **667 passed, zero failed, zero ignored** across 35
test executables and 54 fixed selections. The fifteen new groups join the prior
652 selected ownership/semantic/GC/debugger cases. The same seven pressure/
large-allocation cases remain filtered. Formatting, `git diff --check` and
offline workspace/all-target Clippy with warnings denied pass.

Logs use `/private/tmp/blueice-synthetic-value-`; selection names are listed in
`controls-run-20261017.log` and the fixed offline runner is
`controls-20261017.py`. Final new-group and static logs are
`{values,fmt,clippy}-20261017.log`. Receipt:
`target/synthetic-value-source-20261017.json` records source/log/executable
hashes and staged/committed blob checks. No network/service replay, fresh full
Test262, real Core/MCP replay, coverage or integrated acceptance is claimed.

#### JSON module parser ownership follow-up (implementation checkpoint, 2026-10-17)

Acceptance: parse module JSON directly from its immutable UTF-8 source, retain
an admitted AST until conversion ends, and omit unused reviver source storage.
Admit vector/string growth, conversion roots and heap handoffs before effects;
preserve complete syntax validation before any managed value construction.

- [x] Reproduce tiny scalar/string over-reservation and unadmitted container
      storage cases using finite scalar budgets.
- [x] Implement parser/AST and conversion ownership, bounded nesting, cleanup
      and direct export adoption without a duplicate payload reservation.
- [x] Validate grammar, values, GC, refusal and general JSON compatibility;
      run fixed offline regression/static checks, update evidence and commit.
- [ ] Complete compiled storage, general JSON runtime accounting, other native
      buffers and integrated acceptance. M1u remains the latest accepted
      milestone.

Initial red log: `/private/tmp/blueice-json-module-red-20261017.log`
(zero passed, three failed). Initial fixed log: `development-20261017.log`
under the same prefix (three passed). Sixteen expanded groups pass in
`expanded-20261017.log`. A number now needs only its export header, and a
one-code-unit string admits its AST/result copies with four payload bytes.
Array/object AST storage now refuses before managed construction when only the
former outer-conversion margin remains.

The shared grammar reader leases actual module AST vector/key/value capacities.
Fallible growth admits complete old/new allocation coexistence, then checks
actual capacity before publication. Module mode borrows UTF-8 directly and
omits reviver source copies/maps. Ordinary JSON revivers retain source context.
Full grammar validation precedes managed construction. Bounded module nesting
caps at 128; a lowered limit of two tests refusal with seven input characters.
Standalone parsing retains its independent policy; no deep-recursion workload
is run.

Conversion admits maximum ancestor/child root slots, preserves the operand
prefix and leases primitive/key copies until heap adoption. Index keys use
fixed stack digits. A returned primitive transfers its existing charge into
the prepared export header without double reservation; a foreign-document
charge cannot transfer. Syntax, partial parsing, native refusal, instruction,
string and root-registration failures release temporary owners. The earlier
synthetic-value fixture now checks AST/result coexistence and key copies after
removal of the obsolete UTF-16 input helper.

Exact/one-byte controls cover UTF-16 copies, root storage and AST replacement
growth. Duplicate key order, numeric forms, Unicode and escaped lone surrogates
keep their values. One-object nursery and major collection preserve nested
values. Repeated real imports retain object identity and ordinary `__proto__`
data properties within at most 32 job turns. Inputs and graphs are fixed and
tiny; scalar peers represent budget occupancy without large buffers or
physical resource-exhaustion probes.

Final bounded validation: **694 passed, zero failed, zero ignored** across 38
test executables and 62 fixed selections. Sixteen new ownership cases and
eleven named ordinary JSON grammar/reviver/raw/string/property/cycle cases join
the prior 667 checks. The same seven earlier pressure/large-allocation cases
remain filtered; JSON resource sweeps are not selected. Formatting,
`git diff --check` and offline workspace/all-target Clippy with warnings denied
pass.

Logs use `/private/tmp/blueice-json-module-`; selection names are listed in
`controls-run-20261017.log` and the fixed offline runner is
`controls-20261017.py`. Final new-group and static logs are
`{json-modules,fmt,clippy}-20261017.log`. Receipt:
`target/json-module-source-20261017.json` records source/log/executable hashes
and staged/committed blob checks. No network/service replay, fresh full Test262,
real Core/MCP replay, coverage or integrated acceptance is claimed.

#### Compiled module registry ownership follow-up (implementation checkpoint, 2026-10-17)

Acceptance: installed compiled registries and immutable snapshots retain admitted
table/key/root-code buffers; staged copies and growth remain charged until
publication or refusal. Preserve previous host registry/referrer on rejection,
share unchanged snapshots and keep realm-local synthetic objects isolated.

- [x] Reproduce tiny uncharged-install, replacement and deep-copy failures.
- [x] Implement checked snapshots, staging and publication; validate activation,
      growth, refusal, realm inheritance, GC and final-drop cleanup.
- [x] Run finite offline regression/static checks, update evidence and commit
      with a detailed English message.
- [ ] Complete shared child/source/diagnostic program storage, compiler/producer
      scratch and other graph/cache/service owners plus integrated acceptance.
      M1u remains the latest accepted milestone.

Initial red log: `/private/tmp/blueice-module-registry-red-20261017.log`
(zero passed, three failed). Initial fixed log: `development-20261017.log`
under the same prefix (three passed). Immutable snapshots charge their header,
logical table/key capacities and root code buffers under the existing Value
payload model, including diagnostic instruction-owner vectors. References add
no deep copy or charge; final drop releases storage, including empty reserved
tables and terminal cleanup.

Two additional activation reds are in `activation-red-20261017.log` under the
same prefix (zero passed, two failed): activation returns success for a scalar
owner already beyond the limit, and missing referrer bytes permit a peer to
exceed the document's actual remaining room. Enable execution accounting before
initial measurement and retain the scope on failure. Initial/activation groups
now pass together in `activation-development-20261017.log` (five passed).

The expanded red log `expanded-red-20261017.log` reports seventeen passed and
two failed: cloning reserves unused source capacity, and refused ShadowRealm
filtering leaves a nine-byte referrer contribution in the peer ledger. Clone
planning now uses new vector/string lengths, validates actual capacities, and
publishes the name-lease refund on early failure. All nineteen expanded groups
pass in `expanded-20261017.log` and the final fixed regression.

Host installation admits the incoming registry and referrer while old owners
coexist, retaining the prior state on refusal. Borrowed host graphs admit their
copy before graph mutation. Internal reads share snapshots; only the first
draft insertion clones root buffers. Table growth admits old/new coexistence,
moves existing entries and merges previously admitted ownership. Refused growth
preserves prior data/capacity. Reserved source vectors do not inflate a clone's
new-buffer requirement. Private snapshots cannot attach to another document.

Preparation failures release newly synthesized roots. A linking failure before
evaluation restores the old snapshot and root prefix so no unrooted synthetic
object remains cached. Unchanged ShadowRealm code shares one owner; object
exports require a checked filtered copy before capability creation. Parent
records remain intact and discarded final realms refund their code storage.

Synthetic record construction is separate from registry retention. Component
fixtures keep their returned records in host-owned maps, while public graph and
registry fixtures validate installed metadata admission. Three prior aggregate
ledger assertions now include the still-live compiled snapshot; the parent/child
case also asserts a shared buffer pointer and counts that storage once. Walk,
name and conversion leases still release independently of retained caches.

Final bounded validation: **713 passed, zero failed, zero ignored** across 38
test executables and 63 fixed selections. Nineteen new groups join the previous
694 ownership/semantic/GC/JSON/debugger checks. The same seven prior pressure/
large-allocation cases remain filtered and JSON resource sweeps are not selected.
Formatting, `git diff --check` and offline workspace/all-target Clippy with
warnings denied pass. Inputs are tiny modules and scalar owners without
represented large buffers or physical resource-exhaustion workloads.

Logs use `/private/tmp/blueice-module-registry-`; selection names are listed in
`controls-run-20261017.log` and the fixed offline runner is
`controls-20261017.py`. Final new-group and static logs are
`{registries,fmt,clippy}-20261017.log`. Receipt:
`target/module-registry-source-20261017.json` records source/log/executable hashes
and staged/committed blob checks. No network/service replay, fresh full Test262,
real Core/MCP replay, coverage or integrated acceptance is claimed. Shared
program buffers and wider native storage remain open, beyond this root owner.

#### Module continuation ownership follow-up (implementation checkpoint, 2026-10-17)

Acceptance: await/debugger module frames retain their immutable executable
snapshot without another root Bytecode copy. Identifier copies must be admitted
before frame capture and remain owned across suspension, resume and rejection.
Replacing the active loader must not drop a pending frame's code owner.

- [x] Reproduce tiny await/debugger code-copy and backing-owner release cases:
      the initial offline run failed all three assertions; after the code-view
      change, the same three cases passed (logs under
      `/private/tmp/blueice-module-continuation-{red,first}-20261017.log`).
- [x] Implement snapshot-backed module code and staged identifier ownership;
      thirteen finite groups verify suspension, repeated await, debugger
      transition, GC, loader replacement, late activation, rejection and failure.
- [x] Run fixed offline regression/static checks, update evidence and commit
      with a detailed English message. Shared child/source program storage,
      linked cell maps and wider native/integrated gates remain open.

Implementation: module evaluation/parent settlement receives a typed immutable
registry. Saved code retains that snapshot with a stable map ordinal, so clone
and suspension require no additional root Bytecode buffers. The entire snapshot
stays charged through loader replacement and drops with its last view; finer
per-code-unit reclamation remains separate work. Debugger completion uses its
saved registry for namespace resolution, including after the active cache clears.

Module identifier copies are admitted before allocation and frame capture;
capacity leases follow saved frames and Rust-held resume owners. Repeated await
prepares its replacement identifier before moving execution state. Records count
only unleased storage, including preloaded debugger control/iterator/handler/name
fields on bounded activation. Activation also attaches code snapshots held only
by pending frames, and resumption leases preloaded identifiers. Initial name
refusal restores the caller module and active cells before unwinding. Await
registration refusal consumes no serial and releases its rejected owners.

Thirteen finite groups cover shared/distinct code selection at a full scalar
budget; await/debugger buffer identity; old snapshot retention; exact identifier
admission and one-byte refusal; two awaits with tiny GC and callback ownership;
execution after loader replacement with a peer-ledger refund observation;
debugger step-to-await; late await/debugger activation; rejection; registration
refusal; and terminal debugger cleanup. The fixtures also have the existing
ordinary-library coverage entry point; no new coverage measurement is claimed.

Validation: **726 passed, zero failed/ignored**, across 38 executables and 64
fixed selections. The final runner repeats the prior 713 cases plus the thirteen
new groups after the Clippy-only fixture initialization cleanup. Formatting,
diff checks and offline workspace/all-target Clippy with warnings denied pass.
The same seven pressure cases remain excluded; JSON resource sweeps are not
selected. Cargo uses the existing workspace target with incremental compilation
and dev/test debug artifacts disabled. No network/service replay, fresh full
Test262, real Core/MCP replay, coverage or integrated acceptance is claimed.

Evidence: `/private/tmp/blueice-module-continuation-controls-20261017.py`,
`/private/tmp/blueice-module-continuation-commit-20261017.txt`,
`/private/tmp/blueice-module-continuation-audit-20261017.py`, and logs matching
`/private/tmp/blueice-module-continuation-{continuations,fmt,clippy}-20261017.log`.
`target/module-continuation-source-20261017.json` records source/log/executable
hashes and staged/committed blob checks. This closes this bounded implementation
follow-up; wider M1z/native/integrated work remains open and M1u remains the latest
accepted milestone.

#### Linked module cell ownership follow-up (implementation checkpoint, 2026-10-17)

Acceptance: linked cell tables retain admitted capacity even while their graph
is held in Rust or parked for reentrant evaluation. Module entry and completion
transfer that capacity without charging an allocation twice. Await/debugger
copies must be admitted before moving execution state; bounded activation must
attach preloaded linked tables. Live bindings and GC roots must remain valid.

- [x] Reproduce completed/pending table undercounting and late admission:
      all three initial cases failed, then passed after admitted cell storage
      and frame/graph transfers were implemented. Persistent graph cells use a
      dedicated counter rather than the transient frame counter.
- [x] Implement admitted linking/copies, frame ownership transfers and late
      activation; verify finite live-binding, GC, debugger and refusal cases.
- [x] Run fixed offline regressions/static checks, update evidence and commit
      with a detailed English message. Shared programs, wider graph metadata,
      compiler/native buffers and integrated acceptance remain open.

Implementation: each new record reserves its root-scope cell table before any
managed cells or import aliases are installed. Preparation admits a minimum
slot count, reserves fallibly, then checks actual logical table capacity. A
dedicated realm counter keeps prepared/retained leases visible when graphs are
installed, parked for reentrant evaluation or held in Rust locals. Persistent
tables do not remain on the transient displaced-frame counter.

Declaration initialization and evaluation transfer admitted table capacity into
the active VM and back on completion/error. Await/debugger copies are prepared
while active cells still root their entries, before execution-state capture;
copy refusal follows the existing engine-error cleanup path without publishing
a continuation serial. Initial linking failure drops new table owners and
preserves old records/root prefixes; a failed fresh graph releases its owners
before cleanup accounting is published. Bounded activation attaches preloaded
tables before measuring existing fields and retains a terminal account on
refusal. Empty table moves require no extra reservation.

Thirteen finite groups cover completed/pending records, early managed-allocation
refusal, exact table capacity and one-byte refusal, active ownership transfer,
Rust-held graph accounting, imported live bindings across two awaits and tiny
GC, late activation/refusal, linking rollback, peer-visible realm drop/refund,
pre-capture refusal and debugger steps. The fixtures expose the existing
ordinary-library coverage entry point; no new coverage result is claimed.

Validation: **739 passed, zero failed/ignored**, across 38 executables and 65
fixed selections: thirteen new groups and all 726 prior selected controls.
Formatting, diff checks and offline workspace/all-target Clippy with warnings
denied pass. Cargo reuses the workspace target with incremental compilation and
dev/test debug artifact growth disabled. The same seven pressure cases remain
excluded and JSON resource sweeps are not selected. No network/service replay,
fresh full Test262, real Core/MCP replay, coverage or integrated acceptance is
claimed. Shared programs and surrounding graph/native storage remain open.

Evidence: `/private/tmp/blueice-module-cells-controls-20261017.py`,
`/private/tmp/blueice-module-cells-commit-20261017.txt`,
`/private/tmp/blueice-module-cells-audit-20261017.py`, and logs matching
`/private/tmp/blueice-module-cells-{red,first,cells,fmt,clippy}-20261017.log`.
`target/module-cells-source-20261017.json` records source/log/executable hashes
and staged/committed blob checks. This is a bounded implementation checkpoint;
wider M1z/native/integrated acceptance remains open.

#### Module record-table ownership follow-up (implementation checkpoint, 2026-10-17)

Acceptance: graph record-table slots and canonical-name buffers retain native
ownership while installed, parked or Rust-held. Table growth admits a complete
replacement while the old table coexists; refusal preserves existing records.
Linking name selection borrows immutable registry keys and admits its vector
before allocation. Bounded activation, rollback and final drop preserve quotas.

- [x] Reproduce retained/Rust-held metadata undercounting and late admission:
      the initial three cases failed; the same cases now pass. One tiny graph
      previously omitted 725 bytes of logical table/name storage.
- [x] Implement owned record tables, checked growth/key copies, borrowed name
      selection and lifecycle/refusal controls.
- [x] Run fixed offline regressions/static checks, update evidence and commit
      with a detailed English message. Graph root-list merging, retained record
      values, shared programs and wider native/integrated gates remain open.

Implementation: `ModuleRecords` retains a document charge for logical table
capacity and canonical key capacities. That owner moves with the graph through
installed, parked and Rust-held states. Growth first admits a full detached
replacement while old slots/names remain live, then moves entries without
copying keys or records. Publication merges existing charges and refunds the
freed old table. Refused planning preserves records, capacity and pointers.

New key copies are admitted before allocation; existing spare slots require no
new table reservation. Name removal refunds only released buffers, including
after terminal failure; empty reserved tables keep their charge until drop.
Key capacity is cached so batch rollback does not repeatedly scan all keys.
Host-neutral graph metadata attaches once on bounded activation and rejects a
foreign owner or mutation without its owning document scope.

Linking selects sorted new names directly from immutable registry keys with one
leased borrowed-name vector. It admits minimum slots before allocation, checks
reported capacity and keeps the lease across module JavaScript. Preparation refusal
restores the old graph and new-root prefix; vector ownership drops before final
graph cleanup and Promise draining. Canonical key copies enter the owned table
before managed cell linking begins. Rollback removes only newly inserted names;
retained actual table capacity remains charged.

Thirteen finite groups cover native/Rust-held retention and refund, admission
before managed cells, old/new table peaks, empty-key spare-slot reuse at full
scalar quota, name-copy refusal, terminal cleanup, borrowed sorted-name identity
and slot ownership/refusal, late activation, foreign/missing scope refusal,
existing-graph rollback and fresh-graph failure with peer-ledger observations.
The fixtures expose the ordinary-library coverage entry point; no new coverage
measurement is claimed.

Four older aggregate-ledger fixtures now include each graph's additional native
owner. Their identity, root, lifetime and refund assertions remain in place;
the pending-frame executable check also subtracts record metadata to isolate
its retained code owner. Initial total-only differences remain in diagnostic
`{cells,continuations,contexts,final-unit}-aggregate-red` logs.

Validation: **752 passed, zero failed/ignored**, across 38 executables and 66
fixed selections: thirteen new groups and all 739 prior selected controls.
Formatting, diff checks and offline workspace/all-target Clippy with warnings
denied pass. Cargo reuses the workspace target with incremental compilation and
dev/test debug artifact growth disabled. The same seven pressure cases remain
excluded and JSON resource sweeps are not selected. No network/service replay,
fresh full Test262, real Core/MCP replay, coverage measurement or integrated
acceptance is claimed. Root-list merging, retained record value payloads,
shared programs and other native/integrated gates remain open.

Evidence: `/private/tmp/blueice-module-graph-controls-20261017.py`,
`/private/tmp/blueice-module-graph-commit-20261017.txt`,
`/private/tmp/blueice-module-graph-audit-20261017.py`, and logs matching
`/private/tmp/blueice-module-graph-{red,first,graphs,fmt,clippy}-20261017.log`.
`target/module-graph-source-20261017.json` records source/log/executable hashes
and staged/committed blob checks. This is an M1z implementation checkpoint;
wider native/integrated acceptance remains open.

#### Module root storage follow-up (implementation checkpoint, 2026-10-17)

Acceptance: linking/namespace/synthetic root storage admits native capacity
before managed construction; existing completion/error values acquire admitted
roots before cache publication. Segments retain ownership while stored or
Rust-held and merge during terminal cleanup without allocation. Rollback releases
only new roots/storage. Registration failures preserve runtime error identity;
cached module completion objects remain alive across later GC.

- [x] Reproduce root-slot undercounting, late admission, registration failure
      and cached completion lifetime cases with finite offline probes. All four
      initial cases failed, then passed after admitted storage and persistent
      cached-completion roots replaced the missing owners.
- [x] Implement admitted root segments/reservations, allocation-free merging,
      rollback, late activation and completion/error ownership.
- [x] Run fixed offline regression/static checks, update evidence and commit
      with a detailed English message. Retained value payloads, shared programs
      and other native/integrated gates remain open.

Implementation: graph, nested-load and temporary namespace/synthetic root lists
own native segment headers and actual root-slot capacity. A root reservation
admits that storage before managed construction/registration and commits without
allocation. Refusal or registration failure drops only its staged segment;
rollback pops the new suffix and frees emptied segments. Existing slots require
no extra capacity reservation. JSON primitive modules reserve no fictitious
object root; JSON object/array and bytes modules reserve before construction.

Stored/parked/Rust-held lists retain their owners. Cleanup splices already-owned
segments without allocation or admission, including after terminal failure;
iteration and destruction are iterative. Cached document identity avoids
rescanning attached segments during ordinary appends. Bounded activation
attaches preloaded segments once; foreign attachment and mutation without the
owning scope fail. The fixture checkpoints copy RootIds rather than cloning
storage ownership.

Linking and namespace roots now propagate registration failures instead of
using the infallible fresh-object helper. Persistent normal/debugger/await
completion roots precede cache publication. This fixes the reproduced object
completion collected by a later script. A separate failing await-registration
case reproduced completion publication before rooting; it now roots first and
leaves the completion cache empty on failure. Error-value publication follows
the same ordering. ImportMeta root registration is fallible as well.

Fourteen finite groups cover native retention, exact slot budgeting/refusal,
spare slots at full scalar quota, terminal merge with stable root addresses,
suffix rollback across segments, staged-registration failure, late activation,
primitive JSON, synthetic managed-allocation preemption, normal/debugger/await
completion GC and failed-await cache publication. Ordinary-library coverage
entry points expose the same fixture cases; no new coverage result is claimed.

Earlier aggregate-ledger assertions now include graph and Rust-held root segment
owners. The JSON exact-budget case includes the coexisting root reservation;
debugger registration thresholds include the new persistent record root. Existing
identity, prefix, cleanup and failure-boundary checks remain in place. The first
inherited baseline results are preserved in `*-baseline-red-20261017.log` files.

Validation: **766 passed, zero failed/ignored**, across 38 executables and 67
fixed selections: fourteen new groups and all 752 prior selected controls.
Formatting, diff checks and offline workspace/all-target Clippy with warnings
denied pass. Cargo reuses the workspace target with incremental compilation and
dev/test debug artifact growth disabled. The same seven pressure cases remain
excluded and JSON resource sweeps are not selected. No network/service replay,
fresh full Test262, real Core/MCP replay, coverage result or integrated acceptance
is claimed. Displaced-frame root scratch, retained value payloads, shared
programs and wider native/integrated gates remain open.

Evidence: `/private/tmp/blueice-module-roots-controls-20261017.py`,
`/private/tmp/blueice-module-roots-commit-20261017.txt`,
`/private/tmp/blueice-module-roots-audit-20261017.py`, and logs matching
`/private/tmp/blueice-module-roots-{red,first,completion-red,roots,fmt,clippy}-20261017.log`.
`target/module-roots-source-20261017.json` records source/log/executable hashes
and staged/committed blob checks. This is an M1z implementation checkpoint;
wider native/integrated acceptance remains open.

#### Displaced execution rooting follow-up (implementation checkpoint, 2026-10-17)

Acceptance: registered roots for Rust-held frames use admitted storage and
release partial work on refusal. Queued-job, deferred-namespace and reentrant
realm entry restore every displaced owner before returning a rooting error.
Reference enumeration borrows saved fields without a scratch object vector;
empty checkpoints require no rooting work. Callback execution preserves GC edges.
Fatal host errors retain their identity across the public ShadowRealm boundary.

- [x] Reproduce partial rooting, native admission, empty checkpoints and
      reentrant restoration failures with tiny offline cases. All four initial
      cases failed; a separate public evaluate case reproduced fatal-error
      masking as a catchable TypeError.
- [x] Implement borrowed reference enumeration, admitted registered roots and
      transactional frame/realm setup; fourteen new cases pass across success,
      refusal and collection lifecycles.
- [x] Run fixed offline regressions/static checks, update evidence and commit
      with a detailed English message. Trace-vector growth admission, retained
      value payloads, shared programs and wider native/integrated gates stay open.

Saved-edge enumeration now chains borrowed frame fields directly instead of
building an intermediate ObjectId vector. The shared pending-completion iterator
covers completion-save values and every value-carrying completion kind; bindings,
call metadata, disposables, linked cells, dynamic binding chains and home objects
remain visible. Scoped-root consumers use the same iterator without collecting a
scratch vector.

Temporary persistent registrations reuse admitted ModuleRoots segments. Each
append records its prefix length; refusal unregisters only its suffix and frees
emptied segments. Existing prefix registrations and capacity ownership survive.
This replaces the unadmitted RootId vector and removes the unused infallible
fresh-object rooting helper. Root-registration exhaustion is a runtime error.

Queued-job entry checks terminal scheduling state, then returns immediately for
an empty queue. For real jobs, rooting completes before a job is removed or fuel
and call depth change. Synchronous deferred namespace entry also restores its
observer frame on rooting failure. Success restores the original execution before
unregistering roots; GC inside nested JavaScript preserves saved objects and
subsequent collection reclaims them after their frame owners are cleared.

Reentrant ShadowRealm evaluation stages common frame, parameter environment and
kept weak-target roots before taking other caller fields. Failure restores both
execution and lexical global bindings and releases every temporary registration.
Both public evaluate paths propagate noncatchable host aborts while ordinary
JavaScript exceptions retain the opaque TypeError membrane.

Fourteen finite cases cover exact native root ownership, saved-edge GC, partial
append rollback, queued-job refusal/retention, empty/scalar no-capacity paths,
deferred observer failure/success, common/parameter/weak-target reentrant failures,
successful reentrant GC and public fatal-error propagation. Ordinary-library
coverage entry points call the same cases; no new coverage result is claimed.

Validation: **780 passed, zero failed/ignored**, across 38 executables and 68
fixed selections: fourteen new cases and all 766 prior selected controls.
Formatting, diff checks and offline workspace/all-target Clippy with warnings
denied pass. Cargo reuses the workspace target with incremental compilation and
dev/test debug artifact growth disabled. The same seven pressure cases remain
excluded and JSON resource sweeps are not selected. No network/service replay,
fresh full Test262, real Core/MCP replay, coverage result or integrated acceptance
is claimed. General trace-vector growth admission, retained value payloads,
shared programs and wider native/integrated gates remain open.

Evidence: `/private/tmp/blueice-displaced-roots-controls-20261017.py`,
`/private/tmp/blueice-displaced-roots-commit-20261017.txt`,
`/private/tmp/blueice-displaced-roots-audit-20261017.py`, and logs matching
`/private/tmp/blueice-displaced-roots-{red,boundary-red,focused,displaced,fmt,clippy}-20261017.log`.
`target/displaced-roots-source-20261017.json` records source/log/executable hashes
and staged/committed blob checks. This is an M1z implementation checkpoint;
wider native/integrated acceptance remains open.

#### Temporary execution root capacity follow-up (implementation checkpoint, 2026-10-17)

Acceptance: every append to the temporary execution-root vector admits native
capacity before reserve or publication. Detached growth budgets old and new
allocations while they coexist; refusal preserves existing contents and capacity.
Promise jobs keep their queued owner and original fuel if preparation fails.
Module/async/event entry restores any displaced frame on admission failure;
nested GC sees the same roots. Cleanup can truncate after terminal failure.

- [x] Reproduce pre-admission growth in native-pressure GC, callback completion,
      queued Promise jobs and active iterator roots with tiny offline cases.
      All four initial cases failed and now pass.
- [x] Route temporary execution-root growth through admitted preparation and
      verify frame restoration, queue retention, capacity ownership and GC.
- [x] Run fixed offline controls/static checks, update evidence and commit with
      a detailed English message. Other GC scratch vectors, retained native
      values, shared programs and wider native/integrated gates remain open.

Root-slot preparation admits the full detached replacement while the old vector
remains owned. Reported capacity receives a temporary execution-control lease
and a second budget check before moving any prior roots. Refusal leaves their
contents, pointer and capacity intact. Commit moves handles, drops old storage
and publishes the new retained capacity once. Existing slots and empty appends
need no new allocation, including at a full scalar quota; terminal failure still
prevents new entry. No heap operation or JavaScript runs during preparation.
Borrowed iterators can be cloned for counting without copying their objects.

Promise jobs stage root capacity while the job is still in the queue, before
removal or fuel reset. Async continuations reserve their saved roots before
removing their stored frame. Module/ordinary-async/generator ambient-frame and
native event checkpoint preparation restores the displaced caller if admission
fails; event turn budgets are restored too. Native-pressure GC refuses missing
root capacity before collecting. Iterator roots, callback completions,
finalization holdings, yielded values and Test262 completion errors use the same
preparation path. A fatal host completion stops later delivery but drains pins
from the already-returned host batch without running callbacks or settling waits.

Retained vector capacity stays in the existing realm execution-control ledger
once, including after logical truncation; normal finished-execution cleanup
returns it. Cleanup and ownership reconciliation remain available after terminal
failure. Host-neutral VMs retain the same GC behavior without persistent root
registrations or a document scheduling limit.

Seventeen finite cases cover initial/replacement refusal, prefix stability, exact
retained capacity, spare/empty paths at full scalar quota, frame restoration and stored
continuation preservation, reaction/finalization GC, host-neutral roots and
terminal refunds. The manually populated terminal host batch contains two events
and starts a finite callback with zero fuel; it starts no timer or worker.
Ordinary-library coverage entry points call the same cases; no new coverage
result is claimed. General execution-roots/Promise-edge/debugger/realm traversal
scratch buffers remain separate open ownership work.

The first inherited waiter-control run had one failure: its first import job
now correctly remained queued when earlier root preparation was refused. The
fixture prepares active-job root capacity before filling its scalar budget so it
still reaches waiter publication refusal; its existing shape, prefix, capacity
and job-consumption assertions are unchanged. The original result is preserved
in `/private/tmp/blueice-async-root-storage-waiter-controls-baseline-red-20261017.log`.

Validation: **797 passed, zero failed/ignored**, across 38 executables and 69
fixed selections: seventeen new cases and all 780 prior selected controls.
Formatting, diff checks and offline workspace/all-target Clippy with warnings
denied pass. Cargo reuses the workspace target with incremental compilation and
dev/test debug artifact growth disabled. The same seven pressure cases remain
excluded and JSON resource sweeps are not selected. No network/service replay,
fresh full Test262, real Core/MCP replay, coverage result or integrated acceptance
is claimed. Other GC scratch vectors, retained native values, shared programs
and wider native/integrated gates remain open.

Evidence: `/private/tmp/blueice-async-root-storage-controls-20261017.py`,
`/private/tmp/blueice-async-root-storage-commit-20261017.txt`,
`/private/tmp/blueice-async-root-storage-audit-20261017.py`, and logs matching
`/private/tmp/blueice-async-root-storage-{red,first,focused,async-roots,fmt,clippy}-20261017.log`.
`target/async-root-storage-source-20261017.json` records source/log/executable
hashes and staged/committed blob checks. This is an M1z implementation checkpoint;
wider native/integrated acceptance remains open.

#### Safepoint trace batch follow-up (implementation checkpoint, 2026-10-17)

Acceptance: GC safepoint root and Promise/disposal edge batches own reusable,
admitted native capacity while cached, Rust-held or attached to the heap.
Joint preparation admits coexisting replacements before publication or heap work;
refusal leaves prior capacity and VM state intact. Borrowed traversal builds no
intermediate reference vectors. Owner edges preserve Promise/continuation cycles
without turning side-table values into independent roots. Reuse and cleanup
require no new capacity, including after a terminal scheduling error.

- [x] Reproduce root/edge admission before heap effects with tiny offline cases.
      All three initial cases failed, then passed after joint preparation.
- [x] Implement borrowed traversal, admitted reusable flat batches and scoped
      heap handoff; seventeen ownership/error/GC/realm cases pass.
- [x] Run fixed offline controls/static checks, update evidence and commit with
      a detailed English message. Collector mark/work sets, realm traversal,
      retained native values, shared programs and wider gates remain open.

Implementation: safepoints count borrowed execution and conditional owner-edge
visitors, then prepare a reusable flat batch. Root and edge replacements are
admitted jointly in full while the old allocations coexist; allocator-reported
capacities receive a staging lease and another check before commit. Refusal
returns the old empty cache with stable contents/capacity. Successful preparation
moves one batch into a non-reentrant optional heap slot without metadata-vector
allocation. A control lease follows both buffers throughout Rust/heap ownership;
return clears IDs and restores the charged realm cache on success and heap error.

Promise jobs count borrowed references before reserving active-root slots and
consume no intermediate vector after queue removal. Execution, pending module,
continuation, debugger, Promise reaction and disposable references are visited
directly. Flat edges keep the original conditional owner semantics, including
inverse async invocation/generator links. Reachable owners keep values alive;
unreachable Promise/result and suspended-await cycles remain collectible. Active
parameter eval environments and shadowed dynamic cells are included explicitly.

Cached capacity is charged once and remains reusable at full scalar quota and
after a prior terminal error for internal cleanup. Normal reuse must publish
current ownership and reject a newly exceeded budget before heap work. Late
bounded activation includes host-neutral cache capacity, and discarded child
realms release their caches. Cross-realm marking prepares its batch before
releasing deferred roots and propagates preparation failures.

Seventeen finite lifecycle cases pass, including cached/Rust-held ownership,
full/terminal reuse, heap-error restoration, joint replacement refusal, active
environment GC, live Promise/await/disposal ownership, unreachable cycles, child
realm release and bounded activation. The additional normal-reuse ledger-refusal
case initially entered heap work after the account failed; it now refuses before
that work and preserves the cache. Ordinary-library entry points expose the same
fixtures; no new coverage result is claimed.

The first inherited run had nine failures across seven selections: retained-cache
bytes were absent from seven aggregate assertions, while converter/declaration
exact-budget fixtures needed their separate GC slots prepared before the budget
was filled. Their existing copy/declaration, pointer, collection, refund and
failure-identity checks remain intact. Original results are preserved in
`/private/tmp/blueice-gc-trace-storage-*-baseline-red-20261017.log`.

Validation: **814 passed, zero failed/ignored**, across 38 executables and 70
fixed selections: seventeen new cases and all 797 prior selected controls.
Formatting, diff checks and offline workspace/all-target Clippy with warnings
denied pass. Cargo reuses the workspace target with incremental compilation and
dev/test debug artifact growth disabled. The same seven pressure cases remain
excluded and JSON resource sweeps are not selected. No network/service replay,
fresh full Test262, real Core/MCP replay, coverage result or integrated acceptance
is claimed. Collector work/mark sets, realm traversal scratch, retained native
values, shared programs and wider native/integrated gates remain open.

Evidence: `/private/tmp/blueice-gc-trace-storage-controls-20261017.py`,
`/private/tmp/blueice-gc-trace-storage-commit-20261017.txt`,
`/private/tmp/blueice-gc-trace-storage-audit-20261017.py`, and logs matching
`/private/tmp/blueice-gc-trace-storage-{red,reuse-red,first,gc-trace,fmt,clippy}-20261017.log`.
`target/gc-trace-storage-source-20261017.json` records source/log/executable hashes
and staged/committed blob checks. This is an M1z implementation checkpoint;
collector work/mark sets, realm traversal scratch and wider native/integrated
acceptance remain open.

#### Realm collection metadata follow-up (implementation checkpoint, 2026-10-17)

Acceptance: realm discovery, edge snapshots, owner lookup, ignored pins and
wrapper target metadata retain admitted native storage throughout collection.
Refusal precedes heap marking, collection or realm pruning and refunds every
preparation allocation. A flat plan avoids copied active/target containers in
fixed-point iteration; wrappers forward each target at most once. Preserve live
work/external ownership, borrowed-child fallback and collectible cross-heap cycles.
Collector-produced mark/work sets remain a separate open ownership boundary.

- [x] Reproduce cold metadata refusal before collection/pruning with tiny
      offline cases. Both initial cases failed, then passed after admission.
- [x] Implement admitted discovery, reusable flat metadata and transactional
      cache replacement; eighteen ownership/failure/fallback/GC cases pass.
- [x] Run fixed offline controls/static checks, update evidence and commit with
      a detailed English message. Collector work/mark sets, retained native
      values, shared programs and wider native/integrated gates remain open.

A shared native lease follows actual vector/set capacities while cached or
Rust-held. Detached growth admits coexisting old/new storage, checks reported
capacity before publication and preserves old prefixes on refusal. Discovery
walks each heap once and snapshots child edges directly without a temporary
children vector. A sorted heap index and sorted owner identities replace the
outer lookup maps/sets used during fixed-point traversal.

Each realm node owns its instance/wrapper snapshots, ignored pin set and a target
vector reserved for every incoming wrapper. Active flags live in nodes; each
wrapper forwards at most once, so iteration copies neither active sets nor target
vectors and allocates no additional traversal metadata. Identical target IDs may
appear once per facade; the collector still deduplicates object marking. Known
table-owner identities distinguish true external owners from traversal clones.
Pending work keeps its realm active; unreachable cross-heap facade cycles remain
collectible.

Before reuse, a cached plan is filled without growth. Missing capacity triggers
fresh preparation while the old cache remains owned; complete success replaces
it, and refusal refunds the candidate and restores the old empty cache. Recycle
clears record references, mark results and all logical entries without losing
admitted capacity. Empty graphs and host-neutral contexts release the plan.
Bounded contexts can reuse existing capacity after terminal failure for internal
collection, while preserving the terminal error and scheduling no new cleanup
jobs. Cold/growing plans still require admission and refuse before heap effects.

Immediate borrowed children take the local-collection fallback before metadata
allocation. A borrowed descendant found during discovery returns preparation
storage or restores the cache before fallback. Reentrant/self heap IDs are
excluded from new-child discovery; shared records are visited once.

Eighteen finite cases cover cold pre-GC refusal, native cached/Rust-held ownership,
detached vector/set refusal, partial-discovery refunds, direct/nested borrowing,
shared records, repeated wrapper targets, external ownership, pending jobs,
cross-heap cycles, terminal reuse/refusal, cache rollback, host-neutral release
and empty graphs at full scalar quota. Ordinary-library entry points expose the
same cases; no new coverage result is claimed. Collector-produced mark/work sets
remain separate open ownership work.

The first inherited run had one failure: a cross-realm module-context aggregate
assertion omitted the new cached metadata owner. Its parent/child cache terms
are now explicit; name identity, shared executable identity and callable import
checks remain intact. The original result is preserved in
`/private/tmp/blueice-realm-collection-contexts-baseline-red-20261017.log`.

Validation: **832 passed, zero failed/ignored**, across 38 executables and 71
fixed selections: eighteen new cases and all 814 prior selected controls.
Formatting, diff checks and offline workspace/all-target Clippy with warnings
denied pass. Cargo reuses the workspace target with incremental compilation and
dev/test debug artifact growth disabled. The same seven pressure cases remain
excluded and JSON resource sweeps are not selected. No network/service replay,
fresh full Test262, real Core/MCP replay, coverage result or integrated acceptance
is claimed. Collector work/mark sets, retained native values, shared programs
and wider native/integrated gates remain open.

Evidence: `/private/tmp/blueice-realm-collection-controls-20261017.py`,
`/private/tmp/blueice-realm-collection-commit-20261017.txt`,
`/private/tmp/blueice-realm-collection-audit-20261017.py`, and logs matching
`/private/tmp/blueice-realm-collection-{red,first,focused,realms,fmt,clippy}-20261017.log`.
`target/realm-collection-source-20261017.json` records source/log/executable hashes
and staged/committed blob checks. This is an M1z implementation checkpoint;
collector work/mark sets, retained native values, shared programs and wider
native/integrated acceptance remain open.

#### Collector workspace follow-up (implementation checkpoint, 2026-10-17)

Acceptance: collector work/mark/conditional-edge storage retains an admitted
native owner across cache, collection and returned realm-mark results. Capacity
preparation/refusal precedes marking/sweeping, and automatic or explicit failures
reach the VM as noncatchable scheduling errors. Reuse supports terminal cleanup.
Strong edges, ephemerons, dependent abort signals, minor promotion and realm cycles
retain their semantics without intermediate reference vectors or copied nursery sets.

- [x] Reproduce major/minor workspace admission before heap effects with tiny
      offline cases. Both initial cases failed, then passed after admission.
- [x] Implement reusable admitted collector storage, returned mark ownership,
      bounded worklists and failure propagation; twenty-one finite cases pass.
- [x] Run fixed offline controls/static checks, update evidence and commit with
      a detailed English message. Remaining heap buffers, retained native values,
      shared programs and wider native/integrated gates remain open.

Each heap retains admitted work-vector, mark-set, flat waiter-vector and waiter
index capacity in the document's native account. A replacement is staged while
the old buffers remain owned; requested and reported capacities are checked
before publication, and refusal restores the old cache. Returned realm mark sets
carry a split lease, so Rust-held results remain charged after leaving the heap
and even after that heap is dropped. Recycle transfers the lease without fresh
admission; realm fixed-point passes return prior sets before marking again and
return final sets before local sweeps.

Enqueue checks membership before insertion and queues each heap object at most
once. This matters even for duplicates: inserting an existing key into a full
hash table can otherwise allocate before detecting the duplicate. Two finite
warm-reuse/ephemeron cases first exposed a 64-byte unowned capacity increase;
their original failures remain in the diagnostic logs. The waiter index keeps
keys until the batch clears, avoiding mid-mark tombstones and rehashing.
Borrowed visitors cover object properties/private slots, generator state, native
closures and Web state;
allocation protection uses the same kind visitor without a temporary vector.
Minor collection borrows the monotonically ordered nursery instead of copying it.

Capacity refusal precedes deferred-pin release, marking, weak-target clearing,
promotion and sweeping. Explicit fallible heap collection and automatic GC
propagate the noncatchable scheduling error. Legacy void heap hooks latch it for
the VM safepoint. Module entry/exit and the harness GC hook propagate collection
refusal instead of assuming collection cannot fail. Warm terminal cleanup reuses
existing capacity and preserves the document's failure state.

The first inherited run had 35 failures across twenty selections. Thirty cases
omitted the new collector cache owner from aggregate assertions. Four fixtures now
prepare the separate collector capacity before filling their converter, registry,
metadata or empty-realm allowance. The remaining timer fixture now attaches its
child to the shared document before activation, preserving owner identity instead
of replacing an already-bound account. Existing pointer, publication, serial,
managed-storage, callback and refund checks remain intact. Original results are
preserved in `/private/tmp/blueice-collector-workspace-*-baseline-red-20261017.log`.


Twenty-one finite cases cover early explicit/automatic refusal, managed-store
preservation, cached and Rust-held native ownership, heap destruction, full-quota
recycle/reuse, foreign marks, reported hash capacity, transactional growth,
late activation/refusal, released pins, duplicate conditional edges, minor
remembered/weak edges, ephemeron cycles, dependent signals, warm/cold terminal
cleanup and noncatchable module/harness collection hooks. Ordinary-library
entry points expose the same fixtures; no new coverage result is claimed.

Validation: **853 passed, zero failed/ignored**, across 38 executables and 72
fixed selections: twenty-one new cases and all 832 prior selected controls.
Formatting, diff checks and offline workspace/all-target Clippy with warnings
denied pass. Cargo reuses the workspace target with incremental compilation and
dev/test debug artifact growth disabled. The same seven pressure cases remain
excluded and JSON resource sweeps are not selected. No network/service replay,
physical exhaustion probe, fresh full Test262, real Core/MCP replay, coverage
result or integrated acceptance is claimed. Remaining managed-container backing
buffers, retained native values, shared programs and wider native/integrated
gates remain open.

Evidence: `/private/tmp/blueice-collector-workspace-controls-20261017.py`,
`/private/tmp/blueice-collector-workspace-commit-20261017.txt`,
`/private/tmp/blueice-collector-workspace-audit-20261017.py`, and logs matching
`/private/tmp/blueice-collector-workspace-{red,reuse-red,diagnose,collector,fmt,clippy}-20261017.log`.
`target/collector-workspace-source-20261017.json` records source/log/executable
hashes and staged/committed blob checks. This is an M1z implementation checkpoint;
remaining managed-container buffers and wider native/integrated acceptance
remain open.

#### Cached module values follow-up (implementation checkpoint, 2026-10-17)

Acceptance: cached completion/error values retain admitted native storage while
records are installed, parked or Rust-held. Admission precedes primitive
copying and cached-value publication. Shared asynchronous evaluation errors keep one immutable value and
origin owner; late activation attaches existing values once. Root failures,
replacement, propagation and VM destruction preserve identity and refund the
correct owner. Keep diagnostic-coordinate refinements parked.

- [x] Reproduce missing cached completion/error native ownership with tiny
      offline cases through module execution. Both original cases failed, then
      passed after cached owners were admitted.
- [x] Implement retained owners and pre-copy admission; twenty-one finite
      synchronous, await/debugger, sharing, refusal and release cases pass.
- [x] Run fixed offline controls/static checks, update evidence and commit with
      a detailed English message. Remaining managed-container buffers, shared
      programs and wider native/integrated gates remain open.


Cached completions own admitted immutable values. Cached evaluation errors share
one immutable value and owned origin across async parents and cached replay.
The retained owner admits its allocation headers before requesting the primitive
copy; the existing fallible module-copy boundary checks reported UTF-16 capacity
and transfers its lease without copying again. Cache publication follows value
ownership and successful rooting; debugger error roots roll back if retention
refuses. Synchronous,
await-resume and debugger completion/error paths use the same retention boundary.

A value/error held in Rust remains charged after leaving its graph or outliving
its VM. An error's weak discovery entry keeps a separate shared header lease,
without making a JavaScript object live. Last-strong-owner release frees value
and origin buffers before their native refund; the remaining headers stay
charged until pruning drops the weak entry. Discovery-vector growth stages full
replacement capacity while old storage remains owned, checks actual capacity
before publication and restores the old prefix/refunds staging on refusal.
Empty discovery releases its vector without a cleanup-time reallocation.

Existing host-neutral graph/parked values and Rust-held errors attach once on
bounded activation. A foreign document cannot replace an existing owner.
Ordinary JavaScript results and deliveries retain their value/root rules, and
cloning a cached error adds no primitive copy or native charge. Terminal cleanup
releases values, owned origins, weak headers and discovery slots without new
admission or resetting the document's failure state.

The first inherited run had twelve failures across six selections: aggregate
assertions omitted the new cached-value owner, including a completion held by a
Rust-side graph. Their exact native totals now include those owners explicitly;
pointer, managed-heap, callback, publication, serial and refund checks remain
intact. Original results are preserved in
`/private/tmp/blueice-module-values-*-baseline-red-20261017.log`.


Validation: **875 passed, zero failed/ignored**, across 38 executables and 74
fixed selections: twenty-one new ownership cases, one existing displaced-origin
control and all 853 prior selected cases. Formatting, diff checks and offline
workspace/all-target Clippy with warnings denied pass. Cargo reuses the workspace
target with incremental compilation and dev/test debug artifact growth disabled.
The same seven pressure cases remain excluded and JSON resource sweeps are not
selected. All execution cases are offline, fixed and finite. No network/service
replay, physical exhaustion probe, fresh full Test262, real Core/MCP replay,
coverage result or integrated acceptance is claimed.

Evidence: `/private/tmp/blueice-module-values-controls-20261017.py`,
`/private/tmp/blueice-module-values-commit-20261017.txt`,
`/private/tmp/blueice-module-values-audit-20261017.py`, and logs matching
`/private/tmp/blueice-module-values-{red,first,focused,cached-values,origin-control,fmt,clippy}-20261017.log`.
`target/module-values-source-20261017.json` records source/log/executable hashes
and staged/committed blob checks. Ordinary-library entry points expose the same
fixtures; no new coverage result is claimed. This is an M1z checkpoint. Shared
programs, other native values, managed-container backing buffers and wider
native/integrated acceptance remain open.

#### Pending module error reporting follow-up (implementation checkpoint, 2026-10-17)

Acceptance: admit snapshot slots/name buffers and register every object value
before consuming pending flags or dispatching any report. Preserve cached values
and origins while records are replaced or Rust-held. Preparation refusal restores
flags, roots and native ownership; interrupted delivery preserves unattempted
records only when their name and immutable error owner still match. Keep reentrant
reporting, cleanup and ordinary public error semantics intact.

- [x] Reproduce missing admission, batch roots and registration rollback with
      tiny offline cases through the real module reporting boundary. All three
      original cases failed, then passed after admitted snapshots/rooting.
- [x] Implement admitted snapshots, batch rooting and pending-work restoration;
      twenty-one finite callback/replacement/reentrancy/failure cases pass.
- [x] Run fixed offline controls/static checks, update evidence and commit with
      a detailed English message. Shared programs, other native buffers and wider
      integrated acceptance remain open.


Reporting now prepares detached canonical-name/error snapshots. The vector's
requested and reported slot capacity and every copied name buffer retain one
native lease before any pending flag changes. Shared immutable errors keep their
existing value/origin owner. Object-valued errors receive independent admitted
registered roots before the first report can enter user code. Preparation
refusal rolls back the new root suffix and drops all staged storage while
preserving the original pending flags, record owners and roots.

A report may release the original graph and embedding result root, then collect,
without reclaiming the batch's later values. The test models those independent
owners explicitly: the last public thrown result normally remains rooted until
the embedding starts another execution or discards its context. Batch cleanup
releases its own registrations on success and failure, including terminal abort,
and prunes expired weak error discovery after dropping the snapshot.

The selected flags clear together before callbacks, preventing reentrant delivery
of that batch. An attempted report remains consumed even if it fails. Only its
unattempted suffix can regain pending flags, and only when canonical name,
evaluated state and immutable error-owner identity still match. Removed/replaced
errors are not resurrected; newly pending work survives the old snapshot. Empty
or disabled reporting acquires no snapshot storage, successful records clear
stale flags without allocation and unevaluated records keep their pending work.

Twenty-one finite cases cover original refusal/GC/rollback failures, public
checkpoint/event behavior, exact native allowances, callback-held ownership,
reentrancy, partial delivery/retry, record removal/replacement, newly pending
work, terminal release, empty/disabled/successful/deferred records, primitive
sharing, repeat suppression and independent document failure. Ordinary-library
entry points expose the same fixtures; no new coverage result is claimed.


Validation: **908 passed, zero failed/ignored**, across 39 executables and 76
fixed selections: twenty-one new cases, twelve existing public host-error cases
and all 875 prior selected controls. Formatting, diff checks and offline
workspace/all-target Clippy with warnings denied pass. Cargo reuses the workspace
target with incremental compilation and dev/test debug artifact growth disabled.
The seven earlier pressure cases remain excluded; the added host-error selection
skips its two legacy cases containing infinite loops. JSON resource sweeps are
not selected. Execution cases are offline, fixed and finite. No network/service
replay, physical exhaustion probe, fresh full Test262, real Core/MCP replay,
coverage result or integrated acceptance is claimed.

Evidence: `/private/tmp/blueice-module-reporting-controls-20261017.py`,
`/private/tmp/blueice-module-reporting-commit-20261017.txt`,
`/private/tmp/blueice-module-reporting-audit-20261017.py`, and logs matching
`/private/tmp/blueice-module-reporting-{red,first,focused,reporting,host-errors,fmt,clippy}-20261017.log`.
`target/module-reporting-source-20261017.json` records source/log/executable hashes
and staged/committed blob checks. This is an M1z checkpoint. Broader report/event
scratch, shared programs, managed-container backing buffers and wider native/
integrated acceptance remain open.

#### Persistent root table follow-up (implementation checkpoint, 2026-10-17)

Acceptance: retain admitted native ownership for the root registry's backing
capacity, including bootstrap roots, Rust-held heaps and retained spare storage.
Prepare complete replacement capacity while the old table is still owned;
refusal preserves IDs, registrations and old entries. Attach existing root and
collector storage to the same document once. Removal/released realm pins and heap
destruction refund the right allocation without new admission during cleanup.

- [x] Reproduce unowned capacity and registration before native refusal with
      tiny offline cases through heap/VM root boundaries. Both original cases
      failed, then passed after admission.
- [x] Implement admitted root-table storage and native heap binding; twenty-two
      finite cases cover growth, spare reuse, cleanup, activation, failure and GC.
- [x] Run fixed offline controls/static checks, update evidence and commit with
      a detailed English message. Other heap/native buffers, shared programs and
      wider integrated acceptance remain open.

The root registry now owns its reported hash-table slot capacity in the document
native account. Growth first admits the complete candidate allocation while the
old allocation remains charged, reserves a detached table, checks reported
capacity and transfers entries. The next root ID and registration counter advance
only after preparation succeeds. Refusal refunds candidate storage and preserves
all old entries, counters and the next identifier. Existing spare slots require
no extra native admission; released identifiers remain unavailable for reuse.

Removal can decrease reported hash capacity without freeing its allocation.
The owner therefore retains the original admitted allocation size until a real
replacement or final removal. Removing the last root drops the empty table and
refunds storage, including released realm pins and cleanup after terminal abort.
A Rust-held heap keeps the account alive after its original VM drops and refunds
its allocation when the heap is destroyed.

Generalize the collector budget bridge into a heap-native budget. Existing root
and collector caches bind to one document together and only once. A refusal
installs neither owner, preserves root registrations and discards only unadmitted
collector scratch. A different document cannot adopt either owner. Root-storage
refusal maps to the noncatchable scheduling limit. Infallible Promise rejection
tracking latches that failure and reconciles storage before returning, with no
panic or unrooted pending notification. Fallible bootstrap/foreign-realm callers
propagate root refusal rather than assuming it cannot occur.

The first two tiny cases failed before implementation. Expanding the selection
exposed two rejection-producer panics (native admission and injected root-ID
refusal); all twenty-two cases now pass. The first inherited run exposed 49
failures across 21 selections. Exact aggregate assertions now include heap root
storage, heap-drop assertions account for its refund, and tests isolating other
container boundaries prewarm only a few reusable root slots. These changes keep
the original boundary assertions while adding the new independent native owner.

Validation: **930 passed, zero failed/ignored**, across 39 executables and 77
fixed selections: twenty-two new root-table cases and all 908 prior selected
controls. Formatting, diff checks and offline workspace/all-target Clippy with
warnings denied pass. Cargo reuses the workspace target with incremental
compilation and dev/test debug artifact growth disabled. The same nine legacy
resource/infinite-loop cases remain excluded and JSON resource sweeps are not
selected. Execution cases are offline, fixed and finite. No network/service
replay, physical exhaustion probe, fresh full Test262, real Core/MCP replay,
coverage result or integrated acceptance is claimed.

Evidence: `/private/tmp/blueice-root-table-controls-20261017.py`,
`/private/tmp/blueice-root-table-commit-20261017.txt`,
`/private/tmp/blueice-root-table-audit-20261017.py`, and logs matching
`/private/tmp/blueice-root-table-{red,expanded,root-table,fmt,clippy}-20261017.log`.
Initial inherited failures are preserved in
`/private/tmp/blueice-root-table-*-baseline-red-20261017.log`.
`target/root-table-source-20261017.json` records source/log/executable hashes
and staged/committed blob checks. Ordinary-library entry points expose the same
fixtures; no new coverage result is claimed. This is an M1z checkpoint. Other
heap tables, deferred pin queues, broader report/event scratch, shared programs
and wider native/integrated acceptance remain open.

#### Foreign callable pin storage follow-up (implementation checkpoint, 2026-10-17)

Acceptance: admit retained pin headers and registry slots before root publication;
retain ownership across clones, Rust-held lifetimes and late bounded activation.
Dropping handles and reclaiming released roots must need no new allocation or
admission, including terminal cleanup. Preserve live foreign callables, ignored
conditional pins, native refusal rollback and collector failure ordering.

- [x] Reproduce missing native ownership/admission with tiny offline cases.
      Both initial tests failed before admission and now pass.
- [x] Implement admitted pin storage and allocation-free release; twenty finite
      cases verify clone, heap, activation, terminal, refusal and GC lifecycles.
- [x] Run fixed offline controls/static checks, update evidence and commit with
      a detailed English message. Other heap buffers, shared programs and wider
      integrated acceptance remain open.

A per-heap sorted registry now retains one strong reference to each foreign
callable pin. Its vector slots and each shared pin header enter the document
native budget before root publication. Vector growth admits a complete detached
replacement while old slots remain owned; root refusal drops the prepared pin
storage and preserves the prior registry and root counters. Clones acquire no
additional allocation or native charge.

Dropping the last external handle only decreases its reference count. The
registry retains the header and its charge until a safe cleanup point observes
that sole ownership. Marking skips released pins with a binary lookup over the
monotonic root IDs, preserving conditional/ignored-pin behavior without a
scratch queue or mutation before collector preparation succeeds. Cleanup retains
live pins, refunds released headers and frees the slot vector when it becomes
empty. This works after terminal failure and never allocates in a pin destructor.
Rust-held handles keep their own native header owner after their heap is dropped.

Late bounded activation jointly stages root-table, collector and pin storage.
No owner is installed until all three fit; shared pin headers receive split
leases without another admission. Refusal preserves registrations/handles,
refunds every candidate and drops only unadmitted collector scratch. Same-document
rebinding remains idempotent, and another document cannot adopt bound storage.

Both initial cases failed before admission and now pass. Twenty finite cases
cover root publication/refusal, live storage, clone sharing, handle-drop lifetime,
partial/final release, Rust-held handles/heaps, late activation/refusal, foreign
budgets, invalid objects/IDs, root-growth rollback, complete slot replacement,
exact allowances, minor/major GC, ignored/released pins and public foreign
callback collection/release. The first full selection passed 949 cases and
exposed one old aggregate assertion missing the new 88-byte pin owner; it now
includes both realms' pin storage. The released-pin root-table fixture also
counts pin headers/slots when checking final cleanup refunds.

Validation: **950 passed, zero failed/ignored**, across 39 executables and 78
fixed selections: twenty new foreign-pin cases and all 930 prior selected
controls. Formatting, diff checks and offline workspace/all-target Clippy with
warnings denied pass. Cargo reuses the workspace target with incremental
compilation and dev/test debug artifact growth disabled. The same nine legacy
resource/infinite-loop cases remain excluded and JSON resource sweeps are not
selected. Execution cases are offline, fixed and finite. No network/service
replay, physical exhaustion probe, fresh full Test262, real Core/MCP replay,
coverage result or integrated acceptance is claimed.

Evidence: `/private/tmp/blueice-realm-pins-controls-20261017.py`,
`/private/tmp/blueice-realm-pins-commit-20261017.txt`,
`/private/tmp/blueice-realm-pins-audit-20261017.py`, and logs matching
`/private/tmp/blueice-realm-pins-{red,expanded,pins,fmt,clippy}-20261017.log`.
The initial inherited failure is preserved in
`/private/tmp/blueice-realm-pins-contexts-baseline-red-20261017.log`.
`target/realm-pins-source-20261017.json` records source/log/executable hashes
and staged/committed blob checks. Ordinary-library entry points expose the same
fixtures; no new coverage result is claimed. This is an M1z checkpoint. Other
heap buffers, broader report/event scratch, shared programs and wider native/
integrated acceptance remain open.

#### Host-turn snapshot storage follow-up (implementation checkpoint, 2026-10-17)

Acceptance: admit realm discovery and due callback/abort/realm snapshot backing
storage before filling it, retain ownership across callbacks and Rust-held
snapshots, and refund every temporary on refusal and terminal exits. Traversal
snapshots must keep only weak realm handles. Preserve document filtering,
cycles/borrowed realms, wake queries, timer/source order, interval/abort turn
snapshots, cancellation, debugger behavior and standalone policy.

- [x] Reproduce missing native ownership/admission with tiny offline cases.
      All three original realm/timer/public-wake cases failed before admission.
- [x] Implement admitted snapshot storage; twenty-two finite cases cover
      lifecycle, refusal, timer ordering, weak discovery and nested ownership.
- [x] Run fixed offline controls/static checks, update evidence and commit with
      a detailed English message. Finalization cleanup index/snapshot storage,
      other heap buffers, shared programs and wider integrated gates remain open.

A leased task vector reuses transactional metadata-capacity admission: reserve
complete detached replacement storage while the old allocation remains charged,
check reported capacity, then move the prefix. Callers receive borrowed slices
and explicit admitted append/reserve operations. Spare slots need no extra
native charge; refusal preserves the original prefix, pointer and ownership.
Nested snapshot fields drop before their enclosing slot lease refunds.

Realm discovery owns its weak-handle queue and seen-ID set during preparation.
Each unique same-document realm is visited once; duplicate/cyclic references,
foreign documents and borrowed descendants preserve their existing treatment.
The returned sorted weak vector retains a native lease across child checkpoints
and callbacks without adding strong VM owners. Preparation failure refunds all
queue/set/result storage; dead realms can disappear while a returned snapshot
still exists in Rust.

Due callbacks now use one admitted deadline/ID vector, retaining the original
standalone ordering without a second unowned ID-vector conversion. Both timer
APIs borrow their snapshot throughout delivery. Runnable abort serials use a
bounded flat vector, and per-realm timer plans use an admitted sorted array
with allocation-free lookup. Outer-plan refusal refunds incoming nested buffers.
Empty snapshots reserve no slots, including at a full scalar quota; callback-
excluded turns still snapshot abort work. Existing clock sampling, interval
rearming, cancellation and per-turn eligibility semantics remain intact.

All three initial cases failed before admission and now pass. Twenty-two finite
cases cover exact/refused replacement and spare reuse, Rust-held/native/terminal
lifetimes, weak discovery, cycles/deduplication/document filtering, borrowed
children, partial-discovery rollback, public wake/timer refusal, callback
cancellation/replacement, standalone intervals, abort ownership/refusal,
outer/nested plan lifetime/refusal, empty snapshots and callback inclusion.

Validation: **972 passed, zero failed/ignored**, across 39 executables and 79
fixed selections: twenty-two new host-snapshot cases and all 950 prior selected
controls. Formatting, diff checks and offline workspace/all-target Clippy with
warnings denied pass. Cargo reuses the workspace target with incremental
compilation and dev/test debug artifact growth disabled. The same nine legacy
resource/infinite-loop cases remain excluded and JSON resource sweeps are not
selected. Execution cases are offline, fixed and finite. No network/service
replay, physical exhaustion probe, fresh full Test262, real Core/MCP replay,
coverage result or integrated acceptance is claimed.

Evidence: `/private/tmp/blueice-task-snapshots-controls-20261017.py`,
`/private/tmp/blueice-task-snapshots-commit-20261017.txt`,
`/private/tmp/blueice-task-snapshots-audit-20261017.py`, and logs matching
`/private/tmp/blueice-task-snapshots-{red,first,expanded,task-storage,fmt,clippy}-20261017.log`.
`target/task-snapshots-source-20261017.json` records source/log/executable hashes
and staged/committed blob checks. Ordinary-library entry points expose the same
fixtures; no new coverage result is claimed. This is an M1z checkpoint.
Finalization cleanup index/job snapshots, other heap buffers, broader report/
event scratch, shared programs and wider native/integrated gates remain open.

#### Finalization storage follow-up (implementation checkpoint, 2026-10-17)

Acceptance: admit ready-registry index backing capacity before collection
changes weak cells, pins, objects or counters. Preserve readiness and roots on
refusal; retain one native owner through table reuse, late document activation,
Rust-held heaps and terminal release. Admit page registry snapshots before task
publication. Transfer standalone cleanup cells directly into queued jobs without
bulk job or replacement-cell vectors, preserving existing cleanup order.

- [x] Reproduce missing native ownership/admission with tiny offline cases.
      All three original GC/index/page-snapshot cases failed, then passed.
- [x] Implement admitted index/snapshot storage and direct standalone transfer;
      twenty-five finite cases pass, including reported/full replacement bounds.
- [x] Run fixed offline controls/static checks, update evidence and commit with
      a detailed English message. Other heap buffers, broader report/event
      scratch, shared programs and wider integrated gates remain open.

The ready-registry index now retains one native owner for its hash allocation.
After marking, collection determines which surviving registries will gain their
first ready cell. It admits a complete replacement while old storage remains
charged, checks reported capacity and transfers entries before weak clearing,
pin release, sweep or collection counters change. Refusal recycles the borrowed
mark result before returning and preserves those heap states. Dead registries
and live targets do not demand fictitious ready-index entries.

Existing keys update through mutable value lookup, avoiding HashMap growth when
updating a full table. Deletion may reduce reported capacity without freeing its
allocation, so the owner retains the original allocation size through partial
delivery. GC batches keep prepared spare storage through intermediate removals;
final pruning, unregister and final-cell delivery free an empty index without
new admission. Rust-held heaps keep the same native account until destruction.

Existing ready-index storage joins root, collector and foreign-pin binding in
one transaction. No owner is installed unless all four fit; refusal preserves
roots/readiness and discards only unadmitted collector scratch. Borrowed ready
identities become an admitted sorted page snapshot before tasks are published;
its lease survives Rust-held lifetimes and refunds on success or refusal.

Host-neutral cleanup compacts cells in place and moves holdings directly into
the traced Promise queue in their original per-registry order. There is no bulk
job vector or replacement live-cell vector. Empty cell buffers are dropped;
nonempty buffers keep their existing capacity. Registration protects its at-most-
four object references in a fixed stack array. Cell backing and callback-entry
preparation remain within the broader managed-container/host-buffer gate.

The original three cases failed before admission and now pass. Expanded cases
cover minor/major refusal and generation behavior, released pins, live targets,
full-table reuse, terminal delivery/unregister, dead registries, Rust-held heaps,
late activation/refusal and foreign documents, leased/sorted/empty snapshots,
publication/refusal accounting, standalone cell order/capacity and queued object
holdings through GC. Reported-capacity refusal and complete replacement/exact
allowance cases also pass. The first fixed sweep exposed one old spare-queue
fixture without allowance for its new temporary snapshot. Both queue-growth and
spare-slot fixtures now isolate that snapshot's bytes and assert their full
refund on return, preserving their original capacity boundary checks.

Validation: **1000 passed, zero failed/ignored**, across 39 executables and 81
fixed selections: twenty-five new finalization-storage cases, three existing
cleanup-index cases and all 972 prior selected controls. Formatting, diff checks
and offline workspace/all-target Clippy with warnings denied pass. Cargo reuses
the workspace target with incremental compilation and dev/test debug artifact
growth disabled. The same nine legacy resource/infinite-loop cases remain
excluded and JSON resource sweeps are not selected. Execution cases are offline,
fixed and finite. No network/service replay, physical exhaustion probe, fresh
full Test262, real Core/MCP replay, coverage result or integrated acceptance is
claimed.

Evidence: `/private/tmp/blueice-finalization-storage-controls-20261017.py`,
`/private/tmp/blueice-finalization-storage-commit-20261017.txt`,
`/private/tmp/blueice-finalization-storage-audit-20261017.py`, and logs matching
`/private/tmp/blueice-finalization-storage-{red,first,expanded,finalization,index-controls,fmt,clippy}-20261017.log`.
The initial inherited failure is preserved in
`/private/tmp/blueice-finalization-storage-browser-controls-baseline-red-20261017.log`.
`target/finalization-storage-source-20261017.json` records source/log/executable
hashes and staged/committed blob checks. Ordinary-library entry points expose
the same fixtures; no new coverage result is claimed. This is an M1z checkpoint.
Cell backing/callback-entry preparation, other heap buffers, broader report/event
scratch, shared programs and wider native/integrated gates remain open.

#### Finalization callback-entry follow-up (implementation checkpoint, 2026-10-17)

Acceptance: prepare callback/holdings roots, primitive ownership, argument slots
and initial host instruction fuel while the ready cell and its queued task keep
their original owners. Refusal preserves cells, queued task/root, ambient fuel
and active-registry context. Committed attempts consume one cell, hand ownership
into call dispatch once, and restore roots/context on every exit. Preserve
uncaught-reporting, microtask checkpoints, reentrancy and cancellation semantics.

- [x] Reproduce entry refusal losing cleanup cells with tiny offline cases.
      All three original root/payload/zero-fuel cases failed, then passed.
- [x] Implement prepared entry; twenty-two finite cases cover resource/fuel
      refusal, ownership handoff, callback GC, context and repeated delivery.
- [x] Run fixed offline controls/static checks, update evidence and commit with
      a detailed English message. Cell backing, other heap/host buffers, shared
      programs and wider integrated acceptance remain open.

Cleanup now borrows callback/holdings data from the ready cell while its queued
registry root keeps the original owner. Preparation checks document failure and
starting fuel, admits primitive ownership and a one-argument vector, checks its
reported capacity and prepares callback/holdings root slots. The initial host
instruction runs before dequeue/cell consumption. Preparation errors reconcile
partial ownership and preserve the queued task, cell, root registration, active
registry and ambient fuel. No JavaScript or heap collection runs during staging.

Committing an attempt appends admitted roots, moves one cell into its argument
vector and publishes the task-count change. Dispatch takes over argument/payload
ownership before its first fallible step. The staged guards release directly at
that infallible handoff, so ordinary call-entry ownership acquires the same
buffers once. An attempted dispatch consumes its cell even if the call fails;
unattempted cells retain readiness. Every exit restores the original root prefix
and active-registry context, releases the committed task root and reconciles
ownership before propagating an error. Empty/cancelled tasks require no callback
argument storage or initial host instruction.

The original three cases failed before preparation and now pass. Twenty-two
finite cases cover argument/root/payload refusal, full replacement/exact
allowances, partial/terminal refunds observed through the shared account,
zero-fuel precedence/retry, active context, once-only delivery, ownership handoff,
root-prefix restoration, callback GC, ignored object returns, attempted-dispatch
failure, empty tasks and borrowed-peek validation. The initial expanded selection
exposed an assertion that omitted the existing HostValue conversion buffer;
comparison with an ordinary single-argument call now verifies equal ownership
without treating that independent conversion buffer as an extra argument owner.

Validation: **1026 passed, zero failed/ignored**, across 40 executables and 86
fixed selections: twenty-two new entry cases, four finite public cleanup cases
and all 1000 prior selected controls. Public cases cover cleanup/microtask order,
unregister cancellation, throwing callbacks, error reporting and navigation
handler cleanup. Formatting, diff checks and offline workspace/all-target Clippy
with warnings denied pass. Cargo reuses the workspace target with incremental
compilation and dev/test debug artifact growth disabled. The same nine earlier
resource/infinite-loop skips remain. Public finalization selection names only
four finite cases; its large/resource/infinite cases and JSON resource sweeps
remain unselected. No network/service replay, physical exhaustion probe, fresh
full Test262, real Core/MCP replay, coverage result or integrated acceptance is
claimed.

Evidence: `/private/tmp/blueice-finalization-entry-controls-20261017.py`,
`/private/tmp/blueice-finalization-entry-commit-20261017.txt`,
`/private/tmp/blueice-finalization-entry-audit-20261017.py`, and logs matching
`/private/tmp/blueice-finalization-entry-{red,first,expanded-red,entry-finalization,public-cleanup-*,fmt,clippy}-20261017.log`.
`target/finalization-entry-source-20261017.json` records source/log/executable
hashes and staged/committed blob checks. Ordinary-library entry points expose
the same fixtures; no new coverage result is claimed. This is an M1z checkpoint.
Cell backing, other heap/host buffers, broader report/event scratch, shared
programs and wider native/integrated acceptance remain open.

#### Finalization cell backing follow-up (implementation checkpoint, 2026-10-17)

Acceptance: actual cell-vector capacity belongs to the managed heap/document;
live payload is charged once. Admit complete detached growth while old storage
coexists, check reported capacity before publication and preserve existing cells
on refusal. Spare-slot reuse needs only new payload bytes. Partial removal
retains its backing charge; empty release, collection and heap destruction return
the right bytes. Cover registration protection, shared documents and cleanup.

- [x] Reproduce missing capacity ownership/admission with tiny offline cases.
      All three original cases failed before the fix and now pass.
- [x] Implement capacity/payload accounting and verify growth/refusal, reuse,
      unregister/delivery, GC, document sharing and destruction lifecycles.
      All twenty-four finite offline cases pass.
- [x] Run fixed offline controls/static checks, update evidence and commit with
      a detailed English message. Other managed/native heap/host buffers, shared
      programs and wider integrated acceptance remain open.

The registry's existing managed owner now includes actual retained vector slots
and live payload separately. The old per-live-cell header charge missed spare
slots and refunded headers while the vector still retained its allocation.
Initial/growing registration validates the registry brand first, protects its
at-most-four input objects, admits a complete detached candidate alongside the
old allocation and checks reported capacity before moving cells and publishing
the new registration. A spare slot requires only its new payload allowance.
Refusal preserves existing cell data and vector capacity; ordinary admission
collection may still run.

Unregister, individual cleanup delivery and standalone draining refund only
removed payload while any cell remains. Removing the last cell drops the vector
and returns its complete backing charge. Object collection and heap destruction
use the same managed owner. Document accounting includes capacity created before
attachment, follows Rust-held child heaps and permits cleanup refunds after
terminal failure. There is no second native charge for these managed slots.

All three initial accounting/admission/removal cases failed before the fix and
now pass. Twenty-four finite cases cover initial/exact/one-byte-short admission,
complete coexisting growth, spare-slot payload-only admission/refusal, partial
and empty unregister/delivery/drain, collection, shared-document refusal/reuse,
late/idempotent attachment, Rust-held child destruction, GC input protection,
invalid registry/target/token precedence and cleanup after terminal failure.
Fixtures use at most five cells and tiny strings with scalar byte allowances.

Validation: **1050 passed, zero failed/ignored**, across 40 executables and 87
fixed selections: twenty-four new backing cases and all 1026 prior selected
controls. Formatting, diff checks and offline workspace/all-target Clippy with
warnings denied pass. Cargo reuses the workspace target with incremental
compilation and dev/test debug artifact growth disabled. The same nine earlier
resource/infinite-loop skips remain. Public finalization selection names only
four finite cases; its large/resource/infinite cases and JSON resource sweeps
remain unselected. No network/service replay, physical exhaustion probe, fresh
full Test262, real Core/MCP replay, coverage result or integrated acceptance is
claimed.

Evidence: `/private/tmp/blueice-finalization-backing-controls-20261017.py`,
`/private/tmp/blueice-finalization-backing-commit-20261017.txt`,
`/private/tmp/blueice-finalization-backing-audit-20261017.py`, and logs matching
`/private/tmp/blueice-finalization-backing-{red,first,expanded,backing-finalization,fmt,clippy}-20261017.log`.
`target/finalization-backing-source-20261017.json` records source/log/executable
hashes and staged/committed blob checks. Ordinary-library entry points expose
the same fixtures; no new coverage result is claimed. This is an M1z checkpoint.
Other managed/native heap/host buffers, broader report/event scratch, shared
programs and wider integrated acceptance remain open.

#### Timer callback-entry follow-up (implementation checkpoint, 2026-10-17)

Acceptance: validate starting fuel and admit an interval's independent input
copy and callback/object protection before committing an attempt. Refusal keeps
the timer registration, roots, deadline/serial, run counters and ambient fuel/
nesting. Transfer argument slots and primitive ownership once into dispatch;
retain only still-owned task protection through the Promise checkpoint. Keep
interval self-cancellation/GC, task ordering, exception reporting and rearming.

- [x] Reproduce starting-fuel and duplicate argument-capacity failures with
      tiny finite callbacks.
      All three original cases failed before preparation/handoff changes and
      now pass. The callback observation exposed one extra Value slot (40 bytes
      on this build) beyond ordinary single-argument dispatch.
- [x] Prepare timer entry before consumption and verify exact native ownership,
      refusal/unwind, callback GC and Promise-checkpoint lifetimes.
      All twenty-one finite entry cases pass.
- [x] Run fixed offline controls/static checks, update evidence and commit with
      a detailed English message. Wider managed/native buffers, shared programs
      and integrated acceptance remain open.

Preparation checks document state and starting fuel before staging or removing
anything. A one-shot keeps its existing argument/root allocations until commit;
an interval admits a complete independent argument vector and primitive copy,
checks reported capacity and prepares execution-root capacity. Its callback and
object arguments occupy a fixed array of at most thirty-three references, with
no intermediate object vector or new persistent root registrations. The first
host instruction runs before committing either timer kind. Preparation refusal
preserves the registration, deadline/serial, counters and ambient fuel/nesting;
partial staged owners refund through failure-aware reconciliation.

Committed one-shots move their original arguments into the prepared owner and
retain their root vector separately. Intervals append their prepared object
edges to the execution-root prefix so self-cancellation cannot remove their task
protection. Ordinary call entry adopts argument payload/slots at one direct
handoff. The task retains only its actual root protection through the Promise
checkpoint; consumed argument capacity is no longer charged there. Every exit
restores the original root prefix/nesting and reconciles released owners. An
attempted one-shot is consumed on dispatch failure; an interval keeps its old
deadline/serial until a successful task/checkpoint rearms it.

All three original cases now pass. Twenty-one finite entry cases cover zero-fuel
precedence/retry, one-shot transfer at full scalar allowance, interval argument/
payload admission/refusal, exact preparation, coexisting root replacement,
partial refunds, no new persistent registrations over three interval deliveries,
cancelled identities, ambient-prefix/nesting restoration, attempted failure,
terminal-document refusal, object protection through cancellation/collection,
ignored returns and argument-capacity release before Promise checkpoints.
Ordinary native-call comparisons verify one argument-vector owner independently
of the existing HostValue conversion buffer.

Timer registration's argument/root-vector staging and complete table-replacement
admission remain separate follow-up work, alongside other heap/host buffers,
shared programs and integrated acceptance.

Validation: **1077 passed, zero failed/ignored**, across 41 executables and 94
fixed selections: twenty-one new entry cases, six finite public timer cases and
all 1050 prior selected controls. Public cases cover arguments, due/creation
order, interval self-cancellation, callback/microtask ordering, throwing callbacks
and navigation cleanup. Formatting, diff checks and offline workspace/all-target
Clippy with warnings denied pass. Cargo reuses the workspace target with
incremental compilation and dev/test debug artifact growth disabled. The same
nine earlier resource/infinite-loop skips remain. Public timer/finalization
selection names only finite cases; their large/resource/infinite cases and JSON
resource sweeps remain unselected. No network/service replay, physical exhaustion
probe, fresh full Test262, real Core/MCP replay, coverage result or integrated
acceptance is claimed.

Evidence: `/private/tmp/blueice-timer-entry-controls-20261017.py`,
`/private/tmp/blueice-timer-entry-commit-20261017.txt`,
`/private/tmp/blueice-timer-entry-audit-20261017.py`, and logs matching
`/private/tmp/blueice-timer-entry-{red,first,expanded,entry-timer,public-timer-*,fmt,clippy}-20261017.log`.
`target/timer-entry-source-20261017.json` records source/log/executable hashes and
staged/committed blob checks. Ordinary-library entry points expose the same
fixtures; no new coverage result is claimed. This is an M1z checkpoint. Wider
managed/native buffers, shared programs and integrated acceptance remain open.

#### Timer registration follow-up (implementation checkpoint, 2026-10-17)

Acceptance: preflight all input/root IDs and complete root-table/table/vector
storage before consuming timer IDs or task serials. Stage actual reported
capacity and primitive copies while old allocations remain owned. Commit roots
and one timer without another fallible admission step; refusal preserves old
entries/capacities, IDs, roots and registration counters. Charge only actual
object-root slots for primitive arguments and retain normal timer semantics.

- [x] Reproduce coexisting table-growth, premature serial and partial-root
      publication failures with tiny finite registrations.
      All three original cases failed: growth accepted a net-only allowance,
      root refusal consumed a task serial and partial rooting consumed a root ID.
- [x] Implement admitted detached timer storage and an atomic root batch; verify
      reuse, shared-document ownership, rollback, cleanup and GC lifetimes.
      All twenty-four finite registration cases pass.
- [x] Run fixed offline controls/static checks, update evidence and commit with
      a detailed English message. Other buffers, shared programs and integrated
      acceptance remain open.

After callback validation, delay coercion and clock sampling, count admission
precedes storage preparation. Borrowed fixed input references validate every
object and the complete root-ID/counter range before preparing an owned root-
table replacement. A separate independent native lease covers argument/root
vectors, primitive copies and complete timer-table growth while original storage
remains charged. Actual reported capacity is checked after each allocation and
before allocating its successor. Spare timer-table slots avoid a growth fee;
primitive inputs add no object-root slots.

Task serial acquisition is the last fallible step. An admitted root batch then
commits unique registrations/counters into preallocated output slots, existing
timers move into any admitted table replacement and the new timer/ID publishes.
The staged lease hands ownership directly into the live registry, with no new
admission after mutation. Refusal drops candidates and preserves old capacities,
entries, timer/root IDs, serials and registration counters. Batch preparation
and commit permit no intervening collection or root mutation. Existing single-
root registration uses the same prepared hash-storage mechanism.

The three original failures now pass. Twenty-four finite cases cover complete
replacement/exact/one-byte-short admission, primitive-only root slots, dropped
and committed root batches, duplicate/empty batches, invalid foreign objects,
root/timer/task-serial boundaries, terminal refusal/refunds, spare-slot payload
refusal, Rust-held candidates after VM destruction, shared-document refusal and
released-byte reuse, object GC/cancellation and scalar timer-count admission.
The argument boundary uses thirty-two permitted primitive values and one finite
thirty-three-value refusal; it does not allocate a represented large resource.
The prior checkpoint observation now expects equal native bytes with/without a
primitive argument because both retained root vectors contain only the callback;
consumed argument slots contribute nothing through its Promise checkpoint.

Validation: **1101 passed, zero failed/ignored**, across 41 executables and 95
fixed selections: twenty-four new registration/root-batch cases and all 1077
prior selected controls. Existing finite public timer cases retain arguments,
ordering, interval self-cancellation, exception handling, navigation cleanup and
Promise-checkpoint behavior. Formatting, diff checks and offline workspace/all-
target Clippy with warnings denied pass. Cargo reuses the workspace target with
incremental compilation and dev/test debug artifact growth disabled. The same
nine earlier resource/infinite-loop skips remain. Public timer/finalization
selection names only finite cases; their large/resource/infinite cases and JSON
resource sweeps remain unselected. No network/service replay, physical exhaustion
probe, fresh full Test262, real Core/MCP replay, coverage result or integrated
acceptance is claimed.

Evidence: `/private/tmp/blueice-timer-registration-controls-20261017.py`,
`/private/tmp/blueice-timer-registration-commit-20261017.txt`,
`/private/tmp/blueice-timer-registration-audit-20261017.py`, and logs matching
`/private/tmp/blueice-timer-registration-{red,first,expanded,registration,entry-timer,root-table,fmt,clippy}-20261017.log`.
`target/timer-registration-source-20261017.json` records source/log/executable
hashes and staged/committed blob checks. Ordinary-library entry points expose
the same fixtures; no new coverage result is claimed. This is an M1z checkpoint.
Other managed/native scheduling and heap/host buffers, shared programs and wider
integrated acceptance remain open.

#### Promise queue storage follow-up (implementation checkpoint, 2026-10-17)

Acceptance: job deques and reaction vectors admit a complete detached candidate
while old capacity remains charged; incoming primitive copies have an owner.
Check actual reported capacity before publication. Stage await continuation maps,
frames and queue growth as one transaction, preserving old capacities, records
and serials on refusal. Commit without a second fallible queue admission. Keep
slot reuse, ordering, shared-document refunds and GC reachability intact.

- [x] Reproduce missing coexisting job/reaction/await admission with tiny cases.
      All three original cases accepted only the net-growth allowance and failed
      before implementation; all three now pass.
- [x] Implement owned replacements and atomic await staging; verify exact
      allowance, reuse/refusal, cleanup, shared owners and Promise GC behavior.
      All twenty-eight finite cases pass, including post-frame-drop refunds.
- [x] Run fixed offline controls/static checks, update evidence and commit with
      a detailed English message. Other buffers, shared programs and integrated
      acceptance remain open.

Job/reaction admission checks counts, leases the already-formed incoming payload
and prepares a complete detached replacement while the original container stays
charged. Reported capacity is admitted before publication. Spare slots need no
growth fee, wrapped deques preserve FIFO order and commitment moves existing
owners rather than cloning them. Independent document leases follow Rust-held
candidates and refund on rollback or destruction without keeping a VM/object
alive. Failure in infallible producers still latches the document abort before
later JavaScript work.

Async/module await validates its frame serial, admits counts and settled input/
origin ownership before cloning status. It then leases unowned frame payload,
complete queue replacement and complete continuation-map replacement together.
Only after every candidate fits are frame/map/queue entries and the serial
committed, with no second fallible queue admission. Refusal preserves old
capacities, entries and serials. Error wrappers reconcile after the rejected
frame drops its execution leases, making released bytes visible to sibling
realms even after terminal failure. Preparation/commit run no JavaScript or GC.

The three original net-only admission cases now pass. The expanded selection
exposed a rejected module frame leaving 437 bytes in another realm's account;
both async/module error exits now reconcile after frame destruction. All
twenty-eight finite groups pass. They cover initial/exact/one-byte-short/full
growth, payload-only reuse/refusal, wrapped FIFO order, partial/empty refunds,
Rust-held deque/map candidates, real queued-callback/reaction GC ownership,
async/module pending/fulfilled/rejected await, queue/map rollback and reported
capacity, joint exact allowance, spare slots, pre-copy/serial refusal, scalar
counts and failure-aware sibling observations. Fixtures use at most eight real
await frames with tiny values; represented scalar peers allocate no large buffer.

Other Promise-record/wait-table preallocation, retained hash-allocation ownership,
producer-side clones in other Promise algorithms, other host/scheduling buffers,
shared programs and integrated acceptance remain open.

Validation: **1129 passed, zero failed/ignored**, across 41 executables and 96
fixed selections: twenty-eight new queue/await groups and all 1101 prior selected
controls. Existing finite public Promise, timer, GC, cancellation and navigation
cases retain their behavior. Formatting, diff checks and offline workspace/all-
target Clippy with warnings denied pass. Cargo reuses the workspace target with
incremental compilation and dev/test debug artifact growth disabled. The same
nine earlier resource/infinite-loop skips remain. Public timer/finalization
selection names only finite cases; their large/resource/infinite cases and JSON
resource sweeps remain unselected. No network/service replay, physical exhaustion
probe, fresh full Test262, real Core/MCP replay, coverage result or integrated
acceptance is claimed.

Evidence: `/private/tmp/blueice-promise-queues-controls-20261017.py`,
`/private/tmp/blueice-promise-queues-commit-20261017.txt`,
`/private/tmp/blueice-promise-queues-audit-20261017.py`, and logs matching
`/private/tmp/blueice-promise-queues-{red,first,expanded-red,expanded,queue-storage,fmt,clippy}-20261017.log`.
`target/promise-queues-source-20261017.json` records source/log/executable hashes
and staged/committed blob checks. Ordinary-library entry points expose the same
fixtures; no new coverage result is claimed. This is an M1z checkpoint. Other
tables/buffers, retained-map ownership, shared programs and integrated acceptance
remain open.

#### Promise record allocation follow-up (implementation checkpoint, 2026-10-17)

Acceptance: admit complete native replacement before managed allocation, keep
candidate capacity unpublished until successful object creation and refund
detached storage on trace/managed refusal. Support GC pruning during allocation
without invalidating the prepared table or reallocating an admitted spare slot.
Reuse slots freed by collection and keep old live records traced. Restore table
reclamation on every exit; preserve normal Promise/GC behavior.

- [x] Reproduce net-only growth and premature capacity publication on managed/
      trace refusal with tiny finite cases.
      All three original cases failed, then passed after detached creation.
- [x] Implement detached record creation and GC-aware commitment; verify exact
      allowances, GC/reuse, refusal/refunds and shared-owner lifetimes.
      All twenty-three finite record creation cases pass, including cache
      preservation without GC and reclamation after actual collection.
- [x] Run fixed offline controls/static checks, update evidence and commit with
      a detailed English message. Host-wait/other table allocation, retained-map
      ownership, other buffers and integrated acceptance remain open.

Creation admits a complete detached native table before the managed allocation
or its trace can run. The original table stays installed until a new object
exists. During allocation, GC still removes dead records, while sparse-table
shrinkage is deferred so an already-admitted spare slot remains usable.
Commitment checks the pruned table: an available slot reuses its original
backing and drops any unnecessary candidate; a still-full table moves surviving
records into admitted storage. A final terminal-state check precedes publication.
No JavaScript runs during this handoff.

All exits restore the ordinary reclamation policy and reconcile ownership after
detached candidates drop. An empty cache is reclaimed only if collection really
ran, so trace refusal without GC preserves its previous capacity. The added
cache control initially exposed overly eager empty-table release (capacity three
became zero); collection-counter-aware cleanup closes that failure. Empty
release performs no allocation and remains valid after failure.

The three original cases failed, then passed. Twenty-three finite cases cover
exact/full/reported-capacity refusal before heap allocation, spare-slot reuse at
full scalar allowance, managed/trace refusal and sibling refunds, preserved
status/reactions, GC-created spare slots and discarded candidates, live result
edges through committed replacement, cache preservation/reclamation, empty
refusal/retry, normal post-creation collection, VM destruction, document isolation,
host-neutral policy attachment and custom-prototype protection. Fixtures use at
most four small Promise records and scalar peers without represented large
allocations. Host-wait/other table preallocation, retained hash-allocation
ownership, producer clones, other buffers and integrated acceptance remain open.

Validation: **1152 passed, zero failed/ignored**, across 41 executables and 97
fixed selections: twenty-three new record-creation cases and all 1129 prior
selected controls. Existing finite Promise, timer, GC, cancellation and navigation
cases retain their behavior. Formatting, diff checks and offline workspace/all-
target Clippy with warnings denied pass. Cargo reuses the workspace target with
incremental compilation and dev/test debug artifact growth disabled. The same
nine earlier resource/infinite-loop skips remain. Public timer/finalization
selection names only finite cases; their large/resource/infinite cases and JSON
resource sweeps remain unselected. No network/service replay, physical exhaustion
probe, fresh full Test262, real Core/MCP replay, coverage result or integrated
acceptance is claimed.

Evidence: `/private/tmp/blueice-promise-records-controls-20261017.py`,
`/private/tmp/blueice-promise-records-commit-20261017.txt`,
`/private/tmp/blueice-promise-records-audit-20261017.py`, and logs matching
`/private/tmp/blueice-promise-records-{red,first,expanded-first,cache-red,expanded,record-storage,fmt,clippy}-20261017.log`.
`target/promise-records-source-20261017.json` records source/log/executable hashes
and staged/committed blob checks. Ordinary-library entry points expose the same
fixtures; no new coverage result is claimed. This is an M1z checkpoint.
Host-wait/other tables, retained-map ownership, other buffers, shared programs and
integrated acceptance remain open.

#### Host-wait registration follow-up (implementation checkpoint, 2026-10-17)

Acceptance: prepare complete wait/record replacements and actual argument
storage before publishing a completion capability. Preflight root IDs before
managed allocation; admit the new object's root before consuming its token
serial. Preserve old capacities, records, root identities and serials on
preparation refusal, allowing ordinary GC pruning. Commit the Promise, wait and
root before invoking one host callback; preserve rejection, completion and
cancellation behavior. Unpublished managed objects remain collectible.

- [x] Reproduce net-only joint growth, premature token serial and wait-capacity
      publication on managed/root refusal using tiny in-memory callbacks.
      All three original cases failed: serials advanced and native wait capacity
      changed before registration could complete.
- [x] Implement staged host-wait ownership; verify input storage, preparation/
      rollback, GC, callback lifetime, cancellation and shared-document refunds.
      All twenty-four finite host-wait cases pass; rejection still checks the
      receiver before reporting a deferred argument conversion error.
- [x] Run fixed offline controls/static checks, update evidence and commit with
      a detailed English message. Retained-map ownership, other buffers, shared
      programs and integrated acceptance remain open.

Registration admits its shared count and root-ID range before managed
allocation. Actual converted primitive ownership and vector capacity retain an
independent document lease. Complete detached wait/record candidates coexist
with the original tables. Promise intrinsic setup and managed allocation may
collect, so the prepared record reuses the GC-aware handoff and admitted spare
slots. Root storage is prepared after the managed object exists. Token serial
acquisition is the final fallible step; root, Promise record and wait then commit
without allocation or collection before callback invocation.

Refusal refunds detached ownership without publishing a wait, record, root or
callback, or consuming a token serial. Existing native capacities and identities
remain installed apart from ordinary GC. Later root/serial refusal can leave
an unpublished managed object, which has no persistent root or native record
and is collectible. Converted inputs retain their lease through the callback
and release afterward; Rust-held preparations keep their document lease even
after VM destruction. Completion and cancellation retain their original
Promise/root lifecycle.

Unsupported arguments stay deferred language errors, preserving a rejected
Promise rather than introducing a synchronous throw. Receiver validation still
precedes that error. A further single failing case caught the changed precedence
from eager conversion; moving the deferred error after receiver validation
restores the previous message and releases the wait root without a callback.
The first expanded run also found three fixture setup failures: direct host
calls lacked finite constructor fuel and warm-up collection reclaimed an empty
cache. Corrected setup retains the original ownership assertions.

Twenty-four finite cases cover full versus net-only joint admission, exact
argument copies/slots, partial conversion and Rust-held lifetime, count/root-ID/
root-storage/serial/managed refusal, terminal refusal, sibling refunds,
exact/one-byte-short joint root allowance, GC pruning and live waits, atomic
publication and callback observations, callback/argument/receiver rejection,
completion and cancellation. New callbacks use only local captures and the
UserInteraction source; fixtures use at most eight pending waits, tiny strings
and scalar budget peers without represented large allocations.

Validation: **1176 passed, zero failed/ignored**, across 41 executables and 98
fixed selections: twenty-four new host-wait cases and all 1152 prior selected
controls. Existing finite Promise, timer, GC, cancellation and navigation cases
retain their behavior. Formatting, diff checks and offline workspace/all-target
Clippy with warnings denied pass. Cargo reuses the workspace target with
incremental compilation and dev/test debug artifact growth disabled. The same
nine earlier resource/infinite-loop skips remain. Public timer/finalization
selection names only finite cases; their large/resource/infinite cases and JSON
resource sweeps remain unselected. No network/service replay, physical exhaustion
probe, privileged validation, process killing, fresh full Test262, real Core/MCP
replay, coverage result or integrated acceptance is claimed.

Evidence: `/private/tmp/blueice-host-waits-controls-20261017.py`,
`/private/tmp/blueice-host-waits-commit-20261017.txt`,
`/private/tmp/blueice-host-waits-audit-20261017.py`, and logs matching
`/private/tmp/blueice-host-waits-{red,first,expanded-first,receiver-red,expanded,wait-storage,fmt,clippy}-20261017.log`.
`target/host-waits-source-20261017.json` records source/log/executable hashes and
staged/committed blob checks. Ordinary-library entry points expose the same
fixtures; no new coverage result is claimed. This is an M1z checkpoint. Other
control tables/buffers, retained-map ownership, producer clones, shared programs
and integrated acceptance remain open; M1u stays the latest accepted milestone.

#### Rooted scheduling registration follow-up (implementation checkpoint, 2026-10-17)

Acceptance: admit complete animation-frame, finalization-task and pending-
rejection replacement storage while old allocations remain charged. Prepare
the entry's root before publishing a queue/handle/serial. Root or native refusal
must preserve old capacities, roots and identities; infallible rejection
tracking must still latch terminal resource failure. Preserve registration order,
duplicate cleanup suppression, cancellation and GC lifetime.

- [x] Reproduce net-only growth and premature queue/serial publication with
      tiny finite fixtures at the actual producers.
      All three initial cases failed, then passed after detached preparation.
- [x] Implement detached queue/root preparation and verify exact allowances,
      refusal/refunds, GC, cancellation and shared ownership.
      All twenty-four finite registration groups pass.
- [x] Run fixed offline controls/static checks, update evidence and commit with
      a detailed English message. Browser-task storage, callback-entry scratch,
      retained-map ownership, other buffers and integrated acceptance stay open.

The three producers reuse complete detached vector/deque preparation while their
original allocations stay charged. Admission checks shared counts and root-ID
space, prepares requested/actual queue storage and then prepares the single
root's table storage. Cleanup acquires its document serial as the final fallible
step. Root and queue/handle publication then commit without allocation,
collection or JavaScript; spare slots require no fictitious growth fee.

Native/root refusal leaves original queue capacities, entries, roots, handles
and serials intact. Detached preparations refund on all exits, including terminal
failure. Infallible rejection tracking still latches document failure, after
the preparations have dropped and ownership can be reconciled. Cleanup keeps
its independently admitted registry snapshot through registration and continues
to skip queued or active registries in deterministic order.

Twenty-four finite groups cover complete versus net-only replacements, exact
and one-byte-short queue/root allowances, existing spare slots, shared scalar
count limits, root/serial/handle/foreign-ID refusal, terminal refusal, sibling
refunds and VM destruction. Lifecycle groups clear a callback's other owner,
verify the queued root across GC, then verify collection after dispatch or
cancellation. Cleanup roots likewise retain a ready registry through collection
and release after delivery. Rejection handling releases the pending root while
retaining its admitted empty cache; notification moves its existing vector and
reaction ownership once. New fixtures use at most five small registry/Promise
owners or frame callbacks and numerical peers without represented large buffers.

The first expanded run passed twenty-two groups and exposed two fixture setup
failures: installed host functions are globalThis properties rather than lexical
bindings. Clearing the actual callback property retains the original GC lifetime
assertions and makes both groups pass. All three original regressions also fail
before the change and pass after detached preparation.

Validation: **1207 passed, zero failed/ignored**, across 43 executables and 106
fixed selections: twenty-four new registration groups, seven exact finite
public frame/rejection cases and all 1176 prior selected controls. Public cases
verify frame ordering/nested registration, checkpoint cancellation/navigation
and rejection task/late-handler/navigation behavior. Existing finite timer,
finalization and Promise controls retain their behavior. Formatting, diff checks
and offline workspace/all-target Clippy with warnings denied pass.

Cargo reuses the workspace target with incremental compilation and dev/test
debug artifact growth disabled. The same nine earlier resource/infinite-loop
skips remain. Public timer/finalization/frame/rejection selection names only
finite cases; their large/resource/infinite cases and JSON resource sweeps remain
unselected. No network/service replay, physical exhaustion probe, privileged
validation, process killing, fresh full Test262, real Core/MCP replay, coverage
result or integrated acceptance is claimed.

Evidence: `/private/tmp/blueice-rooted-tasks-controls-20261017.py`,
`/private/tmp/blueice-rooted-tasks-commit-20261017.txt`,
`/private/tmp/blueice-rooted-tasks-audit-20261017.py`, and logs matching
`/private/tmp/blueice-rooted-tasks-{red,first,expanded-first,expanded,rooted-storage,fmt,clippy}-20261017.log`.
`target/rooted-tasks-source-20261017.json` records source/log/executable hashes
and staged/committed blob checks. Ordinary-library entry points expose the same
fixtures; no new coverage result is claimed. This is an M1z checkpoint. Complete
browser-task queue/root/state preparation, callback-entry scratch, retained-map
ownership, other buffers, shared programs and integrated acceptance remain open;
M1u stays the latest accepted milestone.

#### Browser-task preparation follow-up (implementation checkpoint, 2026-10-17)

Acceptance: admit complete browser-task replacements and incoming ownership
while old backing remains charged. Drag, timeout, host-completion, rejection
notification and FileReader initial/successor producers must publish only after
their native, managed and root preparation succeeds. Preserve old queue capacity,
root identities, serials, queued-completion flags and initial FileReader state
on preparation refusal, apart from ordinary GC and the language operation's
handled status. A consumed read stage retains its callback effects; successor
refusal publishes no new task/serial and releases its reader root. Keep queued
order, cancellation/navigation and completion/rejection/read lifecycle behavior.

- [x] Reproduce premature queue/serial publication and net-only admission at
      the actual producers using tiny finite fixtures.
      All three original cases failed, then passed after detached preparation.
- [x] Implement detached task preparation and staged FileReader state; verify
      joint allowances, refusal/refunds, GC and lifecycle behavior.
      All twenty-seven finite groups pass, including exact nested listener
      storage and epoch validation before resource preparation.
- [x] Run fixed offline controls/static checks, update evidence and commit with
      a detailed English message. Callback-entry scratch, retained-map ownership,
      other heap/host buffers, shared programs and integrated acceptance stay open.

All browser-task producers now use one prepared deque/payload owner. Complete
candidate storage coexists with the old deque until commitment; incoming
completion/text/rejection-vector payload has its own lease, and actual reported
capacity is checked before the next allocation. Rooted producers preflight root
IDs and prepare table storage before acquiring a serial. Timeout managed
allocation precedes that final serial step; a later refusal leaves its unpublished
signal unrooted and collectible. Existing spare slots add no queue storage fee.

Initial FileReader reads prepare their incoming managed state before root/serial
publication. The cloned state has an independent native lease for its boxed
header, actual listener slots/names and primitive strings while the old managed
owner remains installed. Heap preparation validates borrowed reference visitors,
protects the reader and incoming edges during GC, and admits managed growth.
Conservative write barriers are prepared before commitment. Root, staged boxed
state and task then publish without further allocation or collection; the native
clone lease refunds when managed ownership adopts its buffers. The existing
general state setter uses the same prepare/commit protocol without bulk reference
and protection vectors.

Completion flags and handled-rejection notification bits change only with an
admitted task. The language handled status remains committed when resource
preparation refuses; the infallible producer latches failure after refunds.
Unhandled notification still moves its existing vector/reaction owner once.
Successor reads prepare after callbacks, since those callbacks may fill the
queue or consume its document count. A refused successor preserves callback
effects, releases its reader root and consumes no enqueue serial or new capacity.
Existing cancellation removes matching entries in place.

Twenty-seven finite groups cover all six initial producer kinds, complete versus
net-only growth, exact/one-byte-short queue/root/input allowances, spare slots,
shared scalar counts, root/serial/managed/epoch/terminal refusal, original reader
fields and wait/notification flags, sibling refunds, VM destruction and Rust-held
leases. Small component cases check preparation protection through major GC,
new young Blob retention after commitment, dropped-state rollback and
unpublished standalone timeout collection. Completion delivery and read abort
retain their root lifecycle. Listener cases include the copied slot and name
buffer under exact and one-byte-short allowances.

The older spare-queue-slot read control initially failed because its zero-byte
allowance omitted the newly admitted reader copy. Funding exactly that independent
header preserves its original slot-reuse/root assertions. After twenty-five
groups passed, a zero-headroom epoch control reproduced a validation-order
regression: task admission masked the original catchable RangeError. Preflighting
the epoch before native/root preparation preserves that language error without
latching document failure. The final twenty-seven groups pass both corrections.

The first fixed replay exposed one obsolete successor allowance: funding only
net deque growth stopped its callback at sixteen entries, before the intended
thirty-two-entry requeue boundary. Model the complete coexisting old thirty-two
slots and new sixty-four-slot candidate, one byte short of that peak. This funds
the callback's replacements while retaining every original queue-capacity and
reader-root-release assertion. All twenty existing browser-task controls pass
with the revised scalar allowance.

Validation: **1238 passed, zero failed/ignored**, across 45 executables and 111
fixed selections: twenty-seven new browser-task groups, four exact finite public
read/timeout cases and all 1207 prior selected controls. Public cases cover read
abort, event/microtask ordering, callback read chaining and timeout behavior.
Existing finite timer, finalization, frame, rejection and Promise controls retain
their behavior. Formatting, diff checks and offline workspace/all-target Clippy
with warnings denied pass. Cargo reuses the workspace target with incremental
compilation and dev/test debug artifact growth disabled.

The same nine earlier resource/infinite-loop skips remain. New public read/
timeout selection names only finite cases; large/resource/infinite cases and
JSON resource sweeps remain unselected. No network/service replay, physical
exhaustion probe, privileged validation, process killing, fresh full Test262,
real Core/MCP replay, coverage result or integrated acceptance is claimed.

Evidence: `/private/tmp/blueice-browser-tasks-controls-20261017.py`,
`/private/tmp/blueice-browser-tasks-commit-20261017.txt`,
`/private/tmp/blueice-browser-tasks-audit-20261017.py`, and logs matching
`/private/tmp/blueice-browser-tasks-{red,first,reader-allowance-red,expanded-first,epoch-red,expanded,browser-controls-red,browser-controls-first,controls-first,browser-storage,fmt,clippy}-20261017.log`.
`target/browser-tasks-source-20261017.json` records source/log/executable hashes
and staged/committed blob checks. Ordinary-library entry points expose the same
fixtures; no new coverage result is claimed. This is an M1z checkpoint.
Callback-entry scratch, retained-map ownership, other heap/host buffers, shared
programs and integrated acceptance remain open; M1u stays the latest accepted
milestone.

#### Animation-frame entry follow-up (implementation checkpoint, 2026-10-17)

Acceptance: independently admit render handle snapshots and callback argument
slots. Validate starting fuel before native allocation; prepare without removing
a callback, registering extra persistent roots or publishing a timestamp.
Preserve unattempted work/context on refusal. Charge the first host instruction
before committing an attempt, transfer argument capacity into dispatch once and
release committed roots on every dispatch/reporting outcome. Keep snapshot order,
nested registration, cancellation/checkpoints and monotonic timestamp behavior.

- [x] Reproduce callback loss on zero fuel, snapshot and argument refusal with
      tiny finite callbacks. All three initial cases failed, then passed.
- [x] Implement admitted snapshots/entry preparation and verify exact allowances,
      refunds, root/argument lifetimes, error cleanup and timestamp semantics.
      All twenty-five finite entry groups pass.
- [x] Run fixed offline controls/static checks, update evidence and commit with
      a detailed English message. Other browser-task/event entry scratch,
      retained-map ownership, heap/host buffers and integrated acceptance stay open.

Render snapshots now use the existing admitted native TaskVector protocol. The
complete requested/reported handle storage retains an independent document
lease through every callback and checkpoint, without rooting a VM or copying
callback objects. Its identities keep registration order while callback or
microtask cancellation affects the live registry and new registrations wait for
the next snapshot. Empty snapshots need no backing storage.

Entry checks starting fuel before snapshot/argument allocation. A one-element
timestamp vector retains its own independent native lease while the registration
still owns its existing callback root. Preparation refusal consumes no callback,
root, timestamp or ambient fuel. The first host instruction commits an attempt,
consistent with timer/finalization delivery, then the prepared vector transfers
to ordinary dispatch once. Its slot charge does not survive into the following
Promise checkpoint. No extra persistent root IDs or root scratch vectors are
needed: the committed registration's root remains live through invocation and
page-exception reporting, then releases before the existing checkpoint.

Dispatch or reporting refusal still releases the consumed root; later unattempted
callbacks remain registered. Every render exit refunds detached storage and
reclaims an actually empty registry, including terminal failure. First-entry
preparation refusal preserves the old render timestamp; attempted work publishes
one monotonic timestamp for that opportunity. Successful empty renders still
advance time and checkpoint jobs. Invalid timestamps keep their language-error
precedence, while debugger pause or an already-canceled identity needs no new
snapshot/arguments or starting instruction.

Twenty-five finite groups cover zero-fuel refusal/retry, fuel-versus-native
precedence, exact/one-byte-short snapshot/argument admission, snapshot identity
stability under cancellation/new registration, empty/paused/canceled entries,
Rust-held leases after VM destruction, single argument ownership during dispatch
and its absence during checkpoints, partial attempted failure, page exceptions,
terminal callback/report refusal, monotonic fractional time and real collection
during a callback after clearing its other owner. New fixtures use at most four
callbacks, tiny primitive inputs, two small garbage objects and numerical peers
without represented large allocations. Other browser-task/event entry scratch,
retained-map ownership, heap/host buffers, shared programs and wider integrated
acceptance remain open.

Validation: **1268 passed, zero failed/ignored**, across 45 executables and 117
fixed selections: twenty-five new entry groups, five exact finite public frame
cases and all 1238 prior selected controls. Public cases cover callback/job
exceptions, invalid/monotonic time, empty-render checkpoints, timer/browser task
separation and debugger pause. Existing finite ordering, nested registration,
checkpoint cancellation, navigation, timer, finalization, read, rejection and
Promise controls retain their behavior. Formatting, diff checks and offline
workspace/all-target Clippy with warnings denied pass. Cargo reuses the workspace
target with incremental compilation and dev/test debug artifact growth disabled.

The same nine earlier resource/infinite-loop skips remain. Public frame selection
names only finite cases; its capacity/resource/infinite cases and JSON resource
sweeps remain unselected. No network/service replay, physical exhaustion probe,
privileged validation, process killing, fresh full Test262, real Core/MCP replay,
coverage result or integrated acceptance is claimed.

Evidence: `/private/tmp/blueice-frame-entry-controls-20261017.py`,
`/private/tmp/blueice-frame-entry-commit-20261017.txt`,
`/private/tmp/blueice-frame-entry-audit-20261017.py`, and logs matching
`/private/tmp/blueice-frame-entry-{red,first,expanded,frame-entry,fmt,clippy}-20261017.log`.
`target/frame-entry-source-20261017.json` records source/log/executable hashes
and staged/committed blob checks. Ordinary-library entry points expose the same
fixtures; no new coverage result is claimed. This is an M1z checkpoint. Other
browser-task/event entry scratch, retained-map ownership, heap/host buffers,
shared programs and integrated acceptance remain open; M1u stays the latest
accepted milestone.

#### Browser-task entry follow-up (implementation checkpoint, 2026-10-17)

Acceptance: classify live, stale and not-yet-due work without consuming it.
Active drag, FileReader, timeout, host-completion and rejection tasks must
validate starting fuel before removal; drag callback vector capacity must be
admitted while the queued text/root remain installed. Charge the first host
instruction before committing an active attempt. Preserve queued state on
preparation refusal; move drag text into dispatch once. Release roots and active
ownership on all committed outcomes, including failed successor publication.
Keep stale cleanup, future deadline/serial preservation and existing checkpoints.

- [x] Reproduce drag task loss on fuel/argument refusal and unbudgeted completion
      delivery using tiny finite operations. All three initial cases failed.
- [x] Verify a failing public turn and implement prepared entry/classification;
      cover refusal/refunds, no-op cleanup, payload handoff and GC/lifetimes.
      The public drag turn initially failed too; all twenty-eight finite entry
      groups now pass, including public drag/completion refusal.
- [x] Run fixed offline controls/static checks, update evidence and commit with
      a detailed English message. Per-event/batch staging, retained-map ownership,
      other heap/host buffers and integrated acceptance remain open.

Entry now borrows task identity/state to distinguish active work, stale cleanup
and future deadlines. Active tasks validate starting fuel before native
preparation or removal. Drag allocates an admitted empty one-slot argument vector
while queued text and its root retain their owners; it checks requested/actual
capacity under an independent document lease. Other task kinds add no argument
storage. The first host instruction commits an active attempt, then drag moves
the queued string into dispatch once without cloning or retaining a second
payload/vector charge through the Promise checkpoint.

Preparation refusal leaves active task/root/wait/notification state and ambient
fuel intact. A future timeout keeps its original queue position, deadline and
serial without sampling an external clock or starting an instruction segment.
Dead/aborted timeouts, stale reads/completions and already-handled rejection
notifications release their owned roots/buffers without event delivery or a
host instruction. Their established checkpoint remains, with ambient fuel
restored afterward. Active settlement/event/batch internals still follow the
committed-attempt failure contract; finer per-event/batch preparation stays open.

The outer entry boundary restores the caller's active-byte context and reconciles
ownership on every result, including reporting and successor-admission refusal.
An actually empty deque releases backing on errors too. Committed task effects
remain; failed dispatch consumes only that attempt, and its root/payload cleanup
still runs. Shared rejection-entry cleanup serves both delivered and stale
notification vectors.

Twenty-eight finite groups cover all five active task kinds, zero-fuel refusal
and native precedence, terminal refusal, exact/one-byte-short drag argument
admission, queued text before commitment, dropped preparations, Rust-held leases
after VM destruction, direct-call ownership comparison, no input charge at
checkpoints, retry and one-instruction settlement, committed failure cleanup,
future/dead/aborted timeouts, stale read/completion and handled-notification
cleanup, public turn refusal, caller context, reporting abort and real callback/
text GC lifetime after queue removal. Fixtures use one task at a time, tiny
primitive inputs, four small garbage objects and numerical peers without
represented large allocations.

Validation: **1300 passed, zero failed/ignored**, across 47 executables and 122
fixed selections: twenty-eight new task-entry groups, four exact finite public
cases and all 1268 prior selected controls. Public cases verify getAsString
ordering/snapshots, handled-notification suppression, microtask read abort and
future timeouts not blocking ready reads. Existing finite frame, timer,
finalization, read, rejection and Promise controls retain their behavior.
Formatting, diff checks and offline workspace/all-target Clippy with warnings
denied pass. Cargo reuses the workspace target with incremental compilation and
dev/test debug artifact growth disabled.

The same nine earlier resource/infinite-loop skips remain. Public selection
names only finite cases; large/resource/infinite cases and JSON resource sweeps
remain unselected. No network/service replay, physical exhaustion probe,
privileged validation, process killing, fresh full Test262, real Core/MCP replay,
coverage result or integrated acceptance is claimed.

Evidence: `/private/tmp/blueice-browser-entry-controls-20261017.py`,
`/private/tmp/blueice-browser-entry-commit-20261017.txt`,
`/private/tmp/blueice-browser-entry-audit-20261017.py`, and logs matching
`/private/tmp/blueice-browser-entry-{red,public-red,first,expanded,browser-entry,fmt,clippy}-20261017.log`.
`target/browser-entry-source-20261017.json` records source/log/executable hashes
and staged/committed blob checks. Ordinary-library entry points expose the same
fixtures; no new coverage result is claimed. This is an M1z checkpoint.
Per-event/batch staging, retained-map ownership, other heap/host buffers, shared
programs and integrated acceptance remain open; M1u stays the latest accepted
milestone.

#### Rejection-batch entry follow-up (implementation checkpoint, 2026-10-17)

Acceptance: prepare the complete native notification snapshot while its browser
task and Promise roots remain queued. Admit event attribute backing, names,
reason copies and source filenames before copying/publishing them. Preparation
refusal must preserve the batch, roots, serial, notification flags and task-entry
fuel. Delivery must recheck suppression after earlier callbacks/checkpoints,
transfer event/source ownership once and release roots/temporary storage on
every committed outcome.

- [x] Reproduce premature task consumption and absent snapshot admission with
      two tiny cases, including the public task-turn boundary: **0/2 passed**
      before implementation.
- [x] Implement a prepared batch with complete requested/actual storage checks,
      independent payload leases and event/console ownership handoff.
- [x] Complete fixed finite controls, static checks, evidence and a detailed
      English commit. This remains part of M1z; integrated gates stay open.

Entry prepares one complete vector while the original queue/root owners remain
installed. Eligible entries reserve eight event attributes without incremental
growth, retain both the event and preview reason copies, and copy source names
under a document lease. Suppressed entries preserve their positions without
payload copies. Starting-fuel validation still precedes native preparation;
the first host instruction still commits the task attempt. Stale-only cleanup
does not prepare snapshots or spend a host instruction.

Preparation splits independently owned event, source and preview leases from
the admitted vector. Event state keeps its native lease throughout managed
allocation, then transfers to the heap or drops on refusal. Console source
ownership stays leased through clock sampling and insertion, then refunds before
console admission. Per-entry payloads release after dispatch or suppression;
vector backing releases before the final event-turn checkpoint. Delivery
rechecks handled state after earlier callbacks and their microtasks. Managed
allocation/dispatch failures retain committed-attempt behavior and release all
notification roots and temporary native ownership.

The earlier zero-allocation non-drag preparation control continues to cover
completion, read and timeout work; rejection work now has its own complete
snapshot admission controls. Initial expanded controls reported **10/15 passed**:
fixtures needed public pre-task checkpoint fuel semantics, read-only account
observation, actual global-value lookup and sufficient reclaimed event storage
for the GC case. The GC fixture uses four small managed event objects. A further
managed-refusal case checks cleanup under a numerical heap limit.

The callback-suppression case also exposed an invalid aggregate-root-count
expectation: callbacks can create unrelated roots. It now clears page references
and proves both original notification Promises are collected. Sixteen groups
then passed. A final delivery-time timestamp refresh preserves event creation
semantics after earlier callbacks; its case uses a fixed sentinel without sleeps.
All seventeen focused finite groups pass.

Seventeen finite groups cover public/private admission refusal, complete exact/
one-byte-short allowances, dropped preparation/refunds, Rust-held ownership after
VM destruction, fuel precedence, stale/mixed notifications, callback-checkpoint
suppression and actual Promise collectibility, source/UTF-16 preservation,
managed/instruction failure cleanup, object-reason lifetime through real GC,
handled-event shape, console handoff and delivery timestamp refresh.

Validation: **1321 passed, zero failed/ignored**, across 47 executables and 127
fixed selections: seventeen new groups, four additional exact finite public
rejection cases and all 1300 prior selected controls. The public additions cover
handling during delivery, adoption/combinator order, uncanceled diagnostics
without invoking getters, and event constructor identity/validation. Formatting,
diff checks and offline workspace/all-target Clippy with warnings denied pass.
Cargo reuses the workspace target with incremental compilation and dev/test debug
artifact growth disabled. Ordinary-library entry points expose the same fixtures;
no fresh coverage result is claimed.

The selection remains offline and finite, with the same nine resource/infinite-
loop skips. Large/resource/infinite cases, JSON resource
sweeps, network/service replay, privileged validation, process killing, fresh
full Test262, real Core/MCP replay, coverage and integrated gates are unclaimed.

Evidence paths: `/private/tmp/blueice-rejection-batch-controls-20261017.py`,
`/private/tmp/blueice-rejection-batch-commit-20261017.txt`,
`/private/tmp/blueice-rejection-batch-audit-20261017.py`, logs matching
`/private/tmp/blueice-rejection-batch-*-20261017.log`, and
`target/rejection-batch-source-20261017.json`. This is an M1z checkpoint;
managed event allocation and other event/dispatch scratch, retained-map ownership,
other heap/host buffers, shared programs and integrated acceptance remain open.
M1u remains the latest accepted milestone.

#### Promise retained-allocation follow-up (implementation checkpoint, 2026-10-17)

Acceptance: Promise deletion must not refund a hash allocation merely because
reported insertion capacity falls. Record growth/replacement must retain the
old allocation until admitted storage takes over. GC must admit a complete
coexisting replacement before sparse-table compaction; refusal must keep the old
backing charged without poisoning successful collection. Empty backing must
release without replacement headroom. Preserve record/root collectibility,
managed preparation's capacity guard and mandatory admission's terminal abort.

- [x] Reproduce unadmitted GC compaction with fifteen tiny pending Promises and
      a numerical native peer: **0/1 passed** before implementation.
- [x] Introduce controlled Promise table allocation ownership and independently
      leased optional compaction with requested/reported capacity checks.
- [x] Complete fixed finite controls/static checks, update evidence and commit
      a detailed English message. Other maps and integrated gates stay open.

The live Promise map now records allocation bytes separately from HashMap's
reported insertion headroom. Reads borrow the map; controlled insert/retain/
replacement methods prevent ordinary producers from bypassing the allocation
ledger. Deleting records leaves the original allocation charged. Prepared record
publication updates the ledger only when the admitted candidate replaces it;
reusing a GC-freed slot keeps the old allocation owner. Existing capacity guards
still preserve cached backing through managed allocation and release a collected
empty table after failed creation.

GC drops dead payload, reconciles its refund, then prepares a complete smaller
table under an independent native lease. Sparsity uses retained allocation,
so tombstones cannot hide mostly empty backing by reducing reported headroom.
It checks requested/reported capacity,
moves survivors without cloning or further allocation, reconciles the smaller
live owner, then refunds the temporary lease. Missing headroom or candidate
allocation failure skips this optional compaction while keeping backing charged.
Candidates that do not reduce backing are discarded. Later GC can retry after
other owners release storage. Empty maps release directly, including when no
replacement headroom exists. Mandatory admission still latches a document abort;
optional reclamation never clears an existing abort.

Initial expanded controls reported **15/16 passed**. The remaining fixture
assumed a fully erased table still reported positive capacity; the fixed
28-scalar collision layout actually reports zero despite retaining its old
allocation. The corrected assertion checks precisely that distinction. Tests
use bounded scalar tables or fifteen tiny Promise objects, never represented
large allocations or resource-exhaustion execution.

Seventeen focused groups passed before final review found that the sparse-table
decision also needed the retained allocation ledger. A further fixed scalar case
proves low reported headroom cannot suppress reclamation of mostly empty backing.
All final-source checks below include that correction.

Eighteen finite groups cover tombstone capacity loss, repeated deletion,
retained empty backing, replacement/refund ownership, requested/reported native
refusal without poisoning collection, exact candidate transfer, already-terminal
and mandatory-failure behavior, real GC with no/exact/one-byte-short headroom,
empty backing release, retry after peer release, managed preparation's capacity
guard, disposal of unhelpful candidates and allocation-based sparsity selection.

Validation: **1339 passed, zero failed/ignored**, across 47 executables and 128
fixed selections: eighteen new groups and all 1321 prior selected controls.
Existing Promise record/queue, host-wait, await, timer, finalization and browser
controls retain their behavior. Formatting, diff checks and offline workspace/
all-target Clippy with warnings denied pass. Cargo reuses the workspace target
with incremental compilation and dev/test debug artifact growth disabled.

The fixed offline selection preserves the same nine explicit resource/infinite-
loop skips and all other finite-public-case exclusions. No large/resource/
infinite cases, JSON resource sweeps, network/service replay, privileged
validation, process killing, fresh full Test262, coverage result or integrated
acceptance is claimed. Other retained maps (host waits, async/module
continuations and provenance tables), event/dispatch scratch, other heap/host
buffers, shared programs and integrated gates remain open. M1u remains the
latest accepted milestone.

Evidence paths: `/private/tmp/blueice-retained-promises-controls-20261017.py`,
`/private/tmp/blueice-retained-promises-commit-20261017.txt`,
`/private/tmp/blueice-retained-promises-audit-20261017.py`, logs matching
`/private/tmp/blueice-retained-promises-*-20261017.log`, and
`target/retained-promises-source-20261017.json`. Ordinary-library entry points
expose the same fixtures; no fresh coverage result is claimed. This remains
an M1z checkpoint with wider acceptance open.

#### Host-wait/continuation map follow-up (implementation checkpoint, 2026-10-17)

Acceptance: external-wait and async/module continuation tables must retain their
admitted allocation cost across deletion. Prepared growth must publish through
the retained owner, empty tables must release after final completion/resumption
or orphan pruning, and optional sparse compaction must fund a complete coexisting
replacement. Managed host-wait allocation must preserve any staged table's old
capacity through GC. Reclaim deferred empty/sparse storage only after candidates
commit or drop; preserve roots, serials, cancellation and pending await behavior.

- [x] Reproduce empty async/module backing retained after the final resumption.
      The initial run reported **0/3 passed**: two valid resumption failures and
      an orphan fixture cleared too early and collected during setup. Retain its
      page references until suspension, then remove them before explicit GC.
- [x] Route all four core scheduling tables through retained allocation owners
      and guarded reclamation. The first three corrected cases pass; all twenty
      expanded finite groups passed on their first run.
- [x] Complete fixed offline controls/static checks, evidence and a detailed
      English commit. Remaining maps and integrated gates stay open.

External waits and async/module continuations now use the same controlled map
as Promise records. Deletion changes insertion headroom without refunding live
backing. Prepared growth moves survivors into admitted replacement storage and
updates the live ledger. Final wait completion or await resumption releases empty
backing directly; GC prunes orphan frames, then reconciles dead payload before
optional compaction of all four scheduling tables. Each successful replacement
retains its independent candidate lease through live-owner reconciliation.

Host-wait registration stages native wait and Promise candidates before managed
allocation can collect. The existing preparation guard now defers replacement
of every staged scheduling map. After real GC, registration completion (success
or failure) reclaims any deferred empty/sparse backing after its candidates have
committed or dropped. Refusal without collection keeps existing caches. Optional
compaction remains nonterminal and may retry later; cleanup releases empty
backing even after terminal failure without clearing that failure.

Twenty finite groups cover final async/module resumption, orphan GC, host wait
completion/root collectibility, exact/no/one-byte-short compaction allowance,
managed registration GC with a sparse staged wait table, failed managed creation,
root/serial preservation on mandatory refusal, independently leased candidates
after VM destruction, cancellation exactly once while backing remains charged,
partial and chained awaits, all-table guards, terminal cleanup, deferred orphan
cleanup, live-frame compaction/retry and deterministic tombstone removal. New
fixtures use up to seven live waits/awaits, four small garbage objects, twenty-
eight scalar keys and numerical peers; no external operation is started.

Validation: **1359 passed, zero failed/ignored**, across 47 executables and 129
fixed selections: twenty new groups and all 1339 prior selected controls.
Existing Promise record/queue, host-wait, async/module await, timer, finalization,
browser and ownership controls retain their behavior. Formatting, diff checks
and offline workspace/all-target Clippy with warnings denied pass. Cargo reuses
the workspace target with incremental compilation and dev/test debug artifact
growth disabled.

The same nine explicit resource/infinite-loop skips and other finite-public-case
exclusions remain. No large/resource/infinite cases or JSON resource sweeps are
selected. Other retained maps, provenance/import tables, event/dispatch scratch,
other heap/host buffers, shared programs and integrated gates remain open. No
network/service replay, physical exhaustion, privileged validation, process
killing, fresh full Test262, coverage result or integrated acceptance is claimed.

Evidence paths: `/private/tmp/blueice-retained-awaits-controls-20261017.py`,
`/private/tmp/blueice-retained-awaits-commit-20261017.txt`,
`/private/tmp/blueice-retained-awaits-audit-20261017.py`, logs matching
`/private/tmp/blueice-retained-awaits-*-20261017.log`, and
`target/retained-awaits-source-20261017.json`. Ordinary-library entry points expose
the same fixtures; no fresh coverage result is claimed. This remains an M1z
checkpoint; M1u stays the latest accepted milestone.

#### Import-registration storage follow-up (implementation checkpoint, 2026-10-17)

Acceptance: admit complete coexisting eager waiter vectors and new-key tables
before publication. Deferred dependencies must admit their input vector/name
buffers and full set backing before moving/deduplicating names; publish their
waiter vector only after complete replacement preparation. Refusal must preserve
live keys, targets, capacities and counts. Retain table/set allocations across
deletion, fund optional compaction and release empty backing without admission.
Keep existing root/namespace and count-limit behavior.

- [x] Reproduce net-only vector/table admission using tiny registrations and
      numerical peers: **0/2 passed** before implementation.
- [x] Prepare complete registration owners and add retained import/dependency
      ledgers with optional leased compaction. The first two cases and all
      twenty expanded finite groups pass.
- [x] Complete fixed offline controls/static checks, evidence and a detailed
      English commit. Import delivery batches remain a separate open dependency,
      including temporary rooting and scratch storage.

An existing eager key drops its temporary lookup string before preparing a
complete vector replacement. A new key independently leases its name, complete
table candidate and target-vector backing before committing the table/entry.
No fallible work or JavaScript runs between commit and publication. Deferred
registration leases the complete incoming vector/name payload and requested/
reported set capacity before moving names once; duplicates release their buffers.
After freeing input metadata, the prepared set keeps its independent lease while
the complete outer waiter-vector replacement is prepared. Publication adopts
the dependency owner and refunds staging without another admission.

Import map and dependency-set ledgers retain allocation cost when deletions
reduce reported insertion headroom. Reclamation reconciles removed payload,
prepares smaller map/set/vector candidates under independent leases, moves live
entries, reconciles their new owner, then refunds staging. Missing temporary
headroom skips optional compaction without poisoning the document. Empty backing
releases directly, including after terminal failure. Scheduling-map GC
reclamation includes import storage while preserving managed-preparation guards.

The older deferred spare-slot allowance now inspects one fixed loader result
before applying the numerical peer and funds input-vector metadata plus the
complete set peak. Its queued import still runs through ordinary job entry,
with the same target/count/capacity assertions. Raw import-map comparisons
borrow the controlled map without changing their equality checks.

The first fixed run exposed one older rooted-storage fixture assumption:
**23/24 passed**. Reusing a root-table tombstone can increase reported insertion
capacity, so filling once against an initial capacity left the table short of
its intended full shape. The fixture now rechecks current headroom within a
fixed 128-step/127-slot cap. Its full-table, exact and one-byte-short root
admission/refund assertions are unchanged; all twenty-four controls now pass,
with no production root behavior changes.

Twenty finite groups cover complete/exact/short eager candidates, spare-key
reuse, dependency input/set peaks, deduplication, refunds and Rust-held leases,
deferred-vector replacement, shared-count precedence, optional set/vector/map
compaction and retry, terminal empty cleanup, and actual eager/deferred target
and namespace collectibility. Inputs use a few keys/dependencies, eight tiny
registrations at most and numerical peers without represented large allocations.

Validation: **1379 passed, zero failed/ignored**, across 47 executables and 130
fixed selections: twenty new groups and all 1359 prior selected controls. The
existing eager/deferred job, namespace-root refusal, fulfillment/rejection and
collectibility controls pass with the complete staging allowance. All rooted,
Promise, await, host-wait, timer, finalization and browser controls retain their
behavior. Formatting, diff checks and offline workspace/all-target Clippy with
warnings denied pass. Cargo reuses the workspace target with incremental
compilation and dev/test debug artifact growth disabled.

The same nine explicit resource/infinite-loop skips and all other finite-public-
case exclusions remain. No large/resource/infinite cases or JSON resource sweeps,
network/service replay, privileged validation, process killing, fresh full
Test262, coverage result or integrated acceptance is claimed.
Delivery batches and their temporary rooting/scratch, Promise provenance and
other retained maps, heap/host buffers, shared programs and integrated gates
remain open. M1u stays the latest accepted milestone.

Evidence paths: `/private/tmp/blueice-import-storage-controls-20261017.py`,
`/private/tmp/blueice-import-storage-commit-20261017.txt`,
`/private/tmp/blueice-import-storage-audit-20261017.py`, logs matching
`/private/tmp/blueice-import-storage-*-20261017.log`, and
`target/import-storage-source-20261017.json`. Ordinary-library entry points expose
the same fixtures; no fresh coverage result is claimed. This remains an M1z
checkpoint with wider acceptance open.

#### Import-delivery ownership follow-up (implementation checkpoint, 2026-10-17)

Acceptance: prepare complete native delivery entries, reference/root vectors,
copied rejection payload/source and all temporary roots before changing waiter
membership or dependency progress. Native/root refusal preserves both eager and
deferred queues. Keep every target and namespace/reason alive across callbacks;
consume only the current attempted entry, preserving the unattempted suffix on
failure. Release temporary roots and storage on every result and transfer leased
rejection inputs to retained Promise/origin ownership without charging them twice.

- [x] Reproduce native fulfillment/rejection and root-ID refusal with tiny
      fixtures: **0/3 passed** before implementation.
- [x] Prepare complete delivery ownership and replace the old fulfillment panic
      with runtime error propagation. All nineteen finite groups pass.
- [x] Finish the fixed offline controls/static checks, evidence and detailed
      English commit. Common settlement/reaction clone scratch stays open.

Preparation counts ready deferred targets and eager targets in their existing
delivery order, admits all entry/reference/root backing plus rejection value/
filename copies, checks actual capacities, then prepares and publishes the whole
root batch. Root-ID/storage refusal publishes no prefix or consumed ID. Eager
namespace construction keeps its independent module roots through preparation
and dispatch. No JavaScript, collection or dependency mutation occurs during
native snapshot preparation.

After preparation, partial dependency completion releases obsolete names using
the existing optional reclamation policy, including when no target is ready.
Each ready waiter remains queued until its own resolution/rejection attempt
begins. A getter, instruction refusal or reaction admission error consumes that
attempt only; subsequent targets retain their original queue edges and pending
dependency marker. All prepared targets and namespace/reason objects stay rooted
across earlier callbacks and collection. Callable namespace exports transfer
ownership to ordinary thenable jobs. Dispatch propagates runtime errors, checks
latched scheduling failure, preserves ambient fuel and always releases temporary
roots and native snapshots after dispatch stops.

Each rejection entry splits independently leased value and filename ownership
from the batch. The origin override and Promise record adopt those inputs before
their staging leases release. This handoff does not complete the broader common
settlement/reaction clone and origin-scratch accounting work.

The first implementation run was **2/3 passed**: the root-refusal fixture took
its expected counter snapshot before its test-only ID-limit setter changed that
counter. Capturing the baseline after the setter fixes that assertion. An
expanded-fixture compile also caught a helper/local-name collision; the helper
was renamed before the successful nineteen-group run.

Nineteen finite groups cover native/exact-short and atomic root refusal with
retry, partial/unrelated completion, abandoned preparation, Rust-held snapshots
after VM destruction, rejection input/source transfer, direct-rejection ownership
comparison, committed native/instruction/reaction errors with retained suffixes,
real getter-triggered GC, explicit root ownership after original queues clear,
eager exports, callable `then` job handoff and object-reason collectibility.
Fixtures use a few targets, four small garbage objects at most and numerical
admission peers; they start no external operation or service.

Validation: **1398 passed, zero failed/ignored**, across 47 executables and 131
fixed selections: nineteen new groups and all 1379 prior selected controls.
Existing eager/deferred imports, Promise/await, host-wait, timer, finalization,
browser and ownership controls retain their behavior. Formatting, diff checks
and offline workspace/all-target Clippy with warnings denied pass. Cargo reuses
the workspace target with incremental compilation and dev/test debug artifact
growth disabled.

The same nine explicit resource/infinite-loop skips and other finite-public-case
exclusions remain. No large/resource/infinite cases, JSON resource sweeps,
network/service replay, privileged validation, process killing, fresh full
Test262, coverage result or integrated acceptance is claimed. Common Promise
settlement/reaction scratch, provenance/other maps, heap/host buffers, shared
programs and integrated gates remain open. M1u stays the latest accepted milestone.

Evidence paths: `/private/tmp/blueice-import-delivery-controls-20261017.py`,
`/private/tmp/blueice-import-delivery-commit-20261017.txt`,
`/private/tmp/blueice-import-delivery-audit-20261017.py`, logs matching
`/private/tmp/blueice-import-delivery-*-20261017.log`, and
`target/import-delivery-source-20261017.json`. Ordinary-library entry points
expose the same fixtures; no fresh coverage result is claimed. This remains an
M1z implementation checkpoint with wider acceptance open.

#### Promise settlement fan-out follow-up (implementation checkpoint, 2026-10-17)

Acceptance: retain pending Promise status and its entire reaction vector until
the complete fan-out has admitted job counts, native snapshots, copied values/
selected handlers/capabilities/source metadata and any queue replacement. Native
refusal must preserve status, reactions and queued jobs without panicking native
resolvers. Successful publication keeps existing job order and hands ownership
to retained records/jobs without charging transferred inputs twice.

- [x] Reproduce premature status publication under input, fan-out and rejection
      source refusal using three tiny fixtures: **0/3 passed** before the fix.
- [x] Prepare complete fan-out ownership; all three reproductions now pass.
- [x] Complete expanded finite cases, fixed offline controls/static checks,
      evidence and the detailed English commit.

Preparation validates the receiver/status before copying provenance or admitting
work. It checks the whole job count, retains or acquires the incoming value's
independent lease, and admits a complete snapshot vector plus all selected
reaction/value/source copies before allocation. Borrowed source metadata gives
the initial filename allowance; actual capacities and cloned payloads are checked
before the next allocation. An explicit unknown-source override still wins over
the active frame. Source preparation and per-job accounting take linear time
without JavaScript, managed allocation or collection.

Queue preparation now accepts a complete additional count, reuses existing spare
slots, or leases a full coexisting replacement. Zero additional jobs requires no
queue allocation. Batch growth preserves the capacity reached by repeated single
appends, including a non-power-of-two existing capacity. All original reaction
edges remain installed during preparation;
publication moves the admitted record origin/value, drops the old reaction
vector, appends every prepared job in order, frees snapshot metadata, transfers
input ownership and reconciles refunds without another fallible admission.

Preparation failure drops all candidates/copies, retains the pending record and
its reactions, and latches the document abort. Native tracked-settlement callers
remain infallible for valid inputs; invalid receivers and pending-as-final-status
still return TypeError. Existing settled records remain a no-op. Notification
tracking runs after successful settlement as a separate committed producer; a
notification-root refusal retains the completed status and latches failure.
Ambient fuel and root-ID space are unchanged by native preparation.

The existing import-delivery failure control now checks this stronger nested
boundary: the outer current import attempt is consumed, its pending Promise and
reaction stay unchanged when settlement preparation refuses, and the remaining
import suffix stays queued. Its earlier expectation of a rejected first record
described the pre-transaction settlement behavior; suffix protection is retained.

The first expanded-fixture compile found ambiguous UTF-16 string conversions and
an incorrect checkpoint API name (six diagnostics). After fixing the comparisons
and using the existing host checkpoint API, **21/22 groups passed**. The remaining
refund assertion mixed aggregate native usage with VM-field usage, omitting 224
bytes of unchanged heap-native storage. It now compares the VM ownership delta
against the aggregate account and keeps the separate post-destruction peer/refund
assertions. Valid-receiver lookup also constructs its error lazily, avoiding an
unused temporary error string on the ordinary settlement path.

The first full fixed run was **1418 passed / 2 failed**. Two older rejected-await
fixtures expected the previous geometric queue shape: five settled reactions
followed by three fixed jobs filled eight slots. Initial batch reservation used
five slots and then grew to ten. Batch growth now preserves repeated single-entry
capacity growth; all older fixture assertions remain unchanged. The new five-
reaction case additionally checks an eight-slot queue. The complete first run
is archived separately before revalidation.

Twenty-two finite groups cover initial/full/one-byte-short preparation, complete
queue growth and spare slots, all five reaction-job variants, native/source/value
ownership transfers and refunds, snapshots held after VM destruction, existing
FIFO/wrapped queues, scalar whole-batch quotas, invalid/already-settled records,
native-frame/unknown-override precedence, infallible refusal with ambient fuel
preserved, committed notification-root refusal, public callback/async order and
actual job-root collectibility. Fixtures use up to five reactions, a handful of
jobs/objects and numerical admission peers; they start no external operation.

Validation: **1420 passed, zero failed/ignored**, across 47 executables and 132
fixed selections: twenty-two new groups and all 1398 prior selected controls.
Both older rejected-await fixtures pass with their original assertions. Existing
Promise/await, import, host-wait, timer, finalization, browser and ownership
controls retain their behavior. Formatting, diff checks and offline workspace/
all-target Clippy with warnings denied pass. Cargo reuses the workspace target
with incremental compilation and dev/test debug artifact growth disabled.

The same nine explicit resource/infinite-loop skips and other finite-public-case
exclusions remain. No represented large allocations, resource/infinite probes,
JSON resource sweeps, network/service replay, privileged validation, process
killing, fresh full Test262, coverage result or integrated acceptance is claimed.
Already-settled reaction/origin producers, provenance/other maps, wider heap/host
buffers, shared programs and integrated gates remain open. M1u stays the latest
accepted milestone.
At that checkpoint, already-settled `perform_promise_then` still marked the
receiver handled and cloned its retained value/origin before queue admission.
The following Promise Then registration checkpoint addresses that separate boundary.

Evidence paths: `/private/tmp/blueice-promise-settlement-controls-20261017.py`,
`/private/tmp/blueice-promise-settlement-commit-20261017.txt`,
`/private/tmp/blueice-promise-settlement-audit-20261017.py`, logs matching
`/private/tmp/blueice-promise-settlement-*-20261017.log`, and
`target/promise-settlement-source-20261017.json`. Ordinary-library entry points
expose the same fixtures; no fresh coverage result is claimed. This remains an
M1z implementation checkpoint with wider acceptance open.
The first fixed-run logs are retained under
`/private/tmp/blueice-promise-settlement-controls-attempt-20261017/`; its summary
is `/private/tmp/blueice-promise-settlement-initial-controls-attempt-20261017.json`.

#### Promise Then registration follow-up (implementation checkpoint, 2026-10-17)

Acceptance: prepare complete pending/settled Then registration storage before
changing handled state, removing a pending rejection notification/root or queuing
late-handling notification work. Admit incoming owned fields and settled value/
source copies before cloning. Refusal preserves record status, queues, handled/
notification bits and roots. Keep valid internal callers infallible under resource
abort, with brand errors unchanged. Successful queue publication precedes the
handled-state transition and its separate committed notification producer.

- [x] Reproduce settled-payload, pending-growth and pending-rejection/root loss
      under tiny numerical refusal controls: **0/3 passed** before the fix.
- [x] Prepare registration ownership and move generic pending reaction handling
      after successful queue publication. All three initial cases pass.
- [x] Complete expanded finite groups, fixed offline controls/static checks,
      evidence and the detailed English commit.

Preparation validates membership before admission and keeps the original record
installed. Pending registrations retain both incoming handlers; settled ones
explicitly drop the unused handler and move the selected handler/capability.
Job/reaction count admission precedes an independent lease covering those owned
fields and every retained value/filename copy. Only then does settled work clone
the original record's value and provenance. The existing prepared await-work
boundary checks actual input payload and prepares complete vector/deque growth
without JavaScript or managed collection; registration uses no saved frame.

Publication adopts the prepared reaction/job and transfers input ownership before
marking the source handled. The generic pending-reaction producer likewise marks
handled only after its queue producer succeeds. A mandatory refusal latches the
document abort, drops all candidates/copies and retains original status, handled
state, notification ownership, root IDs and ambient fuel. Invalid receivers keep
their existing TypeError; valid internal callers do not panic on admission refusal.

After queue commitment, handling removes a still-pending rejection notification
and releases its root. Already-notified sources prepare their follow-up browser
task through the existing producer. Follow-up root/serial/native refusal keeps
the committed job/handled state and retains the notification bit until event
publication can commit; the resource failure remains terminal. This is separate
from a refused Then registration, which changes none of those owners.

The first expanded run was **20/22 passed**. One exact-admission fixture used
UTF-16 string length instead of its actual vector capacity; it now funds the
measured incoming buffers before moving them. The GC fixture initially omitted
the notification task created at checkpoint completion. It now proves the new
task keeps its target/reason alive, drains that one suppressed in-memory task,
then checks collection after the final owner releases. No production behavior
was relaxed to satisfy those assertions.

Twenty-two finite groups cover original and generic-producer refusal, exact/
short admission, complete/spare queue backing, selected/both-handler and source
ownership, refunds, independently leased preparations after VM destruction,
scalar job/reaction quotas, invalid/already-handled receivers, committed late
notification success/refusal, pending-root release, actual job/task roots through
GC and public species/catch callback order. Fixtures use a few promises/jobs and
numerical peers, with one bounded suppressed in-memory task when proving its
root release; they start no external operation or service.

Validation: **1442 passed, zero failed/ignored**, across 47 executables and 133
fixed selections: twenty-two new groups and all 1420 prior selected controls.
Existing Promise/await, import, host-wait, timer, finalization, browser and ownership
controls retain their behavior. Formatting, diff checks and offline workspace/
all-target Clippy with warnings denied pass. Cargo reuses the workspace target with incremental
compilation and dev/test debug artifact growth disabled.

At that checkpoint, other await/generator producers still marked handled before
frame/request preparation. The following continuation await checkpoint addresses
that separate boundary. Other input/origin producers, provenance/other maps,
heap/host buffers, shared programs and integrated gates remain open. The same
nine explicit resource/infinite-loop skips and other
finite-public-case exclusions remain. No represented large allocations,
resource/infinite probes, JSON resource sweeps, network/service replay,
privileged validation, process killing, fresh full Test262, coverage result or
integrated acceptance is claimed. M1u stays the latest accepted milestone.

Evidence paths: `/private/tmp/blueice-promise-reaction-controls-20261017.py`,
`/private/tmp/blueice-promise-reaction-commit-20261017.txt`,
`/private/tmp/blueice-promise-reaction-audit-20261017.py`, logs matching
`/private/tmp/blueice-promise-reaction-*-20261017.log`, and
`target/promise-reaction-source-20261017.json`. Ordinary-library entry points
expose the same fixtures; no fresh coverage result is claimed. This remains an
M1z implementation checkpoint with wider acceptance open.

#### Continuation await preparation follow-up (implementation checkpoint, 2026-10-17)

Acceptance: pre-admit retained status/source copies and complete work storage for
module/async-function awaits and generator delegate/return/yield awaits. Publish
the frame/control resumption owner and queue entry before handled state changes.
Preparation refusal preserves notification bits, pending rejection entries/roots,
queue/map storage and serials. Committed notification failure retains published
owners and surfaces the terminal error. Keep language exceptions, asynchronous
job ordering, original request completions and ordinary cleanup behavior.

- [x] Refine real generator fixtures and reproduce frame/generator notification
      loss under tiny refusal controls: **0/3 passed** on corrected old code.
- [x] Share typed continuation metadata and status/queue preparation across all
      five remaining await producers. All three initial cases pass.
- [x] Complete expanded finite groups, fixed offline controls/static checks,
      evidence and the detailed English commit.

A scalar continuation descriptor distinguishes module, async-function, generator
yield and generator delegate work. Count/value/source admission precedes status
cloning; conversion moves the prepared value/origin into its reaction/job without
copying again. The existing work preparation boundary admits complete queue
growth and unowned frame storage. No JavaScript or managed collection runs during
native preparation. Module/async frames prepare serial and map storage before
publishing their saved frame and work; generators enter this boundary after
PromiseResolve and any language callbacks have completed.

Queue publication precedes handling. The shared completion method checks both
the queue handoff and notification cleanup/follow-up, surfacing late terminal
failure while retaining committed work. Native/heap/fuel errors from generator
await registration propagate before capturing language throw metadata. A refused
return operand restores the scheduler status without converting the original
queued return request into a throw. Language PromiseResolve failures still follow
the existing catch/finally path. Earlier request/control changes and user callback
side effects remain committed; this checkpoint does not roll back PromiseResolve.

The first red fixture attempt had two ownership reproductions and one setup
failure: ordinary async `yield` first enters the compiler's await continuation.
The corrected generator fixture enters AsyncGeneratorYield through completed-
generator `return`, and operand return-await through a generator suspended at
yield. Those three corrected cases all reproduce early handled publication before
the fix and all pass afterward. The initial setup log is preserved separately.

The first expanded run was **22/23 passed**, including actual generator-job and
notification-root GC. Its delegate fixture omitted `return` before `yield*`:
normal delegate completion was correctly discarded by the expression statement.
The fixture now explicitly returns the delegate's value. A further committed-
notification refusal case checks that frame, work and consumed serial survive
the late abort while the notification bit/root-ID state remains unpublished.

The first fixed run passed all **1466** selected cases. Its subsequent Clippy
check found two now-unused release reaction append helpers: every production
Then/await path publishes prepared work. Those direct append helpers are now
limited to test/coverage builds for the existing fixtures; the remaining job-gate
contract distinguishes prepared publication. The complete first run, failed
static log and exact source snapshots are archived before revalidation.

Twenty-four finite groups cover the corrected entry reproductions, frame serial/
map/count preparation and positive pending/settled publication, every descriptor/
status, exact/short copy admission, scalar quotas, spare slots, independent native
ownership after VM destruction, abandoned preparation and original return request
preservation. Public cases exercise async/module ordering, delegate return/throw,
rejected yield/return through catch/finally, and committed notification refusal.
Actual major collections prove generator/target/result/reason survival through
job delivery and final collectibility after the one suppressed notification task
releases its root. Fixtures use a few frames/promises/objects and numerical peers;
no external operation starts.

Validation: **1466 passed, zero failed/ignored**, across 47 executables and 134
fixed selections: twenty-four new groups and all 1442 prior selected controls.
Existing Promise/await, import, host-wait, timer, finalization, browser and ownership
controls retain their behavior. Formatting, diff checks and offline workspace/
all-target Clippy with warnings denied pass. Cargo reuses the workspace target with incremental
compilation and dev/test debug artifact growth disabled.

Other input/origin producers, retained provenance/other maps, wider heap/host
buffers, shared programs and integrated gates remain open. The same nine explicit
resource/infinite-loop skips and other finite-public-case exclusions remain.
No represented large allocations, resource/infinite probes, JSON resource sweeps,
network/service replay, privileged validation, process killing, fresh full
Test262, coverage result or integrated acceptance is claimed. M1u stays the latest
accepted milestone.

Evidence paths: `/private/tmp/blueice-continuation-await-controls-20261017.py`,
`/private/tmp/blueice-continuation-await-commit-20261017.txt`,
`/private/tmp/blueice-continuation-await-audit-20261017.py`, logs matching
`/private/tmp/blueice-continuation-await-*-20261017.log`, and
`target/continuation-await-source-20261017.json`. Ordinary-library entry points
expose the same fixtures; no fresh coverage result is claimed. This remains an
M1z implementation checkpoint with wider acceptance open.
The initial fixed-run/static logs and source snapshots are retained under
`/private/tmp/blueice-continuation-await-static-attempt-20261017/`, with the summary
in `/private/tmp/blueice-continuation-await-initial-static-attempt-20261017.json`.

#### Finally-thunk provenance storage follow-up (implementation checkpoint, 2026-10-17)

Acceptance: retain the weak finally-state provenance table's allocation cost
across deletion. Admit complete replacement/source storage before creating a
managed thunk state; GC may prune dead entries while preserving staged capacity.
Commit only after state/property preparation succeeds, with no fallible table
admission after publication. Compact through admitted optional candidates and
release empty backing without new admission. Pre-admit finally capture and
thrower filename clones and transfer their ownership without charging twice.

- [x] Reproduce guarded GC shrinkage, entry publication before native refusal
      and a thrower publishing a refused filename copy: **0/3 passed** before
      implementation.
- [x] Add a retained table ledger, staged registration and admitted source-copy
      handoffs. The first run was **2/3 passed** because one fixture also counted
      earlier callback/PromiseResolve work in its thunk-allocation snapshot.
- [x] Complete expanded finite groups, fixed offline controls/static checks,
      evidence and the detailed English commit.

The weak map now owns a retained hash-allocation ledger. Deletion returns removed
filename payload while keeping table backing charged. GC prunes unreachable
state keys and then uses the guarded scheduling-map reclamation boundary.
Optional compaction leases a complete smaller table, moves survivors, reconciles
their new owner and refunds staging; unavailable headroom remains nonterminal.
Empty backing releases even after terminal failure. Keys never become heap roots.

Finally registration captures current provenance under an independent filename
lease and prepares the complete table candidate before managed state/value-field
creation. The shared map guard survives nested allocation and preserves admitted
slots while GC prunes entries. Commit reuses a slot freed by GC or adopts the
prepared replacement, publishes the state/origin pair, then transfers ownership.
All candidates commit or drop before post-GC deferred reclamation; every result
restores the caller's guard and root stack. A fulfilled value thunk needs no map
entry. Callback/PromiseResolve side effects before this boundary remain committed.

A borrowed current-source byte preflight is shared with settlement. Explicit
unknown-source overrides still win over a named active frame. Outer finally
capture admits its clone before the callback scope adopts it; thrower reads
pre-admit the stored filename and move it to pending throw provenance before
refunding staging. Refused reads change no pending origin/epoch. Other value/
argument and origin producers remain separate open dependencies.

The first expanded fixture refinement preserves the public operation's refused
entry/count/capacity and stack assertions, and separately calls the native thunk
allocation boundary to prove it allocates no managed state on table refusal.
The earlier broad heap comparison also counted legitimate allocations preceding
table preparation, so it could not establish that narrower ownership invariant.

Twenty-four finite groups cover original/refined refusal boundaries, retained
backing after deletion, optional exact/short/no-headroom compaction and retry,
terminal empty release, full/spare managed-GC handoffs, managed creation failure,
nested guard restoration, independently leased Rust preparations after VM drop,
current/stored source admission, unknown-source precedence, function/state/reason
GC ownership and public finally cleanup ordering/rejection. GC fixtures use at
most eight state objects and four small garbage events; numerical peers perform
no represented large allocation and no external operation starts.

Validation: **1490 passed, zero failed/ignored**, across 47 executables and 135
fixed selections: twenty-four new groups and all 1466 prior selected controls.
Existing Promise/await, import, host-wait, timer, finalization, browser and ownership
controls retain their behavior. Formatting, diff checks and offline workspace/
all-target Clippy with warnings denied pass. Cargo reuses the workspace target with incremental compilation and
dev/test debug artifact growth disabled. The same nine explicit resource/infinite-
loop skips and other finite-public-case exclusions remain. No represented large
allocations, resource/infinite probes, JSON resource sweeps, network/service
replay, privileged validation, process killing, fresh full Test262, coverage result
or integrated acceptance is claimed. Other input/origin producers, other retained
maps, wider heap/host buffers, shared programs and integrated gates remain open;
M1u stays the latest accepted milestone.

Evidence paths: `/private/tmp/blueice-thunk-origin-controls-20261017.py`,
`/private/tmp/blueice-thunk-origin-commit-20261017.txt`,
`/private/tmp/blueice-thunk-origin-audit-20261017.py`, logs matching
`/private/tmp/blueice-thunk-origin-*-20261017.log`, and
`target/thunk-origin-source-20261017.json`. Ordinary-library entry points expose
the same fixtures; no fresh coverage result is claimed. This remains an M1z
implementation checkpoint with wider acceptance open.

#### Module-referrer retained storage follow-up (implementation checkpoint, 2026-10-17)

Acceptance: retain weak closure module-referrer hash backing independently of
reported insertion headroom. Lease complete native table/name preparations before
allocation, with ownership surviving VM destruction. Preserve reserved publication
slots through GC and interleaved preparation. Reclaim sparse backing with optional
admitted candidates; return empty backing after terminal failure. Transfer prepared
owners at an exact allowance without a transient duplicate admission failure.

- [x] Reproduce two detached preparations losing their native charge after VM
      destruction and missing sparse-table reclamation: corrected runtime cases
      were **0/3 passed** before implementation, then **3/3 passed**.
- [x] Implement retained backing, independent leases, guarded reclamation and
      publication handoffs; complete twenty-three finite groups including final
      stale-candidate interleaving coverage.
- [x] Complete fixed offline controls/static checks, evidence audit and detailed
      English commit.

The weak table now uses the retained control-map owner. Pruning/deletion returns
name buffers while its backing remains charged; metadata keys remain weak. GC
uses guarded scheduling-map reclamation. Both the shared table guard and pending
module-publication reservations defer optional compaction. A complete smaller
candidate must fit while old backing remains live; refusal refunds staging and
stays nonterminal. Empty backing releases without allocation, including after a
terminal abort or interpreter unwind.

Closure preparation independently leases the full candidate plus source-copy
minimum before allocation. It checks actual table capacity before allocating the
name, then checks actual name capacity. Data drops before its lease/reservation;
the account outlives the VM while Rust still holds a preparation. Publication
reuses GC-freed slots or adopts prepared backing as outstanding reservations
require, preserving existing entries and other preparations' slots. Only a
candidate with greater insertion capacity may replace the current table, even
when a newer reservation requires further growth. After all staging and
reservations drop, deferred optional reclamation can proceed.

Call preparation uses the same independent source-copy ownership before fuel,
roots or caller-field changes. Associated empty names retain their context-switch
semantics; absent associations retain their fast path. The displaced caller name
keeps its existing separate execution owner and exact allocation restoration.

The first expanded run passed **16/21** groups. One exact-budget handoff exposed
transient duplicate charging when reconciliation checked both the new VM fields
and their still-live staging lease. Fields now adopt storage, then staging drops
immediately before synchronous reconciliation, with no allocation/callback between.
Four aggregate assertions also needed the separately owned heap root/collector/
pin/finalization storage, rather than only VM fields. Their corrected assertions
remain exact. A new empty/spare/full-table exact-publication group brings the
expanded result to **22/22 passed**. Initial fixture compile corrections and all
intermediate outcomes remain in the evidence logs.

Pre-commit review added an eight-object interleaving case. With six entries in a
seven-slot map and a newer growth candidate pending, an older three-slot
candidate attempted to replace live backing and panicked. The added case was
**0/1 passed** before correction. A candidate may now replace only a table with smaller insertion
capacity; otherwise it uses its reserved slot and newer work supplies further
growth. The initial 1512-test/static pass and relevant source snapshots are
archived separately; final fixed controls include this twenty-third group. The
first corrected run reached the final assertion, with **22/23 passed**: the
fixture hard-coded 15 slots while HashMap reported 14. It now checks actual
retained storage and preserved entries, with that failed attempt archived too.

Finite groups additionally cover retained backing after deletion, no/short/exact
compaction headroom and retry, terminal empty cleanup, partial/all-dead GC while
publication is pending, abandoned staging, complete-versus-net growth admission,
forward/reversed interleaved publication, separate post-VM leases, terminal prepared
commit, shared guards, captured-source stability, exact call transfer, actual table
growth and GC-freed candidate reuse. Fixtures use at most eight weak keys and a
few pending preparations; numerical peers allocate no represented large payload
and no external operation starts. Existing real factory/import-origin and caller
restoration controls remain selected.

Validation: **1513 passed, zero failed/ignored**, across 47 executables and 136
fixed selections: twenty-three new groups and all 1490 prior selected controls.
Formatting, diff checks and offline workspace/all-target Clippy with warnings
denied pass on the final source. Source, staged and post-commit audits bind that
source to the final runner, executables, logs and detailed English commit message.
Cargo reuses the workspace target with incremental compilation and dev/test debug
artifact growth disabled. Other input/origin producers, retained maps, wider
heap/host buffers, shared programs and integrated gates remain open; M1u stays the
latest accepted milestone. The same nine resource/infinite-loop skips and other
finite-public-case exclusions remain. No represented large allocation, resource/
infinite probe, JSON resource sweep, network/service replay, privileged validation,
process killing, fresh full Test262, coverage or integrated acceptance is claimed.

Evidence paths: `/private/tmp/blueice-referrer-storage-controls-20261017.py`,
`/private/tmp/blueice-referrer-storage-commit-20261017.txt`,
`/private/tmp/blueice-referrer-storage-audit-20261017.py`, logs matching
`/private/tmp/blueice-referrer-storage-*-20261017.log`, and
`target/referrer-storage-source-20261017.json`. The initial passing-run archive is
`/private/tmp/blueice-referrer-storage-initial-controls-20261017/`, with its
hashed manifest at `/private/tmp/blueice-referrer-storage-initial-controls-20261017.json`.
The capacity-expectation attempt is preserved under
`/private/tmp/blueice-referrer-storage-capacity-attempt-20261017/`, with its
manifest at `/private/tmp/blueice-referrer-storage-capacity-attempt-20261017.json`.
The ordinary-library entry point exposes the same fixtures; no fresh coverage
result is claimed. This remains an M1z implementation checkpoint with wider
acceptance open.

#### Continuation-pruning scratch follow-up (implementation checkpoint, 2026-10-17)

Acceptance: remove temporary native serial-ID sets from post-heap continuation
liveness pruning. Keep pending-reaction and queued-job ownership semantics, with
separate module/function serial namespaces. Each pass must consume its marks so
removing the last owner releases its frame. Preserve local-object/generator
reachability, native refunds and terminal cleanup without requiring new scratch
headroom. Optional table reclamation remains a separate guarded producer.

- [x] Reproduce unadmitted scratch with one pending async frame, one pending
      module frame and both queued frame kinds: **0/3 passed** before the fix,
      observing respectively **1, 1 and 2** allocation attempts.
- [x] Replace the two temporary HashSets with in-place frame-owner marks;
      original finite allocation observations pass **3/3** after implementation.
- [x] Complete expanded finite ownership cases, fixed offline controls/static
      checks, evidence audit and the detailed English commit.

Surviving Promise records and queued await jobs mark their corresponding stored
continuations directly. Unknown serials create no frame, duplicate owners retain
one frame and module/function serials remain independent. Retention consumes each
mark instead of carrying it into the next pass. Every creation/re-suspension and
debugger/generator path initializes the bit empty. The existing retained map
ledger accounts for the native record layout, including this scalar metadata.
The owner traversal and map retention keep their linear traversal shape without
replacing the old sets with repeated scans of all owners for each frame.

No new heap root or Promise/continuation edge is introduced. Real collection and
resumption preserve local objects held by live pending/queued frames; removing
all external owners lets orphan frames and their local objects collect. Mark
reset also releases frames when their last native reaction/job disappears.
Terminal cleanup can drop frame payloads at a full numerical quota while the
shared guard preserves old backing, then release empty backing once the guard
ends. Other post-heap pruning paths, including disposal-map compaction, remain
outside this scratch-removal checkpoint.

The allocation observer exists only in the unit-test binary. It forwards every
allocation/reallocation unchanged to System, never refuses or resizes a request,
and counts only the current test thread during the fixed operation. Its scalar
counter saturates rather than overflowing. A scalar
peer leaves zero native admission headroom without representing a large payload.
The three tiny cases observe the actual post-heap pruning boundary with optional
map replacement guarded off; allocator behavior and production builds are
unchanged. Thirteen functional cases are also exposed through the ordinary-library
fixture entry point. No fresh coverage result is claimed.

The first expanded run was **13/16 passed**. Three collection fixtures changed
only global object properties while declared var bindings still retained the
values. They now use normal JavaScript assignment to clear both owners and verify
that the binding reads null before collection. The failed fixture/log snapshot
remains separate evidence; the production marking implementation is unchanged by
that refinement.

Validation: **1529 passed, zero failed/ignored**, across 47 executables and 137
fixed selections: sixteen new groups and all 1513 prior selected controls.
Formatting, diff checks and offline workspace/all-target Clippy with warnings
denied pass on the final source. Source, staged and post-commit audits bind that
source to the runner, executables, logs, earlier fixture evidence and detailed
English commit message. Cargo reuses the workspace target with
incremental compilation and dev/test debug artifact growth disabled. Other input/
origin producers, retained maps, heap/host buffers, shared programs and integrated
gates remain open; M1u stays the latest accepted milestone. The same nine explicit
resource/infinite-loop skips and other finite-public-case exclusions remain.
No represented large allocation, exhaustion/infinite probe, network/service/
listener/Core/MCP replay, privileged validation, process killing, fresh full
Test262, coverage or integrated acceptance is claimed.

Evidence paths: `/private/tmp/blueice-continuation-pruning-controls-20261017.py`,
`/private/tmp/blueice-continuation-pruning-commit-20261017.txt`,
`/private/tmp/blueice-continuation-pruning-audit-20261017.py`, logs matching
`/private/tmp/blueice-continuation-pruning-*-20261017.log`, and
`target/continuation-pruning-source-20261017.json`. The first expanded attempt is
preserved under `/private/tmp/blueice-continuation-pruning-expanded-attempt-20261017/`,
with its hashed manifest at
`/private/tmp/blueice-continuation-pruning-expanded-attempt-20261017.json`.
This remains an M1z implementation checkpoint with wider acceptance open.

#### Disposal-map retained storage follow-up (implementation checkpoint, 2026-10-17)

Acceptance: retain native backing for both weak DisposableStack brand tables
across deletion and respect staged capacity during GC. Prepare complete native
replacement storage before managed instance allocation or move-source mutation.
Preserve source/resource edges through collection and transfer existing resource
buffers without cloning or another table admission. Fund optional compaction
while old backing coexists; insufficient headroom stays nonterminal and empty
backing releases after terminal failure.

- [x] Reproduce both brands shrinking seven slots to three during guarded GC,
      plus constructor/move accepting only net growth headroom: **0/4 passed**
      before implementation, **4/4 passed** after it.
- [x] Implement retained allocation ownership, admitted optional reclamation and
      prepared constructor/move publication; expanded finite groups pass **22/22**
      on their first run.
- [x] Complete fixed offline controls/static checks, evidence audit and detailed
      English commit.

Both brand maps now retain an allocation ledger independent of deletion-related
insertion capacity. Removing dead keys releases resource buffers while backing
stays charged. GC does no direct shrink allocation; the existing shared guarded
reclamation boundary admits a complete smaller table, moves survivors, reconciles
its owner and refunds staging. No/short candidate headroom remains nonterminal
and retryable. Empty backing releases without admission after terminal failure.
The maps keep their separate brands and weak ownership edges.

After prototype resolution and its observable effects, constructor/move native
preparation independently leases the full table candidate. Move also pre-admits
root-vector capacity and protects its source/resource edges through managed
allocation. Guarded GC can prune old keys without replacing staged backing.
Commit reuses freed slots or adopts the candidate, then initializes an empty
record or transfers the source's existing resource buffer and disposed state.
There is no fallible table admission after source mutation/publication.

Every result restores the temporary root length and previous guard. Already
prepared root-cache capacity remains owned if needed; it is not an untracked
rollback. All candidates commit/drop before the shared post-GC finisher can run
deferred optional reclamation. That finisher performs no managed GC, so the fresh
instance remains safe until its immediate caller adopts the returned value.
Earlier prototype-getter/language effects stay committed. Retire the now-unused
old single-insert net-growth wrapper; wider batch producers remain open.

The twenty-two groups cover original refusals/guards, retained deletion and
resource refunds, optional no/short/exact compaction and retry, terminal empty
cleanup, sequential refunds across both brands, exact full-candidate and spare
slot construction/move, root preparation refusal, managed creation failure,
source/resource survival through allocation-triggered major GC, freed-slot reuse,
nested guard restoration and public synchronous/asynchronous move cleanup order.
Fixtures use at most seven old records per brand, one fresh instance and one
small resource object/string. GC cases use numerical heap ceilings after finite
priming; budget peers allocate no represented large payload and no external
operation starts. Earlier disposal/GC semantics remain selected.

Validation: **1551 passed, zero failed/ignored**, across 47 executables and 138
fixed selections: twenty-two new groups and all 1529 prior selected controls.
Formatting, diff checks and offline workspace/all-target Clippy with warnings
denied pass. Source, staged and post-commit audits bind the final source to build
inputs, runner, executables, logs and detailed English commit text. Cargo reuses
the workspace target with incremental compilation and dev/test debug artifact
growth disabled. The same nine explicit resource/infinite-
loop skips and other finite-public-case exclusions remain. Other input/origin
producers, retained maps, heap/host buffers, shared programs and integrated gates
stay open; M1u remains the latest accepted milestone. No represented large
allocation, resource/infinite probe, network/service/listener/Core/MCP replay,
privileged validation, process killing, fresh full Test262, coverage or integrated
acceptance is claimed.

Evidence paths: `/private/tmp/blueice-disposal-map-controls-20261017.py`,
`/private/tmp/blueice-disposal-map-commit-20261017.txt`,
`/private/tmp/blueice-disposal-map-audit-20261017.py`, logs matching
`/private/tmp/blueice-disposal-map-*-20261017.log`, and
`target/disposal-map-source-20261017.json`. The ordinary-library entry point
exposes all twenty-two groups; no fresh coverage result is claimed. This remains
an M1z implementation checkpoint with wider acceptance open.

#### Template-control preparation follow-up (implementation checkpoint, 2026-10-17)

Acceptance: independently lease the complete detached template table and operand
replacement while old buffers remain owned. Admit minimum and reported capacities
before active stack backing changes. Move existing operands without cloning their
payloads; transfer only the adopted buffer's charge, keeping detached table
ownership alive after VM destruction. Publish cache entries without further
fallible admission and retain identity, frozen descriptors and GC behavior.

- [x] Reproduce lost table ownership after VM destruction, operand growth funded
      only by its net delta and a one-byte-short joint candidate budget being
      accepted: **0/3 passed** before implementation, **3/3 passed** afterwards.
- [x] Implement independent candidate leases and reported-capacity checks before
      stack adoption; expanded finite groups pass **16/16** on their first run.
- [x] Complete fixed offline controls/static checks, evidence audit and detailed
      English commit.

Preparation first synchronizes active ownership, then admits the full requested
table/operand candidates under one independent document lease. Fallible table
allocation is followed by its actual capacity check before the operand allocation;
the operand's reported capacity is checked before either active backing changes.
A refusal drops detached data before refund and preserves prior table entries,
operand pointer/capacity/values, managed heap and fuel at the native boundary.

Once all native storage fits, existing operands move into the new vector without
cloning primitive buffers. VM adoption refunds that portion of staging immediately
before synchronous reconciliation; the detached table retains its independent
lease until publication/drop, including after VM destruction. Cache publication
verifies its original table shape, moves existing entries and refunds staging
before the VM ownership update. Neither handoff checks a transient duplicate
charge against an exact allowance. Final operand sizing keeps the requested
prefix-plus-two-slots shape; stronger admission covers the full coexisting buffer.

The cache remains strong realm storage across frames and managed collection;
building fresh raw/cooked arrays runs no page code. Temporary array roots use the
admitted suffix until the cache owns the cooked array/raw edge. Native preparation
refusal preserves old backing. Later string/heap failure restores the operand
prefix and refunds the table while already adopted operand capacity remains an
accounted VM cache. No permanent root registration is introduced.

The existing joint template control now budgets the full operand candidate and
checks its independent table contribution, while preserving final capacity/refund
assertions. The earlier 605-case checkpoint is explicitly historical evidence
for its original scope, rather than proof of this stronger staging boundary.

Sixteen finite groups cover initial refusals, primitive-buffer move identity,
partial charge adoption before VM destruction, exact/spare publication, preserved
entries, abandoned staging and reported table refusal, host-neutral behavior,
managed/string failure cleanup, allocation-triggered major GC, public-boundary
heap/fuel preservation, terminal refusal and multiple independent preparations.
GC uses four small in-memory garbage events and a numerical heap ceiling after
finite priming. Native peers allocate no represented large payload and no external
operation starts. Earlier async/generator/module template and resource controls
remain selected.

Validation: **1567 passed, zero failed/ignored**, across 47 executables and 139
fixed selections: sixteen new groups and all 1551 prior selected controls.
Formatting, diff checks and offline workspace/all-target Clippy with warnings
denied pass. Source, staged and post-commit audits bind final source/build inputs,
runner, executables, logs and detailed English commit text. Cargo reuses the
workspace target with incremental compilation and dev/test debug artifact growth
disabled. The same nine explicit resource/infinite-
loop skips and other finite-public-case exclusions remain. Wider string/input/
origin producers, shared site/program ownership, other native buffers and
integrated gates remain open; M1u stays the latest accepted milestone. No
represented large allocation, resource/infinite probe, network/service/listener/
Core/MCP replay, privileged validation, process killing, fresh full Test262,
coverage or integrated acceptance is claimed.

Evidence paths: `/private/tmp/blueice-template-storage-controls-20261017.py`,
`/private/tmp/blueice-template-storage-commit-20261017.txt`,
`/private/tmp/blueice-template-storage-audit-20261017.py`, logs matching
`/private/tmp/blueice-template-storage-*-20261017.log`, and
`target/template-storage-source-20261017.json`. The ordinary-library entry point
exposes all sixteen groups; no fresh coverage result is claimed. This remains
an M1z implementation checkpoint with wider acceptance open.

#### Module-dependency batch-storage follow-up (implementation checkpoint, 2026-10-17)

Acceptance: independently admit input/deduplication/name storage and complete
forward/reverse map, dependency-set and parent-vector candidates while every
old graph buffer remains owned. Refuse before either direction changes. Keep
allocation ledgers across deletion; admit optional sparse reclamation without
making an otherwise live document terminal. Preserve parent/source/BFS order
and the existing import/defer/await/GC boundaries.

- [x] Reproduce allocation before zero-headroom refusal, reverse growth funded
      only by its live/net delta, and unadmitted sparse-map reclamation:
      **0/3 passed** on the exact old-code namespace, **3/3 passed** afterwards.
      The earlier broad filter also ran one old deferred-completion control;
      its **1 passed / 3 failed** log is retained separately.
- [x] Implement complete batch preparation, retained map/set allocation owners
      and optional admitted reclamation; expanded finite groups pass **20/20**
      on their first run. The sixteen earlier dependency controls also pass.
- [x] Finish existing fixed offline controls, formatting/Clippy, evidence audit
      and detailed English commit.

Registration first admits the incoming vector's reported capacity, all original
name capacities and the complete deduplication set. Redundant existing names
drop before subsequent staging. A wholly redundant batch takes an allocation-
free path, including at a full quota. Remaining dependency names move once;
copied parent/reverse keys, edge metadata, all required reverse vectors and
complete forward/reverse/set candidates carry independent native leases.
Minimum and allocator-reported capacities must fit before publication. Preparing
an existing map with zero additional keys works even when that map is full.

Preparation preserves both graph directions, all live capacities and managed
heap state. Every abandoned or refused batch drops its data before refund;
detached ownership survives VM destruction. Publication checks prepared table/
vector shapes, moves old owners without cloning their name buffers, appends
each parent's reverse edge in registration order, then refunds staging directly
before synchronous ownership reconciliation. No callback, managed collection
or new fallible allocation occurs during that graph handoff. Scalar peak
tracking follows preparation phases without a per-edge graph rescan.

Forward sets and both outer maps retain their allocation ledgers after deletion,
so removed names refund without pretending that backing has disappeared. Sparse
sets, reverse vectors and outer maps use optional complete replacement leases.
Refusal leaves old storage and document failure state unchanged; releasing a
numerical peer permits a later retry. Reclamation refunds each actual published
reduction before its candidate lease drops and before funding the next layer.
A single initial reconciliation plus scalar reductions avoids rescanning the
whole graph per row. Empty graph storage frees without allocation, including
after a terminal admission failure.

Twenty finite groups cover zero-headroom allocation counts, complete reverse
growth, preserved preparation snapshots, abandonment and detached lifetimes,
exact/one-byte-short peaks, full-quota duplicates, mixed duplicate order,
inner/outer sparse reclamation, stable name-buffer pointers, reported-capacity
refusal, retry after peer release, terminal empty cleanup, retained backing on
deletion, host-neutral behavior, forward-set growth, incoming excess capacity
and full maps needing zero new keys. Fixtures contain at most a few short names;
numeric peer leases represent quota usage without allocating that payload.

The original M1y spare-slot and duplicate controls now budget the full coexisting
preparation peak, then assert the final retained delta/refund and original parent
order. Their earlier 193-case receipt remains historical evidence for its
original callback/graph scope; it does not establish the stronger native staging
boundary added here. Wider pending-name/frontier/DFS/walk producers, full module
caches, shared programs, other native/host buffers and integrated acceptance
remain open. M1u stays the latest accepted milestone.

Validation: **1587 passed, zero failed/ignored**, across 47 executables and 140
fixed selections: twenty new groups and all 1567 prior selected controls.
Formatting, diff checks and offline workspace/all-target Clippy with warnings
denied pass. Source, staged and post-commit audits bind final source/build inputs,
the unchanged fourteen Rust inputs, runner, executables, initial/final logs and
detailed English commit text. Cargo reuses the workspace target with incremental
compilation and dev/test debug artifact growth disabled. The same nine explicit
resource/infinite-loop skips and finite-public-case exclusions remain. No
resource-exhaustion probe, network/service/listener/Core/MCP replay, privileged
validation, process killing, fresh full Test262, coverage result or integrated
acceptance is claimed.

Evidence paths: `/private/tmp/blueice-dependency-storage-controls-20261017.py`,
`/private/tmp/blueice-dependency-storage-commit-20261017.txt`,
`/private/tmp/blueice-dependency-storage-audit-20261017.py`, logs matching
`/private/tmp/blueice-dependency-storage-*-20261017.log`, and
`target/dependency-storage-source-20261017.json`. The ordinary-library entry point
exposes nineteen functional groups; unit-only allocation observations use the
shared fixture observer. No fresh coverage result is claimed. This remains an
M1z implementation checkpoint with wider acceptance open.

#### Module-name producer follow-up (implementation checkpoint, 2026-10-17)

Acceptance: admit request normalization/type-key work buffers, copied pending
names and complete eager/deferred evaluation/frontier vectors before allocating
them. Retain independent native owners across dependency JavaScript, iteration
and dynamic-import result handoff. Move existing payloads, preserve canonical
paths and evaluation order, and refund abandoned owners after their data drops.
Keep DFS/visited/SCC and loading/linking scratch as separate open work.

- [x] Reproduce allocation before zero-headroom eager/deferred evaluation
      refusal: **0/2 passed**, with **6/9 allocation attempts**, before the
      implementation; **2/2 passed** without allocation afterwards.
- [x] Add independent name/list/iterator ownership and prepared-vector adoption;
      expanded finite contracts pass **19/19** on their first run.
- [x] Migrate legacy callback observations and complete the public waiter
      preparation budgets; focused controls pass **16/16** and **12/12**.
- [x] Finish final fixed offline controls/static checks, source/artifact audit
      and detailed English commit.

One independently leased String now handles request path normalization and its
module-type suffix in the evaluation, frontier and dynamic-import producers.
Admission covers the maximum intermediate work length and allocator-reported
capacity before writing. Normalization updates that buffer in place with a
segment count; it creates neither a segment vector nor a second qualified-key
copy. A fixed lexical table compares paths/types/errors with the established
resolver, including empty segments, host-root escapes, Unicode and NUL rejection.
Other host-neutral linking/loading resolver callers keep their original scope.

Each name/list owns its reported String/vector capacities through an independent
document lease. Copies acquire native ownership before allocation. Growth
prepares the complete new vector while old slots/names remain owned, moves the
existing String payloads, refunds old backing and absorbs the candidate's lease
without readmission. Available vector slots consume only an already owned name.
Terminal and foreign-owner refusals preserve existing names/capacity; cleanup
can still drop obsolete names while retaining live vector backing.

An iterator retains the original vector allocation and every remaining name,
splitting off the current name's existing charge on each yield. All coexist
through dependency execution and remain owned beyond VM destruction if a Rust
caller retains them. Pending roots are borrowed during selection and copied only
through the admitted destination list. Deferred frontiers retain their own leases
through execution, result handoff and in-place filtering. Ordinary/deferred
dynamic-import results hold these leases until consumed or dropped. Immediate
batch/waiter consumption releases the incoming lease directly before the
recipient admits retained input or discards a duplicate, without an intervening
allocation/callback or a transient duplicate check against exact headroom.

Fallible producer admission now propagates as a runtime resource refusal through
both eager/deferred evaluation paths. It no longer relies on the old assumption
that every fallible frontier result can only contain a resolution error. The
parent's new dependency edges and suspended state publish only after complete
batch admission. A late pending-copy refusal preserves the preexisting graph and
restores parent evaluation flags; existing dependency execution semantics remain.

Nineteen finite groups cover the two old-code boundaries, lexical compatibility,
one-buffer reported ownership, pre-allocation request refusal, full/exact/spare
vector growth, stable payload pointers, partial/exhausted/detached iterators,
append ownership, terminal retention/append behavior, foreign documents,
host-neutral lists, cached dynamic-import results, frontier result ownership,
late pending refusal and exact native batch handoff. Seventeen functional groups
are exposed through the ordinary-library entry point; two allocation-only
runtime observations stay unit-only. Fixtures use short names, tiny graphs and
numerical native peers without representing their quota-sized payload.

Two historical M1y callback observers initially reported **14/16 passing** because
they read only the displaced-frame subtotal. The corresponding names now have
independent owners. Their observations now use document-budget deltas, with
queued job storage already included in the baseline, and exact steady-state
native ownership after success or abort. Distinct compiled snapshots retained
by suspended continuations count once each. All sixteen controls pass; the
intermediate **15/16** balance observation also remains separate evidence.

The first complete runner reached every fixed selection, with only two waiter
success fixtures failing (**10/12** in that selection). Their old allowance
covered waiter input/set staging but omitted the newly admitted producer peak.
A unit/coverage-only scalar peak observer measures one fixed native operation
against the actual shared document account. Ordinary/deferred fixtures budget
the complete producer/staging peak, retain their public job entry, and check
final refunds and unchanged spare capacity. This uses one tiny preview per
fixture, without a quota search or represented large allocation. Earlier runner,
source-input and Clippy evidence is preserved separately before final validation.

Validation: **1606 passed, zero failed/ignored**, across 47 executables and 141
fixed selections: nineteen new groups and all 1587 prior selected controls.
Formatting, diff checks and offline workspace/all-target Clippy with warnings
denied pass. Source, staged and post-commit audits bind final source/build inputs,
the twelve unchanged Rust inputs, runner, executables, initial/final logs and
detailed English commit text. Cargo reuses the workspace target with incremental
compilation and dev/test debug artifact growth disabled. The same nine explicit
resource/infinite-loop skips and finite-public-case exclusions remain.
DFS/visited/SCC scratch, readiness/
loading/linking producers, full module caches, shared programs, other native/host
buffers and integrated acceptance remain open; M1u stays the latest accepted
milestone. No resource-exhaustion probe, external service/network/Core/MCP replay,
privileged validation, process killing, full Test262 or coverage is claimed.

Evidence paths: `/private/tmp/blueice-module-name-storage-controls-20261017.py`,
`/private/tmp/blueice-module-name-storage-commit-20261017.txt`,
`/private/tmp/blueice-module-name-storage-audit-20261017.py`, logs matching
`/private/tmp/blueice-module-name-storage-*-20261017.log`, and
`target/module-name-storage-source-20261017.json`. The fixture-placement compile
error is preserved separately and is not counted as behavioral red evidence.
This remains an M1z implementation checkpoint with wider acceptance open.

#### Module-walk storage follow-up (implementation checkpoint, 2026-10-17)

Acceptance: independently admit complete visited hash tables, explicit DFS and
loading stacks and lexically ordered SCC candidate vectors before allocation.
Borrow canonical keys, retain backing ownership across scratch reuse, and
propagate resource refusal through frontier/readiness/cycle callers. Preserve
source-order analysis, eager cycle edges, all-phase loading and error selection.

- [x] Reproduce allocation before frontier refusal, an unadmitted cycle-root
      walk and readiness returning a language error at zero native headroom:
      **0/3 passed** before implementation, **3/3 passed** without allocation
      afterwards. Frontier previously attempted two allocations before refusal.
- [x] Replace recursive analysis with admitted explicit frames and borrowed
      visited keys; expanded finite contracts pass **23/23** on their first
      behavioral run. The separate fixture type-inference compile error is
      preserved and is not behavioral red evidence.
- [x] Finish fixed offline controls/static checks, source/artifact audit and
      detailed English commit.

Visited hash storage borrows immutable canonical registry keys instead of
cloning a String for every visit. Growth admits a complete replacement while
the old table is still owned, checks reported capacity, then moves borrowed
entries and transfers the lease. Explicit frames retain a canonical name and
the next request index. Complete vector candidates follow the same prepared
ownership transfer. Clearing a walk preserves allocated backing and its charge;
no operation pretends those retained buffers have been freed.

One query owner reuses visited/frame/candidate storage for mutual reachability
and SCC/error analysis. Candidate keys borrow linked records and remain sorted
lexically; source order is preserved inside each DFS. Frontier and readiness
walks use a separate admitted explicit stack while reusing the SCC query owner.
They execute no JavaScript or managed collection. The query owns an independent
document scope, so a Rust caller can retain scratch after VM destruction while
the immutable registry/record borrow remains valid. Every container drops its
data before its own native lease.

Loading uses admitted pending-node storage and includes requests of every phase.
It resolves each current node's requests in source order before visiting the
last queued child, preserving both resolution-error precedence and the original
reverse pending order. Known targets immediately borrow their registry key;
missing targets retain their leased name until selected or dropped. Unreachable
registry records stay outside this loading walk. Cycle reachability continues
to exclude deferred edges, while frontier/readiness/loading include them.

Resource admission now propagates through asynchronous parent selection,
finished-cycle error lookup and deferred synchronous readiness. Former
resolution-only `expect` assumptions no longer turn resource refusal into a
panic. An ordinary suspended/asynchronous readiness failure still returns its
language TypeError when native work fits. Fresh helpers own their visited state;
internal fixtures migrate away from an externally seeded String set. Standalone
debugger graph validation keeps its host-neutral policy and public signature.

The frontier now writes one unique ordered result list; obsolete recursive
child-list append glue is removed. Earlier list move/refund contracts exercise
the retained iterator and owned-name push operations directly.

Twenty-three finite groups cover the three original failures, allocation-free
identity/reuse, canonical key pointers, exact/short operation peaks, full hash
and frame candidates, detached lifetimes, terminal cleanup, deferred/missing-goal
reachability, cyclic/diamond frontier order, sync cycles and missing records,
SCC completion and candidate reuse, lexical error identity, loading order and
reachability scope, loading refusal, cached import resource propagation and
normal readiness language failure. Twenty functional groups are available to
the ordinary-library entry point. Chains contain at most five short-named nodes;
numerical native peers and one fixed peak observation replace physical pressure.

The first complete runner reached every selection, with one historical success
fixture failing (**15/16** in the dependency selection). Its allowance covered
batch staging rather than the newly admitted analysis peak. One fixed equivalent
runtime preview now budgets complete preparation while retaining final growth,
refund and parent-order assertions. The original runner/source/check evidence
is preserved separately; this introduces no quota search or large allocation.

Validation: **1633 passed, zero failed/ignored**, across 47 executables and 146
fixed selections: twenty-three new groups, four additional legacy resolver/cycle/
registry boundaries and all 1606 prior selected controls. Formatting, diff checks
and offline workspace/all-target Clippy with warnings denied pass. Source, staged
and post-commit audits bind final source/build inputs, the eleven unchanged Rust
inputs, runner, executables, initial/final logs and detailed English commit text.
Cargo reuses the workspace target with incremental compilation and dev/test debug
artifact growth disabled. The same nine explicit resource/infinite-loop skips and
finite public-case filters remain. Linking/export traversal producers, module-body
evaluation recursion, allocator/error-message overhead, full module caches,
shared programs, other native/host buffers and integrated acceptance remain
open; M1u stays the latest accepted milestone. Admission follows the existing
reported-slot accounting model. No resource-exhaustion probe, external network/
service/Core/MCP replay, privileged validation, process killing, full Test262,
fresh coverage or integrated acceptance is claimed.

Evidence paths: `/private/tmp/blueice-module-walk-storage-controls-20261017.py`,
`/private/tmp/blueice-module-walk-storage-commit-20261017.txt`,
`/private/tmp/blueice-module-walk-storage-audit-20261017.py`, logs matching
`/private/tmp/blueice-module-walk-storage-*-20261017.log`, and
`target/module-walk-storage-source-20261017.json`. The intermediate Clippy log
identifies the obsolete append helper before its removal. This remains an M1z
implementation checkpoint with wider acceptance open.

#### Namespace-cache ownership follow-up (implementation checkpoint, 2026-10-17)

Acceptance: retain all five eager/deferred namespace cache/root/reverse-map
allocations and owned name capacities in the document budget. Admit complete
publication storage before managed allocation, preserve identity/rooting across
GC and reject before native scratch when headroom is absent. Roll back recursive
namespace creation and earlier successful namespace creation in a failed linking
transaction without allocating cleanup sets, copied keys or vectors.

- [x] Reproduce two allocations before zero-headroom refusal and missing table/
      name charges: **0/3 passed** before implementation, **3/3 passed** after.
      The first table layer previously reported zero rather than 120 bytes and
      the first padded name reported zero rather than 64 bytes.
- [x] Implement retained cache storage, complete publication preparation and
      marked rollback; expanded finite contracts pass **22/22** on their first
      run, including the real failed-link/retry boundary.
- [x] Pass the 151 fixed offline selections: **1659 distinct cases**, zero
      failed/ignored, across 47 executables; pass offline workspace/all-target
      Clippy. These runs completed before the owner's cybersecurity warning.
- [x] Pass final formatting and diff checks; audit source/artifact, staged and
      committed contents with a detailed English checkpoint commit.

All namespace identity/root maps and the deferred-object reverse map use retained
allocation ledgers. The VM owns their backing and each key/value String capacity
even after deletion lowers reported hash capacity. Publication stages two eager
or three deferred name copies and complete replacement maps, checking requested
and reported capacities before managed allocation. The prepared native owner
survives VM destruction if retained by Rust, and abandonment drops data before
refund. Exact staging handoff refunds the independent lease directly before
synchronous VM reconciliation, avoiding a transient duplicate admission.

Capacity preservation protects prepared table shapes during placeholder
allocation/GC. Temporary and cache roots must both succeed before cache adoption
and linked-record publication. Each namespace kind receives a monotonic mark;
serial refusal occurs before allocation/publication. Cached identities remain
available without new allocation at full healthy quota. Eager and deferred
objects remain distinct and collectible only after their owning realm/cache
lifetime has ended; normal collection preserves the rooted identity.

One outer namespace transaction scans linked-record marks to retract every new
recursive placeholder and its cache root. A failed graph link also retracts all
namespaces created since that linking call began, including earlier successful
namespace builds whose cells belonged to the failed graph. Existing entries
remain intact. A fixed real runtime case first creates a namespace import and
then fails a named import; retry with corrected modules observes the new value
through new binding cells rather than reusing the failed graph's namespace.

Rollback removes marked entries, clears record flags/marks, releases cache pins
and the caller's root delta, and frees empty tables without allocating, including
after terminal failure. Nonempty backing stays charged. Optional sparse table
reclamation is separate and independently admits full coexisting replacements;
refusal stays nonterminal. Each adopted reduction refunds VM ownership before
the candidate lease drops and before funding the next table. Live name buffers
and roots move unchanged. Linking-record marker storage joins the existing
record-table accounting.

Twenty-two finite groups cover all five backing/name layers, zero-headroom
refusal, eager/deferred identity, untouched preparation snapshots, abandonment
and detached ownership, exact/short staging/publication, allocation-free terminal
and empty rollback, retained deletion backing, refused/exact/retried reclamation,
serial guards, recursive failure, numeric root-registration failure, small managed
ceilings and retry, graph-wide rollback/new-cell retry, collection and VM-drop
refund, and host-neutral behavior. Twenty-one functional groups are exposed
through the ordinary-library entry point; allocation observations stay unit-only.
Fixtures use only short names, a few modules/objects and numerical peers.

Existing validation: **1659 passed, zero failed/ignored**, across 47 executables
and 151 fixed selections: twenty-two new groups, four additional finite namespace/
root/source/resolution boundaries and all 1633 prior selected controls. Offline
workspace/all-target Clippy passes. These runs completed before the owner's
cybersecurity warning. Subsequent local checks confirmed the completed runner,
distinct case count, unchanged hashes for all twelve Rust inputs and formatting.
The source/artifact receipt records all fourteen final source files, Cargo inputs,
runner, executables, logs, original regression evidence and detailed English
message; staged and committed audits bind that evidence to the checkpoint.
No test replay or network operation follows that warning for this checkpoint.
The prior rejected push remains pending and is not retried. The same nine explicit
resource/infinite-loop skips and finite public filters remain.
Export traversal/staging, namespace/module-body
construction recursion, other module/source caches, allocator/error-message
overhead, shared programs and integrated acceptance remain open; M1u stays the
latest accepted milestone. No resource-exhaustion probe, external network/service/
Core/MCP replay, privileged validation, process killing, full Test262, fresh
coverage result or integrated acceptance is claimed.

Evidence paths: `/private/tmp/blueice-namespace-cache-storage-controls-20261017.py`,
`/private/tmp/blueice-namespace-cache-storage-rust-inputs-20261017.json`,
`/private/tmp/blueice-namespace-cache-storage-commit-20261017.txt`,
`/private/tmp/blueice-namespace-cache-storage-audit-20261017.py`, logs matching
`/private/tmp/blueice-namespace-cache-storage-*-20261017.log`, and
`target/namespace-cache-storage-source-20261017.json`. This remains an M1z
implementation checkpoint with wider acceptance open.

#### Export-name traversal follow-up (implementation checkpoint, 2026-10-17)

Acceptance: admit `GetExportedNames` traversal/result backing before allocation;
borrow existing canonical module keys and export names; replace native recursion
with explicit frames; retain result ownership through namespace construction.
Preserve sorted distinct names, explicit export forms, star-default exclusion,
cycles, request qualification and the first source-ordered host error.

- [x] Reproduce three tiny namespace failures before implementation: **0/3
      passed**. Empty/local/star cases attempt 6/8/13 allocations where only
      the four already-admitted cache preparations are permitted.
- [x] Implement admitted module/name visited sets, iterative traversal frames,
      qualified star-request buffers and a retained borrowed-name result vector.
- [x] Pass new finite contracts, fixed offline controls, formatting and Clippy;
      complete source/artifact audits and a detailed English commit.

The collector shares the existing admitted walk-storage primitives. Each growth
leases a complete coexisting replacement and adjusts to its reported capacity;
the lease outlives its data. Distinct-module and distinct-name sets borrow existing
String buffers. Explicit frames walk star exports in source order, resolving each
request under its own native owner before borrowing a canonical registry key.
Repeated modules are visited once. Local/indirect/namespace/deferred/source names
are collected without resolving their bindings or following non-star requests.

Only the root's explicit default is included; defaults reached through stars
remain excluded. The result sorts borrowed names in place without a BTreeSet,
returns only its vector owner, and releases all other traversal scratch. Deferred
namespace filtering preserves and charges retained vector backing. Namespace
construction keeps that independent owner alive while allocating/rooting cells
and recursively building namespace exports. A Rust-held result remains charged
after VM destruction and refunds when dropped.

Nineteen finite groups cover the three allocation observations, explicit forms,
star/default rules, cyclic diamonds and duplicate names, first missing-star and
invalid/root errors, numerical partial-refusal refunds, retained pointer-vector
ownership, borrowed source pointers, VM-drop lifetime, deferred-name filtering,
empty-result refunds, terminal refusal, typed host keys, small vector growth,
Unicode ordering, and real cyclic namespace values. Sixteen functional groups
are exposed through the ordinary-library entry point; allocation observations
remain unit-only. Fixtures contain a few modules and at most ten export names.

Validation: **1678 distinct cases passed, zero failed/ignored**, across 47
executables and 152 fixed selections: nineteen new groups and all 1659 prior
selected controls. The first expanded run passes **19/19**. Formatting, diff
checks and offline workspace/all-target Clippy with warnings denied pass. Eight
frozen Rust inputs remain unchanged through these runs. The source/artifact
receipt records ten final source files, Cargo inputs, runner, distinct test
identities, executables/logs, red-phase source copies and detailed English
message; staged and committed audits bind them to this checkpoint.

Validation uses only fixed finite in-memory cases, numerical admission peers and
the shared workspace target, with incremental/dev/test debug artifact growth
disabled. The same nine explicit resource/infinite-loop skips and finite public
filters remain. No resource-exhaustion probe, network/service/listener/Core/MCP
replay, privileged validation, process killing, fresh full Test262, coverage or
integrated acceptance is claimed. The rejected push is not retried.
`ResolveExport` visited pairs/results, namespace export binding-list and
import-alias staging, namespace/module-body recursion, other module/source
caches, allocator/error-message overhead, shared programs and integrated
acceptance remain open. M1u remains the latest accepted milestone.

Evidence paths: `/private/tmp/blueice-export-name-storage-controls-20261017.py`,
`/private/tmp/blueice-export-name-storage-rust-inputs-20261017.json`,
`/private/tmp/blueice-export-name-storage-commit-20261017.txt`,
`/private/tmp/blueice-export-name-storage-audit-20261017.py`, logs matching
`/private/tmp/blueice-export-name-storage-*-20261017.log`, preserved red-phase
sources in `/private/tmp/blueice-export-name-storage-red-source-20261017/`, and
`target/export-name-storage-source-20261017.json`. This remains an M1z
implementation checkpoint with wider acceptance open.

#### Export-resolution storage follow-up (implementation checkpoint, 2026-10-17)

Acceptance: admit `ResolveExport` traversal and result allocations before native
storage changes; borrow canonical host keys for bindings and known targets;
retain independent ownership for qualified target names outside the supplied
bytecode registry. Replace native recursion and preserve explicit/default rules,
renaming, cycles, star ambiguity, result-kind identity and host error order.

- [x] Preserve five old-code failures: **0/5 passed**. Local binding lookup
      allocates four times instead of zero; a refused indirect query allocates
      eight times before the first admission boundary. Namespace/deferred/source
      results keep native String buffers without a document charge.
- [x] Implement iterative admitted traversal, borrowed fast results and owned
      namespace/source results; pass **25/25** on the first expanded run.
- [x] Pass fixed offline controls, formatting and Clippy; complete source/
      artifact, staged and committed audits with a detailed English message.

The private resolution type distinguishes borrowed canonical module keys from
independently owned `ModuleName` targets. Local binding and nontraversing missing/
default results need no allocation at full healthy quota. Terminal failure is
checked even before those fast results. Namespace/deferred/source qualification
admits its work buffer, borrows a supplied registry key when possible and keeps
the complete target owner otherwise. Result identity compares kind, module name
and binding slot rather than storage form. Direct namespace/source imports move
their prepared target instead of cloning its String.

Traversal borrows module/query names, owns complete coexisting vector candidates
for query-wide cycle markers and explicit frames, and releases scratch before
returning. Markers distinguish different names in the same module. Indirect
results propagate through waiting parents; stars merge candidates in declaration
order, suppress missing/cyclic paths and propagate ambiguity. Equal owned
namespace candidates refund duplicate names; ambiguity and later host failure
refund discarded candidates. Qualified request buffers stay independently
charged until replaced by canonical borrowed keys or adopted by a result.

All linking/namespace resolver callers now supply the document scope. Indirect
export validation and linking-phase source/import target preparation use admitted request
names. Source-phase resolution intentionally remains unqualified by the resource
type. Runtime binding-cell lookup and live alias semantics remain intact.

Twenty-five finite groups cover the five original failures, allocation-free full
quota results, canonical pointers after input destruction, renamed reentry,
indirect/star cycles, same-binding diamonds, different slots/modules and result
kinds, explicit/default precedence, owned candidate equality/refunds, source and
typed requests, source-ordered/invalid host errors, numerical partial-refusal
refunds, terminal checks, exact/one-byte-short result ownership, VM-drop lifetime,
known target borrowing, real named-import live updates and pre-evaluation
ambiguity rejection, and later-error candidate cleanup. Twenty-three functional
groups are exposed through the ordinary-library entry point; two allocation
observations remain unit-only.

Validation: **1707 distinct cases passed, zero failed/ignored**, across 49
executables and 157 fixed selections: twenty-five new groups, four additional
finite public cases and all 1678 prior selected controls. The four added cases
cover live/default/namespace bindings, cyclic hoisted functions, namespace identity/
ambiguity under normal and small nurseries, and intermediate star ambiguity.
Formatting, diff checks and offline workspace/all-target Clippy with warnings
denied pass. All seven frozen Rust inputs remain unchanged through the runs.
The source/artifact receipt records nine final source files, Cargo inputs,
runner, distinct test identities, executables/logs, original red-phase source
copies and detailed English message. Staged and committed audits bind those
inputs to this checkpoint.

The same nine explicit resource/infinite-loop skips and finite public filters
remain. Tests use short in-memory fixtures and numerical admission peers, reuse
the workspace target, and disable incremental/dev/test debug artifact growth.
No resource-exhaustion probe, external network/service/listener/Core/MCP replay,
privileged validation, process killing, fresh full Test262, coverage or integrated
acceptance is claimed. The rejected push is not retried. Binding-list/import-
alias staging, namespace/module-body recursion, other synthetic/source request
producers and module/source caches, allocator/error-message overhead, shared
programs and integrated acceptance remain open. M1u remains the latest accepted
milestone.

Evidence paths: `/private/tmp/blueice-export-resolution-storage-controls-20261017.py`,
`/private/tmp/blueice-export-resolution-storage-rust-inputs-20261017.json`,
`/private/tmp/blueice-export-resolution-storage-commit-20261017.txt`,
`/private/tmp/blueice-export-resolution-storage-audit-20261017.py`, logs matching
`/private/tmp/blueice-export-resolution-storage-*-20261017.log`, preserved
red-phase sources in
`/private/tmp/blueice-export-resolution-storage-red-source-20261017/`, and
`target/export-resolution-storage-source-20261017.json`. This remains an M1z
implementation checkpoint with wider acceptance open.

#### Namespace managed-payload follow-up (implementation checkpoint, 2026-10-17)

Acceptance: heap-owned namespace payloads retain their vector and UTF-16 name
capacity charge, including empty reserved lists. Completing a placeholder moves
that ownership into both its object counter and the local/shared managed-byte
counter. Collection must refund every byte. Completion safepoints must protect
the namespace and all incoming binding cells before their edges are published.

- [x] Preserve five old-code failures: **0/5 passed**. Allocation and completion
      charge 42 rather than 384 retained bytes; collection leaves 42 phantom
      bytes; replacing empty backing miscounts ownership; forced completion GC
      reclaims an unrooted incoming cell and panics in the write barrier.
- [x] Use retained namespace payload capacity at allocation/completion, update
      object and aggregate counters together, and trace incoming cells during GC.
- [x] Pass expanded finite cases, fixed offline controls, formatting and Clippy;
      complete source/artifact audits and a detailed English checkpoint commit.

One namespace payload calculation covers the export vector's reported capacity
and every owned UTF-16 name capacity. `HeapConfig` now documents this namespace
exception explicitly; other heap kinds keep their current accounting scope.
Allocation and placeholder completion share the same calculation. Replacing an
empty reserved vector admits only positive growth and refunds released backing
at adoption. The object's byte field changes with the local/document managed
counter so minor/major collection removes the entire adopted payload.

Completion validates incoming cells and the target before its safepoint, and
uses a borrowed namespace/input-cell root view without allocating a temporary
root vector. In-place unstable UTF-16 sorting
avoids extra native sort scratch for valid unique export names. Existing
old-to-young barriers publish live binding edges after collection. Invalid
completion leaves an existing payload intact, and a short managed budget leaves
the placeholder unfilled.

Thirteen finite groups cover retained capacity at allocation/completion, exact
major/minor collection refunds, forced completion GC, replacement backing,
old-to-young edges, exact/one-byte-short local/shared managed limits, invalid
completion, UTF-16 ordering with name-buffer identity, shared-document collection
and heap-drop refunds, and charged empty reserved lists. All thirteen functional
groups are exposed through the ordinary-library entry point. Fixtures use one
namespace, a few cells, an eight-entry vector and thirty-two-unit name buffers;
managed refusal tests use only numerical limits around those small payloads.

Validation: **1723 distinct cases passed, zero failed/ignored**, across 49
executables and 161 fixed selections: thirteen new groups, three additional
finite heap contracts and all 1707 prior selected controls. The first expanded
run passes **13/13**. Added heap contracts cover namespace ordering/error
validation, mapped/live export cells and nonempty completion rejection.
Formatting, diff checks and offline workspace/all-target Clippy with warnings
denied pass. Six frozen Rust inputs remain unchanged through these runs. The
source/artifact receipt records eight final source files, Cargo inputs, runner,
distinct case identities, executables/logs, original regression source copies
and detailed English message; staged and committed audits bind them to the
checkpoint.

The same nine explicit resource/infinite-loop skips and finite public filters
remain. Fixed offline tests reuse the workspace target with incremental and
dev/test debug artifact growth disabled.
Native export binding-list/import-alias/import-slot staging, namespace/module-body
recursion, other synthetic/source request producers and module/source caches,
other heap/native buffers, allocator/error-message overhead, shared programs and
integrated acceptance remain open. M1u remains the latest accepted milestone.
No resource-exhaustion probe, external network/service/listener/Core/MCP replay,
privileged validation, process killing, fresh full Test262, coverage or integrated
acceptance is claimed. The rejected push is not retried.

Evidence paths: `/private/tmp/blueice-namespace-payload-storage-controls-20261017.py`,
`/private/tmp/blueice-namespace-payload-storage-rust-inputs-20261017.json`,
`/private/tmp/blueice-namespace-payload-storage-commit-20261017.txt`,
`/private/tmp/blueice-namespace-payload-storage-audit-20261017.py`, logs matching
`/private/tmp/blueice-namespace-payload-storage-*-20261017.log`, preserved
red-phase sources in
`/private/tmp/blueice-namespace-payload-storage-red-source-20261017/`, and
`target/namespace-payload-storage-source-20261017.json`. This remains an M1z
implementation checkpoint with wider acceptance open.

#### Module binding preparation follow-up (implementation checkpoint, 2026-10-17)

Acceptance: admit import-slot lookup and full import-alias staging before their
storage/managed effects; admit namespace output backing and UTF-16 names before
placeholder publication. Keep native ownership through recursion, GC and abrupt
exits, then transfer completed payloads to the heap's retained-capacity accounting.
Refused preparations must preserve heap/root/cache state and refund their data.

- [x] Preserve four failures from the extracted original producer expressions:
      **0/4 passed**. Import-slot collection, alias growth and namespace backing
      each allocate once at zero headroom; staged export payloads have no charge.
- [x] Own complete import-slot/alias candidates and namespace UTF-16/output
      preparation; transfer payload ownership at the heap operation boundary.
- [x] Pass expanded finite contracts, fixed offline controls, formatting and
      Clippy; record source/artifact and commit audits with detailed English text.

Imported slots use an admitted sorted/deduplicated vector. Its backing stays
owned while binding cells are allocated. Aliases reserve the complete batch
before resolving imports or building namespace/source cells. Publication requires
the complete batch and pre-existing destination cell-table capacity, then copies
the cells without growth and releases the staging buffer. Both preparations
reuse the existing complete-candidate native storage owner.

Namespace preparation admits source-name headers, complete output backing and
all UTF-16 name buffers before allocation, then checks reported capacities at
each step. It creates no managed objects or roots. Kept names move into output
slots without cloning; skipped names release their payload while both vectors'
backing remains charged. The preparation survives Rust retention after its VM
dies and releases data before its lease on abandonment.

After every name is processed, consumed name headers are freed and the remaining
lease covers exactly the heap export payload. A ready owner stays captured
through GC-trace preparation. The heap either consumes the payload or drops it
on error; its native lease drops immediately after that heap call and before VM
trace completion/cleanup. Both sides use the shared retained-capacity payload
calculation. A previously unbounded preparation admits its remaining data before
entering a document; an existing owner cannot silently move to another document
or an unbounded VM. Terminal failure permits refunds and rejects publication.

Twenty-five finite groups cover the four original observations, slot contents,
exact/short slot/alias/namespace preparations, allocation-free alias publication,
detached lifetimes, empty/full-quota and terminal cases, UTF-16 native/managed
handoff, skipped-name refunds/backing retention, unchanged preparation state,
completion GC, managed refusal, trace-preparation refusal, late document admission,
cross-document rejection, host-neutral behavior, terminal cleanup, real mixed
live/deferred imports, and failed-link/new-cell retry. Twenty-two functional
groups are exposed through the ordinary-library entry point; three allocation
observations remain unit-only.

Validation: **1748 distinct cases passed, zero failed/ignored**, across 49
executables and 162 fixed selections: twenty-five new groups and all 1723 prior
selected controls. The first completed expanded run passes **25/25**. Formatting,
diff checks and offline workspace/all-target Clippy with warnings denied pass.
All seven frozen Rust inputs remain unchanged through the runs. The source/
artifact receipt records nine final source files, Cargo inputs, runner, distinct
case identities, executable/log hashes, original red-phase source copies,
separate compile-attempt inputs and detailed English message. Staged and
committed audits bind the evidence to this checkpoint.

The initial expanded compile attempt referenced a heap-private test query; it
is preserved separately and is not behavioral red evidence. Absence checks now
use the public heap read API. The same fixed offline/tiny-fixture constraint,
nine explicit resource/infinite-loop skips and finite public filters remain.
Tests reuse the workspace target and disable incremental/dev/test debug artifact
growth. No physical resource-exhaustion probe, external network/service/listener/
Core/MCP replay, privileged validation, process killing, fresh full Test262,
coverage or integrated acceptance is claimed. The rejected push is not retried.
Other synthetic/source request producers, namespace/module-body recursion,
module/source caches, other heap/native buffers, allocator/error-message overhead,
shared programs and integrated acceptance remain open. M1u remains the latest
accepted milestone.

Evidence paths: `/private/tmp/blueice-module-binding-storage-controls-20261017.py`,
`/private/tmp/blueice-module-binding-storage-rust-inputs-20261017.json`,
`/private/tmp/blueice-module-binding-storage-commit-20261017.txt`,
`/private/tmp/blueice-module-binding-storage-audit-20261017.py`, logs matching
`/private/tmp/blueice-module-binding-storage-*-20261017.log`, red-phase
source copies in `/private/tmp/blueice-module-binding-storage-red-source-20261017/`,
and the separate compile attempt in
`/private/tmp/blueice-module-binding-storage-compile-attempt-20261017/`, and
`target/module-binding-storage-source-20261017.json`. This remains an M1z
implementation checkpoint with wider acceptance open.

#### Source/import-meta identity-cache follow-up (implementation checkpoint, 2026-10-17)

Acceptance: retain all four source/import-meta identity/root table allocations
and key capacities in the document native budget. Prepare complete publication
storage before managed allocation, preserve old identities on failure, and keep
lookup hits allocation-free at full healthy quota. Cold source prototypes must
survive allocation GC without an unadmitted operand buffer.

- [x] Preserve four old-code failures: **0/4 passed**. Import-meta hit/cold
      calls allocate 1/4 times where no allocation is permitted; the first table
      layer reports zero instead of 120 bytes, and its padded key reports zero
      instead of 64 bytes.
- [x] Retain cache backing and names, stage complete publication, borrow hit
      lookup names and protect source prototype inputs through the heap API.
- [x] Pass expanded finite checks, fixed offline controls, formatting and Clippy;
      complete source/artifact and commit audits with detailed English text.

Each cache/root pair uses retained allocation ledgers plus all live String key
capacities. Prepared publication owns two copied names and complete candidate
maps before any heap object is created; the owner is tied to that VM's intrinsic
identity and document account. Abandonment drops buffers before refund, including
when Rust retains a preparation after VM destruction. Publication registers the
heap root first, moves native storage, refunds staging and immediately reconciles
the VM owner without intervening allocation or callback.

Import-meta hits borrow the active module name or fallback literal; distinct
modules keep distinct null-prototype objects. Source identities and metadata
identities stay separate. Source validation preserves existing error kinds.
The source constructor passes its prototype directly to protected heap
allocation; native trace preparation performs no managed collection before that
call. The redundant operand-stack temporary is removed. Root/allocation failure
leaves cache entries and the cold-prototype field unpublished.

Capacity preservation covers managed allocation/GC while candidates are live.
Optional sparse reclamation separately admits complete replacements, releases
each adopted reduction before dropping its lease and funds the next table with
the refunded headroom. Refusal stays nonterminal; retained name buffers and root
IDs move unchanged. Deleted keys refund payload while surviving table backing
remains charged.

Twenty-three finite groups cover the four original observations, identity/hit
behavior, fallback/module names, unchanged preparation state, detached owners,
exact/short preparation and publication, source refusal before heap/prototype
work, root/managed failure and retry, GC/VM-drop ownership, retained deletion
backing, refused/exact/retried sparse reclamation, preserved candidate shapes
during GC, stack-free cold-source prototype protection, host-neutral behavior,
validation errors and real source/meta module identity. Twenty-one functional
groups are exposed through the ordinary-library entry point; two allocation
observations remain unit-only.

The first expanded run passes **23/23**. The runner passes **1773 distinct
selected tests** across 50 executables and 165 fixed selections: 23 new groups,
two inspected finite public source/import-meta identity contracts and all 1748
prior selected controls. Formatting, diff checks and offline workspace/all-target
Clippy with warnings denied pass. Nine frozen Rust input hashes and eleven final
source/document inputs tie the source/artifact, staged and committed audits to
the logs, executable hashes, Cargo inputs and detailed English commit message.
The same nine explicit resource/infinite-loop skips and finite public filters
remain. The initial expanded compile attempt referenced a nonexistent VM field;
its source/log manifest remains separate from behavioral red evidence. The owner
guard now uses the existing fixed intrinsic object identity.
Synthetic/source request producers, namespace/module-body recursion, other
module/source caches, other heap/native buffers, allocator/error-message overhead,
shared programs and integrated acceptance remain open. M1u remains the latest
accepted milestone; the prior rejected push is not retried.

Evidence paths: `/private/tmp/blueice-module-identity-cache-controls-20261017.py`,
`/private/tmp/blueice-module-identity-cache-rust-inputs-20261017.json`, logs
matching `/private/tmp/blueice-module-identity-cache-*-20261017.log`, original
source inputs in `/private/tmp/blueice-module-identity-cache-red-source-20261017/`,
the separate compile attempt at
`/private/tmp/blueice-module-identity-cache-compile-attempt-20261017/`,
`/private/tmp/blueice-module-identity-cache-audit-20261017.py`,
`/private/tmp/blueice-module-identity-cache-commit-20261017.txt` and
`target/module-identity-cache-source-20261017.json`.

#### Static synthetic-request preparation follow-up (implementation checkpoint, 2026-10-17)

Acceptance: admit canonical name-sorting scratch and complete static synthetic
target backing before producing target keys. Each resolved key retains its own
native owner through registration. Keep canonical module order, import-then-
export order, duplicate occurrences, source-phase exclusion and resolution-error
ordering. Resolve the complete batch before any synthetic record is published.

- [x] Preserve four old-code failures: **0/4 passed**. A scan with no synthetic
      targets still allocates once at zero headroom; a text request allocates
      six times. Retained target buffers report zero instead of 154 bytes both
      before and after destruction of the originating VM.
- [x] Own complete scratch/target backing and each resolved key, retaining
      charges through ordinary registration, yielded keys and abandoned drains.
- [x] Pass expanded finite cases, inherited controls, formatting and Clippy;
      complete source/artifact and commit audits with detailed English text.

The red run extracts the original collecting expressions into a read-only
preparation seam without adding budget checks; source copies retain those exact
producer expressions. The implementation uses admitted borrowed-key sorting
scratch, a complete target-vector candidate and the existing admitted module-
name resolver. Imports, indirect/star/namespace/deferred namespace exports and
their type suffixes follow the old ordering. Ordinary JavaScript requests and
source imports/exports remain excluded. Duplicate targets retain distinct
occurrences; existing record construction still provides idempotency.

The batch holds vector backing while its drain is live. Each yielded key keeps
its independent document charge through host record registration, and may outlive
the batch or VM. Abandoned iteration destroys remaining names before releasing
their leases; backing survives until batch destruction. Complete resolution
failure precedes graph mutation and refunds all partial preparation. Native
admission failure keeps its existing terminal document policy.

Twenty-two finite groups cover original observations, empty/terminal documents,
ordinary/source exclusions, canonical/request/type order, four re-export routes,
duplicate targets, exact and one-short peak room, target backing before invalid
specifier resolution, partial failure refunds, no-target scratch refunds,
yielded/abandoned ownership, unchanged graph/heap state, canonical errors,
complete resolution before publication, missing host sources, host-neutral
idempotency and real mixed JSON/text/bytes module imports. Twenty functional
groups are exposed through the ordinary-library entry point; two allocation
observations remain unit-only. The completed focused run passes **22/22**.
Final validation passes **1796 distinct selected tests**, across 50 executables
and 167 fixed selections: twenty-two new groups, one inspected finite public
synthesis-rollback case and all 1773 prior selected controls. The nine explicit
resource/infinite-execution skips and finite public filters remain unchanged.
Formatting, diff checks and offline workspace/all-target Clippy with warnings
denied pass. Six frozen Rust inputs and eight final source/document inputs tie
source/artifact, staged and committed audits to the executable/log hashes, Cargo
inputs and detailed English commit message.
The first expanded run passed **20/22**. Its separately archived source and log
retain two measurement mistakes: subtraction used key length instead of retained
capacity, and heap stats preceded the test's own ceiling setup. Both measurements
are corrected while keeping exact ownership and full heap-state assertions.
The first fixed-control run passes **1796 distinct cases**, while offline Clippy
identifies the two allocation-based key helpers whose last production caller
moved to the admitted resolver. They now remain only in test/coverage builds as
finite fixture oracles. Their source/log archive is preserved before that change;
no dead-code warning suppression is used. The same fixed selections pass against
the final frozen Rust inputs, and final Clippy passes.

Synthetic Bytecode binding/scope/export metadata is still constructed without
its own producer lease; the registry admits that input later. Dynamic source
compilation, other module/source caches and native/heap buffers, namespace and
module-body recursion, allocator/error-message overhead, shared programs and
integrated acceptance remain open. M1u remains the latest accepted milestone.
The prior rejected push is not retried.

Evidence: `/private/tmp/blueice-synthetic-request-storage-controls-20261017.py`,
`/private/tmp/blueice-synthetic-request-storage-rust-inputs-20261017.json`, logs
matching `/private/tmp/blueice-synthetic-request-storage-*-20261017.log`, source
archives at `/private/tmp/blueice-synthetic-request-storage-red-source-20261017/`,
`/private/tmp/blueice-synthetic-request-storage-first-source-20261017/` and
`/private/tmp/blueice-synthetic-request-storage-clippy-attempt-20261017/`,
`/private/tmp/blueice-synthetic-request-storage-audit-20261017.py`,
`/private/tmp/blueice-synthetic-request-storage-commit-20261017.txt` and
`target/synthetic-request-storage-source-20261017.json`.

#### Synthetic Bytecode preparation follow-up (implementation checkpoint, 2026-10-17)

Acceptance: admit and retain binding, scope, export and name buffers before
synthetic value conversion. Keep complete code charged while Rust retains it,
including after VM destruction. Transfer the same owner into registry admission
without charging code twice, preserve existing data on failure, and reject
foreign-document or bounded-to-unbounded transfers.

- [x] Preserve four old-code failures: **0/4 passed**. Metadata allocates six
      times at zero headroom; a full record allocates seven times with only the
      export-owner allowance. Retained metadata reports zero instead of 562
      bytes both before and after destruction of its VM.
- [x] Retain prepared metadata and registry input storage through complete
      admission and publication, with data destroyed before allocation refunds.
- [x] Pass expanded finite cases, inherited controls, formatting and Clippy;
      complete source/artifact and commit audits with detailed English text.

The red phase extracts the original metadata expressions without adding native
admission. Prepared code now owns its Bytecode/key fields before its document
charge. A complete minimum reservation precedes allocation; each actual backing
capacity replaces its outstanding minimum before subsequent construction. Exact
one-element vectors and both default names keep the same immutable default
binding, strict/module flags, scope slot and empty evaluation stream.

Missing host sources preserve their existing type error before metadata work.
Complete metadata precedes value conversion and root preparation. Value
conversion remains a separately callable internal boundary: existing value/JSON/
root ownership fixtures retain all numerical allowances and assertions while
providing their metadata as explicit host input. New complete-code fixtures
exercise the VM producer and registry handoff together.

Registry insertion accepts the prepared owner, checks its document, admits key/
header storage and any coexisting source copy or replacement table before moving
live data. Complete preparation precedes mutation. Publication moves code and
key buffers without allocation, merges same-document leases and releases surplus
backing/replaced payload without a new admission check. The ordinary host-code
entry point uses the same data-before-lease guard.

Twenty-six finite groups cover the four original observations, metadata shape,
exact/short room, terminal scopes, unchanged managed state, separate value/code
ownership and VM destruction, missing-source precedence, exact registry adoption,
full-quota spare publication, six-buffer pointer preservation, foreign/unbounded
owner rules, source-copy and table-growth failure, replaced-buffer refunds,
registry snapshots after VM destruction, root/managed/instruction failure and
retry, host-neutral values and actual rooted object collection. Twenty-four
functional groups are exposed through the ordinary-library entry point; two
allocation observations remain unit-only. The completed focused run passes
**26/26**. Final validation passes **1822 distinct selected tests**, across 50
executables and 168 fixed selections: twenty-six new groups and all 1796 prior
selected controls. The nine explicit resource/infinite-execution skips and finite
public filters remain unchanged. Formatting, diff checks and offline workspace/
all-target Clippy with warnings denied pass. Nine frozen Rust inputs and eleven
final source/document inputs tie source/artifact, staged and committed audits to
executable/log hashes, Cargo inputs and detailed English commit text.
The first expanded run passed **24/26**. Its separate source/log archive preserves
two fixture assumptions: primitive JSON does not consume a per-node instruction
charge, and object construction retains an admitted operand slot. The corrected
cases use one fixed object property for instruction refusal and include the
operand scratch before measuring refunds, retaining exact ownership and error
assertions.

Dynamic source parsing/compilation, other module/source caches and native/heap
buffers, namespace/module-body recursion, allocator/error-message overhead,
shared programs and integrated acceptance remain open. M1u remains the latest
accepted milestone; the prior rejected push is not retried.

Evidence: `/private/tmp/blueice-module-code-storage-controls-20261017.py`,
`/private/tmp/blueice-module-code-storage-rust-inputs-20261017.json`, logs matching
`/private/tmp/blueice-module-code-storage-*-20261017.log`, original source copies
at `/private/tmp/blueice-module-code-storage-red-source-20261017/`, the first
expanded attempt at `/private/tmp/blueice-module-code-storage-first-source-20261017/`,
`/private/tmp/blueice-module-code-storage-audit-20261017.py`,
`/private/tmp/blueice-module-code-storage-commit-20261017.txt` and
`target/module-code-storage-source-20261017.json`.

#### Iterative namespace construction follow-up (implementation checkpoint, 2026-10-17)

Acceptance: build nested namespace re-exports with constant namespace host-call
depth and admitted explicit frames. Keep name/result payloads owned independently
of frame movement, maintain eager/deferred identities and live/source bindings,
and preserve outer transaction rollback, temporary roots and capacity policy.

- [x] Preserve three old-code failures through public `execute_module_graph`:
      **0/3 passed**, with 3/4/2 simultaneous namespace calls for a three-module
      chain, a three-module cycle and a deferred chain. The cases observe actual
      call nesting without overflowing or exhausting a stack.
- [x] Use an admitted explicit DFS controller with movable payload owners,
      cached placeholders, child-result continuations and outer rollback.
- [x] Pass fixed finite controls, formatting and Clippy; complete source/artifact
      and commit audits with detailed English text.

Each frame retains canonical borrowed module/export names, independently owned
UTF-16/output buffers, progress and a pending child identity. Complete coexisting
frame replacements are admitted before allocating or publishing another child.
Export preparation retains an expected count rather than borrowing the frame's
own name vector, so moving/growing the frame array creates no self-reference.
Input-name storage may be destroyed before publishing the already-owned output.

The controller preserves sorted export traversal and depth-first completion:
publish/root a placeholder, finish a cold child, then construct the parent's
immutable alias cell. Cached cyclic/self edges reuse that placeholder immediately.
Eager/deferred objects remain distinct, deferred `then` filtering precedes target
resolution, and source aliases retain the host's stable opaque identity. Local
cells move unchanged; missing/ambiguous exports remain omitted.

The outer call keeps one namespace transaction depth increment. Every error
drops frame payloads before rolling back new cache epochs and temporary roots;
existing namespaces and inherited outer transaction/capacity policy survive.
Invalid initial publication epochs refuse before allocating the controller, retaining
the existing allocation-free rejection. The export-name refusal fixture now
previews both the controller and cache preparation, preserving its exact
allocation-count assertion at the original name-traversal boundary.

The first focused run passes **21/21**. Eighteen functional groups cover detached,
exact/short/spare/growing frame storage, independently owned name input, payload
validity after frame movement, cyclic identities through GC, eager/deferred
self-aliases, source/local/ambiguous exports, late failure after frame growth,
cache hits at full quota, inherited policy, root refusal and host-neutral cycles.
Three unit-only observations measure actual active namespace calls. Functional
groups are exposed through the ordinary-library entry point. The first passed
source/log archive precedes the explicit initial invalid-epoch preflight restored
during review. Final validation passes **1843 distinct selected tests**, across
50 executables and 169 fixed selections: twenty-one new groups and all 1822 prior
selected controls. The nine explicit resource/infinite-execution skips and finite
public filters remain unchanged. Formatting, diff checks and offline workspace/
all-target Clippy with warnings denied pass. Five frozen Rust inputs and seven
final source/document inputs tie source/artifact, staged and committed audits to
executable/log hashes, Cargo inputs and detailed English commit text.

Module-body evaluation recursion, dynamic source parsing/compilation, other
module/source caches and native/heap buffers, allocator/error-message overhead,
shared programs and integrated acceptance remain open. M1u remains the latest
accepted milestone; the prior rejected push is not retried.

Evidence: `/private/tmp/blueice-namespace-build-stack-controls-20261017.py`,
`/private/tmp/blueice-namespace-build-stack-rust-inputs-20261017.json`, logs matching
`/private/tmp/blueice-namespace-build-stack-*-20261017.log`, original source copies
at `/private/tmp/blueice-namespace-build-stack-red-source-20261017/`, the first
passed attempt at `/private/tmp/blueice-namespace-build-stack-first-source-20261017/`,
`/private/tmp/blueice-namespace-build-stack-audit-20261017.py`,
`/private/tmp/blueice-namespace-build-stack-commit-20261017.txt` and
`target/namespace-build-stack-source-20261017.json`.

#### Iterative module-dependency evaluation follow-up (implementation checkpoint, 2026-10-17)

Acceptance: traverse module evaluation dependencies with admitted explicit
frames, preserving DFS/cycle/source order, async dependency roots, completion
and thrown-value identity. Retain request/target/pending owners across JavaScript
and unwind every active importer without recursive dependency calls. Body
execution, await/debugger continuations and legitimate JavaScript-driven module
reentry must keep their existing context semantics.

- [x] Preserve three old-code failures through public `execute_module_graph`:
      **0/3 passed**, with 3/4/3 simultaneously active evaluation calls for a
      small chain, cycle and await chain. These observe call nesting without a
      resource-exhaustion or stack-overflow workload.
- [x] Admit movable evaluation frames before marking a child active, retain
      request/target/pending owners and unwind errors through explicit frames.
- [x] Pass fixed finite controls, formatting and Clippy; complete source/artifact
      and commit audits with detailed English text.

Each frame borrows its canonical name from the immutable registry and retains a
code snapshot, throw epoch, request position, owned target iterator, waiting
target and pending dependency names. Full coexisting frame replacement precedes
child activation. Completed/busy/error records keep their existing fast paths.
Deferred requests retain the existing asynchronous-frontier traversal, and
suspended children still register parents against their asynchronous cycle root.

Body execution uses the existing declaration/context entry, fresh sibling fuel,
parked linked records, interpreter, exit-cell staging and continuation transfer.
Finishing a frame drops request/pending owners before completion/error retention.
A dependency error then finishes active importers in DFS order while preserving
one JavaScript error identity and its origin. Completion cleanup now clears an
unsuspended record's active flag even if error allocation or retention refuses.
Suspended records retain the marks required by their continuation.

The first focused run passes **24/24**. Twenty-one functional groups cover
detached/exact/short/spare/moved frame storage, request payload validity after
growth, child activation refusal, entry/cache admission, diamond/cycle order,
awaited sibling fuel and parent order, asynchronous-only deferred traversal,
deferred-property reentry with import-meta restoration, shared/replayed language
errors, deep fixed-graph error unwind, instruction/resource exits, error-object
and retention refusal, suspended flags, actual GC across a pending await,
successful record flags and host-neutral order. Three unit-only observations
measure dependency-call nesting. Functional groups are exposed through the
ordinary-library entry point. Final validation passes **1867 distinct selected
tests**, across 50 executables and 170 fixed selections: twenty-four new groups
and all 1843 prior selected controls. The nine explicit resource/infinite-
execution skips and finite public filters remain unchanged. Formatting, diff
checks and offline workspace/all-target Clippy with warnings denied pass. Three
frozen Rust inputs and five final source/document inputs tie source/artifact,
staged and committed audits to executable/log hashes, Cargo inputs and detailed
English commit text. A source comparison confirms preserved body expressions
after immutable code borrowing and the formatter's optional argument comma.

This removes recursive dependency traversal per evaluation entry. JavaScript
observing a deferred namespace can still start a separate evaluation controller
through the existing displaced execution context; that valid reentry is retained.
Dynamic source parsing/compilation, remaining async-notification/task/native/heap
buffers, allocator/error-message overhead, shared programs and integrated
acceptance remain open. M1u remains the latest accepted milestone; the prior
rejected push is not retried.

Evidence: `/private/tmp/blueice-module-evaluation-stack-controls-20261017.py`,
`/private/tmp/blueice-module-evaluation-stack-rust-inputs-20261017.json`, logs
matching `/private/tmp/blueice-module-evaluation-stack-*-20261017.log`, original
source copies at `/private/tmp/blueice-module-evaluation-stack-red-source-20261017/`,
the first passed attempt at
`/private/tmp/blueice-module-evaluation-stack-first-source-20261017/`,
`/private/tmp/blueice-module-evaluation-stack-audit-20261017.py`,
`/private/tmp/blueice-module-evaluation-stack-commit-20261017.txt` and
`target/module-evaluation-stack-source-20261017.json`.

#### Module notification storage follow-up (implementation checkpoint, 2026-10-17)

Acceptance: admit completion/rejection queue backing and seed names before graph
mutation. Admit a complete possible parent batch before reverse-row detachment,
retain detached vectors/names independently across JavaScript and VM destruction,
and keep sibling/BFS notification order and shared error identity.

- [x] Preserve four old-code failures: **0/4 passed**. Completion and rejection
      each allocate a queue at zero native headroom; refusal consumes the reverse
      parent row or the rejecting module's dependency edges.
- [x] Own queue/name storage, pre-admit complete batches and transfer detached
      parent-vector ownership without checking or charging its bytes twice.
- [x] Pass expanded finite cases and corrected observation; retain unaffected
      controls with unchanged Rust/Cargo inputs. Pass formatting and Clippy;
      complete source/artifact and commit audits with detailed English text.

Queue inputs now borrow the caller's module name and copy it through the admitted
name producer, removing upstream unadmitted String clones. The queue owns a
complete admitted VecDeque allocation and independent ModuleName entries. A
prepared parent batch can enqueue without backing growth; popping a name leaves
its own charge live through callbacks or later queue destruction.

Reverse-row detachment first synchronizes VM ownership, then moves its reported
vector capacity and actual key capacities into an independent shared charge.
The aggregate account stays unchanged during that allocation-free handoff;
reconciliation refunds the removed map key. The iterator retains backing and
remaining names, splitting each yielded name's charge without copying. VM death
therefore cannot erase charges for Rust-held detached work.

Completion reserves all potential parent slots before detachment and preserves
source/BFS order. A ready parent's language failure immediately runs rejection
before the next sibling, moving its owned name into that work queue. Rejection
prepares queue capacity before dependency removal and rechecks the parent batch
after waiter settlement. Error rooting precedes a newly rejected parent's flag/
error publication. Existing completed work and callback effects retain their
normal ordering; subsequent resource failures remain aborts.

Twenty-six finite groups cover original observations, retained/detached/exact/
short/wrapped/spare/growing queues, prepared publication at full quota, document
handoff rules, unbounded input attachment, full-quota parent transfer, VM-drop
iterator ownership, padded-buffer pointer preservation, whole-batch refusal,
public BFS and failed-sibling order, rejection identity through GC, error-root
refusal, missing rows, host-neutral notifications, terminal scopes and moved
seed identity. Twenty-four functional groups are exposed through the ordinary-
library entry point; two allocation observations remain unit-only. The first
focused run passes **26/26**.

The initial 171-selection control run passes **1892** cases and fails one legacy
observation: independent notification leases no longer contribute to the ambient
displaced-frame subtotal. Preserve that entire run, all ten then-current Rust
inputs and its executable hashes. A first conversion to a pre-checkpoint account
delta also fails (**15/16**): the completed await/dependency owners are released
before callbacks. Preserve that attempt separately. Observe the real document
account during callbacks and compare it with post-checkpoint retained ownership;
the original **153-byte minimum** remains unchanged. The corrected focused run
passes **16/16**.

After the cybersecurity warning, reuse the other **170 passing selections**;
do not replay the broad runner. All nine original Rust-input hashes and Cargo
inputs are unchanged; only the legacy observation fixture differs. Replacing its
failed selection with the focused result provides evidence for **1893 distinct
selected tests**, across 50 executable targets and 171 contributing selections,
including all 1867 prior selected controls. Keep the nine explicit skips and
finite public filters. Formatting, diff checks and offline workspace/all-target
Clippy with warnings denied pass. Source/artifact and commit audits distinguish
retained executable hashes from the focused rerun, bind ten final Rust inputs and
twelve source/document files, and confirm the module evaluation controller/body
are unchanged from the parent checkpoint.

Dynamic source parsing/compilation, other task/native/heap buffers, allocator/
error-message overhead, shared programs and integrated acceptance remain open.
M1u remains the latest accepted milestone; the prior rejected push is not retried.

Evidence: `target/module-notification-storage-source-20261017.json` and prefix
`/private/tmp/blueice-module-notification-storage-`, including original red/first
source copies, full control/observer attempt archives, frozen Rust inputs,
retained fixed controls and the focused observation log.

#### ImportCall argument-storage follow-up (implementation checkpoint, 2026-10-17)

Acceptance: refuse entry storage before operand copies, root-vector growth or
Promise creation; move input values with independent native ownership and retain
their object edges through conversion callbacks. Admit UTF-8 specifier storage
before allocation and retain it across option getters. Keep the attribute-key
snapshot charged through descriptor/value traps, preserve phase/type routing and
language-error rejection, and refund temporary owners/root entries on every exit.

- [x] Preserve old-code entry-allocation/capacity and callback-accounting failures.
- [x] Implement admitted input/root preparation and independently owned argument
      validation storage, preserving ordinary import/defer/source behavior.
- [x] Run focused offline finite cases and relevant public controls, formatting
      and Clippy; bind source/artifact evidence to a detailed English commit.

All three original cases fail (**0/3 passed**): zero native headroom still permits
two allocations, refusal changes operand-stack capacity from zero to four, and
changing a two-byte specifier to a seven-byte Unicode specifier changes callback
accounting by zero rather than five. The red source archive retains the original
ImportCall production body unchanged from the parent checkpoint.

Inputs now move without copies, retaining independent native ownership while
their object references and the Promise capability occupy prepared temporary
root storage. A UTF-16 module-name producer validates and computes exact UTF-8
length without allocating, then admits its complete output before encoding.
Existing string inputs are borrowed. Generic coercion's returned UTF-16 buffer
attaches an owner before UTF-8 output preparation. The converted name stays
charged during options validation; the input lease drops after values are
consumed. Temporary attributes remain rooted through all traps.

Returned own-key snapshots retain actual vector/name capacities, including
symbol descriptions, through descriptor/value callbacks. Recognized attribute
types compare directly as UTF-16 strings. Ordinary import/defer jobs prepare
referrer copies through the owned name producer and return publication refusal
directly. Language errors still reject with their original identity; uncatchable
errors remain aborts. All exits release temporary owners/root entries while
reusable root capacity stays charged to its VM.

Fourteen fixed groups cover the three original failures, padded input capacity,
prepared root-slot reuse at full quota, exact/one-short UTF-8 admission, empty and
ill-formed strings, VM-drop ownership, complete attribute-key capacity/refusal,
public callback order and roots through explicit GC, thrown identity, callback
abort cleanup and numerical job-count refusal. Thirteen functional cases have an
ordinary-library entry point; one allocation observation remains unit-only.
The first expanded run passes **14/14**. Five related library selections cover
Promise queues (28), temporary roots (17), import delivery (19), dependency
ownership (16) and notifications (26). Ten existing dynamic-import cases and
seven exact attribute/type/defer controls also pass: **137 distinct selected
tests**, across five executable targets and 14 contributing selections. Formatting,
diff checks and offline workspace/all-target Clippy with warnings denied pass.
Source/artifact and commit audits bind six frozen Rust inputs and eight source/
document files, preserve red/first evidence and confirm the evaluation controller
and body remain unchanged. This focused evidence does not renew the earlier
broad inventory.

Generic coercion and own-key producers can still allocate before returning to
this boundary. Their preallocation work, dynamic source compilation, shared
programs and integrated acceptance remain open. Do not replay the broad control
runner, pressure/infinite-execution cases or network/Core/MCP services. M1u remains
the latest accepted milestone; the previously rejected push is not retried.

Evidence: `target/import-call-storage-source-20261017.json` and prefix
`/private/tmp/blueice-import-call-storage-`, including red/first source archives,
frozen Rust inputs, explicit focused selections and logs.

#### Source-phase import admission follow-up (implementation checkpoint, 2026-10-17)

Acceptance: synchronize native ownership before source-phase work, admit canonical
name storage before resolver allocation, borrow the ambient referrer, and return
latched settlement/thenable-job failures through the fallible entry. Remove the
obsolete rejection assertion. A refused settlement keeps the pending Promise
and its reaction unchanged. Preserve source identity,
ordinary type/path language errors and successful no-job settlement when another
realm uses the shared microtask count. Keep source getter effects and normal
thenable behavior after committed source lookup.

- [x] Preserve original resolver-allocation, terminal-entry and masked-refusal
      failures using tiny fixed fixtures and scalar quota peers.
- [x] Implement source-phase name admission and propagation of latched settlement
      and thenable-job failures through the fallible source entry.
- [x] Pass focused source/import/Promise controls, formatting and offline Clippy;
      record source/artifact and commit audits with a detailed English message.

All three original cases fail (**0/3 passed**). Zero native headroom and terminal
entry each permit **three resolver allocations** before refusal. With one source
Promise reaction and a scalar peer holding the shared microtask count, rejection
returns `Ok(())` after common settlement latches document failure. A host panic
was not reproduced; the failure is a successful return with the Promise still
pending. The red source archive retains the original source-entry body unchanged
from the parent checkpoint.

The source entry now synchronizes ownership before lookup, borrows its referrer
and resolves through admitted module-name storage. Lookup's temporary name drops
before Promise resolution/rejection callbacks. Uncatchable lookup errors return
directly. Common native settlement retains its latching contract; the fallible
source entry reconciles VM ownership and checks the document failure before
reporting success. The obsolete rejection-allocation assertion is removed.
This covers fulfillment/rejection fan-out and thenable-job refusal while keeping
pending reactions, unqueued work, source identity and committed getter/cache
effects. A full shared microtask count still permits settlement that creates no
new job.

Fourteen fixed groups cover early/terminal allocation refusal, rejection and
fulfillment fan-out, thenable-job refusal without calling `then`, full-count
no-job fulfillment/rejection, cached identity/root preservation, normalized
requests, temporary-name refunds, thrown reason identity, explicit GC in a then
getter, host-neutral calls and public source Promise delivery. Twelve functional
contracts have an ordinary-library entry point; two allocation observations
remain unit-only. The first expanded run passes **14/14**.

The initial related selection passes **86** cases; Clippy then reports that the
allocation-based resolver has only test references. Preserve that full source/
log/artifact state. Restrict the reference to test/coverage configurations and
rerun the same eight fixed selections on four frozen final Rust inputs: **86
passed**. A supplemental **19/19** name-producer selection validates the retained
reference and path behavior, yielding **105 distinct selected tests**, across
four executable targets and nine contributing selections. Related controls
include ImportCall inputs (14), source/meta identity caches (23), common Promise
settlement (22), public dynamic import (10) and three exact static/source/
synthetic cases. Formatting, diff checks and offline workspace/all-target Clippy
with warnings denied pass. Source/artifact and commit audits bind six source/
document files, preserve all earlier evidence and confirm the remaining deferred
helpers and module evaluation controller/body are unchanged. This focused result
does not renew the earlier broad inventory.

Dynamic compilation, generic coercion/own-key producer preallocation, shared
program storage and integrated acceptance remain open. M1u remains the latest
accepted milestone. Do not replay broad/pressure/infinite-execution or network/
Core/MCP validations, and do not retry the previously rejected push.

Evidence: `target/import-source-admission-source-20261017.json` and prefix
`/private/tmp/blueice-import-source-admission-`, including red/first/pre-cfg source
archives, frozen Rust inputs, the fixed runner, supplemental name control and logs.

#### Dynamic module compilation-input follow-up (implementation checkpoint, 2026-10-18)

Acceptance: module lowering borrows the parsed body, including diagnostic
compilation, and strict early errors precede unused diagnostic inventories.
Dynamic compilation admits unavoidable source/EOF parser storage before parsing,
then gives the returned AST an independent document owner. Deduplicate all
SourceText allocations, including template-fragment parser sources, through
separately admitted accounting scratch. Keep the AST charged through owned root-code
preparation and drop it before registry copying/publication. Preserve lazy
syntax errors, import/export metadata and module execution semantics.

- [x] Preserve old-code body-copy/early-error and dynamic parse-before-refusal
      failures using fixed tiny sources and scalar quota peers.
- [x] Implement borrowed-body lowering and independently owned compilation input.
- [x] Pass focused finite compiler/module controls, formatting and offline
      Clippy; record source/artifact audits and a detailed English commit.

Four original cases fail (**0/4 passed**): ordinary/diagnostic strict early errors
perform **7/11 allocations**, while zero-headroom ordinary/empty dynamic sources
perform **57/7 allocations** before refusal. Red source copies preserve the
unchanged compiler and dynamic-compilation body from the parent checkpoint.

Script, module and diagnostic entry points now lower a borrowed statement slice.
Module bodies are no longer cloned into a temporary Program; pure strict
restricted-name validation precedes diagnostic inventories that an error would
discard. The parser exposes its unavoidable copied-source/EOF-token/position
lower bound for preflight, including empty input. This is not its full allocation
budget. Returned ASTs own reported vector/string/box/source payloads through an
independent shared document charge, using the existing logical BigInt-limb and
allocator/control-block exclusions.

An initial **20/20** selection missed a source-identity assumption. A valid module
with a function in a template placeholder returns a separately parsed SourceText
allocation; the added counterexample fails (**0/1 passed**) with SchedulingLimit.
Preserve both states. The corrected accounting traverses structural buffers
without allocation and admits every distinct source payload before growing
separately owned deduplication scratch. Complete coexisting hash-set candidates
retain reported slot charges; scratch drops before the AST owner returns. Mixed
and template-fragment source allocations are counted once each.

The AST remains charged until a PreparedModuleCode admits its unique root buffers.
It drops before registry copying/publication, and the prepared root owner moves
into registry adoption. Existing/missing sources keep their no-work paths.
Syntax/root refusal publishes no new module and refunds the input. Parser resource
errors retain their resource type. Foreign/unbounded escape and terminal rules
match other native owners. The unprepared registry adapter now serves fixtures
only and is restricted to test/coverage configurations.

The corrected expanded run passes **22/22**: five allocation observations remain
unit-only, and seventeen functional contracts also have an ordinary-library
entry point. Cases cover metric compatibility and source identities, VM-drop
ownership, retained/scratch exact and one-short budgets, AST/root coexistence,
refusal refunds/publication, lazy syntax errors, attachment/foreign/terminal rules,
and a public module retaining imports, exports, await, template execution and
function source text after input destruction. Nineteen registry controls, ten
dynamic-import cases, two AST metric controls, two existing diagnostic module
cases, four exact live/cycle/lazy-error cases and one function/template source
case pass: **60 distinct selected tests**, across five executable targets and
twelve contributing selections. A pre-lint 60-pass state and its Clippy fallback
finding are archived; the equivalent cleanup is followed by the same final
60-case selection. Formatting, diff checks and offline workspace/all-target
Clippy with warnings denied pass; source/artifact and commit audits record the
checkpoint.

Source/artifact audits bind ten frozen Rust inputs and twelve source/document
files, preserve original/intermediate/template/final evidence, and compare the
unchanged eval/later compiler helpers and module evaluation controller. The focused
checks do not renew the earlier broad inventory.

This does not close token/AST/compiler producer preallocation, allocator/BigInt-
spare exclusions, shared compiled child code/source/diagnostic storage or
integrated acceptance. In particular, the root-code handoff does not transfer
ongoing charges for compiled shared sources/children. M1u remains the
latest accepted milestone. Broad, pressure/infinite-execution and network/Core/
MCP replay remain prohibited; the rejected push is not retried.

Evidence: `target/module-compilation-input-source-20261018.json` and prefix
`/private/tmp/blueice-module-compilation-input-`, including 2026-10-17 red and
single-source/template archives plus 2026-10-18 final inputs, controls and logs.

#### Compiled dynamic-module source follow-up (implementation checkpoint, 2026-10-18)

Acceptance: transfer independently shared source allocations from parsed inputs
into runtime code before publication. Keep the source charged while any child
code, closure, timer/Promise callback, generator or saved frame retains it, even
after root-code/VM destruction. Release distinct template-fragment sources
independently and preserve ordinary host-compiled code's cross-VM reuse.

- [x] Reproduce premature refunds using three fixed tiny source-lifetime cases.
- [x] Transfer source payload leases and admit owner/lookup metadata separately;
      bind existing and incoming runtime code on document activation.
- [x] Finish fixed related controls, offline formatting/Clippy, source/artifact
      audits and a detailed English commit.

The original three cases fail (**0/3 passed**): child code still retains source
text after the native account has refunded it. Per-source owners now receive
existing AST payload charges by transfer, without admitting that payload twice.
Owner headers and complete coexisting lookup candidates have independent native
charges. AST and lookup data drop before their leases; compiled source owners
survive in shared code until the final holder drops. No Rc native owner is added
to the Send/Sync AST source type. Fresh code is traversed before publication
without copying bytecode or allocating traversal storage.

Unbounded runtime code keeps attachable source owners. Activation visits retained
heap closures/generators and saved async/debugger/module execution code;
allocation/adoption checks incoming code before publication. Same-document
attachment is idempotent; foreign/terminal attachment refuses. Ordinary
host-compiled programs remain reusable across independent VMs.

The initial expanded run passes **16/16**. Nineteen new cases now pass, including
one unit-only allocation observation and eighteen functional contracts with an
ordinary-library entry point. They cover source sharing/reclamation, late
attachment, foreign refusal, quota header preflight, exact/one-short compile
peaks, a rooted closure's final GC refund, lookup growth and public timer/Promise
delivery. The pre-lint selection passes **80 distinct selected tests** in fourteen
selections across five executable targets: the new nineteen, the earlier sixty
compiler/module controls and one exact synthetic-value attachment case. Preserve
those inputs and their callback type-complexity finding before the type-alias
cleanup. The same final control selection also passes **80/80** on the twenty
frozen Rust inputs. Final formatting, diff checks and offline workspace/all-target
Clippy pass. Source/artifact and staged/committed content audits bind twenty-two
source/document files, five test executables and the detailed English commit.
These focused checks do not renew any earlier broad acceptance inventory.

Scope is runtime-parsed dynamic-module SourceText ownership. Host-precompiled
sources, shared child-code buffers/headers, diagnostic maps, parser/compiler
producer preallocation and integrated acceptance remain open. M1u remains the
latest accepted milestone. No broad, exhaustion, infinite-execution or network/
Core/MCP replay is authorized; the rejected push is not retried.

Evidence: `target/compiled-source-ownership-source-20261018.json` and prefix
`/private/tmp/blueice-compiled-source-ownership-` (2026-10-18), including the red
input archive, initial 16-pass log, pre-lint 80-pass source/log archive and final
frozen inputs, controls and commit message.

#### Shared dynamic-module child-code follow-up (implementation checkpoint, 2026-10-18)

Acceptance: charge each shared runtime-parsed module child's complete inline
header and owned buffers once, retaining its lease through registry copies,
closures, generators and saved frames. Release siblings and nested children at
their own final code drop. Preserve host-compiled code reuse and admit complete
coexisting copies before metadata changes to shared code.

- [x] Reproduce missing child-buffer ownership with three fixed tiny lifetimes.
- [x] Introduce shared code owners, transfer every code user to their handles,
      and account for copy-on-write metadata edits and cleanup.
- [x] Finish the fixed related selection, source/artifact audits and a detailed
      English commit; retain the broader acceptance gates as open.

The original three cases fail (**0/3 passed**): a child retained after root/VM
drop has only its source lease, and dropping one sibling refunds no child-code
storage. Runtime compilation now installs a native owner for each returned
child's complete inline header and reported owned buffers before root adoption.
The metric keeps existing code/value capacities and logical BigInt exclusions;
Rc/allocator overhead remains excluded. Each owner's code data drops before its
lease. Root-code buffers remain with their prepared/registry owner; independent
source allocations keep their separate leases. No source payload is counted
again by child-buffer ownership.

Shared code handles replace bare Rc<Bytecode> in closures, generator states,
async frames and compiler child tables. Ordinary host code remains unbound and
reusable. Late bounded activation and incoming code adoption attach runtime
headers/buffers even without SourceText. Sharing one owner adds no charge, while
foreign-document adoption refuses. Runtime code cannot use the compiler's
unchecked mutable access.

Metadata edits admit the complete candidate before copying shared code and keep
the original alive. The copy shares unchanged nested children and source owners.
Unique edits avoid a copy, and retained buffers are reconciled even when the
editor returns an error. Terminal cleanup refunds released buffers without
clearing failure. Program code-unit preparation returns a typed native-storage
limit; diagnostic binding propagates its existing resource limit. This is
architectural storage work; M1q's coordinate refinements remain parked.

The initial expanded **15/15** and existing source-lifetime **19/19** pass. The
final expanded run passes **17/17**, including one unit-only allocation
observation and sixteen functional contracts with an ordinary-library entry.
Added cases prove nested copy sharing and public dynamic class/generator
execution. Source-lifetime observations now include the independently owned
child storage while keeping the separate source metric unchanged.

Final validation passes **103 distinct selected tests** in twenty-one fixed
selections across five executable targets: the seventeen new cases, the prior
eighty controls, four exact closure/generator heap controls, one diagnostic-error
compatibility case and one method/accessor source-text case. Formatting, diff
checks and offline workspace/all-target Clippy pass. Source/artifact and staged/
committed content audits bind twenty-three frozen Rust inputs, twenty-five
source/document files, the five executables and a detailed English commit. The
original code/value metric and later scheduling-controller implementations are
unchanged. These checks do not renew earlier broad acceptance inventories.

Other script/eval/Function programs, host-precompiled shared storage, diagnostic
map/registry storage, full compiler producer preallocation, allocator/Rc-control
overhead and integrated acceptance remain open. M1u remains the latest accepted milestone. Validation stays local,
offline, finite and tiny; no broad, exhaustion, infinite-execution or network/
Core/MCP replay is resumed, and the rejected push is not retried.

Evidence: `target/shared-code-storage-source-20261018.json` and prefix
`/private/tmp/blueice-shared-code-storage-` (2026-10-18), with original red
inputs/log, preserved adapter compile findings, initial green inputs/logs and
final frozen Rust sources, fixed controls and commit message.

#### Dynamic Function input/code follow-up (implementation checkpoint, 2026-10-18)

Acceptance: extend retained native ownership to normal, async, generator and
async-generator Function constructors. Keep converted input buffers charged
across later coercion callbacks, own parsed grammar checks without cloning their
function AST, and transfer source/child-code storage through the final closure,
generator, saved frame or timer/Promise callback. Preserve global scope,
independent grammar validation, exact source text and prototype-getter behavior.

- [x] Preserve three old-code failures with fixed tiny inputs and scalar peers.
- [x] Own constructor text, returned ASTs and shared code; remove the cloned
      function AST and capture-vector copy and preserve source identities.
- [x] Complete fixed related controls, source/artifact audits, final documents
      and a detailed English commit.

The original **0/3 passed** run shows absent generated-code source owners,
premature native refunds after VM destruction, and **234 allocations** before
an empty constructor refuses a zero-headroom document. The first corrected
three cases and twenty-two existing module input controls pass.

Module and constructor Programs now share a retained AST carrier and source
visitor. Parser source/EOF minimums precede parsing; returned native structure,
source payloads and deduplication backing stay charged. Repricing keeps existing
admission until scratch is gone. Configuration validates document identity and
admits replacement binding/source payloads before copying. The three constructor
grammar parses keep their Program owner and validate its shape without cloning
the Function. Resource parse errors preserve their resource type.

Constructor parameters/body/filter/wrappers own reported String capacities.
UTF-16 coercion results have a scoped owner before measured lexer encoding;
growing outputs retain complete admitted candidates beside old backing. The HTML
filter uses UTF-8 byte boundaries and an admitted output instead of Vec<char>.
Original HTML-comment source and nested filtered/template sources keep separate
source owners. Returned children acquire independent code headers/buffers, while
temporary root code has its own admitted lease. Captured slots are checked before
root destruction. AST/root/text temporaries drop before prototype getters run.
Capture scratch moves into the managed closure and existing operand roots supply
binding IDs, removing the second capture vector.

The expanded **22/22** pass: one allocation observation stays unit-only and
twenty-one functional contracts have an ordinary-library entry. Cases cover all
four kinds and their execution, AST/text/capture/code lifetimes, source identity
and nested reclamation, Unicode/surrogate/reserved encoding, exact/one-short
string and constructor peaks, foreign/terminal refusal, malformed source and
prototype-getter cleanup, late bounded activation, GC refunds and public timer/
Promise retention through delivery.

Final validation passes **132 distinct selected tests** in twenty-six fixed
selections across eight executable targets: the new twenty-two, the previous
103 controls, three global-scope cases, two independent-grammar cases, one exact
synthesized-source case and one lone-surrogate case. Formatting, diff checks and
offline workspace/all-target Clippy pass. Source/artifact and staged/committed
content audits bind eleven frozen Rust inputs, thirteen source/document files,
the eight executables and the detailed English commit. Public AST metrics,
compiler lowering, shared source/code owner implementations and later builtin
dispatch remain unchanged. These checks do not renew broader acceptance.

Full tokenizer/parser/compiler/coercion producer preallocation, ordinary script/
eval and host-precompiled shared ownership, diagnostic map/registry storage,
allocator/BigInt-spare/Rc-control exclusions and integrated acceptance remain
open. M1u remains the latest accepted milestone. No broad, exhaustion,
infinite-execution or network/Core/MCP replay is resumed; the rejected push is
not retried.

Evidence: `target/dynamic-function-storage-source-20261018.json` and prefix
`/private/tmp/blueice-dynamic-function-storage-` (2026-10-18), including original
red input/log, initial three/module controls, expanded frozen inputs, fixed
controls and commit message.

#### Direct/indirect eval input/code follow-up (implementation checkpoint, 2026-10-18)

Acceptance: retain direct/indirect eval lexer text, returned AST and compiler
environment snapshots through preparation, then own root buffers for execution
and shared source/child code through final users. Capture storage must transfer
to frame maps before guest code, preserving lexical/global environments,
strictness, with/private/super contexts and non-string fast paths.

- [x] Preserve corrected missing-owner and parse-before-refusal failures using
      fixed tiny sources and scalar peers.
- [x] Own returned eval inputs, compiler snapshots and root/child code; release
      capture scratch at frame adoption and preserve caller restoration.
- [x] Finish fixed related controls, source/artifact audits and a detailed
      English commit.

The initial fixture omitted internal-entry fuel and cold global setup; its
InstructionLimit/panic findings are retained separately. After finite fuel and
global setup, **0/4 passed** reproduce the intended contracts: returned closures
lose all native code/source ownership after VM destruction, and each empty eval
path performs **9 allocations** before refusing a zero-headroom document.

Direct/indirect eval now borrow the original UTF-16 argument and own measured
lexer encoding, returned Program structure and distinct source allocations.
Direct eval retains its inherited parse strictness, while indirect eval uses a
fresh Script context. Non-string input keeps its no-code path. Root lowering and
scope resolution remain unchanged; parser resource failures keep their type.

Returned compiler snapshots own visible vectors, duplicate binding-name buffers,
dynamic-capture table capacity and variable/conflict name vectors until captures
are prepared. AST/root/child owners coexist during compilation; input AST and
snapshots release before guest code. Root buffers retain a separate lease for
declaration setup, getters, execution and restoration. Shared child/source owners
survive returned closures, generators, async continuations and queued callbacks.
Global lookup failure uses error propagation rather than an unchecked assumption.

Capture scratch is admitted before gathering IDs and moves into frame maps with
its lease. Adoption destroys the temporary vector and releases its credit before
guest callbacks. Existing internal eval adapters use this same path. Lexical
conflict checks inspect closure code without copying captures or the receiver.
Normal/throw/refusal/terminal cleanup keeps existing restoration behavior.

The initial four cases and expanded **20/20** pass. Two allocation observations
stay unit-only; eighteen functional contracts have an ordinary-library entry.
Cases cover AST/root/source lifetimes, foreign/unbounded inputs, non-string
headroom, template-source independence, compiler snapshot capacities, exact/
one-short admission, scope/private/super preservation, syntax/throw cleanup,
timer/Promise delivery, final GC refunds and late activation. A paired one/four-
slot scratch test observes identical native usage inside guest code, proving
that freed capture capacity is not retained through execution.

Final validation passes **162 distinct selected tests** in thirty-seven fixed
selections across twelve executable targets: the new twenty, the previous 132
controls, eight exact eval/parameter/with semantics cases and two exact frame/
global-admission controls. Formatting, diff checks and offline workspace/all-
target Clippy pass. Source/artifact and staged/committed content audits bind eight
frozen Rust inputs, ten source/document files, twelve test executables and the
detailed English commit. Compiler lowering, AST metrics, parser/encoding and
shared source/code owner implementations remain unchanged. These checks do not
renew earlier broad acceptance inventories.

Temporary BTree snapshot builders, complete parser/compiler producer
preallocation, ShadowRealm.evaluate account handoff, ordinary host-precompiled
shared script ownership, diagnostic map/registry storage and allocator/BigInt-
spare/Rc-control exclusions remain open with integrated acceptance. M1u stays the
latest accepted milestone. No broad, exhaustion, infinite-execution or network/
Core/MCP replay is resumed, and the rejected push is not retried.

Evidence: `target/eval-source-storage-source-20261018.json` and prefix
`/private/tmp/blueice-eval-source-storage-` (2026-10-18), including initial fixture
findings, corrected red input/log, initial green input/log, expanded frozen
inputs, fixed controls and commit message.

#### Eval environment producer follow-up (implementation checkpoint, 2026-10-18)

Acceptance: admit borrowed selection scratch, returned vectors, copied names
and dynamic-capture tables before their construction. Preserve exact active,
captured and live-name precedence, parameter-eval corrections, variable-scope
membership and lexical conflict ordering. Refusal preserves the caller and any
already-published compiler snapshot; all scratch and discarded candidates refund.

- [x] Reproduce allocation before refusal with two fixed tiny snapshots.
- [x] Replace production tree snapshots with admitted borrowed selection and
      fallible name copies; keep old selectors as test-only references.
- [x] Finish expanded finite contracts, fixed controls, source/artifact audits,
      final documentation and a detailed English commit.

The original **0/2 passed** run records **4 allocations** for the visible-binding
snapshot and **2 allocations** for the name lists before zero-headroom refusal.
Both initial cases now pass with zero allocations. An expanded fixture's two
missing mutable declarations are preserved as a separate compiler finding;
the subsequent **13/14** run preserved a fixture assertion that incorrectly
expected the VM's existing **96 bytes** to disappear. Comparing the unchanged
baseline instead gives **14/14 passed**. Neither fixture correction changed
production behavior.

Borrowed candidates sort by name and original selection priority before any
name is copied. The last active declaration wins; captured and live-name
fallbacks keep their first-match order. Discarded candidates retain references
only. Snapshot slots, dynamic-capture capacity and both copies of each selected
name acquire admission first, then reconcile reported capacity before the next
copy. Parameter eval changes metadata only for the original eligible slots.

Variable and lexical selection use admitted borrowed scratch. The function
variable environment remains narrower than visible bindings; catch parameters
remain excluded from lexical conflicts. Ordinary function parameter evaluation
includes body lexicals, while arrows retain their inherited boundary. Global
declarative names append in the original map order, preserving duplicates and
excluding property-backed names. Complete replacement name lists coexist with
the old snapshot until both succeed; publication destroys old data before its
refund and transfers the new leases without double admission.

The fourteen finite cases include the two unit-only allocation observations and
twelve functional contracts exposed through an ordinary-library entry. They
compare retained test-only tree selectors, including duplicate/shadowed binding
selection, parameter-eval metadata/cells, variable/lexical/catch/function/arrow
boundaries and global append order/duplicates. Exact/one-short scratch-plus-
snapshot peaks, replacement refusal/success, VM-independent lifetimes,
foreign/detached rejection, unbounded attachment and terminal cleanup verify
ownership. Fixtures use at most ten binding slots and tiny names with scalar
quota peers.

Final validation passes **176 distinct selected tests** in thirty-eight fixed
selections across twelve executable targets: the fourteen new cases and all
162 preceding eval/Function/module/GC controls. Formatting, diff checks and
offline workspace/all-target Clippy with warnings denied pass. The first
176-case run and its two fixture-style Clippy findings remain preserved; final
validation follows both style corrections with unchanged production sources.
Source/artifact and staged/committed content audits bind five frozen Rust
inputs, seven source/document files, twelve test executables and the detailed
English commit. Parser/compiler lowering, returned input/source/code ownership,
indirect eval, frame restoration and later builtin dispatch remain unchanged.
These checks do not renew earlier broad acceptance inventories. The ordinary-
library entry exposes the functional fixtures without claiming fresh coverage.

Full parser/compiler/coercion producer preallocation, ShadowRealm.evaluate
account handoff, ordinary host-precompiled shared script ownership, diagnostic
map/registry storage and allocator/BigInt-spare/Rc-control exclusions remain open
with integrated acceptance. M1u remains the latest accepted milestone. Validation
stays local, offline, finite and tiny; no broad, exhaustion, infinite-execution
or network/Core/MCP replay is resumed, and the rejected push is not retried.

Evidence: `target/eval-environment-storage-source-20261018.json` and prefix
`/private/tmp/blueice-eval-environment-storage-` (2026-10-18), including original
red inputs/log, initial green source/log, expanded compiler and baseline findings,
initial validation/style findings, final frozen inputs, fixed controls and the
detailed English commit message.

### M1y — Asynchronous module dependency ownership (implementation checkpoint, 2026-10-16)

The following acceptance/results retain their original historical scope. The
2026-10-17 module-dependency batch-storage follow-up above strengthens native
preparation and retained allocation ownership with a separate source receipt.

Acceptance: account for both dependency maps' table capacity, keys, dependency
set slots/names and reverse vector slots/names. Register one parent's unique
new edges as a batch before either direction or its suspended state is
published. A refused batch preserves prior edges and capacities; valid existing
slots charge only newly owned data. Completion releases empty storage; rejection
removes obsolete reverse edges from siblings that remain live. Keep source/BFS
order and normal import/defer/await behavior.

Rust-held traversal vectors/queues, collected pending dependencies, remaining
evaluation-list storage and deferred frontiers remain charged while dependency/
parent JavaScript executes and release on all exits. The deferred
frontier belongs to its graph call/result rather than a lingering realm cache.
Tests use tiny graphs, deterministic native observations and scalar shared-owner
budgets; no physical resource-exhaustion probe is needed. Active execution
control containers, full module-cache/service buffers and final integrated
acceptance remain separate open work.

- [x] Reproduce the small public/native-boundary accounting, publication,
      cleanup and callback-ownership failures.
- [x] Account for all retained dependency table/set/vector/name storage.
- [x] Admit complete edge batches before publication, moving old owners without
      cloning and preserving duplicate/source ordering.
- [x] Charge traversal/frontier owners across callbacks and drop them on every
      path; remove the lingering deferred-frontier result cache.
- [x] Remove obsolete sibling links and shrink released dependency storage.
- [x] Pass bounded regression/control checks, formatting and offline workspace
      Clippy; record the source/artifact audit with the checkpoint commit.
- [ ] Complete integrated/full-source acceptance under the owner's security
      constraints. M1u remains the latest accepted milestone.

Eleven bounded groups fail on the parent and pass after the first implementation.
They cover real suspended module records, scalar shared-budget refusal,
forward/reverse table and buffer growth, exact spare-slot storage, stale sibling
links, completion release and native observations inside parent/dependency
callbacks. Red log:
`/private/tmp/blueice-module-dependency-ownership-red-20261016.log`.
Initial green log:
`/private/tmp/blueice-module-dependency-ownership-unit-green-20261016.log`.
The expanded sixteen groups also cover duplicate batches, source/BFS order,
parent errors with ready siblings, deterministic resource-abort lease release
and pending-name/evaluation-list storage across a later dependency callback.
The collector group independently failed before its scoped charge was added:
`/private/tmp/blueice-module-dependency-collector-red-20261016.log`.
Final group log:
`/private/tmp/blueice-module-dependency-ownership-final-unit-20261016.log`.

Checkpoint validation: **193 passed, zero failed, zero ignored**. The sixteen
new groups, twelve import-waiter groups and 75 existing unit controls pass.
Eight public selections pass 90 cases: `async_from_sync_delegation` (two),
`async_generator_ticks` (fourteen), `async_yield_star_delegate_close` (seven),
`dynamic_import` (ten), `host_async_tasks` (18), `import_defer` (seventeen),
`promise_capabilities` (ten) and `promise_gc_lifecycle` (twelve). Six other
host-async pressure/limit cases remain filtered under the owner's security
constraint. Tests use finite small graphs and scalar budget observations;
no corresponding large buffer is allocated. Formatting, `git diff --check`
and offline workspace/all-target Clippy with warnings denied pass.

Control logs use `/private/tmp/blueice-module-dependency-ownership-` with
`{waiter,promise,slot,browser,handoff,call,timer,await-refusal,continuation,cleanup,source,public}-controls-20261016.log`.
Formatting/Clippy logs use that prefix with `{fmt,clippy}-20261016.log`.
Receipt: `target/module-dependency-ownership-source-20261016.json`.
Active execution control containers, full module caches/service buffers,
allocator/transient staging accounting and integrated acceptance remain open.
No fresh full Test262, real Core/MCP replay or coverage result is claimed.

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
- [x] Finish Promise-record, host-wait and continuation-table admission before
      object/capability/frame publication. Await must admit its reaction or job
      together with frame/table storage and preserve its serial on refusal.
      Host calls must admit both tables and native argument storage together,
      refund argument reservations on every early exit and preserve starts.
- [x] Finish ordinary/deferred import waiter admission before publishing keys,
      vectors, dependency sets or target references. Refuse shared request
      count/byte growth without changing prior waiters; admit spare slots
      without recharging canonical keys. Release unused wait storage after
      fulfillment/rejection while preserving waiting targets through GC.
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

Browser queue checkpoint (`7c615cf0d`) validation: **161 passed, zero failed, zero ignored**.
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

**Promise-table follow-up — bounded tests passed.** Acceptance: refuse
Promise-table growth before allocating a new Promise; jointly admit host wait
and Promise-table growth, request count and native argument copies before
creating a capability or starting the service. Admit async/module frame
storage, table growth and the matching reaction/job count, payload and slots
as one publication. A refusal must preserve the continuation table, serial
and resumption queues; it must return SchedulingLimit rather than success.
Preserve available-slot admission, normal Promise/await execution, GC ownership
and early-error cleanup.

Empty replacement tables determine actual reported growth before any live
entries move. Existing owners transfer without cloning after admission. A
scoped argument lease remains charged during Promise allocation and refunds
all exits. The initial fifteen groups use eight or fewer entries per tested
table/queue and scalar shared-budget/count models: twelve fail and three
available-slot controls pass on the parent; all fifteen pass after
implementation. The final
selection also checks terminal script refusal and root-refusal lease/capacity
reconciliation. Red log:
`/private/tmp/blueice-promise-table-capacity-unit-red-20261016.log`.
Broader native control/service buffers, allocator overhead and transient
staging storage remain outside this logical owned-capacity checkpoint.

Promise-table checkpoint (`a70cd7795`) validation: **148 passed, zero failed, zero ignored**.
Seventeen new groups pass, along with 58 existing unit controls: the 56
capacity/handoff/call-entry/timer/cleanup/source groups from the preceding
checkpoint and two actual await serial/invalid-owner restoration controls.
Seven public selections pass `async_from_sync_delegation` (two),
`async_generator_ticks` (fourteen), `async_yield_star_delegate_close` (seven),
`dynamic_import` (ten), `host_async_tasks` (18), `promise_capabilities` (ten)
and `promise_gc_lifecycle` (twelve). Six other host-async pressure/limit cases
remain filtered. The new models allocate no represented large shared payload,
and the small root/serial refusal controls use deterministic injected limits.
Formatting, `git diff --check` and offline workspace/all-target Clippy with
warnings denied pass.

Logs: `/private/tmp/blueice-promise-table-capacity-final-unit-20261016.log`,
`/private/tmp/blueice-promise-table-capacity-slot-controls-20261016.log`,
`/private/tmp/blueice-promise-table-capacity-browser-controls-20261016.log`,
`/private/tmp/blueice-promise-table-capacity-handoff-controls-20261016.log`,
`/private/tmp/blueice-promise-table-capacity-call-controls-20261016.log`,
`/private/tmp/blueice-promise-table-capacity-timer-controls-20261016.log`,
`/private/tmp/blueice-promise-table-capacity-await-refusal-controls-20261016.log`,
`/private/tmp/blueice-promise-table-capacity-continuation-controls-20261016.log`,
`/private/tmp/blueice-promise-table-capacity-cleanup-controls-20261016.log`,
`/private/tmp/blueice-promise-table-capacity-source-controls-20261016.log` and
`/private/tmp/blueice-promise-table-capacity-public-controls-20261016.log`.
Clippy: `/private/tmp/blueice-promise-table-capacity-clippy-20261016.log`.
Receipt: `target/promise-table-capacity-source-20261016.json`.
No fresh full Test262, real Core/MCP replay or coverage result is claimed.

**Import-waiter follow-up — bounded validation passed.**
Acceptance: ordinary import waiters
admit the shared count, any map/vector growth and newly retained canonical key
before publication; an existing key/slot is not charged again. Deferred
waiters admit their dependency-set/name payload and vector growth together.
Refusal preserves prior tables, vectors, counts and target references. Waiting
targets survive actual collection, then become collectible when settled and
unreferenced. Fulfillment and rejection release empty wait storage; partial
dependency completion returns obsolete names and excess set capacity.
Namespace construction failures must retain the original waiters for cleanup.

Ten initial groups use small real module graphs and import jobs, plus scalar
models of another document owner. Eight reproduce late publication or retained
empty storage; two available-slot controls pass on the parent. Red log:
`/private/tmp/blueice-import-waiter-capacity-unit-red-20261016.log`.
The new helpers plan storage before publication and reconcile the ledger after
settlement. All twelve expanded groups pass: ordinary/deferred shared-count
refusal; first-key, key-table, waiter-vector and deferred growth; both spare-slot
controls; fulfillment/rejection storage and GC release; partial dependency
shrink; and deterministic namespace-root refusal. The latter confirms that
the original pending import references survive the error. The defer loader
refreshes its separate result-root lease; cumulative root-registration counts
are not a measure of waiting ownership, so refusal compares retained targets,
names, namespaces and capacities directly.
Expanded log:
`/private/tmp/blueice-import-waiter-capacity-expanded-green-20261016.log`.
Import-waiter checkpoint (`1be3b59e5`) bounded validation:
**177 passed, zero failed, zero ignored**.
The twelve new groups and 75 existing unit controls pass: seventeen Promise
table groups, eight initial slot groups, twenty browser groups, twelve handoff
groups, four call-entry controls, seven timer-count groups, two await-restoration
controls, two cleanup boundaries and three task-source boundaries. Eight public
selections pass `async_from_sync_delegation` (two), `async_generator_ticks`
(fourteen), `async_yield_star_delegate_close` (seven), `dynamic_import` (ten),
`host_async_tasks` (18), `import_defer` (seventeen), `promise_capabilities` (ten)
and `promise_gc_lifecycle` (twelve). Six other host-async pressure/limit cases
remain filtered. Formatting, `git diff --check` and offline workspace/all-target
Clippy with warnings denied pass. The new tests use small queued jobs and
deterministic scalar/root-refusal controls, with no represented large buffer
allocation or resource-exhaustion probe.

Control logs use `/private/tmp/blueice-import-waiter-capacity-` with
`{promise,slot,browser,handoff,call,timer,await-refusal,continuation,cleanup,source,public}-controls-20261016.log`.
Clippy: `/private/tmp/blueice-import-waiter-capacity-clippy-20261016.log`.
Receipt: `target/import-waiter-capacity-source-20261016.json`.
Broader native control/service buffers, allocator/transient staging accounting
and final integrated acceptance remain open. No fresh full Test262, real
Core/MCP replay or coverage result is claimed.

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

Timer-count checkpoint (`733c7ef24`) validation: **94 passed, zero failed, zero ignored**.
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
