# BlueJS Coverage Batch Analysis

## Current verified completion

The latest complete measurement is `20261006-183231-de935772`, source fingerprint
`a791618afcfab936be3ea79a37ef40dfcc5d86c9d1d56416341b9e9243372b95`.
All 22 selected files are complete: nine D5 and thirteen D4 files. All 50
cumulative modified production files reach 100% raw LLVM lines, functions and
regions. All 95 originally complete files are preserved. Coverage percentage
regressions and semantic outcome changes are both zero. Overall, 131/162
instrumented production files are complete; the remaining 31 files are outside
the completed selected and modified acceptance set.

The complete verification passed 4,099 Rust cases across all 334 targets,
all 102,921 applicable Test262 modes, 2,170 other workspace cases, two Rustdoc
tests and 73 self-test tooling contracts. Four Rust and 60 workspace ignored
cases retain their existing identities. The semantic audit compares all
102,926 scheduled modes and finds zero changes. Test262 took 441.025 seconds;
the complete workflow took 2,386.816 seconds. All 88 task Rust files pass
formatting; workspace Clippy denies warnings. The 20 unchanged workspace
formatting differences retain their baseline source hashes.

The final correction's graph selected only two agent callback cases in two
owning Rust targets. They passed in 0.029 seconds of case execution before the
single fresh complete verification. Their partial profiles provide diagnostic
witnesses and graph relationships only. Every completion counter above comes
from the subsequent full run's own binaries, source texts and atomic LLVM
profiles. No failed or partial profiles, older maps or complementary-instantiation
union contributes to completion.

### Verified TailCall eval-result boundary

The former two-byte strict eval fixture refuses at its twenty-byte UTF-16
`'use strict'` Constant before the TailCall result check. The dedicated fixture
retains a real 128-byte global string, verifies the compiler emits an
eval-candidate TailCall, and proves every string loaded by a Constant fits
the 32-byte limit. The eval result reaches the actual final refusal. Cleanup,
success after restoring the limit and VM reuse pass under normal and one-object
nurseries. The earlier two-byte test remains unchanged. The full interpreter
file reaches 1,517/1,517 lines, 61/61 functions and 3,360/3,360 regions.

### Verified agent callback preservation boundary

The fixture registers a real host control and invokes the public broadcast and
receiveBroadcast natives through compiled JavaScript. The callback retains its
valid shared wrapper and throws an existing globally retained object. Exact
thrown identity, stack cleanup, broadcast queue consumption, wrapper prototype
and length, the next successful broadcast, repeated shutdown and VM reuse all
pass under normal and one-object nurseries. Both native instantiations
independently hit the callback-error region twice before the full run. The
complete measurement restores the agent host to 488/488 lines, 50/50 functions
and 684/684 regions without changing its production code.

### Verified fixture graph selection

Dedicated ordinary-library entries avoid unrelated execution sweeps. Fixture
selection follows actual function references, including intermediate helpers,
to the public verification roots. Changed public test cases select native names
when shared imports, constants and helpers are identical. Shared-input edits
and removed cases select the whole owning target. The 73 tooling contracts
validate this behavior. Unknown boundaries still use the broader recorded graph.
Passing affected partitions enable one complete run for the same frozen source
and exact test selection. The UI retains live progress, selection reasons, logs,
measured edges and the latest complete coverage results.

### Producer and error-boundary review

TypedArray construction still performs fallible brand, bounds, mutability and
internal-length validation immediately after calling the user constructor.
No JavaScript or allocation intervenes before the later copy-strategy shape
reads. Ordinary objects, detached views, short views and immutable buffers
remain catchable refusals at the first boundary. A foreign constructor returning
an imported parent immutable view additionally exercises the local result path.

Both buffer mirror producers preserve byte length, maximum length and
resizability. A fixed buffer cannot change its internal length. A resizable
mirror either reaches the source length through a fallible resize or returns
that refusal before copying. Detached parent mirrors are skipped; detached
foreign sources detach mirrors; immutable foreign sources return separately.
The final equality branch therefore cannot reject a valid mirror after the
preceding successful resize. The byte write retains its validated range.

A published global binding owns its ordinary cell root. Dynamic eval environments
retain their private ordinary cells. These direct reads cannot invoke a getter,
allocate, or reject their validated handle; an absent value still produces its
ReferenceError. ResolveWithReference materializes the permanent realm global
before publishing a global or unresolvable reference, so subsequent cached
lookups cannot enter a cold initializer. Property reprobes, setter calls, cell
writes and real allocation refusals remain fallible.

The compiled-eval fixture creates cells through the actual declaration ingress,
runs previously compiled with code in current and captured dynamic environments,
and suspends at a compiled GetValue instruction. Its Reference pair comes from
the interpreter. Deletion and writes exercise the real environment helpers;
resumption and cleanup use the actual interpreter error. No bytecode or invalid
heap state is manufactured for this correction batch.

The rest fixture retains its next-result objects and payloads in global roots
before imposing the budget. Collection cannot reclaim a temporary result to
fund the later array_push. Strict functions select the compiler's actual
IteratorClose self-recursion and TailCall direct-eval paths. Missing async throw
fixtures separately exercise close getter/call failure and Promise-constructor
failure when starting the close await, followed by a large native error in finally.

### Verification plan

All source and fixture changes precede runtime verification. Formatting, diff
checking and workspace Clippy run before freezing the batch. The correction
graph selects Reference cases, TypedArray/buffer matrices, both native fixture
instantiations and related Test262 directories. Imports or shared data changes
invalidate narrow contracts. Only after those partitions pass for the same
source and selection does one complete Rust, Test262, workspace and LLVM run
establish the updated canonical report. Failed or partial profiles do not enter
completion counters. All 22 selected and all 50 modified production files must
reach 100% raw lines, functions and regions, with all 95 original complete files
preserved and no semantic or percentage regression to complete the goal.
Each verified round that adds complete files without regressions may be committed
and pushed under the standing authorization, then work continues on its gaps.

Qualified global property deletion now invalidates the property-backed cell
at the shared object Delete boundary; DeleteBinding delegates there. Captured
references reach replacement accessors. Public cases cover qualified and
unqualified deletion, Reflect, Proxy forwarding, minimal nurseries, retained
non-configurable properties and independent lexical bindings. The original
setter failure assertion passes unchanged in unit and ordinary-library builds.
The graph includes this deletion contract and reads independent native harness
lists concurrently within the configured worker limit.

The earlier reviews below retain implementation rationale for their source
snapshots; they are not evidence for this pending correction batch.

## Retained earlier review rationale

## R24 completed static gates and prepared source freeze (2026-10-05)

Whole-workspace Clippy passes all targets with warnings denied. The collected
six fixture diagnostics now use initialized configurations, local constructor
function aliases and borrowed one-value slices. All 77 task Rust files pass
formatting and the diff whitespace check. No runtime tests were executed while
preparing these edits. The next independent snapshot runs all 334 targets,
then complete Test262 and fresh atomic raw LLVM export; all 22 selected and
every modified production file must pass the same completion criterion.

## Prepared quiescent-agent preservation fixture (2026-10-05)

The R17 audit retained a raw percentage regression at the untouched agent
loop's idle scheduler sleep. A public host fixture now starts an agent with
one Promise report job, consumes its completed report, allows the drained
agent to remain quiescent, and checks successful repeated shutdown and VM
reuse. It is prepared before the next source freeze and has not run yet.
The production agent file is unchanged. Fresh raw counters must confirm
preservation; the fixture alone is not a coverage completion claim.

## R23 static preflight correction (2026-10-05)

All 17 critical targets passed. The full runtime verification was cancelled
after workspace Clippy found two required source changes; 226 completed target
logs retain 3,217 passes and no failures. Alignment uses is_multiple_of with
the typed-array enum's nonzero byte width. Generator throw resumption no longer
immediately invokes a redundant closure after its metadata queries became
infallible; cleanup still follows the returned resume result. Workspace static
checking now precedes the next frozen runtime batch. No cancelled-epoch
profiles are used to declare coverage complete.

## R22 full-suite review and R23 complete edit batch (2026-10-05)

All 334 targets completed with 4,041 passes, one fixture failure and four
existing ignored tests. The 16 critical targets passed. The Page runtime
fixture now preserves canonical identity reservations after attempted linking
and verifies actual pre-execution discard/reinstall separately. It joins the
next critical cohort before the remaining Rust suite and Test262.

RegExp's only production allocator installs a non-configurable own lastIndex
data property before returning its matcher. The builtin exec path reads this
own slot directly; observable ToLength and its errors remain. Script coverage
checks prototype traps, object coercion, thrown coercion, no-match reset and
GC pressure. Zip padding now retains a real iterator result in the realm:
without that owner, GC can reclaim the temporary result's value bytes and fund
the metadata write. The matrix observes next() returning while the metadata
remains undefined on actual heap refusal, then checks success and VM reuse.
All R23 edits precede its fresh frozen verification; R22 failed profiles are
retained for diagnosis and excluded from completion evidence.

## R21 private-element rooting correction (2026-10-05)

The 16 critical targets retained 1,388 passes and two failures for one concrete
static private-method GC defect. R20 failures passed; remaining targets,
Test262 and coverage export were stopped at this gate. `set_closure_home`
protects its operands during metadata allocation, but the following static
brand allocation protects the owner without the not-yet-published function.
The interpreter now roots both popped operands through the entire installation
sequence and restores the operand-stack length for success and refusal. Public
static method/getter/setter and lexical super fixtures run with normal and
collection-stressed configurations, beside the existing adaptive heap-refusal
matrix. All R22 edits are prepared before its independent verification.

## R20 critical-gate correction review (2026-10-05)

The complete 16-target gate retained 1,369 passes and 21 failures across three
targets. R20 did not run the remaining 318 targets, Test262, or coverage export.
All revealed failure categories were reviewed before preparing R21:

- Closure home installation calls `ensure_closure_metadata`, which can charge
  managed heap bytes. All three modified call sites again propagate refusal;
  the existing undefined-method bytecode boundary remains supported. Getter
  and lexical arrow allocation matrices complement private and async cases.
- Suspended dependencies need not own a namespace. Settlement builds one
  before removing waiters and restores the graph on refusal. Namespace
  construction roots are released after permanent cache publication.
- Native constructor fixtures now supply an actual Object new target; the
  lazy Object prototype fixture uses `hasOwnProperty`; descriptor Proxy keys
  correspond to an actual target property after prevention of extensions.
- Buffer transport owns parent and child roots. The GC fixture now verifies
  those real owners and bidirectional byte synchronization. No synthetic
  imported-record retirement or dangling object is used.
- Debugger fixtures step from the supported first evaluate instruction to
  the compiler-emitted Halt; module bytecode has no terminal Return here.
- Internal-slot ingress expects the existing RuntimeError TypeError mapping.
- The namespace resource matrix uses acyclic dependencies: a cyclic leaf
  cannot fulfill its import before its whole component finishes evaluation.

R17 remains the last complete Test262 and raw LLVM measurement. R21 uses a
new frozen source snapshot, fresh binaries, and independent atomic profiles.

## Scope and verification policy

The current batch covers the nine D5 files first, then the thirteen D4 files.
All implementation changes and regression fixtures must be prepared before any
build, test execution, or coverage collection. Existing measurements are used
for analysis only. This document does not claim that pending changes pass.

The baseline is the retained measurement in
`target/test262-macos-20261001-arguments-restoration/public-push/`.
The previously complete `vm/builtins/arguments.rs` is restored to 100% raw
line, function, and region coverage. All original 95 complete files remain
complete. The selected 22 files are still incomplete at this baseline.

Completion requires 100% of raw LLVM line, function, and region counters.
Source-location union counts cannot replace this criterion: a unit-test
instantiation can be covered while the ordinary library instantiation remains
uncovered. Tests must exercise valid public behavior or valid internal boundary
states. Production sources cannot be excluded or moved solely to improve a
percentage. Out-of-line unit fixtures retain their tests and follow repository
organization; any resulting denominator change must be reported separately.

## Findings and planned regression boundaries

### R19 build-only result (2026-10-05)

R19 cleared both privacy errors and stopped before test execution on one
remaining closure lifetime inference error, reported by both library builds.
The script compiler is now an explicitly typed function accepting any borrowed
source. The retained import program and all runtime assertions remain. R20 will
build the complete frozen batch; no R18 or R19 coverage measurement exists.

### R18 build-only findings

R18 stopped before test execution with two fixture privacy errors and one
inferred temporary-string lifetime error, each reported by both library builds.
The collected-source mirror fixture is moved to its owning private module.
The declaration accessor query is a pure read of the permanently rooted global
validated by both callers; it now returns a Boolean. Actual declaration ingress
checks remain fallible. The cyclic import bytecode is retained before its matrix.
The scope fixture reads last_module_namespace rather than treating body completion
as a namespace. All corrections form one batch before a new R19 source freeze.

### R18 prepared changes and boundary analysis

Preparation is complete for the next frozen batch on 2026-10-05; measurements
remain R17 until that batch executes. No build or test was run during this
preparation. Explicit formatting covers all 76 task Rust files.

- **Execution:** Global binding producers never use the named-function-expression
  silent immutable assignment case. Eval cell deletion is a retained-cell query.
  CloneScope is emitted after lexical for initializers and retains their cells;
  its fresh-cell allocation and value-growth stores remain fallible. Thenable
  jobs retain the object and the callable then used to enqueue them. Actual
  finalization jobs with primitive holdings are collected before draining.
- **Module cleanup:** A `?` inside post-await settlement previously bypassed
  graph restoration. A contained result closure now completes before publishing
  the graph. Real cyclic awaits and uninitialized `then` exports exercise
  direct and parent settlement errors. Scope cleanup uses actual linked export
  cells; terminal debugger pauses exercise fresh namespace heap/root refusal.
- **Intrinsics and object definitions:** Fallible root registration replaces
  assertions in lazy builders, including Math. Cache rollback and second-root
  cleanup are checked. Literal and private method functions, preconverted field
  keys, and initialized owner bindings are checked against compiler emission.
  Nested Proxy invariant checks retain target exceptions and descriptor record
  allocation errors.
- **Generators:** Staged delegation tests introduce limits after the generator
  has yielded. They cover completion payload growth, awaited return replacement,
  pending requests, close-method failures, and raw language-error construction.
  Status-only writes preserve the checked control's accounted size. Queue and
  generator-frame value growth retain allocation errors.
- **Foreign heaps:** Mirror synchronization is a pure copy over validated live
  records. R18 initially assumed weak ownership; the R20/R21 producer review
  above supersedes that assumption: imported-value records retain both heaps,
  and the collection/flush fixture checks those owners. Refresh remains fallible
  when a resizable mirror grows. Clone, transport, immutable/shared backing,
  destination prototype, source/destination root, error materialization, and
  re-export cases use actual child VMs. Live third-heap ingress remains fallible.
  No fake object identifiers are used.
- **Intl:** Accepted Temporal kinds are checked before conversion. Duplicate
  style no-overlap rejection is removed after the common visibility rejection.
  Numeric and Temporal ranges share one provider error mapping, retaining actual
  invalid-date/provider failures. All conversion and parts methods are retained.
- **Remaining gates:** Freeze the source and prepared patch, build all 334 targets
  and both tools, run the 16 critical targets before the remaining suite and pinned
  Test262, export fresh raw counters, audit all 102,926 mode contracts, preserve
  the original 95 complete files, and require 100% lines/functions/regions for
  all selected and modified production files before final workspace gates.

### R17 measured gaps and next analysis batch

The complete frozen R17 run passed all 334 Rust targets (3,996 passes, zero
failures, four existing ignored) and all 102,921 applicable Test262 modes.
All 102,926 mode contracts are unchanged. Fresh atomic counters have no
unsigned underflow findings. All original 95 complete files remain complete.
Raw completion is 108/162 production files, 8/22 selected files, and 22/43
modified production files. Twenty-one modified files remain incomplete;
completion and final workspace gates are still pending. The untouched agent
scheduling path also has a raw percentage regression to investigate.

The fresh `source-gaps.json` locates every remaining source boundary.
`uncovered-instantiations.json` retains individual covered/uncovered function
instances. Page runtime, debugger, and vm.rs have no missing union locations
but still have raw gaps: physical source-location maxima do not close their
unit/ordinary or generic-instance counters. These require targeted invocation
of the missing instance boundaries, with source and binary ownership verified.
Other gaps include real allocation/root/continuation errors, callback/import
failures, module/deferred cleanup, and private producer guarantees. All missing
regions must be classified and addressed together before another build or test.
Unused error closures must be checked against their producer contracts;
resource or user-code failures must remain observable and get regression tests.

### R17 object callback boundary and delegated native algorithms

