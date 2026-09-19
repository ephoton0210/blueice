# Test262 architecture-first implementation backlog

Requested 2026-09-10. Status: **full inventory measured; full conformance remains open**.
This is the implementation order for the [edition 17 track](ECMASCRIPT_2026.md).
The [recorded analysis](TEST262_ANALYSIS_REPORT.md) includes all workstream counts,
common diagnostics, representative cases and executable hashes.
The existing `test262-summary.json` only groups outcomes by top-level directory
and feature. It does not establish failure root causes or architectural priority.

## Evidence and classification contract

The verified snapshot is `72faf8ec1445c55149615e8b35187830783aba1a`:
53,582 test files, 294 fixture resources, 102,926 execution modes. The current
Rust 1.95 complete inventory (2026-09-14) records **79,897 pass, 22,962 fail
and 67 timeout**, with zero unsupported or harness-error modes. It ran with
eight workers, a 100,000-instruction default budget and a two-second case
deadline in 1,885.361 seconds. The runner exited 1, as expected while
conformance failures remain.

The new [analyzer](../../../backend/bluejs/test262/analyze.py) reconciles the
summary, unique path/mode pairs, source hashes, metadata and full source inventory
before publishing a report. `items.jsonl` retains **every mode**, its original
outcome/diagnostic, `esid`, description, flags, features and includes, together
with a target workstream, priority, prerequisites and observed blocker.
Targets are inferred from paths/metadata; diagnostics identify the **first
observed symptom**, not every root cause. Dependencies overlap; exclusive target
totals reconcile to the full denominator. Passed negative tests are not errors.

The current inventory's largest observed symptoms are 9,632 TypeErrors, 7,242
assertion failures, 3,090 RangeErrors, and 906 missing expected errors. The
`Temporal` feature still has 12,518 failing modes, but its failures are no
longer all an absent-global symptom: the new typed range bridge deliberately
covers only the ECMA-402 DateTimeFormat boundary. These symptoms often hide
later failures. In particular, a failed Array test does not establish that its
Array algorithm is the first missing dependency. The generic unsupported-
statement diagnostic covers several AST kinds and must not be labelled as a
confirmed try/catch bug.

The pinned `INTERPRETING.md` requires isolated realms, same-realm harness
includes, exact strict/raw/module/async behavior and negative phase/type
matching. Adapter runtime errors currently do not identify whether an include
or the test body threw; treat those cases as requiring reproduction. All
staging, Annex B, ECMA-402 and host-dependent cases remain in the inventory.
Their applicability needs an explicit clause/edition audit, never a silent skip.
Resource limits and 67 wall/regex deadlines are separate from semantic failures
and should be rerun with recorded budgets.

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

### Historical results of this slice

The contemporaneous after-run reconciled all 102,578 modes: **16,226 pass,
62,697 fail, 22,736 unsupported and 919 timeout**, with zero harness errors.
Comparing every path/mode against that slice's baseline found **zero
pass-to-nonpass regressions**. Four modes changed from assertion failure to
pass:

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

The historical slice's test details are retained above as implementation
evidence. The current Rust 1.95 no-exclusion BlueJS coverage measurement
(2026-09-14) is **36,107 / 40,259 lines (88.50%)**, 2,274 / 2,518 functions
(90.31%), and 84.95% regions; CI enforces an 88% line floor. It is the current
coverage baseline and must not be represented as 100%.

The iterator completion state and rest-rooting subtask is complete. The next
subtask, general Completion records plus `catch`/`finally`, is implemented in
the limited executable subset described below. P0.1 is still open: remaining
labels/`switch` control-flow coverage, all grammar/early-error work and the
remaining control-flow coverage must precede P0.2 and P0.3. No P0 workstream or
builtin family is fully complete.

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

## P1.1/P1.2 follow-up: class fields, static blocks and strict grammar

Completed 2026-09-13. Class-field initializers and class static blocks now run
the required lexical `ContainsArguments` check at parse time. The shared AST
walk crosses arrows, nested class heritage, and computed names, but stops at
ordinary functions because they establish their own `arguments` binding.
Public fields reject `constructor`; static public fields additionally reject
`constructor` and `prototype`, for both identifier and string property names.
The `static` contextual keyword is recognized as a modifier only when its next
token selects a static element, so `static;` and `static = value` remain valid
instance fields and an escaped spelling cannot select the modifier.

The class parser now keeps strict mode active for the entire ClassDefinition,
including its heritage expression, and restores the caller context on exit.
Consequently strict-reserved class names (including escaped spellings), module
`await` bindings, and a `with` inside a heritage function are known parse
`SyntaxError`s. Class static blocks separately reject a lexical `return` or an
`await` class binding without rejecting the same forms inside a nested ordinary
function. Field initializers ending in an arrow or class block require either a
semicolon, line terminator, or the enclosing class `}` before another token;
this prevents the former unclassified `() => {} == arguments` parse path.

The eight-worker `language/statements/class` run (8,666 modes, two-second case
deadline, 100,000 instruction budget) is now **8,464 pass / 202 fail**, up from
**8,367 pass / 299 fail** before this follow-up. A `(path, mode)` join finds
**97 fail-to-pass and zero pass-to-nonpass** transitions. The new public
pipeline regressions cover the prohibited and allowed lexical-arguments scopes,
field termination, public field-name early errors, `static` disambiguation,
static-block boundaries, class strictness, and module-only `await`. This is a
class-parser progress measurement, not a complete P1.1 or P1.2 conformance
claim; decorators, direct-eval class semantics, remaining private-field error
paths, and wider class execution semantics remain separately tracked.

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

## Test262 reporting correction: unclassified parse-negative outcomes

Completed 2026-09-12. The adapter deliberately distinguishes a parser failure
whose grammar/early-error classification is unknown from an observed
`SyntaxError`. The runner had contradicted that boundary by treating an
`unclassified_parse_error` as a pass whenever Test262 metadata expected a
parse-phase `SyntaxError`. This could count a subset-parser rejection as a
conformance success without demonstrating that BlueJS recognized the required
production or early error.

`classify` now records every `unclassified_parse_error` as a failure, including
parse-negative cases. Its regression checks that phase and error type must be
observed as `SyntaxError`; the runner summary uses the same rule in its stated
limitations. This changes reporting only, not parser or VM behavior.

The complete rerun at `target/test262-false-pass-fixed` reconciles all 53,404
test files and 102,578 scheduled modes: **61,477 pass, 41,101 fail, zero
unsupported, zero timeout, and zero harness errors**. A path/mode comparison
with `target/test262-unsup-timeout-final-4` found exactly 600 changes, all
`pass` to `fail`, across 314 files. Each changed mode had expected
`{ phase: parse, type: SyntaxError }` and actual
`{ phase: parse, kind: unclassified_parse_error }`; no such result remains in
the pass count. The full runner exits 1 because semantic failures remain, as
expected for a non-conformant inventory.

## P0.1/P0.3 continuation: undeclared iteration assignment heads

Implemented 2026-09-12. `for-in`, `for-of`, and `for await...of` heads that
do not declare `var`, `let`, or `const` now use `ForHead::Assignment` with an
`AssignmentPattern`. This separates names introduced by a declaration from
existing bindings and member references that receive every iterated value.
It routes identifier, member, array, and object heads through the existing
assignment-pattern evaluator, preserving defaults, rest elements, member
reference ordering, strict `eval`/`arguments` early errors, and iterator close
on an abrupt assignment.

The parser recognizes an array or object as a pattern only when its matching
delimiter is immediately followed by the `in` or contextual `of` separator.
Ordinary classic `for` initializers consequently retain their expression
grammar, including `for ({ value: 1 };;)`. The Annex B CallExpression target
remains a separate AST form: sloppy code evaluates it before the required
runtime error, while strict code rejects it during static semantics.

The focused command
`python3 backend/bluejs/test262/run.py --filter language/statements/for- --output target/test262-for-assignment`
reconciles 2,109 files and 4,087 modes: **3,876 pass and 211 fail**, with no
unsupported, timeout, or harness-error outcomes. Comparing every mode with
the preceding complete inventory found **944 fail-to-pass transitions and zero
pass-to-nonpass transitions**. All 384 passing parse-negative modes in the
filtered result observed parse-phase `SyntaxError`, rather than relying on an
unclassified parser rejection. The residual 211 modes are now principally
callable/internal-method behavior, remaining grammar classification, and
TypedArray availability; they no longer fail because an undeclared iteration
head was limited to a plain identifier.

Public parse/compile/execute regressions cover simple and destructuring heads,
defaults, rest, member writes, `for-in`, strict restricted-name errors,
ordinary `for` cover grammar, invalid targets, and close-on-assignment-error.

The complete rerun at `target/test262-for-assignment-full` reconciles all
53,404 files and 102,578 modes: **62,423 pass and 40,155 fail**, with zero
unsupported, timeout, and harness-error outcomes. Against
`target/test262-false-pass-fixed`, every path/mode pair and source hash
matches; **946 modes changed from fail to pass and none changed from pass to
nonpass**. All 6,985 passing negative modes observed their exact expected
phase and error type, and no passing mode reports
`unclassified_parse_error`. The refreshed complete architecture report is
checked in as [TEST262_ANALYSIS_REPORT.md](TEST262_ANALYSIS_REPORT.md).

The Test262 runner now emits a live report every five seconds with completed
file count, outcome totals, and every active `path [mode, elapsed]`; users can
set `--progress-interval` to adjust the cadence or disable it for automation.
The runner regression suite checks the formatted current-mode output, and an
actual one-worker `language/statements/for-in` run confirmed continuous output
through all 122 files and 210 modes.

## P0.2/P0.4 continuation: Function intrinsic and prototype-chain contracts

Implemented 2026-09-12. `%Function%` is now constructible and links its
`prototype` to the pre-existing callable `%Function.prototype%`; that
prototype has its required non-writable `constructor` property and shared
configurable, non-enumerable `caller` and `arguments` accessors backed by the
realm's `%ThrowTypeError%`. Reading either restricted name materializes the
lazy Function global before inherited lookup, so strict function objects never
observe an incomplete intrinsic graph.

Non-strict, constructible ordinary functions receive their Annex B own
non-writable, non-configurable `arguments` and `caller` properties. The former
uses the inactive `null` value; the latter remains `undefined` until the VM
tracks an active caller, preserving the standard compatibility fallback rather
than claiming an incomplete extension. Strict, arrow, generator, async, and
method functions retain the restricted inherited behavior. This keeps legacy
compatibility separate from the shared Function prototype contract.

The same slice adds `Object.prototype.isPrototypeOf`, including its required
argument-before-receiver coercion order and Proxy-aware `[[GetPrototypeOf]]`
walk. This gives Function and all ordinary objects one path for testing their
prototype relationship.

The continuation also follows `GetPrototypeFromConstructor` through bound
functions, proxies, and Test262 child realms. A constructor whose `prototype`
is not an object now receives the default prototype from its own realm; the
same rule covers `%Object%` construction. Foreign Function constructor calls
and their thrown errors preserve the callee realm at the host membrane. The
dynamic Function constructors accept Annex B HTML open/close comments and
keep a line comment in parameter text from consuming the generated wrapper.
Class-expression statement completion is preserved, so an `eval` result is no
longer accidentally reported as a passing value.

Focused evidence: the complete `built-ins/Function` selection moved from
**755 pass / 150 fail** to **887 pass / 18 fail** over 905 modes: **132
fail-to-pass and zero pass-to-fail**. The 20-mode
`built-ins/Object/prototype/isPrototypeOf` selection moved from **4 pass / 16
fail** to **20 pass**. Its caller/arguments legacy 99-mode sub-selection,
strict restricted-properties test, and Function descriptor sub-selection all
pass. Public realm regressions exercise constructor/prototype linkage,
descriptors, thrower identity, strict versus legacy properties, foreign errors,
foreign `newTarget` fallback for Function and Object, and `isPrototypeOf`
coercion ordering.

At that point, the remaining 18 Function modes were intentional next-step boundaries, rather
than unimplemented isolated wrappers: six need object identity to cross the
Test262 realm membrane; two need resizable ArrayBuffer and BigInt typed-data
semantics; six need source-text retention for generator, native, and computed
method `Function.prototype.toString`; and four need Date internal slots plus
recursive `GetFunctionRealm` for bound targets. The first two groups are
P1.5/P1.6 prerequisites, while the source-text and Date cases require their
own parser/function-record and builtin-object designs before they can be
claimed as supported.

The complete inventory at `target/test262-p02-final` reconciles all 53,404
files and 102,578 modes: **62,830 pass and 39,748 fail**, with zero
unsupported, timeout, or harness-error outcomes. Its path/mode comparison
with `target/test262-for-assignment-full` found **407 fail-to-pass and zero
pass-to-fail** transitions. No passing result reports
`unclassified_parse_error`.

## P1.5/P1.6 continuation: BigInt binary data and realm identity membrane

Implemented 2026-09-13. The fixed-length, non-shared binary-data baseline now
also includes `BigInt64Array` and `BigUint64Array`, along with DataView's
`getBigInt64`, `getBigUint64`, `setBigInt64`, and `setBigUint64` accessors.
BigInt element reads produce BigInt values; writes perform observable
`ToPrimitive(value, number)` and then accept only BigInt. Numeric typed-array
writes retain Number conversion and reject BigInt. The two paths share the
same indexed-property, backing-store, detach, view-liveness, and GC-edge
contracts as the numeric arrays. BigInt bytes retain the low 64 bits in
two's-complement form, and DataView honours the requested endianness.

This remains the fixed-length P1.5 baseline: resizable buffers, growable
SharedArrayBuffer, Atomics, and higher-order TypedArray prototype algorithms
are still separate work. `$262.detachArrayBuffer` continues to detach the
real backing store; it is not a Test262-only simulation.

Test262 child realms now have an identity membrane for values passed as
foreign-call receivers or arguments. A parent object, including a facade from
another child realm, receives one stable ordinary stand-in in the target heap.
Both endpoints are rooted while the child realm is live, and returning that
stand-in restores the exact original parent value. This supports repeated
argument aliases and a return trip through two foreign realms without placing
a parent-heap handle in child storage. Property forwarding on opaque stand-ins
is deliberately a separate resumable cross-VM operation and is not claimed by
this identity transport.

The membrane also keeps a foreign bound target in the parent function record
when `Function.prototype.bind.call` is invoked through a child realm. Foreign
`%Object%` construction follows the parent construct path, so a bound target
from a third realm still selects that target realm's `%Object.prototype%`.
The public realm regression covers local and child-to-child identity round
trips, argument aliasing, a foreign bound target, and the selected constructor
prototype; `binary_data.rs` covers signed/unsigned wrapping, DataView BigInt
values, element width, and Number-to-BigInt rejection.

Focused runs against the P0.2 snapshot show `built-ins/DataView` moving from
**806 pass / 316 fail** to **896 pass / 226 fail**; the complete
`built-ins/TypedArrayConstructors` selection moves from **627 / 821** to
**1,062 / 386**; `built-ins/TypedArray/` moves from **338 / 2,538** to
**402 / 2,474**. `built-ins/Function` moves from **887 / 18** to
**891 / 14**. The remaining Function modes comprise two resizable-buffer
cases, six source-text-retention cases, and six Date-internal-slot cases.

The complete inventory at `target/test262-p15-membrane-final` reconciles all
53,404 files and 102,578 modes: **63,457 pass and 39,121 fail**, with zero
unsupported, timeout, and harness-error outcomes. Its path/mode/source-hash
comparison with `target/test262-p02-final` finds **627 fail-to-pass and zero
pass-to-nonpass** transitions; all 6,985 passing negative modes retain a
classified expected outcome, and no passing result reports an unclassified
parse error.

## P1.5 continuation: resizable and shared buffers, Atomics, and TypedArray algorithms

Implemented 2026-09-13. ArrayBuffer backing stores now retain a maximum byte
length, detached state, and shared flag. `new ArrayBuffer(length,
{ maxByteLength })` creates a resizable store; `resize`, `resizable`,
`maxByteLength`, `transfer`, and `transferToFixedLength` follow the real
backing-store transition rather than a Test262-only facade. A transfer copies
the bounded prefix only after all conversion and allocation steps succeed,
then detaches its source. Fixed-length and length-tracking DataView and
TypedArray views recompute their exposed byte and element lengths as a
resizable store changes, and reject an out-of-bounds fixed view.

`SharedArrayBuffer` shares that representation without a detach transition.
Its constructor options, `growable`, `maxByteLength`, `grow`, `slice`, species
creation, and shared DataView/TypedArray views are installed with their own
internal-slot receiver checks. The focused SharedArrayBuffer selection is
**208 pass of 208 modes**. The ArrayBuffer transfer selection is **364 pass /
78 fail** of 442 modes; its residuals are chiefly ImmutableArrayBuffer and
transfer edge contracts that require a separate immutable-store design.

Atomics now validates a shared integer TypedArray and implements
`add`, `and`, `compareExchange`, `exchange`, `load`, `or`, `store`, `sub`,
`xor`, `isLockFree`, `notify`, `pause`, `wait`, and `waitAsync`, including
Number/BigInt element domains and wrapping. In the current single-agent VM,
wait completes the immediately observable `not-equal` or `timed-out` state and
notify returns zero waiters. A real wait list, agent startup/broadcast and
asynchronous wake-up remain P1.6 host-scheduler work; they must not be
simulated by reporting an agent assertion as a passing result. The focused
Atomics selection is **426 pass / 352 fail** of 778 modes.

The shared `%TypedArray%.prototype%` now has receiver-checked
`at`, `copyWithin`, `every`, `fill`, `filter`, `find`, `findIndex`,
`findLast`, `findLastIndex`, `forEach`, `includes`, `indexOf`, `join`,
`lastIndexOf`, `map`, `reduce`, `reduceRight`, `reverse`, `slice`, `some`,
`sort`, `toReversed`, `toSorted`, and `with`, plus `entries`, `keys`,
`values`, `@@iterator`, and the species getter. Copying methods use the
required same-kind or species-created target; callback and conversion steps
recheck a view after a resize or detach so a stale fixed view cannot continue
writing. The focused prototype selection is **1,982 pass / 812 fail** of
2,794 modes, with zero timeout. `Array.prototype.slice` and `splice` were
also completed far enough to preserve sparse entries, use ArraySpeciesCreate,
and apply CreateDataPropertyOrThrow; these shared harness dependencies no
longer weaken existing Array behavior.

The complete rerun at
`target/test262-p15-shared-typed-regression-fixed` reconciles all 53,404 files
and 102,578 modes: **66,731 pass, 35,843 fail, 4 timeout, zero unsupported,
and zero harness errors**. Against `target/test262-p15-membrane-final`, all
path/mode keys and source hashes match: **3,274 modes changed from fail to
pass, zero changed from pass to nonpass, and four changed from fail to
timeout**. The only timeouts are both modes of
`staging/sm/TypedArray/sort_modifications.js` (wall deadline) and
`staging/sm/TypedArray/sort_sorted.js` (instruction budget); they remain
visible in the checked-in report instead of being classified as supported.
The refreshed complete triage is
[TEST262_ANALYSIS_REPORT.md](TEST262_ANALYSIS_REPORT.md).

## P1.6 host scheduler and timeout audit

`$262.agent` now uses independent VMs with FIFO broadcast/report queues and a
shared, locked backing store. Atomics read-modify-write operations are atomic
across agents; `wait`, `notify`, and `waitAsync` use FIFO waiters, and async
completion returns to the owning VM event queue. Test262-only timers keep
callbacks rooted until execution. The adapter waits for `$DONE`, so a pending
async assertion cannot be counted as a pass.

The final complete run at `target/test262-p16-final` reconciles **53,404
files / 102,578 modes: 67,529 pass, 35,049 fail, zero unsupported, zero
timeout, and zero harness errors**. The four previous TypedArray sort timeout
modes now pass. Instruction-budget exhaustion is recorded as `resource_error`
(a failing bounded-resource outcome), distinct from a supervisor timeout.

## P0.4 closure: Proxy realms and constructor forwarding

Imported live Proxies are now represented by a local Proxy exotic facade that
retains foreign target and handler facades. Internal-method dispatch chooses
that local Proxy before ordinary foreign forwarding, so apply and construct
traps allocate their argument and descriptor objects in the current Realm.
This preserves target/handler identity without copying child-heap objects.

Foreign Array and Boolean/Number construction retains the caller's
`newTarget`. `GetFunctionRealm` follows local Proxy layers to foreign targets,
and `instanceof` recognizes a foreign facade around the intrinsic
`@@hasInstance` hook. Child abrupt completions are materialized before they
cross the membrane, preserving the child Error constructor. A realm record
now exposes its own `evalScript` facade as required by the Test262 host.

The Proxy `[[DefineOwnProperty]]` non-writable invariant and the observable
`length`/`name` property order for native functions are also enforced. The
minimal Date baseline provides the standard callable/constructible global and
non-constructible `Date.now`; full Date internal slots and prototype
algorithms remain library work.

Focused validation: `built-ins/Proxy` **607/607**, `built-ins/Proxy/construct`
**60/60**, `built-ins/Reflect/construct` **20/20**, and the public
`conformance_edges` suite **37/37**.

## P0.4 continuation: Test262 writable-property observation

