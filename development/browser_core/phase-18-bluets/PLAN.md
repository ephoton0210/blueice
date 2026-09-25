# Phase 18 — BlueTS / BlueTSC: TypeScript Front End, Emitter, Type-Aware Debugging, and Runtime Contracts

[← Back to plan](../BROWSER_CORE_PLAN.md)

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

## Objective

Let a BlueIce page opt in to TypeScript source without a build-time `.js` artifact, while also providing BlueTSC for projects that need to compile TypeScript to portable JavaScript. Both paths preserve the reasons to author code in TypeScript: deterministic static diagnostics, source-level debugging, and—where data crosses a trust boundary—runtime validation of an explicit, reifiable contract.

## Standalone implementation and first BlueJS integration status

The direct bridge lowers parenthesized `new Identifier(args)` expressions with normal or spread arguments. Member constructors and omitted parentheses remain excluded.

The direct bridge lowers `delete` only for ordinary dot or bracket property references (without optional chaining). Identifier and non-reference operands remain outside the v1 subset.

The direct bridge lowers relational `in` and `instanceof` expressions. BlueTSC gives both a `boolean` result in the bounded checker; narrowing and custom-instance analysis remain future work.

Direct calls may use identifier, dot-member, or bracket-member callees with normal or spread arguments, preserving the BlueJS receiver. Optional calls remain excluded.

The current direct bridge subset supersedes earlier phase summaries: literals/identifiers, bounded quoted-string and template escapes, template slots containing supported direct expressions, direct calls/constructors with normal or spread arguments, supported unary/binary/logical/conditional/assignment/sequence expressions including relational `in`/`instanceof`, arrays with holes or spread elements (but not both in one literal), identifier/string/numeric/computed object keys with identifier shorthand and spread properties, dot/bracket reads, property assignments/updates/deletion. Nested templates, optional chaining, object methods, and accessors remain outside the subset.

The direct bridge accepts identifier, quoted-string, numeric, and computed object keys plus spread properties. A computed key lowers its supported direct expression to BlueJS `PropertyKey::Computed`; shorthand remains identifier-only, while methods and accessors remain excluded. BlueTSC merges an identifier-bound known record spread source into an inferred literal; computed keys, unknown/non-record spread sources, and general spread expressions remain `unknown` under the bounded static rule.

The direct bridge preserves non-spread array holes as BlueJS AST holes, rather than materializing `undefined` properties. It also lowers normal and spread array elements, but deliberately rejects any one literal that combines a hole and a spread element because BlueJS's spread construction path would otherwise materialize that hole. BlueTSC infers a known `T[]` spread source as `T` for a surrounding array literal; unknown/non-array sources retain the bounded checker's `unknown` element type.

The direct bridge lowers normal and spread call/constructor arguments. For calls to a known local function, BlueTSC expands only a tuple-typed spread into fixed parameter types; ordinary arrays, unknown values, and general iterable analysis remain outside that static rule.

Named local functions may use optional identifier parameters and a final identifier rest parameter. An optional parameter without an initializer lowers to an ordinary JavaScript parameter; within the function body BlueTSC treats its value as `T | undefined`. The direct bridge uses BlueJS's rest-parameter AST form; with an explicit `T[]` annotation, BlueTSC checks every normal or tuple-spread tail argument against `T` and infers generic `T` from the same tail. General array/iterable spread analysis remains outside the direct rule.

Named local functions may use a default parameter whose initializer is in the direct expression subset. BlueTSC retains the original initializer tokens, verifies a typed initializer against the parameter type, and the bridge lowers it directly to BlueJS `Param::default`; default expressions outside the direct subset remain excluded.

Named local function bodies retain ordered initialized local declarations, semicolon-terminated direct-expression statements, and `return` statements. The bridge lowers each structured expression statement to BlueJS in source order, so assignments and direct calls may have their normal local side effects. BlueTSC applies its existing bounded direct-call, property-access, and arithmetic checks to those statements; control flow, exception handlers, and every other unstructured body statement remain opaque and are rejected by the direct bridge.

Named local function bodies may also use `throw expression;`. BlueTSC retains and checks the direct expression, rejects an omitted value or a line terminator immediately after `throw`, and the bridge lowers it to BlueJS `Stmt::Throw` so the VM preserves the thrown JavaScript value. `try`/`catch`/`finally` and all other exception control flow remain outside the v1 direct subset.

Named local function bodies may use a direct-expression `if` condition with braced consequent, braced `else if` branches, and an optional braced `else` body. BlueTSC retains each branch structurally and the bridge emits BlueJS `Stmt::If` nodes with explicit blocks for braced bodies, while lowering an `else if` as a direct alternate rather than inventing an artificial block scope. The checker applies its existing bounded direct-expression checks to every condition and recursively to the branch bodies; it does not claim control-flow narrowing. Unbraced branches, an `else if` with an unbraced branch, loops, and every other control-flow form remain opaque and fail closed at the direct bridge.

For this structured function subset, an explicit return annotation that does not admit `undefined` must terminate every known path with a value `return` or `throw`. A bare `return;` is checked as `undefined`; `void`, `any`, `unknown`, and an annotation admitting `undefined` may fall through. Opaque control flow never supplies a termination proof and remains independently fail-closed at the direct bridge.

The direct bridge lowers template substitutions containing supported direct expressions to BlueJS expression slots. It tokenizes the original substitution with BlueTS's lexer and constructs BlueJS AST directly; it never calls a BlueJS source parser on BlueTSC output. Nested templates and embedded expressions outside the direct subset remain excluded.

Non-substituted template literals use the same ordinary escape decoder as quoted strings.

The direct bridge decodes ordinary quoted-string escapes (`\\`, quote, `\n`, `\r`, `\t`, `\b`, `\f`, `\v`, and `\0`) to BlueJS strings. Unicode, hexadecimal, legacy octal, and line-continuation escape forms remain deliberately unsupported in this bounded subset.

Record inference and direct lowering accept identifier-keyed object shorthand `{ key }` for a local binding. Direct lowering also accepts computed keys; because their names need not be statically known, the bounded record inference result is `unknown`. Object methods and accessors remain excluded.

The direct bridge lowers simple and compound assignments plus prefix/postfix updates to ordinary dot or bracket property references. The existing template/object/member-call boundaries remain in force.

The initial implementation completes the work that has no BlueJS dependency before page-runtime coupling. The first direct bridge remains host-neutral and deliberately narrower than BlueTSC's emitted-JavaScript matrix:

