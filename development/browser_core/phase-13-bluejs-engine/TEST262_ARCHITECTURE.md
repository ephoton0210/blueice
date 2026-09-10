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

The Test262 adapter maps a parser rejection to a parse-phase `SyntaxError` only
when the parser marked that production as a recognized early error. Other
subset-parser rejections remain `unclassified_parse_error` and cannot satisfy a
negative parse test. In particular, a valid private-name prefix (`#`) remains
unclassified while private elements are unsupported; a malformed identifier
escape is classified only when the lexer has established that the escape itself
is invalid. The adapter and its Python tests therefore keep grammar coverage
separate from evidence that a negative test's required phase and error type were
actually met.

## Dependency order and exit criteria

| Order | Architecture / test items | Dependencies and acceptance criteria |
| --- | --- | --- |
| P0.1 | Completion records and iterator lifetime: `language/statements/{try,throw,return,break,continue,for-of,switch}`, `language/**/dstr`, nested abrupt evaluation | Distinguish normal/throw/return/break/continue, empty/value completion and label targets; keep host resource aborts distinct. Preserve original thrown values and reverse-order cleanup. Iterator `next`, `done`, `value` errors mark done; elisions never read value; defaults/assignment failures close active iterators. Add catch/finally handlers with stack/scope restoration and rooted pending completions before suspension support. |
| P0.2 | Environments and calls: `language/{global-code,eval-code,arguments-object}`, function/default/rest parameter tests | Persistent classic-script global cells, declaration instantiation, `$262.evalScript`, mapped/unmapped arguments and selected direct/indirect eval now cover their measured slices. Dynamic function-local eval bindings, generator call-entry parameter execution, complete declarative/object environment records, full `this`/`new.target`, and independently constructed realms remain; verify each independently. |
| P0.3 | References and abstract operations: assignment/update/delete/call, computed members, destructuring targets, conversion order | Build on environments. Evaluate and retain references before RHS/value/default evaluation as each production requires; GetValue/PutValue and receiver identity must survive side effects and GC. Add per-production traces and strict/sloppy error-precedence tests. |
| P0.4 | Object internal methods and callable/constructor contracts: Object/Reflect/Proxy prerequisites | Build on references. Complete descriptors, receiver-aware Set/Get, own keys, extensibility, arrays/string exotics, call/construct/newTarget and internal slots. Route later Proxy traps through these same contracts and retain GC barriers. |
| P1.1 | Grammar and early-error taxonomy: Unicode identifiers/escapes, reserved words, strict code, labels, remaining operators | Fix foundational early errors needed by P0 in their own slice; then finish the wider grammar. Separate unsupported grammar from proven SyntaxError so parse negatives cannot pass accidentally. |
| P1.2 | Classes/private names/super/derived constructors | Object contracts, environments and grammar first; private brands, initialization ordering and derived this state need internal slots and call frames. |
| P1.3 | Generators, async functions/iteration, Promise jobs | Rooted resumable frames and completion propagation first. Specify resume/throw/return and microtask ordering; test cleanup across suspension. |
| P1.4 | Modules, import/export, dynamic import, top-level await | The dependency-free module baseline now forces strict mode, preserves undefined top-level `this`, and avoids classic global publication. Live bindings, import/export parsing, resolution, linking, cycles, dynamic import and top-level await remain separate acceptance slices. |
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
This was a limited direct-eval slice before P0.2's persistent global-realm
work; it still does not complete declaration instantiation or indirect eval.
Empty try, catch and
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

Function declarations, expressions and class methods share a parameter parser
that rejects a rest default or trailing comma, and recognizes the direct
`await` and `yield` early errors in async and generator parameter initializers.
The compiler enforces duplicate and non-simple strict parameter restrictions,
including strict `arguments`, `eval`, and `yield` bindings. Ordinary functions
reject both `super()` and `super.property`; class methods retain their own home
object. Identifier escapes are decoded before keyword classification, allowing
valid escaped `IdentifierName` class members while rejecting malformed escapes.

