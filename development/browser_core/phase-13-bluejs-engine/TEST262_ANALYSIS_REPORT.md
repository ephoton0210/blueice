# Test262 architecture triage

Snapshot: `6eec1ac9ee144dafd8f344d73a21f36bfc9f6755`.

Reconciled 53,404 files / 102,578 modes; complete inventory: True.

Targets are inferred from paths/metadata; blockers are first observed symptoms, not proven root causes. Feature/dependency counts overlap. Target counts are exclusive and reconcile to all modes. Priorities are dependency order, not failure-count order. Passed negatives remain passes; no outcomes are excluded.

| Order | Target | Pass | Fail | Unsupported | Timeout | Harness error | Prerequisites |
| --- | --- | ---: | ---: | ---: | ---: | ---: | --- |
| P0.1 | completion: Completion records, iterator lifetime, catch/finally, control transfer | 8930 | 729 | 0 | 0 | 0 | — |
| P0.2 | environments: Persistent realms, bindings, parameter environments, arguments, eval | 3724 | 194 | 0 | 29 | 0 | completion |
| P0.3 | references: Reference evaluation, coercion and observable evaluation order | 5753 | 330 | 0 | 0 | 0 | completion, environments |
| P0.4 | objects: Internal methods, descriptors, receiver and callable/constructor contracts | 7637 | 162 | 0 | 0 | 0 | references |
| P1.1 | grammar: Source grammar and classified strict/early errors | 3273 | 1042 | 0 | 0 | 0 | environments, references |
| P1.2 | classes: Classes, private slots, super and derived construction | 14559 | 2561 | 0 | 2 | 0 | objects, grammar |
| P1.3 | suspension: Resumable frames, generators, async functions and promise jobs | 3891 | 1184 | 0 | 0 | 0 | completion, environments, objects, grammar |
| P1.4 | modules: Module linking, live bindings, evaluation and dynamic import | 1667 | 996 | 0 | 0 | 0 | environments, suspension, grammar |
| P1.5 | storage: BigInt, buffers, typed arrays, shared memory and GC weak slots | 5947 | 1703 | 0 | 1 | 0 | objects |
| P1.6 | host: Test262 realm, agent, GC, buffer and async host hooks | 196 | 36 | 0 | 0 | 0 | environments, suspension, modules, storage |
| P2.1 | library: Remaining standard builtin algorithms and descriptors | 16245 | 13004 | 0 | 10 | 0 | completion, references, objects |
| P2.2 | intl: ECMA-402 constructors, algorithms and locale data | 588 | 6090 | 0 | 4 | 0 | library |
| P3.1 | review: Unmapped/staging targets requiring specification and applicability review | 1143 | 925 | 0 | 23 | 0 | — |

## Observed blockers

| Symptom | Modes | Representative test / mode |
| --- | ---: | --- |
| none | 73553 | `—` |
| unresolved-name | 13845 | `annexB/built-ins/escape/argument_bigint.js [sloppy]` |
| exception:TypeError | 5724 | `annexB/built-ins/RegExp/legacy-accessors/index/prop-desc.js [sloppy]` |
| assertion | 4435 | `annexB/built-ins/RegExp/legacy-accessors/index/this-cross-realm-constructor.js [sloppy]` |
| exception:SyntaxError | 2669 | `annexB/language/function-code/function-redeclaration-block.js [sloppy]` |
| missing-expected-error | 1139 | `annexB/language/expressions/template-literal/legacy-octal-escape-sequence-strict.js [strict]` |
| unclassified-parse | 791 | `language/asi/S7.9.2_A1_T3.js [sloppy]` |
| exception:ThrownValue | 196 | `built-ins/Promise/all/capability-resolve-throws-reject.js [sloppy]` |
| deadline | 69 | `built-ins/Array/prototype/every/15.4.4.16-7-c-ii-2.js [sloppy]` |
| exception:RangeError | 63 | `built-ins/Array/prototype/slice/S15.4.4.10_A1.1_T3.js [sloppy]` |
| exception:crash | 55 | `annexB/language/statements/labeled/function-declaration.js [sloppy]` |
| resource-limit | 29 | `language/module-code/top-level-await/fulfillment-order.js [module]` |
| exception:Error | 10 | `built-ins/AsyncGeneratorPrototype/return/return-suspendedStart-broken-promise.js [sloppy]` |

## Frequent diagnostics

Exact per-mode evidence, source hashes, esid, includes and dependency labels are in `items.jsonl`.