- `blueice-bluets` accepts only caller-supplied `ModuleLoader` records. The library itself does not read files, URLs, DOM state or host capabilities. `bluetsc` is the separate, project-root-confined filesystem adapter.
- Its pinned `blue-ts-0.1` matrix parses/binds typed variable and function declarations, named `export default function`, local-value `export default name`, and local named value-export declarations, interfaces, aliases, type-only imports/exports, primitive and literal types, records, arrays, tuples, unions, intersections and the corresponding erasable annotations/assertions. It resolves a closed relative module graph, produces stable diagnostics for parse/unsupported syntax, resolution, duplicate names, unknown types and the implemented assignment / return checks, and never emits on an error.
- `bluetsc check` uses that shared pipeline without writes. `bluetsc build` stages ESM `.js`, optional column-provenance source maps and public `.d.ts` files, then replaces the selected output directory only after every artifact has been staged. Its fingerprint includes source content, the pinned language version, target, source-map/declaration modes, runtime-policy label and resolver identity.
- `bluetsc` accepts either one explicit entry or a `bluetsc.json` project file. The latter supports multiple `entries`, project-root-confined `outDir`, `sourceMap`, `declaration`, `target`, `runtimePolicy`, and exact/prefix `imports` mappings. Config flags cannot be mixed with client-side overrides; an import target, entry or output path that escapes the declared root is rejected before compilation, including a syntactically local `outDir` whose existing ancestor is a symlink outside that root. An atomic output directory also cannot contain an input source module, so it cannot replace `src/`. This is the standalone compiler's closed-world resolver, not a page/network loader or an arbitrary package-manager hook.
- A project-root-confined relative import or exact `imports` mapping may target a local `.d.ts` declaration module. Declaration modules are parsed, checked, hashed and retained in VM-independent debug metadata, but are type-only: they never emit JavaScript or a runtime import-map target. BlueTSC rejects a `.d.ts` entry, a value import resolving to one, or runtime content within one. When declaration output is selected, BlueTSC preserves the authorized `.d.ts` source root-relatively and retains consuming `import type` clauses. Separately, a direct page host may supply an exact, already-verified generated `lib.blueice.d.ts` through `CompilerOptions::ambient_declaration_modules`; it is subject to the same source/module limits and build fingerprint, cannot import or re-export, and exposes static ambient declarations only. Direct-page admission also enables the fingerprinted `require_declared_global_calls` policy, which makes a top-level direct call fail with `UnknownName` unless it resolves to a local or verified ambient function. The standalone CLI cannot configure either host-only facility, and BlueTS never acquires ambient, remote, package-manager, or arbitrary `lib.dom.d.ts` declarations on its own.
- Every successful build stages a root-relative `bluetsc.manifest.json` with language version, project fingerprint, target, runtime policy, requested output modes and emitted entries. A configured `imports` map also emits `bluetsc.importmap.json`, translating source `.ts`/`.tsx` mappings to their generated `.js` locations. Both files are published with the artifacts, so they contain no absolute host path and cannot drift from an atomic build.
- The filesystem adapter supplies root-relative source identities to the shared compiler. Therefore artifacts, source maps and the VM-independent `BlueTsDebugInfo` do not expose checkout paths, and relocating an unchanged project does not change its resolver or source-identity contribution to the build fingerprint.
- The standalone compiler emits the VM-independent portion of `BlueTsDebugInfo`: compiler-minted source-content-hash identities, static types, symbols and spans, plus exact reifiable plans for successful non-generic local declarations only. It also contains a bounded, pure contract IR/validator for data-only JSON-like values. Imported, generic, erased or otherwise unreifiable declarations have no static contract handle; neither artifact claims a runtime type tag or validates a live page boundary.
- BlueTSC source maps use Source Map v3 segments at copied, rewritten and erased-source boundaries rather than line-only placeholders. Generated and original columns use UTF-16 code units; CRLF is represented as one source line transition. This gives portable JavaScript builds column-level TypeScript provenance without claiming a BlueJS bytecode safe-point map.
- `IncrementalCompiler` is a reusable, host-neutral single-entry session for development hosts. It reloads the caller-authorized graph to detect changed source or resolution edges, reuses parsed modules with identical bytes, and rebinds/rechecks only changed modules plus their reverse dependencies. It keeps only a successful cache entry and refuses reuse when the entry or any compiler option differs; its work-selection result is observable without exposing a BlueJS VM or page state.
- Generic aliases, interfaces and direct function calls retain their declarations' type parameters, including through local type-only imports. An interface may extend one or more named interfaces, including a generic instantiation; inherited fields participate in bounded structural checking, property lookup, declaration emission and reifiable contract conjunction. Its exported type-only surface carries inherited local declaration fields so an authorized consumer need not import private parent declarations. Direct local calls may either infer or explicitly supply their type arguments. They instantiate bounded structural checks, enforce `extends` constraints, and resolve trailing default type arguments (including in declaration modules). A declaration's type parameter never leaks into surrounding module scope. The checker additionally infers array literal element types, boolean comparisons, boolean-only `&&`/`||` chains, nullish coalescing after removing left-side `null`/`undefined`, unary `typeof`/`void`/boolean/numeric expressions, conditional branch joins, numeric `+`/`-`/`*`/`/`/`%`/`**` expressions, `<<`/`>>`/`>>>`/`&`/`^`/`|` expressions and known-string concatenation. It rejects numeric, exponentiation or bitwise/shift operators whose two operands are known incompatible primitives; it reports an unparenthesized unary exponent base as an ECMAScript early error; and it rejects a strict equality operator whose known `number`, `string` or `boolean` operands are disjoint. Unknown, `any`, union and structural operand rules remain outside this narrow initial rule; method/callback overload resolution and all other general expression inference remain pending.
- The parser rejects a `.tsx` module at its source-identity boundary, even if it has not yet reached a JSX tag. This prevents a TSX project from being treated as ordinary erasable TypeScript; tagged JSX is rejected by the same stable `UnsupportedSyntax` path. It also reports the ECMAScript early errors for an unparenthesized `??` mixed with `&&` or `||`, and a unary expression used directly as an exponentiation base, including in runtime spans that the bounded front end otherwise preserves for emission.
- Legacy CommonJS-oriented `import =` and `export =` forms are likewise rejected as `UnsupportedSyntax`, rather than being emitted as invalid ESM.
- For a direct call to a locally declared function, the checker verifies the accepted argument count and each annotated parameter after bounded generic substitution, whether type arguments are inferred or explicitly supplied. The same rule applies to semicolon-terminated direct-expression statements in a function body. Optional and default-initialized parameters are omittable while preserving a known return type. In a function body, a bare optional parameter has type `T | undefined`, while a default-initialized parameter has type `T`. Signature-only local overload declarations are resolved in declaration order for direct calls and erased from JavaScript; a non-declaration signature must have a compatible local implementation. This intentionally does not claim method calls, callback analysis, constructors, or general expression inference.
- Direct property access on an inferred record or a local/interface type alias is resolved to the declared field type (including a generic instantiation). Optional fields produce `T | undefined`; chained/member-call analysis and arbitrary JavaScript property semantics remain outside this static subset.
- The standalone `ContractPlan` validator accepts per-boundary `ValidationLimits` for depth, collection entries, visited-node fuel and string bytes. Core now applies it only to its two installed immutable host-to-script snapshots (`blueiceDocumentText` and `blueiceDocumentOrigin`) before realm callback capture; their inventory names stable binding IDs, contract IDs, direction, capability and fixed budgets. JSON/Fetch/XHR, URL/query, storage, messaging, foreign-module, extension, DOM-object, provenance, strict-runtime coverage and allocation-attribution enforcement remain deliberately separate work.
- `backend/bluets/tests/typescript_oracle.rs` is the BlueTSC compatibility-oracle job run on every push and pull request. Its test-only `BLUEICE_BLUETSC_ORACLE` environment variable names the pinned TypeScript 5.9.3 `tsc` executable; the all-caps spelling is environment-variable convention, while BlueTSC is the BlueIce compiler name. The job verifies that pin before executing, then runs a fixture matrix covering generic properties, constraints/defaults, explicit direct-call type arguments and generic interface heritage (including local `.d.ts` parents and rejected conflicting inherited fields), optional record fields and optional/default/explicit-`undefined` parameters, local constrained generic function overloads, ordered direct-expression function-body statements and rejected calls within them, function throws and invalid bare throws, braced function `if`/`else if`/`else` bodies and rejected direct calls in their conditions, ordinary and hole-bearing array literals plus array/object literals with spread elements/properties and computed object keys, direct-expression default and array-typed rest parameters plus direct calls/constructors with spread arguments, simple object literal property reads, templates with supported expression substitutions, generic arithmetic, exponentiation, numeric bitwise/shift, compound assignments, identifier/property updates and comma sequences, boolean comparisons and relational membership (`in`/`instanceof`), boolean logical chains, `??`, `typeof`/`void` expressions and conditional branch joins, named default-function, local-value default, and local named-value ESM exports, rejected assignment/call arguments including incompatible known primitive arithmetic, exponentiation or bitwise operands, disjoint strict primitive comparisons, `typeof` annotation mismatch, `??` annotation mismatch, unparenthesized `??`/logical mixing and unparenthesized unary exponent bases, Source Map v3 shape and accepted Node output. It uses ES2022 modules and a temporary `type: module` package boundary, so both BlueTSC and external `tsc` ESM artifacts execute under Node; the local default and named-value fixtures additionally compare each compiler's exact public `.d.ts` output. Rejected fixtures assert the exact BlueTS diagnostic count, stable code and source line, then require the pinned compiler to report the same count and source lines. Node and the external `tsc` are test tools only; neither is linked, spawned, or discovered by BlueTSC or BlueTS.
- `CompilerLimits` makes source bytes/tokens/type nesting, module count/edge count/import depth, aggregate source bytes, generic-expansion work and source-map segments explicit compiler policy. Every limit participates in cache and artifact fingerprints; adversarial unit tests require a stable `ResourceLimit` failure with no output.
- `blueice-bluejs` owns the public `BlueJsProgramV1` wrapper and its `bluejs-program-v1` identity. `backend/bluets-bluejs` is the sole crate that depends on both compilers. It consumes a checked, already-tokenized BlueTS source module or closed module graph and directly constructs the BlueJS AST before BlueJS compiles bytecode; it never gives BlueTSC-emitted JavaScript to the BlueJS parser. Its executable v1 subset is the authoritative current direct-bridge subset summarized above, which supersedes the historical phase-level lists in this document. It retains ECMAScript grammar boundaries by rejecting unparenthesized `??` mixed with `&&` or `||`, and an unparenthesized unary exponent base. ESM mode maps local named/default exports and runtime imports to BlueJS module entries. Every runtime request uses the exact canonical target retained by BlueTS's caller-authorized resolver; only runtime-reachable modules become BlueJS nodes, so type-only `.d.ts` inputs remain static. The parser represents initialized local declarations, direct-expression statements, throws, returns, braced `if` bodies, and braced `else if` chains in functions; it records every other body token as opaque so the bridge rejects rather than silently drops a statement it cannot lower. Re-exports and other runtime shapes not listed in the current subset fail explicitly until their direct lowering is implemented.
- The public [BlueTS ↔ BlueJS integration contract](INTEGRATION_CONTRACT.md) records the shipped structured-program bridge and its still-pending source-provenance, safe-point-map and generated host-typings requirements without adding a BlueJS dependency to BlueTS itself.
- `blueice_engine::script::direct_page::DirectPageScriptHost` binds the in-process direct bridge to a live core `TabManager`: a typed `TabId` resolves the current `Page`, its HTTP(S) URL is canonicalized to a realm origin, and a private replacement-document generation forces realm recreation even for same-origin navigation. A core owner may inject a validated `DirectPageRealmOwner` at construction, keeping VM, realm, program-count, bytecode, and static-debug retention limits outside every script request; an over-budget direct attachment leaves no retained program or debug record. Its `realm_stats` exposes only the owning tab's retained program count, bytecode charge, and VM heap statistics; navigation releases the old charge before a successor document can admit code. The optional `run_session_with_script_requests_and_direct_page_host` lifecycle seam synchronizes only previously admitted realms after each session batch. `DirectPageInlineExecutor` is the separate, explicitly configured lifecycle owner for inline declarations: it generates its selected profile itself, may receive that same validated realm owner, runs a document's opted-in inline declarations once in document order, and retains only bounded source-free reports. An over-budget inline attachment leaves no program or debug record and becomes a source-free rejection report. `run_session_with_script_requests_and_inline_page_executor` invokes that owner after each session batch. `blueice-core --inline-bluets-profile <known-profile>` may construct this executor with a core-selected profile and fixed default compiler policy; omitted means no executor. The source-free `GetBlueTsScriptReports` frontend query drains only the addressed tab's retained `(document generation, ordinal, classic/module kind, fixed outcome category)` records. It neither enables execution nor returns source text, compiler diagnostics, bytecode, or runtime values; a binary subprocess regression exercises a classic script, module script, and rejected typed call after real HTTP navigation. BlueJS now provides a realm-local, GC-safe primitive host-function ABI plus a page-runtime registrar that cannot execute bytecode or inspect the VM; registrations disappear with the realm on navigation. `DirectPageScriptHost` installs the checked-in `core-script-document-text-v1` profile and the composed `core-script-document-context-v1` profile only: the latter adds `blueiceDocumentOrigin(): string`, a copied canonical tuple origin with no path, query, fragment, URL object, DOM node, event, or mutation capability. Both values are rebound after replacement. This is not a general `document` object, automatic production script owner, or BlueJS process launcher.
- The real-process inline regression suite now also opens a second tab in the
  same opt-in `blueice-core` instance, navigates both tabs to independent
  script-bearing HTTP documents, and drains `GetBlueTsScriptReports` through
  each tab's addressed envelope. This proves report isolation at the process
  boundary: draining tab one reveals and removes only tab one's record, leaving
  tab two's record available only to tab two's query. It is evidence for the
  narrow report control plane, not a substitute for the outstanding general
  multi-tab page-host, resource-accounting, or debugger acceptance work.
