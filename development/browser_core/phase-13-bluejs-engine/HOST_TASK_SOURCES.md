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

Animation-frame vectors, browser/cleanup deques and the pending rejection-
notification vector admit
the slot-capacity delta needed for an append before publishing it. Growth
explicitly requests at least four slots, doubling an existing full capacity.
Available slots add no storage charge. If reservation reports more capacity
than requested, admission checks that actual storage again before publishing;
a refused frame releases its temporary roots. Timer argument storage excludes
the callback and delay and uses the complete preparation protocol above.

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

Host-wait and other control tables still use their separate measured replacement/
delta protocol. Their preallocation, retained hash-allocation ownership,
producer-side clones in other Promise algorithms, allocator overhead and broader
native control/service storage remain open.

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

Closure module-referrer associations are a weak-key realm table. Its actual
table capacity and retained name capacities share the document native budget.
Heap collection removes records for collected closures without tracing those
keys as strong roots. Empty storage is returned once no in-flight closure
publication relies on its spare slots, including during terminal cleanup.

Before a Closure instruction creates prototypes/capture cells or a function,
it jointly admits the referrer copy and full detached replacement table while
the old allocation coexists. Both table and name use fallible reservation;
actual capacities are checked before heap effects. A scoped lease follows
staging until publication moves old records and the new name into the realm
table. A pending-publication guard preserves any source-table slots that GC
would otherwise release; interpreter unwind releases the unused guard/storage.

Calls admit and copy an associated referrer before consuming an instruction,
adding call roots or displacing caller context. The copy stays leased until
active fields adopt it, while the displaced caller name has a separate lease.
Success and error restore the original allocation and release the temporary
owner. No association takes the allocation-free path; an associated empty name
still replaces the caller's context. Generator resume retains its existing
pre-extraction referrer preparation. Other module-name/code/cache/service copies
remain separate open work.

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

Compiled registries retain immutable snapshots with one document charge for
their owned header, logical table capacity, key capacities and root code-unit
buffers. Root vectors include diagnostic instruction owners; constant payloads
use the existing Value charge model. Shared child code, SourceText and
diagnostic maps are excluded from this root-owner count and remain a separate
program-ownership gate. Snapshot references share buffers and final drop
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

A cache miss jointly admits the full detached replacement table and two operand
root slots before allocation; reported capacities are checked before heap
effects. Existing slots and cache hits require no fictitious table-growth charge.
The staged table stays leased until successful publication; refusal leaves its
prior entries intact. Raw/cooked arrays are built directly with their final
frozen data/length descriptors, avoiding temporary value vectors and property-
key enumeration buffers. Both arrays stay on the admitted stack suffix until
the realm cache adopts the cooked array and its raw edge. Array creation errors
restore the operand prefix and release staging ownership. Async work resumed
from an older frame observes sites cached later in the same realm.

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