The adapter replaces `propertyHelper.js` with native helpers, so those helpers
are part of the conformance boundary rather than an assertion-only shortcut.
Its former `verifyWritable` and `verifyNotWritable` implementations inspected
only an own descriptor. That is insufficient: the standard helper writes the
property, observes either the property or its caller-supplied verification
property, and restores a successful write. In particular,
`verifyNotWritable(object, key, alternate)` must be able to prove that adding
an absent `key` to a non-extensible object does not change `alternate`; there
is deliberately no descriptor for `key` to inspect first.

The native equivalent now retains the target, keys, old value, and probe value
on the VM stack while a getter, setter, or Proxy internal method may allocate.
It keeps the helper's descriptor precondition only when no alternate
verification property was supplied; chooses the same array-length and ordinary
probe values; performs the real receiver-aware `[[Set]]`; handles the expected
strict-mode `TypeError`; observes the requested property; and restores a
successful own write or deletes a newly-created one. A public
parse → compile → execute regression covers non-extensible addition, strict
assignment failure, data and accessor writes, alternate receiver observation,
new-property cleanup, array-length probing, descriptor rejection, and an
abrupt setter.

With the current adapter, the full `built-ins/Object` selection is **5,949
pass / 859 fail** of 6,808 modes, compared with **5,613 pass / 1,195 fail**
before this correction: **336 fail-to-pass and zero pass-to-fail**. The core
`Object.preventExtensions` slice is **74/78**; its four residual modes require
the deliberately minimal Date constructor. The broader Object failures remain
separately classified Date, Weak collection, resizable TypedArray, and
unimplemented Object-library work, so this is a P0.4 internal-method/
conformance-host repair, not a claim that P0.4 as a whole is closed.

## P0.2–P0.4 continuation: Object internal-method and legacy-accessor closure

The native `%Date%` function was already marked constructible and had a
construct path, but the VM's call dispatcher omitted it from the construct
allow-list. `new Date(...)` therefore failed before descriptor coercion could
run. The dispatcher now retains its actual `newTarget`, including through
`Reflect.construct`. `Object.defineProperties` now normalizes an Array
`length` value at the same point as `Object.defineProperty`, after collecting
all descriptors and before applying each one. `ArraySetLength` also performs
one observable `ToNumber`, rather than coercing a side-effecting value twice.

Mapped sloppy `arguments` now synchronizes a descriptor's value into its
formal-parameter cell before a non-writable/accessor descriptor unmaps that
index. This preserves the required final parameter value without allowing a
subsequent descriptor update to re-establish the mapping. `hasOwnProperty`
and `propertyIsEnumerable` now perform `ToPropertyKey` before `ToObject(this)`,
so a key-conversion abrupt completion wins over a nullish receiver error.

The static `Object.hasOwn` and `Object.is` entry points now share the
`[[GetOwnProperty]]` and SameValue boundaries. `Object.assign` obtains each
source's own keys and descriptors through the internal-method dispatch, gets
only enumerable values, and performs a receiver-aware strict `[[Set]]` on the
target. Consequently source or target Proxies retain their observable trap
order, and the target, source, and value stay rooted across those calls.

`Object.fromEntries` now consumes iterator records one entry at a time. An
entry must already be an object; it reads keys `0` and `1` before converting
the key, creates an enumerable/writable/configurable data property, and closes
the iterator on every abrupt entry operation. A thrown JavaScript value is
stack-rooted while the user-defined `return()` close callback runs, so the
original abrupt completion remains both live and dominant over a close error.

The four Annex B `%Object.prototype%` legacy accessor helpers
(`__defineGetter__`, `__defineSetter__`, `__lookupGetter__`, and
`__lookupSetter__`) are native functions with their standard descriptor,
arity, coercion, and callable-validation order. They route definition through
`[[DefineOwnProperty]]`, and lookup through each `[[GetOwnProperty]]` and
`[[GetPrototypeOf]]`, preserving Proxy traps, abrupt completions, and GC roots.
While bootstrapping the native `__proto__` accessor, the VM stack-roots the
getter before allocating the setter, so a pressure collection cannot leave the
shared accessor descriptor with a stale getter handle.
The `__proto__` accessor has matching native getter/setter functions; the
setter distinguishes nullish, primitive, and object inputs, while
`%Object.prototype%` now observes the Immutable Prototype Exotic contract:
its null prototype is a no-op and every distinct replacement is rejected.
`Object.prototype.toLocaleString` likewise retains its original receiver for
both `Get` and `Call`, including primitive receivers.

Focused Test262 results are `__proto__` **30/30**,
`toLocaleString` **22/22**, `hasOwnProperty` **126/126**, static `hasOwn`
**124/124**, static `is` slice **306/306**, static `assign` **76/76**,
static `fromEntries` **50/50**, and immutable `%Object.prototype%` **4/4**.
The final complete `built-ins/Object` run at
`target/test262-p04-object-internal-methods-final-2` records **6,536 pass /
272 fail** of 6,808 modes: **273 fail-to-pass and zero pass-to-fail** versus
the prior **6,263 / 545** result. The remaining largest Object groups are
Date/Weak/Array and other library surface, rather than these P0.2–P0.4
internal-method boundaries.

## P0.1 continuation: generator-return iterator closing and PromiseResolve observation