- A separate binary regression replaces one tab's document through two real
  HTTP navigations and drains the report after each one. The report's distinct
  core-private document generations prove the configured executor observes the
  replacement and executes the new inline declaration once; it does not by
  itself prove every stale-handle/debugger invalidation requirement.
- The equivalent standard-JavaScript binary regression navigates one tab through
  two HTTP documents. Each document asserts that `blueiceDocumentOrigin()` is
  its current core-canonical tuple origin before its bounded report is emitted;
  the two successful, distinct-generation reports prove the executor replaces
  the prior realm and snapshot rather than retaining the first document's
  callback. This remains process evidence for the narrow immutable binding,
  not general DOM, debugger, or stale-handle coverage.
- A two-tab standard-JavaScript session regression drives independent HTTP
  documents through the tab lifecycle and drains each addressed
  `GetBlueJsScriptReports` envelope. Draining the first result neither reveals
  nor consumes the second, proving the source-free JavaScript observation queue
  remains tab-isolated at the core session boundary.
- Another binary regression serves a document whose copied text exceeds the
  document-text binding's fixed one-mebibyte contract budget. The normal HTTP
  navigation completes, but inline admission reports only the fixed source-free
  contract-rejection category before BlueTS/BlueJS program admission. This is
  process evidence for the two installed immutable result boundaries, not
  general strict-runtime contract, allocation, provenance, or foreign-data
  enforcement.
- `blueice_engine::debugger` is the core-side dispatcher for the versioned
  debugger IPC. `blueice-core --debugger-socket <path>` accepts a separate
  peer, performs its independent `Hello` negotiation, and forwards later
  requests through a bounded worker-to-session channel. `ListPageRealms`
  returns at most 128 loaded tab/document-generation identities, with no URL,
  source, program, bytecode, or runtime value; an over-cap list fails closed.
  With a selected live `--inline-bluejs` executor, the session additionally
  serves `ListPrograms`, bounded `ListSafePoints`, `ValidateSafePoint`, and v5
  `SetBreakpoint`/`ListBreakpoints`/`ClearBreakpoint` from the exact tab-owned
  BlueJS registry. These return or retain opaque core-minted program IDs and
  instruction-boundary tuples only; they reject wrong tab, realm, program
  generation, and non-boundary inputs instead of remapping them.
  `ProgramLocations` and the deliberately narrower
  `BreakpointConfiguration` capability are available only at that live seam;
  the latter is an idempotent bounded table cleared at realm replacement, not
  a general VM interruption mechanism. With either `--inline-bluejs` or the
  trusted out-of-process child route plus the debugger socket, v5 also offers
  `ArmRootSafePointBreakpoint`,
  `GetExecutionState`, and `ResumeExecution` for a pending classic
  declaration's exact root-code-unit boundary; `ArmEntryBreakpoint` remains
  the zero-offset compatibility form. A non-entry arm runs BlueJS to that
  exact compiler-verified boundary and retains the root interpreter frame
  (operand stack, bindings, scopes, handlers, iterators, completion state,
  and GC roots) until the owner-session scheduler resumes it. A successful
  `ResumeExecution` exposes only the source-free `Resuming` state for one
  bounded session turn before an idle scheduler turn resumes that frame; it is
  observability, not an execution lease. It exposes no source, bytecode,
  stack, scope, object, or completion value. Modules,
  top-level await, child function code units, re-arming/loop hits, stepping,
  arbitrary nested-frame interruption, exception, and runtime-value features
  remain planned. The real-process regressions verify the v4 `ArmEntry`
  compatibility route and the v5 `Hello`/discovery/`Pending`/non-entry-root-
  arm/`Paused`/`Resuming`/same-frame-completion route. The latter rejects
  cross-program and invalid-state resumes without consuming a pending
  declaration, proves child-code-unit, module, repeat-arm, and stale-realm
  targets fail closed, and checks that its debugger replies never reflect
  fixture source/completion data, VM values, or BlueJS opcode data.
