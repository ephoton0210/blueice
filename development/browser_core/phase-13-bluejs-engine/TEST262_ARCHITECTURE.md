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
are not implemented. The P1.2 private-slot reference slice now accepts private
identifiers and supports both instance and static private fields, methods and
accessors through non-enumerable heap slots keyed by their declaring class
owner. Instance construction installs the private brand before field
initializers; static elements brand the constructor itself. The compiler
represents each declared private name with a hidden lexical owner binding.
Consequently arrow and ordinary nested functions, nested classes, generators,
and direct eval capture the correct lexical private-name environment rather
than borrowing a single method `[[HomeObject]]`. `#name in object` performs a
brand check and rejects a non-object RHS. Parse-time private early errors now
reject unbound names, duplicate declarations except a matching getter/setter
pair, `#constructor`, private `super` access, invalid private property-key
contexts, and a class's use of its own private name in its heritage. Arbitrary
derived-field control flow, decorators and async execution remain later work.
Sloppy `with` now has a VM-managed object environment for
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
generators) request queues. The later P1.4 module slice adds a rooted Promise
reaction/loader job queue and settled async-function completion; this older
switch result must therefore not be read as the current async boundary. Generic
resumable async frames, thenable assimilation and async generators remain open.

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

This was deliberately an evaluation baseline, not a module graph. Import/export
grammar, requested-module resolution, instantiation, live bindings, namespace
objects, cycles, dynamic import, and top-level await were P1.4 follow-up
slices. The fresh `language/module-code` run at
`target/test262-next-module-baseline` records **26 pass, 386 fail and 190
unsupported** of 602 modes; before this baseline every selected module mode was
reported as a module-host unsupported result. The representative local-binding
and top-level-`this` cases pass, and a public regression locks strictness and
non-publication behavior.

## P1.4 continuation: static import/export graph linking