The preceding grammar slice measured `language/statements/class` at **2,314 pass, 4,350 fail and
2,002 unsupported** of 8,666 scheduled modes. The paired `function` statement
and expression slices have, respectively, **632 pass, 114 fail, 39 unsupported**
of 785 modes and **450 pass, 26 fail, 8 unsupported** of 484 modes. These runs
use the pinned Test262 snapshot, eight workers, a two-second case deadline and
a 100,000-instruction budget (verified from the stored summaries). Comparing path/mode pairs with the prior class
measurement found 142 fail→pass and 46 unsupported→pass transitions, with no
pass→nonpass transition. This is a focused regression measurement rather than
a conformance claim; raw output is local at
`/tmp/bluejs-test262-class-function-inheritance-final-validated`,
`/tmp/bluejs-test262-function-statements-final-validated`, and
`/tmp/bluejs-test262-function-expressions-final-validated`.

With `--instruction-budget 5000000` (required because the TCO helpers perform
100,000 iterations), the filtered `language/statements/try` run now has **398
pass, 0 fail and 0 unsupported** of 398 scheduled modes, with no harness errors
or timeouts. Raw output is local at `/tmp/bluejs-test262-try-complete` and the
reconciled analysis is at `/tmp/bluejs-test262-try-complete-analysis`.

## Function parameter environments and compiler crash regression