- A launcher real-process regression combines `--out-of-process-bluejs` with
  `--debugger-socket` and uses only the public browser and debugger listeners.
  It navigates two local HTTP documents containing one classic declaration,
  discovers only opaque realm/program/safe-point records through the
  launcher-selected core/child route, then proves the first document's realm,
  program, and safe point all fail with `StaleRealm` after the HTTP reload.
  It does not know, configure, or contact the launcher's private child socket
  or capability.
- A parsed `Page` now discovers the explicit non-portable `application/x-blueice-typescript` and `application/x-blueice-typescript-module` declarations in document order, retaining inline source or an external `src` as data. It never grants loading authority or evaluates them: a future page loader must apply origin, feature-profile, integrity, resolver, and resource policy before assembling the `AuthorizedModuleLoader` for direct admission.
- As a deliberately bounded normal-page fixture seam, `DirectPageScriptHost::execute_inline` may admit one inline declaration only. It derives a canonical module ID from core tab/document-generation/declaration identities and creates a one-module closed loader, so it neither embeds caller text in an identity nor reads an external source. `DirectPageInlineExecutor` can invoke that seam automatically only when a core owner explicitly selects it. By default it rejects and reports external `src` without reflecting the page-controlled URL; `PageScriptSourceAuthorizer` is the sole optional core-owned authority that can turn that declaration into a supplied closed graph plus resolver fingerprint. The executor never fetches, resolves, or falls back itself. `HttpOutOfProcessPageScriptSourceAuthorizer` also implements that direct BlueTS-only authorizer interface, reusing its immutable origin rule, URL/SHA-256 manifest, response checks, static-graph parser, limits, cache key, and resolver fingerprint rather than maintaining a second HTTP policy. The direct executor still receives only a completed graph and redacts every authorization failure to its existing source-free report category. A real core-session/loopback-HTTP regression covers document navigation followed by the manifest-authorized external module; a cross-origin declaration is rejected before fetch without URL reflection. This is a trusted in-process core-construction seam only: the reference binary has no HTTP-authorizer CLI/profile or page/frontend/MCP configuration path. General deployment policy and remaining host bindings remain open.
- The out-of-process portion now has a deliberately narrow shared-realm
  bridge: `OutOfProcessJavaScriptPageExecutor`, selected only when core
  receives both a trusted child endpoint and its per-spawn capability token,
  inventories supported JavaScript and explicit BlueTS declarations under one
  DOM-order ordinal sequence, and turns each inline record into a core-minted
  closed one-module graph. The private v5 child protocol carries a trusted
  language tag; JavaScript follows the normal BlueJS parser while BlueTS uses
  only an `AuthorizedModuleLoader` derived from that graph and child-fixed
  checked options before `blueice-bluets-bluejs` directly attaches it to the
  same `BlueJsPageRuntime`. It redacts child outcomes into separate existing
  JavaScript/BlueTS report lanes and sends exact realm closes on navigation or
  tab removal. Its default constructor rejects external `src` without
  reflecting it. The separately named startup-only constructor accepts one
  immutable core-owned `OutOfProcessPageScriptSourceAuthorizer`; it receives
  the exact tab/document-generation/DOM ordinal/parser language-and-kind/raw
  URL/`src` tuple and may return only an existing typed closed JavaScript or
  BlueTS graph. Core checks the returned language, copies its exact canonical
  IDs, static edges, source bytes, and resolver fingerprint into
  `PageHostModuleGraph`, then rejects a malformed graph or authorizer failure
  source-free. The child revalidates the graph and never fetches, resolves a
  URL/import map, reads a filesystem, or gains the authorizer's cache,
  integrity, or network authority. A missing static edge has no fallback.
  `SpawnedBlueJsHost::spawn_for_core` preserves launcher supervision while
  handing that configuration to one trusted core. `blueice-launcher
  --out-of-process-bluejs` creates the child itself and retains it as part of
  the core generation lifecycle; cutover creates a fresh child/capability pair
  for the replacement core. Before realm replacement, core serializes exactly
  two validated primitive snapshots for ordinary JavaScript:
  `blueiceDocumentText()` (1 MiB) and `blueiceDocumentOrigin()` (4 KiB,
  canonical HTTP(S) tuple). The child repeats those limits and canonical
  spelling checks, installs no other binding, and destroys both copies on
  navigation or close. It receives no DOM object, URL, resolver, fetch/cache,
  IPC, or page-selected capability. `blueice-bluets-bluejs` owns the one fixed
  generated `core-script-document-context-v1` artifact shared by the core
  catalog and child: its byte-checked declaration and exact callback inventory
  make only `blueiceDocumentText(): string` and
  `blueiceDocumentOrigin(): string` ambient to child BlueTS. A missing, extra,
  renamed, or page-selected binding fails closed before compiler admission;
  `document`, `fetch`, URL, resolver, and object APIs remain untyped and
  unavailable. A real launcher-to-core-to-child regression observes both
  classic and ESM BlueTS report lanes in that shared realm and verifies a
  wrong callback arity becomes only the fixed source-free compilation
  category. This still is not a general DOM surface.
  `HttpOutOfProcessPageScriptSourceAuthorizer` now creates the closed graph
  itself under a fixed startup policy: canonical same-document origin or one
  canonical owner-selected origin, a URL/SHA-256 manifest, and fixed resource
  bounds. It accepts only a direct `200`, identity-encoded UTF-8 response with
  a language-approved MIME type and matching bounded `Content-Length`; it
  rejects redirects, missing manifest records, bad integrity, ambiguous/bare
  static URL forms, and over-depth/count/byte graphs. It parses manifest-
  covered JavaScript and BlueTS static edges, privately caches only verified
  bytes under deterministic URL/integrity/language keys, and derives a
  policy-complete resolver fingerprint. Core copies only the finished graph
  into the child protocol, while the child still has no fetch, URL-resolution,
  import-map, filesystem, cache, manifest, or fallback authority. The real
  core lifecycle can select exactly one compiled
  `core-page-http-fixture-v1` profile through a private launcher-to-core
  startup selector: it fixes one same-origin classic JavaScript path and its
  SHA-256 expectation, then constructs the regular HTTP authorizer inside
  core before any page executes. Neither the public launcher CLI nor that
  selector accepts URLs, manifests, resolvers, source, paths, or fetch
  settings; the per-document origin only instantiates the fixed same-origin
  policy. A subprocess regression crosses core, a real HTTP origin, and the
  supervised child to prove the finished graph handoff. This is a bounded
  integration fixture, not a general application-resource profile. The v8
  channel additionally carries source-free debugger-location operations (list
  retained programs, list a fixed bounded set of compiler-recorded safe
  points, and validate one exact tuple) and a 256-record exact-breakpoint
  configuration table. The child mints private IDs only; core verifies an
  exact live child realm, remints every public program handle/generation in a
  disjoint core namespace, and rejects mismatched tab/document/program/safe-
  point replies. Set/clear must echo the complete child-private safe-point
  tuple; core rejects duplicate or unmapped list records and revalidates each
  one before reminting a public record. Replacement and close discard both
  private and core mappings. The route deliberately does not proxy
  generic interruption, stepping, VM frames, stacks, scopes, values, bytecode,
  or source. A core-owned debugger socket selects the otherwise default-off
  document lifecycle, which defers document-order declarations for one turn
  and can arm a pending
  classic program at one exact root-code-unit point. It reports only
  `Pending`/`Paused`/`Resuming`/`Completed`; core revalidates the paused private
  tuple before reminting it publicly, and only a later core-owned advance turn
  resumes the same root frame. `ArmEntryBreakpoint`, modules, child code units,
  re-arms/loop hits, and nested interruption remain unavailable.
  Out-of-process discovery/configuration can consume at most 64 such deferred
  turns for a document, after which core advances it even if the peer keeps
  querying. DOM/event callbacks, URL or import-map resolution, broader
  per-client resource authorization, and page-selected compiler
  profiles remain open. The launcher owns an optional stable public debugger
  listener, but no debugger session or opaque handle survives a core cutover:
  each pre-cutover stream remains pinned to its old private core peer and
  closes fail-closed when that generation ends.