| Diagnostic (numeric details normalized) | Modes |
| --- | ---: |
| ReferenceError: Temporal is not defined | 12240 |
| TypeError: value is not callable | 3061 |
| Test262Error: throws failed | 2043 |
| uncaught JavaScript value: Object(ObjectId { heap: #, serial: # }) | 1678 |
| Test262Error: sameValue failed | 1152 |
| ok | 1139 |
| TypeError: cannot access a property of null or undefined | 1020 |
| ReferenceError: Iterator is not defined | 906 |
| expected a property key (found Invalid("unicode escape does not form a valid private identifier character")) | 882 |
| expected a property key (found Invalid("private identifier requires a name")) | 828 |
| TypeError: property helper requires an object | 326 |
| Test262Error: verifyProperty failed | 221 |
| ReferenceError: DisposableStack is not defined | 188 |
| expected RParen (found Punct(Comma)) | 171 |
| expected a property key (found Invalid("unicode escape does not form a valid identifier character")) | 150 |
| uncaught JavaScript value: String(JsString([#, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #,  | 148 |
| ReferenceError: AsyncDisposableStack is not defined | 146 |
| expected ';' (found Identifier("x")) | 139 |
| TypeError: Number conversion requires a non-Symbol primitive | 132 |
| ReferenceError: ShadowRealm is not defined | 118 |
| default import requires 'from' or ',' (found Punct(Star)) | 94 |
| Test262Error: compareArray failed | 86 |
| Test262Error: assert failed | 80 |
| class fields on one line require a semicolon separator (found Invalid("private identifier requires a name")) | 68 |

## Measurement limits

A passing result is the runner's observation, not proof of complete feature conformance. Unclassified parser rejections cannot establish required negative early errors. Existing adapter records may not distinguish a runtime exception in an include from one in the test body. Missing APIs, assertion mismatches and timeouts require reproduction before assigning a confirmed engine root cause. Staging, Annex B, Intl and host-dependent cases remain visible pending applicability audit.

## Complete Rust 1.95 regression

The 2026-09-13 complete rerun reconciled 53,404 files and 102,578 modes:
**73,553 pass, 28,956 fail and 69 timeout**, with zero unsupported and
harness-error modes. It took 2,160.529 seconds with eight workers, a
100,000-instruction default budget and a two-second case deadline. The runner
exited 1 because conformance failures remain; the analyzer verified every
source path, mode and result classification before publishing this report.
Async tests wait for `$DONE` when the test requests it.

## Targeted P0.4 closure audit

The final targeted realm and Proxy audit records **607/607** passing modes for
`built-ins/Proxy`, **60/60** for `built-ins/Proxy/construct`, and **20/20** for
`built-ins/Reflect/construct`. The remaining Reflect modes required only that
`Date.now` exist as a callable, non-constructor; the Date baseline supplies
that contract without claiming complete Date object support. The public
`conformance_edges` regression suite passes **37/37**.

## Focused P0.4 weak-collection follow-up

The P0.4 weak-collection implementation was checked with focused filters on
the same Test262 snapshot: `built-ins/WeakMap/` is **281/281 pass**,
`built-ins/WeakSet/` is **170/170 pass**, and `built-ins/WeakRef/` is now
**58/58 pass** at `target/test262-p04-weak-ref-final-2`.

The two previous WeakRef failures were unblocked by a non-placeholder
`FinalizationRegistry` substrate: the constructor/callback, weak target and
unregister-token cells, strong holdings, `register`, `unregister`, and dead
target collection behavior are represented in the VM. Cleanup-job scheduling
and callback delivery remain future weak-GC/host work, so this does not claim
full FinalizationRegistry conformance.

The corresponding Date audit rose from **166/1,236** to **1,220/1,236** at
`target/test262-p04-date-final-4`; all 16 remaining modes require the absent
Temporal bridge (`Date.prototype.toTemporalInstant`). The Array audit rose
from **4,846/6,119** to **5,169/6,119** at
`target/test262-p04-array-final-3`, after species-aware map/filter,
`Array.of`, `at`, iterators, and the find family. The remaining Array failures
are predominantly `Array.fromAsync`, concat spreadability, copy-by-value, and
resizable-buffer work.

These focused results remain regression evidence for their respective slices.
The complete Rust 1.95 inventory above was run after those changes and is the
current full-suite baseline; the checked-in summary and tables now use it.