The complete R16 critical gate recorded 1,344 passes and one failure in
280.489 seconds. The retained host API assertion and all 96 callback checks
passed. The unchanged `iterator_wrapper_next_rejects_a_non_wrapper_foreign_receiver`
assertion failed: a local native forwarding receiver access to a foreign VM
acquired that VM's TypeError instead of its own Realm's. Remaining targets and
Test262 were not launched; no full coverage was exported.

A VM is not always the Realm of a native algorithm executing there. The existing
`test262_foreign_next` intentionally imports its delegated operation's raw
error so the local callee selects the error Realm. Applying a blanket conversion
in enter_call defeats that distinction. The general call boundary is restored.
A dedicated `call_object_callback` instead handles all 14 accessor and Proxy
trap invocations inside object internal methods. A different suspended Realm
caller causes the callback result's language error to become a value in its
own VM. Local calls preserve raw RuntimeError results; uncatchable errors remain
uncatchable. Noncallable traps and post-call invariants stay outside this helper.

Fixtures retain the unchanged host and Iterator wrapper assertions. The public
callback matrix covers 96 error-Realm cases. A 54-case matrix covers three
callee Realms, three receiver Realms, absent/noncallable methods, and next/return
on non-wrappers. Boundary fixtures check registry restoration, instruction
refusal, and allocation refusal while constructing getter/setter errors through
a real reverse membrane. Unit and ordinary-library paths both exercise these
contracts. All edits precede the next consolidated build and measurement.

### R16 local API preservation and shared call boundary

The complete R15 critical gate recorded 1,342 passes and two failures in
285.253 seconds. Both are the retained host accessor API assertion for
`node.textContent = {}`, executed in unit and ordinary-library builds. The
cross-Realm Iterator, RegExp, and 48-case accessor error matrix passed. Remaining
Rust targets and Test262 were not run; no full coverage was exported.

Unconditionally materializing accessor errors also turned a local host API's
RuntimeError::TypeError into RuntimeError::Thrown. The original assertion is
retained. Accessor-specific conversion is removed. `enter_call` already
materializes local callback exceptions when a foreign native acts locally for
another Realm. It now also does so when the existing ACTIVE stack contains a
suspended caller with a different heap tag. The registry query only reads tags
and never dereferences a VM pointer. Function-body errors become values before
returning to that caller; local host calls retain their raw error representation.

The same function boundary handles accessors and Proxy traps. Proxy invariants
and noncallable-trap errors raised outside a function's body keep their existing
internal-method propagation. Public regressions expand to 96 cases covering
four language error types, three owner Realms, two callees, accessors, and
Proxy traps. Required-write checks add noncallable and invariant-violating traps.
An ordinary/unit boundary fixture checks empty, own-only, mixed nested, and
restored registrations, error heap ownership, and local API preservation.
Existing instruction/allocation refusal and reuse checks remain. Every edit
precedes the next frozen consolidated build and measurement.

### R15 accessor exception analysis and prepared correction

R14 built all 334 harnesses and two binaries. Its full 16-target critical gate
recorded 1,340 passes and two failures in 284.692 seconds. No remaining Rust
harnesses, Test262 modes, or full coverage export were run. Diagnostic profiles
remain separate from measurement profiles.

The frozen adapter localized the Iterator failure to `home:constructor`:
`cannot assign Iterator.prototype constructor` was a child TypeError instead
of the parent TypeError. The earlier readonly, Proxy, trap identity, creation,
and third-Realm refusal checks passed. Reverse Boolean Set enters the parent's
OrdinarySet, which calls the parent's native home setter. That call returned
an unmaterialized RuntimeError::TypeError. Reverse error export only transports
Thrown values, so the child constructed a new error in its own Realm.

Getter and setter calls now convert their language errors to thrown values in
the function's VM before their surrounding object internal method returns.
Forward/reverse callable membranes already materialize remote call errors.
Existing thrown values pass through; error_value returns uncatchable resource
errors unchanged. Internal-method refusal and Proxy invariant errors are not
blindly materialized in an object's owning Realm. This keeps the Boolean false
result's TypeError in the native algorithm's Realm while retaining an accessor
call's own exception Realm.

The RegExp throwing and success fixtures omitted `flags: 'g'`; the global match
algorithm did not reach their required lastIndex writes. Both fixtures now
provide explicit global flags. A public 48-case matrix covers parent/child/third
accessor owners, parent/child RegExp callees, getter/setter invocation, and four
language error types. Boundary fixtures retain instruction refusal, allocation
refusal while creating native errors, cleanup, successful reuse, and ordinary
library execution. Every edit precedes the next consolidated frozen build.

### R14 required-write and membrane analysis

The R13 build passed all 334 harnesses and two binaries; its 16-target critical
gate recorded 1,340 passes and one failure. Local Proxy setter protocol passed.
The added foreign setter failed: `set_property_value` dispatches a reverse
facade to `test262_reverse_set`, which invokes `parent.set_property`. Forcing
the child VM's strict flag therefore leaves the parent in sloppy mode and
discards `[[Set]]`'s false result. The forward implicit setter has the same
separation. Mapping a parent-generated error afterwards would also choose
the wrong Realm for a native refusal.

Required native writes now use a shared `object_set_or_throw` helper. It retains
the receiver/value, calls `ordinary_set_with_receiver`, and consumes its Boolean
result in the callee VM. Both membrane directions already provide Boolean
Set-with-receiver methods. A false result becomes the callee's TypeError;
errors actually thrown by setters/traps keep their original identity/Realm.
Iterator setters and RegExp's validated-object required writes use this helper.
It does not change either VM's ambient strictness.

Boolean writes can also target a global data property backed by a binding
cell. Ordinary assignment previously synchronized that cell above this
boundary; Reflect.set and membrane writes need the same synchronization after
their successful data-property definition. The shared Boolean implementation
now updates the matching property-backed cell. Prototype/accessor/Proxy and
namespace branches preserve their own results. Allocation sweeps cover growth
of the global property and its cell, including a foreign native setter.

Prepared public regressions cover parent/child/third-Realm receivers and callee
errors, refusal, callback identity, prototype home rejection, successful writes,
global aliases, and RegExp match/replace required writes. All edits and fixtures
precede the next consolidated frozen build.

### R13 Proxy setter correction

The R12 frozen build passed all 334 harnesses and two tool binaries. All R11
failing checks passed. The full 16-target critical gate recorded 1,339 passes
and one failure: the added public Proxy setter case. The remaining 318 Rust
targets and Test262 were not launched; no full coverage was exported.

Iterator setters still used `Heap::get_own_property_descriptor` to choose
between Set and property creation, and `define_data` to create the property.
Both helpers operate on ordinary heap storage and bypass Proxy and membrane
internal methods. A Proxy target's existing own property was therefore reported
absent and its `set` trap was never called. Creation likewise ignored a
`defineProperty` refusal. Both setters now use `object_get_own_property` and
`object_define_own_property`, throwing if creation returns false. Native write
refusal still forces Set's required strictness and restores it afterwards.
The public matrix adds descriptor/definition throws and refusal, exact
creation flags/order, and foreign setter/local Proxy error Realm and identity.
No tests execute during this correction's preparation.

### R12 failure analysis and prepared corrections

The reviewed R11 build passed all 334 Rust harnesses and two tool binaries.
Its 16-target critical gate recorded 1,333 passes and five failures across
three harnesses; the ordinary-library driver duplicates two failures. The
remaining 318 harnesses and Test262 were not run, and full coverage was not
exported. All three distinct defects were analyzed before the R12 batch.