- [`bluets-test-interface`](TEST_INTERFACE.md) now exposes the same persistent JSON-lines ready/request/reply transport as BlueJS's test adapter. It is intentionally compile-only, accepts BlueJS's `sloppy` mode as a `raw` alias, and has stable BlueTS diagnostic codes/spans and caller-controlled compiler limits; Test262 runtime execution remains a future bridge concern rather than a hidden BlueJS dependency.
- The [BlueTS test report](TEST_REPORT.md) records the complete per-platform test-suite results, the TypeScript 5.9.3 oracle matrix and per-file line coverage for `blueice-bluets` and `blueice-bluets-bluejs` (2026-09-21).

This is deliberately not a claim of general `tsc` compatibility. Control-flow narrowing, overload resolution, decorators, enums, classes, TSX, namespace emission, parameter properties, and arbitrary JavaScript expression typing remain pending. A construct outside the implemented matrix must be added with a parser/checker/emitter test and a precise compatibility entry; it must not be advertised merely because its tokens happen to be erasable.

BlueTS is a Rust front end feeding the existing BlueJS bytecode compiler and VM. It is **not** a second JavaScript/TypeScript VM, an embedded `tsc` process, or a type-tagged replacement for BlueJS values. TypeScript's ordinary type system is compile-time only: the checker uses it to accept or reject a program, while BlueJS executes ordinary ECMAScript values. The [official TypeScript documentation](https://www.typescriptlang.org/docs/handbook/typescript-from-scratch) calls this "erased types" and states that types do not change JavaScript runtime behavior. BlueTS follows that compatibility rule for ordinary TypeScript, then adds an explicit BlueIce contract mode at foreign-data boundaries rather than silently making every local operation dynamically typed.

This phase depends on Phase 13 completing the BlueJS host, page-script pipeline and an end-to-end `<script>` fixture. It also depends on Phase 17's source-location debugger capability. It must not delay either prerequisite by pretending that a TypeScript parser can substitute for a correct ECMAScript engine or page-script host.

## Current-state correction

The current BlueJS parser accepts ECMAScript grammar, not TypeScript grammar. A colon annotation such as `const value: number = 1;` is a syntax error. The library example reads arbitrary source text and immediately calls BlueJS `parse`, so a `.ts` filename is not a TypeScript mode switch. The Phase 13 host/core wiring and real script-page test are also still open. BlueTS therefore begins after a working JavaScript path exists; it is not a way to claim current page-script support early.

References to TypeScript elsewhere in the plan refer to external automation clients or Node tooling, not page-language support. In particular, Phase 17's planned `@blueice/automation` client is TypeScript *client code* and must not be confused with executing TypeScript in a page.

## Decisions

### One front end, one runtime

Add a proposed `backend/bluets` Rust crate (`blueice-bluets`) with one shared TypeScript front end and lowering IR. Its in-process use in the long-lived BlueJS host permits a typed source program to lower directly to BlueJS's ECMAScript AST or bytecode input; it does not serialize generated JavaScript text and parse it a second time. The same crate also backs the standalone BlueTSC emitter. BlueTS has no network, filesystem, DOM, capability, or script-evaluation authority of its own. It receives source and host-resolved module records from the same page-script loader as BlueJS, consumes the same per-tab compile/memory budgets, and cannot bypass the script IPC, gatekeeper, origin, or module-resolution policies.

The execution path is deliberately narrow:

```text
TS/TS module source
  -> tokenize + parse -> bind -> type check -> shared lowered IR
  -> page path: BlueJS Program / bytecode -> BlueJS VM
  -> build path: BlueTSC ESM emitter -> .js + optional .js.map / declarations
  -> BlueTsDebugInfo + optional contract plans
```

The direct page path has no `TS -> emitted .js text -> parse .js again` round trip. BlueTSC is the deliberately separate build path that serializes the same lowered IR as JavaScript for use outside BlueIce. A test oracle may compare results with a pinned external TypeScript compiler, but Node.js and `tsc` remain test-only tools, never runtime dependencies.

### BlueTSC: standalone JavaScript emitter

`bluetsc` is the proposed BlueTSC command-line compiler. It invokes the same parser, binder, checker, resolver, lowering rules, contract planner and source-span machinery as BlueTS; it must not fork a second, subtly incompatible TypeScript implementation. It offers a build-time alternative for authors who want their project to run in ordinary JavaScript hosts, publish JavaScript packages, inspect emitted code, or move TypeScript checking out of page-navigation startup.

BlueTSC's default output is standard ECMAScript modules (`.js`) preserving ESM import/export semantics, accompanied on request by `.js.map` and `.d.ts` artifacts. Type-only imports/exports, interfaces and type aliases do not appear in emitted JavaScript. The emitter supports only an explicitly declared ECMAScript target matrix; it neither silently produces CommonJS nor claims that every TypeScript feature can target every JavaScript version. Resolution, import-map and declaration policies match BlueTS's declared project configuration, so a source graph cannot type-check one way in the browser and emit another way on the command line.

`bluetsc check` performs parse/bind/type/resolution validation without writing artifacts. `bluetsc build` type-checks first, lowers once, stages all output and publishes it atomically only if every selected entry and dependency succeeds. The default is equivalent to `noEmitOnError`: no stale or partial JavaScript output is described as a successful build. The initial implemented CLI accepts either `check|build <entry.ts>` or `check|build --config bluetsc.json`. A config has `entries`, optional project-root-confined `outDir`, `sourceMap`, `declaration`, `target`, `runtimePolicy`, and `imports`; a config invocation rejects extra CLI overrides so the recorded settings cannot drift. Its import map permits only exact file keys or trailing-slash prefixes to existing project-root-confined local source files/directories. A build always includes root-relative `bluetsc.manifest.json`; a configured map additionally produces the standard-shape `bluetsc.importmap.json` for the resulting ESM tree. This remains a deliberately small resolver, not Node/package-manager resolution.

BlueTSC's source maps map emitted JavaScript back to original TypeScript spans using the same provenance records that make BlueTS debugger locations correct. Declarations describe the checker-approved public surface, not a claim that an unimplemented BlueIce host API exists. JavaScript and declaration outputs carry a compiler/host-typing/configuration fingerprint; consuming an artifact built under a weaker contract policy must be observable and rejectable by a strict project policy.

For a `strict-runtime` project, BlueTSC lowers the same explicit boundary contracts as the direct BlueTS path. It either emits a deterministic, versioned contract helper/module with the bundle or rejects an output form unable to carry the required validators; it never strips the checks merely because the target is JavaScript. `checked` and `transpile-only` outputs are labelled in build metadata and DevTools/source-map extensions so that a deployment cannot be mistaken for strict-runtime output.

### Explicit BlueIce opt-in, not a claim of web compatibility

TypeScript is not a browser script language. BlueTS initially recognizes only these BlueIce-specific script kinds:

```html
<script type="application/x-blueice-typescript" src="app.ts"></script>
<script type="application/x-blueice-typescript-module" src="app.ts"></script>
```