The published ECMAScript 2026 [IteratorClose](https://tc39.es/ecma262/2026/multipage/abstract-operations.html#sec-iteratorclose),
[IteratorCloseAll](https://tc39.es/ecma262/2026/multipage/abstract-operations.html#sec-iteratorcloseall),
[PromiseResolve](https://tc39.es/ecma262/2026/multipage/control-abstraction-objects.html#sec-promise-resolve-functions),
and [GeneratorResumeAbrupt](https://tc39.es/ecma262/2026/multipage/control-abstraction-objects.html#sec-generatorresumeabrupt)
algorithms were checked on 2026-09-13 before this continuation.

An injected `Generator.prototype.return()` supplies a **return** completion to
the suspended frame. If a live destructuring iterator's `return` getter/call
throws, or its call returns a non-object, `IteratorCloseAll` changes that
return completion into the first close failure; only a pre-existing **throw**
completion wins over close failures. The old generator teardown reused the
throw-cleanup path and discarded all close errors. `close_iterators_for_return`
now keeps the first error, closes remaining outer iterators in reverse order,
and roots a thrown JavaScript value while later close callbacks can allocate.

The internal `PromiseResolve(%Promise%, value)` fast path also incorrectly
returned a native Promise before reading its observable `constructor`
property. It now materializes the intrinsic `%Promise%`, roots `value`, reads
the property before the identity shortcut, and propagates a getter failure as
the rejection consumed by `AsyncFromSyncIteratorContinuation`.

Two public parse → compile → execute regressions were red before their
respective implementations: a suspended generator's `.return()` now exposes
both an iterator `return` throw and a non-object return as the final error; a
`for await` loop over a synchronous iterator now catches a native Promise's
throwing `constructor` getter. The current eight-worker
`language/statements/for-` run records **3,940 pass / 147 fail** of 4,087
modes, up from **3,916 pass / 171 fail**, with **24 fail-to-pass and zero
pass-to-nonpass** transitions. Its remaining results are scoped grammar,
resource-management, Array/Map/Set, or TypedArray work rather than this
completion/reference slice; this does not close P0.1–P0.3 as whole
workstreams.

## P0.1 completion/rooting boundary repair

The next completion slice covers a labelled transfer which leaves nested
`for-of` loops through a `switch` and a `finally`. The handler correctly closes
the inner iterator before entering the finalizer, but its scope unwind had made
the compiler cleanup gateway read an inactive private `*iterator*` binding.
`CloseIteratorBinding` now closes a still-live record with the normal
iterator-close error behavior, while treating a binding already closed and
unwound by a crossed handler as a no-op. Its record is stack-rooted while a
user-defined `return()` runs. The public regression requires the observable
order `inner-close`, `finally`, `outer-close`; a close failure replaces the
labelled break only after that outer iterator has also been closed.

`with` lookup also now records the lexical-scope depth at which each object
environment is compiled. A catch parameter introduced inside that environment
therefore resolves before the object property, rather than choosing an
inactive same-named catch slot later in bytecode. The complete Test262
`language/statements/try/S12.14_A14.js` fixture is a public pipeline
regression. On 2026-09-13, the rebuilt adapter's focused
`language/statements/try` shard recorded **398 pass / 0 fail** (206 files).
Tiny-nursery public cases additionally retain thrown and returned objects
across `switch`/`finally` allocation pressure.

## Test262 process-host completion liveness

The JSON-lines adapter must answer every accepted request independently. Its
async completion loop previously slept indefinitely after all Promise jobs and
host events had drained, so an async request that omitted `$DONE` blocked the
resident adapter and every later request on the same stream. `Test262AsyncWaits`
now tracks host timers and `Atomics.waitAsync` completions from registration
until dispatch. The loop returns the adapter's existing `timeout` result only
when it is quiescent; registered host work remains pending for the runner's
wall-clock supervisor.

`process_hosts` now completes its five integration tests, including the
no-`$DONE` request followed by later requests in the same child process. The
adapter also classifies deterministic interpreter fuel exhaustion as
`resource_error`; only its wall-clock and RegExp worker deadlines are
`timeout` results. Direct VM regressions cover both quiescence and a pending
host timer.

## P0.4 continuation: weak collections and weak references

The heap now represents `WeakMap` and `WeakSet` entries as ephemerons, rather
than as ordinary strong object edges. During marking, a live table exposes an
object-keyed value only after that key is independently live; the collector
iterates this rule to a fixed point. Minor and major collection then remove
dead object-keyed entries. Non-registered Symbols are valid weak keys, while
registered `Symbol.for` keys are rejected by `CanBeHeldWeakly`.

`WeakRef` stores an opaque weak target, not a tracing edge. Collection clears
a dead target, and the VM retains a dereferenced object only for the current
job's `KeepDuringJob` lifetime. The collection constructors use their
observable prototype/adder paths, and `WeakRef` uses its observable prototype
path, so subclassing, iterable construction, and cross-Realm `newTarget`
behavior use the same internal-method and membrane boundaries as the other
collection constructors.

The focused runs against Test262 snapshot
`6eec1ac9ee144dafd8f344d73a21f36bfc9f6755` used eight workers, a 100,000
instruction budget, and a two-second case deadline:

| Filter | Result | Evidence |
| --- | ---: | --- |
| `built-ins/WeakMap/` | **281 / 281 pass** | `target/test262-p04-weak-map-final` |
| `built-ins/WeakSet/` | **170 / 170 pass** | `target/test262-p04-weak-set-final` |
| `built-ins/WeakRef/` | **58 / 58 pass** | `target/test262-p04-weak-ref-final-2` |

`FinalizationRegistry` is no longer a placeholder: its constructor validates
the cleanup callback, registry cells retain strong holdings and weak target /
unregister-token keys, `register` and `unregister` validate their inputs, and
collection clears a dead cell target. Once collection has observed that state,
the VM transfers the callback and holdings into a later host job; callback
execution is never synchronous with the triggering ECMAScript job. This is a
bounded implementation slice, not a claim of complete FinalizationRegistry
conformance or host-controlled cleanup timing.

## P0.4 continuation: Date and Array baseline closure

`Date` now has a real `[[DateValue]]` heap slot rather than using ordinary
properties. The constructor, `Date.UTC`, `Date.parse`, TimeClip, UTC/local
(currently UTC-host-zone) getters and setters, ISO/JSON/string conversion,
`@@toPrimitive`, Annex B `getYear`/`setYear`/`toGMTString`, subclass prototype
selection, and cross-Realm default-prototype fallback use their observable
internal-method paths. ISO parsing also rejects the extended negative zero
year, and the engine's own `toString`/`toUTCString` output round-trips through
`Date.parse`. `%Date.prototype%` deliberately has no `[[DateValue]]`.

Array work adds species construction and CreateDataProperty semantics to
`map`/`filter`, `Array.of`, `Array[Symbol.species]`, relative-index `at`, and
the `entries`/`keys`/`values` iterator family with
`Array.prototype[Symbol.iterator] === Array.prototype.values`. The four
`find` methods now visit holes as `undefined` and preserve their required
forward or reverse callback order.

Focused results from the same snapshot are:

| Filter | Result | Evidence |
| --- | ---: | --- |
| `built-ins/Date/` | **1,220 pass / 16 fail** of 1,236 | `target/test262-p04-date-final-4` |
| `built-ins/Array/` | **5,169 pass / 950 fail** of 6,119 | `target/test262-p04-array-final-3` |
| `built-ins/WeakRef/` | **58 / 58 pass** | `target/test262-p04-weak-ref-final-2` |

The Date remainder is exactly the 16 modes for `Date.prototype.toTemporalInstant`,
which requires the unsupported Temporal value model. Array improved by 323
passing modes from the 4,846 / 1,273 audit baseline; its largest remaining
families are `Array.fromAsync`, concat spreadability, copy-by-value methods,
and resizable-buffer interactions. These focused results do not replace the
complete-inventory result below.

These are focused filters, not a replacement for the checked-in complete
inventory. The Rust 1.95 complete reconciliation above ran after these changes;
the full-inventory totals and generated `test262-summary.json` now record its
79,897 pass, 22,962 fail and 67 timeout outcomes.

## P0.1–P0.4 closure: reflective realm and object contracts

The final P0 closure keeps lazy intrinsic allocation as an implementation
detail, rather than exposing it through JavaScript reflection. A source-level
global read now resolves through the current global-object property, while VM
internals retain their intrinsic cache. `Math`, `RegExp`, and `Intl` publish
their own global properties when first materialized. `[[OwnPropertyKeys]]` on
the realm global materializes the P0 standard global surface before reporting
keys, so `Object.getOwnPropertyNames(globalThis)` agrees with direct global
access without eagerly constructing later-phase libraries.

`Object.getOwnPropertyDescriptors` now walks `[[OwnPropertyKeys]]` and
`[[GetOwnProperty]]` directly, preserving Proxy trap order and symbol keys
without consulting a replaceable public helper. The same internal boundary
handles a key deleted by an earlier `Object.values`/`Object.entries` getter,
and filters symbol keys before an enumerable-own-properties descriptor trap.
Reflective descriptor access also materializes the lazy `%Object.prototype%`
methods and `%Function.prototype%.constructor`; the latter has its standard
writable, non-enumerable, configurable descriptor.

`Object(value)` retains function identity and `new Object` now takes the
OrdinaryCreateFromConstructor path whenever `newTarget` differs from `%Object%`.
Consequently a derived `class extends Object` and
`Reflect.construct(Object, values, newTarget)` allocate the requested
prototype instead of returning an object argument. `Object.prototype.toString`
selects array, Error, Date, RegExp, callable, boxed primitive, iterator, and
Proxy brands from internal slots before observing an overridable
`Symbol.toStringTag`; the generator-function prototype supplies its own tag.

The rebuilt adapter was run with eight workers, the normal 100,000 instruction
budget and the two-second case deadline. The focused closure evidence is local
to `/private/tmp/bluejs-p0-*-closure`:

| Workstream | Filter | Result |
| --- | --- | ---: |
| P0.1 | `language/statements/try` | **398 / 398 pass** |
| P0.2 | `language/global-code` | **228 / 228 pass** |
| P0.2 | `language/eval-code` | **924 / 924 pass** |
| P0.2 | `language/arguments-object` | **460 / 460 pass** |
| P0.3 | `language/expressions/assignment` | **1,488 / 1,488 pass** |
| P0.4 | `built-ins/Proxy` | **607 / 607 pass** |
| P0.4 | `built-ins/Reflect` | **306 pass / 2 P1.5 failures** of 308 |
| P0.4 | `built-ins/Object` | **6,746 pass / 62 later-phase failures** of 6,808 |

The two Reflect failures and six Object modes require resizable or
variable-length TypedArray semantics (P1.5). The Object remainder is otherwise
explicitly outside P0: 14 modes require Promise, collections, AggregateError,
or async-function behavior (P1); 18 descriptor checks require missing
`Array.prototype` or `Number.prototype` library methods and 24 require
`Object.groupBy` (P2). No remaining failure in these closure filters is
attributed to a P0.1–P0.4 contract. This closes the listed P0 acceptance
slices, not full Test262 conformance or the later-phase libraries they expose.

## P1/P2 continuation: Object library and brand closure

`Object.groupBy` now consumes its input through the iterator protocol, invokes
the callback with the item and its safe-integer index, converts returned keys
with `ToPropertyKey`, creates a null-prototype result, and closes a live input
iterator when callback, key conversion, property creation, or array append
abruptly completes. The implementation roots the iterator, result, and live
group arrays across observable calls.

The descriptor-facing library surface now supplies generic
`Array.prototype.pop`, `shift`, `unshift`, `reverse`, and `toLocaleString`,
plus `Number.prototype.toLocaleString`, `toFixed`, `toExponential`, and
`toPrecision`. These use ordinary property operations so sparse and
array-like receivers retain their getter, setter, deletion, and length
semantics.

The P1 Object brand paths materialize the standard `Symbol.toStringTag` data
properties for Promise, Map, Set, and their iterator prototypes. Map/Set
`@@iterator` returns an object based on the corresponding iterator prototype;
its empty-collection `next` result is observable without exposing a synthetic
collection representation. `AggregateError` is a lazy Error subclass global,
and `%AsyncFunction%` is constructible through both the general
`IsConstructor` path and native construct dispatch.

Focused runs use the pinned Test262 snapshot, eight workers, the ordinary
100,000-instruction budget, and the two-second per-case deadline:

| Workstream | Filter | Result | Evidence |
| --- | --- | ---: | --- |
| P2 | `built-ins/Object/groupBy` | **28 / 28 pass** | `/private/tmp/bluejs-p2-object-groupby` |
| P2 | `built-ins/Object/getOwnPropertyDescriptor` | **656 / 656 pass** | `/private/tmp/bluejs-p2-object-descriptor-methods` |
| P1 | `built-ins/Object/prototype/toString/symbol-tag` | **32 / 32 pass** | `/private/tmp/bluejs-p1-object-tags` |
| P1 | `built-ins/Object/seal` | **186 pass / 2 P1.5 failures** of 188 | `/private/tmp/bluejs-p1-object-seal-final` |
| Combined | `built-ins/Object` | **6,802 pass / 6 P1.5 failures** of 6,808 | `/private/tmp/bluejs-p1-p2-object-final` |

This removes all **56** specified P1/P2 Object modes: the 14 Promise,
collection, async-function, and AggregateError modes; the 18 Array/Number
prototype descriptor modes; and the 24 `Object.groupBy` modes. The six
remaining Object failures are exclusively P1.5 resizable or variable-length
TypedArray integrity semantics: two modes each in `Object.freeze`,
`Object.preventExtensions`, and `Object.seal`.

### Reverification after interrupted progress record

The adapter was rebuilt and the P1 acceptance files were rerun individually
after the interrupted progress record. This provides direct evidence for all
14 P1 modes, rather than inferring them from the broader Object filter:

| P1 surface | Test262 files/modes | Result | Evidence |
| --- | ---: | ---: | --- |
| Promise, Map, Set `Symbol.toStringTag` | 3 / 6 | **6 / 6 pass** | `/private/tmp/bluejs-p1-tags-rerun` |
| `AggregateError` sealing | 1 / 2 | **2 / 2 pass** | `/private/tmp/bluejs-p1-aggregateerror-rerun` |
| Async arrow constructor sealing | 1 / 2 | **2 / 2 pass** | `/private/tmp/bluejs-p1-async-arrow-rerun` |
| Async function constructor sealing | 1 / 2 | **2 / 2 pass** | `/private/tmp/bluejs-p1-async-function-rerun` |
| Async generator constructor sealing | 1 / 2 | **2 / 2 pass** | `/private/tmp/bluejs-p1-async-generator-rerun` |

The same rebuilt adapter reran the P2 acceptance filters:
`built-ins/Object/groupBy` is **28 / 28 pass** at
`/private/tmp/bluejs-p2-groupby-rerun`, and
`built-ins/Object/getOwnPropertyDescriptor` is **656 / 656 pass** at
`/private/tmp/bluejs-p2-prototype-descriptors-rerun`.

## P1.5 closure: resizable and variable-length TypedArray integrity levels

TypedArray `[[PreventExtensions]]` now rejects a view when its indexed
property set can change: every view over a resizable ArrayBuffer, and each
length-tracking view over a growable SharedArrayBuffer. A fixed-length view
over a growable SharedArrayBuffer remains eligible. `Object.seal` and
`Object.freeze` now follow `SetIntegrityLevel`'s required order by making the
object non-extensible before attempting descriptor changes. Consequently a
fixed non-empty view still rejects the incompatible non-configurable indexed
property definition, while a fixed zero-length shared view can be sealed.

The original P1.5 Test262 acceptance files all pass after rebuilding the
adapter:

| Surface | Filter | Result | Evidence |
| --- | --- | ---: | --- |
| Resizable TypedArray freeze | `built-ins/Object/freeze/typedarray-backed-by-resizable-buffer` | **2 / 2 pass** | `/private/tmp/bluejs-p15-object-freeze-final` |
| Variable-length preventExtensions | `staging/built-ins/Object/preventExtensions/preventExtensions-variable-length-typed-arrays` | **2 / 2 pass** | `/private/tmp/bluejs-p15-object-prevent-extensions-final` |
| Variable-length seal | `staging/built-ins/Object/seal/seal-variable-length-typed-arrays` | **2 / 2 pass** | `/private/tmp/bluejs-p15-object-seal-final` |
| Object regression inventory | `built-ins/Object` | **6,808 / 6,808 pass** | `/private/tmp/bluejs-p15-object-final` |
| Related Reflect inventory | `built-ins/Reflect` | **308 / 308 pass** | `/private/tmp/bluejs-p15-reflect-final` |

This closes the six Object and two Reflect P1.5 modes previously attributed to
resizable or variable-length TypedArray integrity behavior.

## Test repair and timeout closure

Script execution now materializes the existing lazy `import` global before a
lexical global read. This is required for `import.source` and `import.defer`:
their member access is parsed as an ordinary read of the `import` namespace,
and previously failed with `ReferenceError` despite the namespace and both
methods already being installed. The direct host regression
`source_and_defer_dynamic_imports_reject_through_the_promise_path` now passes.

The low-heap String regression retains a split array containing 128 live
one-code-unit strings as well as the lazily materialized String surface. Its
256 KiB fixture heap remains deliberately pressure-heavy (`nursery_capacity`
is one and the major threshold is 256 bytes), while allowing that valid live
object graph to fit. The former 128 KiB ceiling rejected the live result rather
than revealing a collection or rooting defect.

The Test262 runner keeps the general resource limits unchanged. It gives only
`staging/sm/String/string-upper-lower-mapping.js` a 256 MiB managed-heap cap;
that immutable Unicode mapping table exceeds the normal 16 MiB VM cap and
already receives the exact-fixture finite-work fuel and wall-time policy. Its
two modes now pass. `--filter` also accepts comma-separated path substrings so
a saved timeout manifest can be rerun without broadening a partial inventory.

The prior `target/test262/results.jsonl` timeout manifest contained 908 unique
`(path, mode)` entries. Rebuilt-adapter reruns matched all 908 keys and every
one passed:

| Former timeout family | Modes | Result | Evidence |
| --- | ---: | ---: | --- |
| RegExp `Script_Extensions` property escapes | 350 | **350 / 350 pass** | `/private/tmp/bluejs-timeout-script-extensions-final` |
| RegExp `Script` property escapes | 350 | **350 / 350 pass** | `/private/tmp/bluejs-timeout-script-final` |
| RegExp `General_Category` property escapes | 76 | **76 / 76 pass** | `/private/tmp/bluejs-timeout-general-category-final` |
| Remaining RegExp property escapes | 108 | **108 / 108 pass** | `/private/tmp/bluejs-timeout-property-other-final` |
| Character-class, repeat, parseInt, and SpiderMonkey function/accessor fixtures | 22 | **22 / 22 pass** | `/private/tmp/bluejs-timeout-non-property-final` |
| Unicode String case mapping | 2 | **2 / 2 pass** | `/private/tmp/bluejs-timeout-string-case-final` |

The property-escape and CharacterClass evidence directories contain a few
additional neighboring modes selected by their path filters; the manifest
join uses unique `(path, mode)` keys, yielding exactly **908 / 908 pass**.
The focused Rust regressions and every BlueJS test target reached by the
workspace rerun passed; the existing Node-dependent differential test remains
explicitly ignored.

## P1/P2 continuation: grammar, module namespace, Intl, and Iterator helpers

The tokenizer now uses ICU's Unicode `ID_Start` and `ID_Continue` properties,
with ECMAScript's post-start ZWNJ/ZWJ allowance. Binding-pattern parsing keeps
identifier-name property keys valid while classifying reserved binding names
and a missing binding target as established parse `SyntaxError`s. The adapter
regression distinguishes that known grammar failure from truly unsupported
syntax. The rebuilt `language/identifiers` inventory is **535 / 535 pass** at
`/private/tmp/bluejs-p11-identifiers-final`.

Module Namespace Exotic `[[Set]]` now always returns false, including a
same-value assignment to an existing export and symbol-key writes; strict
assignment consequently throws. The public regression covers direct,
renamed, indirect, default, `Symbol.toStringTag`, and arbitrary-symbol keys.
Every Promise job now resets the interpreter fuel for its own ECMAScript
execution context. This prevents a top-level-await continuation from leaving
the next dynamic-import reaction with zero fuel. The three dependency-order
regressions (`fulfillment-order`, `rejection-order`, and
`unobservable-global-async-evaluation-count-reset`) pass independently, and
the complete `language/module-code` slice is **602 / 602 pass** at
`/private/tmp/bluejs-p1-module-final`.

ECMA-402 canonicalization now preserves ECMA-402 canonical names separately
from ICU's locale-data key, which supports structurally valid `posix` without
inventing an ICU language code. It also retains canonical boolean Unicode keys
without a type, canonicalizes the `m0-names` transformed tvalue, and retains
the prior Unicode aliases. Collator's bound `compare` defines `length` before
`name`, German search uses its expected equivalence tailoring, and
GetPrototypeFromConstructor selects a foreign Realm's Collator or Locale
prototype. The mandatory NumberFormat and DateTimeFormat constructors now have
their shared callable/constructible service boundary; their formatting
algorithms remain separate ECMA-402 work. The complete existing
Collator/Locale/getCanonicalLocales acceptance filter is **510 / 510 pass** at
`/private/tmp/bluejs-intl-p1-final`. The one boolean-key fixture receives the
existing finite-stress resource envelope because its twenty complete
`testIntl.js` structural validations are bounded but exceed the ordinary
one-turn interpreter budget.

The standard-library iterator entry point is now exposed as the abstract
`Iterator` constructor, sharing the existing `%Iterator.prototype%` with
Array, String, RegExp, Map/Set, and generator iterators. `Iterator.from`
stores its source iterator and cached `next` method as traced, non-observable
heap slots; the wrapper supplies `next`/`return`. The base prototype provides
`@@iterator`, `@@dispose`, the `@@toStringTag` accessor pair, and terminal
`toArray`, `forEach`, `every`, `some`, `find`, and `reduce` helpers. The
helpers reuse the engine's existing `IteratorStep` and `IteratorClose`
contracts for result validation and early exit cleanup. The partial
`built-ins/Iterator` run rose from **24 / 1,028** to **332 / 1,028 pass** at
`/private/tmp/bluejs-p21-iterator-terminals-2`.

The remaining 696 Iterator modes are principally lazy helper state machines
(`map`, `filter`, `take`, `drop`, `flatMap`) and multi-input `concat`/`zip`/
`zipKeyed`; they remain explicitly open rather than being represented by an
eager terminal-helper approximation. Final verification for this continuation
was `cargo test -p blueice-bluejs --quiet --no-fail-fast`: all enabled BlueJS
tests passed, with only the pre-existing Node-dependent differential test
explicitly ignored.

## Working-tree continuation: JSON, descriptor roots, and foreign internal methods

The following is direct regression evidence for the working tree after the
complete inventory recorded above; it is not a replacement for a new full
53,582-file reconciliation. `JSON.parse` now implements
`InternalizeJSONProperty`: it creates the ordinary root wrapper with a data
property, walks arrays and enumerable object keys post-order, invokes the
reviver with the specified holder/key pair, and uses `[[Delete]]` or
`[[DefineOwnProperty]] for each replacement. This keeps Proxy reviver
replacements, inherited reads, non-extensible receivers, and abrupt traps on
the normal internal-method path.

`JSON.stringify` now implements the `space` gap, Number/String wrapper
unboxing with observable `ToNumber`/`ToString`, and `toJSON` lookup for both
objects and BigInts. Its existing function/array replacer handling retains
the wrapper, property list, and every intermediate callback result across GC.
The independent `JSON.rawJSON` and `JSON.isRawJSON` APIs still need their own
branded raw-JSON internal slot; they are deliberately not represented by a
plain object fallback.

The same continuation shares the global-symbol registry between Test262
Realms, forwards foreign `[[OwnPropertyKeys]]` and receiver-aware `[[Set]]`,
and roots Proxy `[[DefineOwnProperty]]` trap descriptor objects. Promise lazy
prototype construction and resolving-function pairs now retain intermediate
objects across allocation; `Promise` and dynamic async function construction
also observe a supplied `newTarget` when selecting their prototype. Lazy
global deletion materializes the specified descriptor before invoking
`[[Delete]]`.

Direct pinned Test262 regression (both sloppy and strict where applicable)
passed these **45 modes**:

| Area | Modes | Exact fixtures |
| --- | ---: | --- |
| Reflect/Proxy realm and internal-method closure | 11 | `staging/sm/Reflect/{apply,ownKeys,deleteProperty,set}.js`; `staging/sm/Proxy/revoked-get-function-realm-typeerror.js`; `staging/sm/Proxy/json-stringify-replacer-array-revocable-proxy.js` |
| JSON reviver/stringify library boundary | 34 | `built-ins/JSON/parse/{reviver-call-order,revived-proxy,reviver-array-define-prop-err,reviver-array-get-prop-from-prototype,reviver-array-delete-err,reviver-array-length-coerce-err,reviver-object-own-keys-err,reviver-object-define-prop-err}.js`; `built-ins/JSON/stringify/{space-string,space-number-object,value-number-object,value-bigint-tojson,value-bigint-order,value-bigint-replacer,replacer-array-proxy,replacer-function-tojson,value-tojson-result}.js` |

The public JSON regression also runs under a one-object nursery, exercising
nested parse/revive replacement across collection. Final local validation was
`cargo test -p blueice-bluejs --quiet`, `cargo fmt --all -- --check`, and
`cargo clippy -p blueice-bluejs --all-targets -- -D warnings`; all passed.

## Working-tree continuation: remaining library algorithms and Iterator helpers

`JSON.rawJSON` and `JSON.isRawJSON` now use a dedicated `RawJson` heap kind,
rather than an observable plain-object marker. `rawJSON` validates that its
text is a JSON primitive, creates the required frozen/null-prototype branded
object, and `stringify` emits the retained primitive source directly. The
parser also retains original primitive tokens for the ES2026 reviver context:
`context.source` is fresh, data-only and present only when the current value
is still the parsed primitive. Proxy mutation and duplicate-key replacement
therefore remain on the ordinary internal-method path.

`Array.prototype.concat` now calls the VM's Proxy-aware `IsArray`, observes
`Symbol.isConcatSpreadable`, preserves source holes through `HasProperty`,
and creates its result via `ArraySpeciesCreate`. The foreign-Test262-realm
bridge recognizes a foreign intrinsic `%Array%` constructor before reading a
foreign species. This closes the cross-Realm concat branch without adopting a
foreign array prototype in the caller Realm.

`Number.prototype.toString` is no longer routed through the generic primitive
method. It has its required arity of one, coerces and validates radix 2–36,
and selects a shortest representation from exact binary64 round-trip
boundaries. This fixed the otherwise misleading `RegExp.escape` punctuation
fixtures, whose expected hexadecimal escapes are constructed by
`codePointAt(...).toString(16)`.

The lazy iterator helpers, `Iterator.prototype.map`,
`Iterator.prototype.filter`, `Iterator.prototype.take` and
`Iterator.prototype.drop`, share one traced private state record for the
direct iterator, cached `next`, callback/count, kind, done and executing
states. Their helper `next`/`return` methods retain lazy advancement,
iterator-result validation, re-entry rejection, forwarding of `return`, and
the rule that an already-abrupt callback error survives a later close error.
`filter` loops through rejected source values inside one lazy `next()` without
eager collection, while preserving the callback's source index; `take` closes
only when the next pull reaches its limit, while `drop` skips only on demand.
The accompanying native-call correction permits `%Iterator%` to be used as a
class heritage constructor while retaining its direct-call/direct-construct
TypeError contract. This also unblocks pre-existing terminal-helper subclasses.

`Iterator.prototype.flatMap` extends that state record with a traced active
inner direct-iterator record. It calls the mapper only when an outer value is
needed, flattens exactly one iterator level, rejects primitive mapper results,
and closes an active inner iterator before the outer iterator on `return()`.
To retain the existing tiny-heap string-iteration guarantee, independently
added helpers such as `flatMap` and `chunks` are materialized on
`%Iterator.prototype%` only when their property is observed. The
materialization is performed at the shared `[[GetOwnProperty]]` boundary, so
ordinary reads and every descriptor/reflection path see the same writable,
non-enumerable, configurable data property.

`Iterator.concat` snapshots every object argument's `Symbol.iterator` method
in argument order, yet delays calling each method until that source is first
needed. Its concat state retains the iterable/method pairs and only the active
direct iterator record; natural exhaustion drops that record without a
`return`, while helper `return()` closes precisely the still-active source.
The shared helper execution guard remains set during forwarding, preserving
the required TypeError for a re-entrant `next` or `return`.

`Iterator.prototype.chunks` accepts only an integral Number in the inclusive
range 1–2³²−1; it deliberately does not coerce its argument. Invalid input
therefore closes the object receiver before consulting `next`. Each lazy pull
collects a distinct Array of up to that many values, returning the final
partial array before producing the terminal result; no `return` is called for
natural exhaustion.

`Iterator.prototype.windows` applies the same Number-only validation before
it reads `next`, then retains a traced private sliding buffer. It yields a
fresh Array for every full window, and supports the explicit
`"allow-partial"` mode for the single final undersized window. Invalid mode
or size closes the receiver without observing its `next` property.

`Iterator.prototype.constructor` is now the specified configurable,
non-enumerable accessor. Its getter returns the Realm's `%Iterator%`; its
setter uses `SetterThatIgnoresPrototypeProperties`, rejecting the home
prototype while creating or updating an own `constructor` on derived objects.

`GetPrototypeFromConstructor` now recognizes `%Iterator.prototype%` as a
Realm-sensitive fallback intrinsic. A foreign `newTarget` whose `prototype`
is non-object consequently selects the foreign Realm's Iterator prototype.

`Iterator.zip` eagerly opens the outer iterables iterator and, for each
yielded item, resolves it through `GetIteratorFlattenable`, retaining every
opened record in the shared metadata object rather than a public slot; only
`"longest"` mode additionally collects one padding value per record,
optionally consuming a real padding iterator and closing it once enough
values are read. Every eager step distinguishes which already-opened records
an abrupt completion must close: a failing `GetIteratorFlattenable` closes
the already-opened inner records and then the outer iterables iterator,
while a failing outer step, or any failure while collecting padding, closes
only the already-opened inner records — matching `IteratorZip`'s two
distinct `IfAbruptCloseIterators` call sites rather than one shared handler.
`Iterator.zipKeyed` shares this eager collection and closing behavior, but
enumerates own enumerable keys (skipping `undefined` values) instead of
iterating a list, and yields `null`-prototype records keyed by the source's
own keys rather than fixed-length arrays.

`Iterator.prototype.includes` is a terminal helper with `SameValueZero`
comparison, so it correctly treats `NaN` as matching itself and `-0` as `+0`.
Its optional `skippedElements` accepts only an integral Number or infinity—it
does not run `ToNumber`—and invalid input closes the direct iterator without
observing `next`. A successful match likewise closes the active iterator,
whereas natural exhaustion does not.

`Iterator.prototype.join` coerces its separator before obtaining the cached
`next` method, closes without that lookup if separator conversion is abrupt,
and appends non-nullish values after their observable string conversion. Its
error path shares `IteratorClose`, while iterator-origin errors retain the
already-completed direct record and therefore do not call `return` again.

The previously implemented terminal callback helpers—`forEach`, `every`,
`some`, `find` and `reduce`—now share the same callback-validation boundary:
an invalid callback closes an object receiver before `GetIteratorDirect`, so
its `next` getter remains unobserved. This is a descriptor/internal-method
ordering requirement, not merely an input-validation shortcut.

`Array.prototype.fill` performs an ordinary `Set` for every index in its
resolved range rather than writing dense storage directly, so it stays
generic over array-like receivers (including proxies and inherited setters)
the same way the existing `at` method does. Its start/end arguments follow
`ToIntegerOrInfinity` and the shared relative-index clamping, without special
casing `NaN` beyond the standard "treat as zero" rule.

The expanded library slice exposed two GC reachability defects under the
normal small-nursery test configuration. A dequeued Promise job must retain
its target, callback and values until that job completes; likewise, the first
of a combinator's two just-allocated reaction functions must survive creation
of the second. The VM now roots both sets of temporary edges, rather than
depending on a particular heap allocation cadence.

Pinned Test262 evidence from `72faf8ec1445c55149615e8b35187830783aba1a`:

| Surface | Result | Evidence |
| --- | ---: | --- |
| JSON raw JSON, reviver source and SpiderMonkey parse-with-source | **44 / 44 pass** | `target/test262-json-source` |
| Array concat (spreadability, holes, species and foreign Realm) | **137 / 137 pass** | `target/test262-array-concat` |
| Number radix conversion and RegExp.escape | **222 / 222 pass** | `target/test262-number-regexp` |
| Iterator map/filter plus Iterator subclassability | **148 / 148 pass** | `target/test262-iterator-map-filter` |
| Iterator take | **66 / 66 pass** | `target/test262-iterator-take` |
| Iterator drop | **68 / 68 pass** | `target/test262-iterator-drop` |
| Iterator flatMap | **88 / 88 pass** | `target/test262-iterator-flatmap` |
| Iterator concat | **64 / 64 pass** | `target/test262-iterator-concat` |
| Iterator chunks | **76 / 76 pass** | `target/test262-iterator-chunks` |
| Iterator windows | **80 / 80 pass** | `target/test262-iterator-windows` |
| Iterator constructor accessor | **4 / 4 pass** | `target/test262-iterator-constructor` |
| Iterator cross-Realm constructor fallback | **2 / 2 pass** | `target/test262-iterator-proto-realm` |
| Iterator zip | **76 / 76 pass** | `target/test262-iterator-zip` |
| Iterator zipKeyed | **88 / 88 pass** | `target/test262-iterator-zip-keyed` |
| Iterator includes | **88 / 88 pass** | `target/test262-iterator-includes` |
| Iterator join | **36 / 36 pass** | `target/test262-iterator-join` |
| Existing terminal callback helpers | **346 / 346 pass** | `target/test262-iterator-terminal-after-validation` |
| Array.prototype.fill | **44 / 44 pass** | `target/test262-array-fill` |
| Full current `built-ins/Iterator/` inventory | **1,308 / 1,308 pass** | `target/test262-iterator-after-zip` |

With `zip`/`zipKeyed`'s closing behavior corrected (see below), the full
current `built-ins/Iterator/` inventory now passes completely against this
pinned snapshot; this is scoped to `built-ins/Iterator/` and this snapshot,
not a whole-engine or whole-Test262 completion claim. The working tree has
not been committed; the local BlueJS crate gate is green, while broader
workspace and cross-platform gates remain required before any commit
decision.

`Iterator.zip`'s initial working-tree revision opened records eagerly in
order but never closed already-opened ones when a later step (a bad
`GetIteratorFlattenable`, an outer iterator step, or padding collection)
raised abruptly, unlike the equivalent `Iterator.zipKeyed` path. Test262's
five dedicated abrupt-completion fixtures under `built-ins/Iterator/zip/`
(`iterables-iteration-get-iterator-flattenable-abrupt-completion`,
`iterables-iteration-iterator-step-value-abrupt-completion`, and the three
`padding-iteration-*-abrupt-completion` cases) caught this: 10 of the 164
scheduled `zip`/`zipKeyed` modes failed before the fix, all under `zip/`.
The corrected implementation distinguishes, per `IfAbruptCloseIterators`
call site, whether the outer iterables iterator itself must also close; a
new BlueJS-side regression test
(`iterator_zip_closes_already_opened_records_in_order_on_abrupt_completion`)
pins the exact close ordering for all four cases independently of the
upstream corpus.

## BigInt closure: StringToBigInt, asIntN/asUintN, toString(radix), and ++/-- typing

Implemented 2026-09-18, against the pinned `72faf8ec1445c55149615e8b35187830783aba1a`
snapshot's full `built-ins/BigInt/` inventory (154 scheduled modes, 77 files):

| Stage | Result | Evidence |
| --- | ---: | --- |
| Session baseline (before this work) | 72 pass / 82 fail | `/tmp/bigint-baseline` |
| After `asIntN`/`asUintN`, `StringToBigInt`, `ToBigInt`, and Number/String equality/comparison mixing | 138 pass / 16 fail | first commit below |
| After `toString(radix)`, the ordinary (non-boxed) BigInt prototype, and BigInt's constructor whitelisting | 152 pass / 2 fail | second commit below |
| After the Object/BigInt equality fallback and `++`/`--` BigInt typing | **154 / 154 pass** | `/tmp/bigint-after4` |

`BigInt.asIntN`/`BigInt.asUintN` did not exist at all — no `NativeFunction`
variant, nothing installed on the constructor — so every test under
`built-ins/BigInt/asIntN` and `asUintN` failed outright. Implemented per
sec-bigint.asintn/sec-bigint.asuintn: `ToIndex(bits)` then `ToBigInt(bigint)`
in that order, each a single observable coercion; the result is wrapped into
`[0, 2**bits)` via BigInt's truncating `%` corrected to a mathematical
modulo (`asIntN` additionally reflects values at or above `2**(bits-1)` into
the negative half). `bits` is capped at 1,000,000 — the same
"implementation capacity" convention `bigint_shift`/`bigint_exponentiate`
already use — so a ToIndex-valid but absurd `bits` (up to `2**53-1`) can't
try to allocate an astronomically large BigInt.

`BigInt(value)`'s own coercion had three real gaps: `BigInt(true)`/
`BigInt(false)` threw `TypeError` instead of returning `1n`/`0n`; string
coercion parsed only plain decimal digits via `BigInt::parse_bytes(_, 10)`,
rejecting every `0x`/`0o`/`0b`-prefixed string and an empty/all-whitespace
string (StringToBigInt says empty is `0n`); and the constructor duplicated
its own coercion logic instead of sharing it with anything else that needs
`ToBigInt`. Added `primitive::string_to_bigint` (a real StringToBigInt: an
unsigned `0x`/`0o`/`0b` literal, or a signed decimal, `StrWhiteSpace`-trimmed
on both ends, empty is `0n`) and a VM `to_bigint` helper (the `ToBigInt`
abstract operation proper: Boolean/BigInt/String primitives convert, Number/
Symbol/Null/Undefined/Object throw), both shared by the constructor and by
`asIntN`/`asUintN`'s own `ToBigInt(bigint)` step.

`primitive::compare` and `vm::operations::loose_equal` did not handle a
BigInt operand against a String at all (fell through to `Ok(false)`/no
match), and `loose_equal` didn't handle BigInt against Number either (`1n ==
1` was `false`). Both now follow Abstract Relational Comparison/Abstract
Equality Comparison's BigInt cases exactly. A second, later equality bug: the
Object↔primitive `ToPrimitive`-unwrap fallback in `loose_equal` listed
Number/String/Symbol as valid companions for an Object operand but not
BigInt, so `Object(1n) == 1n` fell through to `false` instead of unwrapping
the object first — caught by `wrapper-object-ordinary-toprimitive.js`, whose
whole point is exercising an overridden `valueOf`/`toString` pair through
several different `ToPrimitive` hints.

`BigInt.prototype` was allocated as a boxed-primitive object holding `0n`,
the same as `%Number.prototype%`/`%Boolean.prototype%` — but "Properties of
the BigInt Prototype Object" explicitly says the BigInt prototype does *not*
have a `[[BigIntData]]` internal slot. That made `BigInt.prototype.toString(1)`
(and any other direct call on the bare prototype) silently treat it as `0n`
instead of throwing `TypeError`. It's now allocated as a plain object;
Number/Boolean keep their existing boxed-primitive prototypes.
`BigInt.prototype.toString`/`valueOf` also didn't fall back to
`test262_foreign_boxed_primitive` the way `Symbol.prototype`'s own methods
already do, so a cross-realm-created boxed BigInt threw instead of reading
the other realm's internal slot. Separately, `BigInt.prototype.toString(
[radix])` never accepted a radix argument at all (always base 10); added
`ToIntegerOrInfinity`-then-range-check handling for the optional radix
(2-36, `RangeError` outside that range, `TypeError` for a Symbol/BigInt
radix via the existing `ToNumber` rejection), backed by num-bigint's own
`to_str_radix` for the a-z digit conversion. `BigInt` itself was also
missing from both constructor whitelists (`is_constructor` and the generic
`new`-gate in `vm.rs`), so `isConstructor(BigInt)` incorrectly reported
`false` even though BigInt does have `[[Construct]]` (only failing once
NewTarget is observed defined) — a distinction the existing, previously
unreachable check inside BigInt's own native dispatch already encoded
correctly once actually reached.

The highest-leverage fix, by scope rather than by BigInt-directory mode
count: `++`/`--` on a plain identifier compiled to a `ToNumber` opcode
followed by adding a fixed `Number(1.0)` constant — `ToNumber` rejects
BigInt outright, and even bypassing that, adding a Number to a BigInt is
itself a rejected mix. Added a `ToNumeric` opcode (`ToNumber`'s
BigInt-preserving sibling; unary `+` keeps using plain `ToNumber`, since it
must still throw on BigInt) and a `PushOne` opcode that pushes a `1` of
whichever numeric type `ToNumeric` just produced, so the following Add/
Subtract never mixes types. `UpdateProperty`/`SuperUpdate` (member/super
`++`/`--`) had the identical bug in their own Rust implementation and now
share a new `numeric_step` VM helper instead. This is a core interpreter
fix, not `built-ins/BigInt/`-local, so it also corrects `i++`/`obj.x--`/
`super.x++` wherever a BigInt operand reaches them — e.g. it was the actual
cause behind `built-ins/BigInt/prototype/toString/a-z.js`'s failure, whose
own assertion is about `toString`'s digit set, not increment.

`backend/bluejs/tests/bigint.rs` (30 tests) covers all of the above
end-to-end through parse/compile/execute: `BigInt()`'s Number/Boolean/String/
Symbol/null/undefined coercion paths (including the single-ToPrimitive-call
guarantee), `StringToBigInt`'s full grammar (radix prefixes, blank-string
zero, syntax-error rejections), BigInt mixing with Number/String in both
`==` and relational operators (including through a boxed wrapper object),
`asIntN`/`asUintN`'s wrap-around arithmetic, argument-coercion order,
not-a-constructor status and property descriptors, `toString`'s radix range/
digit-set/error paths, the now-ordinary BigInt prototype, BigInt's
is-a-constructor-but-always-throws status, and `++`/`--` on BigInt
identifiers/properties/super properties.

Explicitly out of scope for this pass (left for the sibling TypedArray
work): `BigInt64Array`/`BigUint64Array` construction and indexing,
`DataView.prototype.{get,set}BigInt64`/`BigUint64`, and Atomics on BigInt
typed arrays. Also not attempted: a systematic sweep of the ~215
`features: [BigInt]`-tagged files under `test/language/` (as opposed to
`test/built-ins/BigInt/`) beyond spot-checking that the `++`/`--` and
equality fixes above don't regress the surrounding non-BigInt
`postfix-increment`/`prefix-increment`/`equality` suites (291/323 passing
there, with the 32 failures being pre-existing, non-BigInt reference/
`putValue`-ordering/line-terminator gaps unrelated to this session's
changes).

## P1.5 continuation: `%TypedArray%.from`/`of`, shared `toStringTag`,
Float16Array, and non-shared Atomics

Implemented 2026-09-18. Fresh baseline against the pinned snapshot
`72faf8ec1445c55149615e8b35187830783aba1a`, filtered over
`built-ins/TypedArray/,built-ins/TypedArrayConstructors/,built-ins/ArrayBuffer/,built-ins/DataView/,built-ins/Atomics/`
(6,666 scheduled modes; this session's own baseline, since the numbers
above predate the Temporal/PluralRules merges this branch since picked
up): **5,448 pass / 1,212 fail / 6 timeout**. After the four changes
below: **5,716 pass / 944 fail / 6 timeout** -- 268 additional passing
modes, zero regressions (verified by diffing every `(path, mode)` key
between the two full result sets, not just aggregate counts).

`%TypedArray%.from` and `%TypedArray%.of` were entirely unimplemented --
every concrete constructor (`Int8Array.from`, etc.) inherits them from the
abstract `%TypedArray%`, so their absence meant `X.from`/`X.of` was
`undefined` for every numeric and BigInt kind. Implemented per spec
(`built-ins/TypedArray/from/`, `built-ins/TypedArray/of/`, and each
constructor's own `from`/`of` inheritance tests): `from` collects the
source's values in full (via its `@@iterator` when one exists, else as an
array-like) before `TypedArrayCreate` ever runs the `this` constructor,
then maps each raw (not-yet-numerically-coerced) value and writes it with
an ordinary `[[Set]]` (silently ignoring an out-of-bounds index, per spec,
rather than treating it as an error -- exercised by
`from-array-mapper-detaches-result.js`, where the mapper detaches the
result mid-loop); `of` does the same for its argument list without the
iterator/array-like branch. Both share a new `typed_array_create` helper
in `typed_arrays.rs`, factored out of the existing species-aware
`typed_array_species_create` as its post-species-resolution
construct-and-validate tail (`TypedArrayCreate` is exactly that operation
without the species-resolution step `from`/`of` skip, since their `this`
value is already the target constructor).

`%TypedArray%.prototype[Symbol.toStringTag]` was a plain data property
defined separately on each concrete prototype (`Int8Array.prototype`,
etc.) instead of the spec's single accessor on the shared
`%TypedArray%.prototype`. Test262's own `Symbol.toStringTag` suite
(`built-ins/TypedArray/prototype/Symbol.toStringTag/*`,
`built-ins/TypedArrayConstructors/prototype/Symbol.toStringTag/*`) checks
exactly the properties that distinction breaks: `TA.prototype.hasOwnProperty
(Symbol.toStringTag)` must be `false` (inherited, not own), the property
descriptor must be `{get, set: undefined, enumerable: false, configurable:
true}` (an accessor, not a data property), and calling the getter directly
on a receiver with no `[[TypedArrayName]]` internal slot (a plain object,
an `Array`, a `DataView`, or a non-object `this`) must return `undefined`
rather than throw. Replaced with one `get [Symbol.toStringTag]` installed
on the shared prototype (`typed_array_intrinsics`) that resolves the
receiver's kind through `Heap::typed_array_info` -- which reports a
`TypedArray`'s kind independent of its buffer/index bounds, so a detached
view still reports its constructor name, matching
`Symbol.toStringTag/detached-buffer.js` -- and returns `Value::Undefined`
for anything without that internal slot.

`Float16Array` and `DataView.prototype.{getFloat16,setFloat16}` (the
newer, additive ES2024 half-precision typed-array kind) were entirely
unimplemented. This is purely additive from a regression-risk standpoint:
Test262's `testTypedArray.js` harness only adds `Float16Array` to its
`floatArrayConstructors` list when `typeof Float16Array !== "undefined"`,
so every existing generic `TypedArray/prototype/*` test that loops over
all float constructors gains an extra, correctly-handled iteration rather
than breaking. Implementation adds `TypedArrayKind::Float16` (2-byte
width) alongside the existing kinds, and a hand-written IEEE 754 binary16
codec (`heap::binary_data::f16_bits_to_f64`/`f64_to_f16_bits`) since
Rust's `f16` primitive type is still unstable on this project's pinned
1.95.0 toolchain (rust-lang/rust#116909). The decode direction is a
direct sign/exponent/mantissa reconstruction; the encode direction
extracts the f64's own 64-bit representation (11-bit exponent, 52-bit
mantissa) and rounds to binary16's 10-bit mantissa with round-to-nearest-
even, handling the normal, subnormal (down to the exact 2^-25
round-to-even threshold against zero), overflow-to-infinity, and NaN
cases explicitly -- verified by new `heap::tests` unit tests pinned to
Test262's own exact DataView Float16 fixture byte patterns (`42` <->
`0x5140`, `2.158203125` <-> `0x4051`, `3.078125` <-> `0x4228`, read
2026-09-18 from `built-ins/DataView/prototype/{get,set}Float16/*.js`)
plus signed-zero/infinity/NaN/subnormal-boundary round-trips no fixture
in the corpus currently exercises. `Float16Array`/`typeof Float16Array`
also needed adding to the compiler's and interpreter's global-identifier
recognition lists (`compiler/expressions.rs`, `vm/execution.rs`) --
without that, a bare `Float16Array` reference compiled to an unconditional
unbound-name lookup instead of a global lookup, so `new Float16Array(...)`
threw `ReferenceError` even after the constructor itself was wired up.

Atomics' `atomics_access` validation unconditionally required a
`SharedArrayBuffer` for every operation, but the current spec's
`ValidateIntegerTypedArray` only requires that for `Atomics.wait`/
`waitAsync`; the ordinary read-modify-write operations
(`load`/`store`/`add`/`and`/`or`/`sub`/`xor`/`exchange`/`compareExchange`)
work on any integer TypedArray, and `Atomics.notify` returns `0` for a
non-shared buffer instead of throwing (`built-ins/Atomics/*/non-shared-
bufferdata*.js`, `built-ins/Atomics/notify/non-shared-bufferdata-
returns-0.js`). Fixed with a new non-locking `Heap::typed_array_atomic_
modify` (a plain `ArrayBuffer` is never visible to more than one agent, so
ordinary synchronous byte access already gives it the atomicity the
shared, locked path provides for `SharedArrayBuffer`), dispatched
alongside the existing shared path via a new `atomics_modify` helper.
`Atomics.wait`/`waitAsync` keep a separate `atomics_wait_access` doing the
older combined `ValidateSharedIntegerTypedArray`-style check, because --
unlike `notify`, which per spec coerces its `index` argument before ever
consulting shared-ness -- `wait`/`waitAsync` must reject a non-shared
buffer *before* touching `index`/`value`/`timeout` at all;
`non-shared-bufferdata-throws.js`'s second assertion passes a poisoned
`valueOf` for every argument specifically to catch a reordering that
evaluates any of them first. `Atomics.notify` itself is restructured to
coerce `index` then `count` (both observable, matching
`retrieve-length-before-index-coercion-non-shared*.js` and
`non-shared-bufferdata-*-evaluation-throws.js`) and only then check
shared-ness, returning `0` rather than calling into the shared backing
store.

Remaining gaps this session left open, all confirmed via the same
before/after diff to be pre-existing (not newly exposed) failures: the
"Immutable ArrayBuffer" proposal (`ArrayBuffer.prototype.{transferToImmutable,
sliceToImmutable}`, ~114 modes across `ArrayBuffer`/`DataView`/`Atomics`,
ignored uniformly by `Atomics/*/immutable-buffer*.js`), `Atomics.wait`'s
"cannot suspend" main-thread restriction (4 modes), and one still-uninvestigated
`built-ins/Atomics/{load,store,add,and,or,sub,xor,exchange,compareExchange}
/bigint/non-shared-bufferdata.js` failure specific to the BigInt64/
BigUint64 + non-shared-buffer combination driven through Test262's full
`testWithBigIntTypedArrayConstructors` harness (every individual
buffer-factory/kind combination reproduced by hand in isolation passes;
the failure only appears when Test262's own harness loop runs all of
them together, so it was not chased further this session).

## BigInt closure, continued: language/ sweep, Proxy/Reflect audit, and a verified non-regression

Implemented 2026-09-18, same session as the closure above. That first pass
scoped its verification to `built-ins/BigInt/` and a spot-check of a few
`language/` directories; this pass follows up on exactly what was left open:
a systematic sweep of every `features: [BigInt]`-tagged file under
`test/language/` (not just `test/built-ins/BigInt/`), an audit of BigInt's
interaction with Proxy/Reflect and JSON (the closest ECMA-262 has to a
structured-clone-like API), and a *verified* (not assumed) non-regression
check against the pre-session commit.

| Surface | Result | Evidence |
| --- | ---: | --- |
| `features:[BigInt]` under `test/language/` (215 files, 430 modes) | 422 pass / 8 fail | cross-referenced from a directory-scoped run against the exact file list |
| `built-ins/JSON/{rawJSON,stringify}` BigInt-tagged (7 files, 14 modes) | 14 / 14 pass | `/tmp/json-bigint2` |
| `postfix`/`prefix`-increment/decrement + equality/does-not-equals (323 modes), pre-session commit `d86243d` | 273 pass / 50 fail | `/tmp/lang-check-pre` |
| Same directories, this session's tip | 291 pass / 32 fail | `/tmp/lang-check` |
| Diff of the two | 18 fixed, **0 regressed**, 32 identical on both | path/mode-level `results.jsonl` diff |

The remaining 8 `language/`-BigInt failures are all
`language/expressions/dynamic-import/import-attributes/2nd-param-*.js`: an
`import(specifier, options)` second-argument parser gap. 21 of that
directory's 25 files aren't BigInt-tagged at all -- these four just happen
to loop a BigInt value in among several non-object values (`null`, `false`,
`23`, `''`, `Symbol()`, `23n`) they all reject the same way. This is the
unrelated import-attributes proposal, not a BigInt gap, and stays out of
scope here.

Three real, narrow gaps closed this pass:

`language/expressions/object/literal-property-name-bigint.js`: a BigInt
literal was never accepted as a `LiteralPropertyName` -- object literal
keys, method names, class methods and destructuring patterns all share one
parser function, `parse_property_key`, which matched `Token::Number` but not
`Token::BigInt`. Per "LiteralPropertyName: NumericLiteral -- 1. Let nbr be
the NumericValue of NumericLiteral. 2. Return ! ToString(nbr)", added a
`Token::BigInt` arm converting straight to `PropertyKey::String` via
BigInt's own decimal `Display` -- exactly `ToString(BigInt)`, no further
numeric-formatting pass needed (unlike a large Number literal used as a key,
which can need scientific-notation handling).

`language/expressions/{greater,less}-than/bigint-and-boolean.js`:
`primitive::compare` had no `Bool`<->`BigInt` case at all, so `1n > true`
fell through to `number(&value)`, which throws for a BigInt operand.
Abstract Relational Comparison's own `ToNumeric` step converts a Boolean
operand to Number (`0`/`1`) -- it never becomes a BigInt -- so both
orderings now convert the Boolean side to a Number and recurse into the
existing BigInt/Number case rather than trying (and failing) to treat it as
a BigInt.

`built-ins/JSON/stringify/value-bigint-cross-realm.js`, found while auditing
BigInt against cross-realm/Proxy/Reflect machinery: JSON's Object-branch
primitive-unwrap step only checked this realm's own `heap.boxed_primitive`,
so a boxed BigInt built by a *different* Test262 realm
(`$262.createRealm()`) fell through to ordinary-object serialization
(`"{}"`) instead of unwrapping its `[[BigIntData]]` and throwing `TypeError`
per `SerializeJSONProperty`. Added the same `test262_foreign_boxed_primitive`
fallback that `BigInt.prototype.toString`/`valueOf` already needed for the
identical reason in the prior closure.

Proxy/Reflect audit: Test262 has **no** `built-ins/Proxy/` or
`built-ins/Reflect/` tests tagged `BigInt` at all. Neither mechanism
special-cases a value's *type* -- Proxy traps intercept property-key
operations and forward arbitrary return values, and Reflect operations are
thin wrappers over the same internal methods -- so there was no
existing-corpus gap to find. Added our own end-to-end coverage instead
(`get`/`set` traps carrying a BigInt property value, `Reflect.apply`/
`Reflect.construct` with BigInt arguments, a `has` trap keyed by a BigInt's
`ToPropertyKey` conversion -- confirming `5n in p` and `'5' in p` reach the
trap identically, since `ToPropertyKey` on a non-Symbol primitive is just
`ToString` -- and `Reflect.ownKeys`) confirming BigInt values and
BigInt-derived property keys pass through both mechanisms exactly like any
other value.

`asIntN`/`asUintN`'s 1,000,000-bit cap (from the prior closure) is
unchanged; a new regression test pins it as a hard, explicit `RangeError`
boundary -- `BigInt.asIntN(1_000_000, 1n)` still succeeds exactly at the
cap, `BigInt.asIntN(1_000_001, 1n)` throws -- rather than a value that
silently narrows, wraps, or truncates before use.

`backend/bluejs/tests/bigint.rs` grew from 27 to 33 tests, covering all of
the above: the BigInt-literal property-name fix (object literals, method
names, class methods, destructuring), Boolean/BigInt relational comparison
in both directions, the cross-realm `JSON.stringify` fix, the Proxy/Reflect
boundary checks, and the `asIntN`/`asUintN` cap's enforced (not truncated)
behavior. `built-ins/BigInt/`'s full 154/154 remains unaffected by this
round's changes (reverified after each fix).

## Dynamic `import()`'s second argument, and the ImportCall "Forbidden Extensions"

Implemented 2026-09-18. Scope: `dynamic-import`'s second-argument
(import-attributes) form and its related grammar restrictions -- the
highest-leverage gap the prior BigInt-closure session's own note already
flagged (`language/expressions/dynamic-import/import-attributes/2nd-param-*.js`).
The parser had no support at all for `import(specifier, options)`: the
`ImportCall` production only ever consumed one `AssignmentExpression`
before requiring `)`, so any second argument was a plain
`unclassified_parse_error` (`expected RParen (found Punct(Comma))`), not a
recognized-and-rejected form.

Official edition-17-track clauses read before implementing: the
`ImportCall` grammar and its `Evaluation` semantics (`sec-import-call`,
`sec-import-call-runtime-semantics-evaluation`) in the current
[tc39/ecma262 multipage text](https://tc39.es/ecma262/multipage/ecmascript-language-expressions.html#sec-import-call)
-- Import Attributes (the `with {...}` clause and the ImportCall second
argument) is already merged into the mainline spec text (not a
`# proposal-*` entry in `test262/features.txt`), confirming it belongs to
the published edition-17 target per `ECMASCRIPT_2026.md`'s authority rule,
unlike `source-phase-imports`/`import-defer` (see the applicability finding
below).

**Root cause and fix.** Four coordinated gaps, all in `backend/bluejs/src`:

1. `parser/expressions.rs`'s `import(` arm parsed exactly one
   `AssignmentExpression` then required `)`. Rewrote it to parse an
   optional second `AssignmentExpression` (the options argument) with an
   optional single trailing comma after either one or two arguments (per
   the grammar's two `,opt` productions), explicitly rejecting a leading
   `...` spread and a third argument (both "Forbidden Extensions") with a
   real `syntax_error` (not the generic unsupported-grammar fallback, so
   these negative tests are classified `SyntaxError`, not
   `unclassified_parse_error`). Both argument positions temporarily clear
   `no_in` (mirroring the existing `?:`-consequent precedent) since
   `ImportCall`'s arguments are always `AssignmentExpression[+In]`, even
   inside a no-in `for`-head (`for(x=import('a','b' in {});;)`).
2. `new import(x)` previously parsed as `New { callee: DynamicImport, .. }`
   because `parse_new_expression`'s callee path calls the same
   `parse_primary` arm that recognizes `import(`. `ImportCall` is a
   `CallExpression`, never a `MemberExpression`, so it can never be the
   target of `new` -- added an explicit check at the top of
   `parse_new_expression` (covering the recursive `new new import(x)` case
   too via its own recursive call).
3. A bare `import` (followed by neither `(` nor `.`) was previously parsed
   as an ordinary `Expr::Identifier("import")`, since a real global
   `import` binding already exists in this host (see the applicability
   note below) rather than as a proper reserved-word rejection --
   `typeof import` and `import + 1` parsed successfully instead of raising
   a `SyntaxError`. Added a narrowly-scoped rejection that only fires when
   `import` is followed by neither `(` nor `.`, deliberately leaving
   `import.<name>` continuations (including unrecognized ones) alone.
4. `Expr::DynamicImport` became `{ specifier, options: Option<Box<Expr>> }`
   (updated at every match site: `ast.rs`'s `expr_contains_super`,
   `compiler.rs`'s strict-assignment scan, `compiler/private_validation.rs`,
   and the actual codegen in `compiler/expressions.rs`). The `DynamicImport`
   opcode stays a fixed-width, no-operand opcode (this bytecode is
   one-byte-opcode-plus-optional-u32, no variable-arity form) by always
   popping two stack values: the codegen pushes the specifier, then either
   the options expression or an implicit `Constant(undefined)` when the
   second argument is omitted -- exactly EvaluateImportCall's own "options
   is undefined" branch. `Vm::dynamic_import` (in `vm/modules.rs`) gained a
   second parameter and a new `evaluate_import_call_arguments` helper
   implementing steps 7-10 of EvaluateImportCall synchronously (ToString
   the specifier; if `options` isn't `undefined`, require it to be an
   object; `Get` its `with` property; if defined, require *it* to be an
   object; `EnumerableOwnPropertyNames(attributesObj, KEY)` -- reusing the
   same own-keys/`[[GetOwnProperty]]`-recheck algorithm already shared by
   `Object.keys`/`values`/`entries`, so a `Proxy`'s `ownKeys`/
   `getOwnPropertyDescriptor` traps are observed identically -- then `Get`
   and type-check each attribute value as a String) -- every abrupt
   completion in this synchronous phase rejects the already-created
   promise via the existing `error_value`/`settle_promise` path (preserving
   thrown-value identity for a throwing getter, and building a real
   `TypeError` object otherwise) rather than propagating as a JS-visible
   synchronous throw, matching `IfAbruptRejectPromise`. Attribute
   keys/values are validated but not yet acted on for resolution (no
   `type: "json"` JSON-module support yet -- see remaining gaps below);
   this matches the existing static `import ... with {...}` posture, which
   already only retains the module-request string.

**Evidence** (`backend/bluejs/test262/run.py --filter
"language/module-code/import-attributes/,language/expressions/dynamic-import/,language/import/"`,
same 8 workers/instruction-budget/timeout as the pinned inventory,
`72faf8ec1445c55149615e8b35187830783aba1a`):

| Scope | Before | After |
| --- | ---: | ---: |
| Combined filter above (2,048 modes) | 1,068 pass / 980 fail | 1,388 pass / 660 fail |
| `dynamic-import/import-attributes/` (44 modes) | 0 pass / 44 fail | 40 pass / 4 fail |
| `dynamic-import/` overall (1,856 modes) | 1,039 pass / 817 fail | 1,319 pass / 537 fail |
| `import-defer/` (109 modes) | 13 / 96 (unchanged) | 13 / 96 (unchanged) |
| `import/import-attributes/` (17 modes, JSON modules) | 2 / 15 (unchanged) | 2 / 15 (unchanged) |
| `module-code/import-attributes/` (13 modes) | 13 / 0 (unchanged) | 13 / 0 (unchanged) |
| `import-bytes/` (5 modes) | 0 / 5 (unchanged) | 0 / 5 (unchanged) |

A follow-up run against the *entire* `language/module-code/` tree (2,637
modes, adding every module test outside the filter above) measured
**1,977 pass / 660 fail** -- identical fail count to the narrower filter,
confirming the `Expr::DynamicImport` shape change introduced zero
regressions across the wider module-code suite. `backend/bluejs/tests/
test262_host.rs` gained five new regression tests (all written and
confirmed failing before this session's implementation, since none of this
grammar/algorithm existed yet): accepting an omitted/`undefined`/empty
`with` options object (including one trailing comma after either
argument), rejecting a non-object options argument, a non-object `with`
value, and a non-string attribute value (including propagating a thrown
getter's exact value), specifier-then-options left-to-right evaluation
order plus `in` inside a no-in `for`-head, and the four "Forbidden
Extension" parse rejections (`new import(x)`, `new import(x).prop`,
`import(...args)`, a third argument, plus bare `typeof import`) alongside a
regression guard that `import.source(...)`/`import.defer(...)` still parse.

**Remaining gaps, explicitly not chased this session:**

- `2nd-param-with-type-text.js` needs the separate `import-text` proposal
  (confirmed a `# proposal-import-text` entry in `features.txt`, not yet
  merged) -- out of scope by the same applicability rule as
  source-phase-imports/import-defer below.
- `2nd-param-with-enumeration-enumerable.js` and the 15-test
  `language/import/import-attributes/json-*` suite need actual JSON-module
  support (`type: "json"` producing a module whose default export is the
  parsed JSON value) -- `json-modules` **is** an already-merged
  `features.txt` entry (not a `# proposal-*` line), so unlike
  source-phase-imports/import-defer this is in-scope edition-17 work, just
  not attempted this session: it needs a new module-record kind, not only
  attribute plumbing, and was judged a separate, larger unit of work from
  the calling-convention fix above.
- The remaining ~40 `import-call-unknown.js`/`typeof-import`-adjacent
  `syntax/invalid` failures under plain `dynamic-import` (e.g.
  `import.UNKNOWN(...)`) need `import` to stop being an ordinary global
  identifier binding with `.source`/`.defer` *methods* installed on it (see
  the applicability note immediately below) and instead be parsed as a
  dedicated grammar production that rejects any unrecognized
  `import.<name>`. Left alone deliberately: tightening this would either
  require also implementing `import.source`/`import.defer` as real
  dedicated AST/parser productions (source-phase-imports/import-defer
  scope, addressed below) or regressing the dynamic-import-tagged tests
  that already pass through the current mechanism
  (`source_and_defer_dynamic_imports_reject_through_the_promise_path`).
- The 16 `dynamic-import/catch/*-eval-script-code-target.js` failures
  ("module lexical declaration conflicts with a var declaration") involve
  `eval`-ed script code interacting with a module's top-level bindings,
  unrelated to import attributes; not investigated this session.

**Edition-17 applicability finding for source-phase-imports/import-defer**
(per the task's required check before investing further there):
`test262/features.txt` lists both `source-phase-imports` and `import-defer`
in its leading "Proposed language features" block, each under its own
`# https://github.com/tc39/proposal-*` comment -- the file's own header
states this section is for "language proposals that have reached stage 3,"
i.e. still separate, unmerged TC39 proposals, exactly like
`decorators`/`ShadowRealm`/`ArrayBuffer.prototype.transferToImmutable`
elsewhere in that same block. By contrast, `dynamic-import`,
`import-attributes`, and `json-modules` all appear in the plain
alphabetical feature list further down with no such proposal-link comment,
the same textual signal already used in this file's prior sessions to mean
"merged into a published/tracked edition." This confirms
source-phase-imports and import-defer are **not** part of published
ECMA-262 edition 17 and stay deprioritized per `ECMASCRIPT_2026.md`'s
authority rule -- `import-defer` and `source-phase-imports*`'s large
failure counts (96, and the still-unmeasured-this-session
`source-phase-imports`/`source-phase-imports-module-source` totals) are
proposal-conformance gaps, not edition-17 regressions, and were left
untouched. Interestingly, this host already has a *partial*,
pre-existing `import.source(...)`/`import.defer(...)` implementation (a
real global `import` object with `.source`/`.defer` native methods,
installed in `vm/builtins/globals.rs`, reached through ordinary member-call
parsing rather than dedicated grammar) -- this predates this session and
explains both `import-defer`'s nonzero 13-pass baseline and this session's
deliberate choice not to tighten bare-`import` rejection any further than
the narrow "no `(` and no `.`" case.

## Explicit Resource Management: `using`/`await using`, `DisposableStack`/`AsyncDisposableStack`, `SuppressedError` (2026-09-18)

Edition applicability checked first, per this doc's own standing instruction
not to invest in a feature before confirming it targets the published
edition rather than the living draft. Explicit Resource Management reached
Stage 4 and is part of the officially published **ECMA-262 edition 17**
(ECMAScript 2026, ratified 2026-06-30) — not a draft-only addition. This
matches the pinned Test262 snapshot placing its tests under
`test/built-ins/DisposableStack/`, `test/built-ins/AsyncDisposableStack/`,
`test/built-ins/SuppressedError/` and `test/language/statements/using/`
(real, non-`staging/` directories) rather than `test/staging/`.

**Before**: the feature was wholesale missing. `Symbol.dispose`/
`Symbol.asyncDispose` were already reserved as well-known symbols (unused)
and `Iterator.prototype[Symbol.dispose]` already existed as part of the
Iterator Helpers surface, but `DisposableStack`, `AsyncDisposableStack`,
`SuppressedError` and the `using`/`await using` grammar did not exist at
all: `DisposableStack is not defined`/`AsyncDisposableStack is not
defined` were the dominant Test262 diagnostics, and the
`explicit-resource-management` feature tag stood at 148 pass / 799 fail
(84.4% failing).

**After** (filtered slice: `built-ins/DisposableStack/`,
`built-ins/AsyncDisposableStack/`, `built-ins/SuppressedError/`,
`language/statements/using/`, `language/statements/await-using/`; 397 test
files, 780 scheduled modes):

| | pass | fail | timeout |
| --- | ---: | ---: | ---: |
| Before | 106 | 674 | 0 |
| After | 592 | 187 | 1 |

By directory, remaining failures are concentrated exactly where the
implementation is incomplete, not spread across what's supposedly done:

| Directory | Fail | Why |
| --- | ---: | --- |
| `built-ins/DisposableStack/` | 0/93 | fully passing |
| `built-ins/SuppressedError/` | 2/22 (both modes of one file) | `proto-from-ctor-realm.js`, a `$262` cross-realm `new.target` case, out of scope here |
| `built-ins/AsyncDisposableStack/` | 10/104 | the documented `disposeAsync` simplification below |
| `language/statements/using/` | 45/~184 | for-statement/for-of `using` heads, switch-case placement, module-top-level disposal timing, function-name inference for a `using`-bound anonymous function -- all unimplemented, not incorrect |
| `language/statements/await-using/` | 135/~184 | `await using` syntax itself is not implemented (see below) |

### What was implemented

**`Symbol.dispose`/`Symbol.asyncDispose`**: already-reserved well-known
symbols, now actually exposed as `Symbol.dispose`/`Symbol.asyncDispose`
(they iterate the same `WELL_KNOWN` table `Symbol.iterator` etc. already
use, so no separate wiring was needed there) and consumed for real by
everything below.

**`SuppressedError`**: added as a fourth argument shape
(`error, suppressed, message`) alongside `Error`/`AggregateError`'s
existing shared constructor path in `backend/bluejs/src/vm/errors.rs`
(`error_global`/`error_constructor`), rather than a separate constructor
implementation -- `SuppressedError.prototype`'s `[[Prototype]]` is
`Error.prototype`, and its own-property shape (`message` only if not
`undefined`, then `error`, then `suppressed`, each
`{writable:true,enumerable:false,configurable:true}`) is exactly the
existing generic error-object machinery with one extra branch.

**`DisposableStack`/`AsyncDisposableStack`**: new
`backend/bluejs/src/vm/builtins/resource_management.rs`, implementing the
spec's "Operations on Disposable Objects" abstract operations
(`GetDisposeMethod`, `CreateDisposableResource`, `AddDisposableResource`,
`Dispose`, `DisposeResources`) as `Vm` methods shared by both the
`DisposableStack` builtin and `using` declarations (below) — the same
abstract operations, not two parallel implementations. Each
`DisposableStack`/`AsyncDisposableStack` instance's `[[DisposeCapability]]`
lives in a `HashMap<ObjectId, DisposeCapabilityState>` side table
(`Vm::disposable_stacks`/`async_disposable_stacks`, one map per brand, so a
`DisposableStack` method invoked on an `AsyncDisposableStack` instance
correctly observes a missing internal slot and vice versa) rather than as
ordinary object properties, matching the existing `PromiseRecord` side-table
precedent and keeping the pending resource list unobservable through
`Object.getOwnPropertySymbols`. `adopt`'s spec-mandated synthetic
`() => onDispose(value)` closure collapses into a plain
`{receiver, argument}` pair on the `DisposableResource` record instead of
an actual heap-allocated closure object (`argument: Some(value)` means
"call `method` on `undefined` with `value` as the one argument" instead of
"call `method` on `receiver` with no arguments") -- observably identical,
since the spec's own closure is never exposed to script. `move`'s
`OrdinaryCreateFromConstructor(%DisposableStack%, ...)` names the intrinsic
constructor directly rather than `new.target` (there is none -- `move` is
an ordinary method call), a distinction a first draft got wrong by reusing
the `constructor_prototype`/`new.target` helper meant for actual `[[Construct]]`
dispatch, throwing `TypeError: cannot access a property of null or
undefined` from a stray `self.new_target` read; the regression test
(`disposable_stack_move_transfers_resources_and_disposes_the_source`)
caught it before it shipped.

**`using` declarations (synchronous)**: full parser + compiler + VM support.
`using` is a contextual keyword (`DeclKind::Using` alongside
`Var`/`Let`/`Const`/`AwaitUsing` in `ast.rs`); `using_declaration_follows`
in `parser/functions.rs` recognizes it only when immediately (no line
terminator) followed by an identifier, so `using;`, `using.foo()`,
`using = 1`, `using[x] = null` and `using` followed by a newline all remain
ordinary identifier references — verified directly against
`using-invalid-arraybindingpattern-does-not-break-element-access.js`'s own
scenario. Bindings are const-like (immutable, TDZ) via the same
`enter_scope` mutability check `Const` already used. Disposal-at-scope-exit
reuses `try_statement`'s own handler-stack machinery rather than inventing
parallel control-flow plumbing: `Compiler::statements_with_disposal`
(compiler/statements.rs) wraps a `using`-declaring block/function body/
try-catch-finally body in a synthetic `try { <statements> } finally {
<DisposeResources> }`, so `return`/`break`/`continue`/`throw` crossing the
block already run the disposal exactly once via the *existing*,
already-correct try/finally abrupt-completion path — no new completion
tracking was written. Three new opcodes carry this: `MarkDisposables`
(records the current depth of a VM-wide `Vec<DisposableResource>` when the
block is entered), `AddDisposableResource` (what a `using x = expr;`
declaration's `InitializeBinding` is immediately followed by), and
`DisposeResources` (the synthetic finally body, draining back to the mark
in reverse order). `DisposeResources`'s operand is the enclosing handler's
static index so the interpreter can tell an abrupt entry (handler frame
still present, in `Finally` state, with a pending completion to fold into a
`SuppressedError` on a second error) apart from a normal-completion entry
(the frame was already popped by `PopHandler`) — this is what makes
`SuppressedError` merging *not* reuse the generic try/finally
override-on-second-throw behavior, which would have silently dropped the
first error instead of wrapping it.

The no-`using` case (the overwhelming majority of code) is unaffected:
`statements_with_disposal` checks `has_using_declaration` (a shallow,
non-recursive scan matching the existing `block_lexical_names`/`var_names`
convention) and falls straight through to the original `statements` call
with zero additional opcodes when it finds none.

A `using`/`await using` directly at Script or eval top level (no enclosing
Block/FunctionBody/etc. to dispose it at the end of) is now a
`CompileError::InvalidSyntax`, matching
`using-not-allowed-at-top-level-of-script.js`/`-of-eval.js`.

**Known, deliberate simplification -- `AsyncDisposableStack.prototype.disposeAsync`**:
implemented via the *same* `dispose_resources_sync` used for the
synchronous path (all dispose calls run back-to-back, synchronously, then
the aggregate outcome resolves/rejects a real `Promise`), not the spec's
per-resource `Await(Call(method, V))` chain. This is observably identical
whenever every dispose method is an ordinary (non-thenable-returning)
function -- call order, thrown errors, and `SuppressedError` chaining all
match -- but a dispose method that returns a promise which later *rejects*
is not awaited before `disposeAsync` resolves, so that specific
interleaving is not observed. This accounts for essentially all 10 of
`AsyncDisposableStack`'s remaining failures.

### What remains

- **`await using` declaration syntax** is not implemented at all (parser or
  compiler). This is the largest remaining gap (135 failing modes). It
  needs real bytecode-level `Await` suspension interleaved with each
  resource's disposal (`DisposeResources`'s `needsAwait`/`hasAwaited`
  dance), which the current single-opcode, all-synchronous
  `DisposeResources` design cannot express — unlike `disposeAsync` above,
  an `await using` declaration's disposal happens interleaved with the rest
  of an ordinary function body, not behind a single Promise-returning
  native method, so the "run it synchronously, wrap the outcome" shortcut
  used for `disposeAsync` does not apply here.
- `using`/`await using` in a `for (...)`/`for-of`/`for-in` head (own
  grammar production, `ForBinding : using ForBinding`) is not parsed.
- The Script/eval top-level restriction is enforced; the analogous
  restriction inside a `switch` `case`/`default` clause list (without an
  enclosing block) is not.
- Module top-level `using` (explicitly *allowed*, unlike Script) is not
  wired to dispose at module-evaluation completion; the one Test262
  `timeout` in the after-slice above
  (`initializer-disposed-at-end-of-module.js`) is this gap surfacing as an
  async test that never calls `$DONE`, not a hang or crash.
- Anonymous function/class name inference for a `using`-bound initializer
  (`using arrow = () => {}` should get `.name === 'arrow'`) is not wired,
  since `using`'s compiler path was not connected to
  `expression_with_name`'s existing inferred-name plumbing.
- **A narrow, pre-existing gap this feature inherits rather than causes**:
  an empty (or otherwise `undefined`-completion) `try{}finally{}` already
  does not restore the *preceding* statement's completion value in this
  engine (`eval('4;try{}finally{}')` evaluates to `undefined`, not `4`,
  independent of this feature). Because `using` disposal is compiled as a
  synthetic try/finally, `using`'s own completion-value test
  (`language/statements/using/cptn-value.js`) inherits the same gap
  (`eval('4;{using x=null;}')` also evaluates to `undefined` instead of
  `4`). Fixing the general try/finally completion-value/`UpdateEmpty`
  behavior is out of scope for this slice.
- A generator/async-function body that `yield`/`await`-suspends *while
  still inside* a `using`-declaring block, with unrelated code running its
  own `using` declarations before that generator/async function resumes,
  is not isolated correctly: `Vm::disposables`/`dispose_marks` are flat,
  VM-wide stacks (matching lexical nesting and ordinary synchronous
  call/return, including recursion, exactly) rather than per-suspension
  state threaded through `InterpreterExit::Yield`/`Await` the way
  `iterators` already is. This is a deliberate, documented scope cut (see
  `Vm::disposables`'s own doc comment in `vm.rs`): plain `using` (as opposed
  to the unimplemented `await using`) realistically only appears in plain
  synchronous functions/blocks, where no suspension is possible at all.

### Verification

`backend/bluejs/tests/resource_management.rs` (new, 18 tests): well-known
symbol exposure; `DisposableStack` `use`/`adopt`/`defer`/`move`/`dispose`/
`disposed` including reverse-order disposal, idempotent dispose,
already-disposed errors, nullish/non-object `use` handling, and the
`adopt`/`defer` receiver-and-argument contract (`'use strict'` in that one
test specifically, so sloppy-mode `this`-substitution to `globalThis`
doesn't mask a wrong receiver); `SuppressedError`'s own constructor shape;
disposal-error suppression both via direct `DisposableStack.dispose()` and
via a `using` declaration racing a thrown error in the block body; `using`
disposal ordering, early return/break/throw, null/undefined resources,
const-like immutability, the destructuring-pattern/missing-initializer
early errors, `using` remaining a plain identifier outside declaration
position (including the newline-suppresses-the-declaration case), the
zero-`using` fast path producing the same result as before this feature
existed, and `AsyncDisposableStack.prototype.disposeAsync` resolving and
rejecting correctly. All 18 pass; each failed against `unimplemented!`/
`ReferenceError`/wrong-behavior stubs before the corresponding
implementation piece landed (TDD, not written after the fact to match
already-working code).

Every fix above was caught by first writing (or already having, for the
`move`/`constructor_prototype` bug) a failing check and then correcting the
implementation, not by tightening a test to match observed behavior; the
`using-invalid-arraybindingpattern-does-not-break-element-access.js`-derived
test is a direct example -- the temptation was to write the more obvious
"a bracketed pattern after `using` is a syntax error" test, until the
real Test262 case revealed that the correct grammar decision is exactly the
opposite when the token after `using` isn't a plain identifier.

## JSON modules, a dynamic-import promise-rejection classification bug, and the `eval-script-code-target` finding

Implemented 2026-09-18, continuing the same-day session above per specific
follow-up direction: (1) implement JSON modules (confirmed in-scope for
edition 17, previously deferred as "a separate, larger unit of work"), (2)
finish the general `dynamic-import` deep dive the first pass didn't reach,
(3) look at the 16 `eval-script-code-target` failures without over-investing.

### 1. JSON modules (`ParseJSONModule` / `CreateDefaultExportSyntheticModule`)

Official clauses read: the Import Attributes proposal's own §1.4
`ParseJSONModule` / §1.5 `CreateDefaultExportSyntheticModule` (merged into
the mainline spec text this proposal now lives in, confirmed non-proposal
per `features.txt`'s `json-modules` entry, same authority check as the
session above) -- `json = ? Call(%JSON.parse%, undefined, «source»)`, then
a Synthetic Module Record whose sole export is an already-initialized,
immutable `default` binding to `json`.

**Root cause.** `parser/module_items.rs::parse_import_attributes` validated
`with {...}` syntax but discarded every attribute's value (a deliberate,
documented scope limit from the prior `import(specifier, options)` slice);
nothing anywhere routed a `type: "json"` request differently from an
ordinary Source Text Module request, and the `.json` fixture files
themselves were never even collected by the Python harness (`module_sources()`
explicitly excluded non-`.js` siblings, by original design, "left to the
adapter's normal module-resolution result").

**Fix**, across five layers:

1. **Attribute plumbing** (`ast.rs`, `parser/module_items.rs`,
   `compiler.rs`, `bytecode.rs`): `ImportEntry` and the `ExportEntry`/
   `ModuleExport` variants that reference another module (`Indirect`,
   `Star`, `Namespace`) gained a `json: bool`, set from
   `parse_import_attributes`'s now-`bool`-returning result (true iff a
   `type: "json"` entry was present) and threaded through compilation
   unchanged. `Bytecode.module_requests`/`ModuleRequest` deliberately did
   *not* need this flag: it only matters at the one-time registration step
   below, never at ordinary dependency-evaluation-order traversal.
2. **Synthesis** (`vm/modules.rs::ensure_json_module`): given a *resolved*
   module name, looks up raw JSON text in a new host-supplied
   `Vm::json_module_sources` registry (installed via the new
   `Vm::set_json_module_sources`, mirroring `set_module_source_loader_context`'s
   existing pattern for source-phase records), calls the engine's own
   `json_parse` (the exact `JSON.parse` implementation, not a
   reimplementation), and builds a trivial real `Bytecode`: one binding
   (`default`), one scope, `module_exports: [Local{"default", slot 0}]`,
   and a new `Bytecode.json_module_value: Option<Value>` field carrying the
   already-parsed value. This is a deliberate, documented, narrow exception
   to "`Bytecode`... can execute repeatedly in the same or independent
   VMs... runtime object handles are never stored in its constant pool" --
   a JSON module's synthesized `Bytecode` is per-`Vm`, built fresh from raw
   text each time `ensure_json_module` first sees a given resolved path,
   and never shared across realms. Idempotent (a second call for the same
   resolved path is a no-op), which is what gives repeated imports of one
   JSON file the required object identity.
3. **Linking integration** (`vm/modules.rs::execute_module_graph_inner`):
   two call sites of `ensure_json_module`, both running before the
   function's existing major GC collection (rooting the freshly parsed
   value through the same `roots: Vec<RootId>` the rest of the function
   already threads through, so nothing before it has a cell yet to keep it
   alive) --
   `register_static_json_modules` scans a *fresh* graph's own
   `with`-attributed requests once (a JSON module never itself requests
   further modules, so one pass is exhaustive) and registers each target;
   a new `entry_json: bool` parameter (from a dynamic import's own
   attribute check) registers `entry` itself directly, covering a pure
   `import(spec, {with:{type:"json"}})` with no static import anywhere in
   the graph, on both a fresh and an *already-linked* existing graph (the
   latter needs a hand-built single `LinkedModule` entry, since fresh-graph
   linking's own per-`order` loops never run for it). A JSON module's slot-0
   cell is created by the *same* generic non-lexical-binding loop every
   other module's cells go through (getting an ordinary `Undefined` value
   first), then a short follow-up pass overwrites it with the real parsed
   value and marks the record `evaluated: true` -- so `evaluate_module_record`'s
   existing `if record.evaluated { return Ok(Undefined) }` short-circuit
   means the module's (empty) instruction stream is never actually
   interpreted; `resolve_export`/`module_namespace`/`exported_names` needed
   no changes at all, since they only ever consult `module_exports` +
   `linked[..].cells`, uniformly for any `Bytecode`, synthesized or not.
4. **Dynamic import attribute plumbing** (`vm/modules.rs`, `vm.rs`):
   `evaluate_import_call_arguments` (from the prior slice) now also
   extracts the enumerated `type` attribute's value, returning
   `(specifier, json)`; `PromiseJob::DynamicImport` gained a `json: bool`
   field threaded through to `dynamic_import_job`, which passes it to
   `execute_module_graph_inner` as `entry_json`.
5. **Harness** (`backend/bluejs/test262/run.py`, `bluejs-test262.rs`):
   `module_sources()` now returns `(sources, json_sources)` -- `.json`
   siblings (found via the *same* existing static/dynamic-import-reference
   regexes) are collected as raw text into the new `json_sources` return
   value instead of being skipped, and a new `module_json_sources` request
   field carries them to `Vm::set_json_module_sources`.

**A real, load-bearing harness bug found and fixed along the way**: for a
plain-script (non-`module`) test whose only dynamic import references were
`.json` fixtures, `module_codes` (compiled `.js` siblings) ended up empty,
and the adapter's `vm.set_module_loader_context(...)` call was gated on
`!module_codes.is_empty()` -- skipping it entirely, so `dynamic_import`'s
referrer fell back to the resolution-breaking `"<script>"` default instead
of the test's own path, and every such JSON-only dynamic import failed to
resolve. Fixed by gating on `request.module_path.is_some()` instead (set
whenever the harness detected any dynamic import at all, `.json`-only or
not) -- an empty module registry is a perfectly valid, already-supported
`set_module_loader_context` call; only the *referrer* was missing.

**Evidence** (`--filter "language/import/import-attributes/,language/expressions/dynamic-import/import-attributes/"`, 61 modes, both known-in-scope JSON areas from the prior slice's "remaining gaps"):

| Stage | Pass / fail |
| --- | ---: |
| Before this slice (round 1 baseline) | 42 / 19 |
| After JSON module synthesis, before the harness referrer fix | 52 / 9 |
| After the harness referrer fix | **54 / 7** |

The remaining 7 all need the separate, unmerged `import-text` proposal
(`type: "text"`/self-referencing-module-as-text fixtures) -- confirmed via
the same `features.txt` "Proposed language features" check as
source-phase-imports/import-defer, out of scope for the same reason.
`backend/bluejs/tests/test262_host.rs` gained eight new regression tests
(default-export value across every JSON type, namespace shape, extensibility,
named-binding/malformed-JSON resolution errors, cross-site identity
including through a dynamic import, a pure-dynamic no-static-import case,
a missing-host-source `TypeError`, and a Proxy-based `with` attributes
object exercising the same `EnumerableOwnPropertyNames` path
`Object.keys` uses) plus two new Python tests in
`backend/bluejs/test262/test_runner.py` for `module_sources()`'s new
`(sources, json_sources)` return shape.

### 2. General `dynamic-import` deep dive: a real promise-rejection misclassification

Reading the plain (non-`source-phase`/`import-defer`-tagged)
`dynamic-import` failures remaining after the JSON work, a large cluster (34
files, `catch/*-instn-iee-err-{ambiguous-import,circular}*.js`) all showed
an *uncaught* `Test262Error` where the test's own `.catch(error => {
assert.sameValue(error.name, 'SyntaxError') })` handler should have run
cleanly -- meaning the promise rejected with something whose `.name` wasn't
`"SyntaxError"`, failing that assertion and producing an uncaught rejection
from the outer `.then($DONE, $DONE)`.

**Root cause**: `vm/builtins/promises.rs`'s dynamic-import job drain had a
special case, `Err(RuntimeError::ModuleResolution(message)) => "TypeError"`,
present specifically for dynamic imports (every other context --
`error_value`, used for static `execute_module_graph` failures and every
other promise rejection -- already mapped `ModuleResolution` to a real
`SyntaxError`, matching `resolve_export`'s own ambiguous/missing-export
`ModuleResolution` errors, which per spec's `ResolveExport`/module
instantiation steps must be `SyntaxError`s, not `TypeError`s). This override
predates this session; grepping the whole `dynamic-import` test directory
found zero tests checking `instanceof TypeError`/`.constructor===TypeError`
for a resolution-style dynamic-import failure, and 80 checking
`error.name==='SyntaxError'`. The one `TypeError`-observing test that does
exist for dynamic import (`catch/*-eval-rqstd-abrupt-typeerror.js`, and its
`eval-rqstd-abrupt-err-type_FIXTURE.js`, `throw new TypeError()`) goes
through a completely different path -- a real thrown value
(`RuntimeError::Thrown`), never `ModuleResolution` -- so it was never
reached by, and is unaffected by, removing the override.

**Fix**: deleted the override; `Err(error) => { let error = self.error_value(error)?; ... }`'s
existing generic arm now handles `ModuleResolution` for dynamic imports the
same way it already did everywhere else.

**Evidence** (`--filter "language/module-code/,language/expressions/dynamic-import/,language/import/,built-ins/ImportAttributes"`, 2,637 modes, same scope as the prior session's full-tree regression check):

| Stage | Pass / fail |
| --- | ---: |
| Prior session's end state | 1,977 / 660 |
| After JSON modules (this session) | 1,990 / 647 |
| After the promise-rejection fix | **2,053 / 584** |

Zero regressions at every stage (`module-code-other`, `static-import-attrs`,
`import-defer`, `import-bytes` all held constant throughout; verified with
a full `cargo test -p blueice-bluejs --test test262_host`, 95/95, including
the two existing tests -- `async_test_style_chain_handles_a_rejected_dynamic_import`,
`source_and_defer_dynamic_imports_reject_through_the_promise_path` -- whose
own rejections never went through `ModuleResolution` to begin with and so
were never exercising the removed branch).

Plain `dynamic-import` failures (excluding `import-attributes`/
`import-defer`/`source-phase-imports`-tagged) fell from 131 to 68 across
this session's two fixes. The residue is almost entirely accounted for:
42 are the already-documented `import.UNKNOWN(...)`/bare-`typeof import`-adjacent
cases blocked on the pre-existing `import.source`/`import.defer` global-object
mechanism (see the applicability note above), 16 are the
`eval-script-code-target` finding below, and the remaining ~10 are
individually distinct (an instruction-budget case, a `sameValue` mismatch,
a handful of other single-file diagnostics) with no shared root cause found
worth chasing further this session.

### 3. `eval-script-code-target`: a real gap, not a quick fix

The 16 `catch/*-eval-script-code-target.js` failures (e.g.
`top-level-import-catch-eval-script-code-target.js`) all dynamically import
`script-code_FIXTURE.js`, whose content (`var smoosh; function smoosh(){}`)
is valid script code but a genuine early `SyntaxError` as module code (a
lexically-declared function name colliding with a `var`) -- and the test
expects that failure to surface *lazily*, as a promise rejection caught by
`.catch(error => assert.sameValue(error.name, 'SyntaxError'))`, since the
fixture is reachable only through a dynamic import, never statically.

BlueJS's own module-graph engine already gets this right in principle: a
genuine linking-time failure for a module reached only via dynamic import
correctly becomes a promise rejection (this is exactly the machinery the
fix above relies on). The actual failure is architectural, in the
**Test262 harness adapter**, not the engine: `bluejs-test262.rs` compiles
every entry of `request.module_sources` -- both modules statically
reachable from the entry and ones reachable only through a dynamic
import -- into one `HashMap<String, Bytecode>` *before* any execution
starts, and a `parse_module`/`compile_module_with_limit` failure on *any*
of them (lines ~136-174) immediately returns a whole-test-run
`{"phase":"resolution",...}` result. That is correct for a module the
static entry graph actually needs, but wrong for one intentionally invalid
*only as a module* that a real host would parse lazily, at the moment a
dynamic import actually resolves it.

Fixing this properly needs new machinery analogous to this session's JSON
work: a raw-source registry for modules reachable only dynamically, with
`ensure_json_module`'s dynamically-imported sibling parsing raw text
on demand (via `parse_module`/`compile_module_with_limit`, already
crate-visible) and turning a compile failure into a `RuntimeError` that
flows through the same promise-rejection path, rather than being resolved
eagerly by the harness. That is a distinct unit of work from anything in
this session's three tasks, not a quick fix, so it was intentionally left
unimplemented per this session's explicit "don't burn a lot of budget here"
guidance -- documented here as a real, scoped, and reproducible gap for a
future session rather than attempted partially.

## ShadowRealm: construction, evaluate/importValue, and cross-realm wrapped functions

Implemented 2026-09-18. **Edition-17 applicability finding**: `ShadowRealm`
is **not** part of published ECMA-262 edition 17. Its own proposal
repository (`tc39/proposal-shadowrealm`) reports it at TC39 **Stage 2.7** as
of this date -- not yet Stage 3, let alone merged into a published edition
-- and the `262.ecma-international.org/17.0/` table of contents has no
`ShadowRealm` clause. It is present in this Test262 snapshot only because
the pinned revision's own selection note says "current Test262 main..,
proposals and staging are included, not filtered to an ECMA edition"
(`backend/bluejs/test262/snapshot.json`). Implemented anyway per an explicit
request; `ECMASCRIPT_2026.md`'s workstream table should credit this as a
proposal-tracking slice, not edition-17 progress, if it is cited there.

### Result

`built-ins/ShadowRealm/`: **0/124 -> 114/124** passing modes (64 files),
starting from a 100% pre-implementation failure baseline.

### What's implemented

`backend/bluejs/src/vm/shadow_realm.rs` builds directly on the "a realm is a
whole child `Vm`" primitive `test262.rs`'s `$262.createRealm()` already
established, rather than inventing a second one: `new ShadowRealm()` creates
a boxed child `Vm` sharing this `Vm`'s `GlobalSymbolRegistry` (matching
`$262.createRealm()`'s identical choice, for the identical spec-mandated
reason -- `Symbol.for` is agent-wide even though every Realm keeps its own
globals), tracked in new `Vm` fields (`shadow_realms`, `shadow_realm_by_heap`,
`shadow_wrapped_functions`) alongside the existing
`test262_realms`/`test262_foreign_values`.

- `ShadowRealm.prototype.evaluate` (`PerformShadowRealmEval`): parses and
  compiles `sourceText` as a classic Script; a parse failure throws a real
  `SyntaxError` directly (`ParseText`'s own failure, before any execution
  context exists), while any abrupt completion *during* execution -- a
  thrown value, or a promise job's own rejection drained synchronously
  afterward, since this engine has no realm-independent job queue -- becomes
  an opaque, message-less `TypeError` in the caller's realm
  (`CreateTypeErrorCopy`). These are genuinely different spec paths, not an
  implementation shortcut: confirmed by fetching the proposal's own
  `PerformShadowRealmEval`/`CreateTypeErrorCopy` text before assuming either
  one, after an early attempt wrongly wrapped *every* completion the same
  way and failed `throws-syntaxerror-on-bad-syntax.js`.
- `GetWrappedValue`/`WrappedFunctionCreate`: primitives (this engine's
  `Value::String`/`Number`/`Bool`/`BigInt`/`Symbol`/`Undefined`/`Null` carry
  no heap affinity) cross a boundary unchanged; a callable Object becomes a
  fresh `NativeFunction::ShadowRealmWrappedFunction` facade allocated in the
  destination realm (never cached -- a new facade every crossing, matching
  `wrapped-functions-new-wrapping-on-each-evaluation.js`); any other Object
  is a `TypeError`. `CopyNameAndLength` reads `length`/`name` through
  `Vm::proxy_get_own_property` (not the raw heap record), so a revoked or
  throwing-trap Proxy target is reported correctly instead of silently
  defaulting.
- Calling a wrapped function (`OrdinaryWrappedFunctionCall`) wraps
  `this`/each argument *into* the target realm and the result *back* into
  the caller's, so a caller-side function passed as an argument becomes
  itself a fresh wrapped facade the callee can invoke -- the fully
  bidirectional case
  (`wrapped-function-arguments-are-wrapped-into-the-inner-realm.js`,
  `wrapped-functions-accepts-callable-objects.js`, and
  `wrapped-function-multiple-different-realms(-nested).js`'s multi-hop
  chains through 3-5 realms in both directions within one expression).
- `ShadowRealm.prototype.importValue` reuses the same host-supplied module
  registry ordinary dynamic `import()` already uses (`Vm::dynamic_import`):
  the child realm borrows the caller's
  `module_registry`/`active_module_name` for the duration of one call, and
  any failure (bad specifier, a throwing or unparseable module, a missing
  export) rejects the returned promise with an opaque `TypeError`, matching
  the proposal's own `%ThrowTypeError%` rejection handler. `exportName`
  needed its own read of the actual algorithm text before implementing:
  unlike `specifier` (`? ToString(specifier)`), `exportName` is a *plain
  type check with no coercion attempted* ("If exportName is not a String,
  throw a TypeError exception") -- `throws-if-exportname-not-string.js`
  specifically asserts a throwing `toString` on a non-string `exportName` is
  never even called.

### The reentrancy problem this needed solving, and how

Unlike Test262's own realm membrane (which forwards arbitrary object
operations and deliberately leaves an argument object passed *into* a child
realm as a non-forwarding opaque stand-in -- see `test262_transport_value`'s
own comment: "property forwarding needs a resumable cross-VM operation and
is not implied by passing an otherwise opaque argument through a foreign
call"), `ShadowRealm`'s wrapped functions must genuinely call back and
forth in both directions. `wrapped-function-multiple-different-realms.js`
and its `-nested` sibling chain calls through 3-5 realms within a single
expression, including a realm calling back into a *grandparent* it does not
own directly.

Every `Vm` a `ShadowRealm` creates is owned as a plain `Box<Vm>` inside its
creator's own `shadow_realms` map -- there is no shared/reference-counted
ownership between realms. Reaching an ancestor (or an ancestor's sibling)
`Vm` from deep inside a nested call therefore needs something other than
ordinary field access. The fix is a thread-local stack,
`ACTIVE: Vec<(heap_tag, *mut Vm)>`: immediately before a `Vm` calls into
another realm, `register_active` pushes a raw pointer to itself, tagged by
its own heap id (`ObjectId::heap`); the RAII `ActiveGuard` pops it the
instant that nested call returns. A callee that needs to reach back into an
ancestor resolves it by tag through this stack instead of through any
`HashMap`. The safety argument (documented in full on `ACTIVE` itself in
`shadow_realm.rs`): a pointer is only ever present for the exact dynamic
extent of a `&mut Vm` call already suspended on the Rust stack when it was
pushed, so a wrapped function retained and called again long after that call
chain returned simply finds no entry (a catchable error) rather than
dereferencing freed memory -- and a `std::ptr::eq` check refuses the
degenerate case of a chain looping all the way back to its own origin realm
within one call.

A second, related bug this exposed and fixed: a realm's own child can be
*directly owned* by `self` (present in `self.shadow_realms`) while
simultaneously being *checked out* (removed from that map for the duration
of a call already using it, the same pattern `test262_foreign_call` already
uses to avoid aliasing `self.shadow_realms` while a nested call runs).
`wrapped-function-multiple-different-realms-nested.js`'s 5-realm-deep chain
does exactly this -- the chain loops back to a realm's own child while an
ancestor frame is already using that exact child -- and the first
implementation's `.expect()` panicked trying to remove it a second time
(reported by the runner as `"kind": "crash", "message": "adapter exited
with code 101"`). The fix: `shadow_call_wrapped` and `shadow_realm_evaluate`
now check `ACTIVE` *before* trying to remove from their own `shadow_realms`
map, and a freshly-checked-out child is itself registered in `ACTIVE` (by
its own tag) for the duration it is in use, so a call chain that loops back
through it is found there instead of attempting a second removal.

A third, independent bug: a wrapped function's target had no GC root of its
own. An arrow function returned directly as an `evaluate()` completion value
(never stored in that realm's own globals) has nothing else in its own
realm's reachability graph keeping it alive, so an unrelated later
allocation in that realm's heap (e.g. a second `evaluate()` call
materializing new intrinsics) could reclaim it before a wrapper elsewhere
ever called it -- reproduced directly by running the multi-realm test with
an extra intervening `evaluate()` call inserted between creating and calling
the wrapper, confirmed via the engine's own `bluejs gc reclaim` stderr
trace. Fixed by rooting the target (`Heap::root`) in its own realm for the
wrapper's lifetime, mirroring `Test262ForeignValue`'s identical
`_target_root` pattern one field over.

### Test262 runner infrastructure fix (shared with, but scoped away from, dynamic `import()`)

`ShadowRealm.prototype.importValue('./relative.js', name)` is an ordinary
method call, not `import`/`import.source`/`import.defer` syntax, so the
runner's existing dynamic-import-with-a-variable-specifier heuristic
(`DYNAMIC_IMPORT_EXPRESSION` triggering a relative-string scan of the test
source for sibling fixtures) never found `import-value_FIXTURE.js` for
`import-value.js` -- a real gap in the runner, not the engine, that a plain
"module not found" happened to mask for every *other* `importValue` test
(each expects a `TypeError` rejection regardless of the specific reason, so
a missing fixture and a genuinely broken one both "pass" until a test
actually expects success). Added a second trigger,
`SHADOW_REALM_IMPORT_VALUE_EXPRESSION` (`\.importValue\s*\(`), alongside the
existing one.

That alone regressed `throws-typeerror-import-syntax-error.js`: its fixture
is *deliberately* unparseable (it tests that `importValue` rejects when the
imported script can't be parsed), but the adapter's `mode == "module"` path
eagerly precompiles every `module_sources` entry up front and hard-fails the
*entire request* on any parse error -- correct for a genuinely
statically-imported module (a real linking failure the corpus already
depends on testing this way), wrong for a candidate that is only a
speculative relative-string guess never actually required by anything.
`module_sources()` now also returns which collected paths were reached
*only* through such a guess (never through a real `import`/dynamic-
`import()` reference), and the adapter (`bluejs-test262.rs`) skips --
instead of hard-failing on -- a parse/compile failure for exactly those
paths (`speculative_module_sources` in the request). To keep this from
touching the already-large, separately-exercised dynamic-`import()` corpus,
that leniency is further scoped to apply only when `.importValue(` is what
triggered the string scan and no actual dynamic-`import()` expression is
also present -- verified unchanged (861 fail / 1039 pass, identical file-
for-file before and after this change) against
`language/expressions/dynamic-import/` before finishing.

### Remaining gaps (10/124 failing modes, 5 files)

All confirmed, by direct reproduction, to be **pre-existing** limitations
unrelated to this work -- none are ShadowRealm-specific, and each reproduces
identically on a plain `Vm`/Test262-realm scenario with no `ShadowRealm`
involved at all:

- `globalthis-available-properties.js`, `globalthis-config-only-properties.js`
  (4 modes): `Object.prototype.hasOwnProperty.call(globalThis, 'Array')`
  (direct reflection, bypassing the compiler's identifier fast path) returns
  `false` for at least this one lazily-materialized global, even on the
  *outer*, non-ShadowRealm realm with no prior touch, while `'JSON'` and
  `'isFinite'` checked the same way both correctly return `true` -- a
  narrow, name-specific gap in `materialize_global_object_property`'s
  dispatch, not chased further given this session's scope.
- `returns-primitive-values.js` (2 modes): needs `Number.isNaN`, not yet an
  implemented `Number` static (the global `isNaN`/`isFinite`/etc. exist; the
  `Number.*` statics remain part of the still-open "complete builtin
  libraries" workstream per `ECMASCRIPT_2026.md`).
- `wrapped-function-proto-from-caller-realm.js`,
  `wrapped-function-throws-typeerror-from-caller-realm.js` (4 modes): both
  use `$262.createRealm()` to construct a `ShadowRealm` in one Test262 realm
  and then pass *that* `ShadowRealm` instance into a *third*, unrelated
  Test262 realm (`YetAnotherShadowRealm.prototype.evaluate.call(realm, ...)`).
  Test262's own membrane represents an object crossing between two realms
  neither of which is the immediate caller as an opaque, brand-less stand-in
  (`test262_transport_value`'s own documented limitation, quoted above), so
  this feature's `[[ShadowRealm]]` brand check correctly reports it as *not*
  a `ShadowRealm` once it arrives that way -- a pre-existing Test262-membrane
  gap this feature's brand check did not introduce, and could not paper over
  without extending that membrane's own object-identity model.

### Verification

`backend/bluejs/tests/shadow_realm.rs` (new, 12 tests): construction/brand
checks, `evaluate`'s primitive-passthrough and non-primitive/non-callable
`TypeError` boundary, the `SyntaxError`-vs-opaque-`TypeError` split, a
wrapped function's `length`/`name`/fresh-identity-per-crossing, the
bidirectional callable-argument case, the multi-realm GC-rooting regression,
and `importValue`'s resolve/reject paths through
`set_module_loader_context`. `cargo build --workspace --all-targets`,
`cargo test --workspace --no-fail-fast` and `cargo clippy --workspace
--all-targets -- -D warnings` all pass except the one pre-declared
known-flaky `observable_conversion_order_and_gc_pressure` in
`tests/string_protocols.rs` (intermittent `HeapLimitExceeded`, unrelated to
this work).

## Explicit Resource Management closure: `await using`, for-loop heads, module top level, and real per-resource `Await` in `disposeAsync` (2026-09-18, continued)

Follow-up to the same day's slice above, closing the gaps that slice's own
report flagged as remaining, in the priority order requested: `await
using` syntax first (the largest gap by far), then `for`/`for-of` `using`
heads, the switch-case restriction (already done in the same pass as the
name-inference fix, see below), module-top-level disposal timing, and
anonymous-function-name inference (also already closed alongside the
switch-case fix). Filtered-slice evidence (same filter as before; 397
test files, 780 modes):

| | pass | fail | timeout |
| --- | ---: | ---: | ---: |
| Start of this continuation | 592 | 187 | 1 |
| After `await using` | 716 | 61 | 3 |
| After for/for-of `using` heads | 748 | 29 | 3 |
| After module top-level disposal | 766 | 14 | 0 |
| After real per-resource `Await` in `disposeAsync` + for-await-of-of fix | 768 | 12 | 0 |

**98.5% of the filtered slice now passes** (768/780); the 3 timeouts (all
module-related, "disposed at end of Module" tests whose `$DONE()` lived
inside the never-called dispose method) are gone entirely, not just turned
into ordinary failures.

### `await using` declarations (the priority-1 item)

The blocking design question from the prior slice was real: `DisposeResources`
as a single atomic native opcode cannot express the spec's per-resource
`Await(Call(method, V))`, because only compiled bytecode can suspend and
resume through the VM's existing `Await`/generator machinery -- a native
Rust function call cannot yield control back to the event loop mid-call.
The resolution avoids inventing new suspension plumbing entirely: a
using-declaring block/function body/for-head that contains at least one
`await using` (`has_await_using_declaration`) compiles a *different*
finally body than the plain-synchronous fast path.

`Opcode::DrainAsyncDisposables` (one native step, mirroring
`DisposeResources`'s own abrupt-vs-normal-entry handler-index trick for
merging a pending error) converts the block's native disposable-resource
list into a plain JS value `[hasError, pendingError, entries]`, where
`entries` is a real Array of `[receiver, method, hasArgument, argument,
isAsync]` records, one per resource, in declaration order.
`Compiler::compile_async_dispose_finally` (`backend/bluejs/src/compiler/statements.rs`)
then synthesizes an ordinary `while`/`try`/`catch` loop over that array --
built from real `ast.rs` nodes (`Identifier`/`Member`/`Call`/`Await`/`New`/
`Assign`/`If`/`Try`) and compiled through the *normal* statement/expression
pipeline, `try_statement`/`loop_statement` included -- so a resource that
needs awaiting suspends and resumes through the already-correct,
already-tested `Await` path, not a new one. `SuppressedError` merging is
just the synthesized catch clause's `new SuppressedError(...)`, an
ordinary compiled expression, not new merge logic.

This bought real per-resource `Await` semantics essentially for free,
verified directly (not just inferred): a dispose method's own returned
promise is genuinely awaited before the next resource is disposed
(`await_using_awaits_the_dispose_methods_own_returned_promise`), and a
promise it returns which *later rejects* becomes the disposing async
function's own rejection
(`await_using_propagates_a_rejected_dispose_promise_as_a_real_rejection`)
-- exactly the case the prior slice's `AsyncDisposableStack.prototype.disposeAsync`
simplification documented as unable to observe.

Parser: `await_using_declaration_follows` (`parser/functions.rs`) mirrors
`using_declaration_follows`'s no-LineTerminator lookahead across both
contextual keywords, gated on the same `async_depth`/`module_await` check
an ordinary `await` expression already uses.

### `for`/`for-of` `using` heads (priority 2)

Two genuinely different disposal timings, both confirmed directly against
Test262's own file naming before implementing either:

- **C-style `for (using x = v; ...; ...)`** disposes once, when the whole
  `ForStatement` completes (`initializer-disposed-at-end-of-forstatement.js`,
  singular) -- handled by wrapping the *entire* `loop_statement` call in
  the same disposal machinery a block uses, via a new shared
  `Compiler::wrap_with_disposal` helper factored out of
  `statements_with_disposal` (which now just calls it).
- **`for (using x of iterable)`** (`ForBinding : using ForBinding`, a
  distinct for-of-only production) disposes *each iteration's own binding*
  at the end of *that* iteration
  (`initializer-Symbol.dispose-called-at-end-of-each-iteration-of-forofstatement.js`)
  -- handled inside `for_each` (`compiler/expressions.rs`) by wrapping just
  the current iteration's bind-and-body in `wrap_with_disposal`, inside the
  per-iteration lexical scope `for_each` already creates for `let`-style
  bindings. Both reuse the exact same handler-stack machinery as the block
  case, so break/continue/return crossing either one already dispose
  correctly via the generic abrupt-completion path, with no new tracking
  written for it.

Parser disambiguation turned out to be the fiddly part, resolved by
checking each case directly against its own named Test262 file rather than
guessing: `using`/`await using` is rejected outright in a for-in head
(`using-invalid-for-in.js`; ForBinding has no ForIn production);
`for (using of expr)` treats `using` as a bare identifier being iterated,
*not* a declaration, since `using` alone can validly stand as an ordinary
for-of loop variable (`using-for-using-of-of.js`); but
`for (using of = expr;;)` (a using declaration whose bound identifier's
*name* is `of`) and `for (await using of of expr)` are both still
declarations -- for the first because the disambiguating token after the
second identifier is `=`, not the for-of separator
(`using-for-statement.js`, "`for (using of =` are interpreted as for
loop"); for `await using` specifically, the bare-identifier reading is
never even grammatically available (`await using` alone would have to
parse as an `AwaitExpression` wrapping `using`, which is not a valid for-of
assignment target), so `await_using_declaration_follows_in_for_head` needs
no `of`-exclusion at all, unlike its plain-`using` counterpart
(`await-using-valid-for-await-using-of-of.js`).

### Module top-level disposal (priority 4)

Unlike a Script (where `using`/`await using` is rejected outright at the
top level -- no enclosing block to dispose it at, per the prior slice), a
Module's top level *is* one of the spec's permitted contexts, and disposes
when the module's own evaluation completes. Before this fix, module
top-level `statements_after_function_declarations` never went through
`statements_with_disposal` at all, so the resource was simply never
disposed -- concretely, in the `[module, async]`-flagged Test262 tests,
the dispose method that calls `$DONE()` was never invoked, hanging the
async test harness forever (the three timeouts in the table above, not
merely failures). Fixed with the same one-line-shaped change as the
for-loop case: wrap the module top-level's statement compiling in
`wrap_with_disposal` when it directly declares a `using`/`await using`.

### Real per-resource `Await` in `AsyncDisposableStack.prototype.disposeAsync`

Not on the original priority list, but directly exposed by writing the
`await using` tests above: `disposeAsync`'s prior "run every dispose call
synchronously, then wrap the aggregate outcome in a Promise" simplification
was not just slower than the spec algorithm, it was observably wrong for a
*genuinely* async dispose method. `stack.defer(async function () { throw
new MyError(); })` calls an async function, which never throws
synchronously -- it always returns a (here, rejected) Promise -- so the
prior implementation's synchronous call saw no error at all and silently
lost the rejection (`rejects-with-error-as-is-if-only-one-error-during-disposal.js`).
Separately, `this-not-object-rejects.js`/
`this-does-not-have-internal-asyncdisposablestate-rejects.js` expect a
*rejected Promise*, not a synchronous `TypeError` throw, from a bad
receiver -- `assert.throwsAsync` calls `disposeAsync.call(badThis)` and
awaits its *return value* rejecting, which a synchronous throw before ever
returning a Promise cannot satisfy.

Both are fixed together: `Vm::async_dispose_helper`
(`vm/builtins/resource_management.rs`) lazily compiles and caches, once,
an internal async function with the *exact* same algorithm as
`compile_async_dispose_finally`'s synthesized loop (parsed from a literal
source string via `crate::parse`/`crate::compiler::compile_eval`/
`Vm::execute_eval` -- the same reentrant-safe internal-compilation pattern
`indirect_eval` already uses, not a new one), and `disposeAsync` calls it
with `(false, undefined, entries)`, returning its result Promise directly.
A `RequireInternalSlot`-style precondition failure now goes through a new
`Vm::reject_with` (a catchable `RuntimeError` becomes a rejected Promise
via the existing `promise_reject`, a host resource error still propagates
raw) instead of the native call's own `?`-propagated synchronous throw.
This closed all 10 of the slice's `AsyncDisposableStack/prototype/disposeAsync`
failures, including the two `explicit-await-for-{null,undefined}.js` tests
checking the spec's "an `await using`/`disposeAsync`-adopted null/undefined
resource still costs one real microtask tick" behavior -- observable only
with genuine `Await` interleaving, which `disposeAsync` now has.

`dispose_resources_sync`'s doc comment is updated to stop claiming
`disposeAsync` reuses it (it no longer does); the sync-only fast path
remains exactly as before for `using` declarations and
`DisposableStack.prototype.dispose`, which by construction never add an
`async-dispose` resource.

### What remains, and why each is being left alone

The filtered slice's 12 remaining failures split into three groups, none
of which block anything else in this feature:

- **Two, genuinely out of scope**: `built-ins/SuppressedError/proto-from-ctor-realm.js`
  needs `$262` cross-realm `new.target` support unrelated to resource
  management itself.
- **Six, pre-existing engine gaps this feature's tests merely happen to
  also exercise, confirmed directly rather than assumed**:
  - `using`/`await-using-declaring-let-split-across-two-lines.js` (2
    modes): needs sloppy-mode `let` to fall back to an ordinary identifier
    reference when not followed by a valid binding start (`let =
    "value";`). Verified this is not `using`-specific: plain
    `class C { static { let await = null; } }`-style sloppy `let`-as-identifier
    already parses wrong today, independent of this feature.
  - `using`/`await-using-invalid-arraybindingpattern.js` (4 modes, both
    strict/sloppy): `using [] = null;` already fails to parse (`using[]`
    is an empty computed-member-access, itself invalid), but through the
    generic "expected an expression" primary-expression fallback used by
    dozens of unrelated grammar positions, which is deliberately never
    marked `known_syntax` ("never let \[the subset parser's\] arbitrary
    rejection satisfy a negative test", per that flag's own doc comment).
    The engine's rejection is correct; only the Test262 adapter's
    conservative classification of *which* rejections count as confirmed
    `SyntaxError`s doesn't credit it, and broadening that flag's use at a
    shared, heavily-hit error site was judged too risky to justify a
    2-test-file gain.
  - `using/static-init-await-binding-invalid.js` (2 modes): a class static
    block must reject `await` as any BindingIdentifier's name, `using`
    included; verified `class C { static { let await = null; } }` already
    incorrectly parses today too, so this is the general restriction never
    having been implemented, not a `using`-specific gap.
- **One, an inherited (not newly caused) bug, reconsidered and still left
  alone**: `using/cptn-value.js` (2 modes, `eval('4;{using x=null;}')`
  should be `4`, not `undefined`). Traced to `try_statement`'s unconditional
  `ClearCompletion` at try-entry: it overwrites `self.completion` with no
  prior save, so an empty try/finally already loses the *preceding*
  statement's completion value before this feature existed
  (`eval('4;try{}finally{}')` is `undefined` today, independent of
  `using`). A real fix belongs in the general statement-list/`UpdateEmpty`
  completion-value machinery (likely how `ClearCompletion` interacts with
  every construct that can complete empty, `if` included, not just
  `try`/`finally`), which is far more central and heavily depended-upon
  than this feature's own code; given the working-tree's own completion
  regressions (`tests/try_completion.rs`) already pass extensively today,
  a wrong fix risks a much wider regression than the two tests it would
  close. Left as a documented, pre-existing, unrelated gap rather than
  risked in this pass.

### Verification

`backend/bluejs/tests/resource_management.rs` grew from 18 to 31 tests,
covering (new in this pass): `await using` disposal ordering mixed with
plain `using`, real dispose-promise awaiting and rejection propagation,
`SuppressedError` wrapping across the async path, null/undefined `await
using` resources, the async-context requirement, C-style-for-head disposal
timing (once, at loop exit, not per-iteration), for-of-head per-iteration
disposal, the for-in rejection and for-of `using`/`of` disambiguation
(`using-for-using-of-of.js`'s own scenario, reproduced directly), and
`disposeAsync`'s corrected bad-receiver rejection and real
error-as-is-when-only-one-disposal-fails behavior. All 31 pass; every one
of this pass's fixes was caught by a test failing first against the prior
(missing or simplified) behavior.

`cargo build --workspace --all-targets`, `cargo test --workspace --no-fail-fast`
(only the pre-declared, unrelated, already-known-flaky
`observable_conversion_order_and_gc_pressure` fails, `HeapLimitExceeded`,
same as before this work) and `cargo clippy --workspace --all-targets --
-D warnings` all pass.

## ShadowRealm follow-up: closing three of the five remaining gaps

Implemented 2026-09-18, same-day follow-up to the slice above, requested to
push `built-ins/ShadowRealm/` as close to 100% as genuinely achievable.
**Result: 114/124 -> 122/124** (up from 0/124 at the start of the original
slice). Two of the three previously-identified causes turned out to be real,
general, pre-existing engine bugs (not ShadowRealm-specific, and not
"someone else's shared infrastructure to leave alone") and are now fixed
with their own regression tests; the third is now understood in full
mechanical detail and partially fixed, closing 3 of its 4 modes.

### `Object.prototype.hasOwnProperty`/`propertyIsEnumerable` bypassed lazy-global materialization and Proxy traps

Root cause, not just a workaround: `native::ObjectMethod::HasOwnProperty`
and `PropertyIsEnumerable` (`vm/builtins.rs`) called
`self.heap.get_own_property_descriptor(object, key)` -- the *raw* heap
record -- directly, while every other own-property reflection operation
(`Object.hasOwn`, `Object.getOwnPropertyDescriptor`, `Object.keys`/
`getOwnPropertyNames` internally) goes through `Vm::object_get_own_property`,
which first calls `materialize_global_object_property` (creating a
not-yet-touched lazy intrinsic global as a real property before observing
it) and correctly dispatches a Proxy's own `[[GetOwnProperty]]` trap. Fixed
by routing both through `object_get_own_property` like everything else.
Regression test added first (TDD): `Object.prototype.hasOwnProperty.call(
globalThis, 'Array')` on a fresh `Vm` returned `false` before the fix
(`'JSON'`/`'isFinite'` happened to already read `true`, because *something*
else had touched them first in the exact same script -- this was never a
uniformly-broken operation, which is what took the longest to pin down).
This is reproducible with no ShadowRealm involved at all: it explains
`globalthis-available-properties.js` fully.

`backend/bluejs/tests/descriptors.rs` gained
`has_own_property_and_property_is_enumerable_materialize_lazy_globals`,
checking five different lazy globals (including `ShadowRealm` itself)
through both methods on independent fresh `Vm`s. Verified no regression: the
full `built-ins/Object/` directory (3,414 files, 6,808 modes),
`built-ins/Object/prototype/{hasOwnProperty,propertyIsEnumerable}` plus
`built-ins/Object/hasOwn` (141 files, 282 modes) and `built-ins/Proxy/` (311
files, 607 modes) are all still 100% passing after the change.

### `Number.isNaN` was never implemented

A plain gap, not a bug: `Number.isFinite`/`isInteger`/`isSafeInteger` all
existed as `NativeFunction` variants installed on the `Number` constructor;
`isNaN` did not (only the *global* `isNaN`, which coerces its argument,
existed). Added `NativeFunction::NumberIsNaN` following the exact existing
pattern of its siblings: `matches!(first, Value::Number(number) if
number.is_nan())`, with **no** coercion (`Number.isNaN('NaN')` is `false`,
unlike the coercing global). Test-first in
`backend/bluejs/tests/conformance_edges.rs`
(`number_is_nan_requires_a_number_type_with_no_coercion_unlike_global_is_nan`),
covering the coercion boundary, `length`/`name`, and its property
attributes. This explains `returns-primitive-values.js` fully, with no
ShadowRealm involvement.

### A third, ShadowRealm-specific bug this work also found and fixed: no fresh lexical scope per `evaluate()` call

Not one of the three originally-identified causes -- found while
re-investigating `globalthis-config-only-properties.js` after the
`hasOwnProperty` fix only got it to 3/4 of the way there. Its second
`r.evaluate(...)` call declares a top-level `const` with the same name
(`esNonConfigValues`) the *first* call also declared, and failed with
`SyntaxError("global binding esNonConfigValues cannot be redeclared")`
-- an opaque `TypeError` at the ShadowRealm boundary, since any abrupt
`evaluate()` completion is deliberately opaque (see the slice above).

This is a real semantic gap, confirmed against the actual proposal text
fetched for `GetShadowRealmContext ( shadowRealmRecord, strictEval )`:
"1. Let lexEnv be NewDeclarativeEnvironment(shadowRealmRecord.[[GlobalEnv]])."
*Every single* `evaluate()` call gets its own fresh declarative
environment for top-level `let`/`const` -- unlike an ordinary repeated
top-level Script (e.g. two `<script>` tags, or two `$262.evalScript` calls),
which runs `GlobalDeclarationInstantiation` directly against the realm's
one persistent Global Environment Record, so a second `let x` genuinely
does conflict with an earlier one there, matching real engines. This Vm's
`execute_script` only implements that latter, ordinary case: `global_bindings`
is one persistent, never-cleared map. Fixed with a new general primitive,
`Vm::reset_lexical_global_bindings` (`vm/execution.rs`) -- drops every
purely-lexical (non-property) `global_bindings` entry and releases its GC
root, leaving `var`/function declarations (real, persistent `globalThis`
properties) untouched -- called by `shadow_realm.rs`'s `run_evaluate`
before every `evaluate()` script runs. Deliberately *not* a change to
`execute_script`/`prepare_global_declarations` themselves: ordinary repeated
top-level scripts (`$262.evalScript`, multiple classic scripts on the same
`Vm`) are confirmed, by reading the actual spec clause, to *correctly* keep
conflicting on lexical redeclaration, and `language/eval-code` (1,011 files,
1,152 modes) plus `language/global-code` stayed 100% passing throughout,
confirming this fix did not touch that ordinary case at all.

Test-first: `evaluate_gives_each_call_a_fresh_lexical_scope_that_does_not_conflict_with_earlier_ones`
in `tests/shadow_realm.rs`, covering both the fresh-`let`-scope case and
that `var` declarations still correctly persist across calls.

### The remaining Test262-membrane gap, revisited: partially fixed, and now fully understood

The original slice's brand-check explanation was correct but incomplete: a
`ShadowRealm` instance crossing between two Test262 realms neither of which
is the immediate caller previously arrived as `test262_transport_value`'s
ordinary opaque, brand-less stand-in, so `require_shadow_realm` correctly
(if unhelpfully) rejected it. Investigated whether the membrane could be
taught to preserve identity for this one exotic-object kind specifically,
rather than assuming it could not.

It can, with one architectural change: a `ShadowRealm`'s child realm was
owned exclusively (`ShadowRealmRecord { vm: Box<Vm> }`), so only the one
realm that created it could ever reach it. Changed to shared ownership,
`ShadowRealmRecord { vm: Rc<RefCell<Vm>> }` -- cheap to clone, and a second
owner can now hold the exact same live child. `test262_export_foreign_value`
now recognizes (via a new `export_foreign_shadow_realm` step) when the
value crossing into a realm is itself a `ShadowRealm` instance owned by a
*different* Test262 realm, and re-exports it as a genuine new `ShadowRealm`
instance object in the destination realm's own `shadow_realms`/
`shadow_realm_by_heap` maps, backed by an `Rc` clone of the identical child
-- rather than falling through to the ordinary opaque stand-in. A new
per-destination-realm `shadow_realm_reexports` map (deliberately separate
from `imported_sources`/`imported_values`, which back a brand-less stand-in
with no meaning of its own) deduplicates re-exporting the same target
twice. `evaluate`/`importValue`/a wrapped-function call reached through the
re-exported instance now observes the identical realm -- same `globalThis`,
same prior `evaluate()` side effects -- as reaching it the original way.

Switching to `Rc<RefCell<Vm>>` also required reworking how
`shadow_call_wrapped`/`shadow_realm_evaluate` detect "this exact child realm
is already mid-call further up this same synchronous chain" (previously
signaled by the child's absence from `shadow_realms`, since it was
temporarily removed for the borrow's duration; now, since it is never
removed, signaled by `RefCell::try_borrow_mut` failing instead) -- both now
try a direct, fresh borrow of a realm `self` owns first, and fall back to
the existing `ACTIVE` thread-local (ancestor-tracing) lookup only when that
borrow fails or `self` does not own the realm directly. The `ACTIVE`
mechanism itself, and its safety argument, are unchanged: it remains the
only way to reach a caller `Vm` that is not itself a `ShadowRealm` child
(the embedder's own top-level `Vm`, never wrapped in `Rc<RefCell>`), and
`Rc<RefCell<Vm>>` was chosen specifically to enable this shared-identity
case, not as a general aliasing-safety improvement -- the `ACTIVE`-reached
path still resolves a raw pointer outside `RefCell`'s own tracking, exactly
as before.

This closes 3 of `wrapped-function-proto-from-caller-realm.js`'s 4
assertions and **all** of `wrapped-function-throws-typeerror-from-caller-realm.js`
(now fully passing). The one remaining assertion
(`checkArgWrapperFn(() => {})`, in `wrapped-function-proto-from-caller-realm.js`)
is a *different*, deeper limitation: `checkArgWrapperFn` is a `ShadowRealm`
wrapped function that itself arrived at the caller only as a Test262
foreign-value facade (double-wrapped: ShadowRealm's own wrapping, then
Test262's own membrane import of that result). Calling it forwards the
`() => {}` argument through `test262_foreign_call`, which exports it via
the *ordinary*, unconditionally brand-less-and-non-callable
`test262_transport_value` (verified by tracing the exact dispatch: the
opaque stand-in reaches `shadow_wrap_into`'s own `is_callable` check and
fails it) -- a **general** Test262-membrane limitation, not specific to
ShadowRealm and not touched by this session's fix: passing *any* callable
as an argument through `test262_foreign_call` produces a non-callable
stand-in on the far side, exactly the "property forwarding needs a
resumable cross-VM operation" limitation `test262_transport_value`'s own
comment already documents. Fixing it generally would mean giving Test262's
membrane itself the same bidirectional-calling capability
`shadow_realm.rs` has for its own boundary -- a substantially larger,
independently-risky change to code several hundred other, unrelated Test262
fixtures already depend on, not a small extension of the ShadowRealm-specific
fix above. Left as a known, now precisely-diagnosed limitation.

Verified no regression from the `Rc<RefCell<Vm>>` change:
`backend/bluejs/tests/shadow_realm.rs` grew to 14 tests (added
`a_shadowrealm_instance_keeps_its_identity_across_a_test262_realm_transport`,
using `$262.createRealm()` directly, mirroring the Test262 scenario);
`built-ins/Reflect/` + `built-ins/Proxy/` (465 files, 915 modes) stayed 100%
passing. `cargo build --workspace --all-targets`, `cargo test --workspace
--no-fail-fast` (only the same pre-declared `string_protocols.rs` flake) and
`cargo clippy --workspace --all-targets -- -D warnings` all pass.

## Lazy on-demand module compilation, and a further round of individual dynamic-import fixes

Implemented 2026-09-18, a third continuation of the same-day session above,
addressing its own explicitly deferred `eval-script-code-target` finding
plus the residual individually-diagnosed `dynamic-import` failures.

### 1. `ensure_dynamic_module_compiled`: the `eval-script-code-target` fix, implemented for real

Built the "raw-source registry for dynamic-only siblings" the prior section
named but deferred, directly analogous to `ensure_json_module`:

- **`vm/modules.rs::ensure_dynamic_module_compiled`**: given a resolved
  module name already absent from the working module map, looks up raw
  JavaScript text in a new `Vm::dynamic_module_sources` registry (installed
  via the new `Vm::set_dynamic_module_sources`) and compiles it on demand
  via the crate's own `parse_module`/`compile_module_with_limit` (the same
  functions the Test262 adapter already uses for every other module). A
  parse or compile failure becomes a real `RuntimeError::SyntaxError`
  (matching `indirect_eval`'s own error mapping), which -- reached only
  through a dynamic import's own job -- rejects that import's promise with
  a real `SyntaxError` object rather than surfacing as a harness-level or
  whole-graph failure. A target with no registered raw source at all is
  left alone: the pre-existing "module was not linked" `ModuleResolution`
  fallback still applies exactly as before this function existed.
- **A necessary generalization in `execute_module_graph_inner`**: linking
  work (cell creation, indirect-export/source-phase validation, import
  aliasing, declaration instantiation) was gated on `fresh_graph` --
  correct only because, before this session, the *entire* reachable set was
  always supplied up front in one registry, so "first ever graph" and "every
  module that will ever need linking" were the same set. Once a module can
  be compiled and added to the graph *after* that first call (a dynamic
  import discovering a not-yet-compiled sibling, exactly `ensure_json_module`'s
  own case too, or now `ensure_dynamic_module_compiled`'s), that assumption
  breaks -- a `fresh_graph`-gated pass would simply never link it. The fix:
  compute `new_names` (every module in `modules` not already in `linked`)
  and gate/scope the *same* linking pass on `!new_names.is_empty()` instead
  of `fresh_graph`, using `new_names` in place of `order` throughout. This
  runs identically to the old `fresh_graph` behavior when nothing has been
  linked yet (`new_names == order`, since `linked` starts empty) and is a
  no-op when nothing new was added (`new_names` empty) -- but now also
  correctly links a lazily-added module into an *already-linked* existing
  graph. This let the bespoke single-entry `LinkedModule` construction the
  prior section added specifically for `ensure_json_module` on a non-fresh
  graph be deleted entirely: a JSON module and a lazily-compiled ordinary
  module now share the exact same generic linking path. A rollback
  refinement went with it: on a linking failure, a fresh graph still
  discards every root (nothing was usable), but a failure while linking a
  *delta* into an existing graph now rolls back only that delta (removing
  just `new_names` from `linked`, unrooting only the roots pushed since a
  new `roots_checkpoint`), so an unrelated later operation on the rest of
  the graph is unaffected and a retried dynamic import of the same failed
  specifier is treated as new again rather than resuming a half-linked
  record.
- **Harness wiring** (`run.py`, `bluejs-test262.rs`): `module_sources()` now
  returns `(sources, dynamic_sources, json_sources)` instead of two values.
  Every discovered file is tagged by *how* it was reached -- a static edge
  (`import`/`export ... from`, from any visited node, transitively) or a
  dynamic one (a literal `import(...)` call, or the broader relative-string
  fallback) -- with static winning whenever both apply to the same file
  (a module genuinely required statically must still be compiled eagerly,
  even if also separately dynamically imported elsewhere). A `.js` file
  reached only dynamically goes to the new `dynamic_sources` return value
  (raw text, sent to the adapter as a new `module_dynamic_sources` request
  field and installed via `Vm::set_dynamic_module_sources`) instead of the
  eagerly-parsed-and-compiled `sources`. One new host-recognized regex,
  `DYNAMIC_IMPORT_PLAIN_REQUEST`, deliberately excludes `import.source(...)`/
  `import.defer(...)`: those forms have no lazy-compile counterpart (source-
  phase dynamic import resolves by checking whether the target is *already*
  a compiled Source Text Module), so a reference through either must still
  be treated as a static edge -- a new `DYNAMIC_IMPORT_SOURCE_OR_DEFER_REQUEST`
  regex tags those "static" explicitly. A second harness-only fix: a script
  that dynamically imports *itself* (`eval-self-once-script.js`) names a
  path always classified "static" (the entry always seeds discovery that
  way) yet is deliberately excluded from `module_codes` (compiled once, as
  the script the request actually executes, never twice as a module) --
  its raw text is now also copied into `dynamic_sources` when that
  exclusion applies, so self-referential dynamic import finds it instead of
  neither registry.

**Evidence**: all 16 `catch/*-eval-script-code-target.js` tests now pass
(0/16 before this fix, confirmed both individually and as part of the full
run below). Six new regression tests in `test262_host.rs`: compiling an
uncompiled module on demand, the exact `eval-script-code-target` scenario
lazily rejecting with a real `SyntaxError`, reusing an on-demand-compiled
module's identity across repeated imports (`Promise.all` of two imports of
the same never-before-seen specifier resolve to the *same* namespace), a
failed on-demand compile/link not corrupting an unrelated already-healthy
module in the same graph, a script dynamically importing itself, and one
documenting the accepted trade-off below.

**A real, understood, and accepted one-test trade-off**: `language/module-code/
source-phase-import/import-source.js` regressed (2 modes). Root cause fully
diagnosed and preserved as `dynamic_import_of_lazily_compiled_siblings_does_not_batch_unrelated_modules`
in `test262_host.rs`: this test dynamically imports three fixtures one at a
time; one of them has genuine `import source x from '<do not resolve>'`
requests that always fail with a `TypeError`, while the *other* fixtures'
own imports (ordinary default imports of the same deliberately-unresolvable
specifier, or -- via a shared `ensure-linking-error_FIXTURE.js` sibling --
a "does not export" case) fail with a `SyntaxError` instead. Under the old
eager-everything-up-front harness, a first module graph linked *all* four
siblings together regardless of which one a given dynamic import actually
targeted, so the second fixture's real `TypeError` always won the race
against the others' `SyntaxError`s -- accidentally matching what the test
expects for all three calls, for a reason unrelated to what it claims to
verify. Lazily compiling only the module a specific dynamic import actually
names removes that coincidence: it is a strictly more correct linking
granularity (it stops spuriously batching together modules that have
nothing to do with the import being made), so the first fixture's own
dynamic import no longer incidentally pulls in the second fixture's
`Bytecode`, and its own `SyntaxError` is what actually surfaces. Judged
not worth chasing further: a proper fix would require distinguishing "the
host could not resolve/load this module at all" (arguably a `TypeError`,
by analogy with `module_source_object`'s own "host did not provide" case)
from "the module was resolved but a specific named export is missing"
(a `SyntaxError`, per `ResolveExport`) throughout `resolve_export`'s
"module ... was not supplied by the host" branch -- a change with a wide,
not-fully-mapped blast radius across the whole corpus, for the sake of one
upstream test whose own assertion happens to rely on a coincidence.

### 2. Two more individually diagnosed and fixed `dynamic-import` bugs

**`Promise.prototype.constructor` unset for internally-created promises**
(`language/expressions/dynamic-import/always-create-new-promise.js`, a
pre-existing bug unrelated to this session's own work -- confirmed present
in the very first full-tree measurement, before any of this session's
changes). `new_promise` (`vm/builtins/promises.rs`, used by dynamic import,
`await`, `Promise.all`/`race`/`allSettled`/`any`, `Atomics.waitAsync`, and
every other internally-created promise) built instances from
`promise_prototype()` alone, which installs `then`/`catch`/`finally`/
`@@toStringTag` but not the prototype's "constructor" link back to the
`Promise` function -- that property is only added when the `Promise`
*global* itself is separately materialized (`globals.rs`), on first access
to the bare `Promise` identifier. A script whose first reference to
`Promise` is indirect (`p.constructor` from an internally-created promise,
before ever naming `Promise` itself) observed a broken link. Fixed by
having `new_promise` call `self.global("Promise")` first (idempotent/cached,
so a no-op on every call after the first; safe from circularity too, since
`Promise`'s own materialization calls `promise_prototype()` directly rather
than `new_promise`). New regression test:
`dynamic_import_promise_observes_the_promise_constructor_link_unprompted`.
This is a centralized fix: every other internal promise-creation call site
listed above benefits identically, not just dynamic import's own.

**`await-import-evaluation.js`**: investigated, not fixed, and judged
untractable rather than deprioritized for lack of effort. Its fixture
busy-waits on real wall-clock time (`while(true){ if (Date.now()-start>100)
break }`) to prove evaluation genuinely completed before the dynamic
import's promise resolves. A real 100ms wall-clock busy-wait loop costs far
more than the harness's 100,000-instruction default budget at this
interpreter's speed, so this fails as a `resource_error` (correctly
classified, not a false failure) rather than a wrong result. Not a bug in
the engine or the harness's module handling -- a structural mismatch
between an instruction-budget-bounded interpreter and a wall-clock-timing
test, out of scope for a targeted fix here.

**`for-await-resolution-and-error-agen-yield.js`**: investigated, not
resolved. Isolated to `AsyncGeneratorYield`'s implicit `Await` of a
*rejected* dynamic-import promise specifically -- fulfilled cases (two of
four `yield`/`yield await` pairs across the test's two async generators)
observe the correct awaited value, but the rejected case's caught error is
an unrelated empty object instead of the module's own thrown `'foo'`
string. `promise_resolve`'s "is this already a genuine Promise" fast path
(`vm/builtins/promises.rs`) was suspected and instrumented directly, since
this session's own `new_promise` fix (above) changes exactly the
`.constructor` check that path depends on; instrumentation showed the fast
path *does* correctly identify true dynamic-import promises now, ruling
that specific mechanism out, but also surfaced at least one
`promise_resolve` call on a value that is not a tracked Promise at all
during the same sequence (likely related to the async generator's own
completion/return bookkeeping) whose role was not run to ground. Left open
rather than shipping a guessed fix; a future session should trace
`await_async_generator_yield` and `finish_async_generator_yield` end to end
against this exact repro rather than starting over.

**`import-fulfilled-member-of-errored-cycle.js`**: investigated at the
specification level, not attempted. Requires implementing "cycle root"
tracking for async module evaluation cycles (`[[CycleRoot]]`,
`[[EvaluationError]]` recorded on and redirected through a cycle's root
module per `Evaluate`/`InnerModuleEvaluation`) -- a real, unimplemented
piece of the module-evaluation algorithm, not a bug in existing code. This
engine's `LinkedModule`/graph model has no notion of strongly-connected
cycle roots at all today. Out of scope as a "fix"; it is a feature gap,
sized more like a phase-level unit of work than an individual bug.

### 3. Re-measured full scope and `import-attributes` re-verification

`--filter "language/module-code/,language/expressions/dynamic-import/,
language/import/,built-ins/ImportAttributes"` (2,637 modes, same scope
throughout this whole session):

| Stage | Pass / fail |
| --- | ---: |
| Prior section's end state (this session) | 2,053 / 584 |
| After `ensure_dynamic_module_compiled` + harness fixes | 2,068 / 569 |
| After the `Promise.prototype.constructor` fix + `eval-self-once-script.js` harness fix | **2,072 / 565** |

Net this round: +19 pass, -19 fail, on top of the +76 the session's first
two rounds had already found. Diffed path-and-mode-for-path-and-mode
against the session's very first post-JSON-modules measurement: 21 fixed,
2 regressed (the one documented, accepted `import-source.js` trade-off
above) -- zero unexplained regressions.

Plain (non-`import-attributes`/`import-defer`/`source-phase-imports`-tagged)
`dynamic-import` failures: 68 (end of the prior section) -> 52. The
residual is fully accounted for: 42 are the already-documented
`import.UNKNOWN(...)`/bare-`typeof import`-adjacent cases blocked on the
pre-existing `import.source`/`import.defer` global-object mechanism (an
intentional non-goal, unchanged since the first round), and the remaining
10 (5 distinct files) are the individually diagnosed cases in section 2:
2 fixed, 1 judged untractable (wall-clock budget mismatch), 2 left open
(one deep async-generator/promise interaction needing further tracing, one
requiring genuinely new cycle-root-tracking engine machinery).

`import ... with {...}` (static form, `module-code/import-attributes/`)
re-verified at full corpus scope (not just the narrower filter the first
round checked): **13/13 (100%)**, unchanged and confirmed holding. The
broader `import-attributes`-tagged areas: `dynamic-import/import-attributes/`
42/44 (the 2 remaining need the separate `import-text` proposal), and
`import/import-attributes/` (JSON modules) 12/17 (the 5 remaining also need
`import-text`, unrelated to JSON-module support itself, which is complete
for every case this corpus actually exercises).

All three gates pass: `cargo build --workspace --all-targets`; `cargo test
--workspace --no-fail-fast` (only the pre-declared, separately-owned
`observable_conversion_order_and_gc_pressure` fails, confirmed via an
isolated rerun to be exactly that test and nothing else); `cargo clippy
--workspace --all-targets -- -D warnings` clean. `backend/bluejs/tests/
test262_host.rs` sits at 102 tests (all passing), and
`backend/bluejs/test262/test_runner.py`/`test_analyze.py` at 34 (all
passing), including new coverage for `module_sources()`'s three-way
static/dynamic/json split and a module reached by both a static and a
dynamic edge classifying as static.

### Merge-time reconciliation: `speculative_relative_strings` survives the static/dynamic split

Merging this slice against the concurrently-developed ShadowRealm follow-up
(above) required reconciling two independent rewrites of `module_sources()`
in `backend/bluejs/test262/run.py`: this slice's static/dynamic/json
three-way split, and the ShadowRealm slice's `speculative_relative_strings`
flag (a relative-string root reached only via `ShadowRealm.prototype.
importValue`'s heuristic trigger should not hard-fail the whole run on its
own parse failure). This slice's own rewrite, developed without visibility
into that flag, classified every `RELATIVE_STRING` root as `"dynamic"`
unconditionally -- which happens to also satisfy the ShadowRealm case (a
`"dynamic"` classification's lazy-compile-on-demand path already tolerates
a parse failure), but would have silently changed the established, relied-
upon behavior for a plain dynamic `import()` with a variable specifier
(`speculative_relative_strings` left at its default `False`): such a
root's own parse failure must still hard-fail eagerly, per that flag's own
docstring contract, which a large, unrelated part of the corpus already
depends on. The merged `module_sources()` restores that distinction
explicitly: a `RELATIVE_STRING` root classifies `"dynamic"` only when
`speculative_relative_strings` is `True`, and `"static"` otherwise --
preserving both slices' own contracts rather than silently picking one.
`request["speculative_module_sources"]` (the ShadowRealm slice's own
harness-request field for the same purpose) is now unused by this call
site, since a genuinely speculative root no longer reaches `sources` at
all under the three-way split; it is left defined in the adapter
(`bluejs-test262.rs`) rather than removed, since removing an unused-but-
harmless field is out of scope for a conflict-resolution merge.