- Iterator's SetterThatIgnoresPrototypeProperties requires `Set(this, p, v,
  true)`. The implementation inherited caller strictness and silently accepted
  a refused write in sloppy code. Both setters now force the required write
  and restore prior strictness on either success or error. Public regressions
  cover readonly data, getter-only accessors, Proxy refusal, callback throws,
  existing setters, property creation, and both calling modes.
- Diagnostic output for the foreign binary regression was
  `[2,13,42,13,42,0,"13,42"]`: only the local atomic load lost its update.
  Transport cloned a local TypedArray's elements into a separate buffer.
  Atomics arithmetic now chooses the first argument's owning VM. Foreign
  `call` is unwrapped and foreign Atomics `apply` observes its argument list
  locally once. Number/BigInt regressions cover all nine arithmetic operations
  and local storage, native error Realm, coercion throws, and child-owned arrays.
- The existing backtrace profile's allocation-helper function count is one:
  the first delegation scenario failed during its 204th headroom attempt.
  The stack identifies GC during a resumed Yield frame store. A generator's
  function can be owned only by its active `Vm::callee`, since generator
  resumption has no ordinary `call_stack` entry. A nested closure displaced
  that field into a Rust local without rooting it. Ordinary closure calls,
  generator initialization, and generator resumption now retain the caller
  function before swapping fields. Iterator records kept only for abrupt
  cleanup also need roots after IteratorStep/IteratorStepReference removes
  their operand. An interpreter wrapper releases its record suffix on every
  exit. Existing allocation sweeps remain, with nested-generator/destructuring
  cases added. Panic diagnostics now retain source and headroom.

No diagnostic-only profile establishes a completion percentage. R12 changes
and fixtures are prepared together before the next frozen consolidated build.

### R10 measured result and R11 analysis

The reviewed R10 snapshot passed all 334 Rust targets (3,963 checks, zero
failures, four existing ignored tests) and all 102,921 applicable Test262 modes.
The known inventory classifications remain four exclusions and one stale
fixture; the runner's strict exit is consequently still 1. Fresh counters have
no unsigned underflow. All original 95 complete files are preserved; 105 of
162 production files and six of 22 selected files meet the raw three-metric
criterion. Twenty-four modified production files still have raw gaps.

R11 uses those retained measurements for analysis without executing tests.
The source-location union is diagnostic only. In particular, page-runtime
iterator specializations and the host-name predicate have covered source
locations while raw file counters remain incomplete. Inspect each ordinary
library specialization before adding coverage. Shared graph validation can
consume a borrowed iterator through one implementation, preserving validation
order without allocating an intermediate collection.

Fresh native promises remain strongly rooted in `Vm.promises`. Rejecting such
a registered promise is a nonallocating state change with a settled status;
it cannot take the missing-record or pending-status error paths. Promise
resolution, user capabilities, descriptor TDZ, and growing continuation stores
remain fallible. Iterator setter callbacks have already installed their base
prototype, but direct helper boundaries can still receive a foreign heap
handle or a cold realm; their contracts must be checked individually.

Additional analysis covers actual getter/trap failures, typed-array coercion
and detachment, deferred module observation, async queue completion, error
headers, Temporal formatting overlap, and resource refusal during retained
continuations. All production edits and corresponding regression fixtures
will precede the next frozen build and consolidated verification.

### R11 prepared implementation details

- Initialize Function prototypes before allocating unpublished collection,
  WeakRef, and FinalizationRegistry prototypes. Cold bootstrap can collect;
  the old ordering left the new prototype outside the VM's root stack.
- Identify a selected default prototype through rooted intrinsic caches when
  a foreign `newTarget.prototype` is primitive. Avoid eagerly constructing
  unrelated built-ins; retain fallback identity for ShadowRealm, Intl, and
  Temporal families as well as existing constructor families.
- Use one borrowed-iterator implementation for page graph validation. Preserve
  the existing point at which each public iterator is consumed.
- Initialize private zip count/mode metadata before opening iterators. Updating
  the existing numeric count cannot grow managed bytes. Padding and iterator
  getters remain fallible and retain abrupt-close behavior.
- Reject registered private promises through a proven nonallocating transition;
  use initialized own IteratorResult data for the queued private yield result.
  Promise resolution, capabilities, and growing async completions remain fallible.
- Restrict the Intl formatting helper to the six supported Temporal kinds.
  Duration and ZonedDateTime still return TypeError at the input boundary.
  Validated options, immutable intrinsic data properties, rooted buffer metadata,
  and nonallocating normalized byte writes use producer assertions.
- Retain module lookup errors at ingress and host graph traversal errors. Later
  graph retrievals without intervening JavaScript use the established ownership
  guarantee. Disposal reads its compiler-owned synthetic finalizer; it retains
  normal completion and pending throw/return distinctions.
- Add cold/warm adaptive intrinsic and dynamic-function allocation sweeps, both
  parent and child transport sweeps, Proxy import root budgets, RegExp loop fuel
  and prefix/replacement/tail string limits, compiled reference errors, binary
  detachment/content-type errors, namespace TDZ through Proxy traps, actual
  continuation serial exhaustion, and async-generator payload/finally cases.

R11 static review and formatting passed. The consolidated build started only
after freezing 557 runtime source hashes. The initial build rejected three fixture
visibility/argument errors without running tests. They were corrected together
and a separate reviewed snapshot was frozen. Its instrumented build passed all
334 Rust harnesses and two tool binaries; the prepared and built runtime hashes
match exactly. Its critical Rust gate failed as recorded in the R12 analysis
above. Collect fresh profiles for the corrected frozen batch without merging
R10 or R11 profiles, then compare all original inventory contracts and production
counters.

### R10 consolidated corrections

All R9 diagnostic profiles were exported before editing this batch. They remain
Rust-only evidence from a run with eight failures, not a completion measurement.
The next frozen verification includes all seven failed Rust targets in its early
gate, then all remaining targets and standalone Test262 before a fresh raw audit.

- Propagate uninitialized namespace-export descriptors and immutable-buffer
  detach errors. Validate foreign ArrayBuffer and SharedArrayBuffer species
  separately before copying or consulting brand-specific metadata.
- Select a repeated direct eval's existing local dynamic cell and mutable,
  deletable metadata while preserving active lexical/catch bindings. Record
  reused dynamic captures for local declaration recreation after deletion.
  Keep the existing object-free async environment path scoped as before.
- Exercise source re-export identity, each root-registration boundary, every
  allocating namespace construction step, GC after rollback, and clean retry.
- Cover template root refusal without publication and rejected async-generator
  return/delegate transitions, including the second await when return is absent.
- Check debugger uncaught throws before/after a child pause, skipped root targets,
  unresolved top-level await, graph validation, retained child ownership, module
  resume allocation refusals, and exhaustion of actual await/frame serials.
- Installation stamps every immutable child code unit with the root generation.
  Nested entry validation rejects tail-call/direct-eval code before capture;
  module child suspension captures its caller and stores its graph together.
  Replace redundant error fallbacks for those producer guarantees with assertions.
- Private iterator records own ordinary initialized fields. Existing Boolean
  replacement does not grow managed bytes; user iterator getters and calls still
  propagate errors. Generator frame writes that can grow retain their fallible
  rooted paths. Opaque host-object capabilities remain reachable through permanent
  non-configurable realm-global properties after their heap identity is checked.
- Add cold host-entry refusal and Intl style/width, legacy trap, coercion, and
  result-allocation boundaries. Ordinary date parts no longer accept an unused
  range-source argument; the separate range-parts builder retains source fields.
- Temporal regressions cover out-of-range destinations, whole-week overflow,
  bracket overflow, and calendar-field narrowing. A bounded date duration with
  one carried day cannot overflow i64; other range/rounding failures remain checked.

- **Page runtime:** distinguish unknown realms, stale programs, unavailable
  frames, wrong generations, and successful completion. Existing inline unit
  failure arms are test code; retain those fixtures under `tests/fixtures/`.
- **Binary data:** preserve coercion order, species behavior, detachment,
  resize, foreign views, and allocation failure. Restrict the internal atomic
  arithmetic operation type to the five operations that actually reach it.
- **Closures and debugger:** cover frame-serial exhaustion, invalid pause
  targets, cancellation during module execution, abrupt child completion,
  scope lifetime, and execution after cleanup.
- **Generators:** cover all delegated next/throw/return outcomes, missing and
  noncallable methods, primitive results, throwing getters, thenable rejection,
  suspended-start/completed requests, and queued requests after rejection.
- **Execution and interpreter:** cover eval binding lifetime, completion roots,
  iterator cleanup, private brands, lexical errors, and valid compiler boundary
  checks. Do not manufacture corrupted heap objects.
- **ShadowRealm:** cover wrapper name/length descriptors, reentrant calls,
  import failure, opaque errors, and primitive-only boundary transport.
- **Test262 membrane:** every ordinary property-forwarding transport installs
  a reverse facade before publication. The old post-call copy-back loop has
  no producer and is obsolete. Verify immediate writes, callbacks, abrupt
  completion identity, and buffer lifetime across both transport directions.
- **D4 files:** cover async collection close/reject order; object descriptor
  and proxy traps; module namespace/deferred evaluation lifetimes; RegExp
  coercion and legacy compile dispatch; Temporal range, rounding, and calendar
  boundaries. Keep allocation failures distinct from proven immutable metadata
  invariants.

## Prepared implementation batch (not yet verified)

The implementation and regression fixtures were prepared before invoking a
build or test. The batch includes:

- Reserve nested debugger frame serials before replacing caller state.
- Release the source target root when ShadowRealm wrapper construction fails.
- Roll back namespace placeholders, nested namespace caches, and temporary roots
  when a cyclic namespace transaction fails.
- Preserve suspended generator frames when a stale delegation completion has
  no active delegate; share async delegate completion cleanup.
- Restrict atomic arithmetic, simple iterator helper dispatch, async generator
  request dispatch, and awaited return continuations to their actual operation
  types. Preserve language-level validation and error propagation.
- Remove the obsolete membrane post-call copy-back loop, whose only producer
  already installs a live reverse facade before publishing the transported value.
- Reject nonpositive Temporal nudge increments and checked-add whole weeks in
  the plain date-time numeric core.
- Retain unit fixtures under `tests/fixtures/` and reuse internal boundary
  contracts against the ordinary library in coverage builds. The `coverage`
  configuration does not expose fixture drivers in normal release builds.

New regressions cover allocation budgets, heap identity, queue order, GC roots,
debugger lifecycle, nested namespaces, buffer resizing/detachment, proxy traps,
atomic operations, synchronous/asynchronous delegation, iterator reentrancy,
metadata getters, deferred cycles, and Temporal arithmetic limits. The existing
VM contracts are retained, including explicit compiler invariant checks.

Fixture moves change source counter denominators. The final audit must identify
those changes and separately compare every originally complete production file.
No completion or nonregression claim is made before the fresh measurement.

### First batch verification and concentrated corrections

The complete first Rust run executed all 334 Cargo-identified harnesses in
849.0 seconds. Five targets failed; all failures were in new regressions.
Test262 was not launched after this failed Rust gate. Original logs and profiles
are retained in `target/test262-macos-20261001-complete-batch/`.

The failures identified missing reverse-membrane forwarding for
`[[DefineOwnProperty]]` and `[[Delete]]`, and a reentrant ShadowRealm evaluation
that replaced its ancestor function's dynamic environment. Corrections add the
missing internal-method forwarding and preserve the displaced execution and
lexical environment. Other failures were fixture assumptions: the legacy
`WithSet` opcode rejects an absent binding; rejected return operands at active
`yield*` reach the delegate's throw protocol; static debugger capability checks
precede realm lookup; and a loop-local `var` declaration does not reset a value.
The execution fixtures now report independent contract failures together.

Rust-only coverage is preliminary, not a complete-suite measurement. Its text
view contained an `18.4E` counter in `test262_agents.rs`, consistent with an
overflowed counter expression. The next measurement uses LLVM's atomic profile
counter updates to remove concurrent increment races. No old profiles or maps
will be merged into that measurement. Clippy's original ten compiler processes
were stopped after blocking on I/O; its next run uses two compiler jobs.

### Critical gate review and second correction batch

The atomic-counter critical gate executed nine harnesses in 152.0 seconds and
stopped with four failed targets, representing three distinct failures. The
remaining 325 Rust harnesses and Test262 were not launched. Logs are retained in
`target/test262-macos-20261001-complete-batch-corrections/`.

The complete correction batch addresses all three findings:

- Temporal's intrinsic namespace released its root even on success, so direct
  internal allocation without a materialized global object left cached
  constructors dangling after collection. Initialization now retains the
  successful namespace root and publishes constructor caches together only
  after every fallible installation succeeds. Allocation-budget regressions
  check rollback, retry and later use, both with and without a global object.
- Fresh ShadowRealm evaluation restored its idle context's zero instruction
  budget before callable Proxy metadata traps ran. Only reentrant evaluations
  need displaced-context restoration; fresh evaluations retain their ordinary
  completion root and remaining budget for wrapper construction.
- The synchronous delegation fixture's cleanup method returned an unfinished
  result. It now returns `done: true`, so the finally-once assertion measures
  completed cleanup rather than an iterator that deliberately remains open.

Additional review establishes the generator entry/resume exit protocols in
small typed adapters, with boundary tests for accepted exits and invariant
rejections. Every compiled async yield crosses an await first; asynchronous
delegation is captured by `resume_async_continuation`, making the second capture
in the direct synchronous generator exit redundant. The validated async brand
cannot change during promise allocation while the receiver is rooted.
The public native dispatch remains grouped as before; request kinds are narrowed
inside the generator scheduler, which explicitly rejects unrelated operations.
Iterator metadata uses the shared numeric representation accessor. Fresh
zipKeyed results use the common data-property installation, including its
allocation failure handling. The reduce callback is validated once before
GetIteratorDirect; a public regression checks revocation during GetNext on an
empty source and invalid-callback close order. Internal contracts cover the
safe-integer callback index boundary, missing/wrong-type zip state and invalid
RegExp receivers. The allocation batch also exercises async-dispose setup.

These changes are prepared together before the next build. Coverage completion
and absence of regressions remain unverified until the full fresh measurement.

### Verification execution notes

The reviewed source snapshot compiles 334 Rust harnesses and both Test262
binaries without compiler warnings. All 334 Rust targets pass: 3,872 tests
pass, none fail, and four are ignored. These include 956 VM unit tests and
six ordinary-library boundary groups. The Rust run takes 4,146.09 seconds,
including the observed macOS loader delays. The complete Test262 run and
final coverage export are still in progress; Rust success does not establish
100% source coverage.

Some newly built harness processes remain at `_dyld_start` for tens of seconds
before emitting their first test output. A one-second read-only stack sample
records this startup delay in
`target/test262-macos-20261001-complete-batch-reviewed/startup-sample.txt`.
The sampled process had a 128 KiB footprint and had not entered its test main.
The binaries retain their linker-generated ad hoc signatures; no host security
setting or executable attribute was changed. The cause beyond the observed
loader delay is not established. Source hashes remain unchanged during the run.

The reviewed BlueJS Clippy gate passes with warnings denied, and both BlueJS
rustdoc tests pass. The separate workspace run initially stops at three engine
library tests whose children do not create their sockets before the existing
15-second startup deadline. The same child binary subsequently enters its
argument parser in 0.034 seconds. A scoped retry of exactly those three cases
passes in 2.49 seconds; the original failures remain in `workspace.log`.
Previously passing workspace cases are not rerun. The remaining crates,
engine targets and rustdoc tests finish with 2,170 passes, no remaining
failures and 60 ignored cases. Four extension and 19 core subprocess cases
also initially time out before socket creation; their exact scoped retries
pass after the actual children enter argument parsing. The core and launcher
argument-parser diagnostics take 101.49 and 109.76 seconds respectively.
Original Cargo exits and all 26 socket-startup failures remain in retained
logs; `workspace-gates.json` reconciles the successful retries without counting
previously passing cases twice.

A copied subset of already-written profiles is exported in
`target/test262-macos-20261001-reviewed-partial-analysis/` solely for source-gap
analysis while the complete measurement continues. This partial Rust-only
export has no Test262 profiles and cannot establish completion or regressions.

## Reviewed full measurement and next consolidated correction batch

The fresh reviewed measurement executes all 334 Rust targets (3,872 passed,
zero failed, four ignored), both BlueJS doctests, all other workspace targets
(2,170 passed after the scoped startup retries, zero remaining failures,
60 ignored), and all 102,926 Test262 modes. Test262 has 102,921 passes,
four declared host exclusions and one stale fixture, with no failing,
unsupported, timed-out or harness-error outcomes.

All conformance outcomes, expected results, actual error kinds/phases, flags,
features and fixture hashes match the restored baseline. Thirty-six diagnostic
messages differ: 35 contain different process-local heap identities, and one
reports a different SyntaxError from the two erroneous dependencies of
`instn-resolve-order-depth.js`. The unchanged adapter eagerly parses its
`HashMap` of required sources; that diagnostic order is not stable between
processes. The original message differences remain in `outcome-diff.json`;
`outcome-contract-audit.json` compares every conformance contract without
silently rewriting the retained results.

Raw LLVM coverage is 78,979 / 79,817 lines (98.950098%),
5,749 / 5,829 functions (98.627552%) and
132,541 / 135,122 regions (98.089874%). Atomic counter integrity passes.
There are 179 Rust source files, 162 instrumented files, 17 files with no
executable counters, and 97 complete instrumented files. Only one of the
22 selected files is complete: `vm/temporal/conversion/from_value.rs`.
The D5 files still miss 311 lines, 11 functions and 937 regions; the D4 files
miss 67 lines, one function and 509 regions. This batch does not satisfy the
requested coverage gate.

The first priority is the original-completion regression in
`vm/test262/reverse.rs`: its new DefineOwnProperty and Delete operations each
lack coverage for an unavailable active parent. Its lines and functions are
complete, but regions are 934 / 936. The next batch must exercise actual
retained reverse facades after their parent call ends. No production exclusion
or source-location union substitution is permitted.

The raw percentages in page runtime, modules and the two numeric difference
files decrease even though their missing counters decrease: retained unit
fixtures moved out of production files, reducing their denominators. These
changes are not evidence of new passing execution. The unchanged
`vm/test262_agents.rs` has one additional uncovered region at the condition
variable wait return; the baseline counter audit finds no unsigned underflow.
Its scheduling-sensitive wake path needs deterministic regression coverage.
The stricter current measurement and all original counts are retained.

All remaining source gaps are collected in
`target/test262-macos-20261001-complete-batch-reviewed/source-gaps.json`.
The correction inventory below is prepared together; no build or test is
launched while these code and fixture changes are incomplete.

### Consolidated corrections on 2026-10-02

No tests were executed while preparing each correction batch. Its frozen
snapshot is then compiled and verified as recorded below. Formatting with
`rustfmt --config skip_children=true` checks Rust syntax
and formatting only; it does not establish compilation, behavior, or coverage.
The preceding full measurement remains immutable and applies only to its
retained source snapshot. Every fixture below still requires execution against
fresh binaries after the prepared implementation snapshot is frozen.

#### Cleanup and publication failures found by static review

- **Iterator unwinding:** root all pending iterator records while inner return
  callbacks can collect; retain the first thrown object until every outer
  iterator has closed. Share this ordering for ordinary returns and async
  generator aborts. Fixtures use actual compiled callbacks, three nested
  records, object/primitive throws, and both normal and minimal nurseries.
- **Temporary operand roots:** restore stack lengths on iterator getter errors,
  helper failures, zip padding failures, Array.fromAsync rejection, and RegExp
  execution/method failures. Retain helper receivers and private records during
  IteratorClose. Meaningful checks cover close order, exact errors, completed
  helper state, unchanged storage after rejected arguments, and later VM use.
- **Execution cleanup:** completion-root registration failure and rejected
  global declaration preparation must still clear transient frames, cells,
  scopes, and weak keep-alive state. A declaration rejection preserves the
  existing nonwritable global and prevents its script body from running.
  Scripts with no declarations avoid unnecessary global bootstrap.
- **Namespace publication:** acquire both temporary and cache roots before
  publishing a linked record's placeholder. If the second registration fails,
  rollback can no longer leave a placeholder outside the transaction cache.
  Cyclic fixtures preserve an existing unrelated namespace. Ordinary cycles
  need six registrations and deferred cycles need nine: each namespace needs
  two roots, and each namespace export needs its own binding-cell root.
- **Source-import publication:** a cold host source prototype is published only
  after a retained source object roots it. Allocation or root failure otherwise
  leaves a dangling prototype cache after collection. Fixtures cover allocation
  failure, collection, retry, missing host sources, parsed source re-exports,
  and preservation of already cached source identities.
- **Debugger ownership:** a missing retained graph restores the paused module
  continuation so the embedding host can restore the graph and resume once.
  Object throws and completion-root failures clean both child and parent
  frames. Async dependencies that reject prevent the requested entry pause.
- **Await publication:** validate the Promise and reserve its serial before
  publishing an async or module continuation. A refused handoff leaves no
  orphan frame and does not consume a serial. Fixtures transfer actual
  suspensions produced by compiled code, using a live non-Promise object or
  monotonic serial exhaustion; no heap object or bytecode is corrupted.
- **Realm reentrancy:** validate inactive and self ancestor contexts before
  unsafe reborrowing. Preserve displaced execution, lexical state and roots
  around nested ShadowRealm evaluation/import, including initialization failure
  and child Promise work. Wrapper construction failures release their source
  target roots; unavailable reverse parents retain their original error class.
- **Membrane root pairs:** failure of a second parent root registration releases
  the first source root. Failure of a second child registration releases its
  temporary facade root before publication. Fixtures use real created realms
  and objects, assert that no import/reverse cache is published, then collect
  both heaps to check that no failed construction retains its source or facade.
- **Realm creation:** delay parent-facade rooting until immediately before
  publication. If the remaining host facade or `evalScript` installation fails,
  remove every new facade and the private child heap. Root and allocation
  fixtures require no published realm, no retained parent facade after GC, and
  a reusable parent VM.
- **Eval recreation:** release a newly acquired global binding root when its
  allocation-backed cell store fails. Publish recreated dynamic bindings only
  after their parameter environment has accepted the cell.
- **Reflective lazy initialization:** retain the descriptor receiver while
  lazy intrinsics can collect. A minimal-nursery fixture uses an ordinary
  receiver with no permanent root and verifies collection after the operation.
  DefineProperty, Delete, and OwnPropertyKeys also retain their receivers;
  DefineProperty retains all value-bearing descriptor fields.
- **Intrinsic cache publication:** stage Intl constructors together and publish
  the complete namespace only after global-property installation succeeds.
  RegExp, Error, and ordinary global builders likewise publish their cache
  after the final property store. Getter and symbol-method installers restore
  temporary operand roots on every error. Independent cold/warm builder
  fixtures exercise allocation failure, retained cache identity, and retry.
- **Segmenter prototype pairs:** build the Iterator base before acquiring a
  temporary segments root; release that root if iterator allocation or rooting
  fails. Scoped root exhaustion checks both registrations without publishing
  either prototype and verifies reclamation after collection.
- **Foreign new targets:** extend intrinsic recognition to String, disposable
  stacks, ShadowRealm, Intl services, and Temporal constructors. Temporal
  prototype lookup uses its cached constructor identity. Public regressions
  construct actual foreign functions, collections, binary types, Intl services,
  and Temporal values with an explicit caller-owned prototype.

#### Original intrinsic identity is independent of mutable constructor links

Static review found lazy initializers reading `String.[[Prototype]]` and
unwrapping it as the original `%Function.prototype%`. JavaScript can legally
set that link to null or another object. Those reads can panic or install a
native function with the wrong prototype. String bootstrap now retains and
publishes the original Function prototype atomically with its String cache;
every affected initializer reads the shared intrinsic helper instead.

Prepared public regressions change the String link before initializing RegExp,
Error subclasses, dynamic Function, binary constructors, Temporal, Iterator,
string/array/RegExp iterators, generator methods, restricted argument accessors,
legacy function accessors, Intl constructors/bound methods/segment iterators,
and the Test262 host. Both null and ordinary-object replacements run with
normal and minimal nurseries. The old fixture that expected a missing Function
prototype after mutation has been corrected to require the original identity.
Scoped bootstrap root exhaustion checks that neither cache is published when
registration fails.

This shared fix also changes `vm/intrinsics.rs`, `vm/lifecycle.rs`,
`vm/builtins/arguments.rs`, `vm/builtins/globals.rs`, `vm/builtins/dynamic.rs`,
`vm/errors.rs`, `vm/intl.rs`, `vm/intl/shared.rs`, `vm/intl/collator_locale.rs`,
`vm/intl/number_runtime.rs`, `vm/intl/date_time.rs`, and `vm/test262/harness.rs`.
They are included in the final 100% raw coverage gate and original-completion
audit. Their previous completion status is not evidence for these edits.

#### Remaining r4 failures and the complete r5 correction set

The r4 complete-target build passed. Nine critical targets then reported
1,119 passes, six failures, and no ignored tests in 393.392 seconds. Three
targets failed; the other 325 targets and Test262 were not launched. The
retained r4 gate is partial behavior evidence, not full coverage evidence.

The six failures reduce to four distinct causes, addressed together before
another verification run:

- Async-generator queue growth called the heap directly. Its new queue edges
  were protected, but unrelated caller operands and VM-owned Promise records
  were invisible to collection. Queue growth/replacement and allocating
  delegation frame writes now use the VM's full root batch. A completed sync
  delegate's private record remains rooted until its done flag is updated.
- Dynamic import directly fulfilled its Promise with the namespace, bypassing
  an exported callable `then`. Eager results, awaited module waiters, deferred
  waiters, and source-phase results now use Promise resolution. This follows
  [ContinueDynamicImport](https://tc39.es/ecma262/multipage/ecmascript-language-expressions.html#sec-continue-dynamic-import).
  Public regressions exercise eager and top-level-await thenables, thrown
  reasons, noncallable exports, inherited source thenables/getters, and
  ShadowRealm rejection when assimilation produces a primitive.
- Warm Temporal bootstrap and cold Intl initialization exceeded the old
  131,072-byte sweep ceiling. Seven allocation matrices now advance to the
  first refused request's required budget, within 16 MiB. Recording occurs
  after normal collection and preserves the first refusal even if error cleanup
  also runs out of memory. Every visited boundary still checks errors, retained
  roots/cache identity, and retry. No repeated eight-byte retries are needed to
  revisit the same allocating operation.

The added test telemetry is restricted to `test`/`coverage` configurations.
The production allocation checks and GC trigger invariant are preserved.
`heap.rs`, `heap/lifecycle.rs`, and `vm/builtins/promises.rs` are newly modified
production files and therefore join the fresh 100% raw coverage gate. All r5
changes and fixtures are prepared before compilation or execution; they remain
unverified until the frozen snapshot's consolidated gates finish.

#### R5 critical gate and shared accessor cleanup

The r5 complete-target build passed. Its nine critical targets reported
1,125 passes, two failures, and no ignored tests in 194.512 seconds. Both failed
targets identify the same defect in `install_native_accessor`: setter allocation
can fail after the getter is pushed but before entering the cleanup closure.
The r5 dynamic-import, ShadowRealm, async queue, and budget corrections passed
their critical cases. The other 325 targets and Test262 were not launched;
there is still no fresh full coverage export.

The r6 correction moves the setter allocation into the accessor cleanup scope,
retaining getter protection during collection while releasing it on failure.
The existing installer matrix now preserves a caller operand across each
success/refusal, collects after restoring the budget, and executes a later
script. Ordinary and symbol getter installers already cover their allocating
steps; Iterator and RegExp accessor installers use full cleanup scopes. Host
accessors share the corrected installer. These changes precede compilation
and tests. R6 remains unverified until its frozen consolidated gates finish.

#### R6 gate and valid constructor entry

R6 built all targets successfully. Its critical gate recorded 1,114 passes,
thirteen failures, and no ignored tests in 208.850 seconds. Eleven failures
involved worker startup; three assert an unexpected worker reply/timeout after
startup failed. Two identify the same ShadowRealm fixture precondition: `Vm`
starts with zero remaining instructions until script entry, so calling its
native constructor directly cannot model an initial script invocation. R6's
accessor cleanup assertions passed. Remaining targets, Test262, and full
coverage were not executed or exported; original failure logs are retained.

R7 constructs ShadowRealm through a compiled public script, establishing fuel
and constructor context together. The verification harness records a standalone
regex-worker readiness check before executing suites to separate cold process
loading from operation deadlines. No production timeout changes are required.
All fixture changes precede the next frozen gate; R7 remains unverified.

#### R7 gate and cached prototype ownership

R7 built all targets successfully. Its initial worker prerequisite timed out
at 60 seconds before tests started. A later diagnostic launch completed the
READY handshake and exited in 0.086 seconds; the prerequisite then completed
in 0.019 seconds without changing the source or binary. The retained diagnostic
records separate this startup observation from test behavior.

The nine critical targets reported 1,125 passes and two failures in 210.783
seconds. Both failures identify the same cached ShadowRealm prototype lifetime:
the prototype had no permanent root before attachment to its constructor, and
failed construction could collect it while its ID remained cached. No worker
startup failures recurred. No remaining targets, Test262, or full raw coverage
were executed/exported.

R8 roots the ShadowRealm intrinsic before publication, releases the new root
on initializer failure, and checks root refusal plus collection under normal
and minimal nurseries. The other prototype caches were reviewed for the same
failure pattern: iterator/generator/RegExp caches retain permanent roots;
the Date prototype cache instead published before constructor/global completion
and is now published afterward. The initializer matrices explicitly collect
before retry, include materialized global objects and all Error globals, and
aggregate independent failed initializers before failing the test. This avoids
leaving later cases unobserved behind the first failure. All changes precede
the frozen verification gate. R8 remains unverified.

#### R8 aggregate gate and the complete cache root family

R8 built all targets successfully. Its nine critical targets reported
1,126 passes and two failures in 189.314 seconds. The cached ShadowRealm root,
root-refusal fixture, Date publication, and worker prerequisite passed. The
initializer aggregate exposed nine failures together: Map, Set, WeakMap,
WeakSet, WeakRef, FinalizationRegistry, Promise, DisposableStack, and
AsyncDisposableStack. These cached prototypes were absent from `with_roots`
before a constructor owned a heap edge to them; explicit collection after
failed constructor initialization invalidated the cached IDs.

R9 adds all nine strong VM cache edges to the common safepoint root batch.
Independent fixtures initialize each prototype before constructor publication,
collect under normal/minimal nurseries, verify identity, then materialize the
constructor and check its exact prototype. The production allocating paths
were reviewed for direct heap collection outside `with_roots`; the remaining
direct metadata writes do not grow their payload. The full allocation matrix
continues to collect before each retry. All changes precede verification.
R9 built all targets successfully. Its nine critical targets passed all 1,129
tests. The full 334-target Rust run finished in 1,720.6 seconds with 3,942 passes,
eight failures across seven targets, and four existing ignored tests. Standalone
Test262 was not launched. Fresh R9 coverage is exported for diagnosis only:
the snapshot has failed Rust contracts and is not a completion or nonregression
measurement. No earlier profiles or maps are used.

The seven failed targets are `cov_g8_heap`, `try_completion`,
`eval_var_environment_capture`, `cov_g4_test262_language`, `cov_g4_binary_data`,
`test262_host`, and `eval_var_catch_parameter`. They expose five root causes:
uninitialized namespace descriptors; overbroad direct-eval cell aliases;
foreign species buffer brand validation; unvalidated foreign buffer refresh
receivers; and immutable foreign detach operations. The eval aliases affect
references resolved before eval, outside closures, and Annex B catch-parameter
initialization. The next batch must select existing local eval captures without
redirecting those references and must retain genuine namespace/buffer errors.
All failures and remaining raw gaps will be addressed before rebuilding.

#### Producer invariants reviewed before narrowing fallibility

- Heap iterator-helper flag/index updates do not allocate. Private concat,
  zip, flatMap, and window metadata are retained by their validated, rooted
  helper. Object-or-undefined slot replacement has zero additional payload
  bytes under `property_bytes`; genuine new-property allocations remain
  fallible, including the zip-start marker and padding installation.
- ToBoolean and callable/constructor classification are pure reads for a
  freshly returned current-realm value. Entry heap identity checks, revoked
  Proxy errors, user traps, coercion, allocation, detachment and resize checks
  are retained. Validated buffer/view metadata cannot change its object brand.
  Checked shared-buffer prefixes can grow but cannot shrink.
- `compile_eval` filters existing VariableEnvironment names, including current
  dynamic eval bindings, out of fresh declaration slots. Capturing cells cannot
  execute JavaScript before instantiation. The unreachable existing-binding
  merge is removed; public repeated-eval fixtures check captured outer cells,
  local shadowing, reassignment, and closure reads.
- Object GetOwnProperty handles a foreign ordinary facade before lazy work and
  a foreign Proxy through Proxy dispatch. Object kind does not change, so the
  later duplicate foreign dispatch has no producer. Its initial heap/proxy
  validation remains explicit.
- Typed root/module/generator completion adapters distinguish valid return,
  await, yield, pause and handler successors. Invalid internal handoffs are
  rejected without manufacturing interpreter instructions. Async return
  operands are awaited before delegated throw/return dispatch; a rejecting
  operand with no delegate throw method produces the protocol TypeError.
- RegExp source conversion already checks the string limit before flag
  conversion; the immutable source and execution limit cannot change between
  those steps. Escaped-source output and compile-from-existing-RegExp limits
  remain fallible and have separate prepared regressions.
- Ordinary calls enforce return-string limits in `enter_call`; the interpreter
  no longer repeats that check after the call. Direct eval can return its input
  value without entering a call, so its limit checks remain explicit. Private
  iterator completion replaces an existing Boolean field without allocating.
- The private host callback tag remains u32. Its checked integer encoder is
  exercised at zero, the maximum tag, and the first out-of-range index without
  constructing an invalid or enormous callback registry. Registration still
  checks the representation before retaining a callback or publishing a native.
- Temporal validated one-day borrowing, weeks-only civil addition, and bounded
  signed instant spans use their proven numeric domains. Calendar overflow,
  and unrepresentable date-time endpoints remain fallible. Compatible resolution
  uses the closed fixed-offset/named-zone domain and the locked Jiff database.
  Static metadata inspection of `jiff-tzdb 0.1.8` / IANA 2026c found 598 names,
  341 distinct TZif records, offsets within -57,368 to 54,822 seconds, a largest
  historical jump of 86,400 seconds, and at least 601,200 seconds between offset
  changes. Recurring POSIX transitions are separated by calendar months. No
  recorded change lies within four days of the 1970/2000/2001 projection seam
  positions. These isolated changes bracket the two-day probes and gap shift;
  compatible resolution is total for the valid civil inputs used here. The
  resulting instant is still range-checked. The source-data hash and extracted
  metadata are retained in
  `target/test262-macos-20261002-prepared-static-analysis/tzif-domain.json`.
  Pending regressions cover ordinary DST gaps, the skipped Apia day, both Jiff
  boundaries, and a distant 400-year projection seam.

The allocation matrix now also covers class metadata/private brands, lexical
iteration cells, deleted/recreated eval bindings, lazy Intl constructors and
bound methods, argument accessors, and Array.fromAsync await/close turns. Every
case checks the exact heap-limit error, temporary-root cleanup, and later VM
use. Successful async cases check their settled value rather than merely that
job execution returned. Additional sweeps cover async-generator queues,
awaited return operands, yield delegation, finally completion, child-heap
membrane snapshots, and the independently initialized global/intrinsic builders.
ShadowRealm wrapping checks both uncatchable allocation errors and the required
opaque TypeError when metadata copying fails; it checks target-root release
and later use of both realms.

#### Prepared snapshot and consolidated verification

The current static correction pass is prepared for a single consolidated gate:
freeze its source hashes, compile all targets, execute the critical boundary
groups, then run the complete Rust and Test262 suites and export fresh raw
coverage. Compiler or gate failures require corrective work and remain failures
until measured again. This readiness statement is not a completion or coverage
claim. Old counter locations refer to the reviewed snapshot and cannot be
counted as new coverage after refactoring.

Prepared fixtures exercise the scheduling-sensitive agent wake path through an
actual delayed host timer; production agent code is unchanged. This is pending
execution and does not establish deterministic scheduling or restored coverage.

The first consolidated build rejected three unique compiler errors before any
test ran. Its diagnostics and source hashes are retained in
`target/test262-macos-20261002-corrections-batch/`. The complete compiler
correction set restores the String constructor binding, avoids comparing a
descriptor type without PartialEq, and keeps the private RegExp slot assertion
inside its owning module's fixture. The corrected build is retained separately
in `target/test262-macos-20261002-corrections-batch-r2/`. The library compiled,
but two debugger integration assertions still accessed a private name lookup.
Those assertions now observe persistent globals through public script execution
and check the aborted continuation before starting another script. The complete
target build passed in `target/test262-macos-20261002-corrections-batch-r3/`,
emitting 334 Rust harnesses and two tools against 555 retained source hashes.
The consolidated Rust suite stopped after its nine critical targets; four
targets failed. The VM unit target reports 1,004 passes and eleven failures.
The remaining 325 targets and Test262 were not executed. All failures are
retained and reviewed together before another implementation batch. No fresh
full-coverage export or new production completion claim exists.

#### Corrections prepared after the failed critical gate

All eighteen failed checks were analyzed together before another test run.
The production corrections restore RegExp legacy getter/setter stack cleanup,
retain captured-cell aliases for eval var stores even when the caller has an
environment object, and keep active async frames rooted after removal from
their waiting map. Those roots survive interpreter context swaps and cover
yielded objects while a suspended generator frame is stored. A failed store
restores the displaced context before returning the resource error. Public
regressions release a generator before its queued requests finish and exercise
repeated eval declarations over both mutable and immutable outer captures.

The fixtures now supply valid constructor targets, mark the produced iterator
record rather than an unrelated state property, use supported deferred-import
syntax, install `$DONE`, and respect the module error-value contract. Root-ID
exhaustion checks cache publication and live prototype edges after collection;
already-installed bootstrap methods need not disappear. Cold source-module
initialization has enough budget range to reach success. Allocation sweeps
collect setup garbage and retain the production invariant that the major GC
trigger does not exceed the heap limit. The former test hook's `usize::MAX`
trigger allowed the allocator fast path to bypass the reduced limit. The new
snapshot requires fresh consolidated verification; earlier partial profiles
will not be reused.

### D5 reviewed gap counts

| Source | Missing lines | Missing functions | Missing regions |
| --- | ---: | ---: | ---: |
| `vm/builtins/binary_data.rs` | 4 | 0 | 91 |
| `page_runtime.rs` | 9 | 0 | 36 |
| `vm/builtins/execution/closures.rs` | 0 | 0 | 2 |
| `vm/debugger.rs` | 71 | 1 | 92 |
| `vm/execution.rs` | 62 | 1 | 184 |
| `vm/interpreter.rs` | 43 | 3 | 152 |
| `vm/builtins/generators.rs` | 60 | 2 | 161 |
| `vm/shadow_realm.rs` | 24 | 1 | 33 |
| `vm/test262/foreign.rs` | 38 | 3 | 186 |

- **`vm/builtins/binary_data.rs`:** Review remaining constructor/species/atomic/view errors against valid foreign-heap and wrong-brand objects; cover identifier exhaustion separately from allocation budgets. Preserve coercion, detachment and resizing order.
- **`page_runtime.rs`:** Exercise public stale registry/module handles, unknown realms, module graph admission errors and click microtask failures. Review the linked snapshot identity check against its opaque frame producer; keep identity validation explicit.
- **`vm/builtins/execution/closures.rs`:** Cover binding-cell heap identity rejection and nested debugger abrupt completion with an object-valued throw, active iterator and cleanup.
- **`vm/debugger.rs`:** Add generation/code-unit/offset/entry validation, target-not-reached module graphs, nested step refusal, object-valued abort cleanup and caught/uncaught nested failures. Review completion classification without corrupting bytecode.
- **`vm/execution.rs`:** Cover dynamic eval cells across declaration, shadowing, deletion/recreation and captured environments; exercise iterator close on return/throw, failing close hooks, resource exhaustion and global declaration boundaries.
- **`vm/interpreter.rs`:** Exercise retained completion handlers, nested loop iterator cleanup, compiler binding-reference representations and private/class receiver boundaries using valid code and heap objects. Do not manufacture corrupted instructions.
- **`vm/builtins/generators.rs`:** Cover missing/throwing delegate methods, abrupt completion inside catch/finally, rejected awaited return operands, pending delegate completion and queued requests. Review scheduler invariants and resource errors independently.
- **`vm/shadow_realm.rs`:** Cover unavailable ancestor contexts, wrapped function construction refusal, reentrant import/evaluation cleanup and object-result root release. Review fulfilled namespace provenance and resource-error paths.
- **`vm/test262/foreign.rs`:** Cover root-registration failures, detached/out-of-bounds foreign typed arrays, constructor/native buffer operations, mirror synchronization, foreign iterator receiver validation and unsupported intrinsic classifications.

### D4 reviewed gaps

| Source | Missing lines | Missing functions | Missing regions |
| --- | ---: | ---: | ---: |
| `vm/builtins/array_from_async.rs` | 1 | 0 | 37 |
| `vm/builtins/execution/setup.rs` | 2 | 0 | 13 |
| `vm/builtins/execution/runtime.rs` | 4 | 0 | 109 |
| `vm/modules/namespace.rs` | 2 | 0 | 17 |
| `vm/modules/deferred.rs` | 9 | 0 | 31 |
| `vm/modules.rs` | 19 | 0 | 33 |
| `vm/builtins/object.rs` | 22 | 1 | 173 |
| `vm/regexp.rs` | 3 | 0 | 52 |
| `vm/temporal/conversion/from_value.rs` | 0 | 0 | 0 |
| `vm/temporal/duration_relative.rs` | 1 | 0 | 1 |
| `vm/temporal/plain_date_time_difference.rs` | 1 | 0 | 6 |
| `vm/temporal/zoned_difference.rs` | 3 | 0 | 19 |
| `vm.rs` | 0 | 0 | 18 |

- **`vm/builtins/array_from_async.rs`:** Review private state-record reads and rooted promise provenance; cover close/reject/resolve and collection allocation failures. Keep user iterator/getter/callback errors distinct from private metadata invariants.
- **`vm/builtins/execution/setup.rs`:** Cover async disposal adoption/rejection setup, cold iterator prototypes, zip padding state allocations and invalid callbacks before GetIteratorDirect.
- **`vm/builtins/execution/runtime.rs`:** Cover every iterator helper receiver boundary, safe-integer indexing, concat/zip/chunks/windows metadata and close order, callback errors, async-from-sync transitions and allocation propagation.
- **`vm/modules/namespace.rs`:** Review live record and export resolution invariants; exercise namespace root exhaustion and cyclic rollback with existing valid cache entries.
- **`vm/modules/deferred.rs`:** Cover ready/evaluated/error SCC states, missing retained graphs, async dependency gathering and temporary active-record cleanup. Use real linked module records.
- **`vm/modules.rs`:** Review module debugger terminal cleanup, error roots, async frame resumption and promised module lifecycle. Keep genuine suspension errors and module graph cleanup observable.
- **`vm/builtins/object.rs`:** Cover remaining foreign/reverse dispatch, receiver/proxy/constructor boundaries and descriptor errors with valid objects. Review immutable function provenance separately from observable proxy traps.
- **`vm/regexp.rs`:** Cover cold intrinsic allocation and valid receiver heap rejection; review compiled matcher/capture/private iterator provenance. Preserve coercion, lastIndex, species and substitution order.
- **`vm/temporal/conversion/from_value.rs`:** Complete in this reviewed measurement. Preserve all raw counters while preparing the remaining batch.
- **`vm/temporal/duration_relative.rs`:** Exercise an endpoint that is a valid ISO date but an unrepresentable date-time, and retain equal-endpoint behavior at limits.
- **`vm/temporal/plain_date_time_difference.rs`:** Exercise calendar/date overflow at each Option boundary and second nudge-window failure; review one-day borrowing bounds against validated dates.
- **`vm/temporal/zoned_difference.rs`:** Exercise week/month/day window overflow, calendar bubbling at limits, instant resolution failure and signed span conversion. Retain fallible compatible-time-zone resolution unless its totality is proved.
- **`vm.rs`:** Review remaining host-function registration/root failures, call entry/dispatch validation and intrinsic initialization errors. Preserve the already complete raw line and function counters.


## Baseline gap inventory details

Counts below are raw uncovered lines/functions/regions. Contexts identify the
existing uncovered source-location union regions; their totals can differ from
raw counters because LLVM instantiations are distinct.

### D5

#### `vm/builtins/binary_data.rs`

- Raw gaps: 15 lines, 3 functions, 104 regions.
- `atomics_modify:927`: 2 uncovered locations; source lines 934, 935.
- `atomics_read:973`: 3 uncovered locations; source lines 975, 976.
- `typed_array_of:1561`: 2 uncovered locations; source lines 1566, 1574.
- `iterable_values:1404`: 1 uncovered locations; source lines 1419.
- `typed_array_set:1616`: 5 uncovered locations; source lines 1628, 1658, 1664, 1668, 1694.
- `buffer_prototype:8`: 3 uncovered locations; source lines 13, 15.
- `typed_array_from:1437`: 6 uncovered locations; source lines 1442, 1449, 1473, 1487, 1519, 1529.
- `atomics_wait_async:1130`: 4 uncovered locations; source lines 1133, 1164, 1175, 1177.
- `typed_array_values:1357`: 1 uncovered locations; source lines 1366.
- `typed_array_subarray:1702`: 8 uncovered locations; source lines 1725, 1726, 1730, 1731, 1733.
- `typed_array_intrinsics:33`: 4 uncovered locations; source lines 38, 40, 156, 214.
- `buffer_resize:361`: 2 uncovered locations; source lines 367, 373.
- `data_view_get:771`: 2 uncovered locations; source lines 789, 792.
- `data_view_set:802`: 3 uncovered locations; source lines 812, 837, 841.
- `atomics_access:852`: 3 uncovered locations; source lines 860, 865, 871.
- `atomics_notify:1074`: 3 uncovered locations; source lines 1076, 1089, 1092.
- `array_buffer_slice:486`: 7 uncovered locations; source lines 492, 500, 519, 529, 534, 540, 542.
- `atomics_revalidate:949`: 2 uncovered locations; source lines 950, 955.
- `shared_buffer_grow:420`: 1 uncovered locations; source lines 427.
- `atomics_wait_access:893`: 5 uncovered locations; source lines 900, 905, 906, 911, 912.
- `atomics_wait_status:1097`: 3 uncovered locations; source lines 1100, 1114, 1115.
- `atomics_binary_value:979`: 2 uncovered locations; source lines 995, 1008.
- `array_buffer_receiver:332`: 1 uncovered locations; source lines 336.
- `array_buffer_transfer:386`: 6 uncovered locations; source lines 394, 396, 405, 414, 415, 416.
- `data_view_constructor:661`: 5 uncovered locations; source lines 676, 690, 695, 718, 723.
- `species_result_buffer:554`: 1 uncovered locations; source lines 561.
- `typed_array_constructor:1188`: 10 uncovered locations; source lines 1217, 1223, 1242, 1261, 1262, 1271, 1273, 1274, 1275, 1281.
- `shared_array_buffer_slice:571`: 4 uncovered locations; source lines 577, 604, 609, 610.
- `constructed_buffer_prototype:21`: 1 uncovered locations; source lines 25.
- `shared_array_buffer_receiver:344`: 1 uncovered locations; source lines 353.
- `shared_species_result_buffer:616`: 1 uncovered locations; source lines 625.
- `array_buffer_species_constructor:436`: 1 uncovered locations; source lines 453.
- `shared_array_buffer_species_constructor:461`: 1 uncovered locations; source lines 478.

#### `page_runtime.rs`

- Raw gaps: 20 lines, 0 functions, 68 regions.
- `dispatch_host_click:1013`: 1 uncovered locations; source lines 1027.
- `debugger_value_preview:921`: 1 uncovered locations; source lines 934.
- `debugger_uncaught_throw_site:951`: 1 uncovered locations; source lines 959.
- `debugger_linked_value_preview:852`: 1 uncovered locations; source lines 866.
- `debugger_linked_stack_snapshot:816`: 2 uncovered locations; source lines 826, 844.
- `run_click_microtask_checkpoint:1034`: 2 uncovered locations; source lines 1041, 1044.
- `step_debugger_root_instruction:994`: 1 uncovered locations; source lines 1001.
- `module_evaluate_entry_safe_point:1365`: 3 uncovered locations; source lines 1373, 1380, 1390.
- `resume_debugger_module_execution:1270`: 1 uncovered locations; source lines 1276.
- `continue_debugger_nested_execution:749`: 1 uncovered locations; source lines 757.
- `step_debugger_module_root_instruction:1285`: 1 uncovered locations; source lines 1291.
- `resume_debugger_linked_nested_execution:784`: 1 uncovered locations; source lines 791.
- `execute_program_until_nested_debugger_pause:698`: 2 uncovered locations; source lines 711, 725.
- `validate_linked_nested_debugger_target:1235`: 2 uncovered locations; source lines 1250, 1254.
- `execute_module_graph_until_debugger_pause:1125`: 2 uncovered locations; source lines 1133, 1136.
- `execute_module_graph_until_nested_debugger_pause:1157`: 3 uncovered locations; source lines 1164, 1168, 1179.
- `execute_module_graph_until_linked_nested_debugger_pause:1187`: 2 uncovered locations; source lines 1202, 1206.
- `host_click_dispatch_is_realm_bound_and_old_document_listeners_expire:2757`: 7 uncovered locations; source lines 2764, 2765, 2766, 2773, 2792, 2797, 2802.
- `nested_page_module_frame_preserves_graph_and_rejoins_entry:2031`: 1 uncovered locations; source lines 2062.
- `module_entry_pause_retains_exact_page_generation_until_resume:2862`: 4 uncovered locations; source lines 2888, 2898, 2912, 2935.
- `nested_page_frame_is_exact_and_revoked_after_return_or_navigation:1913`: 5 uncovered locations; source lines 1947, 1955, 1998, 2016.
- `linked_module_frame_keeps_both_page_programs_and_expires_on_resume:2119`: 2 uncovered locations; source lines 2155, 2229.
- `nested_page_resume_rejoins_one_module_graph_and_revokes_exact_frame:2344`: 2 uncovered locations; source lines 2376, 2384.
- `failed_nested_page_step_revokes_the_frame_without_claiming_completion:2410`: 3 uncovered locations; source lines 2430, 2440.
- `page_stack_snapshot_requires_the_paused_program_and_exact_nested_frame:1840`: 2 uncovered locations; source lines 1865, 1883.
- `uncaught_site_read_requires_exact_page_owned_program_and_last_execution:1697`: 4 uncovered locations; source lines 1717, 1726, 1735, 1740.

#### `vm/builtins/execution/closures.rs`

- Raw gaps: 6 lines, 0 functions, 5 regions.
- `call_closure:98`: 3 uncovered locations; source lines 397, 431, 440.
- `binding_value:52`: 1 uncovered locations; source lines 57.

#### `vm/debugger.rs`

- Raw gaps: 110 lines, 1 functions, 124 regions.
- `execute_module_graph_until_debugger_pause:209`: 17 uncovered locations; source lines 215, 218, 220, 223, 229, 241, 247, 252, 255, 256, 257, 259, 261.
- `run_module_graph_until_nested_debugger_pause:475`: 7 uncovered locations; source lines 494, 497, 502, 505, 516, 520, 528.
- `run_debugger_script:1017`: 10 uncovered locations; source lines 1045, 1046, 1049, 1055, 1056, 1059.
- `resume_nested_module_throw:757`: 2 uncovered locations; source lines 797, 803.
- `continue_debugger_execution:942`: 4 uncovered locations; source lines 983, 986, 989.
- `abort_nested_module_execution:818`: 12 uncovered locations; source lines 841, 842, 843, 844, 846, 854, 855, 869.
- `ensure_no_debugger_continuation:999`: 1 uncovered locations; source lines 1009.
- `finish_debugger_interpret_result:1075`: 3 uncovered locations; source lines 1087, 1090.
- `find_code_unit:289`: 5 uncovered locations; source lines 298, 303, 305, 314, 324.
- `execute_script_until_debugger_pause:885`: 7 uncovered locations; source lines 891, 905, 918, 919, 920.
- `continue_debugger_nested_instruction:557`: 33 uncovered locations; source lines 581, 582, 646, 647, 648, 651, 659, 666, 667, 668, 669, 670, 671, 672, 739, 740, 741, 742, 743, 749.
- `validate_linked_nested_debugger_target:435`: 6 uncovered locations; source lines 442, 448, 451, 456, 466, 468.
- `execute_script_until_nested_debugger_pause:335`: 2 uncovered locations; source lines 342, 343.
- `execute_module_graph_until_nested_debugger_pause:381`: 4 uncovered locations; source lines 388, 391, 393, 398.
- `execute_module_graph_until_linked_nested_debugger_pause:414`: 1 uncovered locations; source lines 421.

#### `vm/execution.rs`

- Raw gaps: 80 lines, 4 functions, 209 regions.
- `with_roots:1330`: 17 uncovered locations; source lines 1351, 1353, 1366, 1367, 1504, 1540, 1541, 1542, 1543, 1544.
- `delete_dynamic_eval_binding:800`: 20 uncovered locations; source lines 800, 803, 805, 806, 807, 810, 811, 812, 813, 814.
- `prepare_eval_dynamic_var_declarations:1646`: 15 uncovered locations; source lines 1663, 1692, 1693, 1694, 1695, 1696, 1698.
- `create_global_binding:389`: 5 uncovered locations; source lines 399, 409, 426, 433, 442.
- `finish_root_execution:161`: 3 uncovered locations; source lines 167, 172, 198.
- `run:1559`: 2 uncovered locations; source lines 1570, 1572.
- `clone_scope:981`: 4 uncovered locations; source lines 985, 986, 992, 993.
- `reset_scope:962`: 3 uncovered locations; source lines 968, 969.
- `delete_eval_var:724`: 5 uncovered locations; source lines 727, 729, 739.
- `eval_var_deleted:589`: 2 uncovered locations; source lines 598, 600.
- `recreate_eval_var:664`: 14 uncovered locations; source lines 667, 671, 680, 682, 684, 685, 697, 710, 711, 713.
- `store_global_cell:897`: 4 uncovered locations; source lines 910, 915, 930, 933.
- `resolve_completion:1145`: 4 uncovered locations; source lines 1192, 1258, 1265.
- `set_global_binding:488`: 3 uncovered locations; source lines 499, 508, 510.
- `assign_binding_slot:608`: 5 uncovered locations; source lines 616, 617, 619, 628, 633.
- `assign_unbound_name:640`: 10 uncovered locations; source lines 646, 647, 651, 654, 655, 656.
- `delete_eval_env_var:781`: 4 uncovered locations; source lines 788, 791, 793, 796.
- `delete_unbound_name:865`: 7 uncovered locations; source lines 872, 882, 885, 890, 891.
- `remove_eval_binding:746`: 4 uncovered locations; source lines 766, 769, 771, 774.
- `global_binding_value:481`: 1 uncovered locations; source lines 483.
- `global_property_cell:938`: 1 uncovered locations; source lines 949.
- `execute_nested_script:1767`: 1 uncovered locations; source lines 1786.
- `unbound_name_resolves:841`: 2 uncovered locations; source lines 852, 855.
- `with_binding_fallback:1900`: 2 uncovered locations; source lines 1907, 1908.
- `can_declare_global_var:361`: 3 uncovered locations; source lines 366, 369, 371.
- `global_var_is_accessor:295`: 1 uncovered locations; source lines 306.
- `prepare_root_execution:115`: 2 uncovered locations; source lines 125, 134.
- `eval_aware_binding_value:1874`: 5 uncovered locations; source lines 1880, 1881.
- `close_iterators_for_return:1069`: 24 uncovered locations; source lines 1076, 1080, 1081, 1082, 1083, 1084, 1085, 1086, 1087, 1089, 1090.
- `dynamic_eval_binding_value:514`: 1 uncovered locations; source lines 526.
- `materialize_lexical_global:314`: 2 uncovered locations; source lines 328, 331.
- `can_declare_global_function:374`: 3 uncovered locations; source lines 379, 380, 381.
- `prepare_global_declarations:222`: 5 uncovered locations; source lines 235, 246, 260, 266, 278.
- `reset_lexical_global_bindings:466`: 2 uncovered locations; source lines 475, 476.
- `close_iterators_to_first_error:1033`: 1 uncovered locations; source lines 1057.
- `materialize_global_object_property:344`: 1 uncovered locations; source lines 356.
- `prepare_eval_global_var_declarations:1708`: 5 uncovered locations; source lines 1714, 1731, 1737, 1752, 1755.
- `store_dynamic_eval_shadowing_binding:552`: 7 uncovered locations; source lines 561, 564, 565.

#### `vm/interpreter.rs`

- Raw gaps: 73 lines, 3 functions, 191 regions.
- `interpret:8`: 186 uncovered locations; source lines 45, 74, 111, 113, 116, 127, 129, 132, 136, 140, 147, 154, 192, 196, 198, 208, 226, 229, 251, 255, 257, 258, 261, 283, 288, 290, 291, 294, 295, 296, 299, 305, 310, 318, 327, 332, 344, 354, 366, 371, 374, 387, 424, 425, 427, 437, 449, 466, 481, 486, 493, 510, 532, 540, 543, 612, 622, 623, 636, 655, 659, 681, 684, 698, 701, 702, 713, 715, 716, 717, 730, 746, 750, 766, 808, 814, 822, 858, 860, 899, 909, 988, 991, 997, 1000, 1007, 1010, 1015, 1016, 1018, 1019, 1020, 1021, 1025, 1079, 1090, 1091, 1098, 1113, 1142, 1150, 1153, 1171, 1175, 1176, 1177, 1217, 1222, 1223, 1224, 1226, 1237, 1238, 1251, 1257, 1262, 1270, 1273, 1278, 1287, 1290, 1295, 1298, 1306, 1314, 1317, 1322, 1327, 1329, 1332, 1334, 1342, 1384, 1413, 1414, 1446, 1447, 1467, 1468, 1487, 1489, 1500, 1505, 1508, 1520, 1533, 1541, 1542, 1571, 1601, 1611, 1618, 1620, 1629, 1633, 1635, 1705, 1726.

#### `vm/builtins/generators.rs`

- Raw gaps: 136 lines, 11 functions, 256 regions.
- `close_async_generator:1918`: 7 uncovered locations; source lines 1922, 1924, 1930, 1936.
- `generator_resume:214`: 33 uncovered locations; source lines 222, 226, 252, 259, 386, 485, 486, 487, 488, 489, 490, 491, 503, 507, 548, 549, 550, 551, 552, 553, 554, 619, 648, 649, 650, 698, 700.
- `async_generator_request:1521`: 8 uncovered locations; source lines 1541, 1552, 1553, 1554, 1557, 1565, 1570.
- `generator_throw:876`: 9 uncovered locations; source lines 881, 882, 883, 884, 886, 894, 898, 917.
- `sync_delegate_step:741`: 1 uncovered locations; source lines 750.
- `throw_into_generator:858`: 2 uncovered locations; source lines 868, 869.
- `generator_delegate_return:805`: 11 uncovered locations; source lines 810, 811, 812, 813, 815, 818, 822, 837, 838, 844.
- `set_async_generator_status:1154`: 5 uncovered locations; source lines 1161, 1162, 1164.
- `await_async_generator_yield:1602`: 4 uncovered locations; source lines 1618, 1619, 1626, 1627.
- `resume_async_generator_next:1212`: 28 uncovered locations; source lines 1219, 1220, 1221, 1222, 1239, 1246, 1253, 1256, 1267, 1270, 1271, 1290, 1294, 1300, 1307, 1323, 1332, 1333, 1334, 1335, 1336, 1352, 1382.
- `finish_async_generator_delegate:1706`: 51 uncovered locations; source lines 1731, 1738, 1746, 1753, 1759, 1764, 1772, 1774, 1786, 1796, 1806, 1807, 1808, 1809, 1810, 1811, 1812, 1813, 1818, 1820, 1832, 1840, 1844, 1845, 1846, 1847, 1848, 1849, 1850, 1851, 1852, 1859.
- `replace_async_generator_request:1402`: 6 uncovered locations; source lines 1409, 1410, 1414, 1417.
- `complete_async_generator_request:1172`: 8 uncovered locations; source lines 1181, 1185, 1187, 1196, 1200, 1205.
- `generator_return:710`: 1 uncovered locations; source lines 715.
- `initialize_generator:21`: 6 uncovered locations; source lines 96, 101, 102, 152, 155.
- `finish_sync_delegation:765`: 11 uncovered locations; source lines 770, 772, 773, 785, 793, 795, 796.
- `finish_async_generator_yield:1680`: 4 uncovered locations; source lines 1689, 1694, 1697, 1702.
- `throw_at_async_delegate_exit:1868`: 12 uncovered locations; source lines 1874, 1876, 1888, 1896, 1905, 1907, 1912.
- `async_generator_delegate_request:943`: 44 uncovered locations; source lines 950, 952, 955, 957, 959, 966, 974, 975, 995, 1002, 1007, 1011, 1025, 1031, 1032, 1041, 1043, 1044, 1055, 1065, 1066, 1067, 1068, 1069, 1070, 1071, 1073, 1081, 1106, 1107.
- `async_generator_is_suspended_start:1389`: 2 uncovered locations; source lines 1393, 1395.
- `resume_after_async_generator_await:1491`: 3 uncovered locations; source lines 1511, 1513, 1515.

#### `vm/shadow_realm.rs`

- Raw gaps: 41 lines, 2 functions, 64 regions.
- `shadow_call_across:461`: 1 uncovered locations; source lines 475.
- `shadow_realm_evaluate:124`: 28 uncovered locations; source lines 176, 179, 180, 181, 182, 186, 187, 190, 193, 194, 195, 196, 197.
- `shadow_realm_prototype:77`: 6 uncovered locations; source lines 82, 83, 93, 100, 107, 111.
- `shadow_realm_constructor:26`: 6 uncovered locations; source lines 35, 36, 38, 45.
- `shadow_realm_import_value:254`: 8 uncovered locations; source lines 274, 279, 319, 323, 333, 342, 356, 367.
- `run_evaluate:209`: 2 uncovered locations; source lines 235, 242.
- `shadow_wrap_into:510`: 1 uncovered locations; source lines 514.
- `shadow_call_wrapped:376`: 3 uncovered locations; source lines 384, 435, 443.
- `copy_name_and_length:568`: 6 uncovered locations; source lines 585, 587, 596, 597.
- `shadow_wrapped_function_create:523`: 3 uncovered locations; source lines 533, 534, 541.

#### `vm/test262/foreign.rs`

- Raw gaps: 75 lines, 11 functions, 256 regions.
- `test262_foreign_completion:720`: 2 uncovered locations; source lines 728, 730.
- `test262_transport_value:382`: 46 uncovered locations; source lines 397, 402, 406, 407, 408, 409, 411, 418, 419, 420, 421, 423, 433, 434, 435, 436, 437, 439, 442, 445, 461, 473, 478, 480, 491, 502, 504, 513, 520, 528, 532, 535, 537, 541, 551, 559, 565, 574, 584, 614, 622.
- `test262_foreign_set:923`: 6 uncovered locations; source lines 938, 951, 956, 962, 968, 974.
- `test262_sync_imported_data_properties:1518`: 37 uncovered locations; source lines 1542, 1545, 1546, 1548, 1549, 1551, 1553, 1558, 1559, 1560.
- `test262_foreign_typed_array_values:1187`: 7 uncovered locations; source lines 1198, 1202, 1207, 1213, 1214, 1215, 1216.
- `foreign_constructor_intrinsic:2157`: 1 uncovered locations; source lines 2177.
- `test262_create_realm:2098`: 5 uncovered locations; source lines 2099, 2107, 2113, 2114, 2115.
- `test262_foreign_date_value:87`: 4 uncovered locations; source lines 94, 95, 96.
- `test262_foreign_regexp_data:68`: 5 uncovered locations; source lines 75, 76, 77, 81.
- `test262_import_foreign_value:113`: 19 uncovered locations; source lines 136, 137, 138, 140, 141, 151, 158, 162, 179, 185, 186, 187, 191, 192.
- `test262_run_native_for_realm:2016`: 1 uncovered locations; source lines 2028.
- `test262_foreign_boxed_primitive:100`: 4 uncovered locations; source lines 107, 108, 109.
- `test262_foreign_native_function:50`: 4 uncovered locations; source lines 59, 60, 61.
- `test262_foreign_default_prototype:260`: 2 uncovered locations; source lines 274.
- `test262_foreign_typed_array_native_call:985`: 10 uncovered locations; source lines 1004, 1006, 1010, 1039, 1040, 1041, 1045.
- `test262_foreign_call:1565`: 26 uncovered locations; source lines 1593, 1609, 1640, 1641, 1643, 1653, 1654, 1656, 1792, 1793, 1794, 1795, 1888, 1895, 1897, 1903, 1904, 1958, 1974, 1982, 2002, 2005, 2007.
- `test262_foreign_next:2043`: 3 uncovered locations; source lines 2050, 2058, 2085.
- `export_foreign_shadow_realm:335`: 2 uncovered locations; source lines 362, 365.
- `test262_export_foreign_value:288`: 1 uncovered locations; source lines 309.
- `test262_foreign_atomics_call:1130`: 6 uncovered locations; source lines 1148, 1149, 1151, 1163, 1167, 1178.
- `test262_foreign_buffer_clone:1227`: 17 uncovered locations; source lines 1252, 1255, 1256, 1257, 1258, 1260, 1263, 1266, 1284, 1286, 1288, 1300, 1304, 1309, 1313, 1315.
- `test262_create_error_in_realm:739`: 2 uncovered locations; source lines 758, 760.
- `test262_foreign_get_prototype:229`: 1 uncovered locations; source lines 249.
- `test262_foreign_set_prototype:833`: 2 uncovered locations; source lines 848, 852.
- `test262_import_foreign_result:215`: 1 uncovered locations; source lines 223.
- `test262_foreign_get_own_property:868`: 4 uncovered locations; source lines 878, 886, 891, 895.
- `test262_foreign_typed_array_info:1387`: 3 uncovered locations; source lines 1398, 1402, 1407.
- `test262_foreign_set_with_receiver:905`: 2 uncovered locations; source lines 915, 916.
- `test262_detach_local_buffer_mirrors:1439`: 3 uncovered locations; source lines 1455, 1457.
- `test262_foreign_define_own_property:666`: 3 uncovered locations; source lines 681, 689, 696.
- `test262_sync_foreign_buffer_mirrors:1350`: 10 uncovered locations; source lines 1362, 1363, 1364, 1368, 1369, 1374, 1375, 1376, 1378, 1379.
- `test262_detach_foreign_buffer_mirrors:1411`: 3 uncovered locations; source lines 1426, 1429.
- `test262_refresh_foreign_buffer_mirrors:1465`: 12 uncovered locations; source lines 1485, 1488, 1489, 1500, 1501, 1502, 1504, 1506, 1507, 1508, 1509.
- `test262_foreign_array_buffer_native_call:1062`: 2 uncovered locations; source lines 1080, 1084.

### D4

#### `vm/builtins/array_from_async.rs`

- Raw gaps: 9 lines, 1 functions, 55 regions.
- `fa_promise:66`: 3 uncovered locations; source lines 67, 69.
- `fa_close_and_reject:399`: 7 uncovered locations; source lines 401, 407, 409, 411.
- `fa_resolve:438`: 1 uncovered locations; source lines 439.
- `fa_mark_done:389`: 3 uncovered locations; source lines 390, 391, 392.
- `fa_element_next:185`: 4 uncovered locations; source lines 186, 187, 188, 198.
- `array_from_async:73`: 1 uncovered locations; source lines 87.
- `fa_iterator_next:162`: 14 uncovered locations; source lines 163, 164, 165, 166, 167, 168, 170, 177.
- `array_from_async_start:93`: 2 uncovered locations; source lines 102, 114.
- `fa_define_and_continue:359`: 3 uncovered locations; source lines 365, 367, 381.
- `fa_get:46`: 1 uncovered locations; source lines 47.
- `fa_step:277`: 13 uncovered locations; source lines 283, 286, 291, 294, 298, 303, 304, 313, 317, 318, 338, 342, 343.
- `fa_number:59`: 2 uncovered locations; source lines 60, 62.
- `fa_reject:429`: 1 uncovered locations; source lines 432.

#### `vm/builtins/execution/setup.rs`

- Raw gaps: 2 lines, 0 functions, 13 regions.
- `iterator_count_helper:1171`: 1 uncovered locations; source lines 1219.
- `async_iterator_dispose:430`: 2 uncovered locations; source lines 468, 473.
- `iterator_zip:621`: 2 uncovered locations; source lines 707, 708.
- `iterator_from:314`: 2 uncovered locations; source lines 325, 341.
- `iterator_zip_collect_padding:879`: 2 uncovered locations; source lines 889, 925.
- `iterator_helper_create:938`: 1 uncovered locations; source lines 949.
- `base_iterator_prototype:29`: 1 uncovered locations; source lines 35.
- `callback_iterator_record:524`: 1 uncovered locations; source lines 534.
- `iterator_wrapper_prototype:223`: 1 uncovered locations; source lines 229.

#### `vm/builtins/execution/runtime.rs`

- Raw gaps: 34 lines, 1 functions, 136 regions.
- `close_concat_on_error:700`: 2 uncovered locations; source lines 706, 708.
- `iterator_zip_next:269`: 18 uncovered locations; source lines 288, 291, 294, 299, 302, 305, 316, 331, 340, 348, 357, 365, 371, 378, 411, 414, 423, 426.
- `iterator_concat_next:166`: 14 uncovered locations; source lines 185, 188, 201, 207, 211, 214, 218, 224, 228, 244, 245, 253, 256, 261.
- `iterator_windows_next:603`: 10 uncovered locations; source lines 622, 626, 629, 638, 644, 652, 666, 684, 687, 692.
- `iterator_find:1167`: 1 uncovered locations; source lines 1178.
- `iterator_some:1144`: 1 uncovered locations; source lines 1155.
- `iterator_step:1607`: 7 uncovered locations; source lines 1613, 1615, 1618, 1622, 1623, 1632, 1649.
- `iterator_every:1121`: 1 uncovered locations; source lines 1132.
- `iterator_reduce:1190`: 2 uncovered locations; source lines 1199, 1200.
- `template_object:1292`: 4 uncovered locations; source lines 1324, 1325, 1330, 1332.
- `iterator_zip_count:469`: 4 uncovered locations; source lines 474, 477, 479.
- `async_iterator_step:1572`: 3 uncovered locations; source lines 1578, 1587, 1600.
- `iterator_zip_return:989`: 6 uncovered locations; source lines 1009, 1016, 1019, 1024, 1030, 1033.
- `iterator_chunks_next:538`: 5 uncovered locations; source lines 557, 575, 587, 590, 595.
- `iterator_helper_next:8`: 21 uncovered locations; source lines 17, 50, 53, 58, 71, 75, 80, 83, 86, 89, 92, 101, 105, 125, 126, 127, 145, 152, 155, 158.
- `iterator_concat_return:954`: 3 uncovered locations; source lines 973, 975, 984.
- `iterator_helper_return:897`: 4 uncovered locations; source lines 906, 932, 935, 948.
- `async_from_sync_continue:1480`: 2 uncovered locations; source lines 1494, 1540.
- `iterator_zip_keyed_results:435`: 3 uncovered locations; source lines 447, 449, 458.
- `iterator_next:1556`: 4 uncovered locations; source lines 1562, 1564, 1567, 1568.
- `iterator_close:1654`: 4 uncovered locations; source lines 1661, 1664, 1665, 1669.
- `iterator_callback:1042`: 2 uncovered locations; source lines 1048, 1049.
- `async_iterator_next:1429`: 5 uncovered locations; source lines 1435, 1437, 1440, 1441, 1444.
- `mark_async_from_sync:1388`: 1 uncovered locations; source lines 1393.
- `async_from_sync_handler:1458`: 1 uncovered locations; source lines 1462.
- `iterator_helper_callback:719`: 3 uncovered locations; source lines 726, 729, 747.
- `iterator_zip_close_records:482`: 1 uncovered locations; source lines 492.
- `iterator_constructor_setter:1260`: 1 uncovered locations; source lines 1270.
- `iterator_to_string_tag_setter:1231`: 3 uncovered locations; source lines 1241, 1250, 1255.

#### `vm/modules/namespace.rs`

- Raw gaps: 20 lines, 3 functions, 34 regions.
- `exported_names:120`: 6 uncovered locations; source lines 129, 130, 133, 149, 151.
- `resolve_export:11`: 6 uncovered locations; source lines 46, 59, 72, 80, 98, 99.
- `module_namespace:168`: 22 uncovered locations; source lines 184, 185, 186, 187, 192, 194, 204, 211, 227, 238, 239, 242, 256, 257, 258, 259, 263, 264, 265, 266.

#### `vm/modules/deferred.rs`

- Raw gaps: 9 lines, 0 functions, 31 regions.
- `with_module_records:232`: 2 uncovered locations; source lines 245, 246.
- `dynamic_import_defer_job:59`: 2 uncovered locations; source lines 69, 75.
- `scc_evaluated:93`: 2 uncovered locations; source lines 112, 113.
- `cycle_root_error:329`: 2 uncovered locations; source lines 341, 342.
- `evaluate_module_sync:255`: 11 uncovered locations; source lines 260, 269, 281, 285, 286, 291, 293.
- `module_source_object:354`: 1 uncovered locations; source lines 380.
- `dynamic_import_source:19`: 2 uncovered locations; source lines 46, 49.
- `ready_for_sync_execution:201`: 4 uncovered locations; source lines 211, 213, 221, 222.
- `gather_async_dependencies:162`: 5 uncovered locations; source lines 173, 178, 188, 189, 192.

#### `vm/modules.rs`

- Raw gaps: 19 lines, 0 functions, 35 regions.
- `evaluate_module_record:2198`: 2 uncovered locations; source lines 2333, 2336.
- `resume_async_await:1828`: 1 uncovered locations; source lines 1906.
- `resume_module_await:1464`: 3 uncovered locations; source lines 1550, 1580, 1581.
- `suspend_async_frame:1781`: 1 uncovered locations; source lines 1796.
- `suspend_module_await:1724`: 2 uncovered locations; source lines 1733, 1739.
- `resume_async_generator_await:1936`: 1 uncovered locations; source lines 2059.
- `resume_debugger_module_inner:754`: 24 uncovered locations; source lines 766, 853, 856, 860, 879, 893, 895, 912, 915, 926, 927, 931, 946, 949, 952.

#### `vm/builtins/object.rs`

- Raw gaps: 35 lines, 2 functions, 231 regions.
- `proxy_define_own_property:1353`: 14 uncovered locations; source lines 1359, 1360, 1362, 1366, 1377, 1390, 1393, 1394.
- `proxy_revocable:919`: 3 uncovered locations; source lines 929, 930, 938.
- `descriptor_object:1241`: 2 uncovered locations; source lines 1246, 1258.
- `proxy_get_own_property:1267`: 4 uncovered locations; source lines 1278, 1282, 1299, 1300.
- `validate_function_realm:792`: 12 uncovered locations; source lines 799, 800, 811, 814, 815.
- `proxy_get:982`: 9 uncovered locations; source lines 988, 989, 991, 995, 1011.
- `proxy_call:1584`: 5 uncovered locations; source lines 1591, 1592, 1597, 1601, 1611.
- `proxy_delete:1131`: 12 uncovered locations; source lines 1136, 1137, 1139, 1143, 1154, 1157, 1158, 1162.
- `object_delete:163`: 4 uncovered locations; source lines 168, 173, 175, 182.
- `is_constructor:821`: 4 uncovered locations; source lines 831, 834, 837, 841.
- `proxy_own_keys:1167`: 13 uncovered locations; source lines 1171, 1172, 1174, 1178, 1191, 1192, 1195, 1215, 1219, 1230.
- `proxy_constructor:891`: 2 uncovered locations; source lines 907, 908.
- `get_from_prototype:511`: 3 uncovered locations; source lines 540, 544, 560.
- `intrinsic_prototype:575`: 4 uncovered locations; source lines 580, 582, 585, 588.
- `proxy_get_prototype:1456`: 6 uncovered locations; source lines 1460, 1461, 1467.
- `proxy_is_extensible:1425`: 9 uncovered locations; source lines 1429, 1430, 1432, 1436, 1447, 1448.
- `proxy_set_prototype:1506`: 11 uncovered locations; source lines 1511, 1512, 1513, 1514, 1515, 1522, 1536.
- `object_set_prototype:250`: 7 uncovered locations; source lines 279, 280, 282, 286, 295, 300.
- `object_get_own_property:37`: 9 uncovered locations; source lines 48, 59, 63, 64, 66, 76.
- `object_own_property_keys:186`: 4 uncovered locations; source lines 190, 197, 212, 215.
- `proxy_prevent_extensions:1547`: 10 uncovered locations; source lines 1551, 1552, 1553, 1555, 1559, 1570, 1573.
- `constructor_prototype_for:601`: 21 uncovered locations; source lines 621, 623, 626, 628, 696, 703, 733, 735, 737, 739, 741, 743, 745, 747, 749, 751, 753, 757, 759, 778, 782.
- `object_prevent_extensions:304`: 4 uncovered locations; source lines 314, 315, 327, 330.
- `object_define_own_property:86`: 3 uncovered locations; source lines 92, 96, 100.
- `ordinary_set_with_receiver:337`: 23 uncovered locations; source lines 344, 364, 367, 370, 375, 386, 393, 394, 408, 424, 425, 430, 439, 440, 445, 448, 450, 458, 464, 465, 477, 492, 505.
- `typed_array_define_own_property:110`: 5 uncovered locations; source lines 119, 120, 123, 131, 157.
- `proxy_has:1042`: 10 uncovered locations; source lines 1047, 1048, 1050, 1054, 1065, 1067, 1068.
- `proxy_set:1081`: 11 uncovered locations; source lines 1088, 1089, 1091, 1095, 1111, 1114.

#### `vm/regexp.rs`

- Raw gaps: 32 lines, 0 functions, 107 regions.
- `regexp_global:11`: 22 uncovered locations; source lines 15, 16, 19, 20, 23, 31, 39, 47, 55, 62, 79, 94, 114, 122, 123, 137, 141, 142, 143.
- `regexp_allocate:244`: 6 uncovered locations; source lines 254, 277, 279, 298, 302, 304.
- `regexp_iterator_prototype:950`: 11 uncovered locations; source lines 954, 955, 956, 957, 958, 966, 974, 978.
- `regexp_exec:636`: 19 uncovered locations; source lines 644, 661, 665, 670, 675, 676, 717, 720, 721, 729, 737, 742, 750, 757, 762, 770, 774, 778, 779.
- `regexp_slots:225`: 2 uncovered locations; source lines 231, 233.
- `regexp_split:1017`: 2 uncovered locations; source lines 1059, 1083.
- `regexp_create:197`: 1 uncovered locations; source lines 202.
- `regexp_getter:483`: 5 uncovered locations; source lines 509, 519, 521, 580, 587.
- `regexp_method:784`: 12 uncovered locations; source lines 796, 824, 847, 878, 880, 886, 895, 900, 901, 902, 903, 906.
- `regexp_compile:315`: 4 uncovered locations; source lines 321, 331, 352, 354.
- `regexp_replace:1099`: 9 uncovered locations; source lines 1105, 1123, 1133, 1134, 1146, 1150, 1183, 1184, 1192.
- `regexp_legacy_get:447`: 1 uncovered locations; source lines 452.
- `regexp_legacy_set:466`: 2 uncovered locations; source lines 473, 478.
- `advance_last_index:913`: 1 uncovered locations; source lines 919.
- `regexp_constructor:207`: 1 uncovered locations; source lines 216.
- `capture_substitution:1196`: 1 uncovered locations; source lines 1212.
- `regexp_iterator_next:985`: 1 uncovered locations; source lines 991.
- `install_legacy_accessors:360`: 6 uncovered locations; source lines 387, 396, 404, 417, 425, 439.
- `regexp_species_constructor:928`: 1 uncovered locations; source lines 939.

#### `vm/temporal/conversion/from_value.rs`

- Raw gaps: 14 lines, 0 functions, 22 regions.
- `alloc_temporal_value:426`: 2 uncovered locations; source lines 434, 438.

#### `vm/temporal/duration_relative.rs`

- Raw gaps: 1 lines, 0 functions, 3 regions.
- `temporal_duration_receiver:20`: 1 uncovered locations; source lines 27.
- `temporal_duration_relative_to:196`: 1 uncovered locations; source lines 204.
- `temporal_duration_plain_endpoints:652`: 1 uncovered locations; source lines 703.

#### `vm/temporal/plain_date_time_difference.rs`

- Raw gaps: 1 lines, 0 functions, 8 regions.
- `nudge_position:469`: 1 uncovered locations; source lines 486.
- `compute_nudge_window:383`: 3 uncovered locations; source lines 392, 415, 416.
- `difference_iso_date_time:272`: 1 uncovered locations; source lines 294.
- `difference_plain_date_time:101`: 1 uncovered locations; source lines 113.
- `difference_plain_date_time_total:221`: 1 uncovered locations; source lines 230.

#### `vm/temporal/zoned_difference.rs`

- Raw gaps: 3 lines, 0 functions, 21 regions.
- `compute_nudge_window:320`: 5 uncovered locations; source lines 335, 366, 367, 374, 391.
- `resolve:130`: 1 uncovered locations; source lines 134.
- `difference_zoned:238`: 1 uncovered locations; source lines 255.
- `nudge_to_zoned_time:501`: 6 uncovered locations; source lines 514, 515, 519, 520, 521, 542.
- `difference_with_total:213`: 1 uncovered locations; source lines 225.
- `nudge_to_calendar_unit:430`: 3 uncovered locations; source lines 449, 452, 453.
- `bubble_relative_duration:557`: 2 uncovered locations; source lines 597, 598.
- `difference_with_rounding:175`: 1 uncovered locations; source lines 195.

#### `vm.rs`

- Raw gaps: 0 lines, 0 functions, 22 regions.
- `install_host_method:1187`: 1 uncovered locations; source lines 1199.
- `install_host_callable:1207`: 1 uncovered locations; source lines 1216.
- `install_host_function:1144`: 2 uncovered locations; source lines 1151, 1154.
- `enter_call:1548`: 3 uncovered locations; source lines 1568, 1573, 1602.
- `install_host_object:1165`: 4 uncovered locations; source lines 1167, 1170, 1176, 1180.
- `install_host_callable_native:1222`: 1 uncovered locations; source lines 1229.
- `dispatch_call:1630`: 4 uncovered locations; source lines 1641, 1683, 1685, 1699.
- `function_prototype:1291`: 1 uncovered locations; source lines 1294.
- `has_own_property_intrinsic:1491`: 1 uncovered locations; source lines 1497.
- `property_is_enumerable_intrinsic:1469`: 1 uncovered locations; source lines 1475.

## R24 complete measurement and next batch analysis (2026-10-05)

R24 passed all 4,044 Rust tests and all 102,921 applicable Test262 modes, with no changed outcome contracts. All 95 original complete files remain complete. Twelve selected files and 29 modified files meet the raw criterion; the following 16 modified files still need work. Source-location unions are diagnostic only.

| Production file | Missing raw lines | Missing raw functions | Missing raw regions |
|---|---:|---:|---:|
| `page_runtime.rs` | 8 | 0 | 23 |
| `vm.rs` | 0 | 0 | 1 |
| `vm/builtins/dynamic.rs` | 0 | 0 | 3 |
| `vm/builtins/execution/setup.rs` | 0 | 0 | 2 |
| `vm/builtins/generators.rs` | 9 | 0 | 31 |
| `vm/builtins/globals.rs` | 0 | 0 | 1 |
| `vm/builtins/object.rs` | 2 | 0 | 15 |
| `vm/builtins/promises.rs` | 3 | 0 | 11 |
| `vm/builtins/typed_arrays.rs` | 6 | 1 | 67 |
| `vm/errors.rs` | 0 | 0 | 13 |
| `vm/execution.rs` | 15 | 0 | 43 |
| `vm/interpreter.rs` | 13 | 0 | 57 |
| `vm/intl.rs` | 1 | 0 | 1 |
| `vm/modules.rs` | 3 | 0 | 6 |
| `vm/regexp.rs` | 0 | 0 | 2 |
| `vm/test262/foreign.rs` | 5 | 0 | 26 |

The unchanged agent queue also needs preservation of one previously covered
blocking-wakeup region. Its additional existing debt is not claimed complete.

The next batch covers two distinct kinds of gap. Page runtime boundary fixtures
currently run only in the unit-test crate; they must also run against the
ordinary library instance. Generic live instantiations are inspected against
raw counters rather than counted from a source-location union. Iterator wrapper
and helper prototypes need root-registration refusal in addition to their
existing heap-budget matrix. String publication needs a warm intrinsic with a
cold global cache. TypedArray coverage includes pre-existing debt because the
mirror synchronization signature made that production file part of this task.

TypedArray review found that a freshly allocated buffer needs retention during
cold concrete-constructor initialization. Its copy builder will retain the
buffer and restore the stack on success and failure. Valid typed element reads
are pure after receiver validation, including detached or resized views that
produce undefined. Callback truthiness checks receive values validated by the
realm's call boundary. The callback dispatcher can use a closed enum; default
sort inputs have the Number or BigInt type guaranteed by storage decoding.
Fallible coercion, species construction, immutable writes, range checks and
observable callback failures remain fallible. New fixtures exercise owned
views, live handles from another heap at fallible ingress, checked-range
overflow, cold constructors under GC and allocation refusal, and string limits.

No runtime test has run during preparation of this next batch. Its eventual
verification must use new frozen sources, binaries, maps and profiles.

### R25 prepared correction batch

LLVM 22 merges function instantiations using separate maxima for covered
regions and region counts. A union of source locations can hide a branch
missing from the most covered instantiation. The R24 maximum calculation
reconciles exactly with every audited file's raw region summary. The retained
`best-instantiation-gaps.json` records that calculation; the coverage helper
now exposes the same diagnostic and rejects unreconciled counts. Raw LLVM
summaries remain the completion criterion.

Root traversal is now a non-generic function in `vm/execution.rs`, so different
allocation callback types execute the same complete traversal. Host function
installation fixtures use the same callback type for cold failure and retry.
Page runtime contracts execute in the ordinary instrumented library as well
as its unit binary, and graph fixtures use the same container type for valid
entries and refusal paths.

The production correction batch also roots fresh TypedArray backing stores
across cold constructor initialization, restores temporary roots when RegExp
pattern fallback getters fail, and waits for agent workers before collecting
shutdown errors. The agent host is therefore an additional modified
production file and must meet the same full raw coverage requirement.

Prepared regression cases cover byte and root registration refusal, async
eval reference markers, cold global publication and `this` conversion,
existing string values after a limit change, delegated generator completion,
queued Promise jobs, cold stack setters, Proxy argument and descriptor
allocation, AggregateError payloads, Map and WeakMap upsert payloads,
deferred namespace reflection, paused and awaited module error construction,
real membrane prototype traps, immutable buffer detach refusal, mirror
resize and detach transitions, agent worker initialization, parse and compile
errors, shutdown errors, wait events, and host timer failures.

All runtime verification follows the completed source and fixture batch.
R25 is prepared work, not a measured completion claim. Static compilation
and lint corrections may precede its source freeze; runtime profiles must
come only from its final frozen source set.

### R25 critical failures and complete R26 correction

R25 completed all 17 critical targets with 1,408 passes and 15 failures across
three targets. Remaining targets, Test262 and coverage export were not run.
Profiles from this failed source snapshot are excluded from completion.

The production failures identify two cleanup defects and a compiler scope
defect. Object.assign retained its target when source boxing failed; its
coercing dispatcher now restores the original stack on every outcome.
RegExp replace similarly restores the retained matches on every return.
The compiler previously excluded async functions from the separate sloppy
direct-eval environment, permitting caller aliases to affect unrelated
closures. Async functions now receive the same environment as other sloppy
functions containing direct eval; their continuation already retains it.
The original failing public regression is preserved. An owned compiled-eval
contract separately checks static-cell aliases, compound and postfix updates,
original capture storage, cleanup and successful VM reuse.

Captured binding ingress remains fallible because a caller can supply a live
cell from another heap. Restoring propagation preserves this existing boundary
instead of treating foreign captures as an internal invariant.

Fixture corrections set instruction fuel before private helpers, keep a weak
target strongly reachable until collection is requested, avoid a tail-call
entry in a graph contract, exercise Proxy reflective traps explicitly, and
expect brand and immutable-buffer errors through their actual conversion.
The foreign budget matrix distinguishes operations that allocate in a child
from pure child queries while requiring real parent import refusals. Symbol
coercion into a numeric TypedArray exercises foreign TypeError construction.
RegExp output-limit inputs distinguish valid templates from accumulated output.

All 81 task Rust files are prepared before testing. Both newly modified
production files, the agent host and compiler functions, join the full raw
coverage gate, bringing the modified set to 47. R26 requires fresh frozen
sources, binaries, profiles and maps; no result is claimed during preparation.

### R26 refusal tracking findings and R27 correction

All 17 R26 critical targets completed with 1,422 passes and two failures.
Both failures are the same refusal-tracking defect in the unit and ordinary
library allocation matrix. All prior R25 failure cases pass. No remaining
harnesses, Test262 or coverage export ran after the failed critical gate.

The heap emits HeapLimitExceeded from ensure_room and the direct ArrayBuffer
resize budget check. The adaptive test-only tracker recorded the former but
not the latter. Resize now records its actual required byte count under the
same test/coverage configuration before returning its existing refusal.
No production error or success behavior is changed. The existing retained
buffer matrix exercises direct resize failure and subsequent VM reuse.
All changes precede fresh R27 static gates and source freezing. The new heap
source joins the full raw gate: 82 task Rust files, 48 production files.

### R27 complete Rust cohort and R28 species ingress correction

All 334 targets completed with 4,067 passes, one failure and four existing
ignored tests in 1,038.183 seconds. All 17 critical targets pass. The sole
failure is the retained public foreign TypedArray species contract; Test262
and coverage export did not run after the failed Rust gate.

A foreign species constructor can return a plain object with a sufficient
length or a detached facade. The first local typed_array_info call therefore
validates an observable user result and must propagate HeapError through
RuntimeError rather than assert its brand. That initial read is restored to
fallible validation. Later metadata reads occur after validation without a
brand-changing user callback and retain their proven preconditions.
The retained 17-case public suite joins the next critical gate. A unit and
ordinary-library fixture additionally checks real plain and detached foreign
species, default and one-object nursery collection, TypeError propagation,
stack cleanup and successful VM reuse.

Review of with's decoded replacement confirms that typed_array_set_index
returns Ok(false) for an index outside the newly allocated target, so source
growth beyond the initially captured copy length remains a harmless no-op.
The existing growth contracts are preserved. No synthetic heap state or
replacement of the failing public expectations is used. All R28 source and
fixture changes precede consolidated verification; the acceptance set remains
22 selected files, 48 modified files and all 95 formerly complete files.

## R28 verified progress and remaining cohort (2026-10-05)

All 334 Rust targets pass 4,069 tests with zero failures and four existing
ignored tests. All 18 critical targets pass 1,442 tests. Full Test262 passes
all 102,921 applicable modes in 274.207 seconds, and all 102,926 baseline
outcome contracts are preserved. Fresh atomic LLVM 22 profiles show no
counter underflow and zero coverage percentage regressions.

Completion is 119/162 instrumented files, 14/22 selected files (four D5 and
ten D4), and 36/48 modified production files. All 95 original complete files
remain complete. Newly complete files since R24 are vm.rs,
vm/builtins/dynamic.rs, vm/builtins/execution/setup.rs,
vm/builtins/globals.rs and vm/intl.rs. The earlier R24 classification is
corrected to four D5 and eight D4, with its 12/22 total unchanged.

The remaining 12 modified files miss 32 raw lines and 123 regions, with no
unexecuted functions. All remain in the task acceptance scope after progress
commits. The R28 acceptance-scope.json retains the cumulative 48-file set and
baseline commit; subsequent audits must not derive the scope only from an
empty or smaller post-commit working-tree diff. Before another runtime run,
review the best-instantiation gaps against their frozen sources, preserve
fallible ingress and observable callbacks, and finish the whole correction
cohort. Raw summaries, not source unions, determine completion.

## Prepared correction cohort after the R28 measurement

R28 remains the complete measured baseline. This cohort introduces no new
coverage claim until its frozen source and executable maps pass the complete
verification. The preparation covers all twelve remaining modified-file groups;
existing semantic assertions and resource-error propagation are retained.

| Boundary | Prepared contract or implementation review |
| --- | --- |
| Page module execution | Nested graph validation, linked dependency omission, actual body throws and subsequent classic execution use the existing generic handle inventory. |
| Generator and Promise reactions | Delegation completion only decreases the retained frame's accounted edges. Completed-generator return reactions retain their private iterator-result growth refusal. Staged native errors exercise delegate close and resumed finally paths. Synchronous delegate callbacks remain fallible. |
| TypedArray storage | Decoded primitive conversion and copy/write steps following validation cannot invoke callbacks. Foreign buffer accessors and reverse destination writes retain real detachment and immutability errors. Normal and minimal nurseries verify cross-realm slice values and detached source behavior. |
| Object and Error descriptors | Public foreign prototype traps, Proxy deletion, numeric prototype revalidation, lazy deletion refusal and reverse Error cause-descriptor exceptions preserve observations and cleanup. AggregateError allocation sweeps retain both errors-array and property publication refusals. |
| Execution and interpreter | Actual compiled eval captures exercise dynamic shadowing, retained references, growing binding refusal and original-cell preservation. Source property keys are converted once before target evaluation. Compiler-prepared property reads, deletes and updates retain validated bases and canonical keys; simple assignment and raw destructuring target keys remain fallible. Staged name, with, iterator-close, rest, derived-this and cold RegExp cases use valid compiled programs. |
| Module scheduling | An actual second top-level await exhausts continuation identifiers. Immediate and waited deferred imports preserve allocation refusal and defer their body errors until namespace access. A debugger resume preserves an unsettled top-level await and its eventual export cell. Namespace Promise resolution reads initialized completed exports or the absent deferred `then`; callable exports execute in later fallible jobs. Async rejection removes its verified FIFO entry and settles its tracked Promise without heap allocation. |
| Foreign membrane | Boolean completion instantiations retain import refusal. Child native fill grows its real backing during coercion; parent mirror refresh preserves resize refusal and retry. Host detach retains ordinary/shared/immutable validation and synchronizes real buffer identities. |
| Agent scheduler | Setup garbage is collected before refusing a warmed receive wrapper. A real FinalizationRegistry cleanup throw is reported by the worker's idle job loop. |

The self-test runner executes the tooling contracts, retained critical Rust
targets, remaining selected Rust targets and Test262 in that order. Every group
must complete successfully before the next starts. A critical failure therefore
cannot spend the cost of the remaining suite or Test262. Reversed input order is
covered by a subprocess contract. A conservative complete affected selection
uses its fresh execution once as the full evidence; no older profiles or failed
snapshots contribute to completion. Canonical report publication remains gated
on a successful complete run with unchanged source provenance.

All production and fixture changes precede formatting, workspace Clippy and the
next frozen runtime verification. The canonical macOS report continues to
contain the R28 counters until a new complete measurement is available.

The first graph-driven frozen gate completed nine Rust targets with 1,194
passes and 16 failures; later Rust targets, Test262, workspace runtime and raw
coverage were skipped. Its unit target completed all 1,115 cases with 1,105
passes and ten failures. Thirteen failures across the two failing targets stem
from the same regex-worker startup prerequisite; three new unit contracts expose
separate findings. Artifacts remain in
`target/bluejs-selftest/runs/20261006-090641-e6d7cd8a/` and cannot establish
completion. The canonical report remains the full R28 measurement.

The Page fixture now imports its actual linked dependency. Proxy Set delegates
to its target, so absent set traps intentionally skip descriptor and prototype
traps; separate ordinary prototype lookups exercise genuine lazy descriptor and
foreign prototype allocation refusals. A frozen-adapter diagnostic isolates the
TypedArray failure to the differing-kind copy under a one-object nursery. The
new argument array was collected while a foreign set-method lookup allocated a
facade. The complete correction retains the argument and constructed receiver
through lookup and invocation, then truncates temporary roots on either outcome.
Public cases retain default/minimal nursery checks, explicit parent collection
from a foreign getter and the getter's thrown-value identity.

The runner now checks its exact regex worker with an empty-input READY handshake
before any cases, retains readiness profiles outside runtime coverage, and passes
the built path explicitly. No production timeout is extended. A complete affected
selection exports graph observations once after the full gate. All corrections
are prepared before the next frozen verification.

The second graph-driven gate completed all 18 critical Rust targets with 1,452
passes and four failures: two new fixture prerequisites reproduced in both the
unit and ordinary-library targets. Regex readiness and all TypedArray collection,
getter-throw and resource-refusal cases pass. Later Rust targets, Test262,
workspace runtime and raw coverage were skipped. Its artifacts remain in
`target/bluejs-selftest/runs/20261006-092847-35bec52b/`; R28 remains the complete
measured snapshot.

The Page fixture's nested function now retains an ordinary call instead of a
tail call, which the nested debugger intentionally rejects during validation.
The Object fixture retains a live child realm and uses its fresh ordinary
prototype to provoke a real parent-heap facade allocation refusal during cycle
validation. It verifies the untouched target prototype, temporary-root cleanup
and a successful retry. Clearing a private realm registry violated the facade's
producer invariant and is removed from this new fixture. Both corrections are
prepared together before static gates and the next frozen runtime verification.

## Latest complete graph-driven measurement (2026-10-06)

The frozen `47277a1683d6b8bad7693a9392e7207969ccac157515bf589da45b6adfd1c385`
snapshot completes all 334 Rust targets with 4,083 passes, zero failures and
four existing ignored tests. All 18 critical targets pass 1,456 cases. Full
Test262 passes 102,921 applicable modes in 369.950 seconds, with the four
existing exclusions and one stale fixture preserved. All 102,926 outcome
contracts are unchanged. Remaining workspace runtime tests pass 2,170 cases
with 60 existing ignored tests; BlueJS Rustdoc passes both cases. All 82
cumulative Rust task files pass explicit formatting, workspace Clippy passes
with warnings denied, and the 20 existing workspace formatting findings are
unchanged by hash and from HEAD.

Fresh raw production coverage completes 122/162 files, 16/22 selected files
(five D5 and eleven D4) and 39/48 cumulative modified production files.
All 95 original complete files remain complete. Newly complete files are
page_runtime.rs, vm/modules.rs and vm/builtins/promises.rs. The remaining
nine modified files miss eleven lines and 43 regions, with no missing
functions. The canonical English report contains only this complete snapshot.
No commit is eligible yet: regex_worker.rs loses its sibling-discovery counters
when every test receives an explicit worker path. The next prepared batch
restores both Cargo deps and adapter discovery observations, retains a
preservation reference above that decrease, and covers the remaining boundaries
before another consolidated runtime run.

| Remaining modified source | Missing lines | Missing regions | Producer boundary to review |
| --- | ---: | ---: | --- |
| vm/interpreter.rs | 2 | 20 | Super roots, derived-this cells, RegExp literals, actual iterator rest allocation and close, eval/with references, global deletion, direct-eval result limits and raw destructuring target keys. |
| vm/builtins/generators.rs | 1 | 9 | Reentrant synchronous delegation, native-error materialization after async resume, retained FIFO request replacement and private completion results. |
| vm/test262/foreign.rs | 3 | 5 | Detached mirror skips, backing-size changes, real transported detach refusal and foreign result import cleanup. |
| vm/execution.rs | 2 | 3 | Writable binding stores, dynamic-eval writes and parameter-environment recreation. |
| vm/builtins/object.rs | 1 | 1 | SameValue numeric prototype writes with invalid canonical indices after coercion. |
| vm/regexp.rs | 0 | 1 | Cold constructor identity lookup after priming the preceding pattern and own-constructor queries. |
| vm/builtins/typed_arrays.rs | 1 | 1 | A validated mutable transported snapshot after a successful real backing write; verify its range and no-callback guarantees. |
| vm/errors.rs | 0 | 2 | AggregateError errors-array creation and property publication after observable iterator steps. |
| vm/test262_agents.rs | 1 | 1 | Received wrapper allocation after priming the actual constructor/prototype and collecting setup garbage. |

The previous temporary corpus contains directories but no regular files. The
same pinned archive was fetched into persistent workspace artifacts and passed
both archive and manifest checks. The undispatched configuration refusal and
its log are retained; no JavaScript executed in that refused command. The same
unchanged source and all 334 passing executable hashes allowed verification to
continue without repeating Rust cases. The raw export initially contained ten
audited test-only source files; production scope uses the exact pre-existing
coverage_file.py classification, preserving all LLVM production file summaries.
No failed Rust, older epoch, loader or readiness profile contributes to coverage.

Complete artifacts, source hashes, the prepared patch, raw export, outcome
contracts and maximum-instantiation diagnostics are retained in
`target/bluejs-selftest/runs/20261006-094234-eeb3f687/`. Tool corrections and all
remaining source/fixture corrections are prepared together before further
tests. The measured counters do not verify those later working-tree changes.

### Prepared boundaries for the next complete measurement

The next frozen batch addresses the nine modified files together. Synchronous
delegation rejects reentrant next, return and throw while getters, callbacks
and iterator-result getters observe its saved frame. Async return marks the
FIFO head executing before PromiseResolve can invoke a constructor getter;
refusal restores the scheduler status before error materialization. Contracts
exercise actual compiled generators, explicit collection and queued request
ordering under ordinary and one-object nurseries.

Compiler-owned canonical property keys and ordinary super homes are checked
against their producers. Observable coercions, foreign brands, getters,
allocation growth and callbacks retain their fallible boundaries. Execution
fixtures use real compiled eval captures and public debugger pauses at emitted
rest instructions to isolate refusal without fabricated bytecode or corrupting
private state. AggregateError publication, cold RegExp constructor identity,
agent wrapper creation, numeric prototype writes and third-Realm buffer
detachment have public regression cases.

Cross-Realm TypedArray construction validates internal length and bounds
instead of reading an overridable length property, following
[TypedArrayCreateFromConstructor](https://tc39.es/ecma262/multipage/indexed-collections.html#sec-typedarraycreatefromconstructor).
Forward results use the owner's slots; reverse snapshots use ordinary local
validation. Short, detached, non-TypedArray and immutable write targets remain
errors. Getters that throw or detach must not run during this validation. A
successful reverse backing write cannot invoke a callback before the validated
mutable snapshot receives the same bytes.

The runner validates every pinned corpus file before any build, retains the
highest previously verified percentage for each raw metric as a preservation
threshold, and imports graph relationships from successful UI rounds without
rerunning cases. Evidence is bound to executable, profile and source hashes.
Worker-reuse tests and the standalone adapter exercise normal worker discovery;
other harnesses use the exact worker whose READY handshake succeeded. Readiness
profiles remain outside completion coverage. Reports preserve the established
tables from one full snapshot and require matching formatting, diff and
workspace Clippy evidence before declaring the requested gates satisfied.

These changes precede runtime verification. Only a new successful complete
measurement may replace the canonical report or establish completion.

The frozen critical gate in
`target/bluejs-selftest/runs/20261006-112854-e12c847a/` completed all 18 targets:
1,461 checks passed and three failed. The unit suite passed 1,122 checks with
one failure; the ordinary driver duplicates that execution failure. Remaining
Rust targets, Test262, workspace runtime and coverage export were skipped.
This failed round contributes no completion counters.

The compiler's assignment-pattern leaf omitted object-environment resolution
inside with, so a for-of assignment skipped its setter. The correction uses
the existing ResolveWithReference and StoreResolvedWithReference pair for
identifier leaves, retaining lexical shadowing and fallback bindings. Public
for-in/of and array/object assignment cases verify setter throws and VM reuse.
The compiler expression source joins the cumulative production acceptance set.

The existing foreign species suite also expected two non-TypedArray objects'
length getter/coercion to execute. Internal-slot validation must reject those
objects before either observation. Those cases now require TypeError, while
the original getter-7 and coercion-8 assertions are retained through actual
observable species-constructor evaluation. All other existing assertions and
the newly passing internal-length contracts remain in place. This complete
correction precedes the next frozen gate; the canonical report is unchanged.

The second frozen gate, retained in
`target/bluejs-selftest/runs/20261006-114357-84ab7c6d/`, passes 1,462 checks and
fails two duplicated execution contracts. The species suite and with assignment
checks pass. The new recursion fixture had no base case and therefore reached
the normal call-depth limit before its iterator could close. It now performs
one finite recursive call before testing the iterator's throwing return.
Independent cases in the interpreter matrix collect all assertion failures
before reporting the contract result, so one failed case cannot hide later
cases from the same prepared matrix.

The runner now rechecks previously failed named cases in their owning targets
before building the full affected inventory. That phase uses exact Rust filters,
fresh separate profiles and the same frozen source. It preserves the required
affected and full gates after a successful recheck; its partial profiles never
enter the completion export. Ignoring a previously failing case stops the gate;
a removed case defers verification to the complete current target. These changes
are prepared together before the next measurement.

### Global reference correction and Test262 failure selection

The latest failed round retains 4,091 passing Rust checks, four existing ignored
checks and 17 Test262 failures in 12 fixture files. It contributes no completion
coverage. The ten semantic failures are the five destructuring-default fixtures
in both modes; seven sloppy Date DST modes exceeded their existing wall deadline.
The canonical report continues to describe the latest complete measurement.

ResolveWithReference now includes the realm's global Environment Record after
object and active-slot lookup. Its marker preserves that record across an RHS
eval, including global lexical TDZ/const checks and object-property deletion.
A saved unresolvable sloppy reference writes the global object even when the
RHS has introduced a local eval binding. Public scripts exercise these boundaries
under default and one-object nurseries; the operations source joins the cumulative
50-file acceptance scope.

Failure selection reads retained Test262 path/mode outcomes, builds only the
adapter and worker, and rechecks those paths with unchanged metadata and budgets.
Every previously failed mode must pass, including timeout rows. Missing or newly
excluded modes cannot clear a failure. Passing complete Rust targets clear older
case failures. Recheck profiles stay separate from complete measurement profiles.

Explicitly budgeted long fixtures run after ordinary fixtures with at most two
workers. Scheduling changes preserve all inventory identities and each original
deadline and instruction budget. Summaries retain group sizes and concurrency;
dispatched rows now retain execution times. The next frozen verification will
establish whether reduced contention resolves the Date timeouts, before any full
Rust and Test262 execution is allowed.

### Public Reference contract selection

The full run `20261006-132424-1b6def25` stopped after the operators target
exposed an older assertion that redirected an already-unresolvable reference
into an eval variable introduced by the assignment's right operand. The run
retains 108 completed Rust targets, 2,253 passes and one failure; four active
targets were cancelled to avoid additional failed-epoch cost. Test262,
workspace runtime and full coverage were not executed.

The original script and allocation sweeps remain. Its assertion now requires
the eval variable to remain `1` and the global property to contain `xxxx`,
following [PutValue](https://tc39.es/ecma262/multipage/ecmascript-data-types-and-values.html#sec-putvalue).
This corrects the fixture's semantic expectation, without redirecting the
saved reference again. The production source snapshot is unchanged.

The Reference contract now inspects public test names and bodies independently
of Cargo target names. Tests in operator and coverage targets can therefore
join the selected case set. A graph contract checks a neutrally named target
containing a with/eval assignment, excludes an unrelated numeric case, and
rejects fake declarations inside literals or comments. The partition stage
also restores its displayed stage after failure rechecks. All changes precede
the next consolidated partition gate and full run; the canonical report
continues to describe the latest complete measurement.

Per-mode elapsed time is collector metadata. The semantic comparison excludes
that field, while retaining all existing fixture and outcome fields. A contract
checks absent-versus-present timing, differing timings and an actual flag
change. The incomplete run `20261006-135040-bbee753b` was cancelled before
conformance and workspace execution after static review identified this
otherwise inevitable false-positive comparison. Its profiles remain excluded;
Rust production sources are unchanged by the comparison correction.