The suffix alone never selects BlueTS, and `text/typescript`, TSX, or a JavaScript MIME type is not silently reinterpreted. The classic/module distinction follows the corresponding BlueJS realm and module rules; fetching, integrity, origin and policy checks remain the page loader's responsibility. These script kinds are deliberately non-portable: an author who needs ordinary-browser compatibility must precompile TypeScript to JavaScript.

The type-safety policy is core-owned configuration attached to the script request or a trusted application manifest. A fetched document cannot weaken it with an HTML data attribute. The initial product policy is `strict-runtime` for direct BlueTS pages; `checked` and `transpile-only` exist only as explicit operator/development choices and are visibly reported to DevTools and automation.

### Static semantics are required, not a cosmetic stripping pass

BlueTS's normal path implements a TypeScript-compatible parser, binder, module resolver and type checker for its declared compatibility version. It resolves source declarations, control-flow narrowing, generic instantiations, overloads, structural assignability and diagnostics before emitting executable bytecode. A parse, resolution, type, lowering, resource, or contract-planning error prevents that script/module from executing; BlueIce never falls back to treating rejected TypeScript as JavaScript or executes a partial output.

`transpile-only` is retained solely for controlled migration tests. It may erase only declared supported syntax, but it is not advertised as type-safe TypeScript support and cannot be the default direct-page mode.

The compatibility target is pinned by a `BlueTsLanguageVersion` and an explicit feature matrix. The first implementation need not promise bit-for-bit `tsc` compatibility. Each accepted syntax, checker rule, emit rule and diagnostic class must name its supported version and tests; unsupported syntax produces `UnsupportedSyntax`, not an accidental BlueJS parser error. BlueTS's runtime target is additionally bounded by the BlueJS ECMAScript feature matrix. A TypeScript construct may type-check yet be rejected as `UnsupportedRuntimeTarget` until BlueJS can faithfully execute its lowered ECMAScript semantics.

BlueTS owns a small `lib.blueice.d.ts` generated from the actual BlueIce DOM, event and network bindings. It must not claim the full web platform merely by importing a broad `lib.dom.d.ts`: declarations for APIs that BlueIce does not expose would make a successful type check dishonest. Browser URL modules and explicitly configured import maps are the initial resolution model. Node resolution, CommonJS, `node_modules`, arbitrary package-manager hooks and arbitrary remote `.d.ts` acquisition are out of scope.

### Runtime contracts protect data boundaries, not every instruction

Static checking proves properties only under the program's declared assumptions. Network JSON, URL/query data, storage, `postMessage`, extension/native IPC, foreign JavaScript module exports and user-provided values can violate those assumptions. In `strict-runtime` mode BlueTS requires an explicit, reifiable `RuntimeContract` for every type-bearing ingress and egress across those boundaries. A TypeScript assertion (`value as User`) never validates data; unchecked `any` is not an escape hatch in strict mode.

BlueTS may synthesize contract plans for a defined subset of types: primitives and literals; arrays, tuples and readonly collections; finite object records with required/optional fields; tagged unions; named recursive interfaces through a cycle-safe contract table; and generic declarations only when every runtime parameter has a supplied reifiable contract. Validation reports a bounded path, expected contract identifier, observed value category and source location without serializing sensitive input by default.

The following are not automatically reifiable: `any`, unconstrained or erased generic parameters, arbitrary conditional/mapped types, opaque/branded intersections, declaration-only ambient values, and types whose validation would require invoking an arbitrary getter, proxy, or user function. BlueTS rejects their automatic use at a strict boundary and requires a developer-supplied contract with a reviewable validator. Contracts must be pure, resource-bounded bytecode/data plans; they may not call arbitrary page code while inspecting untrusted data.

A developer may choose a registered JSON Schema as the data-plan representation for a JSON-like boundary contract. In that case BlueTS delegates compilation and validation to Phase 19's single `blueice-json-schema` engine rather than translating a schema into an ad-hoc second validator. The contract records the immutable `SchemaResourceId`, dialect, vocabulary set, registry generation and validation-plan hash in `BlueTsDebugInfo` and the BlueTSC build fingerprint. A `$schema`, `$id` or `$ref` present in page data never causes a network/file lookup or weakens the originating page's contract policy; only an operator-registered, pinned schema resource can be used. JSON Schema validates data shape at the boundary—it does not infer arbitrary TypeScript semantics, execute format/content handlers, or make a static `TypeId` into a runtime tag.

The common pattern is consequently explicit and honest:

```ts
interface User { id: string; displayName: string }

const user = await fetchJson("/api/me", User.contract);
// `user` is `User` only after the response has validated.
```

After a value passes its boundary contract, local code relies on the static checker and carries no universal runtime type tag. This concentrates dynamic cost where reality introduces untrusted values instead of checking every addition, property read, or function-local assignment. Contracts provide data-integrity checks, not a sandbox: a malicious script that already executes in a page realm is still constrained by BlueIce's origin, capability and gatekeeper policies.

### Type-aware debugger metadata is a required compiler product

Type erasure at the VM boundary does not justify discarding compiler knowledge. Each successful BlueTS compilation emits immutable `BlueTsDebugInfo` keyed by source identity, source-content hash, language version, compiler-option hash, host-typing ABI, contract-policy version and BlueJS bytecode ABI. It contains:

- original TypeScript source spans and a bytecode-safe-point map;
- module URLs, inline-script provenance and source-content hashes;
- `SymbolId` records for declarations, references, scopes and renames;
- interned `TypeId` records for declared and inferred expression types, generic substitutions and diagnostic messages;
- source-level scope membership and lowered-variable correspondence;
- contract IDs, their boundary sites, and validation-failure locations.

For a JSON-Schema-backed contract, the last group additionally links the pinned schema resource, dialect/vocabulary report and standard validation output locations. Phase 12 exposes that data as a bounded contract/schema artifact so an AI and a human diagnose the exact same failed pointer/keyword without serializing or executing the rejected value.

Phase 17's debugger maps breakpoints, stepping, stack frames and exceptions back to TypeScript locations. A scope/watch display shows the runtime BlueJS value separately from its static TypeScript type; it must never claim that a static `TypeId` is a runtime proof. Watch/evaluate expressions are parsed and checked in the paused source scope before BlueJS compilation, with the same fuel, controller-lease and audit requirements as ordinary debugger evaluation. A generic type's displayed instantiation is the checker result at that source site, not invented runtime reification.

