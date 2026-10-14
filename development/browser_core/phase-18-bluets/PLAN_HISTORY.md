# Phase 18 — delivered-status history (archive)

[← Plan](PLAN.md) · [worklist](TODO.md)

Archived verbatim from the head of PLAN.md on 2026-09-29. It records what each
slice delivered and its exact boundary; it is history, not the current design.

**First DOM/event binding decision:** Phase 13 now fixes the initial
[page DOM/event boundary](../phase-13-bluejs-engine/DOM_EVENT_BINDINGS.md),
including the core-owned DOM, child-owned wrappers and click callbacks,
authenticated generation-bound script calls, session scheduling, capability
policy, and real-page acceptance order. The design checklist is complete;
runtime binding, event delivery, and Phase 18's P0 host-binding acceptance
remain open.

**Reentrant page-host wait (A2.1):** Core now prepares each authorized child
document from an immutable page borrow, releases that borrow, and waits for
the child reply while its owning session thread dispatches bounded DOM calls
for only the executing tab and document generation. The same nested route
serves debugger-controlled resume; a transport-only reader handles the
page-host socket and never owns `TabManager`. Wrong-target calls receive an
error without mutation, and unrelated frontend, debugger, and compiler
messages are not dispatched in the nested wait. Socket-pair, executor,
debugger-resume, and dispatcher tests cover this scheduling boundary. The
child VM's general live DOM binding remains open; this is not yet an interactive
page-script claim.

**Nested-wait limits (A2.2):** Every child document synchronization and
debugger-controlled resume now shares one fixed 60-second deadline across
the page-host request write and reply read, and can dispatch at most 1,024
DOM requests in total (still at most 64 per polling turn). The first request
beyond that allowance receives a structured error before mutation and aborts
the wait. Timeout, disconnect, and abort poison only that child transport;
core does not dispatch unrelated frontend, debugger, or compiler traffic in
the nested wait. Socket-pair regressions cover a stalled peer, prompt
disconnect, and post-timeout unusability; dispatcher regressions cover the
total budget and unchanged DOM.

**Real child DOM lookup proof (A2.3):** A distinct trusted-embedding fixture
option now passes the launcher's per-core private script socket and existing
64-hex child capability to the supervised child. The child lazily performs
script IPC v3 `Hello` and sends one `GetElementById` bound to the executing
tab/document generation. Only this owner-selected proof profile installs the
JavaScript `blueiceTestHasElementById(id)` callback; it returns a boolean and
never exposes a raw core node ID, socket path, or capability to page code.
The copied-snapshot BlueTS typing inventory remains unchanged and does not
advertise this probe. A real launcher/core/child HTTP page checks both a
present and absent ID and then observes two completed scripts without a
session deadlock. The ordinary child profile continues to have no live DOM
binding. This proves the transport/scheduling mechanism, not the eventual
`document` node-wrapper, mutation, or event API.

**Bound script calls (A3.1):** Script IPC v4 wraps every post-handshake DOM
operation in a monotonically numbered call. Core accepts exactly one envelope,
routes only an inner operation with an explicit tab/document-generation target,
and echoes both the call ID and target in its result. The child accepts only a
matching result, drops the stream on malformed or mismatched replies, and
returns only a boolean to the owner-selected JavaScript lookup probe. A real
core socket rejects malformed envelopes; a socket-pair child test rejects
wrong IDs, wrong documents, naked results, and nested results. The supervised
HTTP fixture also asserts this realm has no `fetch` or direct `document`.
There is no child resolver or direct core DOM reference; page source and graph
selection remain with the parent. Lifecycle revocation is proven in A3.2 below.

**Script-route lifecycle (A3.2):** The DOM client is owned by a callback in
the child VM, so realm navigation/reload and exact-generation `CloseRealm`
drop the old socket rather than retargeting it. A child socket regression
observes EOF on both transitions, then sends a predecessor document reply
with the successor's reused call ID: the child rejects it before page code
can see a node and reconnects for a valid successor call. The existing real
core-socket regression rejects predecessor capabilities after path reuse;
the real broker cutover regression now also checks that v1's private script
listener is unlinked and v2 has a distinct listener, alongside replacement
and reaping of the private child. Thus no old child stream or reply is
transferred to the successor core. VM-owned node-wrapper identity is B1.1 below.

**Child-owned node identity (B1.1):** A separate BlueJS host-object factory
preserves the primitive-only `HostValue` callback ABI. An owner-only lookup
callback returns an exact `(tab, document generation, node)` private key;
the VM allocates an ordinary wrapper with a private family prototype, roots
both prototype and wrapper in its heap, and reuses that object for repeated
keys in the same realm. The family is bounded to 4,096 wrappers and dies with
the VM. The raw numeric node ID remains within core/child internals and their
private script IPC; neither it nor the child key becomes a JavaScript
property, argument, or return value. The boolean and wrapper probes share one
authenticated child DOM stream to respect the core listener's long-lived
connection. VM tests force major GC and verify identity, hidden key, null on
miss, and cross-realm rejection; the real HTTP fixture verifies the same
page-visible wrapper behavior through launcher/core/child. The owner-only
probe is not a general `document` binding or BlueTS declaration. Stale-node
validation is B1.2 below; general DOM methods remain B2 work.

**Wrapper liveness (B1.2):** Script IPC v5 introduces an exact-document
`ValidateNode` call with only an acknowledgement or structured error. Core
uses its owned document node table, so newly created detached nodes are
valid while nodes reclaimed by subtree removal are not; replacement and
closed documents are rejected before node resolution. The BlueJS VM now
installs host-object methods on a family-private prototype and resolves the
call receiver through its reverse identity table, preventing ordinary,
prototype-forged, or other-family objects from entering a host callback.
Only the child-private key reaches that callback; JavaScript still cannot
read or provide numeric node IDs. The owner-selected proof profile's
`blueiceTestRequireLive()` method asks core on every call and fails closed on
denial or malformed replies. Navigation/reload and close drop the realm, its
rooted wrappers, and its socket; a replacement never inherits the old global
or VM family. VM, core dispatcher, isolated-child socket, and real HTTP
regressions cover these seams. Ordinary pages remain snapshot-only; actual
`document` and `textContent` methods are B2.

**Live text route (B2.1 delivered):** A separate trusted launcher option
selects a bounded typed child profile. It installs a realm-owned `document`
object whose receiver-checked `getElementById` returns the same VM-rooted
opaque wrappers, plus a native `textContent` getter/setter on the private node
prototype. Each read/write crosses the authenticated, call-ID-bound script
socket with the exact tab/document generation; core checks node existence and
relayouts after writes. A real HTTP launcher/core/child page proves two
document-order JavaScript and BlueTS writes, the final rendered DOM text, and
rejection of a wrapper whose subtree was removed by replacement. The profile grants no
`createElement`, fetch, raw node ID, socket, or resolver to page code.
Ordinary realms retain copied snapshots. BlueTS now parses bounded interface
method signatures and checks direct member calls and text assignments; the
direct bridge erases postfix non-null assertions without a JavaScript-text
round trip. Core and child derive an exact owner-selected declaration,
manifest, and runtime inventory from the same schema. A real HTTP page proves
the typed getter/setter path, an invalid call fails before execution, and the
ordinary snapshot profile still rejects `document` calls. B2.2/B3 and the
broader DOM/event typing publication remain separate later work.

**DOM creation and append (B2.2 complete):** BlueJS's private two-wrapper
method resolves an exact receiver and exact child in one realm-local family,
then returns the original child object only after its host callback succeeds.
The launcher-selected `core-script-dom-mutation-v1` profile now binds
`document.createElement`, `document.createTextNode`, and `node.appendChild` to
the generation-bound child/core script socket. Only opaque keys enter callbacks;
the child checks both owners/generations before core checks node liveness and
tree validity. The separate text-v1 profile remains immutable. Core recomputes
author/UA styles when a new element joins the tree, then relayouts. A real
HTTP launcher/core/child page executes JavaScript and checked BlueTS mutation,
rejects a forged child and an invalid direct BlueTS append call, confirms the
live subtree, and compares its RGBA frame against the same empty page to prove
visible rasterization. VM regressions reject foreign-family and same-family
foreign-generation wrappers. Script IPC v6 also replaces raw per-document
NodeIds with core-minted process-unique child handles that resolve only in the
original document; the handle map is discarded on navigation. Two real HTTP
pages in one core prove that equal internal node counters cannot cause a
foreign detached child to attach in the other tab. The authenticated script
route rejects the append, both live DOMs remain unchanged, and the local
detached child remains valid. Ordinary page realms cannot transfer wrappers
between their separate VMs, so the socket test exercises the lower boundary
where such a foreign handle could otherwise alias. Page-host v34 converts a
core hit-tested NodeId into the same document-bound handle before click
dispatch, preserving exact listener identity; a real JavaScript/BlueTS click
regression covers that handoff. BlueTS recursively infers
chained call-result receivers and validates their method arguments; the
real-page rejected script exercises this path before execution.

**Click delivery and event profile (B3 complete for event-v1):** The BlueJS
host-object family roots, deduplicates, caps, and removes exact-wrapper
`click` callbacks without passing a function or callback handle through
primitive host callbacks or IPC. The launcher now selects a separate event-v1
profile, installs its exact 10-binding inventory, and keeps the node family
with the exact child document. Page-host v33 accepts only a core-hit-tested
node and returns a cancellation bit after a generation-bound, one-slot child
task dispatch. The child runs at most 256 Promise jobs in the click's
microtask checkpoint before replying; ordinary listener exceptions do not
lose an earlier `preventDefault()` or skip later listeners. Resource failures
fail closed. Core serves same-document DOM calls during that wait and applies
link navigation only when `preventDefault()` did not run. A real process test
exercises coordinate Click and ActOn with JavaScript and BlueTS listeners,
microtask DOM mutation before the default action, and canceled navigation.
Child tests prove removed and old-document callbacks do not run, queued
old-generation tasks are discarded, and the queue limit is enforced. The
BlueTS checker parses bounded callback function types and distinguishes
string literals for exact `click` typing; the older mutation-v1 artifact
remains unchanged. This is a synchronous first-event task boundary, not a
general-purpose browser event loop.

**Event `readonly` static enforcement (B4 partial):** BlueTS now retains
`readonly` on interface and record members through generic substitution and
inherited-property lookup, and emitted declarations preserve it. The bounded
checker rejects direct dot-property assignment, compound assignment, update,
and deletion. Exact unescaped string-key computed writes receive the same
field check, while dynamic and escaped keys on a receiver with readonly
members fail closed after bounded alias/inheritance expansion. The verified
event-v1 ambient profile proves that writes to `type`, `target`, and
`currentTarget` fail as BlueTS diagnostics before the direct BlueJS bridge
runs. The child event object already has non-writable, non-configurable
descriptors. At this initial slice, complex or chained receiver writes still
lay outside the bounded static check.

**Chained event receiver checking (B4 partial):** The bounded checker now
resolves the final property against the inferred receiver rather than only
the first identifier. Parenthesized receivers, dot and exact static-bracket
chains, and direct-call results retain the event type through a write; the
same resolver supplies nested read inference. Existing numeric update,
arithmetic, and unary `typeof` inference remain intact. Event-v1 bridge
regressions verify rejection as BlueTS diagnostics before lowering. Dynamic
receiver keys and mutations nested in a larger expression still needed their
own bounded analysis at this slice.

**Nested event mutation preflight (B4 partial):** A separate bounded lexical
scan now checks mutation operators inside call arguments, assignment chains,
arithmetic and boolean expressions, and prefix/postfix update or delete forms.
It resolves the adjacent member target against the same readonly property
lookup and fails closed for dynamic keys on known readonly owners. Distinct
operator and token-window limits bound scan work without consuming generic
expansion fuel for plain assignments. The direct single-member path still
checks writable-property value types; the scan adds readonly protection for
nested forms. Tests also keep a method named `delete` from being mistaken for
the delete operator. Dynamic receiver chains and opaque/unmodeled expression
forms still prevent claiming full B4 qualifier enforcement.

**Computed receiver element typing (B4 partial):** The checker now carries a
typed array element through a dynamic bracket receiver before testing a later
readonly event field, including arrays reached through a type alias and
mutations nested inside calls. Uniform tuple and record fields use the same
conservative inference; canonical numeric literal keys select the exact
element of a heterogeneous tuple, and a mutable field remains writable. The
verified event-v1 direct bridge rejects array-indexed writes to `type` and
`currentTarget` as BlueTS diagnostics before BlueJS execution. At this slice,
heterogeneous dynamic containers and opaque expressions still needed a sound
fail-closed policy before B4's qualifier leaf could be marked complete.

**Heterogeneous computed readonly receivers (B4 partial):** A separate
readonly-only candidate walk now expands typed heterogeneous records, tuples,
array unions, named aliases, and subsequent member chains under a fixed
type-expansion budget. It rejects a mutation when any reachable branch has a
readonly event property, including a mutation nested inside a call, while a
mutable-only branch remains writable. Exhaustion emits a resource diagnostic
instead of treating the receiver as safely writable. The event-v1 direct
bridge proves a heterogeneous indexed event write fails during BlueTS checking.
This walk does not change ordinary expression inference or resolve opaque and
unmodeled receiver forms; B4.2 remains open.

**Readonly through inferred aliases (B4 partial):** A heterogeneous computed
index now infers a conservative union instead of discarding its possible
value types when stored in an unannotated local. Bounded union property lookup
requires each concrete branch to own the property and merges its readonly
qualifier, so `const selected = slots[key]; selected.type = ...` is rejected
even when one branch declares a mutable `type`; common mutable fields remain
writable. The verified event-v1 direct bridge checks the same alias boundary.
Property lookup and index inference now live in separate MPL-licensed modules,
keeping both `checker.rs` and `expressions.rs` under 1500 lines. Opaque or
unmodeled expression shapes remain outside this proof, so B4.2 stays open.

**Readonly through literal-held values (B4 partial):** Array and record
literal inference now uses the supported full member/call expression for each
value rather than its first identifier. A depth-aware record value scanner
retains nested objects and calls with comma-separated arguments. This keeps an
Event's readonly `type` when it passes through `[holder.event][0]`,
`{ picked: holder.event }`, a nested record, or a typed call result; the
verified event-v1 bridge rejects the array and record cases before runtime.
The new recursive inference stops at 128 bracket/record containers and emits
a resource diagnostic on excess input. Opaque expression forms remain open.

**Readonly through erased assertions (B4 partial):** The checker now infers
the operand of a supported `as` or `satisfies` expression before it reaches
the generic identifier fallback. Because both forms erase from the emitted
JavaScript, neither can turn a known event receiver into a mutable one for
readonly checking, even when `as` names a writable-looking record type.
Regression tests cover direct, alias, array, and record paths plus a mutable
control; the verified event-v1 bridge rejects an asserted event alias before
runtime. The checker tests were split into readonly and member-call modules
to keep the parent test file below 1500 lines. This is a qualifier-preservation
rule, not a claim of complete assertion compatibility or coverage of opaque
expression forms.

**Readonly through sequence results (B4 partial):** The supported JavaScript
sequence expression now infers its rightmost operand, matching the value the
VM actually produces. A preceding mutable receiver can no longer disguise a
later readonly event through `(holder, source.event)`, whether the result is
written directly, held in a local or array, or follows a call with its own
comma-separated arguments. A reversed sequence ending in a mutable receiver
remains writable. The verified event-v1 bridge rejects the readonly alias
before execution. Other unmodeled expression shapes remain open.

**Readonly through simple assignment results (B4 partial):** A supported
simple `=` expression now infers the right-hand value that the VM returns,
not the type of a mutable object appearing at the start of its left-hand
target. This retains an event's readonly `type` through direct, local,
record, array, and chained assignment results. A genuinely mutable
right-hand result remains writable. The verified event-v1 bridge rejects an
assignment-result alias before runtime. Compound and logical assignments,
and other unmodeled expression shapes, remain outside this proof.

**Readonly through logical assignment results (B4 partial):** `&&=` and
`||=` now conservatively merge the known type before the write with the
right-hand value, while `??=` uses the existing nullish-exclusion merge.
When either reachable result is a readonly event, a later `type` write is
rejected; a purely mutable result stays writable. The verified event-v1
bridge rejects a logical-assignment alias before runtime. Assignment-result
inference has a fixed 128-logical-operator resource boundary with a diagnostic
on the first excess operator; existing simple-assignment scan limits remain
unchanged. The expression inference method moved to its own
MPL-licensed module, leaving the parent expression checker below 1500 lines.
Arithmetic/bitwise compound results and opaque expression forms remain open.

**Readonly through logical expression results (B4 partial):** Non-boolean
`&&` and `||` now conservatively merge their known operand types instead
of erasing both to `unknown`; the existing two-boolean result stays boolean.
This carries an event's readonly qualifier through aliases and direct
receivers when either reachable result is that event, while mutable-only
results remain writable. The verified event-v1 bridge rejects an `||` alias
before runtime. General `&&`, `||`, and `??` inference has its own fixed
128-operator resource boundary, separate from logical-assignment accounting.
Unknown operands and other opaque expression forms still require a sound
qualifier policy, so B4.2 remains open.

**Readonly through tuple spread results (B4 partial):** Inferred array
literals now resolve spread elements through the indexed-value logic used
for computed receivers, with the existing named-type expansion budget. A
tuple spread, including one named by a type alias, retains every possible
element type instead of becoming
`unknown`; a following indexed alias or direct receiver therefore cannot
discard a reachable event's readonly fields. Mutable-only tuple spreads stay
writable. This ordinary array inference merges tuple element possibilities
rather than preserving their exact positions. The verified event-v1 bridge
rejects the readonly alias before runtime. The array inference implementation
moved into a small MPL-licensed module, leaving the checker below 1500 lines.
Other opaque result forms and unknown spread sources remain open.

**Readonly through record spread results (B4 partial):** Inferred record
literals now parse a complete spread source expression and expand known
interface/type aliases to their record fields. A copied holder therefore
retains the readonly event type in nested field values, including when the
spread comes from a member expression, typed call, or nested literal. Object
spread makes the new outer properties writable, and later explicit fields
override spread fields; neither operation erases a nested event's readonly
qualifier. The verified event-v1 bridge rejects a copied holder's event
write before runtime. Record-result inference moved to its own MPL-licensed
module, leaving the parent checker below 1500 lines. Opaque or non-record
spread sources still need a conservative qualifier policy, so B4.2 remains
open.

**Readonly through union record spreads (B4 partial):** A record spread from
a known union of record/interface branches now joins each shared field's
possible value types and marks a field optional when some branches omit it.
A later write through the copied event therefore sees any reachable readonly
event branch. An optional later spread cannot erase a previously present
readonly event; a definite explicit field or required spread still overrides
it. Mutable-only union spreads remain writable. The verified event-v1 bridge
rejects a readonly union-spread alias before runtime. Exhausting the
type-expansion budget emits a resource diagnostic rather than allowing an
`unknown` alias to pass.

**Fail-closed opaque record spreads (B4 partial):** In checked mode, a spread
source that cannot be proven to have a supported record shape now reports a
type diagnostic, including `any`, `unknown`, and non-record union branches.
This deliberately narrows dynamic object spread in the checked subset;
transpile-only mode remains unchanged. The verified event-v1 bridge rejects
an opaque holder spread before runtime. Other opaque expression forms still
use the general fail-closed receiver policy below, so B4.2 remains open.

**Opaque readonly receiver policy (B4.2.1–B4.2.2):** A write through an
unmodeled receiver (`Unknown`) fails in checked mode when a readonly-bearing
binding is in scope. The scan is bounded by type-expansion fuel and reports a
resource diagnostic on exhaustion. A declared `any` receiver remains an
explicit escape, and an opaque receiver in a scope without readonly-bearing
bindings remains allowed. Through the verified event-v1 public bridge, a
registered click callback writing `pick(event).type = 'click'` receives a
BlueTS diagnostic before an executable script is produced. This is a
conservative scope-level policy, not proof that every unmodeled receiver
actually aliases the event; the remaining limits are recorded below.

**B4.2.3 finite acceptance and limitations:** The delivered readonly guarantee
covers the expression forms listed in TODO.md B4.2, plus the bounded
fail-closed rule for an unmodeled (`Unknown`) write receiver. It is not a
claim of general TypeScript expression or data-flow support:

- Arithmetic and bitwise compound *targets* receive the readonly mutation
  check, but their result types are not part of the precise assignment-result
  inference implemented for `=` and `&&=`/`||=`/`??=`. Do not count an alias
  of such a result as a proven readonly-object carrier. An unmodeled result
  used as a write receiver is subject to the fail-closed rule.
- Destructuring bindings are not in the supported declaration model: variable
  names and function parameters require identifiers, not binding patterns.
  No destructured alias is in the delivered qualifier-preservation list.
- Nested closure capture has no separate lexical-capture/alias analysis.
  The verified event-v1 case is a named click callback with a typed event
  parameter; it does not establish readonly flow through an inner closure.
  Arrow expressions are outside the current direct BlueTS-to-BlueJS bridge.

The event object's runtime properties remain non-writable and
non-configurable. A new expression or binding form needs a concrete page
fixture, checker and direct-bridge evidence, and a public event-v1 regression
before it joins the accepted list; otherwise the bridge must fail closed.

**One real event-v1 BlueTS page (B4.3.1):** The supervised
launcher/core/child HTTP test in `backend/launcher/tests/spawn_core.rs` now
loads a single checked BlueTS click-handler script under the owner-selected
event-v1 profile. Its execution report confirms compilation and execution;
the callback uses `getElementById`, writes `textContent`, creates an element
and a text node, and appends both through the live DOM route. A core `GetDom`
reply after the click contains both the updated status and the created text
node. This proves the supported path through the real child; unsupported
member rejection is tracked separately in B4.3.2–B4.3.4.

**Named unsupported-member diagnostics (B4.3.2):** The verified event-v1
direct-script bridge rejects `event.stopPropagation()` inside a registered
typed click callback and `document.querySelector(...)` at the page root.
Both return BlueTS diagnostics that name the missing member. The generated
profile has no declaration for either operation.

**Catchable unsupported-member runtime calls (B4.3.3):** A real
launcher/core/child event-v1 page executes checked BlueTS functions whose
parameters are explicitly `any`. JavaScript passes the live `document` and
the core-delivered click event into those functions. Calls to the uninstalled
`querySelector` and `stopPropagation` members each throw a page-catchable
`TypeError`; the page writes status text after both catches, and the click
completes without navigation. The verified direct bridge also compiles both
`any`-parameter functions, proving this route reaches runtime.

**Paired static/runtime rejection (B4.3.4):** The same real-process test now
checks the named BlueTS diagnostics for both members against the verified
event-v1 declaration before starting the page. It compiles the exact
explicit-`any` source used by that page, with the same member-name constants,
then verifies both runtime catches and subsequent live DOM writes. This pins
the checker and child behavior together for the two unsupported operations.

**Module-root debugger decision (C1.1.1.1):** The first module pause targets
the entry program of an owner-authorized, checked ESM graph, at its first
compiler-verified root instruction in the evaluate body (at or after
`module_evaluate_entry`). Module declaration instantiation and eager dependency
evaluation may run before this boundary; the reported pause must precede the
entry body's target instruction. The graph is attached before the first
debugger advance, so its entry and dependency program generations and BlueTS
metadata can be inventoried without executing page code. The pending entry
uses the same exact tab, document generation, program, and safe-point identity
checks as classic execution; unrelated module programs cannot arm its root
pause. At the pause, BlueJS retains the linked graph, module cells, roots,
active module identity, execution context, operand/handler/iterator stacks,
and instruction budget as one realm-owned continuation. A resume finishes that
continuation before the child reports the module script completed. Reload,
close, or core cutover discards the entire continuation and rejects its old
tuple. The public state sequence is `Pending` → `Paused` → `Resuming` →
`Completed`; none of these replies exposes source text or VM values. An
asynchronous module suspension or a resource failure must produce a bounded
rejection, never a fabricated completion or a detached continuation. This
first entry-root control does not claim nested-frame or dependency-frame
stepping; those require their own exact frame identity in C1.2.

**Pending module inventory (C1.1.1.2):** Under debugger execution control,
the child attaches a checked BlueTS ESM graph and retains each program's
metadata during document synchronization. It registers the entry as a
`Pending` debugger execution and queues the already attached graph in document
order; advancing executes that exact attachment once and marks the entry
`Completed`. An isolated child subprocess test inventories both programs and
their metadata in a two-module graph before advance, then observes one
successful report and completion. A launcher/core/child HTTP test observes the
same pending entry and its verified safe points through the public debugger
socket before a later browser request advances it. This step does not yet
suspend the entry module; C1.1.1.3 adds the VM continuation.

**Exact module entry point (C1.1.1.3.1):** `BlueJsPageRuntime` now selects
the lowest compiler-verified root instruction at or after the retained
module's `module_evaluate_entry`, after checking the exact live tab/program
generation and module root shape. The selector refuses a classic handle and
an invalidated predecessor; its native regression compares the result to the
compiled entry offset and safe-point inventory. It is a location check only:
the module continuation and retained graph are C1.1.1.3.2.

**Retained module-root continuation (C1.1.1.3.2):** The native VM debugger
now accepts only the first instruction in the entry module's evaluate body.
Static graph linking and eager dependencies run before that boundary. At the
entry suspension, the VM moves the complete module execution context,
iterators, handler stack, code and program counter into a GC-visible
continuation; the linked records and their roots stay with the realm. Resume
restores that exact frame, completes the existing entry record, and creates
its namespace without re-linking or re-evaluating dependencies. A native
two-module regression inspects the paused entry/dependency state, forces a
major collection, resumes, and checks both side-effect counters remain one
even after querying the same graph again. Error and asynchronous cleanup
remain the explicit C1.1.1.3.3 gate; child/core routing remains C1.1.1.4.

**Module-root terminal and async cleanup (C1.1.1.3.3):** A paused entry rejects
other script, module-graph and Promise-job execution on the same VM. Resume
records a catchable throw on the existing linked module, keeps its object
reachable across the next graph query, and releases the debugger continuation.
When the entry awaits, its frame transfers to the normal module-await job
machinery; queued jobs settle its completion or rejection before debugger
resume returns. An async dependency is advanced only until the entry reaches
its requested first instruction, leaving all later jobs untouched during the
pause. Native regressions cover these terminal, fulfilled-await, rejected-
await and async-dependency cases. Child/core routing and page-visible state
transitions remain C1.1.1.4.

**Module route checkpoints (C1.1.1.4):** Keep the generation-bound BlueJS
page-runtime pause/resume API (C1.1.1.4.1), child document-order state machine
(C1.1.1.4.2), and real launcher/core/child BlueTS ESM acceptance
(C1.1.1.4.3) independently reviewable. No protocol widening is needed: the
existing opaque program, safe-point and execution-state requests carry the
entry identity through core to the child.

**Generation-bound page-runtime module pause (C1.1.1.4.1):** The host-neutral
BlueJS page runtime now accepts the same already-admitted module handles as
ordinary graph evaluation, plus the exact current entry program's first
evaluate-body safe point. It validates realm ownership, root location and
unique canonical module IDs before reserving the graph identities and entering
the VM. A separate resume operation advances only the retained module frame.
A page-runtime regression rejects a cross-tab handle and a different root
instruction, verifies a paused graph excludes concurrent execution, checks
resume does not replay its entry, and rejects the old handle after navigation.

**Child module execution control (C1.1.1.4.2):** The deferred BlueTS graph now
stores one optional exact entry-root arm. The child accepts only the pending
entry program's first evaluate-body safe point, leaves the declaration at the
queue head while paused, and dispatches resume to the page-runtime module
continuation. Repeated scheduler advances cannot execute a paused body; module
instruction/source-span step remains unavailable. A child test rejects another
entry instruction and the dependency program, observes `Paused` and
`Resuming` before `Completed`, and rejects the old document tuple after reload.

**Real module debugger route (C1.1.1.4.3):** A local-HTTP BlueTS ESM page now
crosses the launcher, core and isolated child while a public debugger client
with no metadata grants inventories its pending entry and compiler-verified
root points. The client arms the first accepted evaluate-body point, observes
`Pending` → `Paused` → `Resuming` → `Completed`, then reads the module's
successful source-free execution report and loaded DOM through the separate
public browser connection. The native and page-runtime tests separately pin
single evaluation and retained-frame behavior; this process test proves the
opaque protocol and scheduler route without exposing source or VM values.

**Module root-step checkpoints (C1.1.2):** Reuse the existing exact root
instruction-step and bounded BlueTS source-span-step contracts without
claiming nested or dependency frames. First retain and step one module-root
instruction in BlueJS (C1.1.2.1), then route its verified successor through
the page runtime and child queue (C1.1.2.2), apply the existing metadata-bound
source-span limit there (C1.1.2.3), and close with a public real-process test
of both modes (C1.1.2.4).

**Native module-root instruction step (C1.1.2.1):** The retained entry frame
accepts the interpreter's existing after-one-root-instruction suspension.
Each nonterminal step returns the actual verified successor PC and moves the
same module execution context back into its GC-visible continuation without
relinking the graph. A native loop regression observes backward successor
hits, terminal completion, the final page-global result, and no remaining
continuation after completion. Nested and dependency frames are still not
separately addressable.

**Page and child module instruction step (C1.1.2.2):** The page runtime maps
the VM's one-instruction module result to the existing source-free state. The
child accepts a step only for the armed, paused BlueTS entry at the queue head,
rebinds its returned PC to that same opaque program, validates the resulting
safe point against the exact live compiler inventory, and then reports the
successor. Another module in the graph cannot step; a repeated scheduler
advance while paused cannot execute another instruction. Page-runtime and
child tests cover these identity and terminal boundaries without widening the
debugger protocol or claiming nested/dependency frames.

**Bounded module BlueTS source step (C1.1.2.3):** The child now accepts a
source-span step for its exact paused BlueTS entry program as well as for a
classic root. The same metadata/source-ID receipt check and 256-root-
instruction budget apply before any step. The shared advancement path selects
the module VM step when appropriate, validates every successor against the
live program, and stops at a distinct bound span, terminal completion, or the
existing explicit limit state. A child regression rejects a wrong source ID,
reaches the next module statement's span, and resumes the retained graph; the
classic limit regression still exercises the common bounded path.

**Real-process module root steps (C1.1.2.4):** An admitted three-statement
BlueTS ESM page is paused at its evaluate-body root point over the public
debugger socket. The client requests a root-instruction step and validates its
actual successor against the live safe-point inventory, then uses its
metadata/source receipts to request a bounded source-span step. The latter
pauses at a different bound source span; resume completes the same module and
the public browser connection reports successful module execution. The full
out-of-process debugger suite passes with this test.

**Stale module control (C1.1.3):** A real HTTP reload replaces that module's
realm after completion. On the original debugger stream, a root-breakpoint
arm, root-instruction step, and metadata-receipted source-span step using the
old module handles each fail with `StaleRealm`. The new page is still
discoverable under a different realm generation; no old request is retargeted
to its program.

**First nested-frame debugger boundary (C1.2.1.1):** Existing compiler-safe
points already name non-root code units, but that static tuple is not an active
frame: recursive or repeated calls can execute the same code unit at the same
offset. The current BlueJS interpreter nests ordinary calls on the Rust stack,
and its debugger continuation stores only the root frame. Therefore neither
`StepRootInstruction` nor a non-root `DebuggerSafePoint` may be reused as a
nested-frame step target. The first increment supports one ordinary,
synchronous interpreted closure called directly from a paused or executing
classic/entry-module root. It pauses before an exact verified instruction in
that closure, then steps within that same invocation. It does not claim
generator, async, constructor, tail-call replacement, direct-eval, native or
proxy-mediated calls, reentrant host callbacks, or deeper interpreted frames;
an armed request encountering one of these paths must fail with an explicit
unsupported outcome before executing the target, never silently skip the
requested pause or return a fabricated root successor.

On activation, BlueJS must match the closure's compiled code unit to the
selected program generation and mint a nonzero, per-realm invocation serial.
The child/public handle binds that serial to the exact tab, document realm,
program handle/generation, and code-unit ordinal. It is issued only for an
actually paused frame, not inferred from a static breakpoint. A frame step
requires this whole live tuple, and a returned frame or replaced realm revokes
it; even another invocation of the same code unit cannot inherit it. The
public replies disclose only the frame handle, verified safe point, and
source-free state, not operands, bindings, closure objects, or source text.

The VM continuation must own both the child frame and its suspended call-site
parent. Preserve each frame's operand, handler, iterator and execution-context
state, the unconsumed call inputs and pending completion, plus the module graph
when applicable. Every retained object edge must remain visible to the VM GC;
normal execution entry points cannot run concurrently. One nested step
executes exactly one child instruction and reports its real next instruction
boundary, including a backward branch; a terminal child result is integrated
once into the saved parent call site, never dispatched a second time.
Exception unwinding and unsupported call forms must leave no detached frame.
The protocol/page-host version changes only when the new active-frame type
and commands are wired end-to-end, with a separate capability that remains
unavailable on hosts lacking the continuation. C1.2.2 adds one-shot resume of
that same active frame; C1.2.3 proves returned-frame, cross-tab and predecessor-
child rejection.

**Private route checkpoint (C1.2.1.3.2):** Page-host v35 carries a separate
source-free frame tuple and nested arm/state/step commands from the core proxy
to the isolated child. The child issues it only after an actual paused
invocation and compares the whole live tab/document/program/code-unit/serial
tuple before scheduling a step. Core maps the child program identity to its
own program identity and validates each reply. Root-only execution state and
controls do not alias the active child; no public debugger frame or nested
capability is advertised until C1.2.1.3.3.

**Core frame reminting (C1.2.1.3.3.1):** The core proxy retains the exact
child-private serial only in its live per-tab association and issues a
process-unique nonzero frame handle after a verified child pause. Repeated
observations of that invocation keep the same handle. Child return, realm
replacement/close, and predecessor executor loss revoke it; even a replacement
child that starts its program counters at the same numbers receives a different
public-facing frame handle.

**Public active-frame route (C1.2.1.3.3.2):** Public debugger v32 now has a
distinct `DebuggerFrame`, `NestedFrames` capability, and nested arm, state, and
single-instruction step messages. The public frame handle is core-reminted and
never carries the private child invocation serial. The core checks the exact
live realm, program and static safe point before arming, and maps nested state
only from a verified real child frame. A nested step must present that exact
active handle; root controls cannot stand in for it. Hosts without the child
continuation keep the nested capability unavailable. The real core/child route
regression proves the public command semantics.

**Public BlueTS nested acceptance (C1.2.1.4):** A launcher-supervised BlueTS
classic page now proves the public debugger socket advertises nested-frame
control, arms an inner-function instruction, reports the real paused child,
and advances exactly one instruction to a distinct safe point under the same
core-owned frame handle. Root stepping is refused while the child is active;
the returned frame handle is refused after the child rejoins its original
caller and the page completes. A separate real BlueTS page arms an unsupported
deeper call and observes a rejected script rather than a fabricated pause.
This closes nested pause/step; same-frame resume remains C1.2.2.

**Native same-frame resume (C1.2.2.1):** The VM can now continue only the
retained nested invocation identified by its serial without an instruction
suspension. It rejoins the original classic or module root at the saved `Call`
successor, leaving that root paused for its own resume. The page runtime checks
the whole live tab/program/code-unit/serial identity before entering the VM
and revokes it on child return. Classic effects occur once, a linked module
dependency and entry complete once, and wrong or returned identities are
refused. No child or public resume command is exposed by this checkpoint.

**Private same-frame resume (C1.2.2.2):** Page-host v36 carries a separate
frame-bound resume request, acknowledgement, and `NestedResuming` state. The
child accepts it only for its exact paused tab/document/program/code-unit/
invocation tuple; the next owner advance runs that retained child to return
and parks the original root at its verified successor. The core translates
only its live reminted frame handle to the child-private tuple, preserves the
requested state distinctly from one-instruction stepping, and revokes the
association when the child returns. Classic and linked-module scheduler tests
and a real BlueTS child/core socket regression cover the route. Public socket
authorization and its own protocol version remain for C1.2.2.3.

**Public same-frame resume (C1.2.2.3):** Public debugger v33 adds a distinct
`ResumeNestedExecution` command, `NestedResumeRequested` acknowledgement, and
`NestedResuming` state under the existing nested-frame capability. Core
accepts only a well-formed, live, core-reminted frame handle for the exact
realm/program, then asks the private child to finish that invocation. It does
not alias the root resume or one-instruction step controls. A real
launcher-supervised BlueTS page steps once, resumes the same child frame,
observes the separate requested state, returns to the original root, and
finishes after root resume. Wrong and returned frame handles are rejected.

**Cross-tab active-frame denial (C1.2.3.1):** A real Launcher public debugger
session now pauses the same BlueTS child shape in two live tabs. Substituting
either tab's program identity around the other's frame handle fails for both
step and resume, while both genuine handles remain usable. A matching static
code-unit ordinal therefore cannot cross the tab/realm ownership boundary.

**Predecessor-child denial across cutover (C1.2.3.2):** A real Launcher cutover
test reproduces the dangerous case: two core/child generations replay the same
BlueTS page to the same tab, document generation, and program numbers, and
their first local frame counters both mint `1`. Before hardening, the old
public frame was numerically identical to the successor's. Public debugger
v34 now pairs the monotonic handle with a core-instance identity: the 32-bit
process ID guarantees distinction while predecessor and successor overlap
during cutover, and 96 OS-random bits protect against later PID reuse. Core
fails closed if entropy is unavailable. The core proxy compares the whole
frame tuple, including instance identity, before sending anything to the
child; the child invocation serial remains private. The real socket test now
rejects both old step and resume without disturbing the successor's own
paused invocation.

**First bounded stack/scope contract (C2.1.1.1):** The first inspection is a
snapshot only of an actually paused root or the one directly nested child and
its waiting root. Frames are ordered current child first, then its parent;
there is no synthetic deeper frame. A frame carries its verified code-unit
ordinal and instruction offset. Scope entries name only active lexical
binding-slot ordinals and their innermost-first scope depth. They carry no
identifier text, binding value, heap/object handle, static type, or source
position. The VM reads its retained interpreter state only; it never invokes
JavaScript or a getter to create an inspection reply. Callers request positive
frame and per-frame entry limits no greater than the fixed advertised caps
(64 frames and 256 entries); the producer stops at those limits and reports
`stack_truncated` and each frame's `scope_truncated` explicitly, including
when a smaller caller limit cuts off data. A stale, stepping, resuming, or
completed target cannot reuse an old snapshot. Child/core/public transport
stays unavailable until the whole exact-identity route is installed; C2.1.2
adds original BlueTS coordinates rather than implying them here.

**Native snapshot seam (C2.1.1.2):** BlueJS now copies only its parked classic
or module root continuation, or the exact live nested child followed by that
waiting root. Its source-free result records installed program generation,
code-unit ordinal, verified bytecode offset, and active lexical binding-slot
ordinal with innermost-first scope depth. A caller chooses positive limits up
to 64 frames and 256 entries per frame; overflow is reported independently
for the stack and each frame. The page runtime checks the tab, installed
program generation, and exact nested frame before returning the snapshot.
Inactive lexical blocks are excluded; repeated capture neither advances
bytecode nor invokes a getter. Native classic/module and page-runtime tests
cover these conditions. No child IPC or public debugger capability is added
at this seam; that remains C2.1.1.3–C2.1.1.4.

**Private child snapshot seam (C2.1.1.3.1):** Page-host protocol v37 defines
an exact document/program and optional active-frame snapshot request, with
caller-selected positive limits no larger than 64 frames and 256 entries per
frame. Its reply contains only verified code-unit/bytecode locations, active
lexical slot ordinals and depths, and independent truncation flags. The child
admits the request only for the queue-head continuation already paused under
that exact document and installed program; nested requests must match the
entire currently active invocation, and root requests cannot inspect behind
an active child. Stepping, resuming, stale, wrong-program, and over-budget
states fail closed. The v37 private wire and child scheduler are tested; the
core-facing proxy and public capability remain disabled pending the next
leaves.

**Core proxy snapshot seam (C2.1.1.3.2):** The core executor exposes only a
private, default-denied method returning its own source-free snapshot types.
Before asking the child, it verifies the core-owned live document/program and
either the still-paused root or the exact core-minted nested frame mapped to
the child invocation. Afterward it checks the full private reply echo, expected
root/child-first shape and top bytecode offset, frame and per-frame entry
budgets, explicit truncation, monotonic active-scope depth, and each exact
child safe point. The core never passes private program/frame IDs to its
caller. Tests include a real child, malformed shapes, and a transport double
that forges the reply tab, document, program, or frame. No public debugger
request or capability exists yet.

**Private-route acceptance (C2.1.1.3.3):** A live BlueTS classic page, with
the real child host behind its private socket, pauses in a nested function
after two lexical slots become active. A two-frame/full-scope read returns the
child then waiting root without truncation; a one-frame/one-entry read sets
both truncation flags. Repeated reads leave the same paused continuation
intact. Zero or oversized budgets, a wrong core frame handle, another program
generation, and the prior document after navigation all fail closed. A
Launcher-supervised real-process debugger socket still advertises Stack and
Scopes only as Planned while nested control remains Available. The public
protocol is intentionally unchanged until C2.1.1.4.
The complete 14-case Launcher debugger suite remains timing-sensitive around
its pre-existing short pending-admission window: two runs each passed 13/14
with a different pending-state assertion failing; both affected cases passed
when rerun alone. The focused new public-capability case and all 288 engine
library tests pass.

**Public stack/scope contract (C2.1.1.4.1):** The owner-only, opt-in debugger
socket will expose two independent operations and capability reports. `Stack`
reads the exact currently paused root program or an exact core-minted nested
frame, with a caller-chosen positive frame limit up to 64. It returns only
ordered code-unit ordinals and verified bytecode offsets plus
`stack_truncated`; no lexical entries are included. `Scopes` names the same
paused target, a zero-based frame index in that live stack, and the exact
expected safe point of that frame. It requests a positive per-frame entry
limit up to 256 and returns only active lexical slot ordinals/depths plus
`scope_truncated`. The exact expected safe point makes a scope query fail if
the frame is now at a different instruction; it is a location check, not a
step-epoch token. Neither operation treats a
configured breakpoint or static code unit as a paused frame. Both operations
check the core-owned realm/program generation and, for nested targets, the
whole core-instance-bound frame identity before using the private snapshot.
The two capability states are independent and default-denied on ordinary or
unsupported executor routes; source-free inspection needs no static-metadata
receipt, source permission, or value/object authority. The existing
owner-selected debugger socket is the access boundary, not a page/front-end
API. A socket connection cannot promote itself by merely requesting metadata
capabilities. The public wire and protocol version stay unchanged in this
design leaf; bump them together only after both operations and their real
BlueTS socket regressions are complete. Original BlueTS coordinates remain
C2.1.2, and value inspection remains C2.2.

**Public bounded stack/scope route (C2.1.1.4.2):** Debugger protocol v35
adds independent `GetStack`/`Stack` and `GetScopes`/`Scopes` messages. The
owner-selected route advertises Stack and Scopes separately only when the
live core executor can inspect an actual paused continuation; ordinary and
unsupported routes remain Planned and reject both requests. A stack reply
contains the exact current child-first/root-only safe points and explicit
stack truncation, but no lexical entries. A scope request selects one current
frame by index and expected safe point, and its reply contains only active
opaque binding-slot ordinals/depths with explicit scope truncation. Core
checks the live realm/program, complete core-instance-bound nested handle,
positive 64/256 budgets, selected frame and expected safe point before
returning either result. A real Launcher/core/host BlueTS debugger socket
regression proves both truncation flags, invalid-budget and moved-frame
denial, root inspection after child return, and completion denial. A real
supervised-core cutover regression rejects predecessor Stack and Scopes
requests even when tab/program/frame numbers collide. No source text, runtime
value, original BlueTS coordinate, or metadata authority is granted by v35.

**Original BlueTS stack-coordinate contract (C2.1.2.1):** Keep v35's base
`Stack` reply source-free. A separate owner-only debugger operation will take
an exact paused program/nested-frame target, the ordered safe points returned
by a prior bounded stack read, and one caller-supplied source ID per frame.
The server re-reads the current stack, requires the entire ordered safe-point
list to match, and accepts only positive counts within the fixed 64-frame
budget. All source IDs must belong to the same live BlueTS metadata
attachment for that program, must have been inventoried on this same debugger
stream, and must pass the independently granted `OpaqueSafePointSpan`
capability. Each result is the existing exact half-open original UTF-8 byte
range with UTF-16 coordinates, attached to its frame and receipted source ID;
the stack truncation flag is echoed. The operation has no authority to read
source text, choose a nearest position, infer a module path, or inspect
values. An unbound frame or wrong source receipt rejects the entire request
without partial coordinates or a fallback. This is a batched use of the
existing exact-span grant, not a new ambient coordinate permission. The
public wire remains v35 in this design leaf; an implementation and real
nested/module socket tests must be complete before its next version bump.

**Retained nested/module source-map coverage (C2.1.2.2):** The v1 direct map
previously bound only the first instruction of each top-level AST statement.
At a real nested pause, the child entry and the suspended root `Call` were
both unbound even though both belong to checked BlueTS lowering spans. BlueJS
now records each root statement's exact emitted half-open instruction range
and the direct child closure index created by a root function declaration.
The bridge verifies those records against the installed program and attributes
only instruction boundaries inside an owning range or that declaration's
direct child code unit. Root `Halt` and other unowned instructions stay
unbound. A child frame's v1 span is the original whole function declaration;
the suspended root frame's span is its original top-level expression statement.
The map preserves the compiler's original byte/UTF-16 locations, including
non-BMP prefixes, and never infers a nearest statement from an offset. Both
classic and module frames passed the exact private child/core span route.

**Batched stack-coordinate values (C2.1.2.3.1):** The pending public request
uses the complete prior `Stack` snapshot as its expected state, including
program, exact optional nested frame, ordered safe points, and truncation
flag. A parallel source-ID vector has exactly one entry per frame, all under
one live metadata parent for that program. The result repeats the same stack
and ordered exact safe-point/source span pairs. Value-shape validation requires
one to 64 frames, a root-only stack without a nested handle or a matching
nested top frame, well-formed original coordinates, and equal vector lengths.
These types do not themselves grant source access or establish a live pause;
the future dispatcher must verify same-stream receipts, authorization, and
the entire current stack before making any child span queries. No request or
reply variant exists yet, and public debugger v35 is unchanged.

**Public stack-coordinate batch route (C2.1.2.3.2):** Debugger protocol v36
adds `GetStackCoordinates`/`StackCoordinates` only after the complete core
route is installed. It is not part of the source-free base `GetStack` reply.
The handler requires the existing explicit `OpaqueInventory`,
`OpaqueSourceInventory`, and `OpaqueSafePointSpan` grants and same-stream
metadata/source receipts for every ordered frame. It reuses the exact Stack
target checks, reads the currently paused bounded stack, and compares the
entire expected program/frame/safe-point/truncation snapshot before asking
the private child for any original span. Each child response must match its
requested source and be well formed; one failure returns only an error, never
a partial vector. A moved stack returns an execution-state error before a
span query. IPC round-trip and core dispatch tests cover the v36 contract,
missing/cross-stream receipts, guessed IDs, moved frames, and an unbound
later frame. Launcher-supervised socket acceptance remains C2.1.2.3.3.

**Real v36 stack-coordinate acceptance (C2.1.2.3.3):** A Launcher-supervised
core and BlueJS child return exact original BlueTS byte/UTF-16 spans for
both child and suspended root frames in classic and module scripts. The same
socket tests prove that source IDs guessed before inventory, unknown IDs,
wrong but receipted sources, a moved nested safe point, and a replayed
predecessor-stream receipt never produce a coordinate batch. A classic root
also pauses at its compiler-verified terminal `Halt` instruction, which has
no direct lowering span: the single-span and batched routes both reject it
without a partial result. The child already returned a generic private
`InvalidRequest` for this unbound case; core now classifies it as public
`InvalidTarget`, using the existing fail-closed static-target mapping rather
than incorrectly reporting a lost realm. Base `Stack` remains source-free.
The 16-case Launcher debugger suite passed 15 cases in one serial run; its
pre-existing short pending-admission-window test for an unsupported deeper
call shape returned `InvalidExecutionState` while arming, then passed when
rerun alone. Both new coordinate tests passed in the serial run and alone.

**Bounded paused-slot value contract (C2.2.1.1):** Value inspection is a
separate owner/client `BoundedValues` grant, not implied by Stack, Scopes, or
static metadata. Its only selector is a slot actually returned by `Scopes` on
the same debugger stream, together with the exact program, optional
core-instance-bound nested frame, frame index, safe point, slot ordinal, and
scope depth. Core records a bounded receipt for returned entries and rechecks
the live paused frame and active entry before asking the child. A guessed
ordinal, stale/moved frame, replaced realm, or non-BlueTS attachment cannot
dereference a value. There is no arbitrary object ID or expression evaluation
input; a copied preview contains no reusable heap handle. A root slot is read
from its retained classic or module execution, while a nested slot is read
from its own retained invocation, including cell-backed captured bindings.
An uninitialized binding is an explicit refusal, never `undefined`.

The immutable preview is a tagged tree: `undefined`, `null`, boolean, exact
IEEE-754 number bits (so negative zero and non-finite values survive), signed
BigInt bytes, lossless UTF-16 code units (including lone surrogates), dense or
sparse array elements with explicit holes, and ordered string-keyed record
entries. Keys use the same lossless UTF-16 representation. Traversal reads
only own stored data descriptors of VM-owned ordinary objects (null or this
realm's intrinsic Object prototype) and arrays (this realm's intrinsic Array
prototype); it never walks prototypes, calls JavaScript, invokes getters,
runs proxy traps, or stringifies values. Accessors, symbols and symbol keys,
functions, exotic/host objects, private-field instances, custom prototypes,
cross-heap objects, and cycles refuse the *whole* preview, rather than
returning a partial or misleading representation. Arrays may have holes but
not extra named/symbol properties. Ordinary records use own property order.

Hard caps are depth 4 (the selected value is depth 0), 32 elements/entries
per container, 256 total tree nodes including holes, and 4,096 aggregate
payload bytes across all UTF-16 strings/keys and signed BigInt bytes. Lengths
and container shape are checked before cloning/enumeration; over-budget
results refuse atomically, with no truncation that might look complete.
These are output-payload limits, not claims about JSON framing overhead.
Previewing is a read of the paused heap only: no allocation in that heap, no
execution step, and no source-text or static-type disclosure. A runtime string
is still a runtime value even if its contents happen to resemble source text;
the debugger provides no source-text lookup through this operation. C2.2.1.2
and C2.2.1.3 build the native snapshot, C2.2.1.4 transports it privately,
and C2.2.1.5 introduces the public wire and owner receipt gate. Explicit
typed refusals for forged handles, source requests, and cross-realm attempts
remain C2.2.2; this design leaf does not change the v36 protocol.

**Native paused primitive preview (C2.2.1.2):** BlueJS now re-reads its exact
paused root or nested stack, verifies the caller's code unit, instruction,
frame index and active slot/depth, then copies only an initialized primitive
from that frame's binding or VM-owned capture cell. Classic root, retained
module root, and direct nested invocation are distinct sources; the public
page-host and debugger protocols are unchanged. Number bits preserve `-0` and
`NaN`, BigInt uses bounded signed bytes, and strings preserve ill-formed UTF-16.
Objects and symbols refuse for now. Native regressions cover all primitive
tags, a root after resume, wrong frame/slot/offset/serial, oversize string and
BigInt, a retained module binding, and a child-local binding after it becomes
cell-backed by a closure capture. The three focused preview tests, BlueJS
Clippy with warnings denied, formatting, and 529 of 530 BlueJS lib tests pass;
the remaining pre-existing native-stack host-query test fails under this
environment's 1 MiB reported thread stack (measured about 2 MiB).

**Native plain-data preview (C2.2.1.3):** The paused binding preview now asks
the current VM heap to copy a bounded tagged tree. Only ordinary objects with
null or this realm's Object prototype and actual arrays with this realm's
Array prototype qualify. The heap examines its own stored descriptors and
keys, never a JavaScript `Get`, prototype lookup, accessor, or Proxy trap.
Records retain ordinary own-key order and lossless string keys; arrays retain
explicit holes distinct from `undefined`. A cycle or any unsupported nested
value rejects the entire result. Custom prototypes, private-field instances,
host/exotic objects, symbol keys and values, accessors, extra array props,
and cross-heap IDs cannot become plain-data previews. The native walk enforces
depth 4, 32 entries per container, 256 nodes including holes, and 4,096
aggregate UTF-16/string-key/BigInt payload bytes before returning a tree.
Native regressions verify sparse arrays, nested records, getter and Proxy
side-effect absence, cyclic and non-plain shapes, and depth/length/node/byte
excess. All 532 BlueJS library tests other than the already known native-stack
environment case pass; all-target BlueJS Clippy and formatting pass. There is
still no private or public value transport.

**Private value types before transport (C2.2.1.4.1):** The page-host IPC now
defines an exact tab/document/program/frame/safe-point/active-slot target and
a lossless, handle-free preview tree, but adds no request or reply variant and
keeps private protocol v37. The target validator accepts only root index zero
without a child frame, or a matching live child index zero/root index one
shape. The preview validator checks depth 4, 32 entries per container, 256
nodes including holes, 4,096 aggregate UTF-16 key/string and BigInt bytes,
and duplicate record keys iteratively before core remints a reply. Number
bits, BigInt bytes, UTF-16 strings, sparse array holes, and record keys round
trip without lossy JSON number/string conversion. Malformed target/tree tests,
all 103 IPC library tests with local Unix-socket access, and IPC Clippy pass.

**Private v38 paused-value route (C2.2.1.4.2):** The page runtime now admits
only an owned exact program and retained root or nested invocation before the
VM validates the selected active slot. The supervised child adds
`GetDebuggerValueSnapshot`/`DebuggerValueSnapshot` in private page-host v38,
requires the queue-head BlueTS program, live direct-debug attachment and
paused execution state, and echoes the whole exact target with a bounded
lossless tree. There is no object handle, source lookup or JavaScript call.
The child validates the reminted preview again before replying; the snapshot
is boxed in the Rust reply enum so its payload does not enlarge unrelated
error variants or change their wire representation. Page-runtime owner/stale
tests, private IPC socket round-trip, and child classic root, nested and
module-root tests pass. IPC 103/103, BlueJS 533/533 excluding the known
native-stack environment case, and workspace Clippy pass. Launcher full lib
passes 102/105: two old source-breakpoint/source-step expectations and one
private-socket missing-path case also fail when run individually; all five
focused BlueTS child tests pass. Core proxy and public value grants are not
yet installed.

**Core-reminted private paused values (C2.2.1.4.3):** The out-of-process
executor now has a separate private value-read method. It first re-reads the
exact paused stack at the full scope-entry budget and requires the selected
frame, safe point, and active lexical slot to match. A nested core-instance
frame is mapped only through its current child invocation association; a root
read requires the root-only pause. The child must echo the entire private
tab/document/program/frame/safe-point/slot tuple and a complete preview that
passes the IPC depth, container, node, payload-byte, and duplicate-key checks.
Only then does core remint the tree into a core-owned type with no child ID or
reusable object handle. Mismatched and over-budget child replies fail closed.
Launcher-supervised classic and module child processes prove nested binding,
caller-root binding, and resumed root-only reads, plus forged frame/offset
and successor-document denial. The real route leaves public BoundedValues
Planned and debugger v36 unchanged; the owner receipt/public request is
C2.2.1.5. The 292 engine library tests and workspace Clippy pass.

**Public paused-value boundary decision (C2.2.1.5.1):** The public operation
will have its own default-denied `BoundedValues` owner switch and a separate
client opt-in in debugger `Hello`; neither Stack, Scopes, static-metadata
grants, nor protocol version imply it. `HelloAck` grants only the intersection.
The live realm must also advertise the native value route, and the request
must find that exact available capability before reading a slot. These new
handshake fields, the public request/reply, and the protocol bump arrive
together only after the dispatcher is complete; v36 remains unchanged in the
intermediate ledger leaf. The metadata manifest retains its static-metadata
meaning and version.

The public target carries only program, optional core-instance-bound nested
frame, frame index, safe point, slot ordinal, and scope depth. After a
successful `GetScopes`, core records each returned entry in a bounded,
core-local receipt ledger owned by that socket stream. The ledger binds the
complete tuple and the current pause incarnation; a guessed or other-stream
tuple is not sufficient, even when its numeric fields happen to be valid.
The pause incarnation is core-owned, never a client-selectable VM handle.
Every successful command that arms, advances, or releases execution, including
one issued on another debugger stream, and every scheduler transition away
from the observed pause invalidates earlier receipts. A resumed frame that
later stops at the same bytecode offset must therefore require new Scopes.
The exact realm/program generations and nested core frame still guard
replacement and return; core re-reads the current paused stack and active
slot before the child call. Receipts are bounded to 4,096 distinct slots per
stream and cleared on pause invalidation or socket close. A full ledger
refuses the newly requested Scopes snapshot atomically rather than returning
entries that could not be receipted. Scopes on a stream without the value
grant remain unaffected.

The public reply copies the validated core-owned tagged tree, with no child
identifier or reusable heap handle. It preserves IEEE-754 bits, signed
BigInt bytes, UTF-16 units, array holes, and ordered record keys under the
same depth-4, length-32, node-256, aggregate-payload-4,096 limits as the
private route. Core validates the complete public tree before reply; no
truncated or partial value is a success. Missing owner/client/native grant
returns `CapabilityUnavailable`; malformed or unreceipted selectors return
`InvalidTarget`; a moved or no-longer-paused target returns
`InvalidExecutionState` (or the existing more specific stale realm/program
code); and any tree or receipt budget excess returns `ResourceLimit`.
Unsupported VM shapes remain a typed refusal, not an implicit `undefined`.
The value wire has no expression, positive source-text read, arbitrary object,
or property-path selector. C2.2.1.5.2 installs the internal receipt boundary, C2.2.1.5.3
installs the complete public route, and C2.2.1.5.4 proves it on real sockets.

**Core-local scope receipt groundwork (C2.2.1.5.2):** The existing
per-debugger-stream authorization now owns a bounded set of exact Scopes
slot targets, with an independent core pause-incarnation number. The set
remembers no value and cannot be supplied by the client. A snapshot with
duplicate or malformed entries fails atomically, as does any insertion past
4,096 distinct tuples. A new incarnation clears the stream's old tuples;
another stream never shares them. The core request receiver increments its
incarnation on successful arm/step/resume control replies before a later
request can use an old receipt. Counter exhaustion enters permanent deny
rather than wrapping to a previously valid incarnation. The public Scopes
handler does not yet record receipts because no value grant exists; the
complete guarded dispatch in C2.2.1.5.3 will activate observation. Public
debugger v36, Hello, and BoundedValues Planned remain unchanged. Two focused
IPC receipt/budget tests, the engine invalidation test, and all-target
workspace Clippy pass.

**Owner value policy before public transport (C2.2.1.5.3.1):** Launcher and
core now parse the independent `--debugger-bounded-values` owner opt-in and
require an explicit debugger socket. The Launcher launch options carry this
choice to each supervised core generation without enabling any static-metadata
policy. Core retains the choice but debugger v36 still has no value grant,
request, or reply; BoundedValues stays Planned, even with the flag. The next
leaf activates this owner policy only in the same change that adds separate
client negotiation and the complete receipt-gated route. Focused core and
Launcher CLI tests reject the flag without an endpoint and prove it remains
independent of static-metadata inventory; the Launcher launch-option test
passes. Both binaries pass `cargo check`.

**Public value types before transport (C2.2.1.5.3.2.1):** The debugger IPC
now has a serializable exact value target and separately validated snapshot
and tagged preview types, but no `GetValue`/`Value` wire variants yet. The
target contains only the already-defined Scopes selector and core frame;
the snapshot echoes that selector and a handle-free tree. Public validation
independently enforces the private/native depth 4, container length 32, node
count 256 (including holes), and aggregate UTF-16 key/string and BigInt byte
limit 4,096; duplicate record keys and impossible root/nested shapes refuse.
Tests cover lossless number bits and lone-surrogate UTF-16 round trips,
malformed program/frame/index identities, every budget, and duplicates.
Debugger v36 remains unchanged; the complete negotiated route and bump are
C2.2.1.5.3.2.2. All 107 IPC library tests and workspace Clippy pass.

**Core-internal public-value remint (C2.2.1.5.3.2.2.1):** A guarded core
helper now accepts only a combined owner/client grant, a negotiated stream's
exact Scopes-slot receipt, and the current core pause incarnation. It
re-reads that frame's full active scope inventory before calling the private
child proxy, then copies the complete core-facing tree into an independently
budgeted public snapshot. This remint limits depth, container length, nodes
including holes, aggregate UTF-16/BigInt bytes, and duplicate record keys
before allocation grows past a cap. The out-of-process executor separately
reports whether its private value route is installed; no public request can
reach the helper until C2.2.1.5.3.2.2.2 wires the grant and operation.
Mock-core regressions prove exact core frame/slot forwarding, distinct
stream and pause denial, moved live scope rejection, lossless bits/UTF-16 and
array holes, and every remint budget. Two focused tests, all 295 engine
library tests, and workspace Clippy pass. Public debugger v36 and
BoundedValues Planned are unchanged.

**Complete public paused-value route (C2.2.1.5.3.2.2.2):** Debugger v37
introduces `requested_bounded_values` in `Hello` and its exact owner-policy
intersection in `HelloAck`; the static-metadata manifest remains independent.
The core listener reconstructs a stream-local grant that cannot be supplied
by a later request. Only a granted live child route advertises
`BoundedValues` Available. An authorized `GetScopes` records exactly the
returned slots under the core session's current pause incarnation, refusing
an over-budget receipt set atomically; ungranted Scopes remains source-free
and unaffected. `GetValue` requires that same stream/slot/incarnation,
rechecks the active scope and exact frame, invokes the private child proxy,
and returns only a complete validated handle-free `Value` snapshot. The
independently bounded public remint rejects oversized or duplicate-key trees
without returning a partial preview. IPC socket serialization and separate
owner/client handshake tests pass; a mock public dispatcher proves grant,
receipt, cross-stream, stale-pause and moved-frame behavior. A real core
socket confirms owner-enabled v37 grants only the requesting stream; the
Launcher endpoint cutover regression passes with matching rebuilt binaries.
All 108 IPC and 296 engine library tests, format, and workspace Clippy pass.
The Launcher-supervised classic/module root/nested and budget regressions
remain C2.2.1.5.4.

**Real public value reads (C2.2.1.5.4.1):** The Launcher-supervised
classic and module fixtures now enable the independent owner value policy and
request the v37 client grant. Through only the public debugger socket, each
test arms a nested BlueTS entry, receipts exact current Scopes slots, reads
lossless nested and waiting caller-root numbers, then resumes the child and
reads the returned root's own number from a newly receipted slot. Both paths
pass the focused real-process test. Cross-stream, stale-pause, forged-slot,
and complete budget denials remain C2.2.1.5.4.2.

**Real public value refusal and caps (C2.2.1.5.4.2):** On the same
Launcher-supervised classic/module sockets, a client without a value grant is
refused, a separate granted stream cannot reuse the first stream's exact root
slot while that root remains paused, a forged slot is refused, and the nested
slot receipt expires immediately when its pause is resumed. The real BlueTS
budget fixture passes four over-limit values as active nested arguments:
depth 5, container length 33, 298 tree nodes, and 4,098 UTF-16 payload bytes.
Each is refused as a whole at the public `GetValue` route. Four neighboring
arguments at depth 4, length 32, exactly 256 nodes, and exactly 4,096 payload
bytes return complete independently validated previews; an ordinary numeric
argument remains readable. Both focused real-process tests, formatting, and
workspace Clippy pass. The child currently maps a native unsupported or
over-budget preview to `InvalidExecutionState`; the more precise typed
refusal distinctions belong to C2.2.2.

**Typed value-refusal boundary (C2.2.2.1):** Preserve the only positive
value selector: an exact, active `Scopes` slot receipted on this granted
debugger stream. A syntactically malformed or guessed target, a target copied
from another stream or realm, and a slot from an expired pause all fail with
typed `InvalidTarget` before resolving a realm, program, or private child.
This intentionally gives no existence oracle for another realm. An owner or
client without the independent value grant receives typed
`CapabilityUnavailable`; a receipted but no-longer-live frame receives
`InvalidExecutionState` where the live check can establish it. Native
unsupported shapes—including cross-heap object references—and native
over-budget trees remain a whole-value `InvalidExecutionState`, not a partial
tree or a shape-specific oracle. Core's independent remint can still return
`ResourceLimit` if a child violates its output budget. These existing typed
categories are stable identifiers; clients must not parse diagnostic prose.

No positive public source-text read selector is added. The existing unit
`Unknown` catch-all cannot deserialize a payload-bearing unknown JSON
command: it closes the stream before core can give a typed refusal. Therefore
v38 adds an explicitly denial-only `GetSourceText { program }` probe. IPC
parses its opaque program shape, but core never looks up the program, realm,
child, or source; every post-`Hello` probe gets the same generic typed
`CapabilityUnavailable` as a unit unknown command, without echoing its target.
Malformed JSON still fails framing rather than becoming a different request.
Before `Hello`, the existing typed `ProtocolVersion` refusal remains. The
new denial-only variant and changed unknown-operation reply require debugger
v38; no source text can be returned. An authorized runtime string whose bytes
resemble source text is still a value, not a source-read operation. C2.2.2.2
implements the mapping and tests framing; C2.2.2.3 proves it on real sockets.

**Typed public refusal implementation (C2.2.2.2):** Debugger v38 carries
the denial-only `GetSourceText { program }` request. The core never resolves
that program; it returns the same fixed `CapabilityUnavailable` code and
message as a post-`Hello` unit unknown command, whether the parsed program
names the local realm or another one. Before `Hello`, the probe returns
`ProtocolVersion`. A raw framed probe followed by another request remains
parseable, while guessed and cross-realm `GetValue` targets still fail at the
same-stream receipt gate with `InvalidTarget` and no child call. The existing
private child tests retain exact active-frame and plain-value refusals; no
private source-text operation or value grant was added. All 109 IPC and 297
engine library tests, five focused private child tests, formatting, and
workspace Clippy pass. Real public socket proof remains C2.2.2.3.

**Real typed value refusals (C2.2.2.3):** The Launcher-supervised v38
classic and module sockets return identical generic `CapabilityUnavailable`
for local and foreign `GetSourceText` probes and a unit unknown command, then
continue to answer the same stream. On each granted, actually paused BlueTS
child, structurally valid guessed-program and foreign-realm `GetValue` targets
return `InvalidTarget` before a child read. An independent short BlueTS
fixture passes a function as one initialized nested argument beside a readable
number; its exact receipted slot produces only `InvalidExecutionState`, never
a partial preview. The existing four exact-cap/four over-cap real-value test
still passes under v38, and workspace Clippy is clean. The classic/module
test hit the previously observed pending-admission timing race once (program
completed before breakpoint arming) and passed unchanged on rerun; no v38
reply mismatch was observed.

**Uncaught BlueTS exception-location contract (C2.3.1.1):** Location means
the exact original BlueTS span of the instruction that first produced an
*uncaught, catchable* runtime exception, not the caller's `Call`, a nearest
statement, the location where a message was formatted, or a compile/host
failure. BlueJS retains only its already-installed debugger program
generation, code-unit ordinal, and verified instruction start offset as a
source-free throw-site sidecar. Propagation through a direct nested call keeps
the callee's first site; entering a catch clears it, and a new throw that
replaces an older completion records its own site. A normal completion clears
any retained site. These rules must not change JavaScript catch/finally
semantics or turn a host/resource abort into a reportable language exception.
No source identity, text, span, value, or error message enters the VM sidecar.

The supervised child matches that tuple only against the exact live BlueTS
debug attachment and its compiler-bound safe-point map. An unbound
instruction, wrong program generation, missing/ambiguous source record,
ordinary JavaScript program, or replaced document produces no guessed
location. The private page-host route will carry one bounded
source-ID/safe-point/original-byte-range/UTF-16-coordinate record under the
exact tab/document/program identity, never the thrown value, stack, source
text, generated text, or runtime message. The next complete private route
bumps page-host v38 to v39; the frontend script reports remain source-free
fixed categories and do not acquire this privileged detail.

Public `DescribeExceptionLocation { source }` will require the already
independent owner/client `OpaqueSafePointSpan` grant and that exact source ID's
same-stream inventory receipt under the program's metadata parent. Core must
revalidate the live document/program and child record, remint a distinct
exception-location reply containing only the exact program-bound safe point
and original half-open UTF-8 byte/UTF-16 coordinate span, and refuse any
source mismatch or stale generation without a partial position. Missing
grant returns typed `CapabilityUnavailable`; forged/unreceipted source or
unbound mapping returns `InvalidTarget`; a normally completed or still-active
program has no exception location. No exception-policy pause capability or
source-text read is implied. The complete public route bumps debugger v38 to
v39; C2.3.1.2–C2.3.1.5 implement and prove each boundary in order.

**Classic native throw-site groundwork (C2.3.1.2.1):** BlueJS now exposes
`VmDebuggerThrowSite`, containing only the installed debugger program
generation, code-unit ordinal, and exact throwing instruction offset. The
interpreter stamps a catchable throw at its instruction before unwinding, a
catch entry clears the transient site, and outer script completion publishes
the site only if the language exception escaped; a new execution or normal
completion clears it. An unregistered bytecode throw has no site. Classic
direct-throw, caught-throw, successor, and unregistered regressions pass,
alongside all 31 native debugger VM tests and workspace Clippy. No IPC or
source-map lookup changed. Nested/module propagation and replacement by a
new throw remain C2.3.1.2.2.

**Nested and module native throw sites (C2.3.1.2.2):** The VM now distinguishes
a new catchable throw from an error propagating through its caller by a
per-execution throw epoch. The first throwing child instruction keeps its
installed generation/code-unit/offset through a classic or module call;
catch entry clears that transient site, while a new catch or finally throw
replaces it, including when finally calls another throwing child. Module
completion publishes the same source-free sidecar only for an uncaught
catchable error, and new module/debugger executions clear it after their
entry is validated. Direct classic, nested classic, direct module, nested
module, catch/rethrow, finally replacement, and retained debugger-continuation
regressions pass; all 34 native debugger VM tests, workspace Clippy, and
format checking pass. No IPC or source-map lookup changed.

**Private location route split (C2.3.1.3):** The VM sidecar is the most recent
realm execution, not durable per-program debugger state. First, the page
runtime will expose it only for an exact tab-owned, generation-matching
program (C2.3.1.3.1). Next, the supervised child will snapshot each terminal
debugger-controlled execution before another program can overwrite the realm
sidecar and bind it through the live BlueTS attachment's exact instruction
entry; absent or ambiguous bindings retain no location (C2.3.1.3.2). Only
after that complete child operation exists will page-host v39 add the private
request/reply and core transport wrapper (C2.3.1.3.3). This keeps the wire at
v38 until the route is fully implemented and tested; no frontend report gains
privileged position data.

**Page-owned throw-site read (C2.3.1.3.1):** `BlueJsPageRuntime` now returns
the VM's source-free uncaught site only if the requested program is still
owned by the exact tab and its installed generation equals the VM site.
This is explicitly an ephemeral last-execution read, not a durable program
record. Focused tests cover classic nested and module root/nested throws,
another program's nonmatching read, successor execution overwrite,
cross-realm refusal, and discard/navigation invalidation. The child still
needs to snapshot the result before the next execution (C2.3.1.3.2); no
wire or source-map route changed.

**Child-only terminal location snapshot (C2.3.1.3.2):** A debugger-controlled
BlueTS execution now snapshots the ephemeral native site before the scheduler
advances another script. It checks only that pending classic program or the
entry graph's exact installed module handles, allowing a throwing dependency
to own its own private record. The site must pass live program/safe-point
validation and an exact compiler-bound lowering entry with one matching
source, bounded half-open bytes, and valid UTF-16 coordinates; a missing or
unbound entry yields no record. The child retains only the private safe point
and source-ID/span under its document/program record, not text, error value,
stack, or message. Focused tests prove classic snapshot survival across later
scripts, module nested and dependency attribution, normal BlueTS/ordinary JS
absence, unbound-offset refusal, and navigation expiry; all ten focused
private-child tests and workspace Clippy pass. The broader Launcher library
run is not green: it includes socket/sandbox failures and a separately
reproduced existing source-breakpoint expectation mismatch (offset 10 versus
15) in an unchanged route. Current BlueTS direct lowering rejects top-level
`throw` and `try/catch`; native caught-throw clearing was proven in
C2.3.1.2, while real caught-BlueTS socket coverage remains for C2.3.1.5.
No page-host wire or frontend report changed.

**Complete private v39 exception route (C2.3.1.3.3):** Page-host v39 adds
`DescribeDebuggerBlueTsExceptionLocation` and a dedicated reply under the
exact tab/document/program/metadata tuple. The child revalidates its saved
safe point against the live installed program and the exact retained BlueTS
map before returning only the child-private safe point plus source-ID,
half-open byte span, and UTF-16 coordinates. Pending/normal executions have
typed `InvalidDebuggerState`; mismatched program/metadata or failed live
mapping is `InvalidRequest`; replaced documents are `StaleDocument`. Core's
page-host transport wrapper remains default-denied for test doubles and
does not yet remint this into a public debugger response. All 14 page-host
IPC tests pass with local sockets, as do the focused child route test, the
real Launcher-supervised child socket test, the core wrapper socket test,
format checking, and workspace Clippy. No frontend report or public debugger
protocol changed; C2.3.1.4 provides the separate public authorization.

**Public exception-location route decision (C2.3.1.4.1):** The public request
will be `DescribeExceptionLocation { source }`; the only caller-selected
identity is a previously inventoried opaque BlueTS source ID under its
metadata/program/realm parent. The distinct `ExceptionLocation` reply will
echo that source, a newly core-reminted exact safe point under the same
program, and the bounded original half-open UTF-8 bytes/UTF-16 coordinates.
It will contain no error value, stack, message, source text, generated span,
or child handle. The core-private target/result carry only numeric public
program/metadata/source IDs and, on success, child-validated code-unit/offset
and coordinates; default-denied executors cannot fabricate a location.

Validation proceeds in a non-disclosing order: reject malformed source
shape; require the existing independent owner/client `OpaqueSafePointSpan`
grant for the live realm (missing grant is `CapabilityUnavailable` without a
child location query); then require that this stream actually received both
the metadata parent and this exact source ID (missing/forged receipt is
`InvalidTarget`). Resolve the live realm and program and ask the child only
for that parent's terminal site. Core must revalidate the returned exact
safe point and same source binding before reminting. A pending/normal program
has no location (`InvalidExecutionState`), while a stale document/program,
unbound instruction, mismatched source, or malformed child reply is an
`InvalidTarget`-class refusal with no partial position. This is a new reply,
not `StaticMetadataSafePointSpan` or a breakpoint/step grant. The protocol
stays public v38 until the complete route and tests land in C2.3.1.4.3.

**Default-denied core exception adapter (C2.3.1.4.2):** The page-debugger
executor trait now has a separate default-denied terminal-exception API. Its
core-private target carries only the public program/metadata generations and
the selected source ID; its result carries only that source ID, exact
code-unit/instruction offset, and bounded original bytes/UTF-16 coordinates.
The out-of-process adapter translates the live identities to child handles,
requires a matching private v39 reply, validates its source and coordinate
shape, then independently rechecks the child safe point and exact BlueTS
safe-point span before reminting the source-free result. Wrong program/source,
malformed point/span echoes, stale generations, normal completion, and
unimplemented test-double routes yield typed errors without partial data.
The focused adapter test and all 299 engine library tests pass, as does
workspace Clippy; the public debugger remains v38 until C2.3.1.4.3.

**Public debugger v39 exception location (C2.3.1.4.3):** The complete
`DescribeExceptionLocation { source }` command has only a previously
inventoried opaque BlueTS source ID as input. Core requires the existing
independent owner/client `OpaqueSafePointSpan` grant before testing the
stream's exact metadata/source receipts, calls the default-denied exception
adapter only after both gates, and rechecks the returned live safe point and
compiler-bound span before constructing a distinct core-minted
`ExceptionLocation` reply. Missing grant is fixed `CapabilityUnavailable`;
unreceipted/forged source, malformed or mismatched location is `InvalidTarget`;
normal/pending execution without an uncaught site is `InvalidExecutionState`.
The reply contains only source ID, exact safe point, bounded original byte
range, and UTF-16 coordinates; no error value, message, stack, source text,
generated text, or child handle. The public debugger protocol is now v39.
All 110 IPC and 300 engine library tests pass with local sockets, along with
workspace Clippy; real Launcher-supervised public socket proof is C2.3.1.5.

**Real exception-location proof split (C2.3.1.5):** First verify the positive
classic/module nested path over the public Launcher/core/child sockets with
an astral character before the throwing function, checking exact original
UTF-8 byte and UTF-16 column positions and the returned child code-unit
ordinal (C2.3.1.5.1). Then prove the negative owner/client grant, source
receipt, caught/normal execution, and successor-document cases on real
sockets (C2.3.1.5.2). Direct BlueTS lowering does not admit top-level
`throw` or `try/catch`. For caught-throw proof under the page's
`require_declared_global_calls` policy, the BlueTS source declares the
throwing function, exposes it on `globalThis`, and calls a locally declared
eval alias whose JavaScript `try/catch` invokes that function. The direct
bridge regression verifies that this exact source evaluates to `true` after
catching the original thrown value.

**C2.3.1.5.1 real-socket evidence:** The Launcher-supervised public debugger
test exercises classic and module BlueTS nested throws through separate
browser/core/child processes. After same-stream metadata and source inventory,
each returns one originating source, the first nested code-unit safe point in
the public inventory, and the original function declaration's exact UTF-8 byte
span and UTF-16 columns. An astral prefix makes byte and UTF-16 columns
different; the second module source ID receives `InvalidTarget` instead of a
partial location. The focused real-socket test passes.

**C2.3.1.5.2 real-socket evidence:** Separate Launcher/core/child socket
sessions prove that absent owner or client `OpaqueSafePointSpan` grant returns
`CapabilityUnavailable` even after source inventory. With both grants, a
fresh stream refuses the same valid source as `InvalidTarget` until it has
independently inventoried both metadata and source IDs; the exact source then
returns a location. A successor document refuses the predecessor's receipted
source as `StaleRealm`, while a newly receipted normally completed BlueTS
program returns `InvalidExecutionState`. The caught-throw BlueTS program uses
the checked eval-alias fixture above and also returns `InvalidExecutionState`
without a partial location. All three focused real-socket denial tests, the
bridge caught-value regression, workspace Clippy, and formatting checks pass.

**C3.1.1 source-set split:** The existing v36 `GetStackCoordinates` route
already maps a paused classic or module caller/callee stack within one BlueTS
program to exact original byte and UTF-16 spans after same-stream receipts.
C3.1.1.1 strengthens that real-process test: the classic fixture must expose
a second inventoried source, and substituting it for the caller's correct
source must return `InvalidTarget`, not merely any non-coordinate reply. The
module fixture exposes only one source, so its positive nested-stack proof
cannot establish wrong-but-receipted multi-source behavior. Current
`DebuggerStackSnapshot` and `DebuggerStackCoordinatesTarget` require every
frame and source to share one program/metadata parent. A closed module graph
can retain separate entry and dependency debugger programs; C3.1.1.2 first
decides how a truly cross-program stack represents and authorizes per-frame
sources, then implements and proves that contract on real sockets. C3.1.1.1's
focused Launcher test passes for classic and module without a wire change.

**C3.1.1.2.1 linked-module frame decision:** Do not relabel a dependency
closure as an entry-program code unit. Today the VM snapshot carries one root
generation and rejects a child with a different installed generation;
page-runtime and public frame identities similarly use one program, and the
public stack-coordinate target requires every source to share one metadata
parent. A distinct linked-module family will preserve the v39 same-program
route. Its arm target is an inventoried dependency safe point plus an explicit
entry program in the same live realm; the child must prove the dependency is
in the entry's closed authorized graph and that the target instruction belongs
to that exact installed dependency generation. The VM records both entry and
callee generations in the retained continuation and each stack frame, never
inferring a callee from a repeated ordinal or a module URL. The initial
bounded stack is the paused dependency child and its suspended entry caller.

The new public linked-frame identity is core-minted and binds the core
instance, document generation, entry program, callee program, code-unit
ordinal, and invocation serial/handle. `GetExecutionState { entry }` will
report a distinct linked-paused state; separate linked-stack and linked-resume
requests accept that exact frame. A linked stack carries an ordered safe point
with its *own* program for each frame, unlike `DebuggerStackSnapshot`. Its
coordinate request echoes the complete expected stack and one source ID per
frame. Each source ID must be under a separately inventoried metadata handle
for that frame's program; the two handles need not match.

The public operations are `ArmLinkedNestedSafePointBreakpoint` (entry and
safe point), `GetLinkedStack` (frame), `GetLinkedStackCoordinates` (complete
expected stack and ordered sources), and `ResumeLinkedNestedFrame` (frame),
with distinct linked-frame and linked-stack result types; none are added to
v39. Arm, stack, and resume require the existing live nested-frame execution
capability; coordinate reads additionally require the independent owner/client
`OpaqueSafePointSpan` grant and same-stream metadata and source receipts for
**each** frame. Core rereads the complete paused stack, checks exact equality,
then remints only compiler-bound original byte
and UTF-16 spans from the corresponding child attachments. One bad frame
returns no span vector. No source text, module URL, bytecode, or error value
is added to the public reply.

Malformed, cross-realm, cross-graph, or wrong-generation targets return
`InvalidTarget`; missing owner/client span grant or a missing same-stream
receipt returns `CapabilityUnavailable`; a moved or absent continuation
returns `InvalidExecutionState`; an expired document returns `StaleRealm`.
A wrong but receipted source for one frame returns `InvalidTarget`, with no
partial coordinates. The linked family needs a complete private page-host
v40 route and public debugger v40 route, each bumped only when its corresponding
route is implemented; both remain v39 during the native-only step. The acceptance
fixture is an authorized entry module importing a function from a separate
BlueTS dependency and calling it from the entry body; tests must prove the
two installed programs and two original source IDs remain distinct.

**C3.1.1.2.2.1 native evidence:** BlueJS now stamps each retained stack frame
with its own installed program generation. A separate linked-module native
pause accepts an exact entry/dependency generation pair and dependency safe
point only in the entry's reachable closed graph; the interpreter captures the
generation of the *calling instruction* because the active module name can
already denote the callee. This ensures a dependency function called during
dependency evaluation cannot consume the entry's target. The linked snapshot
requires the paused dependency child and entry caller, while the existing
same-program snapshot still refuses mixed generations. Native debugger tests
cover exact child/parent generations, wrong serial, stale entry/dependency
generations, unrelated graph members, and expiry after resume. The full 36
BlueJS native debugger tests pass; the private and public wires remain v39.

**C3.1.1.2.2.2 split and first seam:** The private route is staged as a
host-neutral page-runtime seam, then a child-local state/source-mapping seam,
then one complete private wire change. The page runtime now retains a distinct
linked frame carrying the exact realm tab, entry program, dependency program,
code-unit ordinal, and invocation serial. It accepts only a dependency safe
point owned by the realm and present in the closed graph, delegates exact
entry-to-dependency reachability to the VM, and checks both installed frame
generations on linked stack reads. Its resume consumes only that retained
linked frame and leaves the entry root for its separate module resume. Tests
exercise the real imported dependency call, both generations, old-route
refusal, wrong dependency/serial, cross-realm read, graph exclusion, unrelated
member, and navigation expiry. No IPC shape or version changes in this seam.

**C3.1.1.2.2.2.2.1 child scheduling evidence:** The page host retains an
independent linked target on a pending BlueTS module entry and identifies the
dependency by its separately registered child program. On advance it pauses
only the dependency safe point in the entry's attached graph, keeps the exact
linked runtime frame under the entry's scheduler state, and maps a two-frame
stack back to distinct child program safe points. Linked resume consumes only
that frame and restores the verified entry root boundary for the existing
module resume. Wrong entry/document selectors, moved frames, and attempts to
read the linked state through the v39 same-program execution route are refused.
The child test exercises the actual imported function and full return path;
the private/public wire remains v39. The child-local arm, stack, and resume
methods are intentionally staged until the complete private v40 route is
added, so their temporary dead-code allowances are removed at that wiring
step.

Verification note: the focused linked child test and workspace Clippy pass.
In the wider 49-test `bluejs_host::tests` run, 47 pass and two existing
source-breakpoint/source-step cases fail independently with an expected
bytecode offset of 10 versus the observed 15. Both reproduce in isolation;
neither uses the linked scheduler. This leaf does not claim to fix those
source-mapping assertions.

**C3.1.1.2.2.2.2.2 child source evidence:** The child-local linked coordinate
read first reacquires the complete paused two-frame stack with the original
scope budget and checks it against the caller's full expected snapshot. It
then resolves each frame's safe point through the independently registered
BlueTS metadata handle and compiler source ID for that frame's program. The
result is an all-or-nothing two-span array: a swapped metadata handle, an
unbound child or entry source ID, or a changed expected stack returns no span.
The real imported-dependency test verifies the dependency and entry source
provenance names remain separate. Private and public protocol versions remain
v39 until their complete linked routes are implemented.

**C3.1.1.2.2.2.3 private v40 route:** The page-host wire now has a separate
linked-module arm, execution-state reply, two-frame stack, two-source span
read, and exact-frame resume family. Each frame carries its own child program
safe point; the span reply contains only the complete pair of compiler-bound
original spans, never a partial vector. Large stack/span replies are boxed so
the existing error-returning child helpers keep their bounded enum size.
Before acknowledging an arm, BlueJS validates entry-to-dependency graph
reachability and both exact installed generations without executing or
reserving the graph. Child requests reject malformed/wrong serials, stale
documents, moved stacks, swapped metadata, and either unbound source ID.
Private protocol v40 is enabled only with these handlers and round-trip
coverage; the public debugger protocol remains v39 until core remints and
independently authorizes this route. The IPC library suite passes 111 tests;
the wider launcher host suite has the same two isolated source-offset failures
recorded above and no new linked-route failure.

**C3.1.1.2.2.3.1 core child adapter:** The core transport can now send each
private linked arm/stack/span/resume request, while a separate adapter accepts
only exact echoes of the entry/dependency programs, document, invocation,
complete two-frame stack, ordered source IDs, and compiler-valid byte/UTF-16
span ranges. A moved or malformed private reply returns a typed core failure
with no partial data. The adapter deliberately does not mint public frame IDs
or authorize span access; those are the next core leaf. The public debugger
wire remains v39.

The adapter's focused fake-child test passes after checking the valid
arm/state/stack/span/resume sequence and then altering each reply in turn:
wrong arm safe point, moved frame serial, swapped stack frame, bad second
source span, and wrong resume echo are all rejected. The existing coordinate
validator remains authoritative for byte/UTF-16 bounds; no public source
identity or capability is created in this leaf.

**C3.1.1.2.2.3.2 core linked-frame reminting:** Core now captures the complete
private paused stack before minting two distinct, process-bound frame handles.
Each handle carries its own reminted program and exact safe point; the private
entry/dependency IDs, invocation serial, and source attachments stay in the
executor. Re-reading the same pause preserves both handles, while a moved
invocation remints them and stale document/program mappings discard them.
Malformed or moved child state/stack replies clear the prior association.
The staged two-span read requires a separate grant plus both metadata and
source receipts, checks both ordered public safe points and their independent
metadata attachments before child access, and emits no partial spans. The
focused fake-child test proves stable/moved identities, distinct provenance,
grant/receipt/swapped-attachment denials without child calls, and complete
two-span success. The public debugger wire remains v39 until the final route.

**C3.1.1.2.2.3.3.1 public linked data shapes:** The public IPC crate now
defines a distinct linked-frame identity that can represent both the non-root
dependency and root entry caller. Its fixed two-frame stack requires ordered
distinct programs, one realm/core instance, separate frame handles, and exact
per-frame safe points. The two-source coordinate target and result each bind
their metadata/source attachment to the corresponding frame, rejecting a
swapped pair or reordered span. A focused IPC serialization/denial test passes.
No request/reply variant or callable route was added, and public v39 stays
unchanged until the complete authorized handler is ready.

**C3.1.1.2.2.3.3.2.1 core-facing linked arm:** A distinct default-deny linked
capability and core-facing frame/stack/state/authorization types are staged for
the public dispatcher. The out-of-process arm resolves both independently
reminted live programs, rejects a same-program or root dependency target,
revalidates the dependency safe point, and accepts only the private child's
exact graph-validated acknowledgement. Focused fake-child tests cover valid
arms, stale documents, same-program targets, and a forged arm echo. The
existing core pause collector now keeps stable frame handles when only the
requested scope budget changes. Public v39 remains unchanged.

**C3.1.1.2.2.3.3.2.2.1 core-facing linked state/stack:** The strict private
adapter distinguishes exact entry-bound Pending and Completed replies from
linked Paused and Resuming replies. Core publishes a linked pause only after
the complete dependency/entry stack is re-read and both program generations
are found in the current document's remint map; an unregistered frame clears
the old association. A repeated pause or different scope budget preserves
both core frame handles, whereas a moved invocation remints both and makes
the old top frame unusable. Focused tests cover all four lifecycle states,
complete stack reads, stale-frame refusal, and malformed/missing child data.
No public request/reply variant exists yet; the debugger wire remains v39.

**C3.1.1.2.2.3.3.2.2.2 core-facing linked resume/spans:** Exact resume now
requires the active top core frame and rechecks both independent live program
mappings before asking the child to resume the matching private invocation.
The two-source span operation requires a complete previously returned public
stack, a separate span grant, and independent metadata/source receipts for
both frames. It rejects any changed caller location, swapped metadata, or
missing grant/receipt before the child is contacted, then asks the strict
private adapter for all-or-nothing original byte/UTF-16 spans. Fake-child
tests cover successful resume and both spans, stale top frame, forged caller
stack, swapped attachment, and each missing authorization. The core-facing
route is complete; no public wire request or version change exists yet.

**C3.1.1.2.2.3.3.3.1 public linked control shapes:** The public linked-arm
target now requires one exact non-root dependency safe point under a distinct
entry program in the same realm. The separate linked lifecycle type validates
Pending/Completed, a complete entry-owned paused stack, and a non-root
dependency frame while resuming. The IPC test round-trips the paused shape and
rejects same-program/root targets or role-swapped state. These are data shapes
only; no request/reply route or public protocol bump has occurred.

**C3.1.1.2.2.3.3.3.2 public linked v40 route:** The public debugger now has a
distinct `LinkedModules` capability and complete linked arm, state, stack,
resume, and two-source coordinate request/reply family. Core remints both
programs and both active frames, checks the complete previously returned
stack against the current pause, and requires the existing independent
`OpaqueSafePointSpan` grant plus separate same-stream metadata and source-ID
receipts for each program before contacting the child. Child span mapping
revalidates the whole live stack and both source attachments atomically; no
partial span vector is returned. An IPC socket test round-trips every new
request/reply. The core dispatcher fake-child test proves same-program arm
denial, missing second receipt, absent span grant, a fresh unreceipted stream,
moved caller, and corrupt second span all fail without a partial response.
The real launcher-supervised BlueTS graph test proves the public arm/state/
stack/resume path over the child socket and independently reads exact original
dependency and entry spans. It exposed and fixed a core inventory issue:
refreshing one module's metadata formerly erased its sibling's mapping;
inventory now replaces only the selected program's attachments. The linked
resume returns to the entry root, which remains under its separate root
resume control until completion. Public debugger protocol v40 is enabled only
with this complete route; private PageHost remains v40.
The workspace test run stopped after three BlueJS regex-deadline failures
under concurrent build load; each passed in isolation, and all 576 BlueJS
library tests passed with one test thread. All 305 engine library tests passed
with local socket access and one test thread. Focused linked IPC, core, and
real child tests and workspace Clippy also passed.

**C3.1.1.2.3 real Launcher source-set proof:** An owner-manifest-authorized
HTTP page now loads a two-file BlueTS module graph through the actual
Launcher/core/child processes. The public debugger socket arms a dependency
function entry, observes distinct dependency and entry program/frame handles,
and returns two source-bound spans matching the exact original text and UTF-16
columns. A missing entry-source receipt refuses the whole request; swapped
source handles return a typed invalid-target reply; an HTTP reload expires
the old arm, state, and coordinate targets as stale realms. This closes the
cross-program nested source-set mapping without exposing either module's text
or private child handles in the protocol.

**C3.1.2.1 module breakpoint/symbol contract:** A module entry and each
dependency remain separate generation-bound public programs. Source-position
resolution accepts only a source ID under that program's own metadata and a
bounded original byte offset, with the independent `OpaqueSourceBreakpoint`
owner/client grant and same-stream metadata/source receipts. It yields either
one verified safe point in that same program or an explicit unbound result;
an unrelated program's source never substitutes. A static symbol location is
a declaration range, not an executable breakpoint: it separately requires
`OpaqueSymbolLocation` plus same-stream symbol and source receipts under one
program's metadata. A client may resolve a receipted declaration position,
but an unbound/type-only symbol must not be rounded to a nearby instruction.
Neither operation leaks a source path, text, symbol name, runtime value, or
child handle. Module-root points use the root execution-control family only
when their ordinal is zero; dependency function points use the linked family
only with an exact, distinct entry program and a live closed-graph check.
Any source-to-control path must revalidate the program generation, verified
safe point, pending execution state, and graph at arm time; stale, swapped,
cross-program, unreceipted, or moved targets return typed no-partial refusals.
The existing two-step resolve/arm composition remains available for discovery;
the implementation leaf will decide whether it needs an atomic source-arm
request after proving the existing module behavior. This decision changes no
wire or capability and leaves public debugger v40 unchanged.

**C3.1.2.2 module source/symbol binding:** The two-file Launcher fixture now
negotiates the separate source-breakpoint and symbol-location grants. For
each program it inventories only that program's metadata, source, and symbol
handles; a source position resolves to a safe point in the same program,
while the compiler symbol range names only its own original declaration.
Source-end positions remain explicitly unbound. Guessed source/symbol IDs
fail before child lookup, a dependency symbol paired with the entry source
is invalid, and the old source/symbol handles become stale after HTTP reload.
The test exposed that direct-bridge breakpoint resolution indexed root AST
provenance but omitted verified nested map entries: the dependency function
declaration resolved to its module root instead of the child entry. Both the
live attachment and retained registry now consult the already-validated
nested safe-point entries without duplicating their stored source map. The
resolver selects the most specific containing span, then the nearest
following span; identical declaration/child ranges choose the child's first
verified instruction. All 97 bridge library tests, the focused real Launcher
test, workspace Clippy, and formatting pass. No protocol bump is needed.
The broader 24-test Launcher debugger file passed 22 tests in one run; two
existing nested-arm cases lost their short pending-admission window before
arm. Each passed when rerun alone (the cutover case on its second retry).
All 305 engine library tests passed with local socket access and one test
thread; the full Launcher file is therefore not claimed clean for this run.

**C3.1.2.3.1 module-root source arm:** The existing atomic
`ArmStaticMetadataSourceBreakpoint` route also owns a module root code unit,
not just a classic root. A real Launcher/core/child socket test inventories
the module's own metadata and source, refuses an unreceipted guess, then
proves a verified function-child source position and a source-end unbound
position both refuse arm while execution remains pending. The original root
call position arms its exact program-bound safe point, pauses, resumes, and
completes. The public capability description now names both classic and
module roots; no wire or runtime behavior changed.

**C3.1.2.3.2 dependency source-to-linked control:** The real Launcher debugger
stream now inventories the dependency's metadata and source while the two-file
graph is pending, resolves an original dependency function position to the
verified child-entry safe point, and supplies that exact program-generation
bound point with a distinct entry program to `ArmLinkedNestedSafePointBreakpoint`.
The public arm revalidates the safe point before the private child validates
the live closed graph and pending state in the same owning request. A
same-program entry, wrong dependency generation, or arm after the pause has
moved returns a typed refusal; earlier core/child tests cover unrelated graph
members and forged child acknowledgements. Since source maps cannot change
within a retained program generation, the two-step source discovery plus
arm-time generation/graph validation preserves the contract without adding an
atomic source-specific public operation or bumping debugger v40.

**C3.1.2.3.3.1 symbol-to-control decision:** A symbol location is only an
original declaration range; source-position lookup may legitimately return a
later instruction, so its result alone cannot prove that a symbol is
executable. The supported composed workflow independently negotiates
`OpaqueSymbolDisplay`, `OpaqueSymbolLocation`, `OpaqueSourceBreakpoint`, and
`OpaqueSafePointSpan` with their inventory prerequisites. It accepts only a
same-stream, same-program symbol/source pair whose compiler-owned kind is
`Variable` or `Function`. `Interface`, `TypeAlias`, and `Import` are not local
executable breakpoint candidates in this slice, even if a later statement is
available; a missing display/location/span grant or receipt also refuses.
For a value candidate, resolve the declaration start under the same source
receipt, then require a separately authorized exact safe-point span under
that source to lie within the declaration's half-open range before choosing
root or linked control by its code-unit ordinal. Explicit unbound results and
non-overlapping later spans do not arm. Both control arms still revalidate
the generation-bound safe point and pending graph at the owning core/child
request. This is a checked composition of existing v40 operations, not a new
server-side symbol-arm authority or a wire version change; clients that do
not negotiate every required surface cannot claim symbol breakpoint control.

**C3.1.2.3.3.2 entry-module symbol control:** A data-only v40 helper now
checks the separately observed symbol kind, original declaration location,
source-position binding, and exact safe-point span as one candidate tuple.
It returns only a safe point already in those observations, or `None` for
type-only/import kinds, unbound positions, wrong IDs, a moved source byte,
or a later span outside the declaration. It does not grant any debugger
operation. The real Launcher socket test loads an inline BlueTS module with
an interface and a root variable, negotiates all four independent metadata
surfaces, verifies the variable declaration's original range and contained
root span, and arms only that exact module root. The interface and an explicit
unbound candidate leave execution pending. All 114 IPC library tests, the
focused Launcher test, workspace Clippy, and formatting pass; debugger v40
is unchanged.

**C3.1.2.3.3.3 dependency symbol control:** The real two-file Launcher
fixture now inventories the dependency's function symbol while execution is
pending, verifies its separately granted `Function` display and original
declaration range, resolves that declaration under the dependency's own
source receipt, and obtains the exact child span under the independent span
grant. Only the candidate accepted by the data-only same-program/contained-
span helper is supplied to the distinct entry's linked arm. The existing
same-program, wrong-generation, moved-pause, swapped symbol/source, and
reload-stale checks remain typed refusals. The focused real socket test,
workspace Clippy, and formatting pass; debugger v40 remains unchanged.

**C3.1.2.4 real module breakpoint/symbol acceptance:** The inline module
root and two-file linked dependency now each have a focused Launcher/core/
child public-socket test of independently granted source, symbol, and exact
span evidence feeding their appropriate execution-control arm. A second
debugger stream opened after the first closes cannot reuse its source,
symbol, or span handles, whether it requests no metadata grants or all of
them without same-stream inventory receipts; refusal is typed and returns no
partial payload. The two-file stream additionally proves both entry and
dependency source/symbol targets expire after real HTTP reload, alongside
the linked frame and coordinate targets. Concurrent servicing of two debugger
streams is not guaranteed, so the second-stream probe closes the first before
connecting. Both focused real-socket tests, workspace Clippy, and
formatting pass; public debugger remains v40.

**C3.1.3.1 static/runtime scope contract:** The existing public `Scopes`
inventory exposes only active `(scope_depth, slot_ordinal)` selectors;
`Value` returns a separately granted bounded runtime preview, while
`DescribeStaticMetadataType` and `DescribeStaticMetadataSymbolType` report
compiler-owned static evidence. None currently binds an active VM slot to a
specific checked BlueTS symbol, so equal names, inferred value shapes, or a
nearby source span are not acceptable joins. The bridge must retain a
verified, generation-bound mapping from a supported lowered declaration's
compiler symbol to its installed BlueJS code unit and lexical slot. A query
for one previously receipted active `Scopes` slot must repeat its exact
realm/program/frame/safe-point/slot target and metadata attachment; core
checks the receipt's internal pause incarnation and child rechecks the live
mapping before returning only
opaque symbol and type IDs under a new independent owner/client static-scope
grant. The symbol/type relation and optional type display still require
their own same-stream inventories and grants. No runtime preview, value
handle, type name, source text, or claim of runtime type validation belongs
in this static relation reply. Conversely `Value` remains the only bounded
runtime-preview reply under its separate grant and scope receipt; possessing
a static type grant never authorizes it. A client may present both replies
side by side only for the same live pause incarnation. Unsupported, erased,
ambiguous/shadowed without exact slot provenance, cross-program, moved,
cutover, and stale targets refuse without a guessed type or partial value.
An `Undefined` runtime preview does not erase a declared static type, and a
static `TypeId` is not a runtime proof. This design does not add general
watch/evaluate or dynamic type reification; debugger v40 remains unchanged
until the complete independently gated public relation is ready.

**C3.1.3.5.1 same-slot display/value decision:** The client may pair two
independent successful replies only when they name one exact active slot on
one debugger stream and one still-live pause. For an ordinary classic or
module root/parent, `GetStaticScopeRelation` echoes the complete
`DebuggerValueTarget` inside its static selector; its `static_type` may then
be passed to `DescribeStaticMetadataType` only after that stream inventories
the type and negotiates the separate type-display grant. `GetValue` must echo
the same `DebuggerValueTarget`, after an independently granted bounded-value
read and a same-stream `GetScopes` receipt. A type display is compiler
evidence; the bounded value is a copied VM snapshot. They are never merged
into one reply or used to claim runtime type validation. In particular, a
declared `number` beside `Undefined` remains two truthful observations.

The existing public `GetValue` target is an ordinary `Scopes` selector and
cannot name a linked entry-root slot. Add a distinct linked value selector
using the existing complete `DebuggerLinkedScopeTarget` (exact two-frame
stack, entry frame index 1, and scope entry), and a distinct echoed snapshot
with the existing bounded preview vocabulary. The private child route must
read the retained linked entry root directly from the exact live graph; it
must not synthesize an ordinary root frame or infer a slot from a name,
source span, or runtime shape. Core remints the preview under its existing
depth/node/payload budgets. No new metadata grant is implied: a complete
`GetLinkedScopes` reply may mint a linked slot receipt for a stream holding
either the static-relation or bounded-value grant, but `GetLinkedValue`
requires the bounded-value owner and client grants specifically. The static
relation still requires its own grant plus metadata, symbol, and type
inventories; a type display still requires its separate display grant.

Both paths recheck the core-owned pause incarnation and exact child document,
program generation, linked stack, and active slot before returning a full
snapshot. Truncated linked scopes, a missing or foreign-stream receipt, a
swapped frame/program, a moved step or resume, HTTP reload, or supervised
child cutover yield a typed denial with no partial type/value payload. A
client must discard an earlier half of a proposed side-by-side display if
the other half is refused. Stage the private route and native tests before
adding a callable public linked request; bump private and public protocol
versions only when each respective route is complete. This design leaf does
not change debugger v41 or the current metadata manifest.

**C3.1.3.5.2 real ordinary display/value acceptance:** A focused public
Launcher/core/child socket test runs the same supported BlueTS declaration in
classic and module form. It arms the root before metadata reads, obtains
same-stream compiler metadata/type/symbol and Scopes receipts, and checks the
exact static relation selector and `number` type display. An independent
Value-only stream advances to the initialized slot and obtains a bounded `9`
preview; static-only and relation-only streams refuse Value and type display
respectively, while the Value-only stream refuses the static relation. A
combined stream independently receipts the same live slot, echoes the same
selector in both replies, and proves its preview matches the Value-only
snapshot. After a real instruction step, both old slot requests refuse
without a partial payload; after HTTP reload, old relation, Value, and type
display targets also refuse. The focused real-socket test passes for both
script kinds without changing debugger v41 or the compiler/runtime ABI.

**C3.1.3.5.3.1 native linked entry value:** BlueJS now reads one entry-root
slot directly from the retained module continuation while a distinct
dependency child is paused. It first reacquires the full two-frame linked
snapshot, compares every frame generation, safe point, active slot, and
truncation flag with the caller's expected snapshot, then uses the existing
plain-data copier and its fixed depth/node/payload limits. The page runtime
rechecks exact tab ownership, both installed program handles, invocation
serial, and the complete snapshot before asking the VM for the value. Native
and page-realm regressions read an initialized `9`, refuse wrong serial,
slot, moved child frame, wrong realm/program, and resume-stale targets; a
4 KiB-plus string refuses without a partial preview. Five focused linked
tests and eleven existing plain-preview tests pass. This leaf adds no private
wire route or public Value authority.

**C3.1.3.5.3.2 private linked entry value:** Page-host protocol v42 carries a
distinct linked value target: the exact linked frame, its complete child-first
two-frame stack, frame index one, and one unique entry-root slot. The private
reply echoes that entire target and one bounded plain-data preview. The host
first validates the wire shape, reacquires the active linked frame and full
stack, and then asks the page runtime to recheck both installed programs and
the retained BlueJS continuation before copying the binding. Malformed and
moved selectors, wrong programs or document generations, and resumed frames
return only typed errors. A preview exceeding the payload budget returns
`ResourceLimit` with no partial value. The IPC socket round trip and the
Launcher child-owner linked-pause test pass; this leaf adds no public debugger
request or grant. The child executor and core-facing adapter follow in
C3.1.3.5.3.3.

**C3.1.3.5.3.3 child and core-facing linked value:** The child client now
transports the v42 linked-value request. A strict private adapter accepts only
a well-formed bounded reply that echoes the complete frame, stack, selected
root frame, and slot. The core-facing selector contains only its reminted
two-frame identity and root slot. The executor rechecks the full live linked
scope, exact entry slot, and retained private stack before sending the child
request, then remints the plain-data preview only after the adapter accepts
the exact echo. The public debugger still has no linked Value request or
receipt. The focused engine regression reads `9` and refuses wrong frame and
slot, swapped program order, altered child echo, and over-budget child data.
The native, private wire, owner, and core-facing parts of C3.1.3.5.3 are now
complete; public authorization and same-stream receipt are C3.1.3.5.4.

**C3.1.3.5.4.1 staged public linked value and receipt:** The existing
`DebuggerLinkedScopeTarget` is the exact public linked value selector: it
contains both core-reminted frames, the entry frame index, and one slot. A
distinct `DebuggerLinkedValueSnapshot` echoes this selector with the existing
bounded plain-data preview, while debugger v41 still has no request or reply
that can carry it. The per-stream receipt table now exposes an exact linked
slot check separate from ordinary `Scopes` receipts. A complete
`GetLinkedScopes` reply records its slots when either the independently
negotiated static relation grant or bounded-value grant is present; a visibly
truncated reply records none. Receipts remain limited by the shared fixed
scope budget and expire on a new core pause incarnation. This prepares the
public handler without giving a Value-only stream metadata authority.

**C3.1.3.5.4.2 authorized public linked value:** Public debugger protocol
v42 adds `GetLinkedValue` and an exact echoed `LinkedValue` snapshot. Core
requires the independently negotiated bounded-value owner/client grant and
the complete same-stream `GetLinkedScopes` receipt from the current pause.
Before reading, it reacquires the live linked entry scope, confirms the
unique selected slot, and compares the full core-reminted two-program stack
with the current child state. The already strict core-facing adapter then
reads the retained entry binding. Core remints the plain-data preview under
its fixed depth, node, container, and payload limits and returns one complete
snapshot or one typed error. IPC and engine tests reject absent grants or
receipts, truncated scopes, foreign streams, new pause incarnations, forged
slots, swapped programs, moved stacks, and over-budget data. The IPC 120-test
and engine 309-test library suites pass. The engine value test module was
split into an ordinary-value file and a linked-scope file, both below 1,300
lines. The real Launcher socket acceptance and expiry proof remains
C3.1.3.5.5.

**C3.1.3.5.5.1 linked same-slot socket acceptance:** A two-file BlueTS module
pauses in its dependency while the entry root still owns an initialized
`rootValue` slot. One real Launcher/core/child debugger stream obtains the
complete linked stack and linked-scope receipt, then pairs the exact echoed
linked static selector with its compiler `number` type display and the exact
echoed linked Value selector with a bounded `9` preview. Separate streams
verify that static-only, relation-only, and Value-only grants stay independent;
a Value-only stream must first receipt complete linked scopes. A foreign stream
without that receipt and a stream with truncated linked scopes both reject the
Value target without a partial preview. The focused real-socket test passes
using the shared workspace `target/` directory. Lifecycle expiry remains
C3.1.3.5.5.2.

**C3.1.3.5.5.2 linked lifecycle and cutover acceptance:** The public linked
execution route exposes resume, but no separate linked step command. A real
socket resume invalidates the old pause-bound static relation and linked Value
target without a partial reply. The compiler type display remains valid while
its program generation is live, because it is an independently authorized
metadata query. HTTP reload invalidates all three old selectors. A supervised
child cutover then starts a successor core, pauses the same two-file module,
and proves the predecessor relation, type display, and Value selector refuse
while the successor's fresh Value selector still returns `9`.

The cutover regression first exposed an ABA alias: with matching realm and
program ordinals, deterministic public metadata counters let the old type ID
match a fresh successor type receipt. Core now seeds its public metadata
handle and generation counters from its already-random process incarnation
within the reserved metadata namespace, and fails closed if that incarnation
cannot be minted. The independent linked frame still carries its full core
instance. The focused resume/reload and cutover real-socket tests pass with the
single workspace target cache.

**C3.1.3.5.5.3 workspace and coverage closure:** The all-target workspace
build, sequential workspace test suite, warning-free workspace Clippy, Rust
format check, and diff check pass. The CI-style workspace coverage run reports
90.33% lines against the 90% gate; the independent no-exclusion BlueJS run
reports 93.31% against its 88% gate. Both runs exercised the BlueTS compiler,
bridge, core, and real Launcher tests. `cargo llvm-cov` used a temporary
`target/llvm-cov-target` instrumentation cache, which was removed after the
reports; the ordinary `target/debug` cache was retained.

The line audit found one 1,322-line linked child test owner. Its independent
arm and real-child execution tests now live in an MPL-headed 544-line
`linked_execution.rs`; the paused-stack and static/value remint test stays in
the 784-line `linked_pause.rs`. The three tests pass under their moved module
paths, and the incremental all-target build, workspace Clippy, and formatting
checks pass after the split. No affected BlueTS/debugger source or test owner
exceeds 1,300 lines. C3.1.3.5, C3.1.3, and C3.1 are complete; metadata
invalidation and successor isolation continue under C3.2.

**C3.2.1 reload and tab-close metadata invalidation:** Reload already clears
the child executor's retained metadata and makes old public inventory,
derived type/symbol/contract/source, and lowering requests return typed
stale-realm refusals on a real Launcher stream. A new real socket test closes
the only BlueTS tab after receipting and displaying its `number` compiler
type. The public realm inventory then becomes empty, and old metadata and
type requests return typed `InvalidTarget` or `StaleRealm` errors with no
payload. The core child-executor test additionally mints successor metadata
after reload, closes that tab, checks both retained live-document and
metadata maps are empty, and rejects its former summary. The existing
`close_removed_tabs` lifecycle cleanup supplies the behavior; no public wire
change is needed. Focused core and Launcher tests, affected Clippy, and
formatting pass with the ordinary shared target cache.

**C3.2.2 cache and hibernation generation rule:** Current BlueIce has no page
cache or hibernation restore route, so no static BlueTS metadata can be
serialized and revived by either event. The existing HTTP source cache owns
only verified source bytes; eviction causes a refetch and fresh integrity
check, not an installed-program or metadata generation change. For a future
page eviction or hibernation, the supported fail-closed operation is BlueJS
program invalidation followed by `DirectDebugRegistry::prune_invalid` or
`forget`; a restoration path would require proof that the same checked
generation survived. The direct-bridge regression now invalidates a live
program, prunes its static metadata, reinstalls identical source with a new
generation, rejects the predecessor handle, and explicitly forgets the new
generation. The existing HTTP cache test covers refetch integrity after
eviction. The focused bridge test, Clippy, and format checks pass. This closes
the current no-restore contract without claiming that page-cache or
hibernation features are implemented.

**C3.2.3 successor metadata isolation:** The real supervised-child cutover
regression now asserts that a successor can reuse the predecessor's visible
realm and public program IDs while its full core instance and public metadata
ID differ. With the successor's own complete linked-scope and metadata
receipts, its Value request still returns `9`, while the predecessor's static
relation, type display, and linked Value selectors all receive typed
no-payload refusals. The direct bridge separately proves that reinstalling
identical source after invalidation mints a distinct BlueJS generation and
cannot retrieve predecessor static metadata. The focused socket test,
Launcher Clippy, and formatting pass. C3.2 is closed for the current
no-page-restore lifecycle contract.

**C4.1.1 public debugger capability inventory:** The real Launcher socket
suite already exercises each required public route through core and the
supervised child. `module_lifecycle::launcher_pauses_and_resumes_a_real_bluets_module_entry`
proves pause; `module_lifecycle::launcher_steps_a_real_bluets_module_then_rejects_stale_generation`
proves step; `spans_stack::launcher_batches_exact_classic_and_module_stack_coordinates_only_with_receipts`
proves stack; `scope_relations::launcher_relates_classic_and_module_root_scopes_without_value_authority`
proves scope; `runtime_values::launcher_reads_granted_classic_and_module_root_and_nested_bluets_values`
proves bounded Value; and
`exceptions::launcher_reports_original_classic_and_module_nested_exception_positions`
proves original exception location. These are distinct registered tests in
`backend/launcher/tests/out_of_process_debugger/`; the sequential workspace
suite passed, and `cargo test -- --list` confirms the current inventory.
Rejection classes are audited separately in C4.1.2.

**C4.1.2 public debugger refusal inventory:** Three distinct real
Launcher/core/child socket tests already cover the required rejection
classes. `module_lifecycle::launcher_steps_a_real_bluets_module_then_rejects_stale_generation`
requires `StaleRealm` for old program and safe-point requests after reload.
`exceptions::launcher_refuses_exception_locations_without_owner_client_grants_or_source_receipts`
requires `CapabilityUnavailable` without the independent owner/client
exception-location grant, and `InvalidTarget` without its exact source
receipt. `runtime_values::launcher_refuses_every_bounded_value_budget_on_a_real_bluets_socket`
requires typed `InvalidExecutionState` and no partial preview for depth,
collection, node, and payload over-budget reads. All were registered in the
current Launcher test inventory and passed in the complete sequential
workspace suite. C4.1 needs no additional wire or runtime route.

**D.1.1 real classic direct-page acceptance:** The supervised BlueJS child
previously rejected every multi-source classic BlueTS graph before the direct
bridge could erase a compiler-only `import type` edge. The child now validates
the exact caller-supplied closed resolution records and lets the direct script
compiler consume them; `lower_script` still rejects any runtime import. A
real child-process fixture supplies an entry source and a type-only declaration
module, then proves that only one classic program is installed and executed.
Its exact verified safe-point span names the original typed declaration byte
range after an astral-prefix comment, retains the matching UTF-16 column, and
resolves to the entry's canonical source identity. A replacement document
with an ordinary runtime import remains rejected and installs no program.
All 116 Launcher library tests, 13 real child-process tests, Launcher Clippy,
formatting, and diff checks pass. The original test owner remains 1,124 lines;
the new MPL-headed direct-page module is 194 lines. This proves the real child
transport; browser HTTP policy and public socket acceptance are covered by
later D leaves.

**D.1.2 real ESM direct-page acceptance:** A second supervised BlueJS child
fixture supplies an explicit static edge from `./answer.ts` to a canonical
`blueice://authorized/...` module. Because that target is distinct from the
specifier's relative URL, successful two-program execution proves that the
checked caller edge determines the runtime dependency. Both programs retain
compiler metadata whose verified safe points map to their own original typed
declarations with exact UTF-8 ranges, UTF-16 columns, and canonical source
provenance. Changing only the caller's resolver fingerprint changes the
compiler-options hash. Removing the edge rejects the same source graph and
installs no program, demonstrating no ambient URL fallback. The focused real
child test and Launcher Clippy/format checks pass with the shared target
cache; the new direct-page test module remains below 1,300 lines. D.1 is
complete for the real child transport, with public browser behavior remaining
under D.2 and D.3.

**D.2.1 real-page contract report:** The existing `blueice-core` binary
regression serves an HTTP page whose document snapshot exceeds the
core-selected one-mebibyte `blueiceDocumentText()` contract before inline
BlueTS admission. The public `GetBlueTsScriptReports` response now must have
exactly one rejection attributed to tab 1, document generation 1, ordinal 0,
and classic script kind. It must carry the fixed host-binding contract category
within 64 bytes. The focused subprocess test passes with the ordinary target
cache; protected-content checks are completed separately in D.2.2.

**D.2.2 protected-content boundary:** The same real HTTP document now
includes separate private markers in its oversized body and inline BlueTS
source. The test records the exact public IPC frame while using the normal
framed reader, caps the reply at 512 bytes, and requires both markers and a
128-byte document-text run to be absent from its serialized payload. The
decoded reply remains one fixed-category contract rejection and contains no
source or diagnostic field. The focused real core-binary test, formatting,
and diff checks pass with the shared target cache; D.2 is complete for this
declared immutable document-text boundary.

**D.3.1 public two-tab attribution:** The existing real `blueice-core`
binary regression navigates two separate HTTP pages through one public IPC
session. It requires distinct tab IDs, one source-free BlueTS report with the
exact requested tab/document/script tuple for each page, and the proper tab
ID in each public reply envelope. A repeated tab-one query returns an empty
report before tab two is read, proving the first drain cannot replay its own
result or consume the second tab's result. The focused subprocess test passes
with the shared target cache; later D.3 leaves cover reload, policy, and
resource accounting separately.

**D.3.2 public reload isolation:** Two existing subprocess cases form the
public boundary proof. `page_execution::real_subprocess_reexecutes_opted_in_inline_bluets_for_a_replacement_document`
serves two HTTP replacements to the real core binary and requires a fresh
BlueTS execution report with document generation 1 then 2 for the same tab.
`module_lifecycle::launcher_steps_a_real_bluets_module_then_rejects_stale_generation`
uses a real Launcher/core/child debugger socket after HTTP reload: predecessor
breakpoint, instruction-step, and original source-span-step targets all fail
with `StaleRealm`, while a successor realm/program for the same tab is
inventoried under a different realm generation. Both focused tests pass with
the shared target cache. Together they prove the old execution and debugger
identities cannot be carried into the successor document.

**D.3.3 tab/child source-policy isolation:** A real public Launcher/core/
supervised-BlueJS-child regression serves two HTTP origins with documents
declaring the same `/approved.ts` BlueTS module path. The owner HTTP manifest
pins only the first origin's full canonical URL and SHA-256 integrity. Public
report envelopes prove that tab one's module executes while tab two receives
only the fixed source-free authorization rejection. The test then cuts over
both tabs to a new supervised core/child generation and repeats the public
navigations with the same split outcome. The second origin's HTTP request log
contains only document fetches before and after cutover: neither child can
borrow the first tab's owner-authorized module URL or trigger an unauthorized
fetch. The focused subprocess test passes using the shared target cache; the
new MPL-headed module is below the 1,300-line limit.

**D.3.4 private numeric resource attribution:** The browser IPC intentionally
does not expose child bytecode/heap statistics. A new real supervised BlueJS
child-process test exercises the authenticated accounting boundary used by
core: two BlueTS pages occupy distinct tab realms, and the child-wide live
realm, program, bytecode, and VM-managed heap totals equal the two exact
per-realm charges. Closing tab one rejects its old realm-stats lookup while
preserving tab two's exact charge; the child total then equals only tab two.
Closing tab two zeroes every aggregate. The focused subprocess test passes
with the ordinary target cache. Together with D.3.1's public browser tab
report attribution and existing core validation of private stats tuples,
this proves tab/child accounting without adding a public resource-inspection
capability. The new MPL-headed test module stays below 1,300 lines.

**E1.2.1 boundary inventory record:** Core now defines a core-only
`HostBindingBoundaryRecordV1` with stable and runtime binding IDs, owner,
optional original source span, host-to-script or script-to-host direction,
reviewed contract ID, the owner's effective validation limits, a fixed
source-free failure category, and capability. Its current two records are
derived from the installed immutable document-origin and document-text
contracts. Both correctly leave source position absent because the copied
snapshot boundary is selected before an individual script call site is known;
future source-addressable crossings can fill the same field. A focused test
checks both records and proves a tighter owner-selected text limit is carried
into the record. This defines the shape; E1.2.2 will enforce completeness
against the installed runtime bindings.

**E1.2.2 installed-boundary completeness gate:** Core now compares each
installed runtime value in the document-text and document-context generated
profiles with a reviewed boundary record, matching stable ID, runtime ID,
capability, nonempty contract ID, and fixed failure category. Type-only
declarations do not cross a runtime value boundary. A focused regression
requires both actual snapshot profiles to pass, then removes the origin record
and adds a previously unrecorded runtime value; both mutations fail with a
typed `MissingRecord`. This catches a future snapshot binding added without
its contract inventory entry. The focused test passes with the shared target
cache. E1.3 will apply the gate during strict-runtime admission and add
separate diagnostics for missing, unreifiable, and unchecked boundaries.

**E1.3.1 strict-runtime refusal diagnostics:** The direct-page host now
checks its core-owned installed binding inventory before a strict-runtime
script can compile or capture a callback. Each record names a declared type
and whether the installed crossing validates before realm capture. A missing
or mismatched reviewed record, a type that cannot become a pure `ContractPlan`
(including `any`), and an unchecked crossing return separate typed
`MissingContract`, `UnreifiableType`, and `UncheckedBoundary` diagnostics.
The core inline-report adapter maps them to three different fixed categories
without serializing binding IDs or page source. A public `DirectPageScriptHost`
execution regression injects each faulty owner record under the same live
document-text profile and requires no program or debug attachment; a report
regression checks the three bounded category labels. Focused tests, Engine
Clippy, rustfmt, diff, and line-count checks pass with the shared target
cache. This is direct-page strict admission; standalone emitted strict-runtime
artifacts remain unavailable until the helper work in E3.

**E1.3.2 reviewed strict-runtime admission:** The direct-page gate now
requires each installed value record to match the core-owned reviewed binding
identity, direction, capability, fixed failure category, contract ID, pure
plan fingerprint, and the validator's actual owner-selected limits. A
reifiable but mismatched `number` plan or forged larger recorded limit cannot
authorize the installed string callback. A live loaded-page test successfully
executes both copied document-origin and document-text callbacks with
`RuntimePolicy::StrictRuntime`; a separately bounded host rejects an
oversized document-text value before any program or debug attachment. The
three focused strict-runtime tests pass, as do Engine Clippy, formatting,
diff, and the 1,300-line source audit, using the shared target cache. This
closes E1.3 for the currently installed immutable direct-page result
boundaries; emitted strict-runtime artifacts remain unavailable until E3.

**E2.1.1 validation precedes VM capture:** The current immutable snapshot
paths validate copied document text and canonical origin against their pure,
bounded core-selected contract plans before they call the BlueJS realm binding
registrar. The strict-runtime loaded-page regression now rejects an oversized
document-text result with no program, debug attachment, or bound host profile,
then installs a callback with the same global name in that exact still-live
realm. BlueJS rejects duplicate host globals, so successful registration
proves that the failed result never installed its callback or copied its value
into the VM. The focused test passes with the shared target cache; E2.1.2
will separately prove validation does not invoke active page behavior.

**E2.1.2 validation has no executable inputs or side effects:** The runtime
validator accepts `&ContractValue`, whose closed set of owned variants contains
only primitives, vectors, and ordered string-keyed maps. No variant can hold a
JavaScript object, getter, Proxy, callback, or host handle. The installed
document boundaries copy text and origin into Rust strings before building
that data-only value; the validator traverses it without VM access. At the
loaded-page boundary, a regression supplies an over-budget snapshot, an inline
script, and an external BlueTS module with a counted source authorizer. The
contract rejects both scripts with source-free reports, records zero source
authorizer calls, and sends no document to the child. Thus validation neither
invokes page code nor enters the source-fetch path. This proof applies to the
currently installed immutable snapshot bindings; future VM-object boundaries
must provide a separately reviewed, pure data-copy step before validation.

**E2.2.1.1 direct-page validation attribution:** The pure contract validator
now returns a source-free count of attempted value-node visits, including the
node that trips a fuel/depth/size failure. The direct-page host attributes each
validation attempt, node visit, and copied string byte to the exact live
`TabId` and document generation before returning a contract error. Its private
accounting record is cleared on replacement or close; it is not a page,
debugger, or MCP query. A two-tab public-boundary regression exercises a valid
text/origin pair alongside a rejected text result, checks distinct charges,
then navigates one tab and closes the other to prove scoped release. The
remaining E2.2.1 work is out-of-process snapshot attribution and retained
compiler/contract/debug cache accounting; this leaf does not claim those
costs are already charged.

**E2.2.1.2 out-of-process snapshot attribution:** Core now obtains a metered
result from its document-text and origin contract checks and records attempted
nodes and copied validation-value bytes under the initiating tab and exact
document generation. A contract failure retains its charge even though core
does not authorize external sources or send a document to the child. A two-tab
public-boundary regression checks separate successful and over-budget charges,
then verifies that navigation replaces only the first tab's record and closing
the second removes only its record. The cohesive `prepare_document` body lives
in the existing document-admission submodule; the production executor facade
is below the 1,300-line threshold. Retained compiler/contract/debug cache
bytes remain E2.2.1.3.

**E2.2.1.3.1 live-page cache ownership decision:** Account bounded retained
payload bytes, not allocator overhead or process RSS. Count each owned source,
metadata, contract-plan, and deferred graph copy once at the owner that retains
it; keep VM bytecode and heap in their existing realm statistics. The charge
belongs to the exact initiating tab and document generation and disappears
when that owner replaces or closes the document. A source cache hit within the
same document reuses its charge. A different tab or generation gets a separate
cache entry so a shared retained source is never left billed to a closed tab.
New retained-byte counters use checked arithmetic and fail admission or
caching on overflow.

| Current owner | Retention and identity | Required accounting |
| --- | --- | --- |
| Direct page compilation and primitive `HostBindingContractV1::plan` | The direct bridge calls stateless `compile`, and primitive plans are built for one validation then dropped. No live page compiler or primitive-plan cache survives the call. | Prove zero retained cache in the final audit; keep validation work in E2.2.1.1. |
| `DirectPageRealmOwner::debug_registry` | Static BlueTS source/type/symbol/contract plans and safe-point maps remain under live BlueJS program generations. The realm maps those generations to a tab. | Sum retained metadata payload for that tab, including contract plans; release on realm navigation/close. |
| `BlueJsChildHost` debug registry and deferred execution | Child-private BlueTS metadata and some source-bearing deferred module graphs remain under a document/program generation. | Include both in exact child realm accounting, carry only bounded numeric totals to core, and release with the document. |
| Core out-of-process debugger maps | Opaque program, metadata, linked, and nested records are already keyed by tab but have no byte charge. | Count retained payload per exact live generation and discard it on failed admission, replacement, or close. |
| `HttpOutOfProcessPageScriptSourceAuthorizer` verified cache | Up to 4 MiB of verified source payload is now keyed by URL/integrity/MIME and shared across documents; it has no tab owner or lifecycle release. | Partition entries by initiating tab/document, retain same-document hits, and add a core lifecycle release hook. Preserve manifest and response integrity checks. |
| `RegisteredProjectCompilerService::IncrementalCompiler` | Cache belongs to an owner-registered sealed compiler project, not to a page script or a page tab. | Keep it under the project's separate compiler-service limits; do not invent a tab charge until a page-owned project registration route exists. |

The retained-byte counter measures the concrete payload held by these owners;
it is not a claim about allocator metadata or VM heap. The remaining subleaves
implement each retained owner and close with a real-process two-tab audit.

**E2.2.1.3.4 core debugger maps:** Program and static-metadata inventories
each retain an outer tab entry and a child-to-public identity map with fixed
opaque keys and values. An execution-deferral entry is another fixed-size
core record. Active nested frames retain a fixed record, while a linked pause
also retains two child stack scope vectors whose capacities must be counted.
These maps are keyed by tab, so each query must additionally validate the
live document generation before summing logical map-entry payload. The child
already owns its own debugger metadata and stack; core counts only its copies.
Program/metadata/deferral identities and active-frame payload are separate
substeps, with replacement, failed admission, and close tests at the core
boundary.

**E2.2.1.3.4.1 core identity-map charge:** A checked exact-document query
counts logical outer and inner `BTreeMap` entry payload for core-minted
program and static-metadata identities plus the execution-deferral record.
It validates both the live document and deferral generation, reports zero for
a rejected successor with no debugger records, and returns no value for a
closed or stale document. A real launcher-child/core test shows program and
metadata discovery increase only one tab's charge; replacement, failed
snapshot admission, and close release the appropriate records. Active
nested/linked frames and their scope vectors follow in E2.2.1.3.4.2.

**E2.2.1.3.4.2 active core frames:** The exact-document core query counts
the fixed nested-frame map entry or linked-pause map entry and the capacity of
both linked child-stack scope vectors. It validates the retained core/child
frame tuple against the addressed tab and generation before returning any
charge. A combined query adds these allocations to program, metadata, and
deferral map entries with checked arithmetic. Real child/core nested and
linked tests observe nonzero active-frame charges, linked scope-vector bytes,
and zero after frame return or linked completion; navigation makes the old
generation unqueryable. No child-owned metadata or VM runtime state is counted
inside this core-only total.

**E2.2.1.3.5 verified HTTP source cache:** The cache key now includes the
initiating tab and document generation in addition to canonical URL, expected
SHA-256, and MIME lane. One document still reuses a verified response across
its declarations; a second tab or successor document performs its own fetch
and integrity check. Both page executors tell their private source authorizer
when a document is replaced or closed. The child route also releases an
attempted successor after failed acknowledgement or malformed realm stats,
and rejects old-generation accounting queries. The exact-document meter sums
only retained source lengths under the existing shared 4 MiB source-byte
budget, leaving key/allocator and completed graph copies outside this charge.
The fixed fixture profile forwards release and checked charges across its
per-origin inner authorizers. A real two-tab child/core HTTP test proves
same-document hits, cross-tab isolation, replacement and close release,
re-fetch after release, and rejection of a changed response; a direct route
test proves the same lifecycle callback contract. The cache implementation is
in its own module so the authorizer and child executor stay under 1,300 lines.
Partitioning could otherwise retain arbitrarily many empty-source keys, so
the cache also caps entries at 4,096 and advances the policy fingerprint to
`core-page-http-resource-authorizer-v3`.

**E2.2.1.3.6 final live-page cache recount:** Core exposes one checked
exact-tab/document sum of child static BlueTS metadata, child pending debugger
payload, core debugger maps and active frames, and verified HTTP source. The
direct route sums its exact-generation BlueJS debug/contract metadata and
verified source. Each component is owned by one layer and counted once; VM
bytecode/heap, validation work counters, and completed temporary graph copies
remain in their own accounting. A custom source authorizer without a retained
payload meter leaves the aggregate unavailable rather than reporting a false
zero. A two-tab launcher-supervised child test combines real external BlueTS
HTTP source with all three debugger/cache owners, then checks execution,
independent replacement and close, stale tuples, and failed snapshot
admission. Direct-route tests check metadata and source totals separately.

The live-page compiler has zero independent retained cache: each direct
declaration calls the stateless `compile_direct_script` or
`compile_direct_module_graph`, then passes its temporary artifact to the
realm attachment. Only copied realm debug metadata and VM state survive, and
those are already attributed to the realm. `DirectPageScriptHost` owns no
compiler or incremental-project field. Primitive host-boundary validation
likewise calls `HostBindingContractV1::plan()` inside
`validate_string_metered()` and drops that temporary plan after the call;
the retained compiler-generated contract plans inside debug metadata are
counted by the direct/child static meters. The sealed project compiler's
incremental cache belongs to its owner-registered project, which has no page
tab identity.

**E2.2.2.1 source-free policy/position report decision:** The existing
tab-addressed BlueTS outcome reports are the public seam. Core may add
only the owner-selected runtime policy label (`checked` or `strict-runtime`)
and an optional half-open byte range in the original *inline* BlueTS source.
For a compiler diagnostic, the owner must match its module ID to the exact
core-minted inline declaration identity, validate `start < end` and both
endpoints against that declaration's original source bytes, and retain only
the numeric range. The current precompile document-text/origin contract
crossings have no source call site, so their position is unavailable; the
report must not synthesize a script-tag or callback coordinate. External
source graphs can be owner-private and their module IDs/lengths may disclose
protected input, so their positions remain unavailable on this page-report
surface without a separate owner authorization. A missing, malformed, stale,
or mismatched span is omitted. Fixed outcome categories stay source-free; no
URL, module identity, source text, diagnostic prose, value, failure path,
expected/observed type, contract plan, or compiler option fingerprint enters
the report. The direct executor can validate before reporting; the child
route needs a private bounded position candidate and a second core check
against the original inline declaration before public serialization.

**E2.2.2.2 direct-page report route:** `DirectPageInlineExecutor` records its
construction-time checked or strict-runtime policy with each BlueTS outcome.
For a rejected inline script, it inspects only compiler or unsupported-target
spans, requires the exact core-minted module ID for that tab, document, and
ordinal, checks non-empty offsets within the original extracted inline UTF-8
source at character boundaries, converts them to bounded 32-bit numbers, and
discards the diagnostic, module ID, and source. Other failures, successful
scripts, and every external declaration have no position. The session maps
this private record to a typed tab-addressed `BlueTsScriptExecutionReport`
with a fixed policy enum and optional numeric range; the existing outcome
category stays source-free. Browser IPC advances to v2 because policy is a
required field. A live HTTP/session query proves the public checked-policy
and original inline range without source text; direct tests cover strict
policy, external omission, and malformed/mismatched spans, while IPC tests
round-trip the shape. The direct executor's tests now reside in their own
MPL-licensed submodule so the implementation file remains easy to extend.

**E2.2.2.3.1 private child candidate:** Page-host protocol v44 carries an
optional compiler-position candidate only on a rejected BlueTS script report.
The child selects compiler or direct-lowering spans from its independently
validated graph, requires a nonempty UTF-8 byte range within the referenced
module, and bounds the module ID and 32-bit offsets. JavaScript, successful
BlueTS, graph validation, and runtime failures carry no candidate. Immediate
and debugger-controlled synchronization use the same private field. Core
must still verify the candidate against its own exact inline declaration
before placing a numeric range in a browser report.

**E2.2.2.3.2 core verification and public route:** While an authenticated
document synchronization is pending, core retains the original inline BlueTS
declaration text and its ordinal only until the reply is processed. It first
requires the child report to name a script that core actually submitted for
the same tab and generation. It publishes a numeric position only for a
recognized compiler or direct-lowering rejection whose bounded private
candidate names the exact core-minted inline module and falls on UTF-8
boundaries within those original bytes. Core discards the private module ID,
source, and diagnostic before the tab-addressed report enters browser IPC.
External graph failures, malformed or stale candidates, and later debugger
execution replies have no public position; the child runtime policy is the
fixed checked mode. Real launcher-supervised two-tab, navigation, authorized
external failure, and core-binary/browser-IPC tests exercise these boundaries.

**E2.3.1.1 live-value boundary inventory:** The installed page contract
crossings are the core-owned document-text and canonical-origin results.
They copy Rust strings, run a pure bounded `ContractValue::String` validation,
then permit BlueJS callback capture. This real path can exercise accepted
strings, string-size limits, and selected node/work budgets. Its `string`
contract accepts every in-budget Rust string, so it cannot exercise a
type-malformed value. It also cannot receive an array, object, or cyclic
runtime reference, so a validator-only nested-value test cannot establish
the E2.3 live-boundary claim. The compiler and debugger validation protocols
accept a caller-supplied, data-only `CompilerContractValue` tree for static
plans; that protocol explicitly never describes a live BlueJS value, and its
owned recursive enum cannot encode a cycle. E2.3.1 therefore separates
string-boundary regressions from the work needed to identify or add a
reviewed structured runtime crossing for malformed, deep, and cyclic values.
The latter must preserve pure bounded
validation, explicit owner capability, and rejection before host/VM effects;
no test may claim cyclic/deep live coverage by fabricating an impossible
snapshot or calling the plan validator in isolation.

**E2.3.1.2 current string crossing:** The loaded-page direct host already
accepts a valid document-text result and rejects an overlong copied snapshot
before program admission. A zero-node owner-selected validation budget now
rejects even a short copied document result after accounting one attempted
node to that tab and generation; no BlueJS program or debug record is
installed. The inline page executor repeats overlong and exhausted cases
through the public report seam, returning the same fixed category without a
source position or protected text. These are distinct real string-value
classes. A malformed type, nested value, or cycle is not representable by
the installed string-only callback and remains in E2.3.1.3.

**E2.3.1.3 structured live crossing:** The existing owner-only paused
debugger-value preview is a real VM-to-child-to-core value boundary. It is
selected only by native debugger execution control and an exact live frame,
safe point, and scope slot; the public debugger still marks bounded values
planned. BlueJS reads stored own data directly, never invokes a getter or
proxy trap, and bounds depth, node count, container length, and copied bytes
before producing the private snapshot. A BlueTS page in a real child/core
socket test reaches that boundary with one initialized root value at a time:
a plain number crosses, while an array with a non-index own property, a
self-referential object, and an object deeper than the preview limit each
return only the fixed invalid-state refusal. Existing VM-level accessor tests
also verify no getter runs during preview. This uses an already reviewed
debugger capability rather than adding a page-visible structured host API;
E2.3.1.4 will make the six requested value classes independently visible in
the final regression gate and verify report redaction.

**E2.3.1.4 six-case gate:** Named loaded-page tests cover a valid copied
document-text value, an oversized pre-capture refusal, and a zero-node-budget
refusal charged to the initiating tab. Four independently named real
child/core BlueTS debugger tests share only their setup: a plain value is
copied from one exact paused root slot, while a non-plain array, a cyclic
object, and a value beyond the fixed depth limit are each refused before a
preview is returned. The malformed array contains a private marker, and the
fixed refusal and any page-script report omit it. No public debugger
bounded-value capability was enabled by these tests. These two reviewed
live crossings cover the six required classes without treating a static
compiler validation snapshot as a runtime object or duplicating the build
target.

**E3.2.1.1 emitted helper v1 decision:** The only reviewed live page
contracts today are primitive string results for document text and canonical
origin. The first emitted strict-runtime helper therefore supports that
closed subset: require a primitive string, reject unpaired UTF-16 surrogates,
and count its UTF-8 bytes under an owner-selected nonnegative integer limit.
It must use string indexing, length, comparisons, and arithmetic on that
primitive rather than calling replaceable global encoders, prototype
methods, getters, or callbacks. Unsupported object and array contracts are
not treated as validated strings. The helper is fixed UTF-8 source with an
MPL header and a stable `bluets.runtime-helper.v1.mjs` output name, written
in the same staged atomic publication as the emitted modules. E3.2.1.2 will
test that writer directly with strict metadata and verify exact bytes; the
public standalone strict-runtime build gate remains closed until the
manifest binds the helper identity and output-target checks are enforced in
E3.2.2–E3.2.3. A helper file alone is no proof that generated modules call
it at every required boundary.

**E3.2.1.2 helper staging:** The fixed MPL-licensed v1 ESM source exposes a
version label and one primitive-string validator. It accepts only a
nonnegative safe-integer byte budget, walks the string's own indexed UTF-16
units without calling mutable globals or prototypes, rejects unpaired
surrogates, and counts the equivalent UTF-8 bytes before returning the
original value. Every refusal throws one fixed source-free string. The
atomic BlueTSC publisher writes the exact embedded source as
`bluets.runtime-helper.v1.mjs` only for strict metadata; a checked rebuild
replaces the output directory and drops the helper. A direct publisher test
proves exact bytes and lifecycle, a CLI regression proves standalone strict
builds still refuse before publication, and direct Node checks cover Unicode
byte limits, invalid budgets, surrogate refusal, and a Proxy that receives
no property read. Manifest identity and generated-code retention gates remain
the next steps.

**E3.2.2 manifest binding:** Strict build metadata carries one optional
runtime-helper record with the v1 version, fixed relative output filename,
and a `sha256:` digest of the exact embedded helper bytes. The publisher
recomputes the expected record before staging and rejects missing, altered,
or policy-inconsistent metadata without replacing an existing output
directory. The direct publisher regression reads the serialized manifest,
compares all three fields to the written helper source, then proves a wrong
digest, missing strict record, and unexpected checked record all fail before
replacement. This binds artifact identity but does not yet prove that
generated modules retain calls to the helper; E3.2.3 owns that gate.

**E3.2.3.1 strict emitted-path audit:** The current compiler passes both
`checked` and `strict-runtime` as the same checking-enabled boolean into
`checker::check_incremental`. `emitter::emit` receives policy in options for
fingerprinting, but its `emit_javascript` path has no selected runtime
boundary record or helper-call insertion. Standalone `bluetsc` also builds
with `require_declared_global_calls: false`, so an undeclared ambient call
may enter emitted JavaScript. Thus the newly staged helper file and hashed
manifest prove artifact identity only; they do not prove that any boundary
was checked at runtime. The existing early strict-build refusal is the
correct current target gate. A supported strict emitted path requires an
owner-selected, reifiable boundary descriptor tied to each source crossing,
deterministic helper imports/calls at those sites, and a publisher audit that
refuses any module/target that loses a required check. E3.2.3.2 implements
the crossing route; E3.2.3.3 proves target retention and opens only covered
strict output. No module is considered safe merely because its manifest
names a helper.

**E3.2.3.2.1 bounded BlueTSC owner file:** Before adding emitted-boundary
metadata, the binary's existing Rust unit tests moved to an MPL-licensed
`bluetsc/tests.rs` submodule. The command-line owner now holds only the
publisher and policy code (933 lines instead of 1,221), while its 291-line
test module retains the same 11 cases. This keeps the implementation below
the repository's 1,300-line maintenance threshold as strict output work
continues and uses the same build target.

**E3.2.3.2.2 first emitted crossing contract:** The first standalone strict
profile admits only a directly named `export function` with required primitive
`string` parameters, an explicit `string` result, and one unconditional
`return` expression. That expression may use only those parameters, primitive
string literals, parentheses, and string concatenation (`+`). The checker
must reject every other statement or expression shape, including calls,
property access, object creation, assignments, fallthrough, overloads,
generics, optional/default/rest parameters, and `async` functions. No runtime
import, ambient declaration, global reference, default/value re-export, or
other runtime declaration is admitted in a strict module. Type-only imports
and declarations may remain because they erase; each source module in a
multi-module graph is checked independently. This intentionally small
language ensures there is no unreviewed foreign value between the two
observable crossings: JavaScript caller to each parameter, and the function
result back to JavaScript. Both crossings must call helper v1, including when
the body returns its input unchanged. A module with no selected crossing is
refused rather than receiving a strict label by vacuity.

The owner selects each crossing through a build configuration descriptor, not
through TypeScript source text. A descriptor identifies the canonical
root-relative module, exported function name, exact UTF-8 byte span of its
declaration, stable contract ID, nonnegative safe-integer string-byte budget,
and the fixed `bluets-runtime-helper-v1` version. The same descriptor applies
the budget at every parameter ingress and result egress; a later profile may
add independent budgets only with a new descriptor version. The compiler
requires exactly one descriptor for every emitted function and rejects stale,
duplicate, missing, unused, out-of-graph, or conflicting descriptors. It
checks the parser's exact span and signature before emission, includes the
ordered descriptor data in the project fingerprint, and retains an ordered
checked-boundary record with the output. The CLI accepts these records only
from its owner-read config, not from a source annotation or imported module.
Direct compiler callers must also supply the descriptor explicitly. Missing
descriptor data never silently selects a default budget.

Emission uses one fixed helper import alias and path per module, validated
against source identifiers to prevent capture. It inserts a validation call
for every parameter immediately inside the function body and wraps the
return expression in a validation call at that return's exact source span.
Only these inserted calls can support the strict claim. Source-map provenance
maps inserted code to the corresponding parameter/body or return expression;
the publisher verifies the checked-boundary record, helper version, imports,
and retained calls before replacing the previous build. Both ES2020 and
ES2022 must pass that gate. A stronger checker or different target cannot
turn an unsupported crossing into a successful strict artifact merely by
erasing its syntax. Until all of these checks run, standalone strict build
continues to refuse before publication.

**E3.2.3.2.3.1 descriptor plumbing:** `bluetsc.json` now accepts an owner
`strictBoundaries` array with root-confined source paths, exact declaration
byte spans, contract IDs, function names, UTF-8 byte limits, and helper
versions. The compiler option carries the same records and fingerprints every
field in owner order. Before emission, it rejects records that do not match
an exact named exported function, repeat a contract ID or function, select a
different policy/helper version, or exceed helper v1's safe-integer limit.
Direct page hosts leave the emitted descriptor list empty because their live
boundary inventory is a separate owner-selected route. This leaf only binds
descriptors; emitted JavaScript still has no inserted helper calls and the
standalone strict build refusal remains in force. Two focused compiler tests,
one config path/span test, all 158 BlueTS library tests, workspace Clippy,
and formatting pass against the reused target directory.

**E3.2.3.2.3.2 strict shape admission:** A descriptor-selected compilation
now admits only modules whose runtime declarations are all matching named
exported functions. Each such function has 1–16 required primitive-string
parameters, an explicit string result, and exactly one unconditional return
made from those parameters, string literals, parentheses, and `+` (at most
128 expression tokens). Each owner budget is capped at 1 MiB as well as
helper v1's safe-integer range. The parser retains the `async` marker so a
Promise-returning declaration cannot masquerade as a primitive-string
crossing after type erasure. Ambient declarations, runtime imports, uncovered
modules or functions, default/value exports, extra runtime declarations,
opaque bodies, global calls, property reads, optional/default/rest
parameters, and unsupported expressions fail with `InvalidContract` before
output. Type-only imports to `.d.ts` modules remain erasable. The first
profile requires one CLI entry so descriptors cannot be misinterpreted as
stale when independently compiling multiple entry graphs. Focused
admission/refusal tests, 161 BlueTS library tests, 12 BlueTSC binary tests,
workspace Clippy, and formatting pass in the reused target directory.
Generated JavaScript is still uninstrumented, so standalone strict build
remains closed.

**E3.2.3.2.3.3.1 emitted record shape:** Strict descriptors and every
runtime module now require canonical root-relative `.ts` IDs, matching the
standalone owner's confined project paths. Source containing the reserved
`__bluetsValidateStringV1` alias refuses before emission, including comments
and longer identifiers, so later generated-call site scans cannot confuse
source text with inserted calls. `BuildArtifact` now has an optional strict
module record with helper version/import site and per-contract ingress/egress
source and generated byte sites. It remains absent until helper calls are
actually inserted. The six focused descriptor tests, 162 BlueTS library
tests, 12 binary tests, workspace Clippy, and formatting pass in the reused
target directory; standalone strict publication is still closed.

**E3.2.3.2.3.3.2 helper-call insertion:** The parser now retains each
function body's opening brace byte so the emitter can insert ingress checks
immediately inside it. For each admitted exported function, the emitter adds
one deterministic root-relative helper import per module, validates every
required string parameter, and wraps the sole return expression in the same
versioned helper. Its strict record holds the owner contract ID and budget,
the original parameter/expression spans, and generated offsets of every
exact call. A source alias collision was already rejected; after applying
all ordinary type-erasure edits, emission scans generated calls in source
order and returns `InvalidContract` without output if any import or call is
missing, changed, or unexpectedly duplicated. Compiler emission now returns
a diagnostic rather than treating such a mismatch as success. The focused
two-parameter concatenation test asserts the helper path, exact ingress and
egress text, and retained generated sites. All 162 BlueTS library tests,
12 BlueTSC binary tests, workspace Clippy, and formatting pass in the reused
target directory. The public strict build gate remains closed pending
source-map, multi-module, and publisher verification.

**E3.2.3.2.3.3.3.1 emitter test extraction:** Before adding more strict
emission regressions, the emitter's 15 existing tests moved unchanged into
the MPL-licensed `emitter/tests.rs` submodule. The owner file fell from
1,204 to 787 lines and the tests occupy 420 lines. All 15 focused emitter
tests pass using the same target directory. The following leaf adds the
strict crossing and provenance cases in the bounded test module.

**E3.2.3.2.3.3.3.2 emitted crossing proof:** Inserted multiline helper code
now contributes a source-map segment at each generated line start, anchored
to the function body's opening source byte. The return wrapper maps its
generated call directly to the return expression's source byte. The
pre-emission source-map bound includes strict insertions and their newlines;
the emitter also checks the actual segment count before returning an
artifact. A focused test decodes mappings for two ingress calls and one
egress call, and proves a cap that permits the checked source refuses the
additional strict segments. Another test compiles a two-module graph linked
through an erased type-only import and checks that each module retains its
own root-relative helper path and exact recorded calls. A typed wrong
parameter fails with `InvalidContract` at the function span and no output.
All 165 BlueTS library tests, 12 BlueTSC binary tests, workspace Clippy, and
formatting pass in the same target directory. This closes the supported
crossing insertion leaf; strict standalone publication remains closed until
E3.2.3.3 audits target retention and publisher/manifest consistency.

**E3.2.3.3.1 target retention:** The admitted emitted profile uses only ESM
named function exports, primitive string parameters, string literals,
parentheses, and string concatenation. Its generated validation calls and
static helper import are valid in both declared targets without syntax
downleveling. A focused ES2020/ES2022 test compiles the same owner descriptor
with source maps and proves byte-identical JavaScript, source maps, and
strict crossing records while the target-bound fingerprints differ. This
proves compiler retention for the current narrow profile; publisher
consistency and public strict build admission remain separate gates.

**E3.2.3.3.2.1 manifest boundary inventory:** Build metadata now serializes
each owner-selected strict contract as `strictBoundaries` with its stable ID,
root-relative source module, function name, exact declaration byte range,
fixed UTF-8 byte limit, and helper version. It preserves owner order, which
already participates in the build fingerprint. Checked output with no
selected strict boundaries omits this field. A focused config/binary test
checks every serialized descriptor field; the existing checked publisher
test asserts omission. The publisher must still compare these claims to
generated code before any strict publication.

**E3.2.3.3.2.2 publisher audit:** Strict manifests now add a sorted
`strictArtifacts` inventory with SHA-256 of each compiler-emitted JavaScript
module, serialized source map, and declaration text. The publisher recomputes
that inventory before staging, alongside the already pinned helper hash. It
also requires the exact language and ES2020/ES2022 target, one entry matching
an emitted module, one consistent project fingerprint, matching requested
map/declaration modes and declaration-module inventory, one exact helper
import per module, every manifest-owned boundary, and every recorded helper
call at its generated byte offset. It refuses missing or extra call markers,
changed source spans/budgets, noncanonical module IDs, and any strict claim
on checked output. The audited JavaScript digest covers the emitted module
before the publisher appends its deterministic source-map URL comment; map
and declaration digests cover exactly the staged payloads. A direct
publisher test first creates a valid strict artifact, then mutates metadata,
helper identity, module bytes (including non-call code), import, call, map,
or record and proves every refusal leaves the previous JavaScript and
manifest intact. All 13 BlueTSC binary tests and workspace Clippy pass with
the reused target. Public CLI strict build admission remains E3.2.3.3.3.

**E3.2.3.3.3 strict CLI admission:** `bluetsc build` now requires an explicit
owner `strictBoundaries` configuration for strict-runtime. Missing records
still fail before compilation; stale, unsupported, ambient, or unpaired
records fail in compilation with no output; only the fully admitted
primitive-string profile reaches the pre-staging publisher audit. A real
BlueTSC process test builds both ES2020 and ES2022 with helper v1, source
maps, declarations, exact boundary and artifact identities, and two retained
calls. Where Node is installed, it imports each published module, accepts a
valid Unicode string, and rejects a number, oversized string, and unpaired
surrogate with the fixed helper refusal. The test then updates the descriptor
to the current source span but changes the body to call an ambient global;
the CLI reports `BTS4000` and preserves the previous JavaScript and
manifest. The previous no-descriptor strict build regression still refuses.
All 166 BlueTS library tests, 13 BlueTSC binary tests, 7 CLI tests, 18 CLI
coverage tests, workspace Clippy, and formatting pass in the same target
directory. E3.2's helper, manifest, target, and publication gates are closed;
E3.3 still requires direct-page/emitted malformed-value parity.

**E3.3.1 malformed string parity:** One Engine test sends the exact
`ABCDEFGHI` value through a strict direct-page document-text snapshot with
an owner-selected eight-byte limit. Core returns its typed
`dom.document-text`/`core-script-document-text-result-v1` contract violation
before realm capture, identifying nine observed bytes and retaining no debug
record. The same test compiles a strict ESM `echo` crossing with an eight-byte
owner limit and, where Node is installed, executes the emitted module with
the exact same value. Helper v1 rejects it with its fixed source-free
refusal. Both paths therefore reject the same malformed primitive and
budget; their outward error formats remain appropriate to their hosts.
The 337-test Engine library suite passes with local HTTP/socket access,
alongside workspace Clippy and formatting. The same target directory was
reused; E3.3.2 still checks weaker artifact claims.

**E3.3.2 weaker artifact refusal:** The public BlueTSC CLI regressions now
assert that `checked` and `transpile-only` manifests omit the helper,
`strictBoundaries`, and `strictArtifacts`, and that their emitted JavaScript
contains no strict helper call. A direct publisher regression compiles both
weaker policies, then forges strict metadata with matching recalculated
artifact digests and project fingerprints. Both attempts still fail because
their modules lack emitted strict boundary records, and the previously
published strict JavaScript and manifest remain byte-identical. All 13
BlueTSC binary tests, 18 CLI coverage tests, workspace Clippy, and formatting
pass in the reused target directory. Together with E3.3.1's same-value
runtime refusal, this closes E3.3 for the first supported string profile.

**E2.2.1.3.3.2 deferred child records:** The pending debugger queue stores
JavaScript module source graphs alongside parsed program ASTs, while BlueTS
modules retain bridge attachments with their own module maps. The source-graph
heap payload is counted first, including vector capacity and owned strings;
the parsed AST, BlueTS attachment, and inline pending-record payload require
separate checked counts before the combined deferred total is exposed to core.
Execution pops a completed record; document replacement and close drop its
entire queue. The source-graph-only query must not be presented as the full
deferred charge.

**E2.2.1.3.3.2.1 source-graph charge:** `PageHostModuleGraph` computes the
checked heap payload of its vector capacities and owned strings. The child
query selects only pending JavaScript module graphs for an exact live
tab/document and sums their payload; the graph's inline header remains for
the later pending-record count. A two-tab child-host regression demonstrates
that larger source content costs more and that execution, document replacement,
and close release only the affected pending graphs. No private wire field or
core charge is claimed by this step.

The two remaining nested payloads use separate owners: BlueTS module
attachments contain provenance spans, safe-point maps, root slots, and an
ordered module map; pending JavaScript programs contain parsed ASTs and their
module-ID map. Their recursive storage shapes differ, so E2.2.1.3.3.2.2 is
split into attachment and AST counters before the combined queue count.

**E2.2.1.3.3.2.2.1 BlueTS attachment charge:** A checked bridge meter counts
each program attachment's provenance vector and source-module strings, its
safe-point-map copy, and its root-slot vector. The module-graph meter adds the
entry attachment and every separately retained map attachment and module ID;
map-entry payload is counted without claiming allocator-node overhead. The
child query sums only pending BlueTS module attachments for the exact live
document. A two-tab regression verifies content sensitivity and release after
execution, replacement, and close. The separately retained static debugger
registry record remains under E2.2.1.3.3.1, so no cross-owner duplication is
introduced.

**E2.2.1.3.3.2.2.2 JavaScript AST charge:** BlueJS owns a separate recursive
meter for structured programs. It counts each syntax vector's retained
capacity, each owned UTF-8/UTF-16 string, each boxed node, all nested
import/export/statement/expression payloads, and one copy of each shared
`SourceText` allocation within a program. Opaque `BigInt` storage contributes
its normalized magnitude digits because the dependency does not expose spare
capacity. The child adds module-map entry payload and module IDs only for
pending JavaScript modules in the exact live document. Tests show larger
nested syntax raises the charge and that advance, navigation, and close drop
the affected tab's pending ASTs; bytecode and source graphs stay separate.

**E2.2.1.3.3.2.3 complete deferred queue:** The child sums the
`VecDeque`'s retained slot capacity and the separately counted JavaScript
source graphs, JavaScript parsed AST/module maps, and BlueTS module
attachments for one exact document. The slot allocation includes each
pending record's inline fields. Advancing execution shrinks the queue after
completed records are popped, releasing an empty queue's reserved buffer.
A mixed-language two-tab regression checks the sum and independent execution,
replacement, and close release. E2.2.1.3.3.3 may now transport a bounded
numeric total to core without exposing source or AST data.

**E2.2.1.3.3.3 private accounting route:** The existing exact-generation
`GetRealmStats` reply already carries program, bytecode, and VM heap totals
to core. Private v43 adds mandatory numeric static-metadata and deferred-queue
payload fields to that one reply. Each field and their checked sum must remain
below a fixed 4 GiB per-realm transport ceiling; larger or conversion-failed
measurements are refused. The child populates both fields only after exact
document validation. Core admits the whole v43 record only when its tuple
and bounds validate, then discards it on replacement or close. Wire shape,
child owner production, and real-process core reconciliation are separate
reviewable steps.

**E2.2.1.3.3.3.1–2 v43 shape and child production:** The two new realm-stats
fields are required during deserialization and have no source or AST content.
Their checked sum must fit the 4 GiB envelope before core caches the exact
tab/document tuple. The child now obtains static metadata and complete
deferred-queue charges from its exact-document owner queries, converts them
to `u64` without truncation, and refuses any invalid reply. A local mixed
two-tab test compares wire values with owner totals and observes deferred
release after execution; IPC and core tests reject missing or excessive
fields and preserve valid nonzero charges only for the live generation. The
real-process child/core reconciliation is recorded in E2.2.1.3.3.3.3 below.

**E2.2.1.3.3.3.3 real child/core reconciliation:** Core re-queries and
validates the exact realm-stats tuple after every successful debugger advance,
so execution release no longer leaves a stale cached deferred charge. An
unsupported test transport clears any prior cache rather than preserving a
number it cannot refresh. A launcher-supervised real child with two opted-in
BlueTS tabs confirms nonzero static/deferred fields cross private v43 into
core, deferred bytes fall to zero after execution, one replacement leaves the
other tab's record intact, stale child generations refuse, close drops the
addressed tab, and an oversized failed snapshot admission retains no cached
or child realm. No new public debugger, page, or MCP field is exposed.

**E2.2.1.3.2 direct-page retained metadata charge:** BlueTS now computes a
checked owned-heap payload for static source/type/symbol records and nested
reifiable contract plans. The bridge adds its safe-point map, breakpoint spans,
root slots, and inline retained record. Each `DirectDebugRegistry` entry stores
the measured charge before admission; overflow refuses retention. The direct
page realm owner sums only live program handles in the requested tab, while
bytecode and VM heap remain in their separate runtime statistics. A two-tab
public-boundary test proves a longer retained contract/symbol source costs more,
navigation zeroes only the replaced tab, and close removes only its owner.
The module-graph regression sums both program records and observes zero after
pruning; a compiler metadata test confirms retained contracts add bytes. The
metric counts vector and string capacities plus logical map-entry payload,
excluding allocator metadata and BTreeMap node overhead. All affected files
are below 1,300 lines. Child, core debugger-map, and verified-source charges
remain in the following subleaves.

**E2.2.1.3.3.1 child static metadata charge:** The child now checks the exact
tab/document tuple, obtains that realm's live BlueJS program handles, and sums
only their checked `DirectDebugRegistry` payload charges. It does not expose a
new page-host protocol field yet. A two-tab child-host regression executes
different BlueTS interfaces/symbols, observes independent content-sensitive
charges, rejects the old generation after replacement, and sees zero in the
new empty realm while the second tab retains its charge. Closing the second
tab removes its charge. Deferred source graphs and the private numeric
core/child accounting field remain the next child subleaves.

**C3.1.3.2.1 root declaration-slot evidence:** BlueJS bytecode now records a
compiler-resolved root-scope slot in root statement order only for a
single-identifier variable or named function declaration. The compiler takes
the slot from its own root lexical table, not a post-hoc search of binding
names or an instruction span. Nested blocks, expressions, and compound or
destructured declarations have no single-slot evidence. Repeated `var`
declarations may name the same slot; this is deliberately visible evidence
that a later BlueTS symbol join must reject as ambiguous, not a license to
choose either declaration. The direct bridge's bytecode equality check also
includes this new compiler metadata. This leaf does not yet join BlueTS
symbols or types, retain child mappings, or expose a debugger request; those
are separate ordered leaves.

**C3.1.3.2.2 nested/captured exclusion:** The current BlueTS checker mints
`DebugSymbol` records for module-level declarations, not function parameters
or function-body/branch locals. Even when a BlueJS child code unit captures a
root binding cell, that captured cell is not an active child `Scopes` slot;
the paused stack lists it only in the still-active parent frame. A focused
BlueTS inventory test and a real paused BlueJS child snapshot establish both
boundaries. The first static-scope relation therefore joins only checked
top-level declarations to exact root-code-unit slots. It must not attach a
top-level SymbolId to a same-named child local, parameter, or captured slot;
future local-symbol metadata would require its own compiler provenance and
grant review. A parent frame shown alongside the child can still be queried
through its own active slot and exact frame receipt.

**C3.1.3.2.3.1 binding-layout attachment guard:** An already-installed
BlueJS program could previously pass direct attachment when its instruction
bytes, constants, root statement offsets, and root declaration slot ordinals
matched but its binding name differed. The bridge now also requires exact
compiler-owned binding metadata, scope membership, capture ordering, dynamic
eval slots, global function names, and variable-scope identity for every
recursively compared code unit. The BlueJS API reveals only a boolean layout
comparison, not names or runtime values. A regression deliberately installs
same-shaped code under a forged matching source identity and proves attachment
refuses before any BlueTS symbol-to-slot join is built.

**C3.1.3.2.3.2 checked root join:** Each direct attachment now builds a
static-only root-slot inventory from the exact installed bytecode's compiler
slot record, the corresponding structured root statement, the BlueTS lowering
span/location, and the compiler's symbol/source/type IDs. A candidate must be
one single-identifier variable or named function declaration of the matching
name/kind, with exactly one symbol at that span, one symbol ID, one type ID,
the matching compiler-options/language/source set, and one owner of the
lexical slot. Erased type-only declarations, local/parameter/captured slots,
duplicate `var` slot owners, and absent or inconsistent evidence produce no
entry. Each entry already carries its installed program and root code-unit
identity but does not assert any runtime value or expose a public debugger
request. Classic and module tests exercise valid declarations plus malformed
name, kind, span/location, source, type, fingerprint, and duplicate evidence;
the next leaf proves cross-generation and linked-program isolation explicitly.

**C3.1.3.2.3.3 live generation isolation:** The attachment no longer exposes
its static slot list through an unchecked getter. Reading it requires the
BlueJS program registry to confirm the handle is still live, the attached
safe-point map still belongs to that generation, and every slot's program and
root code-unit IDs match the installed program. Two identical classic
installations yield distinct generation/code-unit IDs; invalidating one does
not affect the other, while a moved handle refuses. In a two-file linked
module graph, each source's top-level variable maps only to its own module
generation. Swapped module attachments refuse; navigation invalidates both.
The bridge still does not retain this relation in the child or expose it on a
debugger wire.

**C3.1.3.2.4.1 retained live slot map:** The direct debug registry now copies
only a live-validated root-slot join alongside the corresponding static
metadata, safe-point map, and breakpoint spans. Retention rejects a forged
slot ordinal or moved generation before installing any partial record; the
registry's lookup checks the live BlueJS handle and revalidates the root
code-unit/slot layout before returning IDs. The page-realm owner exposes the
same checked lookup, so navigation invalidates old slot results even after a
new generation of identical source is installed. No child debugger request or
public wire is added in this leaf; supervised-child call sites and forged
handle/source/type denials remain in the next leaf.

**C3.1.3.2.4.2 supervised-child scope provenance:** `BlueJsChildHost` now has
a child-private lookup that requires the exact live document generation,
child-minted program tuple, and metadata handle before consulting the direct
debug registry's checked root-slot map. Existing metadata inventory and
summary paths use the live-map check without adding a page-host or public
debugger request. Classic-script tests reject a JavaScript sibling, forged
program generation and metadata handle, and the old document after cutover;
two-file linked-module tests reject swapped metadata and both old handles
after realm close. This lookup accepts no source or type ID from a caller, so
there is nothing to guess or partially disclose. A later private static-scope
request (C3.1.3.3) must separately validate any such selector before it can
return an opaque static relation; no runtime value is produced here.
The full Launcher library suite (112 tests) passes. Two older source-debugger
tests were also corrected to select the next *distinct source declaration*
from the per-instruction safe-point map; their former `entries[1]` assumption
mistook another instruction in the first declaration for the second statement.

**C3.1.3.3.1 private static-scope relation contract:** The child/core route
has two distinct exact-pause targets because the existing ordinary `Scopes`
snapshot cannot represent a linked dependency/entry pause. The ordinary
target carries the existing exact active-slot selector shape
(`PageHostDebuggerValueTarget`) plus the selected program's child-minted
metadata handle. It may resolve a root frame, including the still-active
parent root of a nested pause; the child frame's locals and captures remain
unbound because the current BlueTS inventory has no matching SymbolId. The
linked target carries the child-minted linked frame, its complete expected
two-program stack with the original scope-entry budget, the selected frame
index, the selected program's metadata handle, and one exact active slot.
The first supported linked relation selects the entry root at index one;
index zero is a dependency child, not an active dependency root declaration.
The child reacquires and compares the complete stack before using any slot.

One private reply echoes the entire target and returns only the checked
opaque `symbol_id` and `type_id`. No source/name/type display, runtime preview,
runtime type tag, or optional partial result is serialized. Malformed IDs,
unreceipted/inactive slots, wrong metadata owner, moved safe points or linked
stacks, stale documents/programs, and missing/ambiguous compiler joins return
a typed error instead of a relation. A private page-host version bump belongs
to the leaf that adds request/reply variants; public debugger v40 remains
unchanged until C3.1.3.4 supplies an independent static-scope grant and
same-stream receipts. That public leaf must also provide a linked-stack-derived
scope receipt for index one; the ordinary `Scopes` receipt cannot be silently
reused for linked pauses. The existing `Value` route and grant remain separate.

**C3.1.3.3.2 private v41 wire:** The page-host protocol now has a bounded
`StaticScopeTarget` for either an ordinary root (including the parent root of
an exact nested pause) or a linked entry root with the complete two-program
stack. The structural validator requires a live-shaped metadata handle,
root-only frame index, non-truncated linked stack within the original scope
budget, and exactly one selected entry slot. It rejects malformed frame and
program pairings before any relation can be returned. The static-only reply
echoes the entire target and contains only compiler symbol/type IDs; no VM
preview, runtime tag, type display, name, or source is present. The Launcher
intentionally returns `InvalidDebuggerState` for this new request until the
ordinary and linked child handlers land in C3.1.3.3.3–4. Public debugger
remains v40 with no new grant. Both ordinary and linked socket round trips,
negative shape cases, the staged child denial, the full IPC (116) and Launcher
(113) library suites, workspace Clippy, and formatting checks pass.

**C3.1.3.3.3 ordinary child relation:** For an ordinary root pause or the
parent root of an exact nested invocation, the child reacquires the current
two-frame-capable stack without reading a VM binding. It rejects any stack or
scope truncation, a missing/moved root safe point, a changed nested invocation,
and a selected slot that is absent, duplicated, or at a different lexical
depth. Only then does it use the exact document/program/metadata-owned,
generation-checked root-slot map and return that slot's compiler symbol/type
IDs. Child-local and capture frames remain ineligible; linked targets still
return a typed error. The classic-root and nested-parent tests include
forged slots, scope depths, safe points, invocation serials, metadata, missing
realms, and document cutover without a partial reply. The full Launcher
115-test suite, workspace Clippy, and formatting pass; public debugger v40
and the independent `Value` route are unchanged.

**C3.1.3.3.4 linked child relation:** A linked static-scope request first
validates the exact child-minted dependency/entry invocation. The child then
reacquires a complete two-program stack using the caller's original bounded
scope budget and compares the entire safe-point/slot snapshot, not merely the
selected entry frame. Only the entry root at index one may join to the entry
program's live, metadata-owned retained root-slot map; the dependency child
and its captures cannot borrow that metadata or map. Swapped program or
metadata identities, forged invocation/slot, moved dependency or entry safe
points, and stale document cutover all produce typed errors without partial
symbol/type IDs. The full Launcher 116-test suite, workspace Clippy, and
formatting pass. Public debugger v40 and `Value` remain unchanged.

**C3.1.3.3.5.1 core/adapter split:** Keep the core-facing target separate
from the page-host wire. The ordinary form carries the existing core-owned
paused `ValueTarget` selector plus the independently reminted metadata
identity; this is only a slot selector, never a request for a value. The
linked form carries the exact core-reminted two-frame pause snapshot, entry
metadata identity, entry-root index one, and selected lexical slot. Its
stored child stack includes the complete scope entries and original budget;
core must reacquire and compare it before constructing the private child
target. A result contains only compiler symbol/type IDs tied to the input
target. The page-host adapter is default-deny for test doubles, validates the
whole request before sending, and accepts only a structurally valid reply
whose entire target exactly echoes the request; an error or any changed field
has no partial relation. The out-of-process executor separately resolves
core program, nested/linked frame, document generation, and metadata handles
to current child identities, then verifies the selected slot against a fresh
active ordinary or linked stack. A private core debugger helper can consume
that result only after its own live pause and scope checks. Same-stream
ordinary `Scopes` and future linked-stack-derived scope receipts, type-ID
receipt, owner/client grant, and public protocol bump belong to C3.1.3.4;
this split adds none of those public authorities and never calls `Value`.

**C3.1.3.3.5.2 private transport adapter:** `PageHostClient` now defaults the
static-scope operation to unsupported; only the authenticated page-host
connection sends its v41 request. The staged child adapter refuses malformed
targets before transport and accepts a relation only when the full echoed
ordinary or linked target is structurally valid and byte-for-byte equivalent
in its typed fields. Changed metadata, safe points, an unrelated reply, or a
child error never yields partial symbol/type IDs. Transport-double tests cover
both pause shapes, including no send on malformed input. After rebuilding the
workspace's child executable (the prior binary rejected v41 as an old
protocol), the real Launcher/Core integration case and full engine 306-test
library suite pass. Workspace Clippy and formatting pass; no public debugger
request, capability, receipt, or protocol change was added.

**C3.1.3.3.5.3 ordinary core remap:** The core-facing static-only target and
relation types carry only core-owned program/frame/metadata identities and
opaque compiler symbol/type IDs. For an ordinary root or nested parent-root
slot, the out-of-process executor reacquires the full current stack through
its existing live document/program/frame checks, rejects truncation, checks
the exact root safe point and one unambiguous active lexical slot, then maps
the core program and metadata generations to their live child identities.
The private adapter demands a complete exact echo before core remints the
two compiler IDs; it never reads `Value`. Tests cover successful root and
nested-parent mapping, child-local refusal, stale/moved frames, forged slot
and scope depth, sibling-owned metadata, stale program/document, truncation,
and altered child replies. Linked targets remain denied pending the next
leaf. The engine 308-test library suite, workspace Clippy, and formatting
pass; the public debugger protocol and grants are unchanged.

**C3.1.3.3.5.4 linked core remap:** A core-owned linked selector names the
two reminted frames, entry-root index one, one active lexical slot, and its
static metadata generation. The out-of-process executor requires the stored
complete child-first stack and both current core-to-child program mappings,
then reacquires the same linked invocation using the original scope budget.
It compares the entire private stack, not just frame safe points, so scope
drift at unchanged offsets cannot turn an old selector into a new relation.
Only the entry program's metadata can reach the strict private adapter, whose
full-target echo is required before reminting the opaque symbol/type IDs.
The linked core regression covers success and dependency-owned metadata,
swapped/moved public frames, forged slot/index, stale document, private scope
drift, moved invocation, and changed child echo. The engine 308-test suite,
workspace Clippy, and formatting pass. Neither the public debugger wire nor
its receipt/grant authority changes here; a shared private core helper remains
the next leaf.

**C3.1.3.3.5.5 private core staging helper:** Before any future public
dispatch, the debugger session can resolve one exact live realm and request a
static-only relation through a private helper shared by ordinary and linked
targets. The ordinary branch reacquires a complete root/parent stack and
requires one unique active lexical slot at the expected root safe point. The
linked branch validates the reminted two-program frame shape and reacquires
the exact two-frame pause; the out-of-process executor then performs the
complete private scope/slot recheck before the child adapter's exact echo.
The session helper also refuses a changed core-target echo, so no partial
relation is exposed on stale or forged input. Tests cover both successful
shapes, forged/duplicated ordinary slots, stale realm generation, moved linked
stack, and altered echo. The engine 309-test suite, workspace Clippy, and
formatting pass. This staging helper is not wired to a public debugger
request, capability, or grant; those remain C3.1.3.4 work.

**C3.1.3.3.6.1 classic/module root boundary:** The real Launcher-supervised
BlueJS child now proves that an initialized BlueTS root binding in both
classic and module script kinds can pass through the private v41 relation,
the child adapter's exact echo, and the core-reminted program and metadata
identities. The returned relation echoes only the static core target and
opaque compiler symbol/type IDs. Replacing the document makes the same
target fail with no partial relation. The focused real-child test passes;
public debugger routing and grants remain unchanged.

**C3.1.3.3.6.2 nested/linked boundary:** Real Launcher-supervised classic
and module nested pauses now verify the initialized parent-root binding's
static relation through core reminting, while selecting a child-local slot
for that root-owned metadata fails. A real linked dependency/entry module
graph reacquires the full private linked scope budget, relates the entry
root's active slot through its entry-owned metadata, and refuses dependency
metadata or frame index zero. The two focused child/core socket tests pass;
neither shape adds a public static-scope request or grant.

**C3.1.3.3.6.3 no-partial closure:** Against the real Launcher-supervised
child, ordinary nested-parent and linked entry-root relation targets now
refuse forged metadata generations, forged slot ordinals, and moved safe
points. Resuming the respective child/dependency invalidates the old paused
target; replacement documents also refuse both shapes. Each refusal is a
typed error with no symbol/type partial payload. The full engine 309-test
suite, workspace Clippy, and formatting pass, closing the private child/core
static-relation milestone C3.1.3.3. Public same-stream receipts and the
independent owner/client grant remain C3.1.3.4 work.

**C3.1.3.4.1 public static-scope contract and sequence:** Stage only
data-only validators and private core checks under public debugger v40. The
complete public route will use an ordinary selector consisting of an exact
`DebuggerValueTarget` from `GetScopes` plus its same-program opaque metadata
handle, or a linked selector consisting of the complete child-first reminted
`DebuggerLinkedStackSnapshot`, entry-root index one, one selected lexical
slot, and entry-owned metadata. A new `GetLinkedScopes` request under the
existing owner-selected LinkedModules and Scopes availability will take an
exact expected linked stack and bounded scope budget, reacquire the whole
pause, and return only the entry-root active slot/depth list with that stack
echoed. It grants no value read. Only a successful complete reply creates a
distinct same-stream linked scope receipt for those exact stack/slot tuples;
ordinary `GetScopes` creates ordinary receipts. Both receipt sets are bounded
and tied to the core-owned pause incarnation; successful arm/step/resume
invalidates them, and every relation additionally rechecks the live realm,
program, frame, stack, metadata generation, and active slot. A linked receipt
cannot satisfy ordinary `GetValue`, and an ordinary receipt cannot select a
linked slot.

The public `GetStaticScopeRelation` target echoes one of those two shapes.
It requires a new independently negotiated, default-deny owner/client
`OpaqueStaticScopeRelation` grant, not `BoundedValues` or the existing
`OpaqueSymbolType` grant. The canonical manifest includes the prerequisite
opaque metadata, symbol-inventory, and type-inventory grants; the exact
metadata, symbol ID, and type ID must already have crossed that same stream's
inventory receipt boundary before any pair is returned. Core checks the
metadata receipt and matching program before the private relation; after the
child's exact static-only echo, core checks the returned symbol and type IDs
against their separately receipted inventories before emitting a complete
public reply that echoes the full target and contains only those parent-bound
opaque IDs. A missing grant, receipt, moved pause, malformed response, or
unbound slot yields one typed error, never a partial symbol/type pair. Type
display still needs its independent grant and `Value` remains separately
granted, Scopes-receipted, bounded, and runtime-only. Add the new public
request/reply variants, named owner policy, canonical capability identifier,
manifest version, and debugger protocol bump together only after the staged
IPC and core denial checks are ready; then prove real socket behavior before
claiming C3.1.3.4 complete.

**C3.1.3.4.2 staged public data shapes:** `DebuggerLinkedScopeSnapshot`
echoes a complete child-first linked stack, fixes selection to entry-root
index one, and bounds its lexical slot list by the retained scope budget.
Duplicate slot ordinals, zero/out-of-range budgets, and inconsistent
truncation are malformed. `DebuggerStaticScopeTarget` holds either an exact
ordinary root/parent `ValueTarget` with matching metadata owner or an exact
linked entry-root stack/slot selector with entry-owned metadata. Its static
relation echoes the whole target and requires both opaque compiler IDs to
remain under that same metadata parent. Focused malformed-shape and JSON
round-trip tests, the full IPC 117-test suite, workspace Clippy, and
formatting pass. These data-only types add no request, reply variant,
capability, grant, or public protocol change; debugger remains v40.

**C3.1.3.4.3 private linked entry-root scopes:** The page-executor trait now
offers a default-denied complete linked-scope snapshot. The out-of-process
owner accepts only the exact stored core-reminted two-frame identity and live
document, reacquires the same child invocation at the fixed maximum scope
budget, and rejects a changed frame, truncated private scope, or duplicate
entry-root slot ordinal. If the previously stored private stack was already
complete, every reacquired frame and scope entry must also match it, even
when safe points alone are unchanged. Only the complete entry-root
`(slot_ordinal, scope_depth)` list crosses to the core-facing result; child
program and invocation IDs remain private. Focused mock drift/forgery and
real Launcher-child tests, engine 309-test suite, workspace Clippy, and
formatting pass. No public linked-scopes request, receipt, grant, or version
change exists in this leaf.

**C3.1.3.4.4 staged pause receipts:** The core-local debugger session now
retains ordinary `GetScopes` slot targets and complete linked entry-root
stack/slot targets in separate sets under one 4,096-entry bound and one
core-owned pause incarnation. A new or malformed later-pause reply clears
both old sets before denial; stale incarnation values and other streams
cannot borrow either. Linked replies that are truncated, malformed, or
over-budget mint no receipt. The static-only receipt checker accepts the
appropriate shape, while the existing `Value` receipt checker consults only
the ordinary set and its separate runtime-value grant remains required.
Focused tests cover same-stream exactness, forged slots, cross-stream and
stale denials, truncation, and atomic combined-budget refusal. IPC 118-test
suite, workspace Clippy, and formatting pass. No public static relation
request, dispatch, grant, or protocol change is added here.

**C3.1.3.4.5 staged core relation dispatch:** A core-local helper now requires
an explicit independent-grant decision and the exact ordinary or linked
same-stream scope receipt at the current pause incarnation. It rejects absent
metadata and separate symbol/type inventory receipts, then remaps the public
selector through the live realm and core-owned frame/stack identity. The
existing private relation helper reacquires the active lexical slot and
metadata generation; the staged caller accepts only a full private target
echo and a complete parent-bound public symbol/type pair already receipted on
that session. Tests exercise ordinary and linked success without `Value`
authority and deny missing grants, missing/foreign receipts, stale
incarnations, and altered echoes. The engine 309-test suite, workspace Clippy,
formatting, and diff checks pass. No public request, owner policy, manifest
or protocol change exists yet; the explicit grant parameter must be derived
from the actual owner/client negotiation in C3.1.3.4.6.

**C3.1.3.4.6.1 staged linked scopes:** A core-only helper accepts an exact
complete public linked stack and a bounded entry budget, resolves the live
realm, requires LinkedModules and Scopes availability, and remaps the full
stack to the private core identity. The private executor reacquires the whole
pause; core requires its exact stack echo and validates the entire complete
entry-root slot list, including duplicate ordinals beyond the caller's
display budget, before returning only entry-root slot/depth. A smaller
requested budget may produce a marked-truncated public shape, which the
session receipt logic will not accept. Focused tests reject invalid budgets,
forged or moved stacks, hidden duplicate slots, and unavailable scopes; the
engine 309-test suite, workspace Clippy, formatting, and diff checks pass.
There is still no public linked-scopes request or static relation grant.

**C3.1.3.4.6.2 atomic public route decision:** Publish debugger v41 and
metadata manifest v5 only in the same change as both routes and their
fail-closed tests. Name the independent metadata capability
`OpaqueStaticScopeRelation`, map it to a separate public
`StaticScopeRelation` availability report, and add one default-off Launcher
owner option forwarded to the core's startup-only option. The canonical
selection includes opaque metadata, type, and symbol inventories; neither
`OpaqueSymbolType` nor `BoundedValues` implies it. The report is Available
only for a live child with Scopes and all three prerequisite inventories,
and a client must both negotiate that exact grant and retain an Available
report for its realm. In-process/absent-executor routes deny both new
requests. An owner option without its socket and inventory prerequisites is
rejected before listener startup.

`GetLinkedScopes` takes an exact complete linked stack and an entry budget;
it requires live LinkedModules and Scopes availability and echoes only the
entry-root slot/depth list. Its complete, well-formed, untruncated reply
mints a distinct linked receipt only if this stream negotiated the static
relation grant; an incomplete reply remains inspectable but mints none.
Ordinary `GetScopes` mints its existing receipt when either the independent
static relation or bounded-value grant is present. Both receipt families
share the current pause incarnation and cap, but `GetValue` continues to
consult only ordinary receipts and its separate value grant.
`GetStaticScopeRelation` accepts the ordinary or linked exact target. Core
must validate the public shape, live realm, negotiated capability and
Available report, exact same-stream pause receipt, metadata receipt, and
separately receipted symbol/type IDs before returning one complete echoed
pair. No error path may include one half of the pair. IPC tests cover wrong
manifest version/order/prerequisites, wrong-version Hello, malformed wire
targets/replies, and no accidental Value authority. Core tests cover
default-off owner/client grants, truncated/over-budget scopes, stale pause,
foreign stream, forged slot or metadata, moved linked stack, and changed
private echo. Launcher/core startup tests cover prerequisite failures. Real
Launcher socket acceptance remains C3.1.3.4.7.

**C3.1.3.4.6.2 guarded public route delivered:** The complete route now
ships as debugger v41 with canonical metadata manifest v5. Launcher and core
both require an explicit default-off `--debugger-static-scope-relation` owner
selection with debugger socket plus opaque metadata, type, and symbol
inventories; the client separately requests `OpaqueStaticScopeRelation` in
Hello. A live child advertises `StaticScopeRelation` only when Scopes and all
three prerequisites are available for that stream. `GetLinkedScopes` remaps
and reacquires the complete linked stack, returning only bounded entry-root
slots; an untruncated complete reply can mint a linked same-stream receipt.
Ordinary `GetScopes` likewise mints a receipt for the independent static grant
without a value grant. `GetStaticScopeRelation` then requires the exact pause
receipt, metadata receipt, symbol/type inventory receipts, live realm and
active slot, and full private target echo before returning one boxed,
parent-bound opaque pair. The existing `GetValue` gate still requires
`BoundedValues` and only an ordinary receipt.

Focused IPC wire/manifest, core dispatch, and both owner-option tests pass.
The core denial matrix covers missing owner/client grant, missing or foreign
receipt, stale pause, malformed target, moved linked stack, altered private
echo, truncated linked scope, and combined receipt-cap exhaustion. The full
workspace test suite, workspace Clippy, formatting, and diff checks pass.
One old core-binary test still expected Stack/Scopes to be Planned on the
out-of-process route; it was updated to assert their already available
source-free state while retaining Planned runtime values and the new static
relation without a grant. Real Launcher public-socket acceptance and
no-partial lifecycle tests remain C3.1.3.4.7.

**C3.1.3.4.7.1 real ordinary roots:** The Launcher public socket now proves
the complete static-only relation path for inline BlueTS classic and module
pages through a real core and supervised child. Each stream independently
negotiates the static grant without bounded values, inventories one opaque
metadata handle and its symbol/type IDs, pauses a verified root safe point,
then selects an exact `GetScopes` lexical slot. A successful relation echoes
the full ordinary target and only the two IDs already inventoried on that
same stream; no source, display, or runtime preview is included. The selected
slot's `GetValue` remains CapabilityUnavailable. The focused two-kind real
Launcher test passes; nested and linked sockets plus lifecycle refusals
remain separate leaves.

**C3.1.3.4.7.2 real nested parents:** On both classic and module pages, the
real Launcher socket pauses inside a verified nested BlueTS function. The
active child argument can be inspected as a source-free Scopes slot but is
not a valid static-only selector; the caller-root frame's own slot, frame
identity, safe point, and metadata yield one complete parent-bound symbol/type
pair from the same stream's inventories. A guessed parent slot refuses.
The test deliberately inventories metadata after the final `ListPrograms`:
that operation invalidates the core's previous private metadata mapping,
even if old numeric handles happen to match. The focused two-kind real
socket test passes. Linked and lifecycle cases remain separate leaves.

**C3.1.3.4.7.3 real linked entry root:** The existing Launcher-supervised
two-module graph now independently negotiates static-scope authority and
receives a complete `GetLinkedScopes` snapshot for the exact child-first
dependency/entry stack. Only entry-root slot/depth entries cross that reply;
the same stream inventories entry-owned type and symbol IDs before one
compiler-only relation echoes the complete linked target. A selector using
dependency metadata refuses as InvalidTarget, never returning a partial
pair. The focused real linked graph test passes. Lifecycle, cross-stream,
and separate runtime-value grant cases remain C3.1.3.4.7.4.

**C3.1.3.4.7.4.1 forged and cross-stream refusals:** The real linked Launcher
graph rejects a guessed entry-root slot and a modified metadata generation
with complete typed errors, never a partially filled symbol/type pair. Its
breakpoint is now armed immediately after the two program IDs and dependency
safe point are known; waiting for multiple original-source metadata queries
could let the graph complete before arming. The real classic/module root
test first obtains an ordinary static relation, closes that debugger stream,
then proves a fresh stream with the same static grant cannot borrow the
old metadata/scope selector. The debugger socket serves streams serially,
so the cross-stream check is deliberately sequential. Both focused real
Launcher tests pass. On request, a BlueTS >1,300-line modularity audit and
justified refactors will follow C3.1.3.4.7.4 before C3.1.3.4 closes.

**C3.1.3.4.7.4.2 real pause lifecycle and grant separation:** The classic
and module static-only root sockets reject `Value` for the very slot whose
compiler relation succeeded; after stepping that root, its old static
selector also returns only a typed error. The linked graph rejects old
linked-scopes and relation selectors after resuming its dependency pause.
After reload, old linked-scopes and the other linked-program requests are
StaleRealm; the static relation instead returns CapabilityUnavailable
because reload revokes the independent grant/receipts before that route's
realm preflight. Neither path returns a partial pair. Conversely, the real
classic/module value-only nested socket reads its caller-root value but
cannot use a well-formed static selector without the separate grant. All
three focused real Launcher tests pass against the shared Cargo target;
complete socket/workspace gates remain C3.1.3.4.7.4.3.

**C3.1.3.4.7.4.3 public acceptance:** The complete serial workspace suite
now exits successfully, including all 27 real Launcher debugger socket
tests, 309 engine unit tests, and 19 core-binary integration tests. Workspace
Clippy and rustfmt checks pass. Two existing real-process tests exposed
time-sensitive setup while running the full gate: the source-span test now
arms its root breakpoint immediately after safe-point inventory, before
longer source-metadata queries, and the DOM socket core-replacement test
allows both frontend navigation phases a 30-second read window while
retaining a URL-specific timeout diagnostic. No production behavior changed.
All checks reused the workspace `target/`; one verified obsolete Launcher
test executable was removed, retaining both currently used package and
workspace variants. C3.1.3.4.7.4 is closed. The requested BlueTS file-size
and module-boundary audit is C3.1.3.4.7.5 before the parent closes.

**C3.1.3.4.7.5.1 modularity inventory:** `MODULARITY_AUDIT.md` records the
2026-09-27 line-count snapshot for 27 over-1,300-line Rust files containing
BlueTS-owned code or references, including the separate `checker.rs` owner.
Four direct compiler/bridge files have clear test, inference, lowering, or
attachment splits. Mixed child host, debugger, compiler transport, and
acceptance owners need staged internal modules that preserve wire/ABI and
same-stream receipt semantics. Ten adjacent generic browser/BlueJS files
were evaluated but a BlueTS-only extraction was deferred because their
large body is not BlueTS-owned. The first implementation leaf moves the
compiler's inline tests to their own module; subsequent compiler, bridge,
and mixed-runtime work stays file-scoped and runs focused gates.

**C3.1.3.4.7.5.2.1 compiler test owner:** The compiler's inline `#[cfg(test)]`
block moved unchanged to `compiler/tests.rs` with an MPL header, while
`compiler.rs` now names the child test module. The implementation file fell
from 1,364 to 766 lines; the relocated test file is 600 lines. All 156
BlueTS library tests and the crate's full integration suite pass with their
original test names, and workspace Clippy, formatting, and diff checks pass.
No compiler API, cache behavior, or output contract changed.

**C3.1.3.4.7.5.2.2 checker test owners:** The checker tests' structural,
project, generic, and overload cases moved to `tests/project_contracts.rs`
with their original function names and an MPL header. The original 1,469-line
module is now 778 lines and the new child module is 697 lines. The complete
BlueTS crate suite (including all 156 library tests), workspace Clippy,
formatting, and diff checks pass. No checker implementation or public
diagnostic contract changed.

**C3.1.3.4.7.5.2.3 checker type relations:** Structural assignability,
named-type expansion, defaulted generic argument completion, type
substitution, and type labels moved together to the internal
`checker/type_relations.rs` module. The parent keeps its original crate-level
`type_label` import and sibling helper visibility, so checker call sites and
diagnostic text are unchanged. `checker.rs` fell from 1,368 to 1,080 lines;
the new module is 301 lines. The complete BlueTS crate suite, workspace
Clippy, rustfmt, and diff checks pass.

**C3.1.3.4.7.5.3.1 direct lowering owner:** Classic/module AST lowering,
function and variable conversion, and original-source provenance generation
now live in `bluets-bluejs/src/lowering.rs` (342 lines). The facade fell
from 1,711 to 1,378 lines and retains its public bridge ABI/types. Existing
expression tests depended on the facade's private `ExpressionLowerer` import;
it remains available under `#[cfg(test)]` without exposing a production API.
All 107 bridge crate tests, workspace Clippy, rustfmt, and diff checks pass.
The remaining over-threshold attachment/map owner is C3.1.3.4.7.5.3.2.

**C3.1.3.4.7.5.3.2 attachment/map owner:** Source identity validation,
live program attachment, root symbol slots, and safe-point map construction
and validation moved to `bluets-bluejs/src/attachment.rs` (524 lines).
The facade is now 866 lines, with its original public bridge types and ABI.
The page runtime's internal source-identity use remains available through a
private parent import. All 107 bridge tests, workspace Clippy, rustfmt, and
diff checks pass. The direct bridge phase is closed; mixed runtime/transport
files proceed in separately scoped leaves.

**C3.1.3.4.7.5.4.1.1 child-executor test boundary:** The entire existing
inline test body moved, without rewriting the tests, to
`javascript_child/tests.rs` under the same `mod tests` identity. The
production `javascript_child.rs` fell from 14,468 to 7,016 lines; the
7,344-line extracted test file still needs concern-level submodules before
the test owner is considered modular. All 309 engine library tests, workspace
Clippy, rustfmt, and diff checks pass. No runtime or debugger behavior changed.

**C3.1.3.4.7.5.4.1.2 child-executor debugger test owners:** The exact private
static/linked adapter, linked pause, and scope/value test bodies now live in
three bounded child modules (486, 1,226, and 992 lines). The parent test file
retains one shared socket/child fixture harness and the remaining integration
tests; it is 4,658 lines until the transport/authorization and real-child
metadata leaves. All 309 engine library tests pass with the reused target;
runtime and debugger contracts are unchanged.

**C3.1.3.4.7.5.4.1.3 child transport and admission tests:** Socket/session
timing and DOM-pump cases, resource-accounting/lifecycle cases, and closed
graph/HTTP authorization cases now have independent 432-, 294-, and 506-line
test owners. A 3,444-line parent retains shared fixtures and real-child
debugger/metadata cases for the final test split. All 309 engine library
tests pass using the same target directory; wire and runtime behavior is
unchanged.

**C3.1.3.4.7.5.4.1.4 real-child test owners:** Real-child metadata, exact
span/envelope validation, nested pause, stack/scope, and lifecycle tests are
now separate 390-, 935-, 647-, 647-, and 448-line modules. Shared fixtures
remain in a 407-line parent, and the largest file across the completed child
test family is 1,226 lines. All 309 engine library tests pass; production
adapter extraction remains a separate leaf.

**C3.1.3.4.7.5.4.2.1 Launcher host test boundary:** The existing inline
test body moved under the same `mod tests` identity to an MPL-headed
`bluejs_host/tests.rs`, with no rewritten assertions or changed runtime
behavior. Production `bluejs_host.rs` fell from 14,133 to 7,709 lines; the
6,394-line test owner proceeds through DOM/scheduler, metadata, and
admission/stepping concern splits. All 116 Launcher library tests pass with
the reused Cargo target.

**C3.1.3.4.7.5.4.2.2 Launcher host DOM and private debugger tests:** DOM
transport, nested-frame scheduler, exception locations, and static-scope/value
cases now have independent 704-, 659-, 261-, and 636-line test modules.
Shared graph/document/script/value-target fixtures remain in the parent,
which is 4,158 lines until metadata and admission/step concerns are split.
All 116 Launcher library tests pass with the reused target; behavior and
wire contracts remain unchanged.

**C3.1.3.4.7.5.4.2.3 Launcher host metadata test owners:** Resource
accounting, debugger configuration, BlueTS root slots, metadata inventory and
contracts, and source-span cases now have bounded 184-, 516-, 520-, 690-, and
289-line modules. The 1,989-line parent retains fixtures and admission/step
tests for the last split. All 116 Launcher library tests pass with one reused
Cargo target; no wire or runtime behavior changed.

**C3.1.3.4.7.5.4.2.4 Launcher host admission and stepping tests:** Snapshot
and module admission, module/source-span stepping, and linked-module lifecycle
cases now live in 533-, 940-, and 417-line modules. The common fixture owner
is 117 lines; all host test-family files are below 1,300 lines (maximum 940).
All 116 Launcher library tests pass with one reused target. Production host
prepare/debugger extraction remains a separate leaf.

**C3.1.3.4.7.5.4.3.1 child-client contract boundary:** The complete public
`PageHostClient` transport trait moved unchanged to a 765-line internal
module and is re-exported through its original path. This leaves the
production executor facade at 6,259 lines, with separate socket, private
linked/static adapter, core debugger, and helper seams for later leaves.
All 309 engine library tests pass with the reused target; the child protocol
and API are unchanged.

**C3.1.3.4.7.5.4.3.2 child connection transport:** The authenticated
`PageHostConnection`, fixed-deadline socket request/pump, and its full
`PageHostClient` implementation moved to a 901-line internal module. The
original public connection path remains a re-export; request helpers and
stream visibility extend only to the parent so existing transport tests keep
their exact assertions. The executor facade is 5,365 lines. All 309 engine
library tests pass with the reused target; protocol behavior is unchanged.

**C3.1.3.4.7.5.4.3.3 private debugger reply adapters:** Exact linked arm,
state, stack, spans, resume, and static-scope relation checks moved together
to a 277-line internal module. The adapters expose only parent-module access,
leaving public grants, reminted handles, and wire replies unchanged. The
production child-executor facade is 5,097 lines; all 309 engine library tests
pass with the reused target.

**C3.1.3.4.7.5.4.3.4.1 core debugger trait boundary:** The complete
`PageJavaScriptDebuggerLocations` implementation moved intact to an internal
`debugger.rs`; no grant, remint, validation, or reply path was rewritten.
The executor facade is 2,238 lines and the debugger owner is 2,865 lines.
Both still require bounded concern-level delegations and helper extraction.
All 309 engine library tests pass with the reused target.

**Native nested-frame checkpoints (C1.2.1.2):** Stamp the same deterministic
pre-order code-unit ordinal into installed bytecode and each closure descendant
before exposing its inventory (C1.2.1.2.1). This gives the VM an exact
runtime-code match even when two functions have identical instructions; the
registry generation is stamped alongside the ordinal and still belongs to the
page program, not to the ordinal alone.
Then capture a direct synchronous caller and child at the selected inner
instruction without passing a pause marker through JavaScript exception
handling (C1.2.1.2.2). Step only that retained invocation and rejoin its
classic caller once at terminal completion (C1.2.1.2.3), then repeat the
continuation transfer for an entry-module root whose linked graph is retained
separately (C1.2.1.2.4). Each checkpoint needs a native regression before the
public transport gains this control.

**Installed code-unit identity (C1.2.1.2.1):** The debugger registry now
assigns each root and closure bytecode its ordinal in the same traversal that
builds the immutable safe-point inventory. Bare compiler output has no such
tag; both structured-program and precompiled BlueTS installations receive it.
Equal bytecode bodies retain distinct ordinals, deeper closure descendants
keep pre-order identity, and cloning preserves the tag. The ordinal does not
identify an invocation by itself. The registry tests,
BlueJS Clippy, and 507 non-environmental library tests pass.

**Native nested-frame capture (C1.2.1.2.2):** The first native seam accepts a
verified non-root instruction in an installed classic program and runs its
root until a direct synchronous `Call` enters the matching program generation
and code unit. Before that child instruction, the VM moves the child operand,
binding, handler, iterator and execution-context state into a GC-visible
continuation, then suspends the caller before consuming its call inputs. A
fresh nonzero serial names this invocation; ordinary VM entry points cannot
run while it is retained. Deeper interpreted calls and constructor entry fail
explicitly before the target body executes. An older closure with the same
ordinal but a different program generation runs normally and cannot trigger
the new program's pause. Native regressions inspect the preserved call site,
pre-instruction effects, child object roots, unsupported paths, and generation
aliasing. Stepping/rejoin and the public route remain C1.2.1.2.3/C1.2.1.3.

**Native nested-frame step and classic rejoin (C1.2.1.2.3):** A step accepts
only the active invocation serial, temporarily roots the waiting classic
caller's complete execution while restoring the child, then suspends again
after one actual child instruction. A branch reports its real backward PC.
When the child returns, its result replaces the waiting `Call` inputs exactly
once and the root remains paused at the following verified instruction; an
ordinary root resume finishes without replay. A child throw takes the saved
caller's catch/finally path, while an uncatchable error cleans both frames.
Target code containing direct eval or a tail call is refused before execution.
A one-object nursery regression also retains a caller-only operand through
child allocations and collection. This path currently owns a classic root;
the entry-module graph has a different root continuation and remains the
explicit C1.2.1.2.4 checkpoint. No public frame control is advertised yet.

**Entry-module nested checkpoints (C1.2.1.2.4):** The module evaluator already
stores a suspended entry root together with its linked graph when its
interpreter returns `Suspend`. First reuse the exact installed-program
generation and code-unit target to let a direct synchronous child produce
that suspension, retaining dependency effects and the graph (C1.2.1.2.4.1).
Then adapt the child step/rejoin to mutate the saved module-root `Call` frame,
not the ordinary classic root fields, and resume the same graph once
(C1.2.1.2.4.2). Finally route a child throw through the module caller's
handlers and the module evaluator's record/async cleanup, including graph
invalidation on failure (C1.2.1.2.4.3). These are native boundaries; no
public frame identity is minted until C1.2.1.3.

**Entry-module child pause (C1.2.1.2.4.1):** The native module graph entry now
accepts the same exact installed-program generation and non-root code-unit
instruction as the classic seam. It runs the authorized graph until the entry
calls that child, then the existing module evaluator stores its suspended root
and linked graph while the child context remains in the nested continuation.
A two-module regression verifies the dependency's effect ran once, the child
body has not run, the entry `Call` and graph remain retained, and an unrelated
execution entry cannot overtake the pause. Module-child stepping and module
record cleanup remain the next two checkpoints.

**Entry-module child step and rejoin (C1.2.1.2.4.2):** The same serial-bound
child step now recognizes a saved module-root caller. Each successor preserves
the child and linked graph; terminal return replaces the saved module-root
`Call` inputs, advances its real PC, and carries forward the spent instruction
budget before the existing module resume completes the graph. A two-module
regression verifies compiler instruction boundaries and exactly one
dependency/entry effect. An unhandled child throw takes a conservative error
path: both debugger frames are released, the linked entry record is marked
failed and no ordinary execution entry remains blocked. Catch/finally and
asynchronous graph behavior are still C1.2.1.2.4.3.

**Module nested cleanup checkpoints (C1.2.1.2.4.3):** On a catchable child
throw, temporarily restore the retained module-root execution and linked
records, run its existing completion resolver with the saved handler/iterator
stacks, and re-park a catch/finally successor rather than treating every
throw as an unhandled module failure (C1.2.1.2.4.3.1). Then test an async
dependency and entry await against the same pause/step/rejoin path
(C1.2.1.2.4.3.2). Finally force the shared instruction budget to fail during
a nested step and verify the graph has no falsely completed entry or detached
frame while later execution still works (C1.2.1.2.4.3.3).

**Module nested catch/finally rejoin (C1.2.1.2.4.3.1):** A catchable child
throw restores the retained entry execution and parks the linked records as
active module evaluation while BlueJS's ordinary completion resolver examines
the saved handler and iterator stacks. A real catch/finally successor is
saved back into the same module continuation; an unhandled throw instead
flows to the explicit graph-error cleanup. A native module regression checks
the original thrown value reaches `catch`, `finally` executes once, and the
entry completes with no error. The unhandled-throw regression still passes.

**Async graph across a nested pause (C1.2.1.2.4.3.2):** A tagged entry graph
with a top-level-await dependency reaches the requested child pause only after
the dependency Promise job settles. Stepping returns to the retained entry
root, whose own later top-level await then finishes through the existing
module-await continuation. A native regression verifies the dependency,
entry, child and post-await effects each run once, both linked records are
evaluated, and the module-continuation and Promise-job queues are empty.

**Nested module resource failure (C1.2.1.2.4.3.3):** A native entry module
pauses before an inner infinite loop and repeatedly steps under a fixed
256-instruction budget. Once BlueJS returns `InstructionLimit`, the child and
module-root debugger continuations and the transient caller state are gone;
the linked entry is neither evaluated nor suspended, and an unrelated later
script can execute. This is an explicit failed graph, never a fabricated
`Completed` state or an endlessly retained frame.

**Private page-host actual-usage accounting:** Page-host v31 adds one
authenticated child-wide snapshot of currently live realm count, retained
programs/root bytecode, and VM-managed heap. The child recomputes checked
totals from exact live realms and rejects impossible or owner-envelope-exceeding
results; core accepts only a well-formed reply matching its validated
per-realm accounting receipts, not its retry-suppression document table.
Replacement and close drop the predecessor charge. This is core-only and
complements conservative child-wide reservations; it is not a source/cache,
allocator, process-RSS, or fleet budget and is not exposed to page, frontend,
debugger, or MCP peers. The public BlueTS source-span step now also accepts
compiler-minted source ID zero, which is valid when separately receipted.
If a child admits a successor but returns an error or mismatched acknowledgement,
core best-effort closes both exact generations so the successor cannot remain
runnable solely because the predecessor close was rejected as stale.

**Public BlueTS source-span stepping:** Page-host protocol
v30 adds an authenticated child request bound to one paused classic-root
safe point, exact retained BlueTS metadata handle, and compiler-minted source
ID. The child derives the starting span from its verified direct-lowering map,
advances exactly one root instruction per core-owned turn, and stops at the
next different bound original span or completion. A fixed 256-instruction
budget yields a distinct source-free limit state while retaining the same VM
continuation; ordinary step/resume still work. Wrong source/metadata, duplicate
requests, and stale document generations fail before execution. Unit and real
isolated-child socket tests prove transition, document order, redaction, and
budget behavior. Debugger v30 and metadata manifest v4 now expose this seam
only under independent owner/client `OpaqueSourceSpanStep` and
`OpaqueSafePointSpan` grants, exact same-stream metadata/source receipts, and
a core-reminted paused root safe point. Core revalidates the compiler span and
child echo before acknowledging the step, preserves `SourceStepLimitReached`
as a distinct verified stop reason, and rejects an unreceipted peer or stale
generation. A real launcher/core/child socket regression covers those gates,
the next distinct bound span, and resumed completion. Modules, nested frames,
stack, scope, and values remain outside this seam.

**Original-coordinate safe-point metadata:** Page-host protocol v32 and
debugger protocol v31 extend the separately default-denied
`OpaqueSafePointSpan` result with zero-based UTF-16 line/column coordinates.
The BlueTS-to-BlueJS bridge computes them in one scan of the checked original
source before discarding text; the live child retains only the location and
byte span. Child and core validate the bounded coordinates against the exact
range, safe point, metadata handle, and same-stream source receipt before a
public reply. Real HTTP classic and ESM tests verify CRLF/HTML normalization,
module source ownership, redaction, and existing stale/unauthorized rejection.
This does not add arbitrary source-map reads or module execution control.

**HTTP source-cache accounting update:** The core-owned HTTP(S) page-script
authorizer now retains at most 4 MiB of verified source payload across its
documents. Its private deterministic least-recently-used cache evicts old
entries before admitting newly verified bytes; an evicted URL is fetched again
under the same canonical URL, MIME, direct-response, length, and SHA-256 checks.
The policy fingerprint advances to `core-page-http-resource-authorizer-v2`
and includes this fixed budget. Focused tests cover bounded accounting,
eviction order, and a changed response being rejected after eviction. The
budget does not count cache-key/allocator overhead, completed graph copies,
BlueJS VM memory, or process RSS; broader page-host resource accounting is
still open.

**Trusted owner HTTP page-resource policy update:** The public launcher now
accepts `--page-http-policy-file <absolute-json-path>` only with
`--out-of-process-bluejs`; an embedding owner can supply the same bounded typed
policy. The file names canonical HTTP(S) URLs, SHA-256 expectations, and either
the live document's origin or one exact canonical origin. Launcher sends the
policy through a one-shot private, versioned core stdin bootstrap, optionally
alongside the sealed compiler catalog. Core constructs its existing closed
HTTP authorizer before binding any listener. The page, frontend, debugger,
MCP, and supervised child cannot read or replace the manifest; the child gets
only integrity-verified, source-closed JavaScript or BlueTS graphs. A real
launcher/core/HTTP/child regression covers admitted classic JavaScript and a
BlueTS static-import graph, unfetched unlisted URLs, and rejected bad hashes;
another regression proves the policy coexists with both an owner compiler
catalog and the default fixed compiler fixture. This does not add DOM/events,
general fetch, dynamic imports, runtime source-level debugger stepping, or
per-client resource authorization.

**Default-deny compiler catalog visibility update:** A trusted startup owner
can now register a project as private while preserving its closed graph in the
core's sealed catalog. The owner bootstrap is now version 2, rejecting the
earlier version 1 instead of silently changing its visibility semantics. A
JSON bootstrap project must set
`expose_to_compiler_ipc: true` to become queryable; omission defaults to false.
Only exposed opaque IDs enter each accepted compiler stream's `ListProjects`
receipt, and a guessed private ID fails as `UnobservedProject` before reaching
the compiler service. The compiled-in fixed fixture retains its existing
explicitly exposed behavior. This is owner-level project visibility, not
per-client authorization or build/write elevation.

**Owner-only compiler catalog bootstrap update:** A trusted launcher owner
may now supply a complete versioned, bounded, closed project catalog from an
absolute regular JSON file or its typed embedding API. The launcher passes it
to each new core through a one-shot inherited stdin pipe, not compiler IPC or
MCP. Core validates the graph and seals all registrations before binding any
listener. The fixed compiled-in profile remains the default when no catalog
is supplied. Public compiler peers only inventory opaque project IDs and use
the existing read-only query operations; they cannot register, update, load
files, build, or write outputs. Input authorization/canonicalization remains
the trusted owner's responsibility. The chosen entry-module identity is still
visible through the separately authorized `DescribeProject` query, so an owner
must not put a sensitive filesystem path there. Output-write elevation is
still open.

**Owner catalog path-identity hardening:** The version-2 startup catalog now
rejects lexical aliases (dot segments, ambiguous slashes, encoded or
backslash separators, query/fragment suffixes), requires its config, entry,
and ordinary source modules to lie under the declared project root, and
rejects an output root that overlaps any catalog input or another output.
Containment is segment-aware, so `/app` does not capture `/app2`. This check
is source-free and runs before core registration; it neither resolves
filesystem symlinks nor grants build/write authority. A future output adapter
must still canonicalize real paths and enforce its own transaction boundary.

**Atomic source-position arm update:** Debugger v29 adds
`ArmStaticMetadataSourceBreakpoint` without changing the child-private
page-host v29 protocol or metadata manifest v3. One core session turn requires
the independently owner/client-granted source-position capability, the exact
same-stream metadata and source receipts, a live child binding to a verified
root safe point, and available execution control before arming the pending
classic declaration. Unbound, guessed, stale, cross-stream, non-root, and
already-running targets fail before starting it. The reply is the existing
source-free root-safe-point arm acknowledgement. This closes the gap between
the prior separate resolve and arm calls; it does not grant general
source-level stepping, modules, nested-frame interruption, stacks, scopes,
source reads, or values.

**Compiler/MCP sealed-catalog inventory update:** Compiler IPC v10 and fixed
query-only manifest v5 add `ListProjects`, a capped source-free inventory of
sorted opaque IDs from the core owner's exposed sealed startup projects. Core records
the exact IDs returned to each accepted compiler stream and rejects all
project queries before that stream receives its inventory or after it ends.
MCP exposes `bluetsc_list_projects`, validates the inventory, and requires an
observed ID before description or check. Real core, launcher, and MCP
`tools/call` regressions cover the boundary. A subsequent owner-only startup
bootstrap adds multi-project distribution; this inventory still grants no
remote registration, source/path access, build, or write authority.

**Native debugger step foundation:** BlueJS retains the same paused
classic-root continuation across a one-instruction step and reports the actual
next verified root bytecode boundary, including branch and loop successors.
The VM executes nested calls to completion as one root instruction, never
exports operands or completion values, and discards the continuation on realm
replacement. The in-process debugger and launcher-supervised page-host routes
now publish this bounded root-classic step with the observable one-turn
`Stepping` state. Modules, nested frames, source-level stepping, stack,
scope, and values remain unavailable.

**Compiler/MCP original-location update:** Compiler IPC v8 and its fixed
query-only manifest v4 add separate symbol- and contract-location operations.
The MCP tools require the same opaque session receipt, an observed exact
check generation, and both declaration and source IDs from matching inventory
pages before forwarding. Core checks that the retained source owns the exact
declaration and returns only the two IDs, bounded half-open UTF-8 byte range,
and original zero-based UTF-16 coordinates. It offers no arbitrary offset
mapping, source text, source-map read, runtime value, project mutation, build,
or output authority. Wrong source, guessed ID, and stale generation fail
closed; compiler IPC and real MCP/core regressions cover the boundary.

**Debugger original-location update:** Page-host v26 and debugger v25 extend
only the already default-denied, same-stream-receipted symbol- and
contract-location replies with zero-based UTF-16 start/end coordinates from
the original BlueTS declaration. Each reply still names only the exact
opaque declaration/source IDs and its bounded half-open UTF-8 byte range;
there is no arbitrary offset-to-line query, source read, source-map lookup,
module identity, or runtime value. The child and core reject malformed
coordinates before public reminting. A real launcher-supervised regression
checks CRLF and supplementary-plane Unicode plus stale-handle rejection.

**Original-source coordinate foundation:** `BlueTsDebugInfo` now retains
zero-based UTF-16 start/end positions beside each compiler-minted symbol and
reifiable contract's existing half-open UTF-8 byte span. A one-pass temporary
position index derives coordinates from only the compiler's original source;
the retained record still contains no source text or general line-map query.
CRLF and supplementary-plane Unicode are covered. A Unicode block-comment
fixture also fixed a lexer byte-scan panic. The separate debugger and
compiler/MCP location replies now carry these retained coordinates.

**Symbol export update:** Page-host v25 and debugger v24 carry the BlueTS
checker's exact `exported` boolean in the existing bounded symbol-display
reply. This remains under the independently default-denied
`OpaqueSymbolDisplay` owner grant, canonical peer negotiation, live attachment,
and same-stream parent/symbol-ID receipt. It adds neither a new target nor a
symbol-inventory disclosure; source text, spans, types, contracts, bytecode,
VM objects, and values remain unavailable through this operation.

**Compiler/MCP symbol export update:** Compiler IPC v7 carries that same
checker-owned `exported` boolean in its existing exact-generation
`GetStaticSymbol` reply. The v3 fixed query-only manifest is unchanged:
the owner still registers the closed project, and MCP still requires a
same-session symbol-ID inventory receipt before `debug_get_symbol` forwards
the query. No source-read, project-update, build, or output-write authority
is added. Real core/MCP coverage checks both local and exported declarations.

**Compiler work-set pagination update:** Compiler IPC v6 and its v3 fixed
query-only manifest add `ListWorkSet` as the tenth read-only operation.
`bluetsc_list_work_set` exposes bounded pages of the four incremental check
work-sets only after this MCP session observed the exact check generation.
Each continuation is core-minted, one-shot, bound to the accepting stream,
generation, and work-set category, and released on disconnect or replacement.
Module identities are untrusted, source-text-free metadata; no source-read,
registration, build, artifact, or output-write authority is added. Older
v5/nine-operation descriptions below record the prior milestone.

**Contract shape update:** Page-host v24 and debugger v23 extend the already
independently default-denied `OpaqueContractDisplay` reply with a closed
root-kind classification. The child resolves only compiler-retained local
references, bounds cyclic resolution, and returns a fixed enum such as
`Record` or `Union`; it never serializes a contract plan or field name. The
same owner grant, negotiated capability, exact live parent and contract-ID
receipt, and child capability report remain mandatory. No additional handle
or authority is minted.

**Contract location update:** Page-host v23 and debugger v22 add a fifteenth
independently default-denied derived metadata capability,
`OpaqueContractLocation`. The owner must select
`--debugger-static-metadata-contract-location` together with the opaque
parent, source-ID, and contract-ID inventory grants. The peer must negotiate
that exact canonical set, receive both IDs on the same stream, and target a
live child attachment whose capability report authorizes the operation.
Only the echoed opaque contract/source IDs and a bounded half-open UTF-8 byte
range are returned. Guessed or stale IDs fail closed; source text, module
identity, contract name/plan/validation, bytecode, VM object, runtime value,
and general metadata-record reads remain unavailable.

**Metadata transport update:** Page-host v22 and debugger v21 extend the
existing `OpaqueSymbolDisplay` capability with a closed compiler declaration-
kind enum (`Import`, `TypeAlias`, `Interface`, `Variable`, or `Function`). The
v21/v20 statement below records the preceding symbol-contract milestone; no
new metadata capability was added at v22/v21. The owner opt-in, negotiated same-
stream symbol receipt, live attachment, and child report remain prerequisites.
The display may now contain its authorized project identifier and this
classification, but still cannot return a source/module identity, span,
type, contract plan, bytecode, VM object, runtime value, or general record.

**Status**: In progress. `backend/bluets` provides a standalone, host-neutral BlueTS front end and `bluetsc` command for an explicitly bounded initial language matrix; `backend/bluets-bluejs` directly lowers the supported classic-script and resolver-preserving ESM graph subsets to public BlueJS AST/bytecode without reparsing emitted JavaScript. A startup owner seals a closed `CoreCompilerProjectCatalog` before serving opaque v10 catalog-inventory/`describe`/`check`/diagnostic/static-metadata queries through `blueice-core --compiler-socket`; diagnostic and metadata inventory cursors are one-shot and exact-generation-bound. MCP attaches only to this bounded service. Its paired-core constructor keeps the browser and compiler adapters on one already-running core lifetime, and a real `tools/call` acceptance test covers all four metadata inventory kinds, diagnostic queries, exact metadata lookups, and source-free cross-kind, replay, and stale-cursor rejection. `blueice-launcher --compiler-mcp-socket <absolute-path>` owns the stable `0600` query listener and passes the compiled-in fixture by default, or a version-2 owner-selected closed catalog through private startup stdin. Only projects explicitly marked `expose_to_compiler_ipc` enter public inventory; compiler socket and MCP requests still cannot supply a profile, source, path, resolver, option, update, build, or write input. Stale public sockets are reclaimed safely and non-socket/live endpoints reject before child spawn. A cutover stages and health-checks v2's sealed private listener before switching future public accepts; every already-accepted compiler/MCP stream stays bound to its one v1 private peer until v1 ends, then fails closed rather than being retargeted across catalog generations. New public connections after commitment reach v2. MCP now exposes `bluetsc_session_capabilities`: when a compiler adapter is attached, the core listener first mints one source-free 32-byte opaque attestation for that accepted relay stream, then MCP returns that exact core-issued ID as its receipt alongside the fixed read-only operations. MCP now gets opaque project IDs from `bluetsc_list_projects`; `bluetsc_describe_project` exposes only a same-session inventoried handle and its canonical entry-module identity; it cannot reveal source/project/config/output roots. When the adapter is absent it truthfully reports unavailable. Every compiler tool repeats that receipt and static queries additionally require the exact generation first observed by `bluetsc_check` under the same receipt. `bluetsc_list_diagnostics` uses that same observed generation and a core-minted one-shot cursor; it returns only compiler code, severity, canonical module identity, byte range, and untrusted diagnostic prose, never source text. Malformed core evidence, a mismatch, an unobserved generation, or a replayed/stale diagnostic cursor fails source-free; receipt/session state is never retargeted on cutover. `blueice-launcher --out-of-process-bluejs` supervises one capability-authenticated BlueJS child per core generation and routes core-authorized inline JavaScript and BlueTS declarations in DOM order into one bounded realm. An immutable core-owned startup authorizer may additionally pass a complete external closed graph, but the child never fetches, resolves URLs or import maps, reads a filesystem, or falls back to another graph. Core and child enforce the same document contracts; a shared generated, inventory-verified `core-script-document-context-v1` artifact lets the fixed checked BlueTS profile directly call only immutable `blueiceDocumentText()` and `blueiceDocumentOrigin()` snapshots. The page cannot select or widen this profile. No page/frontend/log can configure or reflect the child capability; the default launcher does not enable it. Ordinary child realms expose no DOM/event object, fetch/cache, URL/import-map resolution, or general host callback; separate owner-only JavaScript proof profiles expose only bounded DOM lookup/text routes. The opt-in debugger socket offers generation-checked discovery, lifecycle-bound breakpoints, root-classic pause/resume and instruction stepping, plus separately granted BlueTS source-span stepping with exact same-stream metadata/source receipts and a distinct bounded-limit stop reason. Modules, nested frames, stack, scope, and values remain unavailable. Remaining prerequisites include production source/cache/integrity policy, bytecode source-map aggregation, per-client compiler-catalog authorization and update/output elevation, fuller MCP negotiation, and native debugger execution.

The core-owned compiler query listener now binds each static-metadata and diagnostic pagination cursor to the one accepted stream that actually received it. Its internal worker hand-off carries the core-minted Hello attestation, not a client-supplied session field; guessed or cross-stream cursors fail before reaching the shared compiler service. A fixed core-owner receipt cap bounds outstanding stream bookkeeping. Closing the stream releases any unused cursor slots, and a later check revokes that project's outstanding receipts across streams. This strengthens the existing read-only MCP/core lifecycle without adding project registration, build, artifact, source-read, or output-write authority.

`blueice-launcher --debugger-socket <absolute-path>` now provides a stable owner-only debugger transport. The launcher gives each core generation a fresh private debugger socket and uses the same generation handoff gate as browser and compiler routes. A stream accepted before cutover remains bound to v1 and closes with it; only a stream accepted after commitment reaches v2, so opaque realm, program, safe-point, and static-metadata handles cannot cross a generation boundary. The relay parses no debugger traffic and grants no operation beyond the bounded core debugger protocol. Static-metadata inventory is additionally default-denied: an owner must select `--debugger-static-metadata-inventory`, a client must request `OpaqueInventory` in `Hello`, and the exact realm must report it available. The generation-bound opaque handle is the sole target for fourteen independently default-denied derived capabilities: `OpaqueSummary` requires `--debugger-static-metadata-summary` and returns only the BlueTS language label, compiler-options fingerprint, and source/type/symbol/contract counts; `OpaqueSourceInventory` requires `--debugger-static-metadata-source-inventory` and returns only bounded compiler-minted `source_id` values parent-bound to that exact handle; `OpaqueSourceProvenance` additionally requires `--debugger-static-metadata-source-provenance` and that prior source-inventory grant, then returns one already-inventoried ID's non-filesystem canonical module identity and labeled SHA-256 digest; `OpaqueTypeInventory` requires `--debugger-static-metadata-type-inventory`, `OpaqueTypeDisplay` requires `--debugger-static-metadata-type-display` plus the exact type-ID receipt and returns a compiler-produced display capped at 4 KiB; `OpaqueSymbolInventory` requires `--debugger-static-metadata-symbol-inventory`, `OpaqueSymbolDisplay` requires `--debugger-static-metadata-symbol-display` plus the exact symbol-ID receipt and returns a compiler-produced display capped at 4 KiB; `OpaqueContractInventory` requires `--debugger-static-metadata-contract-inventory`, `OpaqueContractDisplay` requires `--debugger-static-metadata-contract-display` plus the exact contract-ID receipt and returns a compiler-produced name capped at 4 KiB; `OpaqueContractValidation` requires `--debugger-static-metadata-contract-validation`, that exact prior contract-ID receipt, a live tuple, and a child capability report before it accepts a fixed-bounded data-only value and returns only `valid: true|false`; `OpaqueLoweringSummary` requires `--debugger-static-metadata-lowering-summary`, the exact prior parent-handle receipt, a live tuple, and a child report before it returns only the fixed safe-point-map ABI, fixed program ABI, canonical aggregate source-set fingerprint, and bounded safe-point count; and `OpaqueSymbolLocation` requires `--debugger-static-metadata-symbol-location`, separate exact same-stream source-ID and symbol-ID receipts under that parent, a live tuple, and a child report before it returns only those IDs and a non-empty half-open UTF-8 byte range capped at 1 MiB. `OpaqueSymbolType` requires `--debugger-static-metadata-symbol-type`, separate exact same-stream symbol-ID and type-ID receipts under that parent, a live tuple, and a child report; it returns only the exact pair of opaque IDs when the child confirms that the symbol's retained static type matches. `OpaqueSymbolContract` requires `--debugger-static-metadata-symbol-contract`, separate exact same-stream symbol-ID and contract-ID receipts under that parent, a live tuple, and a child report; it returns only the exact opaque pair when the child confirms the symbol's retained reifiable contract. A false relation returns an invalid target, never a contract plan or validation result. The symbol-contract relation never exposes a contract name, source/span, static record, or runtime value. The symbol-type relation never exposes a type display, symbol name, source identity, span, static record, runtime value, or general metadata read. The symbol-location operation never exposes source text, a source/module/path identity, line/column mapping, name, type, contract, span record, map entry, AST node, code-unit identity, bytecode offset, VM object, value, source-map translation, or a general metadata read. Core checks value depth (64), collection entries (4,096), nodes (32,768), strings/object keys (256 KiB), and finite numbers before forwarding contract data; the child repeats those limits before its pure `ContractPlan` validation. An owner may select all fourteen, but none is implied by another or by a transport upgrade. Source, type, symbol, and contract IDs carry no module identity, source/content hash, text, span, name, type, contract plan, validation behavior, bytecode, runtime value, child ID, or dereference operation; displays can contain only their independently authorized project identifiers; validation contains neither submitted data, source/module identity, span, plan, failure path, expected/observed category, bytecode, VM object, value, nor a general static-record read. Page-host v21 and debugger v20 carry the newest operation, and the named `DebuggerMetadataCapabilitySelection` builds the canonical manifest without fragile positional capability flags.

The debugger's metadata policy also has a same-stream opaque-handle boundary: after capability negotiation, only a parent handle actually returned by `ListStaticMetadata` may feed `DescribeStaticMetadata`, `DescribeStaticMetadataLoweringSummary`, `ListStaticMetadataSources`, `ListStaticMetadataTypes`, `ListStaticMetadataSymbols`, `DescribeStaticMetadataSymbol`, `DescribeStaticMetadataSymbolLocation`, `DescribeStaticMetadataSymbolType`, `DescribeStaticMetadataSymbolContract`, `ListStaticMetadataContracts`, `DescribeStaticMetadataContract`, or `ValidateStaticMetadataContract`. A bounded local receipt ledger includes the full realm/program/handle generation tuple and fails closed for guessed, changed, or cross-stream handles before core reaches the child. Source, type, symbol, and contract inventories each have bounded ID receipt ledgers; source provenance needs its exact source receipt, while `DescribeStaticMetadataType`, `DescribeStaticMetadataSymbol`, `DescribeStaticMetadataSymbolLocation`, `DescribeStaticMetadataSymbolType`, `DescribeStaticMetadataSymbolContract`, `DescribeStaticMetadataContract`, and `ValidateStaticMetadataContract` require their exact respective ID receipt in addition to independent grants. Symbol location requires both its exact source and symbol receipts; symbol type requires both its exact symbol and type receipts; symbol contract requires both its exact symbol and contract receipts. Guessed IDs cannot become child-probing targets.

The launcher-supervised out-of-process debugger regression now drives the public debugger socket through `Pending` → `Paused` at a compiler-verified non-entry root safe point → `Resuming` → `Completed`, then reloads the real HTTP document and verifies that the old realm, program, and safe-point tuple fails as stale. The test has no access to the private child endpoint or capability and does not claim stepping, stack, scope, or runtime-value inspection.

After an out-of-process child accepts an exact document, core now asks for one source-free aggregate realm accounting record and retains it only under the matching tab/document generation. The record contains only program count plus bytecode and heap byte totals; it is core-owned state, not a page, frontend, debugger, or MCP surface. Navigation and tab removal clear it. During admission, a malformed/mismatched child reply, child error, or transport loss clears it and closes the newly acknowledged realm; a later failed liveness probe clears the cached record rather than associating a successor with predecessor accounting. Host-wide quotas and public accounting remain separate work.

A trusted launcher embedding may now fix a per-realm BlueJS envelope before each child binds its private socket: live realm count, retained programs, root bytecode, and VM managed heap. The same immutable envelope is applied to ordinary launch and cutover children; the public launcher CLI, core, page, frontend, and page-host IPC cannot select or widen it. This bounds BlueJS admission and managed heap per realm, not child RSS or host-wide usage: source/registry/Rust/allocator/operating-system overhead and aggregate reservations remain outside the envelope.

Compiler IPC v5 supersedes the legacy v3/v4 wording below. A successful exact-version `Hello` now carries both the per-stream opaque attestation and a separately versioned, core-authored fixed query-only manifest containing the complete canonical nine-operation vocabulary. `ListDiagnostics` exposes only pages of diagnostics retained for an exact check generation, each with a fixed-bounded, one-shot cursor; no cursor has offset, source, path, or metadata-ID semantics. MCP validates both values exactly and copies the manifest unchanged into its session receipt; a missing, reordered, subset, duplicate, unknown-version, or locally derived manifest never creates an MCP compiler session. The manifest grants no source, path, resolver, option, registration, update, build, artifact, or output-write authority.

Standalone `bluetsc build` now rejects `strict-runtime` before compilation or output staging because this branch has not yet emitted or imported the versioned runtime boundary helper that would make that policy executable. This prevents an artifact or manifest from naming strict runtime enforcement that it cannot provide; `check` remains a static operation, while direct-page contracts remain core-owned. Emitting the helper and proving equivalent direct-page/ESM malformed-boundary rejection remain open work.

**Owner-exposed compiler project inventory (F1 second condition complete):**
The current sealed catalog distinguishes registration from public compiler
visibility. Wrapping a pre-populated core compiler service now leaves every
existing project private by default while still counting it in the sealed
catalog; only an explicit owner-exposed adapter registration enters any
compiler stream inventory. Both direct adapter requests and accepted-stream
requests reject a private project ID before reaching the compiler cache.
Accepted streams additionally require their own prior inventory receipt. A
real launcher/core/MCP regression connects two independent MCP clients to one
owner catalog, proves each receives a distinct session receipt and only the
public project ID, rejects the private project ID, and still checks the public
project. This does not add per-client project subsets, project registration,
build/output authority, or filesystem-root canonicalization.

**F1.1.1 physical owner roots:** An owner catalog may now use absolute host
paths for its project root, config file, source modules, and output directory.
Validation resolves every such identity against the current filesystem and
requires the supplied spelling to equal the resolved canonical path. Project
and output roots must be existing directories; config and source identities
must be existing regular files. Lexical containment and cross-project
input/output collision checks still run first. An absolute project cannot mix
virtual and physical config, entry, module, or output identities. Existing
`project:///` catalogs continue to represent bundled source text with no
filesystem lookup. The launcher checks the owner file before spawning core;
core validates the private bootstrap again before binding listeners. This
does not compare bundled source text with file contents or open a write
capability. F1.1.3 must separately authorize output writes and revalidate
paths at the time of use, because filesystem entries can change after startup.

**F1.1.2 client request closure:** Compiler IPC now refuses unknown fields in
each request variant and its nested project, generation, cursor, and contract
value envelopes. The compiler MCP adapter applies the same strict decoding to
each typed compiler-tool argument and nested cursor. Extra client paths,
resolver edges, compiler options, or plugin names are rejected instead of
being silently dropped before a read-only query. Unknown operation variants
still take the existing unsupported path; contract-validation JSON remains
data, never a route to a filesystem or compiler option. A real core/MCP
regression injects all four field classes and then checks the same inventoried
project successfully with the fixed query shape.

**F1.1.3 owner output grant:** `canonical_output_root` remains an identity in
every registration, without implying write authority. The optional owner
bootstrap `grant_output_write` flag defaults to false, requires a physical
catalog, and is validated with the complete physical input/output graph.
During core startup, an explicit grant path preflights the existing canonical
files and output directory before mutating the registered-project catalog.
The grant lives separately in the sealed core session under a core-owned
project ID; compiler IPC and MCP still expose only the unchanged query-only
manifest and project receipts. A granted catalog check creates no output.
Actual build publication remains for F2.2 and F3.2, which must verify the
grant and revalidate the filesystem target at the time of every write.

**F2.1.1 result identity:** BlueTS now computes one stable observed
graph/options fingerprint before deciding whether emission is possible. A
diagnostic-bearing compilation therefore has a fingerprint even though it
has no artifact; a successful compilation's fingerprint equals its output
and each module artifact fingerprint. Core keeps this value with every
registered-project check generation and verifies build artifacts against it
before returning the in-memory build. Compiler IPC v11 adds the value to the
check reply; later diagnostic, work-set, and static records carry the exact
generation that the client first received with that fingerprint. MCP checks
project/generation/fingerprint consistency before accepting a replacement
generation receipt. This fingerprint is deterministic cache provenance, not a
cryptographic authorization token or an output-write grant.

**F2.1.2 bounded results:** The registered-project service now charges every
retained build component against its artifact byte budget, including module
map keys, repeated artifact fingerprints, source-map entries, declarations,
and strict runtime helper/crossing records. Its artifact count uses checked
arithmetic. Diagnostic retention uses an independent combined byte budget
as well as the existing count limit, keeping a deterministic prefix and
reporting truncation even if the first diagnostic is too large. Existing
work-set retention bounds both entry count and combined identity bytes; the
core IPC adapter additionally caps each check list and page by its own field,
entry, and response budgets. The limits apply before any future output
transaction or MCP build response can carry these results.

**F2.2.1 atomic owner staging:** A trusted core owner now has one staging
primitive for a current registered generation with a separate output-write
grant. It creates a fresh temporary directory inside the canonical output
root, lets trusted owner code populate it, and makes the complete directory
visible with one same-filesystem rename to an immutable project/generation
name. A failed population removes the stage and leaves previous published
generations untouched; duplicate names, missing grants, stale generations,
and changed output-root aliases refuse before publication. The test removes
its tiny temporary output tree. This primitive is not a compiler IPC/MCP
operation. F2.2.2 must prevent diagnostics from entering staging, and F2.2.3
must map only validated artifact paths under the granted root before any
public build route is opened.

**F2.2.2 no-emit-on-error admission:** The owner build/staging entry checks
the separate grant before calling the registered-project compiler. It returns
the exact check generation with no path when diagnostics prevent output, and
does not invoke the trusted stage-population function. A denied project does
not compile or advance its generation. A successful granted build can stage
its bounded in-memory artifact set; errors retain existing output untouched.
The lower-level staging function is now private to core's owner module, so
callers cannot skip the build result gate through that API. F2.2.3 will
replace the trusted population callback with validated artifact-to-path
mapping before any public build route is enabled.

**F2.2.3 contained artifact publication:** The owner build method no longer
accepts a population callback. It preflights all successful compiler artifact
IDs against the physically canonical project root, maps source `.ts` modules
to `.js` plus optional `.js.map` and `.d.ts`, preserves declaration modules,
and rejects aliases, escape components, mismatched artifact keys, unsupported
source kinds, and destination collisions including file/directory ancestors.
Only after the full plan succeeds does the atomic staging primitive create
files under its fresh generation directory. The output grant also pins the
Unix device/inode of its root and refuses a replacement at use time. Until a
registered strict helper publisher and its audit are available, any emitted
strict-runtime crossing fails closed rather than producing a broken module.
The compiler IPC/MCP route is still query-only; F3.2 must explicitly gate any
public build operation by the separate owner grant.

**F3.1.1 scoped compiler negotiation:** The existing exact-version compiler
Hello gives MCP a core-authored query-only manifest and per-accepted-stream
attestation. The sealed project inventory admits only owner-exposed handles
on that stream; a structurally valid check for one of those handles replaces
its generation receipt and revokes earlier metadata IDs. MCP now also
validates `DescribeProject` against the exact requested handle before
publishing its source-free identity. A real MCP client confirms that another
connection rejects a prior session receipt, while existing real core/cutover
tests prove private project IDs and superseded generations cannot be
borrowed. This negotiation grants only the fixed query vocabulary; the
independent output-write grant is not represented by these receipts.

**F3.1.2 one-shot compiler pagination:** Diagnostic, incremental work-set,
and static metadata continuations remain core-owned cursor records, never
numeric offsets supplied by an MCP caller. The service consumes a valid
record once, and the accepted IPC stream requires its own previously returned
receipt for the exact generation and collection. A rejected cross-stream or
cross-kind use does not consume the owning stream's valid continuation;
successful use makes replay fail. Stream closure and new checks revoke
unused records. The added stream-bound diagnostic/work-set regression and
existing metadata and real MCP tests cover these conditions.

**F3.1.3 untrusted compiler strings:** Every compiler tool returns through
the same MCP result formatter: project-derived entry identities, module
names, diagnostic prose, static displays, and core error messages are JSON
values behind an explicit untrusted-data warning and marker. The capability
report contains only fixed core-authored session evidence. An adversarial
project identity containing JSON delimiters, a forged session field, the
marker, and an instruction remains a single encoded value; parsing the
result still yields the genuine session receipt.

**F3.2.1.1 independent build-authority decision:** The existing compiler
query socket and its exact fixed manifest stay read-only. An owner must
separately configure an output-write endpoint, which binds only physical
projects that received `grant_output_write` at sealed startup. A dedicated
output protocol will mint its own per-stream receipt and inventory only the
granted, publicly exposed project IDs. A build request accepts the output
receipt and opaque project ID, then calls the already gated core owner
build/staging method on the session thread. Its bounded reply contains a
compiler check and publication status, never emitted artifacts, output paths,
or a caller-supplied destination. MCP will attach this endpoint only through
an explicit owner path and advertise its build tool only when that attachment
is present. A successful or failed build revokes prior query-generation
evidence for that project; the independent receipt prevents a query session
from being interpreted as write authority. The launcher relay, if used for
this endpoint, must pin accepted output streams to one core generation.

**F3.2.1.2.1 output wire protocol:** `compiler_output` v1 is independent of
the query-only compiler protocol and has a 64 KiB frame limit. It admits an
exact-version Hello only with a core-minted `ow-`-prefixed 32-byte receipt;
the query protocol's 64-hex attestation cannot pass this receipt's structural
check. Post-Hello requests contain only that receipt and, for build, an
opaque project ID. Unknown path, source, resolver, and option fields are
rejected on decode. The build result carries only its exact generation,
bounded project fingerprint, diagnostic status, and publication bit. No
listener consumes this new vocabulary yet; F3.2.1.2.2 must bind it to the
sealed owner's grant map and staging method.

**F3.2.1.2.2.1 sealed output owner:** The sealed core compiler session now
handles output-protocol requests independently of its query adapter. A
stream's output receipt must match exactly before its inventory is created;
that inventory contains only publicly exposed projects with separate
physical output grants. Build requires that same stream's inventoried ID,
uses the existing staged owner build, returns only generation/fingerprint and
publication status, and revokes old query cursor evidence because the build
may advance the compiler generation. Diagnostic-bearing builds publish
nothing. Closing the output session drops its project inventory. No socket
can reach this owner handler until F3.2.1.2.2.2 connects it.

**F3.2.1.2.2.2 core output socket:** Core accepts an independently configured
`--compiler-output-socket` only with an owner catalog and at least one exposed
physical write grant. It binds the endpoint with explicit `0600` permissions.
Each accepted output connection mints a fresh `ow-` receipt after exact
protocol Hello. Its worker owns framing only and queues output requests on
the same channel that the core session thread drains for compiler queries;
no listener thread owns compiler cache, grant map, or filesystem publisher.
Disconnect drops the stream inventory, and core shutdown removes the socket.
A real subprocess covers valid publication, rejection of a query receipt,
permission mode, cleanup, and refusal to start an output listener with an
ungranted owner catalog. MCP still has no output adapter until F3.2.1.3.

**F3.2.1.3 optional MCP build attachment:** A new explicit constructor pairs
the browser and query sockets with the separately configured output socket.
The output client validates its independent Hello receipt, accepts only a
well-formed granted-project inventory, and checks each build result against
the requested project. MCP's active tool router omits output capabilities,
output inventory, and build entirely when no output connection was attached;
a direct call to the absent route fails. An attached client must supply the
`ow-` output receipt and inventoried project ID to build. Before that request,
MCP revokes its old query-generation evidence under the same lock used by
query operations, so a build cannot leave a usable stale static receipt.
The build result is source-free and omits the physical output path. A real
MCP/core test covers the advertised tools, wrong query receipt, inventory
gate, publication, and stale query generation.

**F3.2.2 query receipt denial:** The read-only query attestation cannot act
as an output receipt at either boundary. A real core output stream rejects it
for both inventory and build after a valid output Hello; a real MCP client
repeats it as `bluetsc_build.output_session_id` after obtaining the granted
project inventory. Both attempts leave the output root empty, and the next
valid build increments the compiler generation only once. The query-only MCP
router also has no build route. This proves that a successful query check or
project receipt never implies write authority.

**F3.3.1 real MCP failure inspection:** A launcher-managed core now receives
two owner-selected virtual projects through its sealed catalog: one produces
a type diagnostic, and the other produces a reifiable local contract. A real
MCP client inventories and describes both project handles, checks each
generation, reads a nonempty diagnostic page, inventories and describes a
static type, then validates a mismatched JSON snapshot to obtain a concrete
contract failure. Source text and physical paths never appear in those
replies; the exact session/project/generation receipts gate every query.

**F3.3.2 real MCP check and build:** A separate real MCP/core fixture starts
an owner-granted physical project. The client inventories the read-only
project, checks it, then inventories the same project's independent output
grant and builds with its `ow-` receipt. The resulting source-free build
record repeats the observed graph/options fingerprint, advances exactly one
generation, and reports publication without revealing the output root. The
owner's immutable generation directory contains the emitted `main.js` for
the authorized module. The same fixture proves old query-generation evidence
is revoked after build.

**F3.3.3 real MCP rejection coverage:** The real MCP process fixtures reject
guessed and owner-private project IDs, unobserved static metadata IDs, wrong
session receipts, and stale generations following another check or build.
The output fixture rejects an unobserved output project and a read-only query
receipt presented as write authority without publishing an artifact. A real
client now also sends a contract string one byte over the MCP adapter's 256 KiB
limit. The adapter returns invalid parameters before compiler validation, and
the same session can still validate the inventoried contract afterward.

**G.1.1 workspace test gate:** A full `cargo test --workspace` run exposed
one outdated launcher assertion in the supervised-child end-to-end fixture:
an inline BlueTS compiler rejection now carries a verified original-source
range. The test now checks that the range is nonempty and within the exact
inline source, then compares the other report fields as before. Its focused
real-process test and the full workspace rerun pass. Both runs reused the
workspace's existing target directory.

**G.1.2 real-process suite gate:** The successful workspace rerun includes
launcher broker and supervised-child tests, real core-binary tests, MCP/core
client tests, and the launcher debugger socket suite. None of these process
test directories has an ignored test. The workspace result therefore covers
all four required process suites without a second build or redundant rerun.

**G.2.1 formatting gate:** `cargo fmt --all -- --check` passes after the
workspace test fix and process-suite evidence were committed.

**G.2.2 Clippy gate:** `cargo clippy --workspace --all-targets -- -D warnings`
passes after the completed workspace and formatting gates, using the existing
shared target directory.

**G.3.1 pinned TypeScript CI oracle:** The manually dispatched CI workflow
[run 36317372058](https://github.com/ephoton0210/blueice/actions/runs/36317372058)
checks out `a9d85172d` and its
[oracle job](https://github.com/ephoton0210/blueice/actions/runs/36317372058/job/108614527760)
completed successfully. The successful step runs the repository's pinned
`typescript@5.9.3` matrix through `npm exec` with
`BLUEICE_BLUETSC_ORACLE=tsc`; it is the opt-in test excluded from the ordinary
workspace test command. Coverage remains a separate CI gate.

**G.3.2 coverage merge failure and recovery design:** The coverage job in
[CI run 36317372058](https://github.com/ephoton0210/blueice/actions/runs/36317372058/job/108614527803)
ran workspace tests but failed before reporting a percentage because
`llvm-profdata` found one raw profile with a corrupt header. An abruptly
terminated instrumented subprocess can leave no usable profile. The CI
coverage commands now use cargo-llvm-cov's `--failure-mode all`: valid raw
profiles still merge, corrupt-profile warnings remain visible, and the run
still fails if all profiles are invalid or the workspace 90% or BlueJS 88%
line threshold is missed. This does not skip a test or lower either threshold;
the workflow YAML parses and the installed cargo-llvm-cov CLI documents the
selected mode. The separate CI rerun is required before G.3.2 can be checked.

**G.3.2.2.1 Windows owner-output path aliases:** The same CI run exposed a
real cross-platform gap in the artifact planner: Windows x86_64 and arm64
accepted an existing test source spelled `nested/../main.ts` even though the
planner requires canonical physical identities. Windows verbatim paths can
retain raw dot segments, so `fs::canonicalize(path) == path` alone is not a
sufficient spelling check. The planner now rejects literal `.` or `..`
segments in the original project root or module ID before filesystem lookup;
Windows checks both slash spellings and Unix continues to treat backslash as
a valid filename character. The existing owner-output test covers root and
source aliases, plus a Windows backslash spelling, while retaining the valid
build, outside-root, collision, and strict-runtime checks. Five focused tests,
workspace all-target Clippy, and rustfmt pass with the shared target. A
Windows CI rerun is still required to close the platform gate. The related
macOS arm64 debugger tests separately reached `Completed` before their pending
breakpoints were armed, so their admission race needs its own leaf before the
full CI rerun.

**G.3.2.2.2 macOS HTTP fixture mode:** The macOS 26 x86_64 core-library job
in the same CI run failed the real-child source-cache test when its accepted
HTTP connection returned `WouldBlock` before any request bytes were read.
The fixture intentionally keeps the listener nonblocking while polling for
four connections, but each accepted connection now switches to blocking mode
with a five-second read timeout before reading its request. This preserves
the actual HTTP/child/cache path and bounds a stalled fixture. The focused
real-child test passes locally in the shared target; CI must still verify
the macOS behavior. The macOS debugger pending-admission failures remain a
separate release-gate blocker.

**G.3.2.2.3 debugger admission contract:** The completed macOS CI jobs for
run 36317372058 show the same failure on macOS 15/26 and x86_64/arm64: 10–12
real launcher debugger failures per job, chiefly `Completed` before an
expected `Pending` state or `InvalidExecutionState` when arming a verified
breakpoint. One macOS 15 x86_64 failure is a separate fixture `WouldBlock`.
The core gives discovery/configuration replies one extra session
turn, but the first request is often sent after its 25 ms idle poll has
advanced the just-admitted entry. The existing 64-deferral child budget
limits request-triggered holds; it does not cover the gap before the first
request. Repeating a test or increasing the idle timeout would not establish
a reliable admission boundary.

Add an explicit, owner-only debugger reservation before navigation. It must
identify one existing tab and its current document generation, apply only to
the next generation, and have a fixed core-owned expiry. Bind its release to
the same negotiated stream; successful entry/root/nested/source breakpoint
arming may consume it, while an explicit same-stream release lets an inventory
test prove ordinary completion. Stream disconnect, superseding navigation,
and timeout must also free the hold. No unrequested page execution is delayed.
Real-process tests must acquire it before navigation, then keep their exact
pending, arm, pause, resume, completion, stale-handle, and refusal assertions.
Unit tests must cover foreign-stream release refusal and automatic cleanup.

**G.3.2.2.3.2 admission route:** Public debugger v43 has exact-tab
`HoldNextDocument` and `ReleaseNextDocumentHold` requests with source-free
acknowledgements. Only a successfully negotiated stream receives the
core-local weak ownership marker. The core records the tab's prior document
generation, activates the hold for one successor generation, and starts a
five-second deadline at that admission. A reservation waiting for navigation
also expires after 30 seconds. A foreign stream cannot release the hold or arm
the held entry. Successful owner arming, explicit release, stream disconnect,
document replacement, and timeout all remove it. Its idle ticks use a distinct
executor hook, so the child's existing 64 request-triggered discovery budget
remains independent. The focused core tests cover missing/foreign authority,
wrong tab, duplicate reservation, generation replacement, disconnect, timeout,
and foreign arm refusal. The IPC suite (127 cases), core debugger tests (33),
targeted all-target Clippy, and a real launcher/core/child pending-to-completed
module test pass from the shared `target` directory. Remaining real debugger
fixtures must reserve before navigating and retain their exact assertions.

**G.3.2.2.3.3 real debugger acceptance:** The affected launcher/core/child
fixtures now complete their negotiated `Hello` and reserve the exact next
document before browser navigation. This covers source and linked-source
breakpoints, module entry and stepping, nested frames and cutover, bounded
values, scope relations, stack coordinates, source spans, and type/value
display. Inventory-only code explicitly releases its hold before checking
ordinary completion. All original `Pending`, arm, pause, resume, completion,
stale-handle, and refusal assertions remain exact. The full 33-case debugger
suite passes without ignored cases. One macOS 15 x86_64 failure in that suite
was separate: the policy-isolation fixture accepted a nonblocking TCP stream
and its bounded read returned `WouldBlock`; it now makes the accepted stream
blocking before reading.

The first workspace rerun exposed four `spawn_core` ten-second reply timeouts
when seven tests concurrently started full core/child process trees. The
real-process test binary now serializes those trees with a local mutex; all
seven pass with their original reply timeouts. The subsequent full workspace
suite passes, including all 33 debugger and seven `spawn_core` cases.
Workspace all-target Clippy with warnings as errors, rustfmt, and whitespace
checks pass. All Cargo commands reused the existing `target` directory; no
instrumented or alternate local build tree was created. The remaining proof
is CI on the repaired commit for macOS, Windows, workspace coverage, and the
independent BlueJS coverage threshold.

**G.3.2.2.3.4 exact-tab reservation:** A second review found that the first
reserved-execution hook still used the old global one-turn scheduler flag.
It could pause unrelated tabs for the reservation's five-second window even
though the public request identified one tab. The core now passes the tab ID
to a separate reserved-tick hook. Both native and supervised-child execution
drivers skip only that tab and let other live documents advance. Focused
two-tab tests prove native `Pending`/`Completed` separation and the child's
exact advance requests without spending its 64 request-triggered discovery
deferrals. All focused tests and workspace all-target Clippy pass in the shared
target. A later full workspace rerun reached 32/33 debugger cases before a
separate one-turn transition-observation race: a real socket's immediate
post-resume `GetExecutionState` saw `Completed` instead of the promised
`Resuming`. G.3.2.2.3.5 will bind that short observation window to the same
stream and exact program, with a deadline and disconnect cleanup, before the
next full gate.

**G.3.2.2.3.5 observable transitions:** The follow-up workspace run found a
real post-resume race: `ExecutionResumed` succeeded, but the same socket's
immediate state query sometimes saw `Completed` instead of the promised
`Resuming` because its only scheduler hold lasted one 25 ms session turn.
Core now records a bounded transition for the exact negotiated stream and
program, or the linked frame for linked resume. An exact transition-state
reply to that stream consumes the hold; a different stream or program cannot
consume it. Disconnect, document replacement, and a one-second deadline also
release it. The tab-scoped executor hook keeps unrelated tabs running, and a
set of held tabs supports independent transitions on multiple live tabs.
Direct in-process callers without a negotiated stream keep the earlier
one-turn behavior. A real launcher/core/child test deliberately waits 75 ms
after `ExecutionResumed`, then still requires `Resuming`; unit coverage checks
foreign/wrong-program observations, linked-frame observation, replacement,
timeout, and disconnect.

The next full workspace run found two `broker_end_to_end` teardown checks
failing when nine independent real launcher/core test trees ran concurrently:
the private child socket still existed after the test's unchanged five-second
shutdown wait. That process-heavy test binary now serializes its outer cases,
while its simultaneous-cutover test still runs concurrent operations inside
one case. All nine focused cases pass. The subsequent full workspace suite
passes, including the 33-case debugger suite and seven `spawn_core` tests;
workspace all-target Clippy with warnings as errors and rustfmt pass. CI is
still required to prove the macOS/Windows matrix and both coverage floors on
the repaired commit.

The prioritized completion worklist is [TODO.md](TODO.md). Update it with this plan when an implementation or acceptance condition changes.

The supervised-child route now also has its first concrete external-resource
authority: `HttpOutOfProcessPageScriptSourceAuthorizer` is immutable core
startup configuration, never page/frontend/child/MCP input. It fixes either a
canonical same-document-origin rule or one canonical exact origin, a canonical
URL-to-lowercase-SHA-256 manifest, and fixed module/depth/per-module/graph-byte
bounds. It accepts only direct `200`, identity-encoded UTF-8 responses with an
allowed JavaScript or BlueTS MIME type and a matching bounded `Content-Length`;
redirects, missing integrity, cross-origin targets under the same-origin rule,
ambiguous URL forms, and over-limit graphs fail closed. Core parses only the
manifest-covered static JavaScript/BlueTS edges, uses deterministic
`(canonical URL, expected SHA-256, language MIME lane)` cache keys, and derives
the resolver fingerprint from the full policy. It transfers only that closed
graph to the child; the child still receives no HTTP/cache/manifest/resolver
capability. A real loopback HTTP plus supervised-child test covers classic,
JavaScript module, and BlueTS module graphs; cache reuse; integrity, MIME,
redirect, and cross-origin denials; and source-free reports.

`blueice-launcher` now also has an isolated `blueice-bluejs-host` child behind
a private versioned, per-spawn-capability-authenticated IPC protocol. The child
owns bounded BlueJS tab realms and accepts only complete caller-authorized
source/resolver graphs, returning source-free outcomes and aggregate accounting.
An explicitly configured core route now forwards one loaded HTTP(S) document's
inline JavaScript and explicit BlueTS declarations as core-minted closed
one-module graphs in original DOM order, and closes their child realms on
navigation or tab removal. The BlueTS child profile is fixed to checked direct
lowering with no ambient declarations or page-selected resolver/compiler
options. Its v3 document record has no profile/capability selector: the core
must validate and supply exactly one copied document-text snapshot and one
canonical HTTP(S)-origin snapshot, and the child repeats their fixed byte and
canonical-spelling checks before installing the two primitive callbacks.
Navigation and close destroy that VM and its copied strings. The shared fixed
typing artifact is generated from the same callback inventory and is the only
ambient declaration admitted to the child BlueTS compiler. The
explicit
`blueice-launcher --out-of-process-bluejs` mode creates that endpoint and token
per core generation, passes them only to its core child, and retains the
`SpawnedBlueJsHost` supervisor through normal shutdown or a cutover. Neither
the launcher CLI, frontend/page traffic, nor logs can configure or reflect this
capability; the default launcher still does not select the mode, so normal page
scripts do not run out of process by default.

The opt-in debugger socket now additionally supports source-free opaque
program-location enumeration and exact compiler-verified BlueJS safe-point
validation for a live JavaScript realm. Every request remains tab, document,
and program-generation bound. It also owns a bounded, idempotent exact
breakpoint-configuration table that is cleaned up on realm replacement. When
the same core process selects `--inline-bluejs` or its trusted out-of-process
child route and a debugger socket, the
post-navigation admission turn does not immediately execute the declaration;
each handshaken bounded discovery/configuration request retains one further
session turn so the peer can learn and arm its exact root code-unit safe point.
For the out-of-process route those deferrals have a fixed 64-turn budget per
document, so repeated discovery cannot become an execution lease.
`ArmEntryBreakpoint` remains the instruction-zero compatibility form. v5
`ArmRootSafePointBreakpoint` starts a pending classic declaration, then stops
immediately before its verified non-entry root instruction and retains the
actual root interpreter frame until one resume. The owner can observe only
source-free state and resume it once. This is not arbitrary interpreter
suspension: modules/top-level await, child code units, re-arming/loop hits,
stepping, stack/scope/exception/object inspection, source or bytecode access,
and runtime-value exposure remain absent.


## Archived from TODO.md: K.11 detailed checklist and dated journal (2026-10-13)

Moved verbatim; TODO.md now carries only the condensed K.11 below.

### K.11 Source maps — M9 — gap G-E5 (L)

Token-level mappings with `names` and `sourcesContent`; compared with `tsc` by decoding both maps and
comparing the mapping sets for the supported constructs (not the text); debugger mappings for generated
helper frames and the relocated decorator text.

**Minimum acceptance contract (all boxes required; K.11 is open).** This is a
bounded JavaScript Source Map v3 claim for the committed matrix below, not a
claim that every TypeScript construct or every emitted byte matches `tsc`.
Each row uses the same explicitly recorded compiler options against pinned
TypeScript 5.9.3 and BlueTSC. A green sample position is not a green row.

- [x] **K.11.1 Freeze the public oracle matrix.** Commit fixtures and at least
  eleven machine-readable rows, one for each of: copied code; erased
  annotations; a standard method decorator; whitespace-compacted relocation;
  erasure inside relocation; composed class and two member decorators; a
  legacy class and member decorator; legacy parameter plus emitted metadata
  helpers; a non-decorator generated helper; a System-module wrapper; and CRLF plus a
  non-BMP character. Record target, module kind, decorator flags, source-map
  options and expected accept/reject verdict per row. Every required row must
  run in the pinned oracle through the public compiler API; external and
  inline publication must also be exercised through BlueTSC's CLI. No
  missing, silently skipped or selectively filtered row counts as passing.
- [x] **K.11.2 Compare complete decoded mappings within that matrix.** Decode
  both Source Map v3 streams, validate monotonic generated positions and VLQ
  fields, and compare every emitted non-trivia token and every unmapped/mapped
  transition, not only hand-picked debugger sites. Where the emitted JavaScript
  bytes are identical, compare the entire decoded segment sets (normalizing
  only source-root paths). Otherwise align every corresponding emitted token
  by a checked token/role table and compare source identity, original line,
  UTF-16 column and optional name index; classify each unmatched token as
  source-anchored or generated-only with a measured expected mapping state.
  Unaligned or unexplained differences fail the row. Do not compare raw VLQ
  text or accept fewer segments merely because a few inspected positions agree.
- [x] **K.11.3 Verify map metadata and publication.** For each applicable
  matrix row, check `version`, `file`, `sourceRoot`, `sources`, `names` and
  `sourcesContent` against Native after documented path normalization; check
  both external and inline map delivery, and `inlineSources` on/off. Include a
  nonempty named-segment encode/decode regression even if pinned Native emits
  an empty `names` table for a given row. Verify CRLF and astral characters use
  source UTF-16 columns. A source-map resource-limit failure must return a
  bounded diagnostic without a partial published map; enabling maps must not
  change executable JavaScript apart from an explicitly requested map trailer.
- [ ] **K.11.4 Prove generated and relocated debugger positions.** Across the
  matrix, generated helper definitions, wrapper scaffolding and synthetic
  punctuation must match Native's measured mapped/unmapped state; helper calls
  must anchor to the measured source token, including standard and legacy
  decorator class/member/parameter/metadata cases and the non-decorator
  helper. Relocated decorator expressions must retain their original token
  positions through compaction and erasure. Exercise the public debugger's
  breakpoint/step and stack-location lookup at mapped, relocated and unmapped
  sites; no synthetic helper body may be reported as user source.
  - [x] Install the exact BlueTSC-emitted JavaScript and its validated map as
    one generation-bound page-host debugger artifact; retain the generated
    source hash and reject stale, substituted or externally asserted maps.
  - [x] Bind generated token positions to exact BlueJS AST/bytecode safe
    points. Do not use a nearest-token or nearest-instruction guess.
  - [x] Extend BlueJS debugger suspension from one directly called child to
    the real decorator path `root → IIFE → class static block`. Preserve both
    suspended callers' operand stacks, instruction budgets, exception state
    and program generations; prove pause, step, stack projection, normal
    resume and thrown completion without replaying decorator side effects.
    A compiler-known target beyond the supported depth must continue to fail
    closed at resolution/arm.
  - [x] For the required System-wrapper row, pause at a mapped execute-body
    safe point reached through a separately installed page-loader program.
    Preserve the suspended cross-program caller and both exact program
    generations; require same-stream receipts before a public stack or
    original-location projection can name either frame. Prove step, resume
    and thrown completion without replaying loader or wrapper effects.
    The single-program classic stack route must not relabel a foreign frame
    as the emitted program. Until this is implemented, generated and raw
    arming must both fail closed without retaining a breakpoint.
  - [x] Route the private v48 System arm through the core-owned page-host
    adapter. Remint both exact program generations, reject a same-program
    loader and stale core identity before transport, validate the emitted
    safe point, and accept only an acknowledgement echoing the complete
    tab/document/loader/target tuple. Keep the public System route closed.
  - [x] Capture an exact private three-frame System pause with independent
    emitted/loader/emitted program generations. Bind the retained invocation,
    reject a stale loader or changed top frame, validate all three safe points
    in core, and leave the ordinary classic stack unable to relabel the
    foreign intermediate caller.
  - [x] Resolve a mapped System execute-body generated token only under the
    exact earlier pending loader. Reuse the artifact-bound AST/bytecode match,
    return unbound for wrapper scaffolding, echo both program generations and
    coordinates, reject substituted replies in core, and retain no breakpoint.
    Keep ordinary generated lookup and raw arming fail-closed for System.
  - [x] Project the exact private System pause through its bound emitted map
    as one complete three-frame response. Reuse the verified AST/bytecode
    token projector for emitted child and root, keep the foreign loader
    source-free, compare the complete stack and original coordinates in core,
    and reject a moved pause or substituted child location.
  - [x] Add the read-only public System three-frame request and core route
    under the independently default-denied paused-location grant. Require
    separate emitted and loader receipts on this exact debugger stream before
    touching the page host, verify the active core frame and both exact
    program generations, reject a substituted source position, and never
    project a BlueTS coordinate onto the foreign loader. Keep System
    resolution and arming closed until their own public checks are complete;
    real-process positive disclosure belongs to the remaining System item.
  - [x] Resolve and arm only an exact mapped System execute-body token through
    the explicit public dual-program route. Require independent same-stream
    emitted/loader receipts and the default-denied generated-breakpoint
    grant, keep wrapper and generic/raw System arming unbound or denied, and
    preserve owner-stream admission holds. Prove a real-process pause,
    generated/original three-frame projection, nested step and normal resume
    without replaying the loader, wrapper or mapped call.
  - [x] In the real-process System route, reject stale emitted and loader
    generations at both resolution and arm without retaining a breakpoint.
    At a valid three-frame pause, require the old single-program generated
    location route to reject the foreign loader frame instead of relabeling it.
  - [x] With the same runtime-adapted artifact derived from the frozen System
    row and a throwing page loader, pause at the mapped execute-body token,
    project the three exact frames, and resume through a caught thrown
    completion. Verify the exact thrown value and that register and console
    each run once; the BlueTS
    script must report successful execution after the foreign caller catches.
  - [x] At the core admission boundary, prove a substituted cross-stream
    System arm cannot consume the owner stream's hold even when both sessions
    independently negotiated the generated-breakpoint grant and receipted
    both emitted and loader programs. Keep the current single-client listener
    limitation explicit; this is an equivalent core-level admission test,
    not a concurrent public-socket demonstration.
  - [x] Repeat the public System pause, source projection and continuation
    with the unmodified frozen `system_wrapper.ts` source and its exact
    BlueTSC-emitted artifact/map. Supply any ambient `console` declaration
    separately rather than appending text to that source, and verify its
    generation-bound installation before checking this parent System row.
  - [x] Repeat the emitted-ESM public debugger pause, generated step and
    original-location projection for the unmodified frozen `basic.ts`,
    `erased.ts` and `crlf_astral.ts` sources. Select the page `console` type
    ambient through an owner-only policy, keep it out of the source graph and
    runtime, and compare the installed JavaScript, complete map, source hash
    and `sourcesContent` with independent BlueTSC output for each row.
  - [x] Repeat the exact-source emitted-ESM public debugger path for all four
    standard-decorator rows: `decorated.ts`, `decorated_parenthesized.ts`,
    `decorated_erased_assertion.ts` and `decorated_multiple.ts`. Verify each
    installed artifact and complete map against independent BlueTSC output,
    keep the page console separate, and resolve, pause, step and project all
    six relocated method/class decorator tokens while leaving the synthetic
    helper definition unbound. Retain the actual two- or three-frame caller
    chain without replaying effects or inventing a source location after step.
  - [x] Repeat the exact-source emitted-classic public debugger path for
    `legacy_decorated.ts`, `legacy_parameter_metadata.ts` and
    `object_spread_helper.ts`. Supply the page `console` type ambient only
    through owner policy, outside the frozen source graph and runtime; compare
    installed JavaScript, complete map, source hash and `sourcesContent` with
    an independent compilation. Resolve, pause, step and project the legacy
    member/class and parameter helper calls and the ES5 object-spread call;
    keep helper definitions and synthetic metadata unmapped/unbound, preserve
    the actual stack and do not inherit a source position after stepping.
  - [x] Add independently default-denied, same-stream receipted public
    debugger operations for generated breakpoint resolution/arming and
    paused step/stack-location projection. Unmapped helper bodies and
    wrapper scaffolding must stay unbound.
  - [ ] Pass out-of-process public debugger regressions for mapped,
    relocated and unmapped sites, including stale generation, wrong artifact,
    denied capability, malformed map and helper-frame non-disclosure. The
    eleven frozen rows now have exact-source positive-path coverage. Before
    checking this item, add a real launcher/core/child public-session fault
    regression for a malformed retained map (or a test-only transport proxy
    corrupting its lookup reply) and a substituted artifact/map. Assert that
    resolution, arming and paused projection disclose no partial source and
    retain no breakpoint. Existing child/core public-adapter unit tests are
    necessary but do not themselves meet this out-of-process acceptance.
- [x] **K.11.5 Verify executable and regression behavior.** For every accepted
  fixture, execute Native and BlueTSC output with the recorded Node version and
  compare exit status, stdout and stderr; retain stable map output across
  repeated builds. Run focused source-map, emitter, declaration-map,
  module-wrapper and debugger regressions. A row cannot be accepted by
  changing the fixture or weakening its comparison without a recorded Native
  re-baseline and review.
- [ ] **K.11.6 Pass the final-source gate.** On the same committed source,
  pass the complete K.0 local gate (format, warnings-denied all-target Clippy,
  workspace tests including explicitly run ignored pinned oracles and coverage)
  and the hosted platform build/test/lint, pinned TypeScript oracle, coverage
  and final CI-gate jobs. Record the exact commit, oracle versions, commands,
  fixture count, coverage and hosted run in PLAN.md. Earlier K.10 or
  intermediate K.11 green runs do not close this item.

**Exit rule:** check K.11 only when K.11.1–K.11.6 are all checked and there is
no known red or unclassified mapping inside the frozen matrix. Any other
supported source-map form remains a named coverage gap (or a precise refusal),
not an implied parity claim; changing the minimum matrix requires updating
this contract and the baseline before claiming closure.

*Current implementation boundary:* The eleven-row pinned compiler oracle and
external/inline CLI publication checks pass locally. A trusted classic-script
page-host request now executes the exact child-compiled emitted JavaScript and
retains its validated map under the live generated-source hash and program
generation. The child-private lookup binds mapped, parser-proven top-level
statement starts to compiler-recorded root safe points and can bind a unique,
literal identifier expression to its exact compiler instruction when the
code unit is within the VM's bounded two-level nested depth. A real
child-process test covers root breakpoint, pause, stack snapshot and one
instruction step. The relocated standard-decorator identifier is now
parser- and compiler-recorded and executes inside an IIFE's class static
block at depth two. BlueJS retains the intermediate IIFE continuation, and
the public launcher/core/child test resolves and arms that exact coordinate,
pauses with a three-frame stack, projects the original decorator token and
returns no original location after an interior step. Other mapped inner
tokens, ambiguous names and unmapped helpers also remain unbound
instead of being guessed. The child projects a paused root or directly nested
instruction back to an original location only when it has a unique exact
binding; an interior step remains unlocated, and a paused generated helper
retains no user-source location. The core
page-host adapter transports those three private emitted-map operations
through core-reminted program identities and rejects a substituted child
generation or reply tuple. Public debugger v45 now has independent,
default-denied owner/client grants for generated breakpoint resolution/arming
and paused-location projection, each requiring a program identity
receipted on that debugger stream. A trusted, default-off launcher/core option
selects the emitted-artifact route for classic BlueTS page scripts; module
scripts retain the direct debugger route. Separately, an explicit
`BlueTsEmitted` module request now compiles every authorized module in the
child, validates each child-owned map and generated source hash, rewrites
static resolution keys with BlueTSC's exact emitted-specifier rule, and
executes the emitted ESM graph in ordinary page-host mode. The supervised
child-process import test passes. Module parsing now also records exact
UTF-8 byte spans for executable body items and identifier expressions while
excluding static-only import/re-export headers. The child retains those
parser-proven spans with each validated module map; its admission test checks
that every span count matches the emitted module's body and every identifier
span slices the emitted token exactly. Ordinary emitted-module execution also
requires the validated-map and preflighted-program canonical ID sets to
match, not merely their counts. An explicit child-private debugger-controlled
`BlueTsEmitted` module request now atomically installs the preflighted ESM
graph, registers each validated map under that module's live opaque program
generation, and rolls back every installed module if graph admission fails.
The child regression checks a mapped imported identifier, entry pause with
exact original-source projection, substituted-map refusal before install,
and resource-limit rollback. The supervised child-process IPC regression also resolves a mapped emitted ESM
statement, arms and pauses it, projects its TypeScript coordinate, and
resumes successfully. Separate default-off launcher/core selectors now choose
emitted classic scripts and emitted ESM graphs independently; absent the
module selector, browser module debugger traffic still uses the direct route.
The first real launcher/core/child public-socket emitted-module regression
resolves and arms a mapped entry statement, pauses, projects its original
TypeScript coordinate and resumes. A second module-specific public regression
proves that selecting this route alone grants neither generated breakpoint
nor paused-location metadata access. There is no silent fallback for an
explicit emitted request. Real
launcher/core/child public-socket tests
cover a mapped classic statement, an unbound inner token, arm/pause/stack
projection, an unlocated interior step, denied grant, wrong program artifact,
malformed generated coordinate, stale navigation generation, and a decorator
helper pause with no original-source disclosure. The decorator public-process
test also measures one relocated token's original source-map coordinate and
requires a positive pause at its exact nested safe point. Child-private map/AST
contradictions are rejected by an additional public
paused-projection cross-check. Debugger v45 and page-host v46 describe the
root-or-nested generated-location reply. Targets beyond the verified
two-level nested continuation still fail closed. Public malformed-
map real-process regressions, emitted-module dependency and negative public
debugger regressions, matrix-wide public debugger
coverage, and the final K.0/hosted gate
are still open. On the current uncommitted source, the pinned TypeScript 5.9.3
23-test oracle suite proves complete decoded mapping/transition comparison for
all eleven rows, and the three public CLI publication tests prove the 44-mode
external/inline and `inlineSources` matrix. K.11.2 and K.11.3 are therefore
checked; the bounded multi-frame subitem is also checked, but K.11.1,
K.11.4, K.11.6 and K.11 as a whole remain open. The same 23-test oracle runs
every accepted fixture under recorded Node v26.7.0, compares exit status and
output, and checks deterministic repeated builds. Focused source-map lookup,
System wrapper, emitter, declaration-map and debugger regressions also pass,
so K.11.5 is checked. These results do not authorize a commit.

*K.11 test-first baseline (2026-10-12):* The pinned 5.9.3 checker accepts two
unique fixtures, and Native/BlueTSC Node output, inline source content and
`names` agree. The decoded, token-aligned mapping oracle is deliberately red
(32 versus 7 raw segments for copied code; 39 versus 10 for erased
annotations). Token-level emission, generated helper/decorator provenance
and the complete K.0 gate
remain open; see PLAN.md.
The earlier map-structure and VLQ-encoder change added optional named segments
with a passing serialization regression; that prerequisite alone left emitted
maps byte-stable.
The two original pinned token-aligned fixtures now pass with copied-token
provenance and bounded segment retention. Generated helper/decorator mappings,
broader syntax, actual named marks, the complete K.0 gate and hosted CI remain
open; see PLAN.md for the measured slice and historical red baseline.
A third pinned fixture now verifies one standard decorator's relocated token,
unmapped helper definition and method-anchored helper call. Other helper
families, non-exact relocation, multi-decorator composition and the full gate
remain open.
A fourth pinned fixture now matches Native token columns through whitespace-
compacted parenthesized relocation; changed-token rewrites retain conservative
range anchors. Other internal rewrites, helper families, multi-decorator
composition and the full gate remain open.
A fifth pinned fixture now covers a type assertion erased inside a relocated
decorator expression. BlueTS preserves the surviving identifier's Native source
column and the expression-end boundary; edits that rewrite text or splice a
token still use conservative range anchors. TypeScript also removes this
fixture's outer parentheses, so generated-byte and full mapping-set parity for
this expression remain open, as do broader internal rewrites and the K.0 gate.
A sixth pinned fixture composes one class decorator with two method decorators.
The relocated expressions and method helper already mapped correctly, but the
class helper inherited the method anchor; it now uses the `class` keyword
token in the checked header. Other class/member orders, generated helper
families, full mapping-set parity and the complete K.0 gate remain open.
The seventh pinned fixture covers a legacy class plus method decorator. Its
generated `__decorate` definition is now unmapped, while the member and class
helper calls carry distinct Native-compatible source anchors; relocated names,
runtime output and inline map content also agree. Legacy array punctuation,
parameter/metadata helper frames, other helper families, the full mapping set
and K.0 remain open.
An eighth fixture adds CRLF source lines and an astral character to the
machine-readable token oracle (three rows total). All three rows now compare
every emitted token's decoded mapping, map metadata after source-path
normalization, runtime status/stdout/stderr, map-on/off JavaScript, repeated
map output and `inlineSources` on/off. The test decoder rejects malformed VLQ,
non-increasing generated columns and out-of-range source/name indices. This
does not close K.11.1's eleven-row minimum or the decorator-wide mapping,
CLI-publication, debugger and final-source gates.
The legacy pinned oracle now also verifies both generated decorator-array
closing brackets are unmapped. A ninth fixture covers legacy parameter and
metadata helper definitions, calls, relocated decorator text and runtime
output; a tenth covers a non-decorator ES5 object-spread helper definition and
call. Native-compatible provenance markers fix the measured helper frames
without changing map-disabled JavaScript. These remain site-level oracles;
the full mapping-set, System wrapper, CLI/debugger and final gates are open.
A separate System-module fixture now executes to the same `42` through a
closed Node loader in both compilers. The generated `System.register` site is
unmapped, while the hoisted function and execute-body call agree with Native
original-source positions; map-on/off JavaScript is unchanged. This is a
site-level wrapper control, not full System mapping-set parity or K.11.1's
machine-readable eleven-row matrix.

Additional standard-decorator provenance now marks the generated class IIFE,
metadata scaffolding, decorator-array setup and synthetic constructor as
unmapped, while preserving the measured decorator-expression and helper-call
anchors. A pinned site oracle verifies six previously overmapped scaffold
positions against Native. Selective class-name/start/end, initializer-call,
member-call-tail and empty-call boundaries now make seven transformed rows'
ordered source-anchored token-role sequences match Native: standard method,
compacted relocation, erased relocation, composed decorators, legacy
class/member, legacy parameter/metadata and ES5 object spread (108, 110, 108,
198, 81, 83 and 42 mapped tokens respectively). The erased-assertion row
now removes the same
synthetic parentheses as Native. A decoded-token transition diagnostic shows
why the role sequence alone is insufficient: TypeScript's generated helper
definitions precede the first user token, whereas BlueTSC emits its own
generated helpers later. The System wrapper's duplicate CommonJS prelude is
now removed and its function-body open matches Native; the row still has 35
Native versus 31 BlueTSC source-anchored tokens with differing export-call
shapes. Generated-only token roles and their measured mapping state across
all divergent layouts still need checked alignment. All eight transformed
matrix rows remain red under K.11.2.

The matrix now records all eleven required fixture categories with explicit
target, module, legacy/metadata flags, external-map and `inlineSources` modes,
line endings, verdict and runtime loader. Its pinned public-API replay checks
metadata, both `inlineSources` states, repeated maps, map-on/off JavaScript and
Native/BlueTSC Node results for every row. The first three copied/erased/CRLF
rows pass token-aligned mapping checks; copied and erased rows also match the
entire decoded mapping set when their JavaScript bytes are identical. The
other eight currently fail because
their emitted JavaScript token streams differ and have no complete checked
token/role alignment table. The failure reports each row instead of accepting
sampled positions. Therefore K.11.1 (which requires committed fixtures) and
K.11.2 remain unchecked, as do the debugger and final-source gates.

Public `bluetsc --project` now emits external `.map` files or inline data URLs
for all eleven pinned matrix inputs and both `inlineSources` states (44
publications), with Native-matching map
metadata and Node output.
Managed `bluetsc build` also switches external → inline → external without
retaining a stale map file; conflicting `sourceMap`/`inlineSourceMap` settings
publish nothing. Decoder tests cover named segments and malformed indices or
VLQ, and a public compiler-limit test refuses partial artifacts. These pass
locally but are only a slice of K.11.3 and do not close K.11.
The next pinned System regression checks every token in the rewritten
`exports.answer` read/write against the original binding site, and the
initializer's export-keyword/name split against Native. The decoder now
rejects out-of-range original source lines/UTF-16 columns and mismatched
`sourcesContent` length. Focused module, System, decoder, CLI-publication,
format and warnings-denied Clippy checks pass. The complete oracle still has
eight red transformed rows, including System's 35-versus-31 mapped-token
callback/object-shape difference. K.11.1–K.11.6 stay unchecked and nothing
from this K.11 work is committed.
The System row now has an exhaustive, checked five-role table for all 35
Native and 31 BlueTSC mapped tokens, plus a mapped/unmapped check for every
non-trivia emitted token in both outputs. The pinned test corrects its Native
execute-region start to the second export call, so post-function scaffolding
and the suffix are actually covered. The eleven-row aggregate invokes that
full checker and reports seven, not eight, remaining divergent rows. This
does not close K.11.2: decorator and ES5-helper generated-only token roles
and transitions remain unclassified, and K.11.1/K.11.4–K.11.6 also remain
unchecked. No commit or push has occurred.
The ES5-helper row now classifies its entire helper prefixes, all 42
source-anchored executable tokens on each side and BlueTSC's four unmatched
generated-only flags. Six decorator rows now have checked source-anchored
role/position sequences plus a measured table of every generated-only token
run, its lexical fingerprint, count, placement and semantic role. An
independent decoded-state SHA-256 inventory covers all mapped/unmapped
transitions, including trivia, for all eleven Native/BlueTSC pairs. The full
eleven-row pinned oracle passes all 23 tests; the three CLI-publication tests,
28 emitter unit tests, declaration-map option regression, six module-wrapper
tests, format and focused warnings-denied Clippy also pass. K.11.2/K.11.3
have local acceptance evidence, but all boxes remain unchecked until the
committed matrix, public debugger route and exact final-source K.0/hosted
gate make the whole K.11 claim verifiable. No commit or push has occurred.
The new public source-map lookup validates the entire map and bounds both
decoded segments and total input bytes. Four ordinary API tests cover
relocated, helper, System-wrapper and UTF-16 sites, malformed maps and
resource limits. The pinned eleven-row oracle now cross-checks its result
against the independent decoder at every BlueTSC emitted token. The page
host's BlueTS execution still uses the direct AST bridge, not the emitted
JavaScript artifact, so no public debugger breakpoint/step/stack session is
yet bound to a generated source-map position. K.11.4 remains open; this
lookup is not a substitute for its end-to-end acceptance. No commit or push
has occurred.
An additional bare-VM feasibility test proves BlueJS can parse, compile and
execute the standard decorator's emitted helper syntax to result `42` after
replacing only its page-host `console.log` sink. The parser-to-safe-point
generated-position provenance and live artifact binding are still absent,
so this does not close K.11.4.
A page-host admission regression also executes the emitted decorator
JavaScript in a real child document and verifies result `42`; its map
resolves the relocated decorator identifier and leaves its helper definition
unmapped. The emitted artifact is still not a debugger-bound attachment, so
breakpoint/step/stack acceptance remains open.
Private page-host protocol v45 now has an explicit child-compiled
`BlueTsEmitted` classic route. Under debugger admission, the exact emitted
JavaScript/map pair is retained against its live program generation with
bounded map validation, generated-source hash checks, map-byte accounting
and realm-replacement invalidation. A private authenticated core/child
lookup returns original coordinates or unmapped for one generated UTF-16
position; unit and isolated-child IPC tests pass. This only advances the
first K.11.4 subcheck: the public capability/opaque receipt, exact
generated-token-to-safe-point binding, breakpoint/step/stack behavior and
emitted-module coverage were still missing at that stage. The core browser
adapter then still chose direct BlueTS lowering; the later owner-selected
emitted routes described below supersede that limitation. No commit or push
has occurred.

The latest emitted-ESM dependency regression binds a generated, mapped token
inside a hoisted dependency function to an exact BlueJS nested safe point,
then pauses and resumes the owner-authorized two-program graph through the
public linked debugger route. BlueJS now retains the owning root-statement
index while compiling that hoisted function; an unreceipted forged generation
is refused. Public debugger v46 now has a separately authorized, two-frame
`GetGeneratedLinkedStackLocations` operation. It requires the generated
paused-location grant and same-stream opaque program receipts for both
frames, compares the complete live stack before and after projection, and
returns an exact dependency source-map location while leaving an unbound
entry caller's location `None`. The ordinary linked-stack read stays
source-free; the older single-program generated-location operation still
refuses linked frames. Public tests reject a substituted frame, denied grant,
missing receipt and post-resume request without partial disclosure. This
does not yet cover every mapped/unmapped/relocated matrix site, the
malformed-map public fault path or K.11.6, so K.11 and K.11.4 remain open and
no commit is authorized.

The public generated-debugger suite also covers runtime-adapted copies of
the frozen parenthesized and erased-assertion standard-decorator fixtures.
For each, BlueTSC's bound map and the real launcher/core/child breakpoint,
nested pause, original-location projection and continuation agree on the
relocated decorator token; the eight-case generated public group passes.
The copies change only the final unsupported `console.log` sink, so this is
not exact frozen-row debugger coverage. Composed/legacy decorator and
parameter/metadata helpers, ES5 object-spread and System scaffolding, and
the public malformed-map fault path remain explicit K.11.4 gaps.

The private page-host v47 request now carries an owner-only, default-standard
emission profile. A launcher startup opt-in selects emitted classic legacy
decorators plus metadata, while emitted ESM remains on its independent
standard profile. A runtime-adapted legacy parameter fixture executes in the
child, and its public generated breakpoint binds, pauses and projects the
`__bluetsParam` call to `@parameterDecor`; the generated helper definition
and metadata call remain unmapped and unbound. The complete generated public
group passes 9/9, focused child legacy tests 2/2, owner CLI tests pass,
IPC 139/139, launcher child-host library 183/183, three-crate all-target
check and warnings-denied Clippy pass. The adapted
copy is not an exact frozen-row run; complete matrix-site debugger coverage,
ES5/System public sites and malformed-map fault injection remain, so K.11.4
and K.11.6 are still unchecked. Do not commit yet.

The frozen legacy class/member row cannot use the metadata profile because
its method has no return-type annotation. A separate owner-only classic
legacy-decorators profile now preserves that row's compiler settings; both
legacy profile flags are mutually exclusive and require the supervised
private child. A runtime-adapted copy executes in the child. In two public
launcher/core/child runs, the member and class `__bluetsDecorate` calls
bind, pause and project to their exact original positions; the helper
definition and synthetic punctuation stay unbound. The generated public
group passes 10/10, IPC 139/139, launcher library 184/184, and related
all-target check/Clippy pass. This does not close K.11.4 or authorize a
commit: composed standard decorators, ES5/System public sites,
malformed-map fault injection, frozen-row completeness and K.11.6 remain.

The composed standard-decorator row now has a separate real-process public
regression: `outer`, `inner` and `classDecor` each resolve to their exact
relocated source token, pause, project and resume through the nested frame;
the synthetic `__bluetsEsDecorate` helper remains unmapped and unbound.
The full generated public group passes 11/11 and target Clippy passes. This
uses a runtime-adapted copy of the frozen fixture, so K.11.4 remains open
for exact frozen-row coverage plus ES5/System and malformed-map public cases.

The ES5 object-spread row now has an owner-only emitted-classic profile and
public debugger regression. BlueJS records the two nested same-name helper
callee safe points; only a parser-certified AST/token pair may bind them.
The generated helper definition remains unmapped/unbound, while the mapped
outer call binds, pauses and projects to the measured `merged` anchor. The
runtime-adapted fixture executes in the child; generated public tests pass
12/12. A focused child test proves both outer and inner callee tokens have
different exact, reversible safe points. The frozen source is unchanged.
K.11.4 remains open for System, malformed-map public fault injection and
complete frozen-matrix debugger
coverage; K.11.6 and its final-source gate remain open. Do not commit yet.

The System row now has executable child-host evidence for the exact frozen
fixture under a page-supplied ordinary `System.register` loader, plus an
owner-selected emitted-classic System profile and a real-process public
debugger regression on the same source with a trailing ambient `console`
declaration required by the page host's checked-global policy. The wrapper
token is unmapped and unbound; the generated execute-body `console.log`
token maps to its original UTF-16 source coordinate. Because execution
passes through a separately installed page-loader program, the current
bounded nested debugger cannot retain that foreign caller chain. Resolution
and arm now fail closed for this mapped nested System target instead of
returning a breakpoint that completes without pausing. The restriction is
bound to the System artifact's map; a mixed-document child regression proves
that an adjacent emitted ESM map retains its Standard profile. Raw nested
safe-point arming is also refused, with no breakpoint record retained; the
public test verifies this cannot bypass generated-position refusal. The generated
public group passes 13/13, and the child, launcher and core owner-profile
controls pass. Cross-program System suspension, full frozen-row debugger
coverage and K.11.6 remain open. This does not
check K.11.4 or authorize a commit.

The malformed-map fault path now has layered tests: test-owned corruption of
one live child-retained emitted map makes both private position and safe-point
lookups return `HostFailure` without a partial result or breakpoint; the core
adapter hides that private category at all three emitted lookups; a receipted
public debugger route returns only the generic unavailable error; and a real
`blueice-core` subprocess with a test-only child fault proxy proves the same
resolve/arm behavior over public and private sockets. The proxy delegates
real BlueTSC admission and program inventory to `BlueJsChildHost`, replacing
only map-lookup replies with the measured malformed-map error category. Two
private faults are observed, the raw public reply excludes the private text,
and the public breakpoint list stays empty. No production fault switch or
client-supplied map is added. The malformed-map public fault-path gap is
closed, but full frozen-row debugger coverage and K.11.6 remain open; do
not check K.11.4 or commit yet.

The copied, erased-annotation and CRLF/astral frozen source prefixes now
also cross the emitted-ESM child and public generated debugger. Each served
copy appends only a trailing ambient `console` declaration; an earlier page
script supplies the runtime `console` and verifies the expected output. The
independent BlueTSC map lookup checks both the final `console.log` and the
actual pause token against original UTF-16 coordinates. The public debugger
holds the document, inventories both programs, arms an exact executable
module entry, pauses, projects the actual instruction and resumes; all three
rows pass. Erased annotation revealed that a hoisted module function
declaration could falsely borrow the subsequent entry safe point and that
the `result` statement start coincides with its instruction-specific
`double(21)` callee. Parser-certified non-evaluating module declarations now
fail closed only in the root code unit; nested function bodies remain
debuggable. The compiler-recorded unique identifier wins over a coincident
statement boundary in paused projection, so erased reports `double`, not a
guessed `result` token. The generated public group passes 14/14, including
the prior linked dependency; launcher library passes 193/193 and focused
all-target Clippy passes. These are runtime-adapted prefixes, not complete
byte-for-byte public execution of every frozen row. K.11.4's remaining
matrix coverage and K.11.6 stay open; do not commit yet.

The three public frozen-prefix rows now additionally require the mapped pause's
one-frame stack, step one root instruction, check the stepped stack's new safe
point, and require no inherited original/generated token location at that
interior instruction. The fixed checked page-host ambient profile still has no
`console`, although every frozen fixture calls `console.log`; a trailing
declaration plus an ordinary page loader remains necessary. This precisely
separates runtime-adapted evidence from K.11.4's unaltered, exact-artifact
requirement. A second compile of each unaltered fixture shows that the
adaptation preserves the executable JavaScript after trailing-newline removal
and preserves every frozen mapping segment as a prefix, but adds emitted
trailing newlines, mapped suffix segments and different `sourcesContent`.
The test does not require that difference to persist if emission improves;
the actual served `sourcesContent` still distinguishes the adaptations.
K.11.4 and K.11 remain unchecked.

The locally recovered TypeScript 5.9.3 executable and Node v26.7.0 rerun
the complete source-map oracle on this uncommitted worktree: 23/23 tests pass
with `--include-ignored`, including all eleven frozen matrix rows. The public
BlueTSC CLI publication suite passes 3/3 with its ignored pinned test enabled,
covering 44 external/inline and `inlineSources` combinations. This strengthens
K.11.1–K.11.3 local evidence but does not satisfy K.11.1's committed-fixture
condition, K.11.4's exact page-host artifact, or K.11.6's final gate.

The legacy class/member public debugger regression now additionally proves
one-frame stack preservation and a single root instruction step at each
mapped `__bluetsDecorate` call. Both stepped interior instructions remain
unlocated rather than inheriting their call's original position. The focused
real-process test and all 11 `generated_` public regressions pass. A complete
workspace build and initial BlueJS suites passed, but the intermediate-source
workspace run was stopped before all integration suites; it does not satisfy
K.11.6. K.11.4 and K.11 remain open, and no commit is authorized yet.

The legacy parameter/metadata public regression now also checks its mapped
`__bluetsParam` pause's root stack, one instruction step, the stepped stack,
and no inherited original location at the interior instruction. The existing
unmapped helper-definition and metadata-call refusals remain intact. The
complete public out-of-process debugger suite passes 47/47. Exact frozen
page-host admission and matrix-wide coverage still keep K.11.4 open.

The System public regression additionally requires the realm breakpoint list
to remain empty after both refused raw and generated arming. A test-first
BlueJS VM regression exposed that the classic stack snapshot would otherwise
relabel a retained foreign intermediate frame with the root program's
generation. It now reads and validates the intermediate bytecode's actual
generation, rejects a missing or different generation, and keeps the linked
route separate. BlueJS library passes 1,150/1,150 and the public debugger
suite passes 47/47 on this fix. This is the fail-closed prerequisite for the
new System cross-program checklist item, not its positive completion;
K.11.4 and K.11 remain unchecked.

A separate VM and page-runtime seam can now pause a System-shaped execute
callback called through an exact, separately installed loader generation. It
retains child, loader and root with their actual generation IDs; the ordinary
classic stack still refuses the foreign loader. Focused VM regressions cover
step/resume and caught throw without replaying loader effects, and the page
runtime checks both live same-realm handles and the exact paused invocation
before its dedicated three-frame read. BlueJS library tests pass 1,153/1,153.
This is not yet a public System breakpoint: launcher/core/IPC need a
generation-bound loader receipt and per-frame same-stream projection before
the fail-closed System arming filters may be lifted. K.11.4 stays unchecked.

The frozen System fixture's actual BlueTSC output now also pauses at the
mapped `console.log` call through a separately installed System loader in a
page runtime; its retained stack preserves both program generations and the
loader/output effects occur once after step and resume. This is an internal
page-runtime acceptance, not a public debugger acceptance. The pinned
TypeScript 5.9.3 source-map oracle passes 23/23, CLI publication 3/3, public
source-map lookup 4/4, and out-of-process debugger regression suite 47/47.
K.11.1, K.11.2, K.11.3 and K.11.5 are complete for this committed matrix;
K.11.4 and K.11.6 remain open.

Private page-host protocol v48 now carries an explicit System loader-program
identity alongside the emitted execute-body safe point. The child accepts
only an earlier, still-pending JavaScript classic loader in the same exact
document, stores the association in its bounded pending execution, and uses
the cross-program page-runtime continuation when execution advances. A
test-first child scheduler regression pauses the frozen System fixture's
mapped execute token, checks the actual three program generations, resumes
without duplicate loader/output effects, and proves wrong/later/stale arms
leave no breakpoint. Ordinary raw System arm and the single-program stack
remain denied. This is a private transport and scheduler milestone only:
core has no receipted public System arm or three-frame original-location
projection yet, so the System subitem and K.11.4 remain unchecked.

Core now routes the private v48 System arm through its page-host connection
and debugger-location adapter. The adapter remints both core program IDs to
their exact child generations, rejects stale or same-program identities,
validates the emitted nested safe point, and accepts only a full-tuple child
acknowledgement. A default client remains unsupported. Focused core tests
cover the exact transport tuple, positive reminting, stale identity, root-target
rejection, wrong tab/document/loader/target echoes and the child-error path.
The engine library suite passes all 473 tests with local socket permission;
engine all-target Clippy and workspace formatting checks also pass.
This completes only the checked private core-routing subitem above; no
public System capability, same-stream loader receipt, or three-frame source
projection is available. The parent System item and K.11.4 remain open.

The private v49 System stack route now preserves a complete child-first
emitted/loader/emitted triple, with a generation-labeled safe point on every
frame. The child checks its retained pending loader association and exact
paused invocation before reading the VM stack; it refuses stale loader and
frame identities and keeps the classic one-program stack route unchanged.
Core remints both programs, checks the complete echoed tuple and shape,
validates all three safe points and rechecks the pause before returning a
source-free private snapshot. The standalone public debugger protocol still
has no System stack operation or authority; same-stream receipts and original
location projection remain open under the parent System item and K.11.4.
After rebuilding the v49 core/launcher/child binaries, the IPC, launcher and
engine library suites pass 140/140, 194/194 and 476/476 respectively; the
public out-of-process debugger regression suite remains 47/47. Three-crate
all-target Clippy with warnings denied, workspace formatting and diff checks
pass. A pre-rebuild engine run reached an old v48 child binary and failed
three protocol handshakes; the rebuilt final-source run passes them all.

Private page-host protocol v50 adds read-only System generated-token lookup
under a separately identified, earlier pending JavaScript loader. It shares
the exact emitted artifact/map and BlueJS AST/bytecode binding with ordinary
generated lookup, but only the explicit System route may bind a nested
execute-body point; ordinary generated lookup still returns unbound. The
frozen System fixture resolves its mapped `console` token exactly, while the
`System.register` wrapper token stays unbound. Wrong, stale, later or
same-program loaders and stale documents fail without retaining a breakpoint.
Core remints both program identities and rejects wrong tab, document, loader,
emitted program, generated coordinate, foreign or root safe point. This
checks only the private resolver subitem; public same-stream receipts,
three-frame original locations and public arm remain open under K.11.4.
After rebuilding the v50 binaries, the IPC, launcher and engine library
suites pass 141/141, 194/194 and 481/481; the real-process public debugger
suite remains 47/47. Three-crate all-target Clippy with warnings denied,
workspace formatting and diff checks pass.

Private page-host protocol v51 now returns a complete System pause stack and
its emitted child/root generated/original token projections atomically. The
loader middle frame has no BlueTS emitted artifact and is always unlocated.
The frozen System fixture proves the mapped execute-body token has an
original position while the wrapper root has none; ordinary classic stack
inspection still refuses the foreign caller. Core remints both generations,
compares every frame and scope entry to an independently read live stack,
checks both projected original coordinates against the retained map, and
rechecks the pause. Forged loader/frame/stack/original/middle-frame replies
fail closed. This is a private projection subitem only: public same-stream
receipts and authorized three-frame disclosure remain open under K.11.4.
After rebuilding the v51 core/launcher/child binaries, the IPC, launcher and
engine library suites pass 141/141, 194/194 and 483/483; the real-process
public debugger suite remains 47/47. Three-crate all-target Clippy with
warnings denied, workspace formatting and diff checks pass. The location
array is boxed on the private reply so the protocol does not inflate every
existing `PageHostReply` error path.

Public debugger protocol v47 adds a read-only
`GetGeneratedSystemStackLocations` request for an exact emitted/loader/emitted
pause. The existing independently default-denied generated paused-location
grant must be negotiated, and `ListPrograms` must have receipted the emitted
and loader generations separately on this same debugger stream before core
queries the page host. Core matches the active nested frame, checks the
complete fixed stack and both generations, revalidates the emitted original
position against the bound artifact, keeps the loader's location empty, and
rechecks the pause. IPC wire/shape and core-route regressions cover an absent
grant, an emitted-only receipt, another stream, the wrong grant, a substituted
loader generation, a stale frame, a contradictory original position and a
forged loader source location. This closes only the read-only public-route
subcheck: no public System generated resolution/arm or real-process positive
pause/step/resume/throw is claimed. The existing generic System routes remain
fail-closed. With v47 binaries rebuilt, IPC, launcher and engine library
suites pass 142/142, 194/194 and 484/484; the real-process public debugger
suite passes 47/47. Three-crate all-target Clippy with warnings denied,
workspace formatting and diff checks pass.

Public debugger protocol v48 adds explicit System generated-token resolve and
atomic nested arm requests. Both require the generated-breakpoint grant and
independent same-stream emitted and earlier page-loader receipts; the core
remints both generations, rejects same-program targets, validates only a
mapped nested execute-body safe point, and delegates to the private
loader-bound arm. The generic generated and raw System arms remain closed.
Core now recognizes the successful public System arm in admission-hold
release and pause-receipt invalidation, so a rejected wrapper never starts a
pending document while a successful owner-stream arm can reach its pause.
The frozen System fixture's real-process public debugger regression checks
wrapper unbound/denied, generic/raw refusal with no retained breakpoint,
explicit mapped resolution and arm, exact three-frame generated/original
location, one nested step, normal resume and terminal completion. The page
loader counts register and console calls, rejecting replay, and the browser
reports the BlueTS script as `Executed` after continuation. IPC wire/shape
and core-route tests cover the distinct dual-program target, emitted-only
receipt denial, unbound wrapper and exact nested arm. This checks the public
System positive-path subitem only: thrown completion and real-process
stale/cross-stream negatives remain open, so K.11.4 is not checked.
With v48 binaries rebuilt, IPC, launcher and engine library suites pass
143/143, 194/194 and 485/485; the real-process public debugger suite passes
47/47, including the focused System report assertion. Three-crate all-target
Clippy with warnings denied, workspace formatting and diff checks pass.

The next real-process System regression refuses both stale loader and stale
emitted generations at explicit resolution and arm, then verifies the realm
breakpoint list stayed empty. At the valid mapped three-frame pause, the
ordinary single-program generated-location operation returns
`InvalidExecutionState` instead of relabeling the foreign loader as emitted.
The focused launcher acceptance test passes 1/1; no production route changes.
The concurrent foreign-stream arm case is not claimed: the current core
`serve_debugger_listener` synchronously serves each connection until EOF
before accepting another. A second public socket can connect but cannot
complete `Hello` while the owner holds its stream. The remaining check needs
bounded multi-client service or an equivalent core admission-stream test.

The runtime-adapted System JavaScript and map derived from the frozen source
also pass a separate real-process thrown-completion regression with a
page-supplied loader that throws from the mapped `console.log` call. The
public debugger resolves and arms the same
execute-body token, observes the exact emitted/loader/emitted pause and mapped
original position, then resumes the nested frame. The foreign loader catches
only the expected thrown value, rejects missing or wrong exceptions, and
checks that both `System.register` and `console.log` ran exactly once. After
root continuation, the browser reports the BlueTS script as `Executed`.
This closes the thrown-completion subcheck but not the separate concurrent
cross-stream admission check or K.11.4 as a whole. The focused throwing
System test passes 1/1 and the full real-process debugger suite passes 48/48;
launcher debugger-test Clippy with warnings denied, workspace formatting and
diff checks pass.

The core admission dispatcher now has a two-session System arm regression:
both sessions independently negotiate the generated-breakpoint grant and
receive receipts for the same emitted and loader generations, then the owner holds
the next document. A foreign stream's explicit System arm returns
`InvalidTarget` before page-host routing, leaves the hold intact, and the
owner can release it. The focused test and full engine library suite pass
1/1 and 486/486; no production route or single-client listener behavior was
changed. This is the checklist's equivalent core-level cross-stream proof,
not a concurrent public-socket test. At that stage, the parent System row
remained unchecked pending an unmodified frozen source and its exact generated
artifact/map without the appended ambient declaration.

The System row now uses the unmodified frozen `system_wrapper.ts` in both
real-process public debugger cases. The owner-selected System compiler profile
supplies the page loader's `console.log` type only as a separate ambient
declaration; it changes neither the frozen source nor the Standard profile and
does not install a runtime loader. A child regression proves that a missing
page loader still rejects execution. The child-installed System program's
generation-bound map, emitted JavaScript, source hash and `sourcesContent`
match an independent BlueTSC compilation of the exact fixture. The public
normal and caught-throw cases each pause, project and continue through the
separately installed loader with no replay. Launcher library tests pass 196/196,
real-process debugger tests pass 48/48, launcher all-target Clippy with denied
warnings and workspace format check pass. The System row and its exact-source
subcheck are complete; K.11.4 remains open for the broader generated/relocated
matrix, and K.11.6 remains open. The public malformed-map fault path was
already closed by the child/core/subprocess proxy regressions recorded above.

An audit of the general K.11.4 artifact-admission subcheck confirms that the
child compiles the emitted JavaScript and map together, validates the complete
map before attaching it to a live opaque program generation, and compares its
retained source hash with the installed runtime program. Existing admission
regressions reject substituted maps, tampered hashes and stale generations,
then clear the map on realm replacement; the page-host request accepts source
graphs, not client-asserted maps. The exact frozen System case above also
compares the installed JavaScript, map and hash with independent BlueTSC output.
The 196/196 launcher library and 48/48 real-process debugger suites on this
source cover those controls. This checks the artifact-admission subitem, not
the remaining matrix-wide public-site acceptance.

The K.11.4 AST/bytecode-binding subcheck uses only a parser-certified root
statement start, a unique literal identifier in its owning statement, or the
separately AST-certified ES5 two-callee shape. Each candidate resolves to an
exact BlueJS safe point and is revalidated against the live program; reverse
projection uses the same exact AST/bytecode relation. There is no nearest-token
or nearest-instruction fallback. An added regression checks that duplicate
`value` tokens within one statement and an interior token offset stay unbound.
The focused regression and full launcher library suite pass 1/1 and 197/197.
This closes the binding mechanism subcheck, while matrix-wide public debugger
coverage and the final K.11.6 gate remain open.

Public debugger protocol v49 now has an explicit `StepGeneratedInstruction`
target for a single emitted program's root or nested pause and for a System
pause with its independently identified loader. The core first performs the
existing fully validated generated-location or System three-frame projection
under the default-denied paused-location grant and same-stream program
receipt(s), rechecks the exact active safe point/frame, then schedules only
one source-free instruction. Denied grant, another stream's receipt, missing
loader receipt, stale generation, an unpaused program or substituted frame
cannot trigger a step. The ordinary source-free step requests remain
compatible; they do not project generated or original coordinates. Real
launcher/core/child regressions now use the new route for emitted root,
relocated nested decorator and System execute-body steps; existing helper
and wrapper unbound checks remain green. IPC 144/144, engine library 488/488,
launcher library 197/197 and out-of-process debugger 48/48 pass on rebuilt
v49 binaries. Three-crate all-target Clippy with denied warnings and workspace
formatting pass. The public-operation subcheck is complete, but K.11.4 still
needs its matrix-wide real-process acceptance and K.11.6 remains open.

Private page-host protocol v52 now carries a separate owner-selected,
type-only `console` ambient for emitted ESM, leaving the default module
profile and page runtime unchanged. The three unmodified frozen `basic.ts`,
`erased.ts` and `crlf_astral.ts` sources execute with an earlier ordinary
page-script console loader. Child admission compares each installed emitted
JavaScript, complete map, source hash and exact `sourcesContent` with an
independent BlueTSC compilation; a missing runtime loader still rejects the
script. The real launcher/core/child debugger routes each frozen source
through its mapped pause, one generated instruction step, one-frame stack
and original UTF-16 projection under explicit owner policy. IPC library
tests pass 144/144, engine library 489/489, launcher library 198/198,
core and launcher CLI tests 30/30 and 29/29, and public out-of-process
debugger tests 48/48. The three-crate all-target warnings-denied Clippy,
workspace format and diff checks pass. This closes only the exact frozen
emitted-ESM three-row subcheck; K.11.4 still requires the remaining matrix
sites, and K.11.6 remains open.

The four exact frozen standard-decorator rows now execute through the same
owner-selected emitted-ESM page host without changing source bytes. Child
admission checks the installed JavaScript, complete map, source hash and
`sourcesContent` against independent BlueTSC output and verifies the
page-supplied console's actual calls. A test-first BlueJS regression exposed
that module nested pauses retained only two frames for a same-generation
three-frame synchronous caller chain. The VM now permits its existing bounded
one-intermediate-frame retention only when target and entry generations
match; distinct-generation linked calls retain their prior restriction. The
public launcher/core/child debugger resolves, arms, pauses and steps all six
relocated method/class tokens across these four rows, projects their exact
original positions, preserves their measured two- or three-frame stacks,
and leaves the generated helper definition and stepped interior instruction
without user-source attribution. BlueJS library tests pass 1,154/1,154,
launcher library 198/198, engine library 489/489 and real-process public
debugger 49/49. The remaining legacy/ES5 frozen rows, broader K.11.4
matrix-site acceptance and K.11.6 final-source gate are still open.

The three exact frozen emitted-classic rows now use an owner-only, type-only
page `console` ambient without changing their source bytes or runtime loader.
Child admission compares installed JavaScript, complete map, source hash and
`sourcesContent` with independent BlueTSC output for legacy member/class,
parameter metadata and ES5 object-spread fixtures. Real launcher/core/child
debugger tests resolve the source-anchored helper calls, reject generated
helper definitions and metadata-only sites, pause at exact original tokens,
read the retained stack, step and verify the interior instruction is unlocated,
then resume to checked execution. This completes exact-source positive-path
coverage for all eleven frozen rows, not the remaining real-process malformed
map/substituted-artifact negative gate; K.11.4 and K.11.6 stay open.

### K.11 closing gates (numbered acceptance for what remains of K.11)

Checked in this order; each must produce a recorded artifact (commit hash, command, counts) in PLAN.md.

- [ ] **AC-K11.A Malformed retained map, out-of-process.** A real launcher/core/child session whose
  retained map is corrupted (bad VLQ, non-monotonic generated column, out-of-range source/name index) must
  make generated-breakpoint resolution, arming and paused projection each return a typed refusal.
  *Pass when:* three assertions per corruption kind (no partial source/location disclosed, no breakpoint
  retained, the page still runs to completion), at least three corruption kinds, all in one named test group.
- [ ] **AC-K11.B Substituted artifact / map, out-of-process.** Same harness: map from another generation,
  map for different JavaScript hash, and externally asserted map are each refused at install or lookup.
  *Pass when:* the three cases assert `stale`/`mismatch`/`unauthorized` outcomes distinctly, with no
  retained breakpoint.
- [ ] **AC-K11.C Helper-frame non-disclosure.** For every frozen row with helper bodies (decorator, legacy,
  metadata, object-spread, System wrapper), `stack` and `location` never return a helper-body line as user
  source. *Pass when:* an assertion iterates all eleven rows and fails on any helper frame labelled with a
  user-source coordinate.
- [ ] **AC-K11.D K.11.4 box.** Tick K.11.4 only when AC-K11.A–C pass in the `out_of_process_debugger` suite
  and the test count is recorded (currently 50 before these three).
- [ ] **AC-K11.E K.11.6 final-source gate.** On one committed hash: K.0 local gate (fmt, warnings-denied
  all-target Clippy, workspace tests, all ignored oracles, coverage >= 90% line) plus hosted
  build/test/lint, `typescript-oracle`, coverage and the final CI gate. *Pass when:* PLAN.md records hash,
  commands, fixture count, coverage and the hosted run id, and no later commit touched production source.
- [ ] **AC-K11.F Close K.11.** Tick K.11 only when K.11.1–K.11.6 are checked and AC-K11.A–E are recorded.



## Archived from TODO.md: K.1–K.10 journals (2026-10-13)

Moved verbatim, in file order, under their original section heading.

### K.2 Read real project configuration — M7 — gaps G-M2, G-C2
  - *Verified (2026-10-06):* 69 pinned configs (59 accept, 10 reject), all normalized options and file sets agree; canonical confinement, owner overlays, declaration inputs, graph output layout and strict-helper relocation covered. K.0: format and Clippy pass; 1,047 tests across 58 target/doctest groups pass, including all 117 ignored oracle tests.
  - *Portability correction (2026-10-06):* sibling precedence retains native Windows parent/prefix paths. The complete corrected K.0 gate passes 1,070 tests in 61 groups, including all 122 ignored oracles; largest production source 1,172 lines.
### K.2 Read real project configuration — M7 — gaps G-M2, G-C2
  - *Verified (2026-10-06):* 32 pinned on/off configurations (16 accept, 16 reject), plus 39 boundary controls and Node/declaration parity. Diagnostic settings change cache identity without changing valid JavaScript. Legacy suites remain unchanged. K.0: format and Clippy pass; 1,060 tests across 60 target/doctest groups pass, including all 120 ignored oracles. Largest production source: 1,172 lines.
### K.2 Read real project configuration — M7 — gaps G-M2, G-C2
  - *Verified (2026-10-06):* 30 pinned native project observations (24 accept, 6 reject), including command/config overrides, default project search, source/emitted lists, pretty selection and exit codes. Node output and exact declarations agree; input/config collisions are refused before publication. K.0: format and Clippy pass; 1,067 tests in 61 target/doctest groups pass, including all 122 ignored oracles. Largest production source: 1,172 lines.
### K.2 Read real project configuration — M7 — gaps G-M2, G-C2
  - *Verified (2026-10-06):* 768 pinned project combinations; all verdicts agree, 384 emitted programs match Node output and 384 noEmit configurations publish nothing. Declaration-enabled cases match exact `.d.ts` output and source maps retain their provenance structure. K.0: format and Clippy pass; 1,069 tests across 61 target/doctest groups pass, including all 122 ignored oracles. Final K.2 CI [37428756632](https://github.com/ephoton0210/blueice/actions/runs/37428756632) passes all 29 jobs on `62f6a440d`; workspace line coverage 90.43%, independent BlueJS 93.00%.
### K.3 TypeScript diagnostic codes — M7 — gap G-C1
  - *Verified (2026-10-07):* 263 base message families plus semantic context refinements, a reproducible 2,121-template TypeScript 5.9.3 catalog, 1,398 primary code/template replays and 97 additional diagnostic source witnesses. Machine JSON preserves BTS aliases and explicit owner/subset reasons. K.0: format and Clippy pass; 1,083 tests in 64 groups pass, including all 124 ignored oracles. Largest production source: 1,185 lines. Position and presentation parity remain in K.3.2/K.3.3.
### K.3 TypeScript diagnostic codes — M7 — gap G-C1
  - *Test-first baseline (2026-10-07):* 800 primary coordinates are absent from machine JSON; derived byte spans differ in 543 entries of the 1,398 language/strictness cases. Another 867 configuration/CLI/option-combination cases add 16 tracked primary failures (14 unstructured configuration/argument failures and two missing source positions). Four pinned controls cover UTF-16 columns and LF/CRLF/CR. Every mismatch remains tracked until zero.
  - *Verified (2026-10-07):* zero primary position differences across 1,398 language/strictness and 867 configuration/CLI/option-combination cases; UTF-16 and original source module identity are enforced. Failed parser sources remain authorized and fingerprinted without reloading. K.0: format and Clippy pass; 1,091 tests in 65 groups pass, including all 125 ignored oracles. Largest production source: 1,185 lines.
### K.3 TypeScript diagnostic codes — M7 — gap G-C1
  - *Test-first baseline (2026-10-07):* 336 primary rendered-message differences and 57 missing related-information cases are tracked across the 800 rejected corpus entries. Twenty-two actual CLI observations record plain/pretty source context, related context, multiple diagnostics, CRLF, summary fields, exact exit status, emitted artifacts and Node execution. Failing replay is committed before implementation. Resource measurements are checked by schema, not compared to TypeScript's different implementation costs.
### K.3 TypeScript diagnostic codes — M7 — gap G-C1
  - *Verified (2026-10-07):* 22 actual CLI observations pass, 800 primary related-information records agree, and all messages outside the three measured G-T1 witnesses match. Format and both-crate all-target Clippy pass; 1,097 unique tests in 67 groups pass, including all 126 ignored oracles. The initial Linux recorder path-normalization failure is preserved in the full gate log and corrected by a passing two-test replay on unchanged backend inputs. Largest production source: 1,185 lines.
### K.3 TypeScript diagnostic codes — M7 — gap G-C1
  - *Verified (2026-10-07):* final-source CI [37590055063](https://github.com/ephoton0210/blueice/actions/runs/37590055063) on `ed8b6d088` passes all 29 jobs, including complete workspace build/test/fmt/all-target Clippy across 25 platform configurations and both pinned TypeScript oracle jobs. Workspace line coverage is 90.60% (181,085 lines, 17,026 missed); independent BlueJS line coverage is 93.00% (77,553 lines, 5,427 missed). No coverage exclusion is added. K.2/K.3 and M7 are complete; the three measured G-T1 iterator-flow wording gaps remain for K.4.1.
### K.4 Type system — M8 — gaps G-T1 to G-T6
  - *Design decision (2026-10-07):* PLAN.md records a bounded flow graph per execution scope, reusing lexical binding identities and authorized syntax. Graph construction, predicates and evaluation remain separate modules; assignments, joins, loops, abrupt completion and closure boundaries require pinned positive and negative witnesses before implementation.
  - *Failing baseline (2026-10-07):* 97 pinned cases (67 accept, 30 reject), eleven accepted runtime/declaration witnesses. Linux replay reports 82 checker mismatches and ten runtime failures; fixture coverage and the pinned recorder pass. No production implementation is included in the baseline.
  - *Supplemental baseline (2026-10-07):* twenty additional path/execution witnesses bring the corpus to 117 cases (81 accept, 36 reject) and thirteen runtime witnesses. Before the extension, Linux replay records eighteen checker mismatches and two runtime failures; recorder and completeness pass.
  - *Optional receiver boundary (2026-10-07):* six additional pinned controls bring the corpus to 123 cases (83 accept, 40 reject) and fourteen runtime witnesses. All six disagree with the pre-fix Linux CLI. Known-null receivers remain rejected with TS2339; nullable aliases and missing-member metadata require correction.
  - *Verified (2026-10-07):* all 123 pinned verdicts/primary diagnostics and fourteen runtime/declaration witnesses pass; 840 CLI primary messages/related records agree with TypeScript. Format and both-crate all-target Clippy pass. The complete 68-group run passes 1,099 tests and all 128 ignored oracles; two legacy bridge examples use TypeScript-invalid known-null sources. Correcting only those test sources preserves AST/debugger assertions and adds a direct TS2339 refusal control; the full 973-test ordinary suite then passes on identical production bytes. Combined: all 1,101 unique tests pass. Largest production source: 1,185 lines. The three historical M7 iterator wording gaps are resolved without allowances; broader exception flow and dotted capture precision stay recorded in G-T1.
### K.4 Type system — M8 — gaps G-T1 to G-T6
  - *Failing baseline (2026-10-07):* 61 pinned programs (30 accept, 31 reject), five runtime/declaration witnesses, including imported signatures and callable aliases. Linux replay records 55 checker mismatches and four runtime/declaration failures; fixture completeness and pinned recorder pass. The shared diagnostic corpus is regenerated to 1,582 programs and 145 templates. Production remains unchanged in this baseline.
  - *Call-position replay (2026-10-07):* the 61-program draft passes the full frozen K.0 gate (1,105 tests, all 130 ignored oracles). Fourteen further pinned controls expand the guard corpus to 75 programs (36 accept, 39 reject); eight invalid call effects and two `undefined`/`void` argument rejections fail before their correction. All five runtime/declaration witnesses and the pinned recorder pass. The shared corpus now contains 1,596 programs and 145 templates; the leaf remains open.
  - *Verified (2026-10-07):* all 75 pinned verdicts/primary and related diagnostics, five runtime/declaration witnesses, and the 123-case narrowing regression pass. Statement/comma call effects follow the pinned parenthesis/initializer/argument controls. Format, format check and both-crate all-target Clippy pass; all 1,105 tests in 69 groups pass, including all 130 ignored oracles. Largest production source: 1,185 lines. G-T1 retains the broader unmeasured predicate/exception/capture forms.
### K.4 Type system — M8 — gaps G-T1 to G-T6
  - *Failing baseline (2026-10-07):* 59 pinned programs (33 accept, 26 reject), three runtime/declaration witnesses. Linux replay records 34 checker/primary-diagnostic mismatches and two runtime/declaration mismatches; completeness and pinned recorder pass. The shared diagnostic corpus contains 1,655 programs and 147 templates. Production remains unchanged in this baseline.
  - *Generic-boundary replay (2026-10-07):* the 59-program draft passes all four focused tests. Eleven further controls expand the corpus to 70 programs (39 accept, 31 reject) and four runtime/declaration witnesses. Linux replay records six checker/diagnostic differences and a missing contextual-inference budget refusal; all four runtime/declaration witnesses, completeness and pinned recorder pass. The shared corpus contains 1,666 programs and 147 templates. The leaf remains open.
  - *Annotated-call replay (2026-10-08):* the 70-program draft passes the full frozen K.0 gate (1,110 tests in 70 groups, all 132 ignored oracles). Four further controls bring the corpus to 74 programs (41 accept, 33 reject); the Linux replay records two TS2344/TS2558 primary-position differences when an earlier variable annotation also has type arguments. All four runtime/declaration witnesses, budget refusal, completeness and recorder pass. The leaf remains open.
  - *Verified (2026-10-08):* all 74 pinned verdicts/primary and related diagnostics, four runtime/declaration witnesses and contextual-inference budget refusal pass through the measured API/CLI paths. Format, format check and both-crate all-target Clippy pass; all 1,110 tests in 70 groups, including all 132 ignored oracles in 35 differential suite files. Largest production source: 1,143 lines. Generic binders, dependent defaults and imported class/function signatures are retained; full operators, generic heritage/methods and broader variance remain in their dependent leaves and G-T2.
### K.4 Type system — M8 — gaps G-T1 to G-T6
  - *Failing baseline (2026-10-08):* 57 pinned programs (30 accept, 27 reject), five runtime/declaration witnesses. Linux replay reports 24 checker/primary-diagnostic differences and one anonymous-signature runtime/declaration compile failure; four other runtime/declaration witnesses, completeness and pinned recorder pass. The shared corpus contains 1,727 programs and 147 templates. Production remains unchanged in this baseline.
  - *Interface/order replay (2026-10-08):* the original 57 overload programs and all 74 K.4.3 generic programs pass. Seventeen further controls extend the overload corpus to 74 programs (39 accept, 35 reject) and six runtime/declaration witnesses. Linux reports 12 checker/primary differences and one additional declaration compile failure; completeness, pinned recorder and the five previous runtime/declaration witnesses pass. The leaf remains open.
  - *Verified (2026-10-08):* all 74 pinned verdicts/primary and related diagnostics and six runtime/declaration witnesses pass. Ordered call/construct/method candidates, direct literal specialization, merged interface groups, callback context and hidden implementation compatibility are retained. Format, format check and both-crate all-target Clippy pass; all 1,114 tests in 71 groups, including all 134 ignored oracles in 36 differential suite files. Largest production source: 1,153 lines. G-T4 retains unmeasured interactions; K.4.5 to K.4.7 and final workspace/coverage verification remain open.
### K.4 Type system — M8 — gaps G-T1 to G-T6
  - *Failing baseline (2026-10-08):* 78 pinned programs (37 accept, 41 reject), three runtime/declaration witnesses. Linux reports 68 checker/primary-diagnostic differences and two runtime/declaration compile failures; the indexed-access runtime witness, completeness and pinned recorder pass. The shared corpus contains 1,822 programs and 152 templates. Production remains unchanged in this baseline.
  - *Literal-order replay (2026-10-08):* the 78-program draft and all runtime/declaration witnesses pass. Four further controls extend the corpus to 82 programs (39 accept, 43 reject); Linux records two template-union diagnostic-order differences. Both pinned recorders, 652 default-library numeric-cache observations, completeness and six operator-budget controls pass. The shared corpus contains 1,826 programs and 152 templates. The implementation draft remains uncommitted and the leaf stays open.
  - *Verified (2026-10-08):* all 82 pinned verdicts/primary and related diagnostics, three runtime/declaration witnesses, both recorders and six operator-budget controls pass. Explicit operator forms, conditional distribution/infer scopes, mapped modifiers/remaps, bounded recursive expansion and pinned numeric template ordering are retained. Format and both-crate all-target Clippy pass; all 1,120 tests in 72 groups, including all 137 ignored oracles in 37 differential suite files. Largest production source: 1,153 lines. G-T3 retains unmeasured combinations; K.4.6/K.4.7 and final workspace/coverage verification remain open.
### K.4 Type system — M8 — gaps G-T1 to G-T6
  - *Failing baseline (2026-10-08):* 84 pinned programs (38 accept, 46 reject), three runtime/declaration witnesses. Linux reports 51 checker/primary-diagnostic differences and two runtime compile failures; completeness and the pinned recorder pass. The shared diagnostic corpus records 1,910 programs and 163 templates. Production remains unchanged in this baseline.
  - *Supplemental baseline (2026-10-08):* The isolated 16-control supplement expands the corpus to 100 programs (46 accept, 54 reject), with six runtime/declaration witnesses. The 84-program implementation passes all four focused oracle tests and Clippy; its unit replay exposes two legacy opaque-receiver regressions that remain open. Before the supplement fixes, Linux records 13 checker/primary differences, including four compiler panics on interface index signatures, and two runtime/declaration failures. Completeness and the pinned recorder pass. Evidence: `/private/tmp/blueice-k14-linux/blueice-k46-supplement-red-gate.log`. The shared corpus records 1,926 programs (882 accept, 1044 reject) and 164 templates. Production implementation changes remain unstaged; K.4.6 stays open.
  - *Verified (2026-10-08):* All 100 pinned programs (46 accept, 54 reject), exact primary/related diagnostics, six Node/runtime and every-declaration witnesses and the pinned recorder pass. The frozen-source Linux K.0 gate passes format, format check and both-crate all-target Clippy with warnings denied; all 1,124 tests in 73 groups pass, including all 139 ignored oracles in 38 differential suite files. All 47 frozen changed backend files match the host snapshot. Largest production source: 1,153 lines. G-T5 retains unmeasured combinations; K.4.7 and final workspace/coverage verification remain open.
### K.4 Type system — M8 — gaps G-T1 to G-T6
  - *Failing baseline (2026-10-08):* The pinned corpus contains 78 programs (40 accept, 38 reject), with four runtime/declaration witnesses. Linux records 41 checker/primary-diagnostic differences and two runtime/declaration failures: freshness declaration formatting and variance syntax parsing. Completeness and the pinned recorder pass (two tests pass, two fail). The shared diagnostic corpus records 2,004 programs (922 accept, 1082 reject) and 167 templates. Production remains unchanged from the verified K.4.6 commit `9b292ac12`. Evidence: `/private/tmp/blueice-k14-linux/blueice-k47-red-gate.log`. K.4.7 remains open.
### K.4 Type system — M8 — gaps G-T1 to G-T6
  - *Position baseline (2026-10-08):* Ten further position controls expand the corpus to 88 programs (44 accept, 44 reject), retaining four runtime/declaration witnesses. The original 78-program draft passes all four focused oracle tests and all-target Clippy. Linux replay of the supplement records six missed excess-property rejections in variable/member/bracket/array-element assignments and method/function-property arguments; completeness, pinned recorder and all four runtime/declaration witnesses pass (three tests pass, one fails). A legacy readonly declaration unit assertion still expects the previous one-line formatting and remains open. Evidence: `/private/tmp/blueice-k14-linux/blueice-k47-supplement-red-gate.log`. Implementation changes remain unstaged; K.4.7 stays open.
### K.4 Type system — M8 — gaps G-T1 to G-T6
  - *Verified (2026-10-08):* All 88 pinned programs (44 accept, 44 reject), exact primary/related diagnostics, four Node/runtime and every-declaration witnesses and the pinned recorder pass. The frozen-source Linux K.0 gate passes format, format check and both-crate all-target Clippy with warnings denied; all 1,128 tests in 74 groups pass, including all 141 ignored oracles in 39 differential suite files. All 41 frozen changed backend files match the host snapshot. Largest production source: 1,153 lines. G-T6 retains unmeasured combinations; final K.3/K.4 workspace/coverage verification remains open. M8 remains open for K.5–K.9.
### K.4 Type system — M8 — gaps G-T1 to G-T6
  - *Fixture provenance (2026-10-08):* Ten supplemental fixtures now carry the required MPL-2.0 headers. Their program bodies are unchanged; the pinned compatibility and shared diagnostic recorders regenerate only source positions. The frozen 12-file Linux replay passes format, format check, both-crate all-target Clippy and all eight compatibility/diagnostics tests, including both pinned recorders and all four runtime/declaration witnesses. Production sources and Rust test harness inputs match `4abe6111f`. Evidence: `/private/tmp/blueice-k14-linux/blueice-k47-headers-status.json` and its compatibility/diagnostics logs. Final workspace CI remains open while an unrelated openSUSE repository initialization failure is repaired.
### K.4 Type system — M8 — gaps G-T1 to G-T6
  - *CI bootstrap correction (2026-10-08):* Initial final-source CI [37698111419](https://github.com/ephoton0210/blueice/actions/runs/37698111419) exposes an openSUSE Leap 15.6 x86_64 initialization failure before checkout or Rust build/test: the unused OpenH264 repository cannot connect, so zypper reports exit 106 after installing the required packages. The SUSE bootstrap now disables only `repo-openh264` when its configuration file exists; required repositories and package-install failures retain their existing behavior. The exact corrected CI step succeeds in a fresh Linux openSUSE Leap 15.6 aarch64 container, with all twelve required RPMs verified. Evidence: `/private/tmp/blueice-k47-ci-suse-failure.log` and `/private/tmp/blueice-k47-suse-install.log`. The vendor documents exit 106 as a skipped repository at [zypper(8)](https://manpages.opensuse.org/Leap-15.6/zypper/zypper.8.en.html). Production sources and Rust test harnesses are unchanged. The initial coverage job passes with workspace line coverage 90.65% and independent BlueJS 93.00%; no coverage exclusion is added. A fresh final-source CI run is required after this bootstrap correction.
### K.4 Type system — M8 — gaps G-T1 to G-T6
  - *Final K.3/K.4 verification (2026-10-08):* Final-source CI [37702047861](https://github.com/ephoton0210/blueice/actions/runs/37702047861) on `40de14abd` passes all 29 jobs, including complete workspace build/test/fmt/all-target Clippy across 25 platform configurations and both pinned TypeScript oracle jobs. Workspace line coverage is 90.65% (188,432 lines, 17,621 missed); independent BlueJS line coverage is 93.00% (77,553 lines, 5,425 missed). No coverage exclusion is added. K.3 and the measured K.4 leaves are complete. M8 remains open for K.5–K.9 and the inventory retains unmeasured combinations.
### K.5 Remaining class forms — M8 — gap G-T9
  - *Failing baseline (2026-10-08):* 95 isolated pinned programs (45 accept, 50 reject), five Node/runtime witnesses and six exact declaration witnesses cover abstract member obligations and constructor types, structural implementations, override rules and imported boundaries. The frozen-source Linux replay records 94 verdict/primary-diagnostic differences and six emission failures; completeness and the pinned recorder pass (two tests pass, two fail). The shared diagnostic corpus contains 2,109 programs (971 accept, 1,138 reject) and 184 templates. Production sources are unchanged from the verified post-Test262 merge source. Evidence: `/private/tmp/blueice-k14-linux/blueice-k51-red-gate.log`. Baseline `05925c6cc` was committed before implementation; final verification follows.
  - *Verified (2026-10-08):* All 95 pinned programs and twelve further constructor capability controls match, including exact primary/related diagnostics; all five Node/runtime and six exact declaration witnesses pass. Abstractness, accessibility and inherited/imported parameters survive constructor aliases; generic constructor assignment uses bounded contextual inference. The frozen-source Linux K.0 gate passes format and both-crate all-target Clippy with warnings denied; all 1,142 tests in 77 groups pass, including all 144 ignored oracles in 41 suite files. Both direct field modes pass; all 46 frozen backend files match the host. Refusal inventory regenerated: 162 sites. Largest changed production source: 1,188 lines. Evidence: `/private/tmp/blueice-k14-linux/blueice-k51-full-status.json` and logs. K.5.2/K.5.3 and M8 remain open.
  - *CI portability correction (2026-10-08):* Full CI [37785330991](https://github.com/ephoton0210/blueice/actions/runs/37785330991) on `30db65f66` exposed the macOS `/var` versus `/private/var` temporary-root alias in the modifier runtime/declaration harness. Canonicalizing the harness root keeps configured output paths within the existing compiler boundary. A Linux replay uses a symlinked `TMPDIR`; cross-platform CI is rerun on the correction.
  - *Final CI verification (2026-10-09):* Corrected-source CI [37797735267](https://github.com/ephoton0210/blueice/actions/runs/37797735267) on `bb580bdf9` passes all 29 jobs: complete workspace build/test/fmt/all-target Clippy across 25 platform configurations, both pinned TypeScript oracle jobs, coverage and the final gate. Workspace line coverage is 95.04% (194,767 lines, 9,664 missed); independent BlueJS line coverage is 99.53% (81,248 lines, 384 missed). No coverage exclusion is added. The macOS oracle temporary-root mismatch is resolved. No BlueTS regression from the merged Test262 branch is observed in these gates. K.5.2/K.5.3 and M8 remain open.
### K.5 Remaining class forms — M8 — gap G-T9
  - *Failing baseline (2026-10-08):* 83 isolated pinned programs (50 accept, 33 reject), five Node/runtime and six exact declaration witnesses cover every K.5.2 syntax category and imported generic inheritance. The frozen Linux replay records 83 verdict/primary-diagnostic differences and six emission failures; completeness and the pinned recorder pass (two tests pass, two fail). Production remains the verified K.5.1 commit `30db65f66`. Shared diagnostic corpus: 2,192 programs (1,021 accept, 1,171 reject), 190 templates. Evidence: `/private/tmp/blueice-k14-linux/blueice-k52-red-status.json` and `blueice-k52-red-gate.log`. Commit the baseline before implementation; the leaf stays open.
  - *Supplemental baseline (2026-10-09):* Thirteen controls expand the corpus to 96 programs (59 accept, 37 reject), nine runtime and fifteen exact declaration witnesses. The 83-program implementation draft passes its focused oracle and legacy replay. The supplemental Linux replay records four missed TS1166 rejections and failures in eight runtime/declaration fixtures, including a recursive declaration compiler panic; the direct computed-field witness fails in assign mode. Completeness and the pinned recorder pass. The shared diagnostic corpus records 2,205 programs (1,030 accept, 1,175 reject) and 191 templates. Evidence: `/private/tmp/blueice-k14-linux/blueice-k52-supplement-red-matrix.log` and `blueice-k52-supplement-red-direct.log`. This baseline commits only fixtures, harness/recorder metadata and plans; production remains an uncommitted draft and K.5.2 stays open.
  - *Scope baseline (2026-10-09):* Fourteen further controls expand the corpus to 110 programs (69 accept, 41 reject), fourteen runtime and twenty exact declaration witnesses. The 96-program draft passes all four focused oracle tests, both-crate all-target Clippy, the legacy 2,205-program diagnostic replay and both direct field modes. The new Linux replay records six checker/primary differences and one runtime/declaration fixture failure, covering class-expression argument bodies, captured outer generic parameters and inheritance through a class-expression constructor alias. Completeness and the pinned recorder pass; all three direct tests pass. Shared diagnostic corpus: 2,219 programs (1,040 accept, 1,179 reject), 191 templates. Evidence: `/private/tmp/blueice-k14-linux/blueice-k52-96-pass-status.json` and `blueice-k52-scope-red-matrix.log`. This baseline commits only test evidence and plans; production remains an uncommitted draft and K.5.2 stays open.
  - *Capture baseline (2026-10-09):* Twelve controls expand the corpus to 122 programs (77 accept, 45 reject), eighteen runtime and twenty-four exact declaration witnesses. The 110-program draft passes all four focused oracle tests, both-crate all-target Clippy, the legacy diagnostic replay and both direct field modes. The new frozen Linux replay records nine checker/primary differences and failures in three runtime/declaration fixtures for captured named self types, constructor alias chains and inheritance from a generic factory result. Own class parameters and defaults already pass. Completeness and the pinned recorder pass; all three direct tests pass. Shared diagnostic corpus: 2,231 programs (1,048 accept, 1,183 reject), 191 templates. Evidence: `/private/tmp/blueice-k14-linux/blueice-k52-110-pass-status.json` and `blueice-k52-capture-red-matrix.log`. Only test evidence and plans are committed; implementation remains a local draft and K.5.2 stays open.
  - *Import baseline (2026-10-09):* Four cross-module controls expand the corpus to 126 programs (79 accept, 47 reject), retaining eighteen runtime and twenty-four declaration witnesses. On the recorder's frozen 110-program implementation draft, twelve verdict/primary differences remain: nine captured-self/alias differences plus three import differences. An exported class-expression instance loses its private type definition and incorrectly accepts a string assignment from a numeric field; inherited imported constructors also lose their class surface. Three runtime/declaration fixtures still fail. Completeness, pinned recorder and all direct tests pass. Shared diagnostic corpus: 2,235 programs (1,050 accept, 1,185 reject), 191 templates. Evidence: `/private/tmp/blueice-k14-linux/blueice-k52-import-red-status.json`, `blueice-k52-import-red-frozen-source-hashes.json` and `blueice-k52-import-red-matrix.log`. Only fixtures and plans are committed; newer local captured-self/alias edits are excluded from this failing replay. K.5.2 remains open.
  - *Shadow baseline (2026-10-09):* Nine controls expand the corpus to 135 programs (84 accept, 51 reject), twenty runtime and twenty-six exact declaration witnesses. The 126-program draft passes both-crate all-target Clippy, all four focused oracle tests, the legacy 2,235-program diagnostic replay and both direct field modes; source hashes match all 74 changed backend files. The new frozen Linux replay records seven checker/primary differences and failures in two runtime/declaration fixtures: distinct same-named class/method binders are conflated, a returned class's `this` is incorrectly checked in its enclosing function, computed-key `this` needs TS2465, and nested declaration binders require TypeScript's `T_1` spelling. TS2719 retains its TS2208 related origin. Completeness, pinned recorder and all direct tests pass. Shared diagnostic corpus: 2,244 programs (1,055 accept, 1,189 reject), 193 templates. Evidence: `/private/tmp/blueice-k14-linux/blueice-k52-126-pass-status.json` and `blueice-k52-shadow-red-matrix.log`. Only failing test evidence and plans are committed; production remains an uncommitted draft and K.5.2 stays open.
  - *Assignment-origin baseline (2026-10-09):* Eight adjacent controls expand the corpus to 143 programs (90 accept, 53 reject), retaining twenty runtime and twenty-six exact declaration witnesses. The 135-program draft passes all four dynamic oracle tests, including pinned recording, runtime and declaration comparisons. Its full replay also finds a static-block output whitespace regression; that correction remains an uncommitted draft. An isolated pinned recorder and public-CLI replay of the eight controls record one primary difference (TS2322 instead of TS2719 for a same-named method-local assignment) and two missing TS2208 related origins at assignment/argument boundaries. Six controls for ordinary and union class receivers, inferred/shadowed inherited returns, captured defaults and named-self constructor queries already pass. The shared corpus contains 2,252 programs (1,061 accept, 1,191 reject), 193 templates. Evidence: `/private/tmp/blueice-k14-linux/blueice-k52-shadow-controls-red.json`, the isolated recorder logs and the frozen 79-file source snapshot. The two receiver controls use `dynamic-shadow-this-*` names so legacy class discovery does not also claim them. Only test evidence and plans are committed; K.5.2 stays open until the corrected final source passes the full two-crate gate.
  - *Static/local binder baseline (2026-10-09):* Eight controls expand the pinned corpus to 151 programs (95 accept, 56 reject), twenty-two runtime and twenty-eight exact declaration witnesses. The preceding frozen 83-file implementation completes the full two-crate gate with 1,149 passing tests and one recorder-order failure; format, all-target Clippy and every other ignored/live oracle pass. Recorder metadata now uses directory-name sorting, with all existing observations unchanged. An isolated pinned recorder and unchanged public Linux CLI record eight new verdict/primary differences: static generic arrow/function binders are confused with class parameters, callable annotation binders are missing from lexical lookup, and a method binder rewrites a captured outer function parameter through a local named self type, including one unsafe false acceptance. The rejection retains TS2322 and its TS2208 origin. The shared corpus contains 2,260 programs (1,066 accept, 1,194 reject), 193 templates. Evidence: `/private/tmp/blueice-k14-linux/blueice-k52-static-local-red.json`, its frozen 83-file hashes, the isolated pinned recorder logs and `blueice-k52-143-code-complete-full-status.json`. Only fixtures, recorder metadata and plans are committed before the binder correction; K.5.2 stays open.
  - *Expression-target baseline (2026-10-09):* Nine controls expand the corpus to 160 programs (103 accept, 57 reject), thirty runtime and thirty-six exact declaration witnesses. The preceding frozen 88-file implementation passes format, both-crate all-target Clippy and all 1,150 tests in 79 groups, including all 146 ignored oracles in 42 suite files. An independent ES2020 replay then records eight runtime failures, five declaration differences and one checker rejection for static fields/blocks, private fields/methods, derived and nested class expressions, and per-evaluation computed keys. Internal class identities leak into after-class JavaScript and private stores escape expression scope. Literal assertions in a computed-key getter lose their target; the invalid assertion control reports TS2322 instead of TS2352. Empty instance/export declarations also need exact TypeScript formatting. Shared diagnostic corpus: 2,269 programs (1,074 accept, 1,195 reject), 194 templates. Evidence: `/private/tmp/blueice-k14-linux/blueice-k52-151-pass-full-status.json`, its frozen 88-file source hashes, `blueice-k52-expression-target-red.json` and isolated pinned recorder logs. The baseline commits fixtures, target-aware harness, direct witnesses and plans before lowering changes. K.5.2 remains open until these supported-target boundaries pass.
  - *Verified (2026-10-09):* All 160 pinned programs (103 accept, 57 reject), exact primary/related diagnostics, 30 Node runtime and 36 exact declaration witnesses, the pinned recorder and four direct class tests pass. Generic identities, captured self/constructor aliases, computed keys, index contracts and `declare` erasure compose with both field modes. ES2020 expression wrappers retain per-evaluation private/static stores and contextual names; original assertion spans reach the direct AST route. The frozen-source Linux K.0 gate passes format and both-crate all-target Clippy with warnings denied; all 1,151 tests in 79 groups pass, including all 146 ignored oracles in 42 suite files. All 90 changed backend hashes match the tested source. Shared diagnostic corpus: 2,269 programs, 194 templates. Generated inventory: 166 refusal sites in ten areas; largest changed production source: 1,194 lines. Evidence: `/private/tmp/blueice-k14-linux/blueice-k52-final-full-status.json` and its frozen source hashes. G-T9 retains K.5.3 and unmeasured combinations; K.5.3/M8 and final hosted workspace/coverage verification remain open.
  - *Hosted CI correction (2026-10-09):* Initial workspace CI [37845418066](https://github.com/ephoton0210/blueice/actions/runs/37845418066) on `8132479c4` passes both pinned BlueTS oracle jobs but exposes an IPC fixture race on macOS 26 arm64: concurrent allocations use the same timestamp and `create_dir` fails with `AlreadyExists`. The test helper adds an atomic sequence; all 136 IPC tests, its sixteen-thread isolation/cleanup regression, format and all-target Clippy pass on Linux. Production IPC behavior and coverage exclusions are unchanged. Evidence: `/private/tmp/blueice-k14-linux/blueice-k52-ipc-fixture-status.json` and the retained macOS failure log. Fresh final-source hosted CI follows the K.5.3 push.
### K.5 Remaining class forms — M8 — gap G-T9
  - *Failing baseline (2026-10-09):* 35 isolated pinned programs (18 accept, 17 reject), 16 Node and 17 exact declaration witnesses cover independent accessor read/write types, inferred getters, imported field/accessor/method redeclarations and constructor branch initialization. On verified K.5.2 source `8132479c4`, the public CLI/Rust replay records 27 verdict/primary differences, 14 emission failures and two declaration-format differences; completeness and the pinned recorder pass. All three direct regression tests fail, including an unsafe acceptance when a constructor returns before initializing its field. The shared corpus records 2,304 programs (1,092 accept, 1,212 reject), 195 templates; all prior observations are unchanged. Evidence: `/private/tmp/blueice-k14-linux/blueice-k53-baseline/` (`rust-baseline-status.json`, `runtime-red-status.json`, `direct-red-status.json` and frozen source hashes). No production implementation is included in this baseline. The derived parameter-property branch refusal remains separate: pinned TypeScript rejects assignment mode with TS2401, but accepts ES2022 define mode and emits a `this.extra` assignment before `super`, which throws ReferenceError under Node. Existing deferred coverage preserves this measured limitation. K.5.3 remains open.
### K.5 Remaining class forms — M8 — gap G-T9
  - *Verified (2026-10-09):* All 35 pinned programs, exact primary/related diagnostics, 16 Node runtime and 17 exact declaration witnesses, the live recorder and all three direct regressions pass. Accessor read/write types survive generic and imported surfaces; imported member kinds retain override diagnostics; bounded merged interface heritage contributes erased members; constructor branches and early returns enforce definite initialization. Three obsolete deferred class fixtures return to the ordinary matrix; the measured branch-super parameter-property limitation remains. The frozen-source Linux K.0 gate passes format and both-crate all-target Clippy with warnings denied; all 1,158 tests in 81 groups pass, including all 148 ignored oracles in 43 suite files. All 99 backend hashes match. The strict replay metadata correction changes no verdict or diagnostic in the 2,304-program, 195-template shared corpus and preserves every prior 2,269 observation. Generated inventory: 162 refusal sites in ten areas; largest changed production source: 1,196 lines. Evidence: `/private/tmp/blueice-k14-linux/blueice-k53-final-full-status.json`, `blueice-k53-final-full-report.json`, frozen hashes and `blueice-k53-metadata/comparison.json`. K.5 is locally verified; K.6–K.10, M8 and final hosted workspace/coverage verification remain open.
### K.6 Module syntax — M8 — gap G-T10
  - *Failing baseline (2026-10-09):* 74 pinned programs (56 accept, 18 reject), 41 Node and 47 declaration witnesses cover default expressions and anonymous/named declarations, alias/snapshot behavior, named/star/namespace re-exports, transitive surfaces, diamonds, conflicts, cycles, evaluation and imported identities under ESM/CommonJS. On verified K.5.3 source `13079a4f3`, the public CLI replay records 74 verdict/primary differences and 47 emission failures; completeness and the live recorder pass. All 41 pinned Node programs run successfully before BlueTS failures are collected. Three direct export regressions fail; the declaration-only runtime-authority control passes. Format and both-crate all-target Clippy pass; all 217 frozen backend hashes match. Shared corpus: 2,378 programs, 198 templates, 1,148 accept, 1,230 reject; every prior 2,304 observation is unchanged. Two embedded fixture roots are normalized exactly as in the existing shared replay, with all codes, positions, related information and verdicts retained. Evidence: `/private/tmp/blueice-k14-linux/blueice-k61-baseline/` (`report.json`, `rust-baseline-status.json`, `comparison.json`, `normalization-comparison.json` and source hashes). The baseline contains no production change and stays unpushed until the implementation passes K.0. Twelve isolated probes retain a pinned limitation: ES2020 anonymous default classes with private fields are accepted but fail under Node in both module and field modes; named private and anonymous public-field controls run normally. The runtime matrix uses valid public-field witnesses without weakening comparisons. K.6.1 remains open; K.5.3 final-source CI [37859872555](https://github.com/ephoton0210/blueice/actions/runs/37859872555) is still running on its exact pushed SHA.
  - *Verified (2026-10-09):* All 74 pinned programs (56 accept, 18 reject), exact primary/related diagnostics, 41 Node and 47 exact declaration witnesses, the live recorder and five direct tests pass. Incremental re-exports refresh reused and changed dependency types; anonymous default function/class names and identifier snapshot versus live alias semantics are verified. The existing direct arrow refusal and the measured ES2020 anonymous-private default-class refusal remain, with positive controls. Five further pinned Node controls confirm emitted default names and snapshots. The frozen-source Linux K.0 gate passes format, both-crate all-target Clippy with warnings denied, all 1,019 ordinary tests, and the complete 1,169-test run in 83 groups, including all 150 ignored oracles in 44 suite files. All 255 backend hashes match. Shared corpus: 2,378 programs, 198 templates; generated inventory: 160 refusal sites in ten areas. No diagnostic allowance or runtime grant is added. Evidence: `/private/tmp/blueice-k14-linux/blueice-k61-full-fifth-{status,report}.json`, frozen hashes/logs, committed private/cache/name failing regressions and `blueice-k61-default-names-reference/report.json`. Parser/emitter reach 1,207/1,208 lines; the queued split must be separately tested, committed and pushed before K.6.2. Hosted full-workspace/platform/coverage verification remains open.
  - *Generator failing regression (2026-10-09):* Four supported synchronous and four retained async-refusal controls are accepted, executed and declaration-emitted by pinned TypeScript 5.9.3/Node before collecting BlueTSC differences on `d8b9e6a68`. Only CommonJS anonymous synchronous default generation fails: emitted `function default_1*` is invalid JavaScript; ESM and named controls match exact output/declarations. Async generators retain the mandatory frontend refusal and publish no output. Format and both-crate all-target Clippy pass, all five frozen backend hashes match. Evidence: `/private/tmp/blueice-k14-linux/blueice-k61-generator-red-{status,report}.json`, logs/hashes and the independent `blueice-k61-generator-probe.json`. This test-only baseline is committed before the insertion fix and remains unpushed until K.0 passes. K.6.1 reopens for this correction; K.6.2 remains open.
  - *Generator correction verified (2026-10-09):* The CommonJS synthetic name is inserted after the generator star. All four supported synchronous controls match pinned Node names/output and exact declarations; all four async controls retain the mandatory refusal and no output publication. `module-exports-v2.1` enters artifact/cache identity. The integrated frozen nine-file Linux gate passes format, all-target Clippy with warnings denied, 1,019 ordinary and all 1,170 BlueTS/bridge tests in 84 groups, including every 151 ignored oracle in 45 suite files, plus 139 IPC and 20 real core subprocess tests. All hashes match; inventory remains 160 sites in ten areas and all BlueTS production sources stay below 1,200 lines. Evidence: `/private/tmp/blueice-k14-linux/blueice-k61-generator-ci-{status,report}.json` and logs/hashes. The IPC frame-prefix regression is separately committed before its repair. K.6.1 closes again; K.6.2/K.6.3 and full hosted CI remain open.
  - *Hosted CI fixture correction (2026-10-09):* Full CI `37879746425` on `5e5bcb241` exposes the SLE x86_64 HTTP origin-limit fixture assumption that successive freed ephemeral ports are distinct. A Linux probe reproduces duplicate ports. Reserve all 17 listeners together, assert distinct ports, then close them before the real requests; retain all fetch/origin-limit assertions. Format, engine all-target Clippy, all 507 engine tests and 20 focused repetitions pass on the frozen source, with launcher binaries built first. Evidence: `/private/tmp/blueice-k14-linux/blueice-k6-ci-origin-final-{status,report}.json`, probe/repeat records and logs/hashes. Final-source full hosted CI remains open.
### K.6 Module syntax — M8 — gap G-T10
  - *Failing baseline (2026-10-09):* On verified source `d8b9e6a68`, 86 pinned programs (63 accept, 23 reject), 54 successful Node references and 60 declaration witnesses retain all named/default/namespace/import-equals and inline type forms, arbitrary module names, type queries, type-only stars/namespaces, attributes, valid JSON imports and UMD namespace restrictions. The public CLI records 68 verdict/primary differences and 63 emission differences across 59 witnesses. Two direct graph regressions fail; ordinary type-only declaration elision and runtime-authority refusal controls pass. Format, both-crate all-target Clippy, live recorder and shared completeness pass; all 257 frozen backend hashes match. The shared corpus now has 2,464 programs and 211 templates (1,211 accept, 1,253 reject); every prior 2,378 complete observation is unchanged. Evidence: `/private/tmp/blueice-k14-linux/blueice-k62-baseline-final-{status,report}.json`, logs/hashes and `blueice-k62-shared-comparison.json`. Explicit inline imports of `.d.ts` files retain pinned TS2846; valid extensionless elision controls remain. ESNext accepts the measured legacy assertion and duplicate-attribute controls; observations are not replaced by assumed restrictions. Positive JSON witnesses require owner-bounded resolution/asset emission before this leaf can close. No production change; this baseline stays unpushed until its implementation passes K.0. The separately measured K.6.1 generator output defect takes priority. K.6.2 remains open.
  - *Resolution-mode failing regression (2026-10-09):* Two additional pinned programs select import/require package conditions for the same source/specifier, directly and through type-only re-exports. Both TypeScript/Node controls run to `3` and emit declarations before collecting BlueTSC failures on production baseline `46f908786`; BlueTSC selects the import type for both and reports four TS2322 errors. Format, both-crate all-target Clippy and all six test hashes pass; all 23 draft production files are restored from the committed baseline for this replay. Evidence: `/private/tmp/blueice-k14-linux/blueice-k62-resolution-red-{status,report}.json`, logs/hashes and both independent condition probes. Per-edge resolution mode, canonical owner bounds and fingerprints are required before K.6.2 closes; this supplemental test-only baseline stays unpushed until complete implementation verification.
  - *Relative-resolution/identity failing baseline (2026-10-09):* Pinned trace confirms `.ts` → `.tsx` → `.d.ts`. Public resolver tests require this priority, earlier-candidate invalidation and canonical-root symlink refusal. The public CLI artifact fingerprint must change with an observed manifest even if its chosen source does not. On committed production `10e688937`, format/all-target Clippy pass; 14 controls pass and all three new assertions fail. All 5,176 backend hashes match the test-only snapshot; no production draft is included. Evidence: `/private/tmp/blueice-k14-linux/blueice-k62-relative-red3-{status,report}.json`, frozen hashes and trace. Commit before production; keep unpushed until K.0 passes.
  - *JSON asset failing controls (2026-10-09):* Before JSON production support, three public owner-CLI controls require original asset bytes, nested JSON-derived types, content-sensitive artifact fingerprints, explicit effective `resolveJsonModule`, malformed-input refusal and canonical symlink bounds. The disabled-option control passes; valid asset and canonical-root controls fail because JSON is unresolved. Format/all-target Clippy pass; all 317 frozen draft-source hashes match. The existing 86-program committed pinned baseline supplies the independent positive JSON references. Evidence: `/private/tmp/blueice-k14-linux/blueice-k62-json-red-{status,report}.json` and logs/hashes. This additional test-only commit precedes JSON production work and stays unpushed until K.0 passes. K.6.2 remains open.
  - *JSON direct-entry failing regression (2026-10-09):* The complete 86-program pinned matrix now passes all verdict/primary/related, 54 Node and 60 exact declaration witnesses; the two conditional-resolution programs, three JSON asset controls and 18 resolver/cache controls pass. A new direct test on the JSON-support draft exposes an empty executable graph for a JSON entry; the runtime-import refusal and four earlier direct controls remain. Format/all-target Clippy pass; all 322 hashes match. Evidence: `/private/tmp/blueice-k14-linux/blueice-k62-sixteenth-{status,report}.json` and logs/hashes. Commit the public refusal witness before correcting that entry guard; the full K.0 and hosted CI gates remain open.
  - *Contextual import failing regression (2026-10-09):* Four additional pinned 5.9.3 programs accept `type` as an ESM/CommonJS default binding and a CommonJS value/type-only `import =` binding. All four independent Node programs print `42` and emit declarations before BlueTS reports parser failures. The eighteen-round frozen gate passes format/all-target Clippy and restores class discovery, all 18 CLI controls, all 123 frontend controls and the pure-type namespace `typeof` refusal. One inline `type Missing` primary span still needs refinement. Evidence: `/private/tmp/blueice-k14-linux/blueice-k62-eighteenth-{status,report}.json` and logs/hashes. Commit this public contextual baseline before changing its parser; K.6.2 and the full gates remain open.
  - *JSON parser budget failing regression (2026-10-09):* Four public closed-loader controls retain Unicode asset bytes and owner source-byte, token and schema-depth bounds. Byte/depth refusal and the positive control pass; JSON ignores `max_tokens = 2` and incorrectly emits an asset. Format and both-crate all-target Clippy pass, and all 326 frozen backend hashes match. The preceding complete gate executes all 1,187 tests in 90 groups with no ignored tests left: 1,186 pass and only the shared diagnostics live recorder fails. Both shared record files have 28 ordering differences after two fixture renames, with identical 2,464-case identity sets. Evidence: `/private/tmp/blueice-k14-linux/blueice-k62-json-limits-red-{status,report}.json`, `blueice-k62-twentieth-{status,report}.json` and `blueice-k62-diagnostics-order-proof.json`, logs/hashes. The test-only commit precedes enforcement of the existing token limit; source ordering and the complete K.0/final-source hosted gates remain open.
  - *JSON default-property failing regression (2026-10-09):* Two additional pinned 5.9.3 programs distinguish a JSON module's default object from the object's own `default` property. The accepted reference prints `42` and emits an inferred `number` declaration; the rejected string assignment reports exact TS2322 text/location and emits nothing. BlueTS instead reports TS2339 on both property accesses. Format and both-crate all-target Clippy pass; all 327 frozen backend hashes match. The prior complete frozen gate passes all 1,191 tests in 91 groups, including every 155 ignored oracle, and all 1,036 ordinary tests; the actual 2,464-case/211-template live diagnostics record is byte-identical after ordering correction. Evidence: `/private/tmp/blueice-k14-linux/blueice-k62-json-default-red-{status,report}.json`, `blueice-k62-final-first-{status,report}.json` and independent `blueice-k62-json-default-probe/` records, logs/hashes. Commit this failing public baseline before protecting the module default binding from field-name collisions. K.6.2 and final-source hosted CI remain open.
  - *Verified (2026-10-09):* All 86 pinned matrix programs (63 accept, 23 reject), 54 Node and 60 exact declaration witnesses pass, together with eight supplemental pinned resolution-mode/contextual-name/JSON-default programs, five direct controls and public JSON asset, resource-limit, canonical-root and observation/cache controls. Type-only syntax is erased only from JavaScript; declarations preserve its static surfaces. JSON uses the existing lexer token budget, retains original asset bytes and protects the module default binding from an object field named `default`. Runtime-profile refusals remain. Two fixture renames are reflected in deterministic shared record ordering without changing any recorded observation. The frozen-source Linux K.0 gate passes format, both-crate all-target Clippy with warnings denied, 1,036 ordinary tests and all 1,192 tests in 92 target/doctest groups, including every one of the 156 ignored oracles in 49 suite files. All 327 frozen backend hashes match. The actual shared diagnostics record is byte-identical to the committed 2,464-program/211-template record. Evidence: `/private/tmp/blueice-k14-linux/blueice-k62-final-second-{status,report}.json`, frozen hashes and logs. Generated inventory: 161 refusal sites in 11 areas. Largest production source: `compiler.rs`, 1,224 lines; execute queued K.6.R.2 as a separate tested commit/push before K.6.3. Full hosted workspace/platform/coverage CI remains open, including the unresolved macOS real-core navigation timeout on prior source.
### K.6 Module syntax — M8 — gap G-T10
  - *Failing baseline (2026-10-09):* On production `4e975cef`, 48 pinned TypeScript 5.9.3 programs (28 accept, 20 reject) retain ambient named/implicit/merged/shorthand modules, module/global augmentation and `reference path/types/lib`. All 28 independent Node/declaration controls run before BlueTS comparison. The public CLI reports 46 verdict/primary differences and 26 emission fixture differences. Format and both-crate all-target Clippy pass; completeness and both pinned recorders pass. Shared replay records 74 diagnostic and 20 presentation differences. All 157 frozen backend hashes match. The shared corpus contains 2,512 programs and 216 templates; all prior 2,464 full observations, templates and 638 strict replay roots are unchanged. Evidence: `/private/tmp/blueice-k14-linux/blueice-k63-baseline-final-{status,report}.json`, `blueice-k63-corpus-baseline-report.json`, `blueice-k63-shared-comparison.json` and `blueice-k63-baseline-final-oracle-reuse.json`, logs/hashes. No production change; commit this test baseline before implementation and keep it unpushed until K.0 passes. K.6.3 and full hosted CI remain open.
### K.6 Module syntax — M8 — gap G-T10
  - *Imported-global baseline supplement (2026-10-09):* Four imported-global context controls and the required MPL headers expand the pinned ambient corpus to 52 programs (30 accept, 22 reject), and the shared corpus to 2,516 programs with the same 216 templates. Previous observations retain their exact content after header position adjustment. The frozen draft passes format and both-crate Clippy; its 36 ambient checking, 44 shared diagnostic and 14 presentation differences keep K.6.3 open. See PLAN.md and `/private/tmp/blueice-k14-linux/blueice-k63-context-red-report.json` for the measured scope. Commit only the test supplement before its production repair.
### K.6 Module syntax — M8 — gap G-T10
  - *Global augmentation cache baseline (2026-10-09):* Three public compiler controls expose incorrect incremental reuse after either a global augmentation or its imported type changes. TypeScript and fresh BlueTS reject the unconnected consumer with TS2322, while the incremental draft accepts both; the alias-scope TS2304 control passes. The frozen 5,368-input gate passes format and both-crate all-target Clippy, with one test passing and two failing. The earlier complete draft replay runs all oracles: 1,192 pass and four known ambient/shared tests fail, zero ignored, in 93 groups. See PLAN.md and `blueice-k63-cache-red-report.json`. Commit the failing cache tests before production invalidation repair. K.6.3 remains open.
### K.6 Module syntax — M8 — gap G-T10
  - *Global augmentation cache draft (2026-10-09):* Following baseline `5086a2a85`, the old/new dependency closure invalidates consumers after an affected global augmentation or its transitive import changes. The frozen 5,368-input Linux gate passes format, both-crate all-target Clippy and all three regressions. Ordinary replay has 1,038 pass, three known ambient/shared failures and 158 ignored in 94 groups, with no additional failing target. See PLAN.md and `blueice-k63-cache-green-report.json`. Production remains an uncommitted draft; named ambient modules, module augmentation, references and the complete K.0 gate remain open.
### K.6 Module syntax — M8 — gap G-T10
  - *Static graph/cache baseline (2026-10-09):* The frozen draft now matches all 52 ambient/reference programs, 30 Node/declaration witnesses, shared diagnostics and three global-cache regressions. Six independent pinned graphs agree with fresh BlueTS; incremental referenced globals still reuse an accepted, unconnected consumer. Named and module-augmentation cache controls pass, as do four exact owner-edge/resource/runtime-authority controls. The frozen 5,374-input baseline passes format and both-crate Clippy, with six new tests passing and one failing; all source hashes match. Evidence: `blueice-k63-static-red-report.json`, `blueice-k63-reference-first-report.json` and `blueice-k63-static-cache-probe/report.json`. Commit the failing static-cache tests before repair and keep them unpushed until K.0 passes. K.6.3 and complete hosted CI remain open.
### K.6 Module syntax — M8 — gap G-T10
  - *Reference cache/library baseline (2026-10-09):* The frozen reference invalidation draft passes all seven static controls, three earlier cache regressions and all 1,048 ordinary tests in 96 groups, with 158 ignored oracles. A further pinned TypeScript probe verifies the 100 library selector labels and accepted `dom`/`es5` controls. The new public refusal test fails because a known, unavailable library is incorrectly reported as absent (TS2726); four other boundary controls and three earlier cache controls pass. Format and both-crate Clippy pass, all source hashes match. See PLAN.md, `blueice-k63-static-green-report.json`, `blueice-k63-library-red-report.json` and the library catalog probe. Commit this test-only baseline before repair; full K.0 and hosted CI remain open.
### K.6 Module syntax — M8 — gap G-T10
  - *Identical referenced-root baseline (2026-10-09):* The known-library refusal repair passes. Two new public regressions fail when a canonical declaration with identical contents is selected by both a path reference and owner/project roots; pinned TypeScript accepts both root arrangements. The frozen 5,375-input gate passes format and both-crate Clippy, with five boundary controls and three earlier cache controls passing, two identical-root controls failing, and all source hashes matching. Metadata proof records 97 unique labels from 100 pinned library entries. See PLAN.md, `blueice-k63-reference-roots-red-report.json` and its independent probe. Commit the test-only baseline before the deduplication repair; K.6.3 and full hosted CI remain open.
### K.6 Module syntax — M8 — gap G-T10
  - *Ambient re-export baseline (2026-10-09):* The earlier frozen draft passes all 1,209 tests in 96 groups, including every 158 ignored oracle, with matching hashes and a byte-identical 2,516-case recorder. Independent composition controls expose failed type-only re-exports from named ambient surfaces. Twenty-four pinned additions expand the matrix to 76 programs (42 accept, 34 reject), with 42 Node/declaration witnesses, and the shared corpus to 2,540 programs with the unchanged 216 templates. Every prior full observation is unchanged. The expanded frozen 5,475-input baseline passes format/Clippy, completeness/live recording, but checking and emission fail on the new forms. See PLAN.md, `blueice-k63-reexports-red-report.json` and corpus/probe evidence. Commit the test supplement before static export propagation repair; keep it unpushed until expanded K.0 passes. Original hosted CI is terminal with 27/29 success: macOS 15 Intel navigation and the final CI gate fail; coverage and both pinned oracle jobs pass. K.6.3 and full hosted CI remain open.
### K.6 Module syntax — M8 — gap G-T10
  - *Exported ambient value baseline (2026-10-09):* The 76-program draft passes all 1,209 tests in 96 groups, including every 158 ignored oracle, with matching hashes and a byte-identical 2,540-case recorder. Four pinned controls expand the ambient corpus to 80 programs (44 accept, 36 reject), 44 Node/declaration witnesses, and the shared corpus to 2,544 programs/217 templates; every prior full observation is unchanged. The final frozen 5,492-input baseline passes format and both-crate Clippy; public controls have one pass and two failures, and expanded checking/emission fails. Checked misses TS1362/1377, valid type queries retain runtime imports, and type-export declarations change source quotes. TranspileOnly retains its explicit unchecked policy and receives no ambient physical resolution. See PLAN.md, `blueice-k63-runtime-red-third-report.json` and independent recorder/probe evidence. Commit this test-only supplement before repair, keep it unpushed until expanded K.0 passes. K.6.3 and full hosted CI remain open.
### K.6 Module syntax — M8 — gap G-T10
  - *Exported-type correction draft gate (2026-10-09):* Following baseline `f86740e44`, Checked value use retains TS1362/1377 export origins, legal value-query imports are elided from JavaScript, declarations preserve source quotes, and `module-exports-v4` enters cache identity. All 80 pinned ambient programs, 44 Node/declaration witnesses and three public controls pass. An independent pinned proof corrects one old quote-normalizing unit expectation. Final format, both-crate all-target Clippy, 16 static/cache controls and all 1,054 ordinary tests in 97 groups pass; the complete K.0 gate passes all 1,212 tests in 97 groups, including every 158 ignored oracle, with zero failures and zero ignored tests. All 5,492 frozen hashes match. See PLAN.md, `blueice-k63-runtime-final-second-{status,report}.json` and retained earlier failure reports. Inventory is regenerated to 162 sites/11 areas; largest production source is 1,182 lines. The 2,544-case live diagnostic record is byte-identical. K.6.3 is complete in its measured scope; commit/push its implementation and baselines. M8 workspace and complete hosted CI remain open.
#### K.6 source refactoring queue
  - *Verified (2026-10-09):* Move the entry/root execution-control methods into `debugger_support/execution_control.rs`. Mechanical comparison preserves every moved method byte and string literal; the parent only removes those methods and declares the child module. Parent source is 959 lines and child source is 317 lines. The independent frozen five-file Linux gate passes format, engine all-target Clippy with warnings denied, launcher binary build, the public completed-module/classic regression and all 508 engine tests. All hashes match. Evidence: `/private/tmp/blueice-k14-linux/blueice-k6-root-refactor-final-{status,report}.json`, frozen hashes/logs and `blueice-k6-root-refactor-mechanical-report.json`. This refactor gets a separate commit/push; full normal final-source hosted CI remains open.
#### K.6 source refactoring queue
  - *Verified (2026-10-09):* Mechanical comparison after formatting preserves graph-loader and fingerprint bodies and string literals, apart from required `pub(super)` visibility. Source sizes are `compiler.rs` 756, `project_builder.rs` 348 and `fingerprint.rs` 140 lines; largest production source is the CLI at 1,165 lines. Public APIs, canonical bounds, per-edge modes, resolver observations, work limits, cache identity/reuse and source provenance are preserved. The frozen Linux gate passes format, BlueTS/bridge/engine all-target Clippy with warnings denied, all 507 engine tests, 1,036 ordinary BlueTS/bridge tests and all 1,192 tests in 92 groups, including every 156 ignored oracle in 49 suite files. All 331 backend hashes match, and the actual 2,464-program diagnostics record is byte-identical. Evidence: `/private/tmp/blueice-k14-linux/blueice-k62-refactor-final-{status,report}.json`, logs/hashes and `blueice-k62-refactor-mechanical-report.json`. Inventory remains 161 sites in 11 areas. Full hosted workspace/platform/coverage CI remains open; test-only navigation/HTTP diagnostics are integrated for the unresolved macOS timeout, without changing assertions or timeouts.
#### K.6 source refactoring queue
  - *Verified (2026-10-09):* Module records retain their public re-exports in `parser.rs`; declaration emission and its export/type-parameter helpers move to `emitter/declarations.rs`. Mechanical comparison preserves every AST field, declaration code body and string literal. Source sizes are parser 1,150, module records 68, emitter 863 and declarations 357 lines; all BlueTS production sources stay below 1,200. The complete frozen four-file K.0 gate passes format, both-crate all-target Clippy with warnings denied and all 1,169 tests in 83 groups, including every one of the 150 ignored oracles in 44 suite files. All source hashes match. Evidence: `/private/tmp/blueice-k14-linux/blueice-k6-refactor-final-{status,report}.json`, logs/hashes and `blueice-k6-refactor-mechanical-report.json`. Refusal inventory regenerated to 160 sites in ten areas. The refactor gets its own commit/push; K.6.2/K.6.3 and hosted complete CI remain open.
### K.7 Emit breadth — M9 — gaps G-E1 to G-E4
  - *Isolated failing baseline (2026-10-10):* 187 programs cover eleven targets and seventeen forms. Pinned TypeScript 5.9.3 accepts 180 and rejects seven; every accepted reference passes Node execution, exact declarations and pinned Acorn 8.15.0 edition validation. The repository recorder reproduces all observations. The shared record grows from 2,544 to 2,731 cases/219 templates, with every old observation unchanged. On unchanged `1f76ff28c` production, the public replay has two passing and four failing tests: target/config verdicts, emitted output, helper-version manifest identity and existing ES2020 logical-assignment syntax. Format, both-crate all-target Clippy and shared completeness pass. Evidence: `blueice-k71-test-baseline-second-{status,source-hashes}.json`, `blueice-k71-shared-reference-proof.json` and logs. Commit the failing baseline on the isolated branch; keep it unpushed and leave implementation pending until the M8 complete hosted CI gate closes. BlueJS currently has no edition-selecting parser API; Acorn is independent test evidence, not completion of that requirement.
  - *Independent syntax witnesses (2026-10-10):* 55 authored JavaScript examples record 605 pinned edition decisions (403 accept/202 reject), including ES5 controls for property keywords and future-looking string/comment text. All 55 parse through the current public BlueJS API; the live independent recorder, completeness, format and both-crate all-target Clippy pass. Evidence: `blueice-k71-syntax-profiles-{status,source-hashes}.json`. Older-edition rejection remains pending, and no production source is changed.
  - *Module-paired failing baseline (2026-10-10):* Both existing module kinds expand the corpus to 374 programs (360 accept/14 reject). Every accepted reference passes Node/declarations/edition parsing, and all paired observations agree; the original 187 CommonJS observations remain unchanged. The shared record now has 2,918 cases/219 templates with all previous 2,731 observations unchanged. Format, both-crate all-target Clippy and completeness pass. The expanded target replay remains two pass/four fail; all 402 primary and fourteen presentation differences in shared replay belong to targets. Evidence: `blueice-k71-esm-*-proof.json`, `blueice-k71-esm-targets-status.json` and `blueice-k71-esm-baseline-final-third-status.json`. Production is unchanged, the baseline stays isolated/unpushed, and M8 must complete before implementation.
  - *Control-flow failing baseline (2026-10-10):* Twelve more forms cover return/throw/finally, delegated and async iterator closing, rejected await, lexical receivers, getter/default ordering and tuple spread. All 638 pinned programs (600 accept/38 reject) are recorded; all 600 Node/declaration/syntax references pass. TS2496/TS2556 rejection controls are retained with independently accepted companions. The shared 3,182-case/221-template record preserves all previous 2,918 observations. An ordinary build/declaration replay covers all accepted emit paths. Format and both-crate all-target Clippy pass; target replay is two pass/five fail, and all 714 primary/38 presentation differences belong to targets. Evidence: `blueice-k71-control-flow-*-proof.json` and frozen final reports/logs. Production is unchanged and K.7.1 remains isolated/unpushed pending M8 and implementation.
  - *Edition implementation in progress (2026-10-10):* Public BlueJS `SyntaxEdition`, `parse_with_edition` and `parse_module_with_edition` select all eleven recorded versions. An iterative AST walk and consumed-token lexical checks match all 605 original observations (403 accept/202 reject); all 55 latest ASTs remain identical to the original library. Another 341 pinned context observations (264 accept/77 reject) cover nested templates, Unicode, RegExp and property-name controls. Actual native replays and live Acorn pass; Cargo format, three-crate all-target Clippy and four bridge syntax tests pass. Full BlueJS regressions pass: 4,109 tests in 335 groups, zero failures; the final bridge routing passes all five syntax tests and all four ignored BlueJS Node differential tests pass. The live context oracle is routed through the existing bridge CI job. Production stays isolated/uncommitted/unpushed; target lowering, helpers, library/iteration options and the complete K.0 gate remain open. Evidence: `blueice-k71-edition-*-report.json`, `blueice-k71-edition-cargo-first-status.json` and `blueice-k71-context-oracle-ci-routing-proof.json`.
  - *Configuration and first lowering in progress (2026-10-10):* Explicit ECMAScript `lib` selection and `downlevelIteration` are read through the API/configuration, recorded in artifacts and bound to fingerprints. All 29 pinned library verdicts match; sixteen focused tests, the six existing library oracles and eighteen CLI regressions pass. Additional owned library profiles are cumulative; existing ES2020/ES2022 declaration source bytes are retained. ES2020 binding logical assignments now preserve Node results and pass the ES2020 syntax floor. An original nullish helper has a versioned manifest identity; all 22 target/module fingerprints are distinct. A further Node comparison matches pinned TypeScript and native ES2021 for getter reads, short circuits, nested assignments, throws, helper-name collisions and `await`. BlueJS now accepts the named async export; its full ordinary suite passes 4,110 tests and all four Node oracles. Two new Acorn controls preserve all 341 original context observations and expand the record to 363 (279 accept/84 reject). Braced statement boundaries, computed object methods and explicitly annotated named async generators now pass their pinned public regressions. An original ES2015 exponentiation transform passes Node effect-order, declaration and edition checks. The latest frozen replay (`blueice-k71-async-generator-implementation-second-status.json`, 7,006 files) passes format, three-crate all-target Clippy, 241 BlueTS unit tests, all nine focused context/library tests and all 22 protocol declarations. Explicit receiver parameters also pass thirteen pinned diagnostic sources and Node/direct-runtime arity and declaration comparisons. The receiver replay (`blueice-k71-explicit-this-implementation-third-status.json`, 7,011 files) reduces target differences to 88 across four forms. Array-rest calls, numeric/string reduce contexts and rest declaration ellipses then pass eight pinned diagnostic sources and Node/direct-runtime/declaration comparisons. The latest frozen replay (`blueice-k71-rest-array-implementation-third-status.json`, 7,017 files) passes format, three-crate all-target Clippy, 241 unit tests and all 22 protocol declarations; target differences fall to 66 across binding patterns and guarded catch yields, with three failing ordinary target tests. Remaining lowering/checker work and K.0 are open. Production remains uncommitted/unpushed.
  - *Binding, flow and declaration progress (2026-10-10):* Twelve pinned catch/try/finally controls, ten variable-pattern observations (including both relocated source-name rejections), ten constant-template observations and their live recorders are covered. Flat object/array defaults and rest have retained AST records and direct BlueJS lowering; getter/default effects and exact declarations match native Node/TypeScript. Exported/ambient, re-exported and nested variable patterns remain precise refusals. All 638 target verdicts and primary diagnostics now match; all 600 accepted target declarations, the owned target declarations and the shared diagnostic matrix pass in `blueice-k71-template-constant-final-gate-status.json` (7,028 files). That ordinary replay passes 1,089 tests but exposes an obsolete destructuring syntax expectation; both original missing-source controls are moved to exact native TS2304 replays. The corrected complete ordinary/every-ignored gate is pending. Original downlevel transforms and the final K.0 closure remain open; production remains isolated/uncommitted/unpushed. Largest production source: 1,190 lines.
  - *Original downlevel progress (2026-10-10):* The corrected complete ordinary gate passes 1,090 tests; every ignored oracle finishes with 179 passes and two target/protocol syntax failures. There are no target build/runtime/declaration differences. Original optional/nullish, object spread and flat object-rest transforms pass format, three-crate all-target Clippy, native effect/declaration controls and the prior target diagnostics/declarations. Enumerable own-value copies include symbols and prototype-named data; ordinary property runs preserve descriptors. Rest defaults execute before copying excluded keys. The latest frozen 7,035-file gate passes 241 unit tests and nine target tests, with one remaining target test reporting 73 older-edition syntax differences, down from 101. Every helper source contributes to fingerprint identity. The subsequent original async, async-iteration and async-generator transforms pass their native request/effect/declaration witnesses and reduce remaining syntax differences to 49, all ES5; every ES2015-or-newer target now passes syntax checks. Indexed calls now pass all six exact pinned acceptance/diagnostic controls, 1,091 ordinary tests and the shared diagnostic/target matrix. See PLAN.md and frozen `blueice-k71-{async-generator-first,indexed-call-broad}-status.json`. Original ES5 lexical/loop/arrow, default/rest, template, flat-pattern, for-of and computed-property lowering then passes the committed scope oracle, format, Clippy and 241 unit tests. Remaining syntax differences fall to 28, with no build/runtime/declaration differences (`blueice-k71-es5-objects-first-status.json`, 7,061 frozen files). Original ES5 array/call spread and named-class/inheritance lowering then passes those gates and all target build/runtime/declaration observations; syntax differences fall to 18, exclusively generator/async/for-await (`blueice-k71-es5-classes-second-status.json`, 7,065 frozen files). Full pre-suspension verification finishes with 1,091 ordinary tests and 184 ignored oracles passing; two ignored tests retain only those ES5 syntax differences (`blueice-k71-es5-baseline-full-status.json`). Original structured generator/async continuations and loop ownership then pass format, Clippy, 241 unit tests, all ten target tests and all three protocol tests: 638 verdicts, 600 declarations/Node/Acorn observations and all 22 protocol configurations match (`blueice-k71-es5-statements-first-status.json`). A separate public-API gate confirms all 600 emitted programs parse through BlueJS at the selected edition (`blueice-k71-emitted-bluejs-first-status.json`). Unsupported named ES5 suspension graphs refuse emission explicitly. Inventory is regenerated; the final whole-leaf K.0 gate remains pending and production is uncommitted/unpushed.
  - *Verified (2026-10-10):* The final 7,071-file frozen-source K.0 gate passes format, three-crate all-target Clippy with warnings denied, all 1,093 ordinary tests and every one of the 187 ignored oracles in 122 target/doctest groups (1,280 unique tests). All 6,939 backend/Cargo file hashes match the tested snapshot. The 638 pinned verdicts, all 600 accepted Node/declaration/Acorn observations, all 600 actual outputs through the edition-selecting public BlueJS parser, every protocol configuration and the shared 3,182-case diagnostic matrix agree. Unsupported named ES5 suspension graphs produce an explicit diagnostic and no artifact. The regenerated inventory contains 197 refusal sites; G-E1 retains unmeasured combinations. The largest changed production source is 1,190 lines, below the 1,200-line review threshold. Evidence: `blueice-k71-final-k0-first-{report,status,source-hashes}.json`, exact logs and `blueice-k71-final-source-size-audit.json`. Commit and integrate this green leaf, then run complete hosted CI on that final source.
  - *Complete CI storage failure (2026-10-10):* Run [38015722230](https://github.com/ephoton0210/blueice/actions/runs/38015722230) on `411a1a41c` proves 6,902 workspace tests in 530 groups with zero failures on RHEL UBI 10 and all 187 ignored oracles in 122 groups on macOS. Its coverage runner exhausts disk space; GitHub check annotations identify `System.IO.IOException: No space left on device` in the runner log writer, so it publishes no coverage result. Disable Cargo incremental and dev/test debug information for CI to reduce build artifact storage. Keep both coverage thresholds and all existing exclusions unchanged; run the complete workflow on the pushed CI fix. This does not change K.7.1 backend/Cargo source hashes.
### K.7 Emit breadth — M9 — gaps G-E1 to G-E4
  - *Isolated failing baseline (2026-10-10):* 95 original programs/configurations cover AMD/UMD/System (3), per-file Node16/NodeNext and verbatim syntax (24), helper options with an original provider (32), and live/default/re-export/type/cycle/order contexts (36). Live pinned TypeScript 5.9.3 confirms 77 accepts and 18 rejects, exact diagnostic origins, declarations and per-file output suffixes. The public BlueTSC CLI differs in 86 decisions on unchanged K.7.1 production. The 7,398-file third snapshot passes format/all-target Clippy and both native completeness and recorder tests; only the public decision replay fails. The initial archive omitted six original dependency files; their native source hashes and root sets are restored before the native replay passes. Native execution observations remain recorded from the original probes; a repository BlueTSC emit-and-run replay is still required before implementation closure. The native-generated `module-systems-checker-matrix.tsv` and final 7,399-file gate pass format, three-crate all-target Clippy, matrix completeness and native replay; only the public decision test fails (two pass/one fail). Commit this failing replay before implementation. Keep this test preparation isolated/unpushed while the complete K.7.1 hosted CI runs on `411a1a41c`.
  - *Native execution failing supplement (2026-10-10):* Original closed AMD/UMD/System loaders, both UMD paths for the wrapper controls, real Node per-file execution and an original helper provider reproduce all 77 accepted native observations in the live recorder. The frozen 7,400-file gate passes format, three-crate all-target Clippy, completeness and live native decisions/declarations/execution. The public BlueTSC replay fails with 86 decision differences and 69 emitted differences (68 unsupported configurations and one CommonJS side-effect-import declaration difference). Eight existing CommonJS contexts match execution/declarations. Evidence: `blueice-k72-execution-baseline-first-{status,source-hashes}.json` and exact logs. All existing K.7.1 production hashes are unchanged; commit this failing supplement before implementation and keep the branch unpushed.
  - *Verified (2026-10-10):* All 106 pinned configurations (82 accept, 24 reject) match native verdicts and primary diagnostic origins; all accepted actual execution, declaration and per-file suffix comparisons pass. Original AMD/UMD/System wrappers preserve measured bindings and cycles; owner-selected Node formats and nearest-package observations revalidate without wider authority. Six no-import/export Node controls require the native TS2354 helper refusal. The immutable 7,445-file final K.0 gate passes format, three-crate all-target Clippy, 1,107 ordinary tests and all 190 ignored oracles (1,297 total; 127 target/doctest groups per test command). Evidence: `blueice-k72-implicit-first-{status,source-hashes}.json` and `blueice-k72-final-verification-proof.json`. Largest production source: CLI 1,210 lines; publish this tested leaf, then separately test and publish the FileLoader split before K.7.3. Broader module/helper/target and declaration-input combinations remain unmeasured.
  - *Verified modularity split (2026-10-10):* FileLoader moves unchanged into a 276-line module; the CLI drops from 1,210 to 945 lines. The separate complete K.0 gate passes format, three-crate all-target Clippy, all 1,107 ordinary tests and all 190 ignored oracles. All frozen source hashes match. Evidence: `blueice-k72-cli-split-first-status.json` and `blueice-k72-cli-split-final-proof.json`. Publish this split separately before K.7.3.
  - *Hosted CI fixture correction (2026-10-10):* Complete CI [38047766020](https://github.com/ephoton0210/blueice/actions/runs/38047766020) on `ec5823ed4` exposes 24 primary-origin differences in the Windows 11 VS2026 arm64 module replay. Windows Git converts the source fixtures to CRLF while their pinned native observations use LF; compiler positions correctly identify the actual CRLF input. A frozen Linux CRLF replay reproduces the same 24 case IDs, with three passes and one failure. Pin the module-system fixture tree to LF in `.gitattributes`. A forced `core.autocrlf=true` checkout retains all 354 original fixture byte sequences. Backend sources, fixture bodies, diagnostic expectations and coverage exclusions are unchanged. Evidence: `blueice-k72-windows-crlf-reproduction-proof.json`, `blueice-k72-windows-checkout-proof.json` and the retained Windows job log. A fresh complete final-source CI gate remains required.
  - *Verified fixture correction (2026-10-10):* The restored LF snapshot passes format, three-crate all-target Clippy and all six module-system tests, including the live TypeScript 5.9.3 recorder and all 82 accepted Node/declaration/output comparisons. All 106 verdicts and primary origins agree. All 7,446 snapshot files match; 7,314 backend/Cargo/workflow files match the previously complete K.7.2 CLI-split K.0 snapshot exactly. Evidence: `blueice-k72-windows-ci-green-status.json` and `blueice-k72-windows-ci-green-source-proof.json`. Publish the fixture fix and rerun the complete hosted CI on its final SHA.
  - *Verified hosted CI fixture repair (2026-10-10):* All five Windows jobs and both complete Native oracle jobs pass on `6dd75b11a`. A macOS 26 engine binary fixture races ordinary execution after a rejected child breakpoint. Use the existing negotiated next-document admission hold before both navigations, retain every state/security assertion, and deliberately cross idle scheduler turns. The immutable 7,446-file controlled red replay reproduces the exact Completed/Pending failure; the corrected snapshot passes five exact repeats, all 20 engine binary tests, format and warnings-denied engine all-target Clippy. Source hashes match. Commit and push this separate repair, then run complete hosted CI on the final integrated source. Evidence: `blueice-k72-debugger-idle-proof.json` and `blueice-k72-debugger-idle-source-proof.json`. Production and authority remain unchanged; two other jobs fail downloading crates before compilation.
### K.7 Emit breadth — M9 — gaps G-E1 to G-E4
  - *Final verified leaf (2026-10-10):* The immutable 7,864-file snapshot passes format, warnings-denied three-crate all-target Clippy, all 1,111 ordinary tests and all 194 ignored oracles (1,305 total; 129 target/doctest groups per test command). All 176 output configurations (170 accept, six reject), 18 validation controls, actual accepted Node observations, exact declarations, map metadata, option diagnostics and strict-format boundaries agree with the pinned reference. All 7,730 backend/Cargo/workflow hashes match the final tested implementation. Largest BlueTS source is 1,186 lines; the largest changed source is 986 lines. Inventory records 206 precise refusals across 11 areas and retains unmeasured nested/inferred declarations, complete mapping parity and later declaration-layout work. Commit and push this coherent K.7.3 implementation; integrate the separately tested debugger fixture correction, then run complete hosted CI on the final source. Evidence: `blueice-k73-final-k0-seventh-proof.json` and `blueice-k73-signature-members-focused-proof.json`.
  - *Verified callable/indexed alias correction (2026-10-10):* The 128-configuration snapshot passes the complete frozen K.0 gate: format, three-crate all-target Clippy, all 1,111 ordinary tests and all 194 ignored oracles. The separate 48-control supplement reproduces exactly 40 declaration mismatches with three passing tests and one failing replay. Its 176-configuration correction then passes format, Clippy, all four output tests, all 170 native/public Node observations and exact declarations, plus the strict-format regression. The inferred-variable and constructor-arrow printers retain their behavior; empty aliases use native `{}` while empty interfaces retain their native multiline form. The final corrected complete K.0 gate now also passes; final-source hosted CI remains pending. Evidence: `blueice-k73-final-k0-sixth-proof.json` and `blueice-k73-signature-members-focused-proof.json`.
  - *Callable/indexed alias failing supplement (2026-10-10):* Forty-eight native controls cover call, construct and index signatures, their combinations, fully internal members and empty aliases in both module modes and all four comment/internal flag combinations. All 48 native executions and public builds succeed; 40 exact public declarations differ. The full native corpus now contains 176 configurations (170 accept, six reject); every previous 128 observation is unchanged. Commit this failing replay before extending the documented-member renderer; retain the existing inferred-variable and constructor-arrow printers. Evidence: `k73-signature-member-baseline.json` and `k73-signature-members-native-proof.json`. K.7.3 remains open.
  - *Verified output-boundary supplement (2026-10-10):* The complete 120-configuration implementation passes its frozen K.0 gate: format, three-crate all-target Clippy, 1,111 ordinary tests and all 194 ignored oracles (1,305 total). The additional eight type-alias controls then reproduce exactly six declaration differences with three passing tests and one failing replay. Their corrected 128-configuration implementation passes format, Clippy, all four output tests (including the live native recorder, 122 actual executions and exact declarations) and the strict-format regression. The complete corrected K.0 gate is running; production remains uncommitted/unpushed and K.7.3 remains open. Evidence: `blueice-k73-final-k0-fourth-proof.json` and `blueice-k73-alias-members-focused-proof.json`.
  - *Type-alias member failing supplement (2026-10-10):* Eight native controls cover member documentation and `stripInternal` on exported object type aliases in both module modes and all four flag combinations. All eight native programs execute successfully; six actual public declaration comparisons differ before correction. The full recorder contains 128 configurations (122 accept, six reject), with every previous 120 observation unchanged. Commit this failing supplement separately and retain exact native declarations before implementing. Evidence: `k73-alias-member-baseline.json` and `k73-alias-members-native-proof.json`. K.7.3 remains open.
  - *Interface/enum member failing supplement (2026-10-10):* Sixteen additional native declaration boundaries record member documentation and `stripInternal` behavior in both module modes and all four comment/internal flag combinations. All sixteen native programs execute successfully; twelve public declaration comparisons fail before correction. The complete recorder now verifies 120 configurations (114 accept, six reject), all 114 actual Node executions, exact declarations and output metadata. Every previous 104 observation is unchanged. Keep this failing replay isolated/unpushed and correct these remaining output boundaries before closing K.7.3. Evidence: `k73-type-member-baseline.json`, `k73-type-member-native-proof.json` and the reproducible native corpus.
  - *Option-validation failing supplement (2026-10-10):* Eighteen additional pinned controls record eight accepts and ten rejects, real native CLI exits/file inventories, exact primary diagnostics, and accepted Node executions/declarations. The frozen public CLI differs in six cases: both `downlevelIteration` command values, three missing-map dependencies, and the invalid `newLine` diagnostic. Command `sourceMap` overrides repair otherwise invalid project combinations. Native CLI configuration-type errors can emit files despite `noEmitOnError`; these observations are retained without assuming all rejected configurations suppress output. Commit the new replay separately before its production correction. Evidence: `k73-output-validation-blue-baseline.json` and the original native recorder/reference corpus. The complete existing K.0 gate remains running; K.7.3 remains open.
  - *Isolated output implementation and member baseline (2026-10-10):* All original 78 native configurations (72 accept, six TS5102 rejects) match public CLI verdicts and primary diagnostic origins; all accepted Node executions, exact declarations, BOM/newline/comment properties and source-map metadata agree. Format and three-crate all-target Clippy pass on the frozen implementation. Sixteen additional class/namespace member controls all execute successfully under pinned TypeScript 5.9.3; their public declaration replay has four passes and twelve failures before the member correction. All original observations remain unchanged. The corpus now contains 94 configurations (88 accept, six reject). Commit the new failing controls separately and keep the implementation isolated/unpushed until the complete K.0 gate and final-source hosted CI pass. Evidence: `blueice-k73-options-implementation-fourth-{status,source-hashes}.json`, `blueice-k73-member-native-evidence.json` and `blueice-k73-member-blue-baseline.json`. K.7.3 remains open.
  - *Member and strict-format correction; map-root baseline (2026-10-10):* All 94 native configurations, Node/declaration observations and the live recorder now pass. The existing strict-runtime CLI/Node regression covers both ES2020/ES2022, both LF/CRLF and both BOM modes; all eight combinations pass without changing contract rejection behavior. Format and three-crate all-target Clippy pass. Ten additional native path/URL/empty-root controls execute successfully; the BlueTSC header/directive replay records four passes and six failures before the root correction. The corpus grows to 104 configurations (98 accept, six reject); only temporary absolute case paths are replaced by `<case>` in source-map observations. All prior observations remain unchanged. Evidence: `blueice-k73-members-implementation-second-status.json`, `k73-root-controls-native.json` and `blueice-k73-root-blue-baseline.json`. The complete K.0 and hosted CI gates remain open.
### K.7 Emit breadth — M9 — gaps G-E1 to G-E4
  - *Verified native failing baseline (2026-10-10):* Record the complete 20-form × eleven-target × two-module × two-field-mode corpus: 880 configurations, 816 native accepts and 64 no-output rejections. All accepted actual native JavaScript executes: 782 directly under Node and 34 preserved ESNext proposal outputs through a documented TypeScript 5.9.3 emitted-JavaScript-only ES2023 adapter. Raw proposal syntax, parser verdicts, exact declarations and effects remain recorded. The prior public replay accepts only 148 configurations, with 668 verdict differences, 52 rejected-primary differences and ten ES5 generated-syntax failures; the final K.7.3 source now reproduces exactly those counts. The immutable 10,510-file snapshot passes format and three-crate all-target Clippy, and its complete test command has two passes (matrix completeness and live native recorder) and two expected public replay failures. Source hashes match. Evidence: `blueice-k74-baseline-first-proof.json`. Commit the failing replay before implementation and keep it isolated/unpushed until the complete corrected K.0 gate passes. Evidence: `k74-full-native-proof.json`, `k74-portable-tools-proof.json`, `k74-portable-observer-native-proof.json` and `k74-blue-current-public-baseline-proof.json`.
  - *Complete 880-configuration focused replay (2026-10-11):* Format, three-crate all-target Clippy and all four decorator-target tests pass, including both ignored oracles. All 816 actual accepted emitted outputs, exact declarations and syntax/proposal observations agree; all 64 rejected primaries match. Native golden bytes remain unchanged. ESNext proposal outputs retain the documented actual-emitted-JavaScript-only execution adapter. Integrate the verified regex CI fixture repair and migrated ordinary refusal regressions before running the complete K.0 gate. Private auto-accessors and broader static-initializer/block super remain open; K.7.4 is not yet complete. Evidence: `blueice-k74-declarations-first-proof.json`.
### K.7 Emit breadth — M9 — gaps G-E1 to G-E4
  - *Private-accessor/static-super supplement (2026-10-11):* Test-only commit `31394a54b` records 264 further native configurations (240 accept, 24 reject), with 190 verdict and sixteen primary differences before correction. The focused corrected replay passes format, three-crate all-target Clippy, every actual emitted program/declaration/syntax observation, exact rejected primaries and the live native recorder. Private auto-accessors retain lexical descriptors and brands; static initialization uses the replacement constructor receiver, while native static accessors retain dynamic receivers. Original 880 goldens remain unchanged. Evidence: `blueice-k74-private-accessor-super-first-proof.json`. The complete corrected K.0 gate remains running; K.7.4 remains open.
### K.7 Emit breadth — M9 — gaps G-E1 to G-E4
  - *Verified (2026-10-11):* All 1,144 pinned configurations agree (1,056 accepts, 88 rejects), including every actual emitted execution, exact declaration and syntax/proposal observation. Both live recorders verify unchanged native goldens. The immutable 11,313-file Linux K.0 gate passes format, three-crate all-target Clippy with warnings denied, all 1,118 ordinary tests and all 198 ignored oracles: 1,316 tests in 131 target/doctest groups per pass. All 11,177 backend/Cargo/workflow hashes match. Generated inventory: 211 refusal sites in eleven areas. Largest changed production source is 1,019 lines. Evidence: `blueice-k74-full-k0-second-proof.json` and retained source hashes/status/logs. K.7.4 is complete within the measured subset; unmeasured compositions remain explicit inventory gaps. Push the verified leaf and run final-source hosted workspace/platform/coverage CI.
### K.8 Declarations — M9 — gaps G-D1, G-D2
  - *Native failing baseline (2026-10-11):* 52 configurations (49 accept, three reject) across 26 forms and both module modes record exact inferred/generic/overload/accessor/merged namespace/export-assignment/ambient-module declarations. All actual native outputs execute. On unchanged K.7.4 production, all decisions/primaries and all 49 execution/syntax observations agree; 22 exact declarations differ. Format, three-crate all-target Clippy, completeness and the live recorder pass. The isolated test-only replay must be committed before correction and remains unpushed until complete K.0 passes. Evidence: `blueice-k81-baseline-second-proof.json` and actual declaration pairs. K.8.1 remains open.
  - *Freshness failing supplement (2026-10-11):* Sixteen further pinned controls retain regular annotated/asserted literals, const/explicit generic arguments, literal returns and enum element types. All 68 native configurations (65 accepts, three rejects) and all 65 actual executions are recorded with original 52 observations unchanged. Before array correction, format/three-crate Clippy pass, the public namespace-symbol regression passes, and the replay records two const-generic parser refusals plus thirteen exact declaration differences. Commit the supplement before correction. Evidence: `blueice-k81-freshness-baseline-first-proof.json`. Full K.0 remains open.
  - *Const-position failing supplement (2026-10-11):* Eight additional pinned controls bring the corpus to 76 configurations (69 accepts, seven rejects), retaining all original 68 observations. Function/constructor type aliases accept const parameters; interfaces and generic type aliases reject with exact TS1277. Before parser support, format/three-crate Clippy pass and the replay retains two expected failing tests. The namespace/export-assignment inference correction now passes. Evidence: `blueice-k81-const-positions-baseline-proof.json`. Commit these tests before parser correction; full K.0 remains open.
  - *Explicit enum-element supplement (2026-10-11):* Four further Native controls confirm mutable arrays widen to the owning enum even with explicit member assertions/annotations. All 80 configurations (73 accepts, seven rejects) retain original 76 observations. Format/Clippy and all verdicts, primaries, actual execution/syntax observations pass; four exact declarations remain different in these new controls. Evidence: `blueice-k81-focused-final-first-proof.json`. Commit the test-only replay before correcting named member types; K.8.1/full K.0 remain open.
  - *Focused corrected replay (2026-10-11):* All 80 pinned configurations agree (73 accept, seven reject): exact declarations and primaries, every actual emitted execution and syntax observation, live Native recorder and checked public namespace facts. Format and three-crate all-target Clippy pass. Original Native golden observations remain unchanged. Literal/enum widening, absent anonymous-object keys, const parameter positions and namespace declaration facts are corrected. Evidence: `blueice-k81-focused-final-second-proof.json`. The complete K.0 gate remains open.
  - *Callable-selection failing supplement (2026-10-11):* Four Native controls bring the corpus to 84 configurations (77 accepts, seven rejects), preserving original 80 observations. The exact frozen public CLI accepts all four but widens literal returns incorrectly when a local callable shadows a generic function or a regular literal overload precedes a generic overload. Record these tests before selecting the lexical callable and actual overload for freshness. Evidence: `k81-call-scope-baseline-proof.json`. The first 80-case full K.0 run remains immutable/running; final 84-case K.0 remains open.
  - *Selection-aware corrected replay (2026-10-11):* All 84 configurations (77 accept, seven reject) pass every exact declaration/primary and actual emitted execution/syntax observation, the live recorder and checked namespace facts. Freshness uses the lexical callable and the actual selected overload. Format and three-crate Clippy pass. Evidence: `blueice-k81-call-selection-first-proof.json`. The final full K.0 gate remains open.
  - *Verified (2026-10-11):* The immutable 11,576-file Linux K.0 gate passes format, three-crate all-target Clippy with warnings denied, all 1,121 ordinary tests and all 200 ignored oracles: 1,321 tests in 132 target/doctest groups per pass. All 11,440 backend/Cargo/workflow hashes match. Every one of the 84 native configurations agrees (77 accepts, seven rejects), including exact declarations/primaries and actual emitted execution/syntax observations. Original 52/68/76/80 native observations remain unchanged. The earlier 80-case full run was superseded after the four actual callable-selection declaration failures; this final 84-case run is complete. Largest changed source is parser.rs at 1,187 lines. Evidence: `blueice-k81-full-k0-second-proof.json`. K.8.1 is complete within the measured subset; broader G-D1 forms remain explicit. Commit and push the verified leaf; final-source hosted workspace/platform/coverage confirmation remains pending.
### K.8 Declarations — M9 — gaps G-D1, G-D2
  - *Native failing baseline (2026-10-11):* 48 configurations (28 accept, 20 reject) record declarationDir/declaration-only/maps/isolated checks in both module modes. Twenty-four actual Native JavaScript outputs execute; four accepted declaration-only builds emit no JavaScript. The corrected isolated baseline on verified K.8.1 passes format, three-crate Clippy, completeness and the live recorder, while public decisions and outputs fail as expected. Original Native fixture bytes remain unchanged. Evidence: `blueice-k82-baseline-second-proof.json`.
  - *Focused corrected replay (2026-10-11):* All 48 configurations agree on exact declarations, complete declaration-map JSON, output inventory, primary diagnostics/positions and every actual emitted execution/syntax observation. Three additional real CLI tests prove declaration-only/map overrides and pre-publication escape/symlink refusal. Format, warnings-denied three-crate all-target Clippy and all seven tests pass. Maps currently cover annotated top-level scalar variables/functions; broader mapping and isolated-declaration compositions remain explicit G-D2/G-E5 gaps with precise map refusal. Evidence: `blueice-k82-options-fourth-proof.json`. The complete K.0 gate remains open.
### K.8 Declarations — M9 — gaps G-D1, G-D2
  - *Verified (2026-10-11):* The immutable 11,730-file Linux K.0 gate passes format, warnings-denied three-crate all-target Clippy, all 1,126 ordinary tests and all 202 ignored oracles: 1,328 tests in 134 target/doctest groups per pass. All 11,592 backend/Cargo/workflow hashes match. All 48 Native configurations agree (28 accepts, 20 rejects), including 24 actual executions, four declaration-only builds, exact declarations/full map JSON/inventory and rejected primaries/positions. All 148 Native fixture files remain unchanged; three CLI owner-root/override guards pass. Generated inventory: 213 refusal sites in eleven areas. Largest changed production source is emitter.rs at 1,009 lines. Evidence: `blueice-k82-full-k0-first-proof.json`. K.8.2 is complete within the measured subset; broader G-D2/G-E5 maps and isolated forms remain explicit. Commit and push the verified leaf, then proceed to K.9.1; final-source hosted workspace/platform/coverage CI remains pending.
### K.9 Resolution extras — M9 — gap G-M1
  - *Native failing baseline (2026-10-11):* Test-only commit `213ac23a7` records 54 configurations (43 accepts, eleven rejects) with 43 actual Native executions, exact selected source graphs/declarations/inventory and rejected primary positions. On verified K.8.2, the corrected test-only replay passes format, three-crate all-target Clippy, completeness and the live Native recorder, while public decisions/output fail as expected (two passes, two failures). Test-only follow-ups fix Clippy inventory iteration and accept project-relative module IDs without changing Native goldens or exact diagnostic assertions. Evidence: `blueice-k91-baseline-third-proof.json`; the frozen public K.8.2 probe records 28 verdict and eleven primary differences. K.9.1 remains open pending corrected replay and full K.0.
  - *Focused corrected replay (2026-10-11):* All 54 Native configurations agree (43 accepts, eleven rejects), including every actual emitted execution, exact selected inputs/declarations/inventory and rejected primary diagnostics/positions. Original Native fixture bytes remain unchanged. Four real filesystem controls prove typesVersions target confinement, package-manifest symlink refusal before reading, changed manifest/target cache invalidation and nearer classic-file invalidation. Format, three-crate all-target Clippy and all eight tests pass. Evidence: `blueice-k91-package-draft-fifth-proof.json`. The complete K.0 gate remains open.
  - *Existing diagnostic regression (2026-10-11):* The first complete K.0 replay found the existing array-diagnostic raw-message assertion. Preserve its original inferred array and annotation labels while retaining the precise Native element types/span. Format, three-crate Clippy, all eight focused tests and the exact preexisting regression pass (`blueice-k91-diagnostic-second-proof.json`). The first full gate is superseded after that actual failure; the final complete gate remains open.
  - *Classic refusal migration (2026-10-11):* The second full replay reached an obsolete assertion requiring rejection of now-supported classic mode. The existing package test now proves missing installed-package lookup fails, a local classic file succeeds, and an unknown strategy still receives the precise configuration refusal. Format, three-crate Clippy, all eight Native/boundary tests and both exact existing regressions pass (`blueice-k91-classic-regression-first-proof.json`). No production or Native golden change was needed. Final full K.0 remains open.
  - *Verified (2026-10-11):* The immutable 12,247-file Linux K.0 gate passes format, warnings-denied three-crate all-target Clippy, all 1,132 ordinary tests and all 204 ignored oracles: 1,336 tests in 136 target/doctest groups per pass. All 12,107 backend/Cargo/workflow hashes match. Every one of the 54 Native configurations agrees (43 accepts, eleven rejects), including all actual emitted executions, selected physical inputs, exact declarations/inventory and rejected primary positions. All 510 Native fixture files remain unchanged. Four canonical-read/cache controls and both existing regressions pass. Generated inventory: 213 refusal sites in eleven areas. Largest changed production source is package_resolution.rs at 1,163 lines. Evidence: `blueice-k91-full-k0-third-proof.json`. The two earlier full runs retain their measured raw-message/obsolete-refusal failures; this corrected final gate is complete. K.9.1 is complete within its measured subset; unmeasured range/composition/declaration contexts remain G-M1 gaps. Commit and push the verified leaf, then proceed to K.9.2.
### K.9 Resolution extras — M9 — gap G-M1
  - *Native failing baseline (2026-10-11):* Test-only commit `c5a948863` records 46 configurations (38 accepts, eight rejects), with 38 actual Native executions, exact selected physical inputs/declarations/inventory and rejected primary positions. The immutable 12,450-file replay passes format, three-crate Clippy, completeness and the live Native recorder; public verdict/output tests fail as expected. Evidence: `blueice-k92-baseline-third-proof.json`. Native goldens remain unchanged.
  - *Corrected focused replay (2026-10-11):* Format, warnings-denied three-crate Clippy, all 46 Native configurations and seven CLI regressions pass (`blueice-k92-mapping-third-status.json`). Separate modules implement path selection and type inclusion. Exact/wildcard/longest-prefix mappings, ordered fallback, baseUrl/rootDirs precedence and automatic/explicit type roots use bounded resolution. Inherited path origins, symlink escapes, changed library contents, empty types and authorized external declaration identities are covered. A frozen public CLI first reproduced the external-root identity panic; the shared identity mapping corrects it. The observer path fix changes no Native fixture. Complete K.0 and final-source hosted CI remain pending.
  - *Manifest identity and existing syntax regression (2026-10-11):* The first full K.0 run found the old assertion requiring rejection of now-supported `declare type`. It now checks a malformed alias and separately accepts the supported alias. An independent frozen public CLI also proved type-library manifest changes did not update the build fingerprint. Implicit type discovery now contributes the resolver's manifest/probe observations. Format, three-crate Clippy, all four Native tests, eight CLI controls and the exact existing regression pass (`blueice-k92-fingerprint-fourth-status.json`). Native goldens are unchanged. The failed first full gate remains recorded; the corrected full gate is pending.
  - *Complete K.0 (2026-10-11):* The immutable 12,454-file Linux K.0 gate passes format, warnings-denied three-crate all-target Clippy, all 1,142 ordinary tests and all 206 ignored oracles: 1,348 tests in 138 target/doctest groups per pass. All 12,312 backend/Cargo/workflow hashes match. All 46 Native configurations agree (38 accepts, eight rejects), with 38 actual emitted executions, exact selected physical inputs/declarations/inventory and rejected primaries/positions. All 200 K.9.2 Native fixture files remain unchanged; eight CLI guards and the existing syntax regression pass. Generated inventory: 213 refusal sites in eleven areas. Largest changed production source: bluetsc.rs, 976 lines. Evidence: `blueice-k92-full-k0-second-proof.json`. K.9.2 passes within its measured subset; broader mapping/library compositions and missing-library diagnostics remain G-M1 gaps.
  - *Hosted macOS oracle correction (2026-10-11):* K.9.1-source CI run `38086393838` failed only its package Native recorder on macOS because noncanonical temporary roots dropped imported declarations from selectedInputs. An isolated Linux symlink TMPDIR reproduces the failure in both package/path recorders. Both now canonicalize their temporary roots. All 54 package and 46 path configurations pass under default and aliased roots, with 714 fixture/support files unchanged (`k92-canonical-corrected-proof.json`). Rust production is unchanged by this recorder correction. The final affected-caller replay passes format, three-crate Clippy and all 17 tests (`blueice-k92-canonical-final-status.json`). Every backend/Cargo/workflow file matches the full K.0 source. Final-source hosted CI remains pending.
  - *Windows assertion correction (2026-10-11):* Hosted run `38090629141` on `d352031fd` found two slash-only selected-input assertions; the Windows output selects the correct files. Normalize both comparisons while preserving wrong-sibling rejection. Host format, isolated Linux warnings-denied Clippy and all eight boundary tests pass against the exact pushed compiler (`k92-windows-corrected-proof.json`), without changing production, Native goldens or the shared target. Final-source hosted verification remains pending.
### K.9 Resolution extras — M9 — gap G-M1
  - *Native failing baseline (2026-10-11):* Test-only commit `ea7728e7a` records 28 configurations (eighteen accepts, ten rejects), with eighteen actual Native executions, exact selected physical inputs/declarations/output inventories, JSON asset values and rejected primary positions. The immutable 12,588-file replay passes format, three-crate Clippy, completeness and the live recorder; public decision/output comparisons fail as expected (`blueice-k93-baseline-first-proof.json`). All Native fixture files remain unchanged.
  - *Corrected focused replay (2026-10-11):* Format, warnings-denied three-crate all-target Clippy, all 28 Native configurations and six CLI boundary/policy tests pass (`blueice-k93-source-eighth-status.json`). Normal and symlinked Native temporary roots also pass, with eighteen actual executions per variant. Owner-selected JavaScript loading retains canonical-root bounds and TypeScript sibling precedence; unchecked JavaScript publishes inferred surfaces while its TypeScript importer remains checked. Checked assignment/implicit-any diagnostics and JavaScript declarations match Native; source options change fingerprints. Existing typed JSON assets retain their observations. The full K.0 gate and final-source hosted verification remain pending. Package/path-mapped JavaScript, JSDoc types and broader source compositions remain G-M1 gaps.
  - *Existing CLI diagnostic regression (2026-10-11):* The first full gate found an old assertion requiring the generic unsupported-source message for disabled JSON imports. It now preserves BTS2000 rejection with the exact `resolveJsonModule` refusal. Format, three-crate Clippy, all eighteen existing CLI tests, four Native corpus tests and six guards pass (`blueice-k93-existing-cli-ninth-status.json`); no production or Native golden changes. The corrected complete gate remains pending.
### K.9 Resolution extras — M9 — gap G-M1
  - *Verified (2026-10-11):* The immutable 12,590-file Linux K.0 gate passes format, warnings-denied three-crate all-target Clippy, all 1,150 ordinary tests and all 208 ignored oracles: 1,358 tests in 140 groups per pass. All 12,446 backend/Cargo/workflow hashes match, including all 131 unchanged Native fixture files. All 28 Native configurations agree (eighteen accepts, ten rejects), including eighteen physical emitted executions, exact declarations/inputs/inventory/assets and rejected primaries/positions. The integrated `45d7bf5ed` correction and a matching diagnostic-message separator control change only two test comparisons after the full gate; every compiler/runtime/Cargo/workflow file remains identical. Final format, all-target Clippy and all 36 affected tests pass with the final 12,446 backend hashes verified. Evidence: `blueice-k93-full-k0-second-proof.json`, `blueice-k93-final-callers-status.json`, `blueice-k93-publish-source-proof.json`. Generated inventory: 213 refusal sites in eleven areas. Largest changed production source: package_resolution.rs, 1,164 lines. K.9.3 is complete within its measured subset; commit/push and proceed to K.10.1. Final-source hosted confirmation remains pending.
### K.10 JSX and decorator residuals — M9 — gap G-C3
  - *Native baseline and focused replay (2026-10-11):* Test-only commit `6cb16ad37` retains sixty configurations (26 accepts, 34 rejects), 26 actual Native executions, exact declarations/inputs/inventory, primary diagnostics and sixteen related records. The committed failing replay has 28 verdict differences. The corrected four-test replay agrees on every configuration and all 243 fixture hashes remain unchanged (`blueice-k101-managed-sixth-proof.json`). Nine direct Native/Node programs also pass. A further no-output argument guard exposes an unknown tag argument diagnosed as a props error; owned parser references now report TS2304. Corrected final caller format/Clippy and all eleven tests pass (`blueice-k101-final-focused-second-status.json`). The immutable 12,839-file Linux K.0 gate passes format, warnings-denied three-crate all-target Clippy, all 1,153 ordinary tests and all 210 ignored oracles: 1,363 tests in 141 groups per pass. All 12,693 backend/Cargo/workflow hashes and all 243 Native fixture files match their tested snapshots. Largest changed production source is parser.rs at 1,191 lines; inventory records 213 refusal sites in eleven areas. Evidence: `blueice-k101-full-k0-first-proof.json`. K.10.1 is complete within its measured subset; K.10.2/K.10.3 and exact final-source hosted confirmation stay open.
### K.10 JSX and decorator residuals — M9 — gap G-C3
  - *Native baseline and focused implementation (2026-10-11):* Test-only commit `7134ea6df` records 28 configurations (sixteen accepts, twelve rejects), sixteen actual Native executions, exact declarations/inputs/inventory, primary diagnostics and eight related records. All 509 fixture hashes remain unchanged. The exact preimplementation public compiler has fourteen verdict and eight rejected-primary differences. Corrected Native replay passes under ordinary and symlinked temporary roots; the committed failing replay passes format/Clippy and two tests, with two expected public failures. The third implementation passes all ten Native/owner/cache/budget/symlink tests and all 35 affected caller tests. The immutable 13,355-file Linux K.0 gate passes format, warnings-denied three-crate all-target Clippy, all 1,161 ordinary tests and all 212 ignored oracles: 1,373 tests in 143 groups per pass. All 13,207 backend/Cargo/workflow hashes and 509 Native fixture files match their tested snapshots. Largest changed production source is binding/namespaces.rs at 1,156 lines; inventory retains 213 refusal sites in eleven areas. Evidence: `K102_NATIVE_BASELINE.md`, `blueice-k102-native-baseline-proof.json`, `blueice-k102-namespace-third-status.json`, `blueice-k102-full-k0-first-proof.json`. K.10.2 is complete within its measured subset; K.10.3 and exact final-source hosted confirmation stay open.
### K.10 JSX and decorator residuals — M9 — gap G-C3
  - *Native baseline and focused replay (2026-10-11):* Test-only commit `08491aea3` retains sixty configurations (32 accepts, 28 rejects; 48 standard and twelve legacy), 32 actual executions, exact declarations/inputs/inventory and four related records in 243 unchanged Native files. The exact published compiler `26a7ce5a8` has 34 verdict and ten rejected-primary differences. Ordinary/symlinked-root Native recording reproduces all observations. The corrected fourth replay passes all four public Native tests plus the static-library test, format and warnings-denied three-crate Clippy. All 58 tests in the eight affected caller suites also pass, including their ignored oracles. The immutable 13,605-file Linux K.0 gate passes format, warnings-denied three-crate all-target Clippy, all 1,163 ordinary tests and all 214 ignored oracles: 1,377 tests in 144 groups per pass. All 13,455 backend/Cargo/workflow hashes and 243 Native fixture files match the tested snapshots. Largest changed production source is type_relations.rs at 1,124 lines; separate context, relation and diagnostic modules keep new responsibilities bounded. Generated inventory retains 213 refusal sites in eleven areas. Evidence: `K103_NATIVE_BASELINE.md`, `blueice-k103-native-baseline-proof.json`, `blueice-k103-diagnostics-fourth-proof.json`, `blueice-k103-full-k0-first-proof.json`. K.10.3 and K.5–K.10 are complete within their measured subsets. Exact final-source hosted workspace/platform/coverage confirmation passed in run 38106557070 at `87ebda8c7`; broader decorator compositions remain recorded gaps.
