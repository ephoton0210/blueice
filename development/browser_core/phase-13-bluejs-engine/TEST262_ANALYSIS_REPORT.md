# Test262 architecture triage

Snapshot: `6eec1ac9ee144dafd8f344d73a21f36bfc9f6755`.

Reconciled 53,404 files / 102,578 modes; complete inventory: True.

Targets are inferred from paths/metadata; blockers are first observed symptoms, not proven root causes. Feature/dependency counts overlap. Target counts are exclusive and reconcile to all modes. Priorities are dependency order, not failure-count order. Passed negatives remain passes; no outcomes are excluded.

| Order | Target | Pass | Fail | Unsupported | Timeout | Harness error | Prerequisites |
| --- | --- | ---: | ---: | ---: | ---: | ---: | --- |
| P0.1 | completion: Completion records, iterator lifetime, catch/finally, control transfer | 8887 | 772 | 0 | 0 | 0 | — |
| P0.2 | environments: Persistent realms, bindings, parameter environments, arguments, eval | 3735 | 212 | 0 | 0 | 0 | completion |
| P0.3 | references: Reference evaluation, coercion and observable evaluation order | 5729 | 354 | 0 | 0 | 0 | completion, environments |
| P0.4 | objects: Internal methods, descriptors, receiver and callable/constructor contracts | 6426 | 1373 | 0 | 0 | 0 | references |
| P1.1 | grammar: Source grammar and classified strict/early errors | 3265 | 1050 | 0 | 0 | 0 | environments, references |
| P1.2 | classes: Classes, private slots, super and derived construction | 14498 | 2624 | 0 | 0 | 0 | objects, grammar |
| P1.3 | suspension: Resumable frames, generators, async functions and promise jobs | 3921 | 1154 | 0 | 0 | 0 | completion, environments, objects, grammar |
| P1.4 | modules: Module linking, live bindings, evaluation and dynamic import | 1668 | 995 | 0 | 0 | 0 | environments, suspension, grammar |
| P1.5 | storage: BigInt, buffers, typed arrays, shared memory and GC weak slots | 5476 | 2171 | 0 | 4 | 0 | objects |
| P1.6 | host: Test262 realm, agent, GC, buffer and async host hooks | 188 | 44 | 0 | 0 | 0 | environments, suspension, modules, storage |
| P2.1 | library: Remaining standard builtin algorithms and descriptors | 11425 | 17834 | 0 | 0 | 0 | completion, references, objects |
| P2.2 | intl: ECMA-402 constructors, algorithms and locale data | 518 | 6164 | 0 | 0 | 0 | library |
| P3.1 | review: Unmapped/staging targets requiring specification and applicability review | 995 | 1096 | 0 | 0 | 0 | — |

## Observed blockers

| Symptom | Modes | Representative test / mode |
| --- | ---: | --- |
| none | 66731 | `—` |
| unresolved-name | 15912 | `annexB/built-ins/Date/prototype/getYear/B.2.4.js [sloppy]` |
| exception:TypeError | 9365 | `annexB/built-ins/Object/is/emulates-undefined.js [sloppy]` |
| assertion | 5628 | `annexB/built-ins/RegExp/legacy-accessors/index/this-cross-realm-constructor.js [sloppy]` |
| exception:SyntaxError | 2677 | `annexB/language/function-code/function-redeclaration-block.js [sloppy]` |
| missing-expected-error | 1139 | `annexB/language/expressions/template-literal/legacy-octal-escape-sequence-strict.js [strict]` |
| unclassified-parse | 791 | `language/asi/S7.9.2_A1_T3.js [sloppy]` |
| exception:ThrownValue | 158 | `built-ins/Promise/all/capability-resolve-throws-reject.js [sloppy]` |
| exception:RangeError | 87 | `built-ins/Array/prototype/slice/S15.4.4.10_A1.1_T3.js [sloppy]` |
| exception:crash | 55 | `annexB/language/statements/labeled/function-declaration.js [sloppy]` |
| resource-limit | 25 | `built-ins/Object/entries/observable-operations.js [strict]` |
| exception:Error | 6 | `built-ins/Promise/exception-after-resolve-in-executor.js [sloppy]` |
| deadline | 2 | `staging/sm/TypedArray/sort_modifications.js [sloppy]` |
| instruction-limit | 2 | `staging/sm/TypedArray/sort_sorted.js [sloppy]` |

## Frequent diagnostics

Exact per-mode evidence, source hashes, esid, includes and dependency labels are in `items.jsonl`.

| Diagnostic (numeric details normalized) | Modes |
| --- | ---: |
| ReferenceError: Temporal is not defined | 12218 |
| TypeError: value is not callable | 4894 |
| TypeError: cannot access a property of null or undefined | 2733 |
| Test262Error: throws failed | 2507 |
| uncaught JavaScript value: Object(ObjectId { heap: #, serial: # }) | 1694 |
| ReferenceError: Date is not defined | 1653 |
| Test262Error: sameValue failed | 1409 |
| ok | 1139 |
| ReferenceError: Iterator is not defined | 898 |
| expected a property key (found Invalid("unicode escape does not form a valid private identifier character")) | 884 |
| expected a property key (found Invalid("private identifier requires a name")) | 830 |
| TypeError: property helper requires an object | 434 |
| Test262Error: verifyWritable failed | 286 |
| Test262Error: verifyProperty failed | 253 |
| Test262Error: isConstructor failed | 204 |
| ReferenceError: WeakMap is not defined | 199 |
| ReferenceError: DisposableStack is not defined | 188 |
| expected RParen (found Punct(Comma)) | 171 |
| expected a property key (found Invalid("unicode escape does not form a valid identifier character")) | 150 |
| uncaught JavaScript value: String(JsString([#, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #,  | 148 |
| ReferenceError: AsyncDisposableStack is not defined | 146 |
| expected ';' (found Identifier("x")) | 139 |
| TypeError: Number conversion requires a non-Symbol primitive | 138 |
| Test262Error: assert failed | 134 |
| ReferenceError: ShadowRealm is not defined | 118 |
| ReferenceError: WeakSet is not defined | 104 |
| default import requires 'from' or ',' (found Punct(Star)) | 94 |
| ReferenceError: FinalizationRegistry is not defined | 86 |
| Test262Error: compareArray failed | 84 |
| class fields on one line require a semicolon separator (found Invalid("private identifier requires a name")) | 68 |

## Measurement limits

A passing result is the runner's observation, not proof of complete feature conformance. Unclassified parser rejections cannot establish required negative early errors. Existing adapter records may not distinguish a runtime exception in an include from one in the test body. Missing APIs, assertion mismatches and timeouts require reproduction before assigning a confirmed engine root cause. Staging, Annex B, Intl and host-dependent cases remain visible pending applicability audit.