For a BlueTS script, source maps are therefore a release prerequisite rather than a deferred DevTools nicety. Debug metadata is retained according to the script/debugger memory policy, released with its tab realm, and redacted from unprivileged automation clients just as source text and scopes are. The same metadata backs the target-aware AI MCP debugger: [Phase 12's debug-environment contract](../phase-12-mcp-server/DEBUG_ENVIRONMENT.md) exposes it only through generation-bound, paged, capability-scoped source/type/symbol/contract resources and tools.

### Performance and cache contract

Type checking is real work; BlueTS does not hide or repeat it unnecessarily. A source/module compilation cache stores bytecode, debug metadata, contract plans and dependency fingerprints under the complete key above plus source bytes. A stale dependency, declaration, import-map, option, policy, host-typing or bytecode-ABI change invalidates the dependent entry. Content-addressed output is immutable; a cache hit never reuses bytecode checked under a weaker policy.

Development mode maintains an incremental module graph and rebinds/rechecks only affected strongly connected components. Production may precompile/cache a verified graph before navigation. A type check is never performed in a VM hot loop. Contract plans are compiled once and applied only at declared crossings; validator work has explicit recursion, byte, collection-length and fuel limits. Phase 8 attributes parser, checker, metadata, cache and validator allocations to the initiating tab.

BlueTS cannot make a bytecode interpreter as fast as ahead-of-time native code. BlueJS interpreter dispatch, DOM IPC, allocation and host calls remain independent performance work. BlueTS's performance objective is instead to avoid duplicate parsing, avoid type work in hot execution paths, keep contracts boundary-local, and make cold compilation/cache costs observable in DevTools.

### Safety, analysis and resource behavior

The Phase 7 gatekeeper receives both the source-level BlueTS capability summary and the post-lowering BlueJS capability summary. Approval is based on the executable lowered behavior; source-level types, declarations and contracts add explanation but cannot hide a network call or DOM mutation introduced by a lowering transform. The summaries are linked by source span and transformed-node identity so an audit can identify their origin.

All tokenizer/parser/checker/contract operations enforce source-size, token, AST-depth, type-instantiation, union/intersection expansion, module-graph, diagnostic, compile-time and metadata budgets. Exceeding one produces a defined resource failure without executing the script, preserving the existing page/document state. BlueTS never resolves imports through arbitrary filesystem access or a checker-owned network request. It accepts only loader-supplied, origin/policy-approved module records.

## Compatibility levels

| Level | Static checker | Runtime contracts | Intended use | Promise |
| --- | --- | --- | --- | --- |
| `transpile-only` | No | Explicit contracts only | Controlled migration and compiler tests | Syntax conversion only; never advertised as type-safe. |
| `checked` | Required | Explicit opt-in contracts | Compatibility/performance measurement under operator control | Type errors prevent execution, but foreign values may remain unchecked. |
| `strict-runtime` | Required | Required at all declared foreign-data boundaries | Default direct BlueTS page mode | Static diagnostics plus validated ingress/egress for the supported contract subset. |

No level treats `as`, `!`, `any`, an unchecked cast, or a `.ts` extension as proof that a runtime value is safe. A page's active level and any unvalidated/unsupported boundary must be visible in its DevTools target metadata.

## Initial language boundary

The first executable slice targets type-bearing but eraseable syntax needed for ordinary typed page code: annotations, interfaces, type aliases, generics, union/intersection/literal types, narrowing, overload declarations, `readonly`, access modifiers, `as`, `satisfies`, non-null assertions, `declare`, `import type`/`export type`, named `export default function` declarations, `export default name` for a local runtime value, and local `export { name as publicName }` bindings. It emits only ECMAScript constructs currently supported by the selected BlueJS target.

The first slice explicitly rejects TSX/JSX, decorators, generic arrow functions, default-export expressions other than a local bare identifier and anonymous default functions, named value re-exports from another module, `enum`/`const enum`, runtime `namespace`/`module`, parameter properties, `import =`, `export =`, declaration merging with runtime emission, and custom transformers. Some are not erasable; for example, ordinary TypeScript enums emit runtime objects. They can be proposed later only with a precise lowering specification, BlueJS runtime support, debugger mappings, capability-summary coverage and differential tests. TSX additionally requires an explicit JSX factory/runtime and is a separate UI-framework decision.

## Delivery order and acceptance

### Slice 0 — JavaScript and debugger prerequisites

1. Complete Phase 13's BlueJS host/core wiring, DOM binding design and a real script-bearing page fixture.
2. Complete Phase 17's script source/safe-point debugger path and the source-map representation required by BlueTS.
3. Connect the shipped resolver-preserving `BlueJsProgramV1` script/module graph hand-off to an origin-preserving page AST/IR, with compiler/cache resource accounting and safe-point validation.
4. Generate `lib.blueice.d.ts` from implemented host bindings using the published host-typing version policy.

Acceptance: a JavaScript fixture executes through the page loader and debugger with a stable source location, while an unimplemented host API is absent from `lib.blueice.d.ts` and fails an attempted typed use.

### Slice 1 — checked TypeScript source and source-level debugging

1. Build the TypeScript tokenizer/parser, binder, URL-module resolver and checker for the initial boundary.
2. Extend direct lowering of supported typed syntax to BlueJS AST/bytecode input, without JavaScript-text round trips.
3. Reject type/resolution/lowering errors atomically and expose structured diagnostics through the script, debugger and automation APIs.
4. Emit `BlueTsDebugInfo`; prove TS source breakpoints, stepping, stack traces, scopes, symbol navigation and static-type displays map to the executing bytecode and to the Phase 12 AI MCP debug interface.

Acceptance: a local typed classic script and module execute with no generated `.js` file; a deliberate type error runs neither the entry nor dependent module; a breakpoint and exception point at original `.ts` lines; a debugger shows a static type and the separate runtime value.

### Slice 2 — BlueTSC JavaScript emission

1. Build `bluetsc check` and atomic `bluetsc build` on the shared checked/lowered IR.
2. Emit standard ESM JavaScript, source maps and the supported declaration output without reparsing or rechecking a different source representation.
3. Define reproducible output/cache fingerprints and ensure source/contract policies, host typings and resolver results cannot drift from direct BlueTS execution.
4. Make strict-runtime output retain or import its deterministic contract helper; reject targets that would silently remove required validation.

Acceptance: a checked local TS module produces deterministic ESM JavaScript and a source map pointing to original `.ts` lines; an error leaves the previous output intact and reports no successful artifact; the resulting JavaScript has the same supported observable behavior as direct BlueTS execution; a strict boundary check still fails malformed input after emission.

### Slice 3 — strict runtime contracts

1. Define the pure contract IR, its reifiable type subset, bounded validation engine and failure schema.
2. Add contracts for JSON/Fetch/XHR once Phase 17 supplies those APIs, then URL/query, storage, `postMessage`, foreign module and native/extension boundaries as each exists.
3. Make `strict-runtime` require coverage of each supported boundary; reject `any`/unreifiable types there unless a declared reviewed contract is supplied.
4. Attribute validator/cache/debug allocations to the tab and feed lowered contract behavior to the gatekeeper summary.

Acceptance: malformed JSON cannot enter a `User`-typed value; the validation error names the bounded failing path and source location; valid data reaches BlueJS without repeated local type checks; a recursive/deep/oversized input fails within the resource budget.

### Slice 4 — incremental projects and compatibility growth

1. Implement dependency-aware incremental checker/cache invalidation and trusted local declaration files. Both, plus external-oracle coverage and bounded erasable generic substitution, are complete in the standalone front end.
2. Expand the supported TypeScript feature matrix only alongside its BlueJS lowering/runtime, source mapping, contract and oracle evidence.
3. Compare the supported fixture matrix against a pinned TypeScript compiler for accepted syntax/diagnostics and add Node/BlueJS behavioral differential tests for the lowered output.
4. Expose carefully redacted project/type metadata to Phase 17 DevTools and the documented, capability-scoped Phase 12 MCP debug interface.

Acceptance: editing one module invalidates only its dependents; a cache entry checked under another policy is refused; every supported compatibility feature has parser/checker/lowering/debug/contract expectations and an independently checked fixture.

## Testing strategy

- Keep public parser, binding, checker, lowering, contract and debugger fixtures under the eventual `backend/bluets/tests/` boundary, with shared HTML page fixtures beside the Phase 13/17 integration tests.
- Test diagnostics by stable code/category, source span and semantic condition—not copied compiler-message wording. Every unsupported feature must have an explicit rejection fixture.
- Compare accepted syntax and checker behavior against the pinned TypeScript 5.9.3 reference in the CI BlueTSC compatibility-oracle job (`BLUEICE_BLUETSC_ORACLE=/absolute/path/to/tsc cargo test -p blueice-bluets --test typescript_oracle -- --ignored`); compare resulting runtime behavior against the reference JavaScript output where the selected BlueJS feature subset supports it. The reference never determines BlueTS's security policy or makes an unsupported test silently pass.
- Run BlueTSC's emitted JavaScript and direct BlueTS bytecode against the same deterministic fixtures; compare public results, exceptions, module order, source-map locations, contract failures and type-only import elision. Assert an erroneous multi-entry build publishes no partial artifact set.
- Test contracts with malformed, adversarial, recursive, cyclic, getter/proxy-like, deep, oversized and resource-exhausting inputs. Assert no arbitrary user code runs during a pure validation path.
- Test debugger round trips: source breakpoints, async/exception locations, renamed symbols, type displays, erased type-only imports, transformed spans, stale source-map rejection and privacy redaction.
- Test cache keys/invalidation, per-tab attribution, failed compilation atomicity, page/gatekeeper policy enforcement and multi-tab isolation through real processes once the host exists.

## Explicit non-goals for the first release

- Claiming full `tsc`/`tsserver` compatibility, all TypeScript syntax, all diagnostics, or all JavaScript package ecosystems.
- Using BlueTSC as an opaque wrapper around a bundled `tsc`, or permitting its emitter to drift from BlueTS's parser/checker/lowering rules.
- Replacing BlueJS with a TypeScript VM, preserving universal runtime type tags, or adding a type check to every local operation.
- Treating TypeScript types or contracts as a security sandbox, permission system, or substitute for Phase 7 review.
- Executing TSX/React, decorators, enums, runtime namespaces, CommonJS, Node builtins, arbitrary `node_modules`, arbitrary remote declarations, or custom compiler transformers.
- Advertising complete web-platform typings before the corresponding BlueIce host APIs and their runtime behavior exist.
- Exposing source text, static types, diagnostics, contracts or debugger scopes to an unauthenticated/unprivileged automation client.

## Checklist

- [x] Decide that BlueTS is a BlueJS front end, not a second VM or a `tsc` runtime process
- [x] Decide direct-AST/IR lowering, no emitted-JavaScript reparse path
- [x] Decide that BlueTSC is a shared-front-end standalone compiler emitting standard JavaScript rather than a second TypeScript implementation
- [x] Decide atomic no-emit-on-error builds, ESM-first output, source-map/declaration artifacts and strict-contract-preserving JS emission
- [x] Decide explicit non-standard BlueIce script kinds and a core-owned policy
- [x] Decide that full static semantics and TypeScript debugger metadata are required for supported direct pages
- [x] Decide `strict-runtime` boundary contracts as the default direct-page safety policy
- [x] Define the initial reifiable-contract boundary and explicit non-reifiable failures
- [x] Define cache keys, resource accounting, debugger metadata and gatekeeper visibility requirements
- [ ] Complete Phase 13 page-script host and Phase 17 source-map prerequisites
- [x] Expose one exact classic-root instruction step through debugger v26 on the in-process page route, retaining the BlueJS continuation and reporting only opaque safe points; source-level mapping remains Phase 13/17 work
- [x] Carry the exact classic-root step through page-host v27 and the supervised child route, including retained BlueTS classic metadata and a public source-free debugger transition; nested-frame and source-level stepping remain Phase 13/17 work
- [x] Add page-host v31 core-only child-wide actual-usage snapshots with checked live-realm aggregation and owner-envelope validation; keep conservative reservations and RSS/fleet limits distinct.
- [x] Expose a bounded classic-root BlueTS source-span step through debugger v30 and metadata manifest v4 with independent owner/client grants, same-stream receipts, core reminting, a distinct limit stop reason, and real launcher debugger-socket acceptance; modules and nested-frame stepping remain open.
- [x] Add the page-host v28 private exact BlueTS safe-point-to-original-byte-span lookup under a live child metadata handle, with no nearest-position fallback or public debugger grant; public source-level breakpoint/stack routing and capability policy remain open.
- [x] Bind the private v28 lookup to core-reminted program and metadata handles, require an exact compiler source ID, and reject mismatched child tuples/ranges before the public debugger adapter can use it; session receipts and owner/client grants remain open.
- [x] Expose debugger v27 `OpaqueSafePointSpan` only with independent owner/client grants, same-stream opaque metadata/source receipts, exact live safe-point validation, and a bounded source-text-free original BlueTS byte range; source-level breakpoint/stack execution and cache/hibernation invalidation remain open.
- [x] Add page-host v29 and debugger v28's separately default-denied bounded BlueTS source-byte-position binding: retain unbound lowering spans beside the live metadata, require exact program/metadata/source receipts and owner/client grants, remint and revalidate bound safe points in core, and return explicit unbound results without source text or automatic execution. General non-root/module interruption and stack/scope mapping remain open.
- [x] Create the standalone `blueice-bluets` crate and pin the initial `blue-ts-0.1` compatibility matrix
- [x] Publish the versioned BlueJS AST/IR hand-off, bytecode-safe-point-map and host-typing compatibility contract
- [x] Implement the first public BlueJS structured-program hand-off for bounded host-neutral classic-script and resolver-preserving ESM-module-graph subsets, without emitted-source reparsing
- [x] Generate and test `lib.blueice.d.ts` from actual host bindings using the published host-typing strategy
- [x] Implement the independent initial parser/binder/closed-module resolver/checker/type-erasure ESM emitter with atomic compile failures
- [x] Implement `bluetsc check`/staged `build`, ESM/column-provenance-source-map/declaration emission and reproducible artifact fingerprints for the initial matrix
- [x] Implement VM-independent `BlueTsDebugInfo` (source hashes, symbols, static types and spans); direct and isolated child page admission retain it only against an exact live BlueJS generation. Compiler/MCP provenance now uses a labeled SHA-256 digest, so the source-text-free compiler identity is collision-resistant; it remains a query-only metadata record, not source-read authority. Page-host v10 can enumerate a separately minted opaque metadata handle for an exact live BlueTS program, describe it with a bounded fingerprint/count summary, list only compiler-minted source-record IDs parent-bound to that handle, and—only under a distinct source-provenance request—describe one prior ID with canonical module identity plus the labeled SHA-256 digest. Debugger v9 defaults to a denied negotiated manifest; an owner must separately enable `OpaqueInventory`, dependent `OpaqueSummary`, dependent `OpaqueSourceInventory`, and/or `OpaqueSourceProvenance`, and a client must request the exact canonical set for the live realm. The public summary is handle/generation-bound and carries only language/options fingerprints plus source/type/symbol/contract counts; source inventory carries only numeric source IDs under that same handle; provenance is source-text-free metadata only. No debugger surface has source text, spans, names, type/symbol/contract records, bytecode, or values. Broader static-record disclosure, diagnostics, bytecode source mapping, and controlled debugger read exposure remain pending.
- [x] Expose source-free, generation-bound BlueTSC diagnostics through compiler IPC v9 and MCP with optional original-source zero-based UTF-16 coordinates, derived only from valid byte spans in the immutable authorized graph; unavailable or invalid positions are omitted. This does not grant source reads, arbitrary offset mapping, build, or output writes.
- [x] Validate core diagnostic evidence again at the MCP boundary before publishing check generations or diagnostic pages: exact project/generation, ordered byte ranges, plausible optional coordinates, and usable one-shot continuation shape are required; a new check revokes old local receipts even if its reply is lost or malformed.
- [ ] Expose TypeScript diagnostics, symbols, types, contracts, lowering provenance and BlueTSC check/build through Phase 12's negotiated MCP debug interface. A core startup owner can now seal fixed closed projects before opening the query-only compiler listener and explicitly attach MCP's bounded v2 client; exact static source-hash provenance plus reifiable-local contract read/validation and exact-generation paged opaque-ID discovery are shipped. Phase 12 authorization, catalog distribution, lowering/bytecode provenance, broader capability/session negotiation, and output-write elevation remain absent.
- [x] Implement the pure runtime-contract IR and bounded JSON-like validator; the only installed page host result boundaries now have an explicit inventory and pre-admission primitive-string validation, while broader host-boundary discovery, JSON Schema delegation and page enforcement remain pending
- [x] Implement host-neutral dependency-aware incremental parser/checker cache invalidation; cache reuse is refused across compiler-policy changes and failed compilations preserve the last successful entry
- [x] Support root-confined, type-only local `.d.ts` modules without runtime emission or package/remote declaration acquisition
- [x] Bound the pure contract validator's depth, collection, node-fuel and string-byte work with caller-visible limits
- [x] Add an opt-in, TypeScript-5.9.3-pinned external-oracle job for the implemented initial matrix
- [ ] Add real-process page, debugger, contract, resource, policy and multi-tab regression coverage
