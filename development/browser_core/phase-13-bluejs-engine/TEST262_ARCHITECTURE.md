# Test262 architecture-first implementation backlog

Requested 2026-09-10. Status: **full inventory measured; full conformance remains open**.
This is the implementation order for the [edition 17 track](ECMASCRIPT_2026.md).
The [recorded analysis](TEST262_ANALYSIS_REPORT.md) includes all workstream counts,
common diagnostics, representative cases and executable hashes.
The existing `test262-summary.json` only groups outcomes by top-level directory
and feature. It does not establish failure root causes or architectural priority.

## Evidence and classification contract

The verified snapshot is `6eec1ac9ee144dafd8f344d73a21f36bfc9f6755`:
53,404 test files, 279 fixture resources, 102,578 execution modes. The baseline
for this work includes the comma/void changes now committed as `401eb8e`:
16,216 pass, 62,701 fail, 22,736 unsupported, 925 timeout, zero harness errors.
It was run with 8 workers, 100,000 instructions and a 2-second case deadline.
The runner exited 1, as expected for a non-passing full inventory.

The new [analyzer](../../../backend/bluejs/test262/analyze.py) reconciles the
summary, unique path/mode pairs, source hashes, metadata and full source inventory
before publishing a report. `items.jsonl` retains **every mode**, its original
outcome/diagnostic, `esid`, description, flags, features and includes, together
with a target workstream, priority, prerequisites and observed blocker.
Targets are inferred from paths/metadata; diagnostics identify the **first
observed symptom**, not every root cause. Dependencies overlap; exclusive target
totals reconcile to the full denominator. Passed negative tests are not errors.

The baseline has 39,447 unclassified parser rejections, 14,768 unresolved-name
errors, 10,160 TypeErrors, 9,166 unsupported harness prerequisites and 3,955
unsupported compiler cases. These symptoms often hide later failures. In
particular, a failed Array test does not establish that its Array algorithm is
the first missing dependency. The generic unsupported-statement diagnostic
covers several AST kinds and must not be labelled as a confirmed try/catch bug.

The pinned `INTERPRETING.md` requires isolated realms, same-realm harness
includes, exact strict/raw/module/async behavior and negative phase/type
matching. Adapter runtime errors currently do not identify whether an include
or the test body threw; treat those cases as requiring reproduction. All
staging, Annex B, ECMA-402 and host-dependent cases remain in the inventory.
Their applicability needs an explicit clause/edition audit, never a silent skip.
Instruction exhaustion (906 baseline modes) and wall/regex deadlines (19) are
separate from semantic failures and should be rerun with recorded budgets.

## Dependency order and exit criteria

| Order | Architecture / test items | Dependencies and acceptance criteria |
| --- | --- | --- |
| P0.1 | Completion records and iterator lifetime: `language/statements/{try,throw,return,break,continue,for-of,switch}`, `language/**/dstr`, nested abrupt evaluation | Distinguish normal/throw/return/break/continue, empty/value completion and label targets; keep host resource aborts distinct. Preserve original thrown values and reverse-order cleanup. Iterator `next`, `done`, `value` errors mark done; elisions never read value; defaults/assignment failures close active iterators. Add catch/finally handlers with stack/scope restoration and rooted pending completions before suspension support. |
| P0.2 | Environments and calls: `language/{global-code,eval-code,arguments-object}`, function/default/rest parameter tests | Build on completion propagation. Persistent global object + declarative records, declaration instantiation even on abrupt script exit, TDZ, parameter/body separation, captured cells, mapped/unmapped arguments, direct/indirect eval and this/new.target. Verify multiple scripts in one realm and independent realms. |
| P0.3 | References and abstract operations: assignment/update/delete/call, computed members, destructuring targets, conversion order | Build on environments. Evaluate and retain references before RHS/value/default evaluation as each production requires; GetValue/PutValue and receiver identity must survive side effects and GC. Add per-production traces and strict/sloppy error-precedence tests. |
| P0.4 | Object internal methods and callable/constructor contracts: Object/Reflect/Proxy prerequisites | Build on references. Complete descriptors, receiver-aware Set/Get, own keys, extensibility, arrays/string exotics, call/construct/newTarget and internal slots. Route later Proxy traps through these same contracts and retain GC barriers. |
| P1.1 | Grammar and early-error taxonomy: Unicode identifiers/escapes, reserved words, strict code, labels, remaining operators | Fix foundational early errors needed by P0 in their own slice; then finish the wider grammar. Separate unsupported grammar from proven SyntaxError so parse negatives cannot pass accidentally. |
| P1.2 | Classes/private names/super/derived constructors | Object contracts, environments and grammar first; private brands, initialization ordering and derived this state need internal slots and call frames. |
| P1.3 | Generators, async functions/iteration, Promise jobs | Rooted resumable frames and completion propagation first. Specify resume/throw/return and microtask ordering; test cleanup across suspension. |
| P1.4 | Modules, import/export, dynamic import, top-level await | Persistent environments, live bindings and async jobs first. Test parsing, resolution, linking and evaluation phases independently, cycles and fixture resolution. |
| P1.5 | BigInt, ArrayBuffer/DataView/typed arrays, shared memory, weak references | Value/internal-slot and GC contracts first. BigInt arithmetic/coercion, buffer detach/resize, views, agent memory model and weak reachability each get a separate design and acceptance suite. |
| P1.6 | `$262` realm/agent/GC/buffer hooks, `$DONE`, async completion | Implement each hook alongside its owning architecture (not only at the end). Validate harness correctness before counting newly enabled tests. Never replace a missing operation with a dummy success. |
| P2.1 | Array/collections/Date/RegExp/JSON/numeric and remaining builtin methods | Consume the P0/P1 contracts. Work one family at a time, ordered by remaining shared dependencies, then failing-mode count. Cover coercion/descriptor/exception edges before declaring a family complete. |
| P2.2 | Remaining ECMA-402 constructors, algorithms and locale data | Reuse the same object/conversion/exception model; track edition 13 and locale-data requirements separately. |
| P3.1 | Unmapped/staging and edition/host applicability audit | Audit alongside implementation; mandatory edition-17 cases move into the appropriate earlier workstream. No exclusion or passing claim follows from a P3 label. |

