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

Refusing a registration above the document limit latches terminal scheduling
failure; navigation creates a fresh account. Host-neutral VMs retain their
1,024-per-realm catchable `RangeError` policy. Abort-timeout browser entries
remain in the separate 64-task browser/cleanup cap; microtask/reaction caps
are unchanged. Replacement and close drop the owning queues and reset source
selection with the realm. These Rust count snapshots do not change the
private transport's existing byte totals or layout.

Callback-timer/frame vectors, Promise-job/browser/cleanup deques, each Promise's
pending reaction vector and the pending rejection-notification vector admit
the slot-capacity delta needed for an append before publishing it. Growth
explicitly requests at least four slots, doubling an existing full capacity.
Available slots add no storage charge. If reservation reports more capacity
than requested, admission checks that actual storage again before publishing;
a refused timer/frame releases its temporary roots. Timer argument storage
excludes the callback and delay and retains one callback root plus reserved
argument-root slots.

All browser-task producers reserve one entry before publishing task roots,
completion-queued flags or FileReader state. The ledger records actual reserved
capacity before later native allocations, even if the operation then fails.
Unhandled rejection delivery transfers its existing pending vector and reaction
count rather than admitting a second owner. A FileReader event callback can
fill the queue's spare slots or consume the shared document count; the next
read stage must readmit its entry after that callback and release its root on
refusal. FileReader cancellation removes matching entries in place, preserving
other queued work and its storage without a replacement allocation.
The broader native control/service-buffer gate remains open.

Promise-record and async/module continuation tables prepare empty replacement
storage to measure actual reported capacity before moving live entries. An
await admits its continuation payload/table delta and the matching reaction or
job count, payload and slot growth together. Work slots are reserved and
checked before the frame and serial are published; failed work admission cannot
leave an orphan frame while returning success. Existing owners move without
cloning after admission. Staging allocations, allocator overhead and broader
native control/service storage remain outside this logical capacity policy.

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
Their table/set/vector capacities and retained UTF-8 names share the document
native-byte budget. A parent's unique edge batch is admitted before either
direction or the parent's suspended state is published. Existing names move
without cloning; duplicate edges add no allocation or ordering change.

Completion returns obsolete names and unused capacity. Rejection removes the
parent from every remaining dependency, including siblings that keep running.
Breadth-first/source order is preserved. Rust-owned queues, remaining parent
iterators and current names retain scoped charges while callbacks can execute;
all exits reconcile their released ownership. Deferred graph evaluation hands
its frontier back to its caller without a realm cache and charges it across
dependency JavaScript. Collected pending dependency names and the remaining
evaluation-list buffer also retain scoped charges while a later dependency
executes. Wholly native staging, full module caches and active execution control
containers remain in the broader native-accounting work.

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

Suspended frames retain both primitive and control charges, and their container
records add only storage outside those charges. Stack splits and debugger stack
edits refresh both saved charges, including a primitive returned into a paused
parent; restore returns ownership
to the active context. Generator resume consumes its saved operand vector
instead of keeping an unused cloned copy across the body. Completed bounded
root scripts return their frame containers; persistent template identity and
module association remain charged to the realm.

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
it. Disposal/resource temporaries, call-entry staging and admission before other
active-container allocations remain open in the broader control-storage work.

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

### Document managed heaps

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
Ordinary and generator parameter initialization release that lease only after
the VM takes ownership. Direct eval retains its source-argument lease in normal,
spread and tail-call positions. Proxy trap getters run under the incoming
lease; forwarding the same buffers transfers ownership rather than charging a
second copy. A trapped call releases the incoming lease once its argument array
owns those values and the trap invocation takes over.

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
