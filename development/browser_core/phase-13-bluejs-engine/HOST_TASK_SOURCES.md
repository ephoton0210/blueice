# Document task sources

M1j replaces M1i's arbitration between storage groups with logical task
sources. The document owns the source-selection cursor, enqueue serials,
monotonic clock and scheduling account. A same-document ShadowRealm shares
these identities; its separate VM and callback table do not create another
source. An adopted foreign document never participates.

## Current associations

| Logical source | Work | Order within the source |
|---|---|---|
| JavaScript engine | FinalizationRegistry cleanup | Document enqueue serial |
| File reading | FileReader read stages | Document enqueue serial; every continuation receives a new serial |
| Timer | setTimeout, setInterval, AbortSignal.timeout | Expired deadline, then document enqueue serial |
| DOM manipulation | DataTransferItem.getAsString callbacks | Document enqueue serial |
| Networking | M1k owner-installed asynchronous service completions | Completion enqueue serial |
| User interaction | M1k owner-installed UI service completions | Completion enqueue serial; real Core input association remains open |

The [DOM timeout algorithm](https://dom.spec.whatwg.org/#dom-abortsignal-timeout)
assigns abort timeouts to the timer source. The
[File API](https://www.w3.org/TR/FileAPI/#task-source) specifies its file-reading
source. HTML specifies the
[JavaScript engine cleanup source](https://html.spec.whatwg.org/multipage/webappapis.html#hostenqueuefinalizationregistrycleanupjob).
The DOM-manipulation association for drag string callbacks is a BlueIce policy:
the [getAsString algorithm](https://html.spec.whatwg.org/multipage/dnd.html#dom-datatransferitem-getasstring)
currently queues a task without naming its source.

The selector rotates among runnable sources in the table's order, skipping
empty or paused owners. Within a source it selects the oldest eligible work
across the root and every same-document child. Serial order resolves equal
timer deadlines independently of callback representation and realm traversal.
This is BlueIce's scheduling policy, within HTML's
[task-queue selection freedom](https://html.spec.whatwg.org/multipage/webappapis.html#event-loop-processing-model).
It does not promise one global FIFO across unrelated sources.

Finalization readiness uses a separately admitted native index. Collection
counts newly ready surviving registries and prepares complete backing capacity
before clearing weak targets, releasing pins, sweeping or changing counters.
An index refusal recycles the mark result and preserves those heap states.
Existing keys update in place even when the table is full. Reported-capacity
changes after deletion do not refund a retained allocation; final removal and
registry reclamation release its native owner without new admission.

Page cleanup borrows ready identities into an admitted sorted registry snapshot
before publishing tasks. Its native lease follows the complete Rust-held
snapshot lifetime. Host-neutral cleanup instead compacts cells in place and
moves cleared holdings directly into the traced Promise-job queue, preserving
per-registry cell order without bulk jobs/live-cell scratch vectors. Registration
protects its at-most-four object references in a fixed stack array.

Cell-vector slots belong to the registry's managed heap charge at actual retained
capacity; live primitive payload is charged separately once. Growth admits a
complete detached candidate while the original allocation remains charged,
checks its reported capacity, then moves cells and publishes the new entry.
Refusal preserves existing cells and backing. Spare slots require only the new
payload allowance. Unregister and cleanup refund removed payload immediately,
retain the capacity charge for a nonempty vector and release all backing when
the last cell leaves. Collection and heap destruction refund the same owner.
The existing managed document account includes capacity across child heaps and
Rust-held heaps, including storage created before joining that account; cleanup
can refund bytes after terminal document failure.

Cleanup callback entry first borrows the ready cell without consuming it.
Primitive callback/holdings ownership, a one-argument vector and required async
root capacity are admitted while the queued registry root and cell retain their
original owners. Starting fuel is validated before allocation, and the first
host instruction is charged before committing the task/cell. Preparation refusal
keeps queued work, roots, active context and ambient fuel; partial leases refund
through failure-aware ownership reconciliation.

A committed call receives admitted arguments and roots exactly once. Dispatch
adopts staged payload/slot ownership before its first fallible step. An attempted
call consumes its cell even if dispatch or the callback fails; unattempted cells
remain ready. Every exit restores the previous root prefix and active registry,
releases the committed task root and reconciles counters. Empty/cancelled tasks
need no callback argument storage or host instruction. Existing exception
reporting, callback/microtask ordering and post-task requeue behavior continue.

Existing ready-index storage joins root-table, collector and foreign-pin native
binding as one transaction. Refusal installs no partial owners, preserves live
registrations/readiness and drops only unadmitted collector scratch. Rust-held
heaps retain their account; unregister and cleanup release an empty index after
terminal failure. Wider native/integrated acceptance remains open.

## Clock, turns and ownership

Page abort timeouts use the same host-supplied monotonic milliseconds as
callback timers. Sampling an injected host clock publishes its maximum to the
document account; child registrations and wake queries observe that clock.
Explicit public runtime time remains available for deterministic tests.
Standalone language VMs retain their existing Instant-based abort timeout
policy and do not acquire a browser clock or a timer global automatically.

At entry, the document snapshots due callback timers and runnable abort
timeouts. Each runs at most once in that turn. Timeouts registered by a
callback or its microtasks wait for another entry. An interval receives a new
deadline and enqueue serial after its callback and checkpoint, unless cleared.
A late interval does not replay missed periods. Canceling a snapshotted timer
does not spend another delivery.

Callback entry validates document state and starting fuel while the original
timer remains registered. A one-shot transfers its existing arguments and root
vector. An interval first admits its independent primitive copy and argument
slots, checks reported capacity and prepares temporary execution-root capacity.
Its callback and object arguments use a fixed reference array and remain traced
through self-cancellation and the Promise checkpoint without additional
persistent root registrations. Partial preparation returns staged ownership;
refusal preserves the registration, deadline/serial, run counters and ambient
fuel/nesting. The first host instruction is charged before committing an attempt.

Dispatch takes over argument payload and vector capacity exactly once. Task
accounting retains only its independent root protection through the checkpoint;
consumed argument slots do not remain charged there. All exits restore the
original execution-root prefix and nesting and reconcile released ownership.
An attempted one-shot is consumed even if dispatch fails; an interval retains
its registration and is rearmed only after a successful task/checkpoint.

Host-turn snapshots own admitted native storage until their Rust-held lifetime
ends. Due callback timers use one sorted deadline/ID vector, preserving its lease
through standalone delivery as well as document turns. Runnable abort serials
use an admitted flat vector; per-realm timer snapshots use an admitted sorted
array with no allocation during lookup. Empty snapshots reserve no backing slots.

Document realm discovery admits its weak-handle queue, seen IDs and returned
weak snapshot before filling each container. Requested and reported capacity
are checked while old/new replacements coexist. Temporary discovery storage
refunds on every exit, while the returned weak slots stay charged throughout
child checkpoints and callbacks. They retain no strong VM owner, so traversal
does not keep discarded realms alive. Wake and task-turn callers propagate
native refusal through the existing noncatchable scheduling error.

The public turn still admits at most 64 total deliveries. The production child
selects one, completes its checkpoint and returns to Core. Rendering stays in
its separate entry. Ready tasks request an immediate wake; future deadlines
request their remaining relative delay. Dead weak abort entries can be reaped
without waiting for their former deadline. The private transport retains its
existing bounded wake chunks and exact-document checks.

Callbacks, arguments, read stages and cleanup registries stay rooted in their
own VM. Enqueue metadata participates in the existing owned-slot byte ledger.
Page-owned callback timers share a 1,024-registration document cap across the
root and same-document ShadowRealms. `HostSchedulingUsage.pending_timers`
reports that aggregate. Registration admits one count before publishing its
ID, roots or table entry. A firing one-shot leaves the pending count; an
interval keeps its registration until cleared, including during its callback.
Cancellation and realm/document destruction return the owning contribution.
An unreachable realm with pending work remains alive until that work finishes.

Callback-timer and abort-timeout clock sampling precedes final registration
admission and serial assignment.
Work enqueued by an embedding clock callback is therefore included in that
admission and receives the earlier enqueue serial, before the outer timer.

After callback validation, delay coercion and clock sampling, timer registration
checks the shared count and validates all input/root identities. A detached root
batch admits its complete hash replacement, and a separate native lease admits
argument/root vectors plus complete timer-table replacement while old storage
remains charged. Each allocation's reported capacity is checked before the next
allocation; primitive copies are made under that lease. Timer-table spare slots
avoid a table-growth fee. Root-vector slots cover only the callback and actual
object arguments, including independent registrations for duplicate objects.

Task serial acquisition is the final fallible step. Root IDs/counters, moved
table entries and the new timer then commit without another allocation. Native
ownership transfers directly into the live registry; old replacement storage is
refunded once freed. Refusal preserves old entries/capacities, timer/root IDs,
serials and registration counters, and all detached owners refund on drop even
after terminal failure. Rust-held candidates retain independent document leases.
The borrowed root-batch protocol permits no collection or root mutation between
preparation and commit.

Refusing a registration above the document limit latches terminal scheduling
failure; navigation creates a fresh account. Host-neutral VMs retain their
1,024-per-realm catchable `RangeError` policy. Abort-timeout browser entries
remain in the separate 64-task browser/cleanup cap; microtask/reaction caps
are unchanged. Replacement and close drop the owning queues and reset source
selection with the realm. These Rust count snapshots do not change the
private transport's existing byte totals or layout.

Promise-job deques and reaction vectors admit complete detached replacements
while their original capacity stays charged. Already-formed incoming work first
acquires its payload lease; candidate storage reserves at least four slots,
doubling a full capacity, and checks actual reported capacity before publication.
Spare slots need only new payload/count allowance. Moving a wrapped deque keeps
FIFO order. Publication transfers ownership once into the live queue, and
discarded candidates refund through terminal failure. Rust-held candidates keep
independent document leases without retaining a VM or JavaScript object roots.

Animation-frame vectors, page cleanup deques and pending rejection-notification
vectors now lease complete detached replacements while old allocations remain
charged. Shared counts and root-ID space are checked before replacement
preparation. Growth requests at least four slots, doubling a full capacity, and
checks actual reported capacity before preparing the entry's root-table storage.
Cleanup acquires its task serial as the final fallible step. Root and queue then
commit without allocation, collection or JavaScript before publishing the new
handle/entry. Available queue/root slots add no new native storage charge.

Refusal preserves existing capacities, roots, handles and serials, and detached
owners refund after terminal failure. Infallible rejection tracking still
latches document failure when root or native admission fails, after refunding
its preparations. Cleanup snapshots keep their independent lease throughout
registration and continue to suppress already-queued/active registries.
Cancellation, notification handoff and callback/cleanup completion retain the
existing root and GC lifecycle. Timer argument storage excludes the callback
and delay and uses the complete preparation protocol above.

Animation-frame render snapshots now own admitted native handle storage through
callbacks and checkpoints. Requested and actual capacity are checked while the
live registry remains installed; snapshot handles retain no VM or callback
object roots. Cancellation checks the live registry, while registrations made
after the snapshot wait for another render. Empty snapshots reserve no backing.

Pending-frame entry validates starting fuel before snapshot/argument admission.
Its timestamp argument vector has an independent document lease, preserving
unattempted registrations, roots, ambient fuel and time on preparation refusal.
The first host instruction commits an attempt; ordinary dispatch then adopts
argument capacity once. The existing callback root protects invocation and error
reporting without new persistent roots, and releases before the established
Promise checkpoint. Dispatch/report refusal consumes only the attempted callback
and releases its root; later entries stay registered. Empty-registry and staged
storage refund on every exit, even terminal failure or Rust-held lifetimes after
VM destruction. Paused/canceled work needs no starting instruction. Valid empty
renders still advance one monotonic clock and checkpoint jobs; invalid timestamp
validation precedes resource preparation.

All browser-task producers now admit a complete detached deque replacement while
old backing stays charged. Incoming text/completion/rejection-vector payload
keeps an independent document lease through preparation. Actual queue/input
capacity is checked before subsequent allocation; spare queue slots require
only the newly owned payload. Managed-only preparation may collect while the
installed task queue remains unchanged. Root storage is prepared afterward,
and enqueue serial acquisition is the final fallible step before publication.

Drag roots and handled-rejection event vectors publish with their admitted
tasks. Completion flags change only when the prepared task can commit.
Unhandled rejection delivery moves its existing vector/reaction ownership once.
Handling a notified rejection commits the language handled status first; its
notification bit clears only after event preparation succeeds. Infallible
resource refusal latches the document abort after detached owners refund.

Timeout allocation precedes serial publication. A later refusal leaves its
unpublished signal unrooted and collectible. Initial FileReader reads lease
their complete cloned state, including listener slots/names and primitive
payload, while the old managed owner remains installed. A staged boxed update
validates incoming references and admits managed growth with borrowed reference
visitors protecting the reader and new edges across GC. Conservative remembered
edges are prepared before publication, avoiding callback/reference scratch
vectors and allocation during commitment. Root/serial/native/managed refusal
drops that update without changing the reader's fields or task queue.

The root, staged reader state and prepared task then commit without another
allocation or collection. Native clone ownership refunds when the managed state
adopts its buffers. Epoch validation retains its language-error precedence over
resource preparation. A callback can consume queue slots/count before its read
stage returns, so the successor readmits complete deque storage afterward.
Refusal retains callback effects, publishes no successor/serial and releases
the read root. Cancellation removes matching entries in place, preserving
other queued work. Empty/old backing refunds when actually released, including
terminal failure and Rust-held preparation lifetimes beyond VM destruction.
Callback-entry scratch, retained-map allocation ownership, other heap/host
buffers and broader native/integrated acceptance remain open work.

Browser-task entry now classifies queued state before removal. Active drag,
FileReader, timeout, host-completion and rejection work validates starting fuel;
drag admits a one-slot callback vector while the queued text/root stay installed.
The first host instruction commits an active attempt. Its string then moves
directly into ordinary dispatch, transferring payload and vector capacity once;
neither owner remains charged through the following checkpoint. Preparation
refusal preserves queued roots, wait/notification flags and ambient fuel.

Future deadlines stay queued with their old position and serial, without clock
callback invocation or instruction reset. Dead/aborted timeouts, stale reads/
completions and already-handled rejection notifications release their roots and
buffers without an event/host instruction. Cleanup retains its post-task
checkpoint while restoring ambient fuel. All entry outcomes restore caller
active-byte ownership and reconcile refunds; empty deque storage also releases
after failure. Event/settlement/batch internals retain their committed-attempt
cleanup behavior, and finer event/batch staging remains open alongside retained
maps, other heap/host buffers, shared programs and integrated acceptance.

Rejection-task entry additionally prepares a complete native notification
snapshot while the original task and all Promise roots remain queued. It admits
the snapshot vector, each eligible event's complete attribute backing, both
reason copies, event-name storage and copied source filename before publication.
Requested and reported capacities are checked under an independent document
lease. Suppressed entries keep their position without copying payload; delivery
rechecks handled state after earlier callbacks and their checkpoints.
Each timestamp refreshes at actual delivery, after earlier callbacks, without
growing the prepared attribute backing.

Each entry owns separate event/reason/source leases split from that admitted
batch. Managed event allocation retains the native event lease until the heap
adopts or drops its state. Console transfer retains the filename lease through
clock sampling and releases it after insertion, before console admission. Entry
payloads drop after each attempted/skipped notification; vector backing drops
before the final event-turn checkpoint. Preparation refusal consumes no task,
root, serial, notification bit or ambient fuel. Managed event allocation and
dispatch are committed attempts; their errors still release the entire batch's
roots and temporary ownership. Other event producers and dispatch scratch,
retained maps and integrated acceptance remain open.

Async/module await admits counts and settled value/origin ownership before
copying its status, then leases unowned frame payload, complete queue growth and
complete continuation-map replacement together. Refusal leaves live queue/map
capacities, records and frame serials unchanged. Publication moves admitted
storage and installs both the frame and its reaction/job without another
fallible queue admission. Error exits reconcile only after the rejected frame
has dropped its execution leases, so sibling realms see those refunds too.
Preparation and commit run no JavaScript or collection.

Promise record creation leases a complete detached table candidate before
managed object/trace admission and publishes it only after object creation
succeeds. Collection may remove dead records during that operation; sparse-table
shrinkage is deferred so an admitted spare slot survives. Commitment reuses any
available slot, discarding a now-unnecessary candidate, or moves surviving
records into the admitted replacement. No JavaScript runs during this handoff.
Failure refunds detached storage and restores the ordinary reclamation policy.
An existing empty cache survives a trace refusal without collection; a real
collection may reclaim it without another allocation. Returned objects retain
their requested prototype and normal Promise/GC ownership.

Live Promise records keep a separate allocation ledger owned by the VM. Removing
records changes insertion headroom without refunding retained hash backing;
prepared replacement or explicit empty-table release updates the ledger.
GC reconciles dead payload before preparing a complete smaller replacement
under an independent native lease. Sparsity uses retained allocation so a
tombstone-reduced capacity cannot suppress reclamation. It checks requested/
reported capacity, moves survivors, reconciles the new live allocation, then
refunds the candidate lease.
Insufficient headroom skips optional compaction without poisoning collection;
later GC may retry. Empty backing releases without a candidate, and managed
Promise preparation still preserves admitted spare slots. Mandatory admission
continues to latch an abort; reclamation never clears a prior abort.
Other control maps still need retained-allocation ownership.

Host waits and async/module continuations now keep allocation ledgers too.
Prepared growth commits through those owners; partial removal retains backing
until admitted compaction, and final wait completion/await resumption releases
empty tables. GC reconciles pruned payload, then optionally reclaims all four
scheduling maps with a candidate lease held through each ownership update.

Managed Promise/host-wait preparation defers replacement of every staged map,
including a sparse wait table whose candidate expects its original capacity.
After actual collection, the completed registration boundary reclaims deferred
storage only after candidates commit or drop. A refusal without collection keeps
existing caches. Optional compaction never clears terminal failure; empty-table
cleanup needs no admission. Cancellation retains committed token/root semantics
and still runs once for each unfinished wait while live backing is charged.

Import registration now prepares complete eager map/vector replacements before
publication. Existing keys discard temporary lookup strings before vector
preparation; new keys retain an independent name lease. Deferred dependencies
lease both input-vector/name storage and complete set backing before moving or
deduplicating names, then retain the prepared set lease through outer-vector
preparation and publication. Admission refusal leaves live storage unchanged.

Import maps and dependency sets retain their allocation ledger across deletion.
Optional map/set/vector compaction admits a complete coexisting candidate, moves
survivors, reconciles live ownership, then refunds staging. Missing headroom
keeps backing charged without poisoning collection; empty cleanup needs no
admission. GC uses the guarded scheduling reclamation boundary for this storage.
Import delivery prepares a complete native batch before consuming any waiter or
advancing dependency membership. Entry/reference/root vectors and copied
rejection values/source filenames have independent document leases checked
against actual capacity. The complete root batch registers atomically; refusal
leaves queues, dependency markers and root IDs unchanged. Eager namespace roots
remain installed through native preparation and dispatch.

Partial dependency completion then reclaims obsolete names. Each ready entry
stays queued until its own attempt begins. Prepared roots protect every target
and namespace/reason across earlier getters and collection; an error consumes
only the current attempt and leaves the unattempted suffix queued. Fulfillment
propagates runtime errors, including the caller's instruction limit. Rejection
transfers its leased value/origin to the Promise record/override before refunding
staging. Callable namespace exports hand roots to ordinary thenable jobs. Every
result releases batch roots and scratch and reconciles usage; ambient fuel is
preserved. Other Promise reaction/input/origin producers, provenance/other maps
and wider heap/host buffers remain open.

Common Promise settlement prepares its entire reaction fan-out while the
original pending record and reaction edges remain installed. It admits all new
job counts together, leases incoming value ownership, and prepares complete
snapshot backing plus copied selected handlers/capabilities, values and origins.
Source preflight borrows the winning override/frame filename before its first
clone; requested/reported storage is checked before each further allocation.
Native preparation runs in linear time without JavaScript or managed collection.

The job queue reuses enough spare slots or prepares a complete coexisting
replacement for the whole batch, preserving repeated single-entry geometric
growth even for non-power-of-two existing capacities. Publication adopts the
record value/origin,
drops obsolete reactions, appends prepared jobs after existing work, frees
snapshot metadata and refunds staging without further admission. Refusal retains
all pending status/reactions/jobs and latches the document abort rather than
panicking native resolvers. Already-settled records remain no-ops and invalid
inputs retain TypeError. Rejection-notification registration follows successful
publication as its own committed attempt. Native preparation changes neither
ambient fuel nor root IDs. Already-settled reaction/origin producers, provenance
maps and other native-buffer ownership remain open.

Then registration validates the source and admits its new reaction/job count
before cloning any retained status. Pending work leases both incoming handlers
and capability payload; settled work drops the unused handler and leases its
selected fields plus the record's value/source copies. The existing prepared
work boundary checks actual cloned storage and prepares complete vector/deque
growth with no JavaScript or managed collection.

Only queue publication commits handled state. Preparation refusal preserves
notification bits, pending rejection entries/roots and all original queue storage,
latching the document abort without panicking valid internal callers. Fixture
direct reaction appends retain this ordering; production Then/await paths publish
prepared work. Successful handling releases
a still-pending notification root; an already-notified source uses the existing
follow-up task producer after its job commits. Follow-up refusal retains that
committed job/handled state and its notification bit. Invalid receiver errors and
ambient fuel are preserved. Wider input/origin/provenance/native-buffer ownership
remains open.

Module/async-function frame awaits and generator delegate/return/yield awaits now
share a scalar continuation descriptor and prepared status/work boundary. Counts
and copied value/source ownership are admitted before cloning; conversion moves
those copies into the prepared work. Frame storage and complete queue growth
coexist under independent leases; module/async serial and map preparation precede
joint frame/work publication. Generator PromiseResolve and any language callbacks
finish before native preparation begins.

After publication, a common completion step marks the source handled and checks
notification cleanup/follow-up. Refusal during preparation preserves the source
notification/root and uncommitted serial/map/queue state; late notification failure
retains committed owners while surfacing the abort. Native/heap/fuel errors pass
through generator scheduling without capturing language throw metadata or
replacing a refused original return request with a throw. Language exceptions
keep their existing catch/finally behavior. Preparation runs no JavaScript or
managed collection and does not roll back earlier request/control transitions
or user callbacks. Other input/origin producers and wider native-buffer ownership
remain open.
The superseded direct reaction append helpers are compiled only for test/coverage
fixtures; production await/Then registration uses the prepared work boundary.

After heap collection removes dead Promise records, continuation liveness is
marked in the already-admitted frame entries. Surviving Promise reactions and
queued ModuleAwait/AsyncAwait jobs mark their matching module/function frame;
the two serial namespaces remain independent. Unknown serials create no entries,
and duplicate owners mark the same entry. Retention consumes each mark, so a
removed last reaction/job releases its frame on the next pruning pass. New and
re-suspended frames initialize their marks empty, including generator and debugger
paths. These bits are native record storage accounted through the existing map
allocation ledger, rather than separate native buffers.

This replaces two temporary HashSets allocated at every post-heap pruning pass.
The owner walk and map retention remain linear in their traversed storage; no
per-collection set, additional native admission or scan of all owners for each
frame is required. The marks introduce no heap roots and leave the existing
Promise/continuation ownership edges unchanged. Queued work continues to keep its
frame/local values live; orphaned frames and their captured values can collect.
Terminal cleanup can discard unowned frames without new headroom while the
staging guard retains old table backing. Optional map reclamation remains a
separate guarded/admitted producer. Disposal brand maps now share the guarded
reclamation boundary described below; other pruning/native-buffer ownership
remains open.

DisposableStack and AsyncDisposableStack brand records also use weak retained
control maps. Record deletion releases resource buffers but keeps hash backing
charged independently of reported insertion capacity. GC only prunes dead keys;
the shared guarded reclamation boundary funds optional complete replacements,
moves survivors and reconciles ownership before refund. Unavailable headroom
stays nonterminal and retryable. Empty backing releases without admission,
including after terminal failure. Both brand namespaces stay independent.

After observable prototype resolution, constructor/move preparation leases the
complete table candidate before managed instance allocation. Move also admits
temporary root-vector capacity and protects its source/resource edges. The shared
guard preserves old table capacity while managed GC prunes dead records. Commit
reuses freed slots or adopts prepared backing, then initializes a fresh record
or moves the source resource buffer and marks the old record disposed. Publication
performs no further fallible table admission and preserves the moved buffer's
existing native owner. Refusal before commitment preserves source resources and
disposed state; root length and nested guard restore on every result. Native root
cache capacity already prepared may remain as an owned cache.

All candidates commit/drop before the shared post-GC finisher performs deferred
optional reclamation. No GC runs in that finisher, so the returned fresh instance
can leave the helper unrooted for its immediate caller to adopt. Constructor
prototype getters and other earlier language effects stay committed. Wider
resource/argument producers, other maps and native buffers remain open.

Finally-thunk provenance uses a weak retained-allocation map too. Pruning returns
dead filename buffers without refunding retained hash backing. Guarded scheduling
reclamation skips staged maps, funds optional smaller replacements under native
leases and releases empty tables without admission, including terminal cleanup.
The metadata keys never root their state objects.

Finally capture pre-admits its current-source clone before adopting it as the
callback override. After onFinally and PromiseResolve, registration leases the
copied source and complete table candidate before managed thunk state/value-field
creation. The shared guard preserves capacity through GC; commit reuses a freed
slot or adopts prepared backing, then transfers the state/origin owner without
another table admission. Candidates commit/drop before deferred GC reclamation;
every result restores the previous guard and temporary root stack.

Thrower reads pre-admit the stored filename before cloning and transfer it into
pending provenance before refunding staging. Refusal publishes no origin/epoch.
Unknown-source overrides retain precedence over active-frame source metadata.
Earlier callbacks and PromiseResolve effects stay committed. Wider argument/value
scratch, other origin producers/maps and native-buffer ownership remain open.

Host-wait registration first admits its shared pending count and preflights
root-ID space. Converted primitive inputs and actual argument-vector slots have
an independent document lease. Unsupported input retains a deferred language
error instead of turning the ordinary rejected Promise into a synchronous throw.
The wait and Promise tables each admit complete detached replacements while
their original storage remains installed. Intrinsic setup and managed object
allocation may collect; the record-table handoff supports the resulting pruning.

After managed allocation, registration admits the object's root-table storage.
Token serial acquisition is the final fallible preparation step. Root, Promise
record and wait then publish without further allocation or collection, before
the host callback observes its token and primitive arguments. Receiver validation
and argument errors retain their previous rejection order. Converted argument
ownership releases after callback dispatch, and normal rejection, queued
completion and VM cancellation release the committed wait root.

Preparation refusal publishes no new wait, record, root or callback and consumes
no token serial. Detached buffers refund on every exit, including terminal
failure and Rust-held lifetimes beyond VM destruction. Existing native owners
remain installed apart from ordinary GC; an object created before a later root/
serial refusal stays unrooted and collectible. Other control-table preallocation,
retained hash-allocation ownership, producer-side clones in other Promise
algorithms, allocator overhead and broader native control/service storage remain
open.

Ordinary and deferred dynamic-import waiters consume the same document
reaction/request count. Ordinary registration checks table/vector growth and a
new canonical key together; an existing key and spare slot add no payload
charge. Deferred registration moves dependency names into a prepared set and
admits its actual slot/name storage with the waiter-vector growth. Admission
precedes publishing the new target reference or dependency set. Import loading
still owns its independent namespace-result root while preparing that request.

Namespace construction succeeds before completed waiters are removed. A
construction refusal retains their ownership. Fulfillment and rejection
release empty or mostly unused waiting-table/vector storage and reconcile the
account before publishing further Promise work. Partial deferred completion
drops obsolete dependency names and shrinks mostly empty sets. Pending targets
remain GC roots through the existing module-work edges, then become collectible
when delivery has finished and no other owner retains them.

Static asynchronous module evaluation owns a forward dependency set for each
waiting parent and an ordered reverse-parent vector for each dependency.
Evaluation and ordinary/deferred dynamic-import jobs prepare request targets in
one independently leased normalization/type-key buffer. Relative path processing
uses that buffer in place, covering the largest intermediate path and reported
capacity before writing; it creates neither a segment vector nor a second typed
key. Remaining linking/export-resolution helpers retain their existing policy.

Pending-name and eager/deferred evaluation/frontier lists independently own every
String capacity and vector slot. New copies and complete vector replacements
are admitted before allocation. Growth moves existing names, returns old backing
ownership and absorbs the prepared candidate without readmission. Iterators
retain their original backing and split off each yielded name's existing lease;
both iterator and current name stay owned across dependency JavaScript. Frontier
filtering drops obsolete names in place. Detached import results keep their name/
frontier leases until consumption or drop, including beyond a VM's lifetime.
Immediate dependency-batch/waiter handoff refunds the incoming lease directly
before its recipient admits retained input or discards a duplicate, with no
intervening callback or allocation and no transient duplicate native charge.

Reachability, SCC membership/error selection, deferred readiness/frontiers and
requested-module loading use independently leased analysis scratch. Visited hash
tables borrow canonical registry keys, and explicit DFS frames replace recursive
analysis calls. New hash tables and frame/candidate/pending vectors admit complete
coexisting replacements and reported capacities before allocation/adoption.
Clearing a traversal reuses backing while retaining its native charge; a query
owner can retain that scratch after VM destruction while its immutable registry
borrow remains valid. Analysis executes no JavaScript or managed collection.

SCC/error candidates remain lexically ordered, with one query workspace reused
for mutual reachability. Deferred edges stay outside evaluation-cycle reachability
and inside loading/frontier/readiness walks. Loading resolves a node's requests
in source order before visiting its last queued child, retaining missing-target
names until their error is selected. Unreachable registry entries remain outside
that loading walk. Resource refusals propagate from cycle/readiness/frontier
queries without the previous resolution-only panic assumptions.

Eager/deferred namespace identity caches, both root maps and the deferred-object
reverse map retain reported allocation backing and all owned name capacities in
the document budget. Publication prepares every copied key/value name and full
replacement table before managed allocation. Capacity preservation protects the
prepared table shapes across placeholder allocation/GC. Both temporary and cache
roots must succeed before the placeholder adopts native cache storage; staging
refunds directly before synchronous ownership reconciliation.

Linked records carry monotonic marks for each namespace kind. One outer namespace
transaction retracts all new recursive placeholders and cache roots by scanning
those marks, without snapshot sets, cloned rollback keys or a cleanup vector.
The surrounding linking transaction also retracts namespaces successfully built
by earlier imports if a later link fails before evaluation starts. Existing
identities remain retained, and retry binds newly created cells. Removed names
refund while nonempty backing remains owned; empty maps release without allocation
even after terminal failure. Optional sparse reclamation separately admits full
candidates and refunds each adopted reduction before funding the next map.

`GetExportedNames` borrows canonical registry keys and existing export-name
buffers. Native owners admit complete visited-set, explicit-frame and result-vector
replacements before allocation; star requests use independently owned qualified
name buffers. The iterative walk follows star declarations in source order,
collects explicit forms without resolving their bindings, excludes star defaults
and visits repeated modules once. It sorts distinct names in place and releases
traversal sets/frames before returning the still-owned pointer vector. Deferred
filtering retains its backing charge. Namespace construction retains that result
owner across managed allocation, GC and nested namespace construction; a
Rust-held result remains charged after its VM dies.

`ResolveExport` uses admitted query-wide module/name markers and explicit frame
vectors instead of native recursion and copied input pairs. Local binding and
nontraversing missing/default results borrow canonical registry keys without
allocation at full healthy quota; terminal refusal still takes precedence.
Traversal admits complete coexisting replacements and borrows query names while
propagating indirect results or merging star candidates in declaration order.
Cycle markers distinguish different export names within one module.

Resolved targets borrow canonical registry keys when supplied, or retain an
independent qualified `ModuleName` owner for namespace/deferred/source results
outside that registry. Equality compares result kind, module name and binding
slot, including owned names. Duplicate/discarded candidates and failed walks
refund their owners; a retained result outlives its VM without losing its charge.
Linking and namespace callers supply the document scope for resolver and request
preparation, and direct namespace/source imports move prepared targets instead
of cloning Strings. Source-phase requests remain unqualified by resource type.

After namespace export payloads enter the heap, managed ownership includes the
export vector's reported capacity and each UTF-16 name's retained capacity.
Placeholder completion updates both the namespace object's bytes and the local/
shared document counter, refunding any replaced empty backing. Minor and major
collection therefore remove the full adopted charge. The completion safepoint
borrows the namespace/input-cell roots directly, preserving unrooted incoming
cells before their edges are published. In-place UTF-16 sorting preserves valid
unique export ordering without native sort scratch. Other heap kinds retain
their current accounting scope.

Native preparation precedes namespace placeholder publication: complete output
backing and UTF-16 names retain independent native ownership while cells and
nested namespaces are resolved. Kept names move without copying; skipped names
refund payload while retaining vector backing. Consumed name headers are freed
before the ready payload crosses the heap boundary. Its owner remains captured
through trace preparation, then refunds immediately after heap adoption/error,
before VM trace completion. Native and managed handoff use the same payload-size
calculation. Late document attachment admits unbounded input; existing owners
must retain their document identity. Data drops before the owner on every exit.

Namespace construction uses an explicit admitted frame vector. Complete
coexisting replacements precede child placeholder publication; each frame keeps
its name-result and UTF-16/output owners independently of vector movement.
Export output retains a count rather than a borrowed frame slice, avoiding
self-reference. Child completion resumes the parent's alias-cell construction;
cached self/cyclic edges reuse stable placeholders. One outer transaction owns
new epochs and temporary roots, restores inherited capacity policy and rolls
back on failure. Invalid initial epochs refuse before controller allocation, and cache
hits retain their allocation-free fast path. Namespace-call depth stays constant.

Module evaluation dependency DFS uses a separately admitted frame vector. Each
frame retains an immutable code snapshot, canonical name, throw epoch, request
cursor, owned target iterator, waiting name and pending dependency names through
child JavaScript execution. Complete coexisting frame replacements precede child
activation. Cached completed/busy/error records retain their prior behavior;
async dependency registration still uses cycle roots and notification order.
Body entry, interpreter/await/debugger continuations and displaced execution
contexts keep their existing path. Request owners drop before final error/
completion retention; errors unwind active importers explicitly, preserving
throw identity and origin. Error-retention/allocation refusal also clears an
unsuspended record's active flag. JavaScript-driven deferred-property reentry
starts a separate controller through the existing context mechanism.

ImportCall prepares native input ownership and the complete temporary-root
capacity before creating its Promise capability. Popped input values move without
operand-stack copies; argument objects, the capability and temporary attributes
remain traced through coercion and option/Proxy callbacks. A validated UTF-16
specifier computes its exact UTF-8 length without allocation, then admits one
owned output buffer that remains charged across options validation. Returned
own-key snapshots retain actual vector/name capacity through descriptor/value
traps. Generic coercion and own-key producers still need their own preallocation
boundaries. Attribute types compare directly as UTF-16 strings; no extra UTF-8
attribute-value buffer is needed. Job publication returns admission errors to
the caller, while language errors retain Promise rejection and original identity.
Temporary owners/root entries release on every exit; reusable root capacity
remains in the VM's native account.

Source-phase import synchronizes document ownership before lookup, borrows its
ambient referrer and resolves the canonical request through admitted module-name
storage. Uncatchable lookup errors return directly. Shared native Promise
settlement retains its infallible/latching contract; the fallible source entry
checks the document failure after resolution or rejection before reporting
success. This covers fulfillment/rejection fan-out and thenable-job admission.
Refusal preserves pending reactions and unqueued work; a full shared microtask
count still permits settlement that creates no job. Source identity, then-getter
effects and already-published source cache entries retain their normal lifetime.
Temporary-name owners drop and VM ownership reconciles on all exits.

Module completion/rejection notifications own complete admitted queue backing
and independent module-name leases. Whole parent batches precede reverse-row
detachment; seed copies are admitted before graph mutation. Detaching a reverse
vector transfers published VM bytes into a shared charge without changing the
aggregate account. Iterator backing and yielded names remain charged after VM
destruction, with original buffers moved unchanged. Completion retains source/
BFS order; ready language failures reject ancestors before the next sibling.
Rejection rechecks capacity after waiter settlement, and roots an error before
publishing a newly rejected parent's flags/value. Removed keys and optional map
backing reclamation refund independently of detached work.

Import-slot lookup uses admitted sorted/deduplicated backing during own-cell
creation. Import aliases reserve a complete native batch before request/cell
construction, publish into already-prepared linked cell tables without growth,
then release staging. Both remain charged if Rust retains them after VM drop.
Other synthetic/source request producers and module/source caches remain open.

Source/import-meta identity and root maps retain their reported backing and all
owned key capacities in the document native account. Each new identity prepares
both name copies and complete map candidates before managed allocation, preserves
table shapes across GC, and registers its root before publication. Staging
refunds immediately before VM reconciliation; retained Rust preparations keep
their own charge after VM destruction. Cache hits borrow lookup names. Source
allocation passes its prototype directly as a protected heap argument, avoiding
an operand-stack temporary during cold construction.

Optional sparse identity-map reclamation admits full coexisting replacements
and refunds each adopted reduction before the next table. Refusal stays
nonterminal, while name buffers and root IDs move unchanged. Deleted key payload
refunds independently of retained table backing. Other synthetic request
preparation, module/source caches and broader native/heap buffers remain open.

Static synthetic requests own admitted canonical-name scratch and complete
target-vector backing before resolving keys. Resolved keys keep independent
native owners through registration; yielded keys may outlive their batch or VM.
The drain destroys remaining names before releasing their leases, while backing
remains owned until batch destruction. Canonical module order, per-module import-
then-export order, duplicate occurrences and source-phase exclusions are
preserved. All resolution completes before record publication.

Synthetic Bytecode binding/scope/export vectors and default names own admitted
native storage before value conversion. Missing-source validation still precedes
metadata admission. Prepared code keeps its document lease across Rust retention
and VM destruction. Registry insertion retains that same owner while admitting
keys, headers, source copies and complete replacement tables; publication moves
buffers, merges leases and releases replaced/surplus storage without charging
code twice. Foreign-document and bounded-to-unbounded handoff is rejected.
Value conversion/root ownership remains a separate internal boundary. Dynamic
source compilation, other module/source caches and broader native/heap buffers
remain open.

The graph's retained table/set/vector allocations and UTF-8 names share the document
native-byte budget. Registration first admits the incoming vector, original
name capacities and complete deduplication set. It then independently leases
all edge metadata, copied names and complete replacement forward/reverse maps,
dependency set and parent vectors while the old graph remains owned. Requested
and reported capacities must fit before either direction changes. Existing
names move without cloning; wholly redundant batches discard incoming buffers
without scratch allocation, including at a full document quota. Registration
into available slots still funds the full preparation that precedes publication.

Deletion returns obsolete names while hash allocation ledgers retain their
backing charge. Optional sparse reclamation admits complete coexisting
candidates; a refusal retains old storage without making the document terminal.
Each adopted reduction refunds published ownership before its candidate lease
drops and before the next candidate is funded. No full graph rescan is needed
per reclaimed row. Empty storage releases even after terminal admission failure.

Completion returns obsolete names and unused capacity. Rejection removes the
parent from every remaining dependency, including siblings that keep running.
Breadth-first/source order is preserved. Rust-owned queues, remaining parent
iterators and current names retain scoped charges while callbacks can execute;
all exits reconcile their released ownership. Deferred graph evaluation hands
its frontier back to its caller without a realm cache and charges it across
dependency JavaScript. Collected pending dependency names and the remaining
evaluation-list buffer also retain scoped charges while a later dependency
executes. Earlier traversal/frontier evidence covers ownership across callbacks;
Linking/export traversal producers, module-evaluation recursion, full module
caches, shared programs and broader native/integrated acceptance remain open. The
module-dependency batch follow-up is an M1z implementation checkpoint, separate
from the original M1y evidence and final integration acceptance.

### Native execution ownership

Active execution owns its operand/argument/binding buffers, binding names,
scope vectors and nested slot buffers, completion storage, cell/eval maps,
dynamic outer environments, lexical names, template table, module association
and call stack. Their reported capacity joins primitive payload accounting at
execution safepoints. Scoped control charges follow allocations displaced into
Rust callers. Call, eval, nested-script and generator transfers select the
owners each path actually displaces; ordinary calls forward eval maps into the
callee and make recovery copies only when async/debugger suspension can occur.
Necessary recovery copies retain their own charge.

Call entry now jointly admits forwarding-stack growth and any async/debugger
recovery copy before constructor getters, result-capability publication or
caller-context replacement. Preparation includes each copied table, name,
shadowed-cell buffer and module-name string, then checks actual capacities.
Reserved forwarding slots stay in VM ownership; synchronous calls allocate no
recovery copies. A scoped owner retains required copies through callbacks and
detached field transfers, releasing after caller restoration. Refusal preserves
caller environment contents and reconciles temporary ownership for peer realms.

Suspended frames retain both primitive and control charges, and their container
records add only storage outside those charges. Stack splits and debugger stack
edits refresh both saved charges, including a primitive returned into a paused
parent; restore returns ownership
to the active context. Generator resume consumes its saved operand vector
instead of keeping an unused cloned copy across the body. Completed bounded
root scripts return their frame containers; persistent template identity and
module association remain charged to the realm.

New binding frames first admit value slots, metadata/name copies and function
variable-scope lexical names as one staged allocation. A scoped owner charges
their actual capacities while constructor getters/field initializers or other
entry work runs, and releases after all parts enter VM fields. Root script,
ordinary/async call, generator parameter/resume, eval, nested script and module
entry share this path. Generator resume prepares names before taking saved
state; module entry admits before taking input cells or clearing the context.
Other container growth, map/scope restoration and non-JavaScript staging remain
part of the wider preallocation gate.

Scope entry jointly admits both scope-record vectors, the copied slot list and
cell-alias table growth before changing bindings. It checks minimum table
growth before preparing a replacement and then admits its reported capacity.
Entries removed by the same scope free slots for new aliases; available record
slots add only the copied slot-list allocation. Eval aliases keep priority over
script globals, and module outer cells stay owned by the linked graph.

Generator resume prepares copied scope-slot lists before taking its saved
state. The outer vector and each nested buffer retain a native charge until
transfer into active fields. A common resume exit reconciles released staging,
including refusal after binding metadata was prepared, so peer realms see the
released contribution. Other module-association
copies and per-iteration cell staging remain in the broader transient gate.

Function-eval declaration batches prepare their dynamic-binding and slot-alias
tables, copied names, shadowed-cell lists and temporary root slots before cell
allocation or environment writes. Replacement tables coexist with live tables
and retain a lease for their full reported capacity until commit. Existing
table slots require no replacement; already recorded capture aliases require
no new name copy. Deleted-variable recreation uses the same admission path
before restoring the retained cell's value.

All cells are initialized before publishing environment properties. A failed
heap write rolls back published environment edges and keeps a recreated cell
deleted, leaving both native maps untouched. Rollback of an existing ordinary
environment edge can restore its old cell even after a document heap failure
is latched. Staged names/tables and Rust-held primitive values retain native
ownership through heap collection and release on every exit; new/replaced
object edges remain rooted until publication or rollback completes.

Realm-global binding records own their table capacity and name buffers for the
realm lifetime, independently of active script frames. Script and global-eval
declarations prepare both the persistent table and current slot aliases before
creating declaration cells/properties. Full replacement capacity is leased while
old tables coexist; copied initial property values retain a separate payload
lease before any declaration cell is created. Standard-global validation retains
its lazy materialization behavior. Existing accessors receive no cell/alias,
and already recorded aliases need no copy. Staging leases update by byte deltas
as values/names/tables transfer rather than rescanning the batch for every row.

Deleted-global recreation admits its record and incoming value first, registers
the root, initializes the retained cell and then defines the ordinary global
property. Refused property/heap/root operations leave the cell deleted and no
new global record/root. Permanent roots are released when records are removed
or replaced. Lexical cleanup removes entries in place, avoiding a temporary
cloned-name list, and releases an empty table when no restoration slots remain.
Ordinary abrupt scripts keep their specified persistent global TDZ records.

ShadowRealm reentry admits a temporary lexical table before extracting entries;
keys and permanent-root ownership move without cloning names. A native lease
persists while the inner evaluation runs. New global publications reserve slots
for every suspended lexical record so restoring them requires no allocation,
including after a resource abort. Restoration releases an overwritten property
binding's old root. Eval/nested-script return reconciles the final caller fields
after displaced/staged owners drop, so peer accounts see released aliases too.

Generator resume peeks at the saved frame's owned buffers, operand length,
completion count, inherited environments and callee without extracting state.
Joint admission includes the incoming native contribution, caller/environment/
resource/pending-completion roots, operand/result slots, the typed completion
target and the required module-name copy. Actual vector capacity is checked
before taking state or displacing caller fields. Refusal leaves the saved frame
and caller contents intact; completed/running states allocate no resume buffers.

Extracted buffers retain a native lease until active fields and typed records
adopt them. Completion conversion moves values into an admitted vector without
allocating at installation. A borrowed pending-reference iterator appends roots
directly instead of building an intermediate root vector. Module association
preparation preserves the original referrer and caller restoration behavior.
Generator parameter entry admits caller/inherited root stack capacity together
with any extra parameter-eval environment slot before creating the generator
or displacing caller fields. Planning includes the outer receiver/generator
edges and the temporary root used while constructing that environment. Actual
reported capacity is checked before entry effects. Forwarded environment growth
extends the existing call-entry lease, which releases only when active fields
adopt the allocation.

Root construction preserves bindings, cells, caller/closure environments,
arguments, pending completions, disposal records, new.target, home/parameter
environment and current/outer dynamic cells including shadowed references.
Only object edges are copied for GC, leaving primitive buffers with their real
owners. Refusal preserves caller fields and terminal failure; parameter throws
use the ordinary restoration path. Other association copies and wider native
storage remain in the broader preallocation gate.

Closure module-referrer associations are a weak-key realm table with a retained
hash-allocation ledger. Deletion refunds dead name buffers while backing remains
charged independently of insertion headroom. Heap collection never traces those
keys as strong roots. Empty storage releases without admission once no in-flight
publication relies on its slots, including terminal cleanup. Sparse compaction
leases a complete replacement, moves survivors and reconciles the smaller owner
before refunding its candidate. Insufficient temporary headroom is nonterminal;
both the shared staging guard and pending-publication count defer reclamation.

Before a Closure instruction creates prototypes/capture cells or a function,
it jointly leases the referrer copy and full detached replacement table while
the old allocation coexists. Reported hash capacity is admitted before allocating
the name, then its actual capacity is checked too. Both allocations are fallible.
The independent document lease follows Rust-held staging even after VM destruction;
unpublished data drops before the lease and reservation. Publication can reuse
GC-freed slots, while keeping enough backing for other outstanding reservations.
A candidate is adopted when those reservations need its capacity and it grows
existing backing. An older candidate never shrinks a larger table, even when a
newer reservation requires further growth; newer work supplies that capacity. All data and the
reservation transfer/drop before optional deferred reclamation. Interpreter
unwind releases unused staging and empty backing.

Calls independently lease and copy an associated referrer before consuming an
instruction, adding call roots or displacing caller context. The copy stays
charged after VM destruction and until active fields adopt it; the displaced
caller name has a separate execution owner. Closure/call publication refunds
staging immediately before the synchronous VM ownership update, without an
intervening allocation or callback. This avoids checking a transient duplicate
charge against an exact allowance. Success and error restore the original caller
allocation. An absent association uses the allocation-free path; an associated
empty name still replaces the context. Generator resume keeps its existing
pre-extraction referrer preparation. Other module-name/code/cache/service copies
and retained maps remain separate open work.

Module instantiation/evaluation prepares its context-name copy before replacing
caller fields or entering the frame. A scoped name lease swaps the prepared
allocation into the VM and owns the displaced caller allocation while module
code runs. Return, throw and await restore the original caller buffer; entry
refusal releases preparation and reconciles before returning to its caller.

A debugger-paused module keeps its ambient name in the VM, while the suspended
execution owns the module name. Resume, step, nested throw and abort move the
ambient allocation into a scoped lease and back. Continuations no longer clone
or retain a second ambient-name backup. Moving these existing owners requires
no additional native budget and remains available during terminal cleanup.

ShadowRealm import prepares the inherited name after the required argument
coercion/type checks and before Promise capability creation. The parent leases
that independent copy until it transfers into the child, then reconciles the
parent contribution before admitting the child's active owner. Name-preparation
refusal creates no capability. Full source-registry, continuation identifier/
code/cell-map and other module-cache/service storage remain open work.

Raw JavaScript, JSON, decoded text, bytes and source-phase name tables are
immutable snapshots shared by attached realms. One owner charges the document
for its retained table capacity, key/data capacities and owned header; another
realm's snapshot reference adds no buffer charge or deep copy. Replacement keeps
old snapshots live until their final owner releases them, including discarded
realm cleanup. This data never contains realm-local ObjectIds. Compiled
snapshots can also share immutable data; synthetic object exports retain their
separate per-realm filtering/rebuild policy.

Host installation admits the complete incoming owner while prior snapshots
coexist. The existing void setter API retains the old snapshot and latches
document scheduling failure on refusal. Clearing sources remains available
after terminal failure. Source-phase input vectors deduplicate without another
table, admit minimum storage before fallible set reservation, check actual
table capacity and charge the incoming vector until its strings move. Bounded
activation attaches previously installed snapshots once; snapshots cannot be
adopted into another document's account. Transient parse/compile/conversion and
compiled-registry/code/cache/continuation storage remain separate open work.

Synthetic JSON/bytes values require a successful GC root registration before
their module record is published. A refused registration propagates its original
heap error through graph preparation. Fresh-graph preparation releases any
earlier synthesis roots on failure; an existing graph preserves its prior roots
and linked records. JSON synthesis wraps only malformed input's SyntaxError as
a module-resolution failure. Heap, instruction and scheduling refusals keep
their original host-abort types; configured string limits retain their ordinary
RangeError behavior. This failure contract does not account for transient
conversion or compiled-cache storage; those ownership gates remain open.

Synthetic exports now retain one immutable derived-value owner across Bytecode
snapshots. Its document charge includes the owned header and actual primitive
buffer capacity; clones add references without copying or recharging that
storage. Object exports still require graph GC roots and are filtered before a
ShadowRealm inherits compiled records. The child rebuilds those realm-local
objects. Final owner drop refunds the charge, including terminal cleanup.

Bounded activation, host loader installation, borrowed graph inputs and binding
installation attach preloaded exports once. Another document cannot adopt an
existing owner; refusal latches the new document and a void host setter retains
its previous registry/referrer. Raw source snapshots remain borrowed during
conversion rather than making another UTF-8 copy. Text conversion admits minimum
UTF-16 storage before fallible reservation and checks actual capacity before
filling it. JSON modules borrow their immutable UTF-8 sources directly, retaining
an admitted AST until conversion ends; they need no UTF-16/UTF-8 input copies or
reviver source maps. General JSON runtime accounting remains separate work.

The shared JSON grammar parser leases actual module AST vector/key/value
capacities. Fallible growth admits a complete replacement while its prior
allocation exists and checks actual capacity before publication. Module mode
omits primitive source-text copies; general reviver parses preserve that data.
The complete grammar is validated before any managed object construction.
Bounded documents cap module container nesting at 128; the standalone parser
retains its independent policy. Tests lower the cap to two for a tiny refusal
case rather than running a deep-recursion workload.

JSON conversion admits its maximum ancestor/child root slots before heap
effects, preserves the operand prefix and keeps object children rooted through
property installation. Independent UTF-16 key/value copies retain scoped leases
until managed adoption. Array index keys use fixed stack digits. The returned
primitive's existing charge merges with its prepared export header in the same
document without another payload reservation; foreign-document transfers fail.
AST, copy and root cleanup preserve original syntax/resource error identities.

Bytes copies own scoped capacity until the heap adopts their immutable buffer.
The temporary buffer root's operand slot is admitted before prototype or buffer
allocation. Binding installation similarly admits its independent primitive
copy. These leases release inside the heap operation immediately before managed
adoption, and unwind on earlier refusal or heap failure. Managed strings/buffers
then carry their heap charge; the original export keeps its separate shared
native owner. Shared child/source/diagnostic program storage, compiler staging
and other parser/continuation/cache/service buffers remain separate gates.

Dynamic module compilation preflights the unavoidable copied-source/EOF-token/
position minimum before parsing, including empty input. Returned AST buffers
then retain a document owner through compilation. Two allocation-free traversals
count native structure and invoke source admission; distinct SourceText payloads
use independently admitted, coexisting hash-set scratch. Template placeholders
may own separate source allocations and are counted alongside their parent.
Scratch drops before the parsed owner returns. Accounting preserves the existing
AST metric's logical BigInt-limb model and allocator/control-block exclusions.

Script, module and diagnostic lowering share a borrowed body slice. Module
entry no longer deep-copies its executable tree into a temporary Program, and
strict restricted-name errors precede unused diagnostic inventories. Parsed
input and admitted root-code buffers coexist until handoff; the input drops
before registry copying/publication. Parser resource errors remain resource
errors. Full token/AST/compiler producer preallocation and compiled shared child/
source/diagnostic storage remain separate open gates.

Runtime-parsed dynamic modules transfer each distinct SourceText payload out of
the AST lease into a shared source owner before compiling. Source-owner headers
and a compilation-only identity lookup are admitted separately; growing lookup
tables charge the complete replacement while the old backing still exists.
Fresh compiler output receives those owners before publication without copying
code. Each code unit retains its own source owner, while nested functions share
the owner for the same allocation. Distinct template-fragment sources release
independently. Dropping the parsed input releases its AST and lookup backing;
dropping a registry/root code does not refund sources still held by closures,
timer/Promise callbacks, generators or suspended execution frames. The final code
user releases the source data before its lease.

Unbounded runtime sources retain uncharged owners for later document activation.
Binding a heap visits existing closures and generator states; new closure and
generator allocation validates source ownership before allocating the object.
Registry adoption and saved async/debugger/module frames also bind their code.
Same-document attachment shares the lease, foreign-document attachment fails,
and a cached identity cannot bypass terminal failure. Ordinary host-compiled
programs receive no runtime source owner and keep their cross-VM reuse contract.
This source transfer covers runtime-parsed dynamic modules only: host-precompiled
sources and their shared child-code buffers/headers, plus diagnostic maps, remain
open, along with compiler producer preallocation and allocator/control-block
exclusions.

Runtime-parsed dynamic-module child code now has a shared owner for its complete
inline header and owned buffer capacities, using the same code/value metric as
root buffers. Each shared child is charged once; registry copies, closures,
generators and saved frames retain its handle without copying the code or adding
a second charge. Nested children own their own storage, and final drop destroys
the code before refunding its lease. Source allocations keep their independent
owners. Root-code buffers remain with their prepared/registry owner, so they are
not also charged to a child owner.

Unbounded runtime children keep attachable owners. The source/code adoption paths
bind their native storage before execution; even a child without SourceText must
fit its header and buffers and cannot move into another document. Host-compiled
code keeps its existing reusable, host-neutral policy. Compiler-only mutable
access ends when runtime ownership is installed. Debugger/diagnostic metadata
edits use an admitted copy-on-write path: the old and complete candidate coexist,
unchanged nested children stay shared, and retained buffers are reconciled even
when the editor returns an error. Unique edits need no new copy; terminal cleanup
may release buffers without clearing failure. Diagnostic binding and code-unit
preparation return their resource error if a copy cannot be admitted. This is
storage ownership work; M1q's coordinate refinements remain parked.

Admission here covers returned code before publication and coexisting metadata
copies. Other script/eval programs, compiler producer preallocation,
host-precompiled shared program storage, diagnostic map/registry storage,
allocator/Rc-control overhead and integrated acceptance remain open.

All four CreateDynamicFunction kinds now use the same retained AST/source/code
ownership as runtime module compilation. Encoded parameter and body strings own
their reported capacities across later ToString calls. Encoding measures UTF-8
lexer bytes before growing the output, with a separate owner for the temporary
coerced UTF-16 string. Growing strings admit complete coexisting replacements.
HTML parameter filtering writes into an admitted output without a character
vector. It preserves UTF-8 boundaries, comment behavior and the specification's
original unfiltered source for Function.prototype.toString.

Parameter-only, body-only and joined grammar checks each retain their returned
AST through validation; no Function AST is cloned out of the parsed Program.
Script and module inputs share source deduplication and structural accounting.
Repricing retains previous admission while deduplication scratch exists, then
reconciles the returned payload. Renaming the internal binding and restoring
unfiltered source reserve replacement payload first. A source visitor keeps
distinct original, filtered and template-fragment allocations independent.

Compiled child code receives its durable native owners. Temporary root buffers
are admitted while the input and child coexist, capture-slot provenance is
checked, and root/input/text storage drops before constructor prototype getters
can call user code. Capture IDs use admitted scratch, then move into the managed
closure without a second vector; already rooted operand slots supply subsequent
binding updates. Syntax, prototype-getter and admission errors unwind these
owners. Live closures, generators, suspended async frames and timer/Promise
callbacks retain generated code after the constructor returns. Previously
unbounded generated functions attach when document accounting starts.

This covers constructor input/retained ownership and the returned AST/code
handoff. Full tokenizer/parser/compiler/coercion producer preallocation, ordinary
script/eval and host-precompiled shared ownership, diagnostic map/registry
storage, allocator/BigInt-spare/Rc-control exclusions and integrated acceptance
remain open. M1q coordinate refinements stay parked.

Direct and indirect eval now encode their borrowed UTF-16 argument into an
independently admitted lexer-text buffer. Non-string inputs retain their no-code
fast path. The returned Program uses the shared AST/source carrier, preserving
inherited strictness for direct eval and fresh Script grammar for indirect eval.
Parser resource errors remain resource errors. Encoded text drops after AST
admission; compiler lowering stays unchanged.

The direct-eval compiler snapshot owns returned visible-binding vectors, both
copies of binding names, dynamic-capture map capacity, variable names and lexical
conflicts through compilation/capture preparation. Root code has a separate owner
for its own buffers while shared children and sources retain theirs. The AST
drops before execution and compiler snapshots drop before guest code. Root code
stays charged throughout declaration setup, getters, execution and caller
restoration. Global-object lookup errors propagate through the fallible path.

Capture scratch is admitted before gathering IDs. Frame maps take the values and
release the scratch lease immediately after adoption, before guest callbacks.
Legacy internal eval entries use the same handoff. Lexical conflict inspection
reads only closure code, avoiding a copied capture vector and receiver. Normal
returns, throws, admission refusal and VM destruction release the appropriate
owners while keeping terminal failure latched. Returned/queued closures,
generators and saved async code retain independently shared source/code storage;
unbounded eval code attaches when document accounting begins.

Eval environment producers now admit borrowed selection vectors before sorting
and deduplicating, then admit returned vector/map capacity and all selected name
payloads before copying. Active declarations retain last-match precedence;
captured/live fallback bindings retain first-match precedence. Parameter-eval
metadata and dynamic cells keep their existing eligibility. Variable and lexical
name lists preserve function/arrow/catch boundaries and global append ordering.
Complete replacement lists coexist with their old snapshot until publication;
old data drops before refund, and new leases transfer without a second charge.
Refusal leaves prior names unchanged. Test-only tree selectors provide the
original semantic reference; production no longer builds those trees.

This covers direct/indirect eval returned inputs, compiler snapshots, their
environment producers and durable code. Full parser/compiler producer
preallocation remains open, together with
ordinary host-precompiled shared script ownership, diagnostic map/registry
storage, allocator/BigInt-spare/Rc-control exclusions and integrated acceptance.

ShadowRealm evaluation borrows its original native UTF-16 argument and uses the
same admitted text/AST/root/source/child-code carriers. Before encoding, it
selects the child's document account, falling back to a bounded caller for a
neutral child. Active-call guards retain zero-owning account views, allowing
reentrant compilation-policy lookup without another VM-pointer dereference.
Different document accounts refuse before compilation/context changes; a busy
record lacking an active ancestor still parses before reporting that error.

Prepared entry synchronizes the child and validates or attaches code ownership
before resetting lexicals or suspending a frame. Root code remains charged
through execution, Promise cleanup and wrapper metadata getters. Shared code and
source survive returned functions, pending timer/Promise callbacks and Rust-held
handles, including late activation of previously neutral code. Catchable getter
failures remain opaque; fatal host errors keep their identity and release
candidate pins. Complete neutral-tree policy adoption and thread-local active-
call backing remain separate open control-storage work, alongside the wider
native/integrated gates above.

Ordinary host-compiled children acquire document-owned bytecode copies at heap
closure/generator creation, generator-state restoration and late document
activation. The host input keeps its neutral identity and cross-document reuse.
A shared weak cache lets related heaps reuse one live copy and one source owner
per immutable source allocation. Already owned runtime code keeps its original
owner, and foreign runtime provenance refuses before publication.

Candidate headers/buffers and full coexisting cache growth acquire admission
before cloning/allocation, with reported capacities reconciled before storage.
Weak zero-sized host tokens prevent identity reuse without retaining original
code allocations; edits replace an observed host identity before mutation.
Cached document metadata edits admit complete coexisting copies.

Code/source allocation headers retain separate native leases while weak entries
outlive their data. Weak pointers drop before their header credit is returned.
Caches hold no strong code/source roots and cannot keep a closure or realm alive.
GC removes dead entries, releases empty backing and optionally replaces sparse
backing through nonterminal coexisting admission. Failed optional compaction
keeps old backing charged. Actual heaps hold cache ownership; the account's weak
discovery pointer creates no strong cycle and clears when that cache drops.

Returned code, generator/async frames and timer/Promise callbacks retain their
document code/source owners through final use. This covers retained host child
code. The borrowed outer/debugger buffer follow-up below has an implementation
checkpoint. Host-side program registries remain separate required
program-storage work.

The root/debugger buffer follow-up (2026-10-18) introduces a temporary native
lease for the unique outer buffers borrowed by public script,
direct execution and dependency-free module entries. The lease survives guest
calls and outer cleanup. ShadowRealm's prepared eval entry uses its existing
root owner after document validation, avoiding another root-buffer charge.

Debugger root, nested and intermediate snapshots admit their candidate header
and unique buffers before cloning, then reconcile reported capacities. Saved
code acquires shared child/source owners through the existing document cache.
The saved owner moves through pause, step and resume; its bytecode drops before
its native credit. Late activation attaches existing paused snapshots to the
document budget, and foreign or terminal admission refuses before publication.
This code-buffer checkpoint covers saved code. The retained-frame follow-up
below adds saved headers, execution/record buffers and handler origins.

At the code-buffer checkpoint, the twenty-case local/offline run matched all
fifteen frozen Rust inputs by SHA256. It covered normal/throw cleanup, refusal before guest effects,
late activation, saved-code lifetimes after VM destruction and allocation
identity through step/resume. Nineteen additional exact tiny semantic controls
pass without replaying resource-admission probes. Together these cover
thirty-nine distinct tests across three executable targets. Formatting, diff
checks and offline workspace/all-target Clippy pass, with source/artifact and
Git content audits. Prior broader regression counts remain historical evidence;
these results do not establish fresh coverage or integrated acceptance. M1u
remains the latest accepted milestone and M1z's wider gates remain open.
Evidence: `target/root-code-storage-source-20261018.json`; the earlier
`target/root-code-storage-static-review-source-20261018.json` preserves the
initial review's pending status.

Retained debugger state now transfers published execution/record credit into
independent document leases. Active temporary contexts keep their VM counters;
durable owners remove their counter contribution before retaining the same
credit. Neutral saved state admits its existing storage during activation.
Data drops before its credit, while controller header leases drop last. Saved
code/records and captured frames can outlive the VM without keeping it alive.

Handler origins join record payload ownership, replacing the separate active
and paused diagnostic projections. Mutations reconcile the saved owner and VM
before limit/peak observation. Moving a filename has no duplicate peak, and
replacing it with equal-sized data immediately updates peer-realm usage.
Allocation producers still perform their admission, and cleanup retains the
terminal failure latch.

Root/nested/intermediate controller headers acquire admission before saved-code
copying; records become durable before new pause/serial publication. Activation
attaches all root/nested/intermediate/module storage, including module names.
Stepping and re-parking retain the next snapshot. Caught nested module throws
also stage a complete cell-table copy before installing the caller's updated
state. Failed captured handoff restores its original stack allocation.

The retained-frame checkpoint passes eighteen tiny lifecycle/conservation
contracts and forty-five exact related semantic controls: sixty-three distinct
tests across three executable targets. Formatting, diff checks and offline
workspace/all-target Clippy pass, with source/artifact and Git content audits.
The ordinary-library hook does not establish fresh coverage. Request/inspection
producers, other temporary/retained maps, program/diagnostic registries and wider
native/integrated gates remain open. M1u remains the latest accepted milestone;
M1z's integrated gates remain open. Evidence:
`target/retained-frame-storage-source-20261018.json`.

Active cross-realm registrations now admit individual metadata nodes before
allocation. Each node owns native document credit; an empty thread registry has
no retained allocation. Preparation synchronizes current ownership and captures
the heap tag/policy without a VM pointer. Publication stays at each original
synchronous call boundary and performs no allocation or admission. Forward
Test262 preparation precedes raw-child pointer extraction; reentrant ShadowRealm
preparation precedes lexical/frame displacement. Existing unsafe call sites and
their pointer validity windows remain unchanged.

The VM's bounded account pays first. Neutral calls may use an explicit peer or
the nearest active metadata owner's budget without adopting that owner's host
policy. Zero-owning account snapshots and newest matching-tag resolution retain
their behavior; safe linked traversal adds no VM-pointer dereference. Independent
bounded documents preserve separate admission/failure state. Nodes drop policy
and predecessor references before their native credit; published guards retain
the original synchronous stack discipline. Unpublished metadata captures no
pointer and can outlive its VM. The node payload follows the existing native
model's allocator/Rc-control exclusions.

The active-registry checkpoint passes eighteen new finite contracts, including
one Promise and one manually delivered timer callback across GC, eighteen
retained-frame contracts and forty-nine exact related semantic controls: eighty-
five distinct tests across three executable targets. Formatting, diff checks and
offline workspace/all-target Clippy pass; frozen source/artifact and Git content
audits bind the result. Ordinary-library hooks make no fresh coverage claim.
Request/inspection producers, complete neutral-tree adoption and wider native/
integrated gates remain open. M1u remains the latest accepted milestone; M1z's
integrated gates stay open. Evidence:
`target/active-realm-storage-source-20261018.json`.

Allocating native debugger stack/value APIs now return
`VmDebuggerInspection<T>`: shared immutable output with an independent document
lease. Cloning shares data and credit without another allocation; dereferencing
borrows it. Separate reads own separate copies. Output headers, vector slots and
owned key/value buffers retain their native credit until the final result drops,
including after VM destruction, navigation or close. Data drops before its lease;
the existing allocator/Rc-control and Rust-stack exclusions still apply.

A fixed borrowed frame plan validates exact classic/linked/foreign generations
before copying frames or scope slots. Value inspection borrows binding/cell data.
Heap inspection validates and measures a complete tree before admitted copying;
fixed stack ancestors and borrowed key ordering replace temporary heap scratch.
Sparse array holes, stored string order, UTF-16, primitive bit patterns and fixed
plain-data limits keep their semantics. BigInt output is encoded into its admitted
final buffer. These immutable passes run no getter, Proxy trap, coercion, bytecode
or GC. Unsupported VM targets/shapes allocate no output.

Module pause request names acquire `ModuleName` ownership before request/
diagnostic publication; late activation attaches existing names once, with foreign
or terminal refusal preserving their previous owner. Scalar nested requests remain
inline. Console object formatting uses the owned preview and propagates fatal
native refusal. Page Runtime preserves returned owners; launcher conversion borrows
them while explicitly copying wire data. Host IPC reply/serialization buffers
remain separate required transport ownership work.

The inspection/request checkpoint passes twenty-two finite lifecycle/data
contracts, eighteen active-realm and eighteen retained-frame contracts, and fifty-
three exact related semantic controls: one hundred eleven distinct tests across
four executable targets and fifty-six selections. Formatting, diff checks and
offline workspace/all-target Clippy pass, including static host consumers. Frozen
source/artifact and Git content audits bind the result. No fresh coverage or
integrated acceptance is claimed. Host reply/serialization buffers, other native
producers and complete neutral-tree adoption remain open; M1u remains latest
accepted and M1z's integrated gates stay open. Evidence:
`target/debugger-inspection-storage-source-20261018.json`.

Runtime-inspection transport now preserves that ownership across mapped copies
and encoded frames. `VmDebuggerInspection::try_map` admits a complete output
payload/header and buffer plan before conversion. `VmDebuggerOutput` stages new
vectors/boxes and raw backing moved into the result, consuming the complete plan
and reconciling reported excess before copies. Existing leased data keeps its
own owner. Terminal admission refuses before conversion and before publication;
data drops before credit, clones share completed storage and neutral policy stays
neutral. Independent documents retain separate admission/failure state.

Classic/linked/System stacks, linked spans, System locations and ordinary/linked
values retain native owners through launcher mapping. Linked intermediate stacks
use the wire shape with borrowed comparisons. The linked value target's moved
expected-stack backing joins its outgoing owner. Production `handle_reply` and
`BlueJsHostReply::write` retain mapped data until delivery finishes. Legacy raw
mapping helpers remain test-only; compile-only Core fault delivery uses the owned
entry too. No authority, target or wire-schema change is introduced.

JSON framing counts before allocation and serializes into fixed admitted storage
before publishing its prefix, preserving the existing write cap and bytes.
Encoded runtime replies have a separate document owner that stays live through
write/flush. Native admission failure becomes the ordinary bounded ResourceLimit
reply, preserving the connection for healthy documents; other encoded-candidate
preparation failures use HostFailure. Actual I/O errors propagate while releasing only their candidate.
Other metadata/control/error replies and incoming decode buffers remain separate
ownership gates.

The reply checkpoint passes eleven native projection, six in-memory framing and
nine pure mapping/writer contracts plus the previous 111 finite controls: one
hundred thirty-seven distinct cases, six executable targets and fifty-nine
selections. Formatting, diff checks and offline workspace/all-target Clippy pass,
with frozen source/artifact and Git audits. All runtime tests are local finite
functional/in-memory cases; no service/listener/Core/MCP or resource-boundary
replay is resumed. No fresh coverage or integrated acceptance is claimed. Wider
native and complete neutral-tree gates stay open; M1u remains latest accepted and
M1z's integrated gates remain open. Evidence:
`target/debugger-reply-storage-source-20261018.json`.

Late bounded activation now adopts an existing mixed realm graph before work
resumes. A paid iterative discovery uses shared ShadowRealm anchors and paths
through exclusively owned Test262 children; it introduces no VM-pointer access
or recursive graph walk. Queue/member/path/set/task storage is admitted before
allocation. Each heap joins once across aliases and cycles. Discovery checks
exclusive availability, so an immutable or mutable member borrow refuses before
policy publication and can be retried. Active neutral roots also refuse. Shared
records from another document are skipped, including active foreign records with
a captured independent scope; conflicting owned members refuse without changing
the foreign account.

One native account retains an opaque managed budget whose native identity is
weak, avoiding an ownership cycle. Activation publishes a common terminal owner
to all prepared members, preflights their aggregate existing managed storage,
then joins each heap and attaches local code/debugger/collector/symbol/module
storage. A managed terminal failure stops shared native admission, including
retained native owners; releasing old credit stays permitted. Existing same-
document scopes keep their own usage, and repeated activation reuses the ready
policy. Fresh ShadowRealm and Test262 children inherit and attach before
publication; Test262 harness storage is reconciled before publishing its facade.

Neutral pending timer/browser/finalization serials migrate into one reserved
shared range. Old serial plus heap identity gives a deterministic tie order where
independent neutral realms had no document-wide enqueue order. Within-realm order,
timer handles and deadlines survive; already-bound serials are preserved. Stored
clocks merge without invoking an embedding callback during activation. Existing
ShadowRealm task delivery can now see adopted child timers and Promise work.
Owned Test262 host-turn traversal remains a separate open mechanism; accounting
adoption does not claim that its conformance timers become browser task sources.

The realm-policy checkpoint passes nineteen fixed tiny contracts and the previous
137 finite controls: one hundred fifty-six distinct tests, six executable targets
and sixty selections. Mixed ownership, alias/cycle conservation, borrow/refusal/
retry, active/foreign boundaries, fresh inheritance, idempotence, timer identity/
serial/deadline conservation and timer/Promise completion through GC are covered.
Synthetic terminal-state checks use no exhaustion or represented large payload.
Formatting, diff checks and offline workspace/all-target Clippy pass, with frozen
source/artifact and Git audits. No fresh coverage, service/runtime sweep or
integrated acceptance is claimed. Wider native and transport ownership gates stay
open; M1u remains latest accepted and M1z integrated gates stay open. Publication
remains local; rejected external push/process inspection is not retried. Evidence:
`target/document-policy-source-20261018.json`.

Document host turns now follow both owned Test262 and shared ShadowRealm edges.
An iterative discovery admits queue/set/result/path backing before allocation,
visits each heap once and sorts the resulting identities. Each snapshot stores
only scalar ownership steps and heap identities, retaining no VM or raw VM
pointer. Route replay revalidates member identity and document scope, skipping
removed, active, foreign or borrowed members. Nested path credit survives the
snapshot and is released with its buffers; owning maps and runtime reachability
continue to determine VM lifetime.

Shared entry registers its suspended ancestor for the callback's dynamic extent;
owned entry reuses the existing Test262 reverse-membrane helper. Its ordinary
foreign-operation mode still resets child fuel, while the host-routing mode does
not. Registry metadata is admitted before leaf work and released on every exit.
Route replay uses the existing native-stack reserve check. No new unsafe block or
VM-pointer dereference is introduced; pure wake inspection preserves realm fuel.
Reentrancy here covers ancestors on the selected ownership route. Callback
reachability through alternative alias routes and agent-wide WeakRef/notification
boundaries remain unverified and are separate agent-level gates.

Document checkpoints revisit root and reachable members until a pass runs no
Promise job, sharing one remaining-job counter across passes. Rediscovery catches
new children and jobs queued back into an earlier ancestor. Every selected task
finishes a document checkpoint, including the final task in its allowed turn.
Existing callback-local checkpoints are preserved. Logical-source FIFO and source
arbitration remain document-wide; initial timer/abort snapshots preserve interval
and newly queued timer deferral. Browser-only turns still exclude callback timers.
Conformance timers remain a separate harness mechanism: verification reads their
installed native tag without invoking their background-thread scheduling API.

The mixed-realm checkpoint passes twenty fixed tiny contracts, the previous 156
finite controls and seventeen selected snapshot/task-source/finalization controls:
one hundred ninety-three distinct cases, six executable targets and seventy-five
selections. Formatting, diff checks and offline workspace/all-target Clippy pass,
with frozen source/artifact and Git audits. The new contracts cover direct/nested
ownership, parent callbacks, ancestor Promise jobs,
alias/cycle discovery, busy retry, foreign boundaries, task-source order, interval
eligibility, abort-only delivery, finalization, nonretaining snapshots, terminal
entry and unchanged wake fuel. No quota/resource-boundary, exhaustion, infinite-
input, network/service/listener/Core/MCP or privileged runtime replay is resumed.
No fresh coverage or integrated acceptance is claimed. Wider native/transport
ownership and integrated gates remain open; M1u remains latest accepted and M1z
integrated gates remain open. Publication remains local, with no retry of rejected
external push/process inspection. Evidence:
`target/host-realm-tasks-source-20261018.json`.

Document checkpoints now preserve kept WeakRef object targets across bounded
script return, child/ancestor Promise jobs and callback-local cleanup. The public
host checkpoint drains the reachable same-document graph to quiescence with one
remaining-job counter and aggregates catchable job exceptions. A suspended
same-document call defers local completion using its captured account view;
registered VM pointers are not dereferenced to decide that policy. Reentrant
ShadowRealm setup explicitly defers the child's document even with a neutral
caller, because it temporarily hides the ancestor's call depth.

Rejection notification publication waits until peer jobs can attach handlers.
Completion reporting may queue further jobs, which are included before clearing.
All available kept sets clear before any realm's optional post-checkpoint GC.
Bounded script cleanup leaves kept targets for the explicit owner checkpoint;
neutral jobs retain their local boundary. Bounded reentrant evaluation keeps both
inner and outer targets in the document set rather than discarding the inner set
when restoring its caller. Existing callback-local checkpoints still run, but
protected task entry prevents them from ending document keep-alive prematurely.
Direct timer entry drains earlier document jobs and finishes the same boundary.

Account-local deferrals hold no VM and balance on normal and failed exits.
Discovery reports borrowed/active routes; a blocked member leaves completion
pending, returns an immediate retry wake and prevents later task consumption.
Completion resumes after the borrow ends. Published microtask counts, rejection
records and kept buffers request cleanup themselves, including work created by
onerror after ordinary script cleanup already inspected the VM. Foreign document
activity does not acquire this document's completion boundary.

Kept object-handle backing has an independent native owner. Complete replacements
are admitted while old storage coexists; duplicate keeps use existing storage.
Late activation attaches existing neutral backing once. A foreign rebind refuses
without failing its source account. Moving a kept buffer through Rust preserves
its credit, including after VM destruction; buffers drop before credit. Explicit
completion frees backing, and terminal cleanup admits no new storage. Five-entry
replacement and synthetic-terminal cases verify ownership without quota pressure.

The document checkpoint/storage checkpoint passes twenty-five fixed tiny contracts,
the previous 193 finite controls and twenty-seven selected public/saved-owner
controls: two hundred forty-five distinct tests, nine executable targets and 103
selections. Formatting, diff checks and offline workspace/all-target Clippy pass,
with frozen source/artifact and Git audits. The contracts cover eight reproduced
failures, eventual collectibility,
displacement, late binding, duplicates, native growth, terminal release, busy retry,
independent documents, reentrant targets, notification uniqueness, later handled
events and direct timer ordering. No fresh coverage or integrated acceptance is
claimed. Alternative alias-route callback reachability and wider native/transport/
program ownership remain open; M1u remains latest accepted and M1z integrated gates
remain open. No quota/resource-boundary, exhaustion, infinite-input, network/service/
listener/Core/MCP or privileged runtime replay, conformance timer thread, process
inspection or killing is resumed. Publication remains local; rejected external
push/process inspection is not retried. Evidence:
`target/document-checkpoint-source-20261018.json`.

Compiled registries retain immutable snapshots with one document charge for
their owned header, logical table capacity, key capacities and root code-unit
buffers. Root vectors include diagnostic instruction owners; constant payloads
use the existing Value charge model. Shared child code, SourceText and
diagnostic maps are excluded from this root-owner count. Runtime-parsed module
sources use the separate owners above; the remaining program storage is a
separate program-ownership gate. Snapshot references share buffers and final drop
refunds them, including empty reserved tables and terminal cleanup.

Host loader replacement admits its incoming registry/referrer while old owners
coexist and preserves both on refusal. Empty registry/empty-name cleanup remains
available after terminal failure. Bounded activation measures existing execution
fields before admission and retains a failed scope. Preloaded compiled snapshots
attach once and cannot transfer into another document's account.

Borrowed host graphs admit a checked root-buffer copy before graph mutation.
Internal immutable views share their installed snapshot; a lazy draft copies
only on its first insertion. Clone admission uses new vector/string lengths,
then validates reported capacities. Draft growth admits a full replacement
table while old storage and incoming records coexist. Existing entries move
without another code copy; publication merges already-admitted charges and
refunds old/replaced buffers. Refusal preserves prior draft contents/capacity.

Preparation failures release all newly synthesized roots. Linking failures
before evaluation restore the prior compiled snapshot and root prefix, avoiding
a cached synthetic record whose object lost its roots. Evaluation retains its
published snapshot. ShadowRealm inheritance shares unchanged code, or admits a
filtered copy excluding caller-heap object exports before Promise capability
creation. Refused filtering immediately publishes its referrer-lease refund to
peer realms. Graph/cell identity and heap roots remain realm-specific. Compiler
production scratch and other graph/cache/service owners remain open work.

Top-level await and debugger module frames retain a code view into the immutable
registry that supplied their executable. The view owns that entire snapshot;
its ordinal remains valid because the map never mutates. A clone neither copies
root Bytecode nor reserves its buffers again. Replacing the active loader keeps
pending executable generations charged until their final frame/view drops.
Debugger completion also resolves its namespace against the saved snapshot.
This is snapshot-level retention, not per-code-unit reclamation.

Frame identifiers are admitted before copying and before execution-state capture.
Their capacity leases follow stored frames and Rust-held resumption owners;
repeated await prepares its new identifier before moving the next frame. Saved
records subtract already-leased capacity. Enabling bounded mode attaches pending
code snapshots even when the active registry has been replaced; unleased saved
debugger fields participate in initial measurement. Resume transfers preloaded
identifier capacity into an execution lease. Completion, rejection, registration
refusal and terminal debugger cleanup return these owners. Shared child/source/
diagnostic programs, compiler staging and linked-cell-map copies remain open.

Linked module cell tables now reserve admitted root-scope capacity before any
managed binding cells are created or import aliases are installed. Prepared
copies validate both minimum slots and reported logical capacity. Their scoped
leases use a separate realm counter, so installed, parked and Rust-held graphs
remain accounted without retaining persistent tables on frame-displacement
counters. Entry and declaration instantiation move admitted storage into active
VM ownership; completion/error moves it back without a second reservation.

Await and debugger exits stage independent graph copies while the active frame
still roots their cells. Refusal follows engine-error cleanup before capturing
the next frame. Copy publication replaces/refunds the old record owner; return,
throw and debugger abort handoffs update the retained table's capacity. Bounded
activation attaches preloaded graph tables before measurement. Failed linking
releases only its new records and roots, while fresh failed graphs drop cell
owners before publishing cleanup. Realm drop refunds its contribution to peers.
The surrounding graph record/name/root containers, shared programs and other
compiler/native/integrated ownership remain separate open gates.

Module record tables retain one document charge for logical table slots and
canonical key capacities. The owner moves with stored, parked and Rust-held
graphs. Growth admits a full detached table while old slots and keys coexist;
existing entries move after admission, their charges merge, and freed old slots
refund. Key copies are admitted before allocation and spare slots require no
table-growth charge. Removal refunds key storage even after terminal failure;
empty reserved tables remain owned until their final drop. Cached key capacity
keeps batch rollback from repeatedly scanning the complete table.

New-name selection borrows immutable registry keys into one admitted, sorted
vector held across module JavaScript. Failure preserves the previous graph and
releases only preparation roots; the vector drops before final cleanup and
Promise draining. Bounded activation attaches preloaded record metadata once;
foreign-document attachment and mutation without the owner scope are refused.
Link rollback removes new names and keeps actual retained capacity charged.
Graph root-list merging, native payloads of retained record values, shared
programs and other compiler/native/integrated storage remain open work.

Module graph, nested-load and temporary namespace/synthetic root lists retain
admitted segment headers and reported root-slot capacity. Reservations precede
managed construction and root registration, then commit without allocation.
Failed registration drops only staged storage; rollback releases new roots and
emptied segments while preserving the old prefix. Primitive JSON needs no
object-root reservation; object/array JSON and bytes reserve before conversion.

Cleanup splices already-owned segments without allocation or new admission,
including after terminal failure. Iteration/destruction use loops. Cached owner
identity avoids repeated scans of attached segments; bounded activation attaches
host-neutral lists once and rejects a foreign document. Root snapshots used by
fixtures copy handles without duplicating storage ownership.

Cached object completions acquire persistent roots before publication in normal,
debugger and await paths. A later script collection therefore preserves the
cached value; failed await rooting publishes no completion. Error roots follow
the same publication order. Linking, namespace and ImportMeta registration
failures propagate their runtime identities.

Displaced execution references borrow saved frame fields through an iterator,
including pending completions and completion-save values. Scoped-root consumers
need no intermediate object vector. Temporary persistent registrations use
admitted ModuleRoots segments; transactional append unregisters only its suffix
on refusal and preserves an existing prefix and its capacity ownership.

Queued jobs remain queued until frame rooting succeeds. Empty checkpoints perform
no frame displacement or registration. Rooting errors restore the caller before
returning, including deferred namespace observers. Reentrant ShadowRealm entry
stages frame, parameter environment and kept weak-target roots before taking the
remaining owners. Failure restores execution and lexical bindings; success keeps
objects alive through nested GC and releases temporary roots after restoration.
Fatal host failures propagate through both public evaluate paths; catchable
JavaScript exceptions retain their opaque TypeError translation.

Temporary execution-root vectors admit full detached replacement capacity while
the old allocation remains charged. Reported capacity is leased and checked before
moving roots; refusal preserves contents and capacity. Commit moves existing
handles and publishes the retained allocation once in realm execution ownership.
Spare slots and empty appends require no new reservation. No heap operation or
JavaScript runs while preparing storage.

Promise jobs prepare roots before queue removal or fuel reset; async continuations
reserve saved roots before consuming their stored frame. Module/async/generator
and event entry restores displaced callers on refusal. Native-pressure collection,
iterator roots, callback completions, finalization holdings and yielded values use
the same preparation path. Fatal Test262 host completions stop later delivery but
release every pin in the returned host batch. Truncation retains charged capacity;
finished-execution cleanup returns it, including after terminal failure.

Safepoint root and conditional owner-edge buffers use one reusable realm cache.
Borrowed visitors enumerate execution/debugger/module/job/disposal/Promise edges
without intermediate vectors; edges are flat `(owner, reference)` pairs. Root
and edge replacement capacity is admitted jointly while old allocations coexist,
then allocator-reported capacity is leased and checked before commit or heap work.
Refusal restores the old cache without changing its capacities or VM execution.

A control lease follows the batch while Rust or the heap owns it. Heap operations
run no JavaScript, so one non-reentrant optional batch requires no metadata vector.
The heap traces results, reactions, disposable values and suspended frames only
from live owners; inverse async invocation/generator edges preserve await liveness
without rooting unreachable cycles. Cached lengths clear on every normal/error
return, while retained capacity stays charged once and can serve terminal cleanup
without new admission. Host-neutral caches attach through ordinary bounded realm
accounting; discarded realms release their caches. Cross-realm marking prepares
its batch before releasing deferred roots and propagates native preparation errors.

Realm collection metadata uses a flat admitted plan: discovery nodes/seen IDs,
edge snapshots, sorted heap/owner indices, ignored pins and reserved wrapper
targets retain one shared native lease. Detached growth admits coexisting old/new
storage and validates reported capacities. Each wrapper forwards at most once;
node active flags and borrowed target slices avoid copied per-iteration metadata.
External owners and pending work keep realms active without treating traversal
clones as independent owners.

Bounded contexts retain empty capacity without realm records or mark results.
Reuse attempts no growth; a larger plan is prepared while the old cache coexists,
and refusal restores the old empty cache and refunds all candidate storage.
Existing capacity serves terminal internal cleanup without clearing failure or
scheduling new jobs. Empty graphs and host-neutral contexts release the plan.
Borrowed-child fallback happens before allocation where directly detectable;
descendant borrowing refunds/restores discovery storage before local collection.
Metadata refusal precedes collection and realm pruning, preserving owners/pins.

Collector-produced mark/work sets, native payloads of retained values, shared
programs and other wider native/cache/integrated ownership remain open work.

Tagged-template identity belongs to a single realm cache. Suspending, restoring,
resuming or discarding a frame does not move, copy or merge that table. Native
accounting counts its capacity once as realm storage, independently of active
or displaced execution fields. The realm traces cached objects directly;
template construction creates no additional permanent root registrations.

A cache miss leases both the full detached replacement table and the complete
operand-vector replacement needed for two additional root slots. Both old
allocations coexist with staging. Requested minima are admitted before fallible
allocation; reported table capacity is checked before operand allocation, and
reported operand capacity before active backing changes. Preparation refusal
preserves both old table entries and operand backing. Spare capacity and cache
hits require no fictitious growth charge.

The independent document lease follows a Rust-held table after VM destruction.
Once all native storage fits, existing operands move into their prepared buffer
without cloning primitive payloads; the VM adopts it, staging refunds its portion
and a synchronous ownership update follows without intervening allocation or
callbacks. The detached table retains the remaining lease until publication/drop.
Native array construction runs no page code and collection keeps cache entries
strongly owned, so table length/capacity remain stable through that construction.
Publication verifies that shape, moves entries and refunds staging immediately
before reconciliation, avoiding transient duplicate charges at exact allowance.

Raw/cooked arrays are built directly with final frozen data/length descriptors,
avoiding temporary value/key enumeration buffers. Both arrays remain on the
admitted suffix until the cache adopts the cooked array and its raw edge. Later
string/heap errors restore the operand prefix and release table staging; already
adopted operand capacity remains an accounted VM cache. Async work resumed from
an older frame observes sites cached later in the same realm. Wider string/input
producers, shared site/program ownership and other native storage remain open.

Outgoing generator, async and nested-debugger snapshots jointly admit stack
suffix slots and generator-completion target slots before allocating either
buffer or moving frame contents. Actual capacities are checked before capture.
A whole-stack capture moves the existing allocation; an empty completion list
needs no new target. Conversion moves completion values into the staged vector.
Saved error messages count retained capacity in both native and managed owners.

Detached stack/completion buffers keep a scoped native lease, including their
payloads, until generator state or a suspended execution adopts them. Execution
capture immediately returns the caller's stack prefix to the active VM while
the continuation owns the suffix. Refusal follows the ordinary caller/iterator
cleanup and does not publish the refused continuation. If a parent debugger
capture fails after its child paused, terminal root cleanup discards all saved
debugger descendants and releases their uniquely owned result roots before
final collection. A paused root retains its frames and can still resume.

Call, eval and generator entry prepare capture-cell tables before constructor
entry effects. Minimum slot admission precedes reserve; reported capacity is
checked before the staged owner escapes. Prepared tables retain a native charge
during callbacks and detached handoffs, and adopted captures fill their already
reserved slots by move.

Suspended generator state now stores the owned cell and dynamic-binding maps
directly. Ordinary/parameter/async-yield capture and resume move those tables;
they preserve spare capacity, bucket allocations, name/shadowed buffers and GC
edges. Managed accounting uses actual table capacity. Resume admits the saved
tables' future native contribution before state extraction and adopts them
without rebuilding their allocations. Start-state captures still prepare a
new table because their saved input is a capture vector.

Rust-held outgoing state carries a native lease for its owned buffers through
parameter return, snapshot admission and heap store. Native refusal drops the
state, closes the abandoned frame and reconciles the shared contribution.
The heap operation releases the lease immediately after adoption/refusal,
before VM collection bookkeeping can admit native cleanup work. Terminal
Done/Running cleanup remains allowed after resource abort. Other association
copies and wider native storage remain open work.

Iterator and handler vectors carry scoped native charges through execution,
cleanup, async/module continuations and debugger pauses. New records plan and
admit capacity before reserve and publication, including actual-capacity
readmission; available slots add only the new record's owned payload. Retained
continuation records add storage outside those charges, preserving one owner.
Diagnostic throw origins keep their separate existing accounting path.

IteratorClose roots the remaining set and pops records in reverse order from
the original charged allocation. It avoids creating an uncharged split buffer
across callbacks. Managed generator suspension takes the raw vectors without
intervening JavaScript; resumption returns them to native record ownership.
Interpreter exits reconcile the document contribution after local owners drop,
including errors, so peer realms see the released storage. The iterator GC-root
vector's reported capacity is also charged and completed root scripts release
it. Other transient staging and admission before remaining active-container
allocations remain open in the broader control-storage work.

Declaration resources and disposal scope marks belong to one execution frame.
Calls/eval preserve caller ownership and root saved ephemeral disposer methods;
async/module/debugger suspension moves the records with the saved context.
Managed generator states retain both resource buffers and GC references and
restore their native ownership on resume. Interleaved frames therefore dispose
their own declarations, including sibling module awaits.

Resource/mark records and both disposal capability maps share native admission.
Appending checks capacity and payload before publication; moving a stack admits
the destination table slot before marking the source disposed. Detached disposal
records, popped resource payloads, pending errors and async entry vectors keep
their scoped charge during disposal/conversion. Roots copy object references
without duplicating primitive buffers. Native temporary storage that is wholly
within an operation's non-JavaScript staging remains part of the wider staging
gate.

Disposal side tables add conditional resource edges from each stack object.
They are not permanent roots: an unreachable stack or callback cycle can be
collected without running a disposer. Collection prunes its rows, releases
mostly empty capacity and reconciles shared ownership before peer admission.

### Native-payload collection pressure

Large Promise results and suspended-frame strings enter the native scheduling
ledger independently of the managed heap's collection trigger. A timer can
create and discard many such values before its checkpoint. Rooted call entry
and return boundaries therefore give unreachable native Promise state a major
collection opportunity before new pressure exhausts the document account.
Returned objects are protected until the caller takes ownership; callback
arguments, live reactions, awaits and current-job WeakRef targets retain their
ordinary tracing and keep-alive guarantees. Collection runs no queued callback
or nested task turn.

The collection watermark grows with retained live bytes, bounded by the
existing scheduling limit, so a large live owner does not trigger collection
at every small call. Primitive arguments and results can prompt earlier
collection. The 8 MiB hard limit and terminal document failure are unchanged;
live queued work is not discarded to make admission succeed. Navigation and
close still release the owning queues and reset the new realm's watermark.

### ShadowRealm ownership

Explicit script, call-pressure and checkpoint collections trace the realm
ownership graph. Live instances and wrapped functions retain their child VM;
queued jobs, cleanup tasks, host waits and module work retain their owning VM.
The child's globals become roots only after an actual owner makes that realm
live. A discarded instance alone therefore cannot leave global Promise results
charged forever, and unreachable wrapper cycles across heaps can be reclaimed.

Foreign callable roots use leases whose release is queued in the target heap.
Agent tracing treats those pins as conditional edges from live wrappers, then
sweeps before JavaScript can observe WeakRefs. It runs no callbacks. Active
cross-realm borrows defer agent reclamation to an outer safepoint; allocation
safepoints continue to protect their own Rust operation inputs. Independent
Test262 membrane owners have distinct ownership tokens; temporary scheduler
and call snapshots do not create owners. A retained wrapped callback keeps
the same realm even after its original instance is collected. Navigation and
close discard the document's realm tree and queued work together.

This closes discarded-realm retention within the existing scheduling ledger.
Complete native control-table/service-buffer accounting remains part of the
broader resource-containment work.

Collector work queues, mark sets and conditional-edge waiters share the native
scheduling account. Each heap keeps admitted reusable capacity. Detached growth
keeps old and replacement storage charged together, checks reported capacities
before publication and preserves the old cache on refusal. Marking visits borrowed
object references directly; enqueue-time deduplication bounds the queue by the
heap's object count. Flat waiter links cover Promise side-table edges, weak-table
ephemerons and observed dependent abort signals. Minor collection borrows the
ordered nursery index instead of copying it into a set.

Returned realm mark sets carry their own native lease until recycled or dropped,
including while Rust holds them outside the heap. Realm traversal recycles each
previous result before another pass and returns final results before local sweeps.
Cold or growing collector storage can fail before marking or sweeping; explicit
fallible collection and automatic allocation propagate that failure as the
document's noncatchable scheduling limit. Existing void collection hooks preserve
their signature and report failures at the VM safepoint. Warm capacity remains
reusable during terminal cleanup. Remaining managed-container backing buffers,
retained native values, shared programs and service buffers remain open work.

Cached module completions and evaluation errors now retain separate native
owners after their execution frame ends. Retention admits the owner headers and
primitive copy before publication, checks reported copied capacity, and moves
the copy's existing lease into immutable storage. Async parents share the same
error value and owned origin buffers. Cloning that cached error adds no value
copy or native charge; delivering an ordinary JavaScript value keeps the usual
root and scheduling rules.

An error's weak discovery record does not root its JavaScript value. Once its
last strong owner drops, primitive and origin buffers are released before their
native charge. The discovery record keeps the remaining allocation headers
charged until pruning. Values held by Rust can outlive their VM without an early
refund. Existing host-neutral caches attach to a document once on activation;
foreign-document attachment fails before replacing the original owner. Full
cleanup remains possible after terminal scheduling failure. Shared programs,
other native values, managed-container backing buffers and wider service/native
acceptance remain open.

Pending page-module reports prepare a detached snapshot before clearing any
delivery flags. Snapshot slots and copied canonical names retain a native lease;
all object values have independent registered roots before the first report can
enter user code. Reentrant reporting therefore skips the already-selected batch,
and releasing the original graph during a report cannot reclaim later values.

Preparation refusal preserves pending flags and original roots, rolls back any
new registrations and refunds snapshot storage. Once reporting begins, an
attempted report remains consumed. A failure restores only the unattempted
suffix whose canonical name and immutable error owner still match the current
record; removed or replaced errors are not revived. Newly pending work remains
pending for a later pass. Success and failure both release the batch's roots and
storage, including after terminal document failure. Empty/disabled reporting
needs no snapshot allocation. Broader report/event scratch buffers, shared
programs and integrated acceptance remain open.

### Document managed heaps

Persistent root registries retain a separate native owner for their backing
hash-table slots. Bootstrap roots, collector caches and foreign pins attach to
the same document together; a failed attachment preserves live registrations and refunds
candidate charges. Root growth prepares a complete replacement while old storage
remains charged, checks reported capacity, then moves entries before publishing
the next root ID. Refusal preserves entries, counters and the next identifier.

An append that fits current reported spare capacity needs no new native storage.
Deletion can change reported hash capacity without freeing its allocation, so
the retained charge follows the allocation's original admitted capacity until
replacement or release. Removing the final root drops the empty table and
refunds its storage; released realm pins use the same removal path. These
releases remain available after terminal failure. Native root-storage refusal
becomes a noncatchable scheduling error; infallible rejection tracking latches
the document failure instead of panicking or publishing an unrooted notification.

Foreign callable pins retain admitted shared native headers and a per-heap
registry of strong references. Slot replacement is prepared alongside the pin
header before root publication; refusal preserves registrations and frees all
candidate storage. Dropping a facade/handle only decreases its reference count.
At collection, a pin with only the registry's reference is released: marking
skips its root, and cleanup removes the registration and refunds its header.
The sorted monotonic IDs support borrowed lookup without deferred queue growth.
Live pins preserve their slots, the final removal drops the registry allocation,
and terminal cleanup needs no allocation or admission. A Rust-held handle can
outlive its heap while keeping its own native header charged. Late activation
binds existing headers and slots in the same root/collector transaction.

Other heap tables, shared programs and wider native/integrated acceptance remain
open.

Document scopes share one managed-storage envelope across the root VM and all
ShadowRealms, including nested realms. The immutable root heap limit also
bounds the document total. Each heap's storage counter joins that account once;
multiple instances or wrappers retaining the same VM do not duplicate its
charge. Host-neutral VMs without a document scope retain their per-heap policy.

Allocation and growing properties, collection entries, strings and buffers
check the shared account before committing storage. Minor/major collection,
deletion, cleanup, buffer shrinking and heap destruction release that heap's
actual delta. Page runtime stats keep root collector diagnostics in `heap` and
expose the aggregate as `document_heap_bytes`; private child-host `heap_bytes`
and child-wide actual totals use that aggregate. The existing heap reservation
remains separate from retained page-cache metadata/scheduling payloads.

Rooted call-pressure safepoints collect managed garbage before admission;
their adaptive watermark avoids repeated collection of unchanged live data.
Page realm entry also collects the caller before borrowing its child, allowing
dead caller storage to be released before large child growth. Allocating heap
operations retain their existing protected-input rules. Collection runs no
JavaScript or nested task turn and retains queued/running owners.

An exhausted document account stays failed across later script, task and
checkpoint entries. Another tab has its own account; navigation creates a new
one and drops the old realm tree. This is a managed-storage budget, with the
existing per-value string and native scheduling bounds still separate. Native
control/service payload accounting remains in the open resource work.

### Shared agent registries

The agent's `Symbol.for` registry holds one charge in the document's existing
8 MiB native account, shared with Promise results, timer arguments and other
scheduling payloads. Root and nested ShadowRealms share both the registry and
its charge. Collecting a child VM preserves registered identity and does not
refund storage still owned by the agent. Navigation and close release the
document's registry together with its realm tree.

Committed bytes include table entry capacity and the actual UTF-16 capacities
of stored keys and symbol descriptions. Repeated keys reuse the entry without
adding a charge. Table growth is prepared separately; admission succeeds
before the live table or registry entries change. Refusal leaves committed
bytes unchanged and makes the document's resource failure terminal. Key
coercion completes before borrowing the registry, allowing reentrant calls.

Enabling document policy on an existing host-neutral VM first admits its
retained registry and scheduling storage. Oversized storage is refused before
publishing rejection-event bindings or changing that VM's host-neutral policy.
Standalone VMs without an owner-selected document scope retain unbounded
registry semantics. Runtime/Core native totals include the shared charge;
the managed-heap and private transport layouts are unchanged. This accounts
for retained registry storage, while allocator/control/service overhead
remains outside this ledger.

### Active execution payloads

Active operand stacks, local bindings, arguments, receivers and completion
records own primitive buffers outside the managed heap. Those buffers share
the document's native account with Promise, timer and agent-registry storage.
Interpreter and native-call safepoints synchronize changed execution payloads;
unchanged payloads avoid scanning the remaining scheduling owners repeatedly.
Exhaustion stops the document before another user callback or instruction.

A pause is also an ownership handoff. Before returning it to the host, the
interpreter admits payload produced by the preceding instruction. A refused
pause unwinds the frame and leaves the document failed. This check does not
spend fuel for the paused instruction; single-step and resume keep the same
remaining instruction budget.

Replacing a caller's fields transfers their payload to a scoped native lease.
Normal return, throw and entry refusal release it. Suspended async/module and
debugger contexts carry a lease until restoration or destruction; their normal
continuation charge adds slots and metadata without charging those buffers a
second time. Values promoted into managed cells retain managed ownership.
Root and child scopes share the same account, and navigation drops old leases.

Call-entry buffers keep their lease while a constructor's prototype getter or
base-class field initializer runs, before its arguments enter VM fields.
The entry lease carries primitive payload and actual argument-vector capacity
separately. Closure captures and copied with-environments join its native
control charge before getters or initializers run; metadata inspection borrows
the closure instead of cloning its environment. Replacing a primitive receiver
releases that original value before refreshing the entry charge.

Bound dispatch retains an admitted vector of wrapper IDs, then admits and
builds one final argument buffer. It releases wrapper metadata before executing
the target. Host callbacks retain an independently admitted conversion vector
and any copied strings for their entire callback. Tail calls reuse the carrier
allocation after removing its callee/receiver prefix.

Ordinary and generator parameter initialization release that lease only after
the VM takes ownership. Direct eval retains its source-argument lease in normal,
spread and tail-call positions. Proxy trap getters run under the incoming
lease; forwarding the same buffers transfers ownership rather than charging a
second copy. A trapped call releases the incoming lease once its argument array
owns those values and the trap invocation takes over.

Generator parameter entry keeps the original inputs in native ownership and
uses a rooted running state instead of cloning them into a temporary start
state. Saved generator states charge their fixed box, actual capacity of each
retained vector and owned payloads. Their GC reference lists describe
reachability rather than the size of those allocations. Native-to-managed
handoffs therefore preserve the storage charge, including spare slots.

Task callbacks use the same transfer rule. Timer arguments, drag text and
finalization holdings carry an input lease until call dispatch takes ownership.
The task's roots and control storage remain accounted separately. A live
interval registration still owns its retained argument copy; one-shot inputs
and cleared interval copies do not remain charged through the callback's
Promise checkpoint. Cleanup consumes ignored returns and reported page
exceptions before checkpoint jobs run.

Promise jobs likewise carry a scoped payload lease while outside the queue.
Reaction inputs transfer to handlers, resumed frames or settled Promise
records. Their targets and returned/thrown values keep a separate charge
while still owned by Rust. Species resolution roots object edges without
copying primitive results solely for GC protection. Dynamic import retains
its borrowed referrer/specifier strings through module execution. Generator
yield values release their native copy after the iterator-result property
owns it, before a successor request runs.

Host-completion tasks transfer their lease after primitive conversion or
wrapper/error creation and before the Promise takes ownership. Restoring an
execution context always reconciles scheduling owners, including removed
continuations, even when primitive byte totals are unchanged. Ordinary
instruction/call checks retain their unchanged-payload fast path.

GC protection retains object edges without cloning primitive buffers. Normal
calls move primitive operands to their argument owner; exact debugger calls
preserve the operand snapshots needed for continuation. Native parameter copies
remain charged while owned. Rooted pressure collection reserves room for the
additional copies resolving algorithms can hold before settlement. Existing
large-payload fixtures reserve that active headroom while preserving their
collection, terminal-failure and liveness assertions.

This is a retained primitive-payload budget at execution safepoints. Active
control-table/vector capacity, allocator overhead and temporary buffers wholly
inside one native operation remain in the broader native-accounting work.
Standalone VMs keep their host-neutral policy.

Completion records belong to their execution context. Ordinary/async calls
and generator parameter initialization move the caller's pending completions
aside, keep their object edges rooted and restore them on every exit. Async
return records cannot accumulate in the caller, and a child await cannot
capture a caller's pending `finally` return. Cleanup reconciles released bytes
even after failure, while leaving the document's failure latched.

The legacy browser-only entry uses the same logical source selection while
leaving callback timers to its separate timer entry. Standalone browser-task
embeddings retain their existing bounded delivery API.

## UI and networking integration boundary

Core currently validates and services native user input between child task
polls; the one-task boundary gives that ingress and rendering an opportunity
before another queued child task. Native dispatch remains one event task,
with M1h's callback cleanup inside it. No nested task turn may be run from a
native listener or a microtask checkpoint.

M1k provides [opaque Promise waits and queued service completions](HOST_ASYNC_OPERATIONS.md).
Real Fetch/XHR completions must enter as document-owned networking tasks after
typed request-service validation. They must carry the exact document identity,
use the initiating tab's scheduling account, preserve networking-source FIFO,
and join the source rotation instead of invoking JavaScript during a request
service callback. Promise settlement belongs to that task and its following
checkpoint. UI input queued inside the child must similarly retain a distinct
user-interaction source; a drag string callback is not a native input task.
The completion mechanism installs no service global by itself. Full typed
request-service IPC and Core's native UI source association remain open.

The TODO's complete timer/network/UI ordering and host-wait-hook requirements
remain open until those real ingress paths and their acceptance fixtures exist.

## Acceptance

The initial public regressions failed on M1i: a later abort timeout ran before
an earlier callback timer, and advancing the page clock did not advance its
abort deadline. Focused coverage also exercises equal deadlines across polls,
late deadlines, injected clocks, registration inside callbacks, interval
rearming, independent file/drag sources, file-stage FIFO, and shared-source
ordering and wake ownership across same-document realms.

The real JavaScript/BlueTS fixture mixes callback timers and an abort timeout,
requiring three unsolicited distinct PNG frames and matching post-microtask
Representations in order. The milestone record tracks final acceptance gates.