An individual API may be needed earlier as a prerequisite; record that dependency
instead of advancing an entire library ahead of its execution model. Full
Test262 support requires all applicable rows, not merely P0 or the largest group.

## First P0.1 slice: iterator completions and rooting

Design before implementation, 2026-09-10. Official published edition-17 clauses
retrieved and read: [IteratorNext](https://262.ecma-international.org/17.0/#sec-iteratornext),
[IteratorStep](https://262.ecma-international.org/17.0/#sec-iteratorstep),
[IteratorStepValue](https://262.ecma-international.org/17.0/#sec-iteratorstepvalue),
[IteratorClose](https://262.ecma-international.org/17.0/#sec-iteratorclose), and
[IteratorDestructuringAssignmentEvaluation](https://262.ecma-international.org/17.0/#sec-runtime-semantics-iteratordestructuringassignmentevaluation).

Keep compiler-owned fixed-width bytecode and the existing managed heap. Add an
elision instruction that advances an iterator without obtaining its value.
Share next/result/done validation with value-consuming iteration. Iterator
records retain exhaustion/abrupt state, and cleanup only closes still-active
records. A rest array must remain on the VM root stack while subsequent iterator
callbacks allocate; an unrooted Rust `Vec<Value>` cannot own live JS objects.
Preserve the original throw over errors from `return()`, and close nested active
iterators inside out. This is the first part of P0.1, not complete catch/finally
or generalized completion-record support.

Public regressions were run before implementation: elisions invoked a throwing
value getter; rest errors invoked `return()`; an inner iterator error closed
both inner and outer instead of only outer. An allocation-pressure regression
also reproduced a stale managed object in rest collection. All four regressions
failed before the corresponding fixes.

### Results of this slice

The full after-run reconciles all 102,578 modes: **16,226 pass, 62,697 fail,
22,736 unsupported, 919 timeout, zero harness errors**. Comparing every path/mode
against the baseline found **zero pass-to-nonpass regressions**. Four modes
changed from assertion failure to pass:

| Test path under `test/` | Modes | Change |
| --- | --- | --- |
| `language/expressions/assignment/dstr/array-rest-iter-thrw-close-skip.js` | sloppy, strict | Iterator-origin throw no longer calls `return()` |
| `language/expressions/assignment/dstr/array-elem-trlg-iter-rest-thrw-close-skip.js` | sloppy, strict | Same guarantee after an earlier yielded element |

Six other modes changed from timeout to pass (RegExp emoji sequences and Unicode
identifier sweeps); these are timing variation, **not attributed semantic gains**.
The four public iterator regressions additionally cover elision value-getter
suppression, exhaustion, next/done/value/non-object errors, parameter and for-of
consumers, nested close order, original thrown-object identity and rest GC
reachability. The independent Node oracle passes 22,271 isolated scripts.

Validation: 249 default BlueJS tests plus two doc examples, nine Python
runner/analyzer tests, workspace build/all-target Clippy (`-D warnings`) and
workspace tests pass. Workspace socket tests required execution outside the
socket-restricted sandbox. The no-exclusion BlueJS line gate reports
**8,488/8,488 lines (100%)**, 881/881 functions and 94.18% regions. The existing
unary AST test was extended for `void` alongside public malformed-expression
and inclusive bytecode-budget cases. `cargo fmt --all -- --check` still reports
pre-existing formatting differences across the workspace; the new Rust test
file is rustfmt-clean. No workspace coverage claim is made by the BlueJS gate.

The iterator completion state and rest-rooting subtask is complete. The next
subtask, general Completion records plus `catch`/`finally`, is implemented in
the limited executable subset described below. P0.1 is still open: labels,
`switch`, all grammar/early-error work and the remaining control-flow coverage
must precede P0.2 and P0.3. No P0 workstream or builtin family is fully complete.

## Second P0.1 slice: Completion records and `try` handlers

Official edition-17 clauses read on 2026-09-10: [Completion Record
specification type](https://262.ecma-international.org/17.0/#sec-completion-record-specification-type),
[TryStatement](https://262.ecma-international.org/17.0/#sec-try-statement),
[ReturnStatement](https://262.ecma-international.org/17.0/#sec-return-statement),
[BreakStatement](https://262.ecma-international.org/17.0/#sec-break-statement),
and [ContinueStatement](https://262.ecma-international.org/17.0/#sec-continue-statement).

The fixed-width bytecode now owns static handler metadata and control-transfer
metadata. `PushHandler` captures operand-stack, lexical-scope and active-
iterator depths; `PopHandler`, `SaveCompletion`, and handler-tagged
`ResumeCompletion` distinguish normal finalizer entry from a pending abrupt
completion. `AbruptJump` refers to a cleanup gateway and its eventual target,
so an inner `break` or `continue` runs exactly the finalizers it leaves without
running an outer handler whose protected region still contains that target.
Unwound scope slots and pending return/thrown values stay in VM root sets during
allocation. Normal finalizer completion applies `UpdateEmpty` behavior to the
prior statement completion; an abrupt finalizer overrides it.

Runtime `ReferenceError`, `TypeError`, `RangeError`, `SyntaxError`, explicit
throws, and Test262 assertion errors enter JavaScript `catch` as their matching
error object or original value. Heap failures, instruction and string limits,
and regex host failures remain distinct, uncatchable resource aborts. Labels
remain outside this slice.

Public regressions cover caught values and language errors, catch scope exit,
normal/throw/return/break/continue finalization, finalizer override, nested
handler selection, a nested normal finalizer while an outer abrupt completion
is pending, GC pressure on a caught object, and host-limit bypass. They were
run through parse → compile → execute rather than a VM-only helper.

The post-change filtered Test262 run covers `language/statements/try`: **249
pass, 97 fail, 52 unsupported** of 398 scheduled modes across 206 files, with
zero harness errors, timeouts, or adapter crashes. This is a current slice
measurement, not a whole-inventory comparison or conformance claim. The
remaining modes are visible: 73 unclassified parser rejections, 28 unresolved
names (principally `eval`), 16 assertion failures, 14 unsupported compiler
cases, eight syntax/early-error mismatches, eight missing expected-error
results, and two uncaught-value outcomes.
Its raw output is local at `/tmp/bluejs-test262-try-final`.

## P0.1 continuation: `switch`, `for-in`, direct eval and early errors

The executable subset now also compiles strict-match `switch` dispatch with
fall-through and completion-aware `break`; `for-in` snapshots enumerable string
keys through the prototype chain and uses the same finalizer/iterator cleanup
boundary as `for-of`. Catch parameters have a distinct parameter environment,
Annex B simple-parameter `var` redeclaration behavior, inferred names for
default anonymous functions, and the required strict/lexical early errors.

Direct eval parses and compiles source in the active VM frame. It shares the
caller's visible binding cells, current `this`, strictness and instruction
budget, while its fresh lexical declarations remain local to the eval frame.
This is a limited direct-eval slice, not P0.2's persistent global environment,
complete declaration instantiation or indirect eval. Empty try, catch and
finally blocks now establish their own empty Completion, so normal finalizers
restore the protected clause and apply `UpdateEmpty` correctly.

The parser distinguishes the recognized binding-rest and malformed try/catch/
finally early errors from arbitrary unsupported grammar; only the former are
reported as parse `SyntaxError` by the Test262 adapter. The filtered run now
covers `language/statements/try`: **358 pass, 38 fail and 2 unsupported** of
398 scheduled modes, with zero harness errors, timeouts or adapter crashes.
Raw output is local at `/tmp/bluejs-test262-try-syntax`. The remaining blockers
are generator and class grammar/execution, `with`, named-function tail
recursion, and static-class grammar; they require their owning P1/P0.2 designs
rather than a broader parse-error classification.

## P0.1 continuation: named tails, generators, classes and `with`

Named function expressions now create their required immutable internal name
binding. In strict functions, a direct tail call through that unshadowed binding
reuses the active frame after catch/finally completion processing, rather than
growing the Rust or JavaScript call stack. This covers the Test262 `try/tco-*`
cases; it is not yet general proper-tail-call optimization for arbitrary call
expressions.

Generator functions compile `yield` into resumable bytecode. A heap-owned
generator state preserves its operand stack, bindings, captured cells, `this`,
arguments and active lexical scopes across `.next()`; generator iterators expose
`next`, `return` and `@@iterator`. This slice supports the destructuring and
overridden `Array.prototype[Symbol.iterator]` generator cases. Yielding while a
try handler is active remains outside this slice.

The class subset now evaluates base class declarations and expressions with
constructors, instance/static methods and accessors, generator methods, public
instance/static fields, and static blocks. It also evaluates `extends`: a valid
superclass creates both constructor and instance prototype chains, and a default
derived constructor forwards its arguments through `super()`. Explicit derived
constructors support direct `super()` calls; instance fields run after that call.
Field initialization behind conditional or otherwise indirect `super()` control
flow remains a classified compiler gap rather than running at an incorrect
point. The available eval path rejects a lexical `super()` call from an
instance field before its source can take effect; this does not implement the
P0.2 distinction between direct and indirect eval. Method `[[HomeObject]]`
metadata supports instance and static `super` reads, calls, assignments and
updates, is inherited by arrows (including a derived constructor arrow's
lexical `super()`), and remains live while a generator method is suspended.
It is installed for every class constructor so an arrow created by a base-class
field can use `super` property access. Fields, static blocks, accessors and
non-derived constructors with a `super()` call now report a known parse
`SyntaxError`. Methods are strict, non-enumerable, non-constructible closures;
constructors require `new` and report an ordinary `TypeError` when a class
element cannot replace a non-configurable property. Static blocks and
static-field initializers execute with the class as `this`, and a class
declaration is initialized before those elements run. Named class expressions
retain an immutable internal name binding for their methods and static blocks.
Async methods, async generators, async functions and async arrows retain
`await`, `yield*`, `for await`, and lexical `new.target` syntax, then report
the explicit `async functions` compiler gap: Promise jobs and the `$DONE` host
are not implemented. Private elements, decorators, async execution and
arbitrary derived-field control flow remain later work. Sloppy `with` now has a VM-managed object environment for
simple identifier reads/writes, is unwound with handlers, and is rejected in
strict code. It does not yet model every `with` interaction with closures,
`typeof`, updates or implicit global writes.

The current `language/statements/class` slice has **2,126 pass, 4,476 fail and
2,064 unsupported** of 8,666 scheduled modes. It uses the pinned Test262
snapshot, eight workers, a two-second case deadline and a 1,000,000-instruction
budget. This is a focused regression measurement rather than a conformance
claim; raw output is local at `/tmp/bluejs-test262-class-super-early`.

With `--instruction-budget 5000000` (required because the TCO helpers perform
100,000 iterations), the filtered `language/statements/try` run now has **398
pass, 0 fail and 0 unsupported** of 398 scheduled modes, with no harness errors
or timeouts. Raw output is local at `/tmp/bluejs-test262-try-complete` and the
reconciled analysis is at `/tmp/bluejs-test262-try-complete-analysis`.

## Reproduction and continuation

Use a unique output directory for each run; do not run two writers against the
same results file or rebuild its adapter during measurement.

```sh
cargo build -p blueice-bluejs --bins --offline
python3 backend/bluejs/test262/run.py --jobs 8 --output target/test262-architecture-baseline
python3 backend/bluejs/test262/analyze.py --run target/test262-architecture-baseline --output target/test262-architecture-baseline/analysis
python3 -m unittest discover -s backend/bluejs/test262 -v
```

For each remaining slice: select representative failing modes and their prerequisites,
write public regressions, record red results, implement, run focused Test262 and
regression/coverage gates, then reconcile a full before/after inventory. Compare
unique path/mode pairs, including pass-to-nonpass changes, rather than totals
alone. Preserve raw outcomes so improved parsing that exposes a later missing
dependency is not confused with a newly passing test. Never mark this backlog
complete until the full applicable inventory passes with audited host behavior.