The edition-17 [FunctionDeclarationInstantiation](https://262.ecma-international.org/17.0/#sec-functiondeclarationinstantiation)
algorithm was read on 2026-09-10 before this continuation. Parameter defaults
and computed binding keys now compile in a parameter environment established
before body declarations. Every parameter starts uninitialized and is bound in
source order, so references to a later parameter, including `typeof`, throw
`ReferenceError`. A separate body environment copies same-name `var` values
from initialized parameters; hoisted function declarations supply their own
values. Default-created closures retain parameter cells after body writes,
collection, function return and tail-frame reuse. Parameter lists without expressions
keep the existing shared parameter/var scope, including sloppy duplicate names.
Mapped/unmapped `arguments`, full direct-eval declaration instantiation, and
generator parameter execution at call entry remain open P0.2/P1.3 work.

The `var` declaration inventory now traverses `with` bodies. Previously, a
valid declaration such as `with ({}) { var f = function () {}; }` reached a
compiler binding assertion and terminated the adapter. Nested control flow,
function declarations, destructuring and lexical conflicts have regressions
through the public parse/compile/execute boundary. The broader `with` closure
environment behavior remains incomplete.

All four [public regression tests](../../../backend/bluejs/tests/function_environments.rs)
failed before implementation. They cover parameter TDZ and binding order,
parameter/body closure visibility, same-name declarations, GC, tail-frame reuse
and `with` hoisting. Forty-four contained JavaScript scripts were also checked
with Node in isolated contexts. The JSON-lines adapter tests exercise the
former crash and parameter errors followed by a successful request.

Using the same snapshot, 8 workers, two-second deadline and 100,000-instruction
budget as the preceding measurement, the current results are:

| Filter | Modes | Pass | Fail | Unsupported |
| --- | ---: | ---: | ---: | ---: |
| `language/statements/function` | 785 | 638 | 105 | 42 |
| `language/expressions/function` | 484 | 456 | 20 | 8 |
| `language/statements/class` | 8,666 | 2,334 | 4,330 | 2,002 |

Source hashes, unique path/mode pairs and execution budgets were reconciled:
32 fail-to-pass transitions, no pass-to-nonpass transitions, and zero adapter
crashes, harness errors or timeouts. Six former adapter crashes now produce
ordinary recorded results; three still expose the unsupported implicit-global
assignment operation. The other three still fail due to incomplete `with`
semantics. Raw runs are at `/tmp/bluejs-crate-functions-statements-final`,
`/tmp/bluejs-crate-functions-expressions-final` and `/tmp/bluejs-crate-class-final`;
the comparison is `/tmp/bluejs-crate-comparison.json`.

Validation: BlueJS all-target tests, workspace tests (outside the socket-restricted
sandbox), workspace all-target Clippy with `-D warnings`, and all 10 Python
runner/analyzer tests pass. A further public-pipeline and internal-boundary
regression pass covers `super` scanning, class element failures, adapter
unsupported results, suspended-generator GC roots, `with` lookup, and malformed
bytecode invariants. The Test262 adapter contract is exercised at its public
JSON-lines boundary: ordinary derived construction, `super` reads/writes,
static fields/blocks and the `extends null` error path return their specified
outcomes; async source stays an explicit `unsupported` outcome until promise
jobs and `$DONE` exist. The parser, compiler and heap regressions use their
public crate APIs where possible; private VM/heap invariants retain focused
unit tests.

The no-exclusion line gate now reports **10,758/10,767 (99.92%)**, with all
1,103 functions covered. It still exits 1 against the required 100% threshold:
the remaining nine line-map entries are in `heap.rs` (2), `parser.rs` (3),
`vm.rs` (3) and `vm/builtins.rs` (1), despite their behavior being exercised
by the all-target suite. No exclusions or threshold changes were made, so this
continuation does not claim that every crate gate passes. The earlier crate-wide
rustfmt differences also remain; the new standalone regression file is
rustfmt-clean.

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

## P0.1 continuation: labelled control transfer

The labelled-statement clauses, [Labelled Statements](https://262.ecma-international.org/17.0/#sec-labelled-statements),
[Break Statement](https://262.ecma-international.org/17.0/#sec-break-statement),
and [Continue Statement](https://262.ecma-international.org/17.0/#sec-continue-statement),
were read before this slice. The AST now retains a label wrapper and optional
label targets on `break`/`continue`. The compiler collapses a consecutive label
chain onto its final loop or `switch`; a label over another statement owns its
own non-breakable control context. Consequently an unlabelled `break` still
selects only an enclosing loop or `switch`, while a named break can leave any
labelled statement and a named continue is accepted only for a label on an
iteration statement.

Every transfer uses the existing `AbruptJump` cleanup gateway, so an intervening
`finally` runs before the target is reached. Breaks close every active iterator
they leave, from inner to outer; a continue preserves its target iterator while
closing only inner iterators. The parser handles ASI after labelled sloppy
`let`, rejects the `let [` lookahead, rejects lexical/class/async/generator
declarations as labelled items, and classifies a class-static-block `await`
label as a syntax error. Sloppy unresolvable simple assignment now writes the
global object, which permits an unreachable labelled `let` expression to
compile; unqualified global reads and the remaining global-environment model
stay in P0.2.

The pre-change pinned Test262 run for `language/statements/labeled` scheduled
38 modes from 25 files: **0 pass, 16 fail, 22 unsupported**. The post-change
run with eight workers, two-second timeout and the standard 100,000-instruction
budget reconciles **35 pass, 0 fail, 2 unsupported, 1 timeout**. The unsupported
modes are the two module-host cases. The sole timeout is `tco.js`, whose helper
deliberately exceeds the standard budget; its isolated run passes with the
documented 5,000,000-instruction TCO budget. The post-run analysis is at
`target/test262-label-final/analysis`; the independent high-budget output is at
`target/test262-label-tco/analysis`.

Public regressions cover nested labelled break/continue targets, chained loop
labels, finalizer ordering and completion values, iterator closing, malformed
labelled items, strict `yield`, and sloppy `let` ASI. BlueJS all-target tests
and all ten Python Test262 runner/analyzer tests pass. The remaining P0.1 work
is labels' broader grammar interactions and control-flow cases outside this
slice; module support remains P1.4, while the later P0.2 section records the
classic-script global-realm implementation.

## P0.1 continuation: switch scope ordering and duplicate defaults

The [Switch Statement](https://262.ecma-international.org/17.0/#sec-switch-statement)
and its [CaseBlock static semantics](https://262.ecma-international.org/17.0/#sec-static-semantics-early-errors)
were read on 2026-09-10. A CaseBlock has at most one `DefaultClause`; the
parser now reports a classified SyntaxError when it sees a second `default`.

Switch execution evaluates its discriminant in the surrounding lexical
environment, then creates the case-block lexical environment before evaluating
case selectors and consequents. The compiler previously entered that scope too
early, so closures created by the discriminant captured a case-local `let`
binding. It now emits the discriminant first and enters the lexical scope
afterward. A public regression verifies that a discriminant closure captures
the outer binding while selector and consequent closures capture the inner
binding; the parser regression separately verifies the duplicate-default early
error.

The pre-change `language/statements/switch` run reconciled **144 pass, 23
fail, 48 unsupported, 3 timeout** across 218 modes. The post-change run at
`target/test262-switch-after-scope/analysis` reconciles **150 pass, 17 fail,
48 unsupported, 3 timeout**. The six newly passing modes are the duplicate
default negative plus the `scope-lex-open-case` and `scope-lex-open-dflt`
closure-scope cases, each in sloppy and strict mode. The remaining switch
failures are principally broader declaration-redeclaration early errors; async
function syntax and host hooks remain separately classified. This is a scoped
P0.1 improvement, not a claim of complete switch conformance.

## P0.1 continuation: CaseBlock function declarations

The Switch Statement and CaseBlock static semantics were revisited on
2026-09-10. Direct function and generator declarations in a CaseBlock belong
to its `LexicallyDeclaredNames`, rather than its `VarDeclaredNames`. Therefore
they participate in duplicate lexical-declaration and lexical-versus-`var`
early errors. The sloppy-mode Annex B compatibility exception remains limited
to duplicate ordinary, non-async, non-generator function declarations.

The compiler now derives CaseBlock lexical names from direct `let`, `const`,
class, function and generator declarations, and collects its `var` names with
those function declarations excluded. This makes the declarations genuinely
switch-local at execution time as well as classifying their early errors. A
public regression covers duplicate and `var` conflicts, preserves the allowed
single ordinary-function case, and verifies that a switch-local generator name
produces `ReferenceError` after the switch. Annex B's additional outer `var`
binding behavior is not implemented by this slice.

The final run at `target/test262-switch-case-scope/analysis` reconciles
**199 pass, 0 fail, 16 unsupported, 3 timeout** across the same 218 modes.
Relative to the preceding switch run, all 17 failures now pass and 32 expected
negative cases previously classified unsupported are correctly rejected during
compilation. The remaining modes are parser/async-function gaps, host hooks,
and tail-call instruction-limit outcomes; this remains a bounded P0.1 advance,
not full switch conformance.

## P0.1 continuation: Switch production syntax classification

The parser now explicitly marks malformed `switch` productions as known
SyntaxErrors when the grammar is unambiguous: omitted parentheses or body,
an empty discriminant, a missing case expression or colon, and a CaseBlock item
that is neither `case` nor `default`. Expression parsing itself remains
conservative: a rejection there can still denote valid syntax outside the
implemented subset, so it is not promoted merely because it occurs in a switch.
Parser and JSON-lines adapter regressions cover the classified boundary.

The run at `target/test262-switch-grammar/analysis` reconciles **209 pass, 0
fail, 6 unsupported, 3 timeout**. The five historic malformed-switch tests now
pass in both strict and sloppy modes, replacing ten unclassified parse outcomes.
The only remaining switch modes are four async-function execution gaps, two
`$262`/IsHTMLDDA host-hook cases, and three tail-call instruction-limit
outcomes. This is still a P0.1 subset result rather than full switch
conformance.

## P0.1 continuation: Tail-call fixture resource policy

The three remaining switch tail-call fixtures declare
`tail-call-optimization` and include Test262's `tcoHelper.js`, which requires
100,000 consecutive calls. BlueJS already emits `TailRecur` for an unshadowed,
strict named function-expression self call, including one returned from a case
or default clause. Their former outcomes were therefore the runner's generic
100,000-dispatch limit, not a retained-frame or switch tail-position failure.

The Test262 runner now gives only modes declaring `tail-call-optimization` a
minimum 3,000,000-dispatch budget, while retaining the 100,000 baseline and
per-case wall deadline for every other mode. A runner regression locks that
policy and the summary records both thresholds. The final run at
`target/test262-switch-tail-budget/analysis` reconciles **212 pass, 0 fail, 6
unsupported, 0 timeout**. The remaining four async execution modes and two
IsHTMLDDA host-hook modes require their P1.3 and P1.6 implementations; they
are deliberately still reported unsupported rather than being reclassified.

## P0.1 continuation: current Annex B host exotic and async declarations

The latest published [ECMAScript 2026 Annex B
text](https://tc39.es/ecma262/2026/multipage/additional-ecmascript-features-for-web-browsers.html)
was checked before closing the remaining switch modes. Its `[[IsHTMLDDA]]`
compatibility slot changes only `ToBoolean`, `IsLooselyEqual`, and `typeof`;
it deliberately does not alter Strict Equality Comparison. The Test262 adapter
therefore installs a fresh, feature-gated `$262.IsHTMLDDA` host exotic. Its
false boolean coercion, loose equality with `null` and `undefined`, and
`typeof` result are special, while switch case selection still uses ordinary
object identity and reaches `case IsHTMLDDA`.

The current [async function algorithms](https://tc39.es/ecma262/2026/multipage/control-abstraction-objects.html)
require Promise capabilities, async completion propagation, and (for async
generators) request queues. BlueJS does not yet have that Promise/job-queue
architecture, so it does not claim execution support. It now does compile an
empty async or async-generator declaration into a non-constructible closure,
which lets CaseBlock declaration instantiation create the required lexical
binding. Calling that closure reports the explicit runtime `unsupported` gap;
non-empty async bodies remain compiler-unsupported.

The resulting fresh run at `target/test262-switch-is-html-async/` reconciles
**218 pass, 0 fail, 0 unsupported, 0 timeout** across all 218 current
`language/statements/switch` modes. Public regressions cover all three Annex B
observable operations, strict switch matching, feature-gated adapter setup,
async CaseBlock scope exit, and the explicit async-call boundary. This closes
the filtered corpus, not P1.3 async execution or general Test262 conformance.

## P0.1 continuation: Number bitwise, shift, and static-property edges

The latest published [ECMAScript 2026 expression grammar and evaluation
rules](https://tc39.es/ecma262/2026/multipage/ecmascript-language-expressions.html)
were checked on 2026-09-10. The grammar orders bitwise AND, XOR and OR above
logical AND, with shift expressions beneath relational expressions; compound
assignment uses the same binary operations after obtaining the left reference.
BlueJS now represents `~`, `&`, `^`, `|`, `<<`, `>>`, `>>>` and each matching
compound assignment in its AST and bytecode. The parser follows the specified
precedence chain, while the compiler preserves single evaluation of member
assignment references.

For the implemented Number branch, operands are converted through the existing
observable primitive/number coercion path. `&`, `^`, `|` and `~` use ToInt32;
the shift count uses ToUint32 masked to five bits; signed right shift sign-fills
and unsigned right shift returns a non-negative uint32 result. BigInt remains a
separate unsupported value and literal workstream, so the mixed-type and
BigInt-only branches of these operators are deliberately not claimed here.

The same pass exposed Number static-property prerequisites in otherwise
Number-only shift fixtures. `Number.EPSILON`, safe integers, extrema, `NaN`
and signed infinities are now non-writable, non-enumerable and
non-configurable data properties. `Number.MIN_VALUE` is the IEEE-754 smallest
positive subnormal (`5e-324`), not the smallest normal number. The related
`Object.prototype.propertyIsEnumerable` implementation consults only an own
property descriptor, and is initialized through the existing lazy intrinsic
path before object-property lookup.

The pre-change `language/statements/break` selection had **36 pass, 4 fail**:
the four sources use bitwise AND in expression statements. The fresh final run
at `target/test262-break-verified` now has **40 pass, 0 fail, 0 unsupported, 0
timeout**. The broader `language/expressions/bitwise` run at
`target/test262-bitwise-verified` has **169 pass, 40 fail**; every remaining
failure is a BigInt-literal parse case. The precise Number shift selections are
now `<<`: **76 pass, 12 BigInt failures, 1 unsupported**; `>>`: **60 pass, 12
BigInt failures, 1 unsupported**; and `>>>`: **76 pass, 12 BigInt failures, 1
unsupported**. Each unsupported mode is strict implicit-global assignment,
which belongs to the global-environment P0.2 workstream rather than shift
semantics. The focused Number static-property selections for `EPSILON`, safe
integers, `NaN`, extrema and infinities total **40/40** passing modes.

Public regressions first covered coercion, 32-bit truncation/masking,
precedence and compound-member-reference evaluation; they then covered every
Number data constant/descriptor and own-only enumerability. This is a
Number-only language and builtin slice, not BigInt, complete Object.prototype,
or edition-wide Test262 conformance.

## P1.5 slice: BigInt bitwise and shift operators

The current [ECMAScript 2026 numeric-literal grammar](https://tc39.es/ecma262/2026/multipage/ecmascript-language-lexical-grammar.html#sec-literals-numeric-literals), [BigInt type](https://tc39.es/ecma262/2026/multipage/ecmascript-data-types-and-values.html#sec-ecmascript-language-types-bigint-type), and [operator evaluation rules](https://tc39.es/ecma262/2026/multipage/ecmascript-language-expressions.html) were checked on 2026-09-10. `n`-suffixed decimal, binary, octal and hexadecimal integer literals now become arbitrary-precision runtime values. Fractions, exponents, invalid leading-zero decimal forms and identifier continuations remain syntax errors. BigInt is a distinct `Numeric` branch: after observable `ToPrimitive`, `~`, `&`, `^`, `|`, `<<` and `>>` require two BigInts, preserve two's-complement semantics, and permit a signed (including negative) BigInt shift count. `>>>` rejects BigInt; a Number/BigInt pair rejects rather than silently converting either operand.

Unary negation and bitwise not use the same numeric branch, so negative BigInt literals and `~` do not lose precision. `typeof` reports `"bigint"`; String conversion, `Object(1n)`, the minimal callable `BigInt(1n)`, `BigInt.prototype.valueOf`/`toString`, and the BigInt `Object.prototype.toString` tag support the observable primitive-wrapping paths exercised by this corpus. This deliberately does not claim BigInt arithmetic, comparison/coercion, parsing from other primitive inputs, typed arrays, buffers, or a complete BigInt builtin.

The strict unresolvable-reference cases that previously appeared as shift `unsupported` results now compile a deferred `PutValue`: the RHS is evaluated first, then strict assignment to a missing global throws `ReferenceError`; an already-existing `globalThis` property is assignable. This follows the current [PutValue](https://tc39.es/ecma262/2026/multipage/ecmascript-data-types-and-values.html#sec-putvalue) ordering and removes the three shift-family unsupported modes without treating them as shift semantics.

The fresh focused runs are fully passing: `language/expressions/bitwise` has **209/209**, `left-shift` **89/89**, `right-shift` **73/73**, and `unsigned-right-shift` **89/89** scheduled modes. Public regressions cover arbitrary precision, BigInt `ToPrimitive` through a wrapper and user `valueOf`, negative counts, mixed Numeric rejection, BigInt `>>>`, syntax boundaries and basic boxing. These four filtered selections are not a P1.5 completion claim or evidence of full Test262 conformance.

## P0.2 slice: persistent global realm and legacy global declarations

The current [GlobalDeclarationInstantiation and global environment-record
algorithms](https://tc39.es/ecma262/2026/multipage/global-object.html) and
[Annex B.3.2/B.3.3 web-compatibility rules](https://tc39.es/ecma262/2026/multipage/additional-ecmascript-features-for-web-browsers.html#sec-web-compat-globaldeclarationinstantiation)
were reviewed on 2026-09-10 before this architecture slice. A classic script
now uses cells retained by the VM's one global realm rather than a new compiled
scope for every call. It validates global lexical declarations before execution,
performs the specified global `var` and ordinary-function declaration checks
against own property descriptors and extensibility, then creates the lexical or
property-backed binding. That preserves bindings and `globalThis` properties
across `$262.evalScript` calls, including observable failure-before-side-effect
when a declaration cannot be made.

`Object.preventExtensions`, `Object.prototype.hasOwnProperty`, the relevant
intrinsic global properties, and a dynamically compiled global `Function`
constructor provide the object/host prerequisites exercised by this group. The
dynamic constructor deliberately has no capture of its caller's lexical
environment. Top-level `super()` and `super.property` are now parser early
errors in script code, including arrow-function containment, rather than an
execution-time unsupported result.

For non-strict legacy syntax, eligible ordinary (not async or generator)
function declarations in blocks, CaseBlocks and `if` clauses retain their
block-lexical binding and copy the function value to their Annex B outer `var`
binding only when the block/clause executes. The candidate analysis rejects an
outer binding where replacing the function declaration with `var` would create
an early error; it also implements the Annex B.3.5 simple-catch-parameter
exception. Class declarations use mutable lexical bindings, matching the
declaration's specified binding kind.

The fresh `language/global-code` run at
`target/test262-global-code-script-super` has **212 pass, 0 fail and 16
unsupported** across 195 files / 228 scheduled modes. The remaining outcomes
are explicitly parser-classified feature gaps: module import/export syntax (4),
private names (8), and top-level `new.target` handling (4). This validates the
selected classic-script architecture only; it is neither an implementation of
direct `eval` or modules nor a claim of full Test262 conformance.

## P0.2 continuation: arguments objects and eval environments

The current [arguments exotic-object algorithms](https://tc39.es/ecma262/2026/multipage/ordinary-and-exotic-objects-behaviours.html), [PerformEval](https://tc39.es/ecma262/2026/multipage/global-object.html#sec-performeval), and [EvalDeclarationInstantiation](https://tc39.es/ecma262/2026/multipage/global-object.html#sec-evaldeclarationinstantiation) were read on 2026-09-11. Ordinary function entry now creates the specified mapped object only for sloppy simple parameter lists. Index mappings share parameter cells, and are disconnected by delete, accessors, or non-writable redefinition. Strict and non-simple functions instead receive the unmapped object with the thrower `callee`; arrows retain their enclosing `arguments`. Generator execution restores the called closure while its frame runs, so an arguments object created during execution has the correct `callee`.

Direct `eval(...)` now has bytecode distinct from an ordinary call, including the spread form. It uses the caller's visible cells, strictness and `this`; sloppy global direct eval publishes configurable global `var`/function properties, while strict eval creates a temporary variable environment. Calls through aliases, comma expressions, or properties use indirect eval, whose lexical lookup and `this` begin at the realm global environment. The VM records the active VariableEnvironment scope so a sloppy direct eval rejects a `var` that would cross an intervening block, parameter, or not-yet-entered body lexical binding, without rejecting an ordinary shared variable binding.

The fresh `language/arguments-object` selection at `target/test262-arguments-object-current` has **250 pass, 140 fail and 70 unsupported** of 460 scheduled modes. All 140 failures are visible unclassified parser gaps (private names or generator/async-generator method syntax); the 70 unsupported modes require async functions. The current `language/eval-code` selection at `target/test262-eval-code-current` has **739 pass, 109 fail and 76 unsupported** of 924 modes. Its remaining 53 assertion failures are primarily dynamic function-local eval bindings and generator parameter evaluation at call entry; parser/async/module host gaps remain separately classified. These are bounded progress measurements, not a claim that either Test262 family is complete.

## P0.3 continuation: retained references and Annex B call targets

Reference construction is now explicit where a later evaluation can alter name
resolution. A sloppy direct eval separates the active function's
VariableEnvironment from merely captured outer cells, so `eval("var x")` in
an inner closure creates its local dynamic binding instead of overwriting an
outer capture. `with` assignment resolves its object/binding/unresolvable
reference before evaluating the RHS; the reference remains stack-rooted through
the RHS and is then used for PutValue. Compound `with` assignment additionally
performs GetValue on that retained reference before the RHS, including the
correct unresolvable-reference error precedence.

The current Annex B web-compat CallExpression assignment-target behavior is
also retained in the AST. In sloppy code a call target is evaluated exactly
once and then throws `ReferenceError`, without evaluating the assignment RHS or
coercing the call result. Strict code remains a syntax error. The same rule
applies to prefix/postfix updates and `for-in`/`for-of` heads.

Against the prior `language/expressions/assignment` reference slice, the fresh
run at `target/test262-next-assignment-final` changes **21** non-passing modes
to pass with **zero** pass-to-nonpass transitions: the seven Annex B sloppy
runtime cases, eight strict CallExpression early errors, and six module-mode
assignment-target negatives. The group now has **615 pass, 229 fail and 644
unsupported** of 1,488 scheduled modes. Public pipeline regressions cover
direct-eval capture shadowing, `with` simple/compound reference latching,
unresolvable compound-error precedence, and all six CallExpression target
forms. This remains a P0.3 slice rather than a complete reference model.

## P1.4 foundation: dependency-free module evaluation

`parse_module`, `compile_module`, and `Vm::execute_module` establish one
module evaluation context without reusing classic-script global declaration
publication. Module compilation always enables strict semantics; execution
keeps top-level `this` undefined, including lexical arrows, while harness
classic scripts continue to run in the same realm. Module declarations are
therefore observable locally but do not create `globalThis` properties.

This is deliberately an evaluation baseline, not a module graph. Import/export
grammar, requested-module resolution, instantiation, live bindings, namespace
objects, cycles, dynamic import, and top-level await remain P1.4 follow-up
slices. The fresh `language/module-code` run at
`target/test262-next-module-baseline` records **26 pass, 386 fail and 190
unsupported** of 602 modes; before this baseline every selected module mode was
reported as a module-host unsupported result. The representative local-binding
and top-level-`this` cases pass, and a public regression locks strictness and
non-publication behavior.
