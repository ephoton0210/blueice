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
The 64 pending browser/cleanup task cap, 1,024 timer cap, microtask/reaction
caps and document resource-failure behavior are unchanged. Replacement and
close drop the owning queues and reset source selection with the realm.

The legacy browser-only entry uses the same logical source selection while
leaving callback timers to its separate timer entry. Standalone browser-task
embeddings retain their existing bounded delivery API.

## UI and networking integration boundary

Core currently validates and services native user input between child task
polls; the one-task boundary gives that ingress and rendering an opportunity
before another queued child task. Native dispatch remains one event task,
with M1h's callback cleanup inside it. No nested task turn may be run from a
native listener or a microtask checkpoint.

Fetch/XHR completions must enter as document-owned networking tasks after
typed request-service validation. They must carry the exact document identity,
use the initiating tab's scheduling account, preserve networking-source FIFO,
and join the source rotation instead of invoking JavaScript during a request
service callback. Promise settlement belongs to that task and its following
checkpoint. UI input queued inside the child must similarly retain a distinct
user-interaction source; a drag string callback is not a native input task.
Neither unimplemented ingress has a synthetic queue or script-visible global.

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