The current [ECMAScript 2026 module-record algorithms](https://tc39.es/ecma262/2026/multipage/ecmascript-language-scripts-and-modules.html)
were checked on 2026-09-11. `ParseModule` retains ImportEntry and ExportEntry
information independently of executable ModuleItems; linking creates module
environments for a connected graph before evaluation, and import bindings are
immutable indirect references to the resolved exporter binding. Cyclic module
records use graph traversal for both linking and evaluation, so function
declarations are instantiated before an evaluator in the strongly connected
component can call them.

`parse_module` now returns a `Module` with local, indirect, star and namespace
export entries, plus named/default/namespace import entries. `compile_module`
records the entry metadata and a top-level declaration-instantiation boundary.
`Vm::execute_module_graph` accepts host-resolved source keys and resolves
relative requests against those keys. It first visits the complete reachable
graph, allocates one cell per module binding, aliases named import slots to
the exporter cell, runs every declaration prefix, then evaluates dependencies
depth-first. Local assignments therefore update imports after evaluation.
Default function declarations have a dedicated hoisted binding, rather than
being incorrectly lowered to a later `const` initializer.

The Test262 runner now recursively supplies relative `_FIXTURE` sources to
the adapter. The initial `language/module-code` run at
`target/test262-module-final` recorded **109 pass, 348 fail and 145
unsupported** of 602 modes, versus the dependency-free baseline's 26 passes.
Representative named import, indirect re-export, default-function cycle and
namespace binding cases passed. Public regressions cover named live cells,
function-cycle instantiation, default export hoisting, indirect exports/import
immutability, and namespace property updates.

## P1.4 continuation: namespace exotics, dynamic import and top-level await

The [ECMAScript 2026 Module Namespace Exotic Object algorithms](https://tc39.es/ecma262/2026/multipage/ordinary-and-exotic-objects-behaviours.html)
were checked on 2026-09-11. Namespace exports now retain exporter cells in the
heap rather than copied values. Their `[[GetOwnProperty]]` descriptors are
live, writable/enumerable/non-configurable data descriptors; `[[Set]]` still
rejects export writes. Namespace objects have a `null` prototype, are
non-extensible, list string exports in UTF-16 code-unit order before ordinary
symbol keys, and own a non-writable/non-enumerable/non-configurable
`Symbol.toStringTag` value of `"Module"`. The VM roots one cached namespace
per realm module, so static namespace imports and repeated dynamic imports
observe object identity as required. `Reflect.defineProperty`, `Reflect.set`,
`Reflect.deleteProperty` and `Reflect.preventExtensions` expose the relevant
boolean internal-method results; `Object.seal`/`freeze` and
`isSealed`/`isFrozen` make the integrity-level distinction observable. The
focused `language/module-code/namespace` run at
`target/test262-namespace-exotic-final-2` records **21 pass and 17 fail** of
38 modes; remaining failures require unrelated iterator/callable support or
the still-incomplete TDZ error-object path.

`import()` is parsed as an expression rather than a static ModuleItem. A host
provides a finite precompiled registry and referrer key; relative requests are
resolved within that registry and no filesystem lookup occurs in the VM. A
dynamic-import job links/evaluates a previously unseen entry and fulfills its
Promise with the cached namespace. The Test262 collector includes reachable
literal dynamic-import fixtures for module and script cases. The Promise slice
adds `resolve`, `reject`, `all`, reactions and loader jobs sufficient for this
path and `$DONE` draining.

Module-goal `await` compiles to an Await opcode. Settled Promise completions
continue immediately; for a pending loader Promise, the VM roots and saves its
current module execution state, drains queued jobs, then restores the frame
and continues. A Promise still pending after that drain remains an explicit
unsupported outcome: generic resumable async frames, thenable assimilation,
async dependency ordering and host-driven loading are not claimed. The fresh
`language/module-code` run at `target/test262-module-namespace-dynamic-tla-final`
records **310 pass, 152 fail, 137 unsupported and 3 timeout** of 602 modes.
The focused `language/expressions/dynamic-import/reuse-namespace-object` run
at `target/test262-dynamic-import-reuse-final` has **5 pass of 5**, and the
focused `language/module-code/top-level-await/await-awaits-thenable-not-callable`
run at `target/test262-tla-basic` has **1 pass of 1**. These are bounded
acceptance slices, not a claim of complete module or async conformance.

### Implementation update: complete `language/module-code` boundary

Implemented 2026-09-11. The final P1.4 gap audit started from **578 pass, 12
fail and 12 unsupported** modes. The unsupported set was deliberately handled
first: module-only early syntax errors (`export default var`/lexical
declarations, direct invocation of anonymous default function declarations,
invalid updates, lone `?`, and HTML-like comments) are now classified as known
SyntaxErrors. A syntax or compiler early error in a requested fixture is
reported at resolution phase, while the entry record remains parse phase.
This keeps a subset-parser rejection from falsely passing a negative test.

`Module` now preserves executable `[[RequestedModules]]` in source-text order
and bytecode retains that sequence separately from the import/export tables.
`Vm::evaluate_module_record` and async-cycle reachability follow it directly.
This corrects interleaving such as `import A; export {} from B; import C`,
including the zero-specifier export case that otherwise has no ExportEntry.
Source-phase imports remain excluded from evaluation while retaining their
separate linking validation. The Test262 runner also collects fixture edges
whose import or export names are string literals, so namespace-name tests do
not accidentally run with an incomplete host registry.

The completion audit found a second suspension defect: handler stack offsets
were restored relative to the entire saved stack for top-level-await and async
continuations. That stack is the suspended frame itself, rather than an
ambient caller frame, and a subsequent `await` could underflow while
re-serializing a catch/finally handler. Interpreter restoration now records
the correct base for each boundary: zero for isolated module/async frames and
the reconstructed frame base for ordinary generators. This removes the crash
in repeated caught top-level awaits while retaining P1.3 generator-finalizer
semantics.

Namespace dynamic import now materializes a namespace for an already evaluated
static dependency when no prior namespace access created it. `Reflect.get` is
available alongside the other Reflect internal-method adapters, including
integer-indexed module namespace export names. Public regressions cover
interleaved import/export dependency ordering, static DFS followed by a
dynamic import of that dependency, requested-fixture resolution phase, module
HTML comments, integer namespace keys, and repeated top-level-await catches.

The pinned eight-worker run at `target/test262-module-p14-complete` records
**602 pass, 0 fail, 0 unsupported and 0 timeout** across 599 files and 602
scheduled modes (`language/module-code`, two-second deadline, 100,000
dispatch budget). The analyzer reconciles every mode with blocker `none`. Its
adapter SHA-256 is
`ece0d6ccf1becc765520da156ca3d86477435e38d8e15531ca1602bc719049a5` and
runner SHA-256 is
`cc2c798f5ccc2bb9a731c9b42c9a652fae2a072ecbfdfbec05518e5c5594106e`.
This closes the P1.4 `language/module-code` scope for the pinned snapshot; it
does not imply complete Test262 coverage for unrelated grammar, host loading,
storage, library, or Intl workstreams.

## P0.2/P0.3 follow-up: eval Annex B and assignment references

Sloppy direct eval now records the exact captured cells that a dynamically
declared `var` or function binding masks. A name-only lookup incorrectly
treated an eval-local Annex B block-function cell as the outer dynamic binding
of the same name. The cell-based relation preserves the block function's
independent mutable lexical binding while still making a dynamically declared
name shadow a closure capture from an outer function. The relation is retained
when a generator suspends and resumes.

Destructuring assignment defaults now use `SetFunctionName` inference for an
anonymous function, class, or arrow when their target is an identifier. Simple
computed member assignment retains the raw property value until `PutValue`, so
the RHS runs before observable `ToPropertyKey` coercion. Direct global-object
access materializes lazy standard globals before a get or set, preserving the
non-writable descriptors of `undefined`, `NaN`, and `Infinity` in strict code.

Fresh focused runs using the pinned corpus and eight workers record
**924/924** passing modes for `language/eval-code` at
`target/test262-current-eval-code-final`, and **1,185 pass, 191 fail,
112 unsupported** of 1,488 modes for `language/expressions/assignment` at
`target/test262-current-assignment-final`. The remaining assignment modes are
principally unsupported grammar/async-generator cases and unrelated
destructuring iterator paths; these measurements do not close P0.2 or P0.3.

## P0.1–P0.3 continuation: assignment slice closure

The assignment parser now recognizes exponentiation, logical-assignment and
optional-chain syntax sufficiently to classify all assignment-target early
errors, while execution of positive grammar outside the implemented runtime
surface remains explicit. Parenthesized expressions retain their
AssignmentTargetType: `(name)` is a valid reference but is not an
IdentifierReference for anonymous function name inference. Strict-mode
`(eval)` and `(arguments)` assignments continue to fail during early-error
checking.

Identifier assignment and update bytecode now retain a resolved binding
reference across RHS execution. This preserves PutValue semantics when a
sloppy direct eval introduces a same-named `var` while the RHS runs. Generator
suspension retains active destructuring iterator records, including the
required close on `.return()`. Keyed object and array destructuring preserve
the specified target/source-key evaluation order, and computed `super`
assignment defers `ToPropertyKey` until PutValue.

The intrinsic surface gained `Array.prototype.reduce`, and the realm global
no longer incorrectly exposes a read-only function `length` property.
`Proxy` currently implements construction and the `has` trap needed by
with-environment lookup; its target and handler are heap-traced. Other Proxy
internal methods remain deliberately outside this bounded slice.

A fresh eight-worker run at `target/test262-current-assignment-complete-3`
records **1,488 pass, 0 fail and 0 unsupported** of 1,488 scheduled
`language/expressions/assignment` modes. This closes that focused assignment
slice only; it is not a claim of complete ECMAScript conformance.

## P0.3/P1.1 continuation: exponentiation expressions

The [Exponentiation Operator](https://tc39.es/ecma262/2026/multipage/ecmascript-language-expressions.html#sec-exp-operator),
[Number::exponentiate](https://tc39.es/ecma262/2026/multipage/ecmascript-data-types-and-values.html#sec-number-exponentiate),
and [BigInt::exponentiate](https://tc39.es/ecma262/2026/multipage/ecmascript-data-types-and-values.html#sec-bigint-exponentiate)
were checked on 2026-09-11. Exponentiation parses as a right-associative
operator whose base is an UpdateExpression: an unparenthesized unary base such
as `-x ** y` is an early SyntaxError, whereas `x ** -y` and prefix/postfix
updates are valid. Parenthesized unary bases remain valid.

The compiler emits `Exponentiate` for both `**` and `**=`. The VM applies
ToNumeric to the left operand before the right, rejects mixed Number/BigInt
values, preserves exact BigInt results and rejects a negative BigInt exponent
with RangeError. Number execution additionally handles the specified
`abs(base) === 1` with infinite exponent case as NaN, rather than inheriting
the host `powf` result of one. Regression coverage locks grammar, association,
updates, coercion order, signed zero/NaN behavior, BigInt arithmetic and
error categories.

A fresh eight-worker run at `target/test262-next-exponentiation-complete`
records **88 pass, 0 fail and 0 unsupported** of 88 scheduled
`language/expressions/exponentiation` modes. This is a focused reference and
grammar slice, not a completion claim for all numeric operations.

## P0.3 continuation: logical assignment references

The [logical-assignment evaluation algorithms](https://tc39.es/ecma262/2026/multipage/ecmascript-language-expressions.html#sec-assignment-operators)
were checked on 2026-09-11. `&&=`, `||=` and `??=` evaluate and read their
left-hand Reference before deciding whether to evaluate the RHS. Their bypass
returns the old value and must not call PutValue; the assignment path retains
that original Reference through RHS evaluation. Anonymous function, class and
arrow RHS values receive identifier names only on the assignment path.

`DiscardReference` removes a retained binding/property/super Reference beneath
the bypass result without performing a write. This makes the existing reference
opcodes support short-circuiting across captured bindings, `with`, members and
`super`. Computed member References now check a nullish base before observable
ToPropertyKey conversion, retaining one canonical key for the later write.
Regression cases cover all three operators, RHS elision, target evaluation
count, strict non-writable bypasses, name inference, BigInt truthiness and
the nullish-base/key-coercion error order.

The P1.2 private-slot bridge represents a private Reference as its object base
and private name, with a bytecode operand selecting a compiler-generated,
captured lexical owner binding. Heap-private brand, element and slot maps
retain their GC edges but are absent from ordinary own-property enumeration.
Fields are writable slots; methods reject `PrivateSet`; accessors call their
getter/setter with the original receiver and reject a write when no setter
exists. The same owner binding drives `#name in object`, static private
elements, and the constructor's instance-brand initialization.

A fresh eight-worker run at `target/test262-private-slot-final` records **138 pass,
0 fail and 0 unsupported** of 138 scheduled
`language/expressions/logical-assignment` modes, including the 42 generated
`class-fields-private` modes that had previously rejected `#` during parsing.
This closes that reference slice only; it is not a full private-name or class
conformance claim.

The follow-up private-name work was checked on 2026-09-11. Fresh eight-worker
runs record **44 pass, 0 fail and 0 unsupported** of 44 scheduled
`language/statements/class/elements/private-static` modes, **30 pass, 0 fail
and 8 unsupported** of 38 scheduled `language/expressions/in/private-field`
modes (the eight are async-host gaps), and **40 pass, 0 fail and 0 unsupported**
of 40 `privatename-not-valid` modes. The wider class-element early-error run
now records **444 pass, 0 fail and 0 unsupported** of 444 modes. The public
class grammar follow-up classifies parenthesized-arrow heritage, duplicate or
special-form constructors, and static method/accessor `prototype` names as
specified parse-time errors.

## P1.3 foundation: ordinary async-function continuations

Ordinary `async function` execution now uses the same displaced interpreter
state model as top-level `await`. Reaching `Await` saves the bytecode program
counter after the opcode, operand stack, binding cells, completion/handler
state, active iterators, `this`, arguments, `new.target`, lexical home state
and call depth in an `AsyncContinuation`. The continuation owns its result
Promise and is registered on the awaited Promise as a distinct job reaction.
Both fulfilled and rejected inputs resume only in a later Promise job turn;
rejection re-enters the preserved completion handlers rather than escaping the
async call.

`SuspendedModuleExecution` is now a shared full-frame representation instead
of an implicit module-only GC assumption. The VM enumerates and roots every
object edge from module and ordinary-async continuations at all allocation
safepoints, including captured cells, pending abrupt values, iterator records,
dynamic-eval cells and private frame metadata. This is the reusable P1.3 base
for async generators and async iteration; those protocols, their request
queues and host-driven loading remain separate work.

Public regressions cover pending resolution, rejection into `catch`, two
successive awaits, Promise-return adoption, observable job ordering,
post-suspension template-object identity, and a captured object surviving
allocations while the frame is suspended. A fresh eight-worker run at
`target/test262-next-async-functions` records **134 pass, 27 fail and 0
unsupported** of 161 scheduled
`language/expressions/async-function` modes. Eighteen remaining modes are
classified async grammar/early-error gaps; the other nine require unrelated
function-name, `new.target`, `with`/`Symbol.unscopables` semantics. This is a
continuation foundation, not a claim of async-generator or complete async
conformance.

## P1.1/P0.2/P1.3 continuation: async function expression environments

The contextual async grammar, [Function Environment Records](https://tc39.es/ecma262/2026/multipage/executable-code-and-execution-contexts.html#sec-function-environment-records), [Object Environment Record HasBinding](https://tc39.es/ecma262/2026/multipage/executable-code-and-execution-contexts.html#sec-object-environment-records-hasbinding), and [Arrow Function Definitions](https://tc39.es/ecma262/2026/multipage/ecmascript-language-functions-and-classes.html#sec-arrow-function-definitions) were checked on 2026-09-11. An escaped `async` can no longer select an async function or arrow production, and `await` is now rejected as an async-function binding identifier, function name, or label. These are known parse `SyntaxError` results rather than subset-parser accidents.

Immutable bindings now distinguish `const` from the non-strict immutable name
environment of a named function expression. The latter ignores a sloppy write
but rejects a strict reference, including a direct `eval` that captures the
name. The eval visible-binding collection includes initialized bindings that
are not block-scope entries, so a named function expression's self binding is
captured as a cell before eval executes. This preserves the binding through
allocation and keeps direct eval from falling through to a global name.

`with` lookup now implements the `Symbol.unscopables` branch of object
environment `HasBinding`, with the unscopables object rooted across an
observable property read. A blocked object property falls back to the innermost
lexical binding: bytecode captures precede local bindings, so that lookup uses
the last matching slot rather than accidentally selecting an outer capture.
Nested arrows inherit their containing function's syntactic `new.target`
context, while ordinary nested functions establish their own context.

The public parser/compiler/VM regression covers escaped contextual keywords,
`await` bindings and labels, sloppy and strict named-async-function writes
(including direct eval), unscopables lookup with a same-named captured outer
binding, and an async arrow's lexical `new.target`. A fresh pinned Test262 run
at `target/test262-next-async-functions-environment-final` reconciles **161
pass, 0 fail and 0 unsupported** of 161 scheduled
`language/expressions/async-function` modes, up from **134 pass and 27 fail**;
every previous failure transitioned to pass. This closes that focused ordinary
async-function-expression selection, not async generators, async iteration,
thenable assimilation, host-driven loading, or full async conformance.

## P1.1/P1.3 continuation: async-arrow parameter contexts and intrinsics

The [Async Arrow Function Definitions](https://262.ecma-international.org/17.0/#sec-async-arrow-function-definitions)
and [AsyncFunctionCreate](https://262.ecma-international.org/17.0/#sec-async-functions-abstract-operations)
algorithms were checked on 2026-09-11. The parser now keeps the async grammar
parameter active while it parses an async arrow's formal parameter list. That
includes defaults containing nested arrows, so `await` cannot silently become
an identifier in either an outer parameter or a nested arrow parameter. Arrow
formal parameters are always `UniqueFormalParameters`, including in a sloppy
surrounding script; this preserves the legacy duplicate-name allowance only
for the ordinary simple-function form where the specification permits it.

The VM now lazily creates one rooted `%AsyncFunction.prototype%` per realm and
its non-global `%AsyncFunction%` constructor. Every async closure, including
an async arrow, inherits from that prototype rather than `%Function.prototype%`.
The intrinsic has the specified Function constructor/prototype chain, name,
length, descriptors and `Symbol.toStringTag`. Calling the constructor compiles
an `async function anonymous` in the realm global environment and therefore
uses the existing Promise continuation machinery; it remains non-constructible.
The ordinary and async dynamic-function paths share allocation/root handling,
without pretending that either generator constructor family is complete.

The pre-change `language/expressions/async-arrow-function` run had **97 pass
and 13 fail** of 110 scheduled modes. Eleven failures were unrecognized
`await`/duplicate-parameter early errors and two were the missing
`%AsyncFunction.prototype%` relationship. The fresh pinned run at
`target/test262-next-async-arrow-functions-final` has **110 pass, 0 fail and
0 unsupported**. Public regressions verify the prototype chain, dynamic
constructor async settlement, no own `prototype` on the resulting closure,
and rejection of construction. This closes the focused async-arrow selection,
not async generators or async iteration.

## Next architecture-first slice: async generators and iteration

The most consequential immediately-ready P1.3 gap is async generators. The
same pinned `language/expressions/async-generator` baseline at
`target/test262-next-async-generators-baseline` schedules 1,212 modes with
**376 pass, 594 fail and 242 unsupported**. The repeated first symptom is
`TypeError: value is not callable`: current `async function*` code reaches the
ordinary generator path, whose `.next()` returns an immediate iterator-result
object instead of a Promise. That hides later destructuring, abrupt-close and
async-iterator assertions; it is not evidence that those individual algorithms
are all independently broken.

The next implementation must therefore be a new shared runtime boundary, not
a per-builtin patch: a rooted async-generator state with FIFO `next`/`return`/
`throw` requests; Promise-returning methods; suspension through both `yield`
and `await`; completion/iterator-close propagation; and GC tracing of queued
arguments, Promise capabilities and displaced frames. `for await` and
`AsyncFromSyncIterator` should consume that same protocol afterward. P0.4's
remaining receiver-aware object/Proxy internal methods remain the broader
cross-cutting prerequisite, but this P1.3 slice is now executable directly on
the already-established continuation and Promise architecture and exposes a
single shared failure across hundreds of modes.

## P1.3 continuation: serialized async-generator requests and injected completions

Design continued 2026-09-11 after `38fba2e` and `bbf2334`. Those commits add
the essential first part of the preceding slice: async-generator prototypes,
Promise-returning `next`/`return`/`throw`, suspended async frames, basic `for
await`, and async `yield*` forwarding through sync or async iterators. In
particular, the latter preserves a delegate's `next` argument and its final
value, and forwards a direct `return` or `throw` into an active delegate.

They deliberately do not complete the request protocol. Today
`async_generator_request` starts every request immediately and an
`AsyncContinuation` owns the request Promise directly. When the first request
reaches `await`, `generator_next` stores `GeneratorState::Done` while the live
frame is held by that continuation; a second request can consequently observe
a completed generator before the first request resumes. After a `yield`, a
second request can also execute before the first yielded value's asynchronous
settlement has completed. Outside `yield*`, `return` and `throw` currently
close the suspended generator directly, so a surrounding `finally` cannot
yield for a return request and a surrounding `catch` cannot consume a throw
request. These are one execution-model gap, not independent builtin bugs.

The checked-in 1,212-mode async-generator measurement above is a
pre-implementation baseline, not a result for the two commits. After the
queue, handler-completion, and delegate-metadata implementation, the pinned
8-worker filtered run at `target/test262-async-generator-p13-final` records
**1,212 pass, 0 fail, 0 unsupported and 0 timeout** across 623 files, with the
100,000-dispatch budget and two-second deadline. Its adapter SHA-256 is
`ece0d6ccf1becc765520da156ca3d86477435e38d8e15531ca1602bc719049a5` and the
runner SHA-256 is
`cc2c798f5ccc2bb9a731c9b42c9a652fae2a072ecbfdfbec05518e5c5594106e`.
The analyzer reconciles all 1,212 modes with blocker `none`. This focused
result verifies this P1.3 boundary; it does not establish full Test262
conformance outside async-generator expressions.

The implementation follows [AsyncGeneratorStart,
AsyncGeneratorEnqueue, and AsyncGeneratorResumeNext](https://tc39.es/ecma262/2026/multipage/control-abstraction-objects.html#sec-asyncgeneratorstart),
[AsyncGeneratorYield](https://tc39.es/ecma262/2026/multipage/control-abstraction-objects.html#sec-asyncgeneratoryield),
and [AsyncGeneratorCompleteStep](https://tc39.es/ecma262/2026/multipage/control-abstraction-objects.html#sec-asyncgeneratorcompletestep).
It has the following boundaries and invariants.

1. Give every async-generator object heap-owned control data, rather than a
   VM side map: an execution status (`suspended-start`, `suspended-yield`,
   `executing`, an await gate, or `completed`), a FIFO `VecDeque` of request
   records, and an optional explicit active-delegate record. A request records
   a monotonic request ID, a normal/return/throw completion with its value,
   and the Promise capability's target. Refactor the current shared
   `GeneratorState::{Start,Suspended,Done}` storage into a frame plus this
   async control envelope, so a displaced frame may be absent while its queue
   and status remain owned by the generator. Do not put this queue in a VM
   `HashMap`: a discarded generator would otherwise leave its queued promises
   and argument values rooted or stale outside heap lifetime accounting.

2. Native `next`, `return`, and `throw` must only create a target Promise,
   append one request, and call one `resume_next` scheduler. The scheduler may
   begin the first request synchronously, as the abstract operation does, but
   it must never run a later request while the status is `executing` or behind
   an await gate. `AsyncContinuation`, `PromiseReaction`, and `PromiseJob`
   carry the generator and request ID; the head queue record remains the sole
   owner of the target Promise. A continuation or reaction whose ID is no
   longer the head is an internal invariant failure, never an opportunity to
   settle a later request.

3. Resume a normal head request by supplying its value at the saved `yield`
   continuation. Resume return and throw heads as `Completion::Return` and
   `Completion::Throw`, routed through the existing completion-handler
   machinery before interpreting more bytecode. This lets `finally` run and
   lets `catch` turn a thrown request into another yield. Only a completed
   frame, an uncaught abrupt completion, or the specified completed/start
   special cases may call `AsyncGeneratorCompleteStep`; direct
   `generator_return`/`close_async_generator` remains appropriate only after
   that completion model has selected closure and iterator cleanup.

4. A `yield` leaves its head record in place while `AsyncGeneratorYield`
   awaits its value. Fulfilment changes that iterator result's `value`,
   completes and removes exactly the head, marks the generator
   `suspended-yield`, then invokes `resume_next` for the following record.
   Rejection performs the required close/reject path before it advances the
   queue. Even an already-fulfilled `Promise.resolve(value)` takes a Promise
   job turn here; directly settling it makes a later request observable before
   the required await boundary. Awaiting in the body, yielded-value awaiting,
   and delegate-method awaiting need distinct gate metadata, but share that
   one head-completion path.

5. Integrate the existing `yield*` paths with the queue instead of passing a
   free-standing target Promise through them. A queued return or throw may
   call a delegate method only when it reaches the head. Its fulfilled
   iterator result either becomes the outer request's yielded result or
   resumes the outer frame with the final value; rejection, a non-object
   result, and a missing delegate `throw` complete that same head with the
   specified error and then advance the queue. Store active delegation as
   frame metadata rather than inferring it from a bytecode offset pattern, so
   compiler layout changes cannot alter externally visible request semantics.

6. Extend heap tracing and byte accounting to include every queued completion
   value, target Promise, active delegate and frame edge. Extend VM roots for
   continuation/reaction/job records to include the request ID's generator and
   any result object until that record is removed. Allocation in a queued
   argument, a delegate getter, a thenable, iterator closing, or a Promise job
   must not collect either the generator, the first request Promise, or a
   later queued object. Removing a settled request must release those edges.

The regression-first acceptance set belongs in
`backend/bluejs/tests/function_environments.rs`, with adapter-facing cases in
`process_hosts.rs` where `$DONE` ordering is observable. It must cover two and
three immediate `next` calls, a second request while the first body `await` is
pending, FIFO mixing of `next`/`return`/`throw`, a return that yields from a
`finally`, a caught and an uncaught throw, and a request after completion.
Repeat those cases through `yield*` with sync and async delegates, including a
missing `throw`, a delegate result that is not an object, and a pending
delegate method. Force collection between enqueue, await settlement and queue
drain while values, Promise targets and delegate iterators remain observable.
Assertions must also establish that the first request's await job settles
before its successor executes, rather than merely checking final values.

After the public regressions are green, regenerate the focused evidence with
the installed pinned corpus:

```text
cargo test -p blueice-bluejs --test function_environments
python3 backend/bluejs/test262/run.py --filter language/expressions/async-generator --output target/test262-async-generator-p13-final
python3 backend/bluejs/test262/analyze.py --run target/test262-async-generator-p13-final --output target/test262-async-generator-p13-final/analysis
```

Record the scheduled/pass/fail/unsupported/timeout counts, executable hashes,
and path/mode transitions in this file. Remaining failures must then be split
between parser/early-error work, generator constructor and prototype details,
full async-iterator/`AsyncFromSyncIterator` closing semantics, and unrelated
object or builtin prerequisites. A successful filtered run closes only this
serialized-request boundary; host-driven module loading, Proxy/object
internals, typed arrays/shared memory, the remaining builtin families,
ECMA-402, and the full Test262 inventory remain their existing workstreams.

### Implementation update: complete serialized-request boundary

Implemented 2026-09-11. Async-generator objects own the request queue in their
heap record, including queued completion values and target Promises for GC
tracing and byte accounting. A single scheduler admits only its head while the
generator is suspended; `await` in the body and `AsyncGeneratorYield` both
install an await gate. The latter always completes in a Promise job, including
an already-fulfilled yielded value. Head completion removes exactly one
request, settles its target, then drains later completed requests iteratively
or starts the next suspended request. Queued `next`, `return`, and `throw`
therefore cannot observe the temporary `GeneratorState::Done` used while a
continuation owns the live frame.

`GeneratorState::Suspended` now carries handler stack offsets relative to its
saved frame, pending catchable completions, saved normal completions, and
active async-delegate metadata. Interpreter entry applies the explicit frame
base when an ordinary generator is rebuilt above an ambient caller stack; an
isolated module or async continuation uses zero because its saved stack is the
frame itself. Suspension converts offsets back with that same base. A queued
return now flows through a paused `finally`, including one that awaits before
yielding; a queued throw flows through a paused `catch`. These saved values
and the active delegate record are heap references and contribute to
managed-byte accounting, so collection cannot discard a live finalizer,
caught exception, or delegated iterator.

The compiler records the resume and exit offsets of each async `yield*` loop.
At its public yield boundary the generator saves an explicit delegate record;
return and throw forwarding consult that record rather than scanning a bytecode
pattern. A completed delegate return resumes the outer return completion, which
allows an enclosing outer `finally` to yield before the request completes.
When a synchronous generator finishes from an injected `return` or `throw`,
its saved destructuring iterator records are closed before the suspended frame
becomes done; the completion and records remain rooted while close callbacks
run. This prevents an abrupt generator termination from skipping
`IteratorClose`.
Regressions cover FIFO requests across a body await, yielded-value job order,
return/finally and throw/catch injection, finalizer await suspension, sync and
async delegate forwarding, outer finalizers, pending delegate returns,
non-object delegate results, missing delegate throws, and heap edges in a
suspended frame. The focused Test262 result above and its reconciled analysis
are the completion evidence for this P1.3 slice. Host-driven module loading,
Proxy/object internals, typed arrays and shared memory, the remaining builtin
families, ECMA-402, and the full Test262 inventory remain separate workstreams.

## P1.3 continuation: ordinary generator `yield*` and full-inventory status

The complete 2026-09-12 inventory at
`target/test262-unsup-timeout-current` ran all 53,404 files and 102,578 modes
without the runner aborting on a non-JavaScript fixture: **59,366 pass, 41,046
fail, 870 unsupported, 1,120 timeout, and 176 harness errors**. This is a
diagnostic baseline, not a conformance claim. The unsupported modes group into
ordinary `yield*` (145 modes), optional chaining (94 modes), parser/early-error
classification, host boundaries, and proposal syntax. Most timeouts are the
large generated Unicode RegExp corpus exhausting the deliberately bounded
100,000-dispatch policy; increasing the wall deadline alone does not make those
inputs complete.

Ordinary generators now use the same explicit delegate-boundary design as
async generators. The compiler records each synchronous `yield*` resume and
exit offset. A suspended frame retains its iterator record, so `next(value)`
forwards its value to the delegate; `throw(value)` and `return(value)` invoke
the delegate methods, validate iterator results, preserve closing behaviour,
and resume the outer frame with the delegate's completion value. The delegate
record is traced and charged as managed state. `%GeneratorPrototype%` now also
exposes the standard `throw` method. Focused checks at
`target/test262-sync-yield-star-language` and
`target/test262-annexb-sync-yield-star` pass all 8 scheduled modes, including
the two original Annex B missing-method cases. The broader `yield-star`
selection has no unsupported modes (1,384 pass and 62 unrelated failures).

The Test262 adapter additionally ignores legacy fixture diagnostics through a
Test262-only `print` no-op. The affected
`built-ins/RegExp/prototype/Symbol.replace/coerce-global.js` selection now
passes both modes at `target/test262-print-host-fixed`.

## P0.4 and P1.5 slice: internal-method boundary and fixed binary data

Implemented 2026-09-12. Ordinary object operations now pass through a VM
boundary for receiver-aware `[[Get]]` and `[[Set]]`, own-property descriptors,
own keys, prototype/extensibility changes, and callable/constructor checks.
This is the same boundary used by Proxy traps rather than a second
trap-specific object model. Proxy `get`, `has`, `set`, `deleteProperty`,
`ownKeys`, `getOwnPropertyDescriptor`, `defineProperty`, prototype,
extensibility, `apply`, and `construct` dispatch use it and enforce their
applicable target invariants. A revocable Proxy now clears its target and
handler edges on revocation; a native revoker retains the proxy while live so
GC cannot discard the internal slot before it is called. Callable and
constructible capability is retained separately, which keeps `typeof` correct
for a Proxy whose target was already revoked while calls still throw.
The ordinary prototype operation rejects a proposed cycle before mutating the
heap; its public boundary returns `false` through `Reflect.setPrototypeOf` and
causes `Object.setPrototypeOf` to throw `TypeError`.

The Reflect adapters now cover `apply`, `getOwnPropertyDescriptor`,
`getPrototypeOf`, `setPrototypeOf`, and `isExtensible` as well as the earlier
operations. `Reflect.apply` validates the callable target before it reads its
array-like argument list and preserves the supplied receiver; object-only
Reflect operations reject primitive targets instead of boxing them.
`Reflect[Symbol.toStringTag]` is an own non-writable, non-enumerable,
configurable data property with value `"Reflect"`.

Descriptor conversion roots each observed descriptor value until conversion
finishes, rejects mixed descriptors, and completes a Proxy
`getOwnPropertyDescriptor` trap result before invariant checks and reflection.
`CopyDataProperties`, `Object.create`, `Object.defineProperties`, `for-in`,
JSON property enumeration, descriptor `HasProperty` checks, sparse-array
checks, and `super` assignment now use that same boundary, so they cannot
skip a Proxy's own-key, descriptor, prototype, get, set, or has behavior. The
regressions cover explicit Reflect receivers, traps and their receivers,
descriptor completion, revocation including allocation pressure, a revoked
Proxy target, `freeze`, object spread, JSON, `for-in`, and construct-trap
arguments/newTarget. The focused Proxy run used four workers, a
100,000-instruction budget and a two-second case deadline: **468 pass / 139
fail / 0 unsupported / 0 timeout** of 607 modes under
`built-ins/Proxy`. It supersedes the prior 397/607 partial boundary result.
The sibling `built-ins/Reflect` selection is **298 pass / 10 fail** of 308
modes. Its ten residual modes require Date, `Symbol.for`, primitive-property
dispatch, or resizable/shared buffer support, which are outside this P0.4
boundary. Proxy residual failures include cross-realm paths and
Array/class/newTarget behaviour outside this object-method slice; neither
selection is a P0.4 or whole-suite completion claim.

The first non-shared binary-data baseline adds fixed-length `ArrayBuffer`
stores, backing-store accounting and tracing, detach state, `DataView` integer
and floating-point accessors, and numeric `Int8Array`, `Uint8Array`,
`Uint8ClampedArray`, `Int16Array`, `Uint16Array`, `Int32Array`, `Uint32Array`,
`Float32Array`, and `Float64Array`. Views hold their backing buffers as heap
edges and therefore survive minor and major collection. The implementation
supports buffer-backed, length, array-like, iterable, and numeric-TypedArray
copying construction; indexed access; shared backing bytes; `set` with an
offset; `subarray`; `BYTES_PER_ELEMENT`; `ArrayBuffer.isView`;
`ArrayBuffer.prototype.slice`; and the `ArrayBuffer[Symbol.species]` accessor.
`slice` now follows `SpeciesConstructor` and validates the constructed
non-shared result before copying. ArrayBuffer, DataView, and numeric typed
arrays create their instances from `newTarget.prototype`, with the intrinsic
prototype fallback for a non-object value. DataView preserves its `buffer`
slot after detachment, and performs `ToIndex`/value conversion before the
detach and range checks at the order required by its getter and setter
algorithms.
Concrete typed-array constructors now inherit from the non-global
`%TypedArray%` constructor, and their per-kind prototypes share its prototype.
Integer-indexed `[[Get]]`, `[[GetOwnProperty]]`, `[[DefineOwnProperty]]`,
`[[Set]]`, `[[Delete]]`, `[[HasProperty]]`, and `[[OwnPropertyKeys]]` keep
canonical numeric keys out of ordinary property lookup. In particular, a
distinct `Receiver` follows OrdinarySet for a valid typed index without
coercing its value; invalid canonical indices succeed without coercion. The
typed-array receiver itself performs `ToNumber` before it discovers that a
canonical key is `"-0"`, fractional, or out of bounds, including when that
typed array is reached through a prototype chain. This preserves abrupt
conversion completion and leaves the indexed result consistent with the
completed conversion. The conversion uses
ECMAScript Number-string formatting rather than Rust display formatting, so,
for example, `"0.0000001"` remains an ordinary property while `"1e-7"` is an
invalid numeric index. `Object.defineProperties` batches descriptor conversion
before applying the same definition boundary. Buffer allocation is bounded
before Rust allocation, and `ToIndex(NaN)` follows the zero-length path.

`$262.detachArrayBuffer` now calls the actual backing-store detachment path.
DataView access to a detached buffer throws `TypeError`; ArrayBuffer length and
numeric TypedArray length/byte-length/byte-offset expose their detached zero
state. Integer-indexed writes preserve `ToNumber` ordering: coercion may detach
the buffer, after which no byte is written but the completed indexed operation
still reports success. Resize/grow, SharedArrayBuffer, Atomics, BigInt typed
arrays and weak references are deliberately not part of this baseline.

Fresh focused evidence used the same runner settings: `built-ins/ArrayBuffer`
is **166 pass / 276 fail** of 442 modes (the initial missing-global baseline
was 0/442); `built-ins/DataView` is **804 pass / 318 fail** of 1,122 modes; and
the broad `built-ins/TypedArrayConstructors/ctors` selection is **160 pass /
296 fail / 2 timeout** of 458 modes. The integer-indexed
`built-ins/TypedArrayConstructors/internals` selection is **193 pass / 255
fail / 6 timeout** of 454 modes. The TypedArray selections intentionally still
contain SharedArrayBuffer and BigInt constructor cases plus broader Array
helpers, so they are coverage evidence for the fixed numeric baseline rather
than a conformance target. Public regressions in `binary_data.rs` cover shared
backing bytes, views, slices, numeric constructor copying, iterable input,
Float32/Float64, species, offset copying, common TypedArray prototypes,
canonical numeric properties, descriptor batches and detached-view behaviour;
heap tests cover binary-view reachability across both collectors.
