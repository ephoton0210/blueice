# Test262 architecture triage

Snapshot: `6eec1ac9ee144dafd8f344d73a21f36bfc9f6755`.

Reconciled 53,404 files / 102,578 modes; complete inventory: True.

Targets are inferred from paths/metadata; blockers are first observed symptoms, not proven root causes. Feature/dependency counts overlap. Target counts are exclusive and reconcile to all modes. Priorities are dependency order, not failure-count order. Passed negatives remain passes; no outcomes are excluded.

| Order | Target | Pass | Fail | Unsupported | Timeout | Harness error | Prerequisites |
| --- | --- | ---: | ---: | ---: | ---: | ---: | --- |
| P0.1 | completion: Completion records, iterator lifetime, catch/finally, control transfer | 8843 | 816 | 0 | 0 | 0 | — |
| P0.2 | environments: Persistent realms, bindings, parameter environments, arguments, eval | 3555 | 392 | 0 | 0 | 0 | completion |
| P0.3 | references: Reference evaluation, coercion and observable evaluation order | 5725 | 358 | 0 | 0 | 0 | completion, environments |
| P0.4 | objects: Internal methods, descriptors, receiver and callable/constructor contracts | 6269 | 1530 | 0 | 0 | 0 | references |
| P1.1 | grammar: Source grammar and classified strict/early errors | 3263 | 1052 | 0 | 0 | 0 | environments, references |
| P1.2 | classes: Classes, private slots, super and derived construction | 14421 | 2701 | 0 | 0 | 0 | objects, grammar |
| P1.3 | suspension: Resumable frames, generators, async functions and promise jobs | 3915 | 1160 | 0 | 0 | 0 | completion, environments, objects, grammar |
| P1.4 | modules: Module linking, live bindings, evaluation and dynamic import | 1661 | 1002 | 0 | 0 | 0 | environments, suspension, grammar |
| P1.5 | storage: BigInt, buffers, typed arrays, shared memory and GC weak slots | 2093 | 5558 | 0 | 0 | 0 | objects |
| P1.6 | host: Test262 realm, agent, GC, buffer and async host hooks | 180 | 52 | 0 | 0 | 0 | environments, suspension, modules, storage |
| P2.1 | library: Remaining standard builtin algorithms and descriptors | 11049 | 18210 | 0 | 0 | 0 | completion, references, objects |
| P2.2 | intl: ECMA-402 constructors, algorithms and locale data | 498 | 6184 | 0 | 0 | 0 | library |
| P3.1 | review: Unmapped/staging targets requiring specification and applicability review | 951 | 1140 | 0 | 0 | 0 | — |

## Observed blockers

| Symptom | Modes | Representative test / mode |
| --- | ---: | --- |
| none | 62423 | `—` |
| unresolved-name | 18158 | `annexB/built-ins/Date/prototype/getYear/B.2.4.js [sloppy]` |
| exception:TypeError | 10716 | `annexB/built-ins/Object/is/emulates-undefined.js [sloppy]` |
| assertion | 6376 | `annexB/built-ins/RegExp/legacy-accessors/index/this-cross-realm-constructor.js [sloppy]` |
| exception:SyntaxError | 2680 | `annexB/built-ins/Function/createdynfn-html-close-comment-params.js [sloppy]` |
| missing-expected-error | 1139 | `annexB/language/expressions/template-literal/legacy-octal-escape-sequence-strict.js [strict]` |
| unclassified-parse | 791 | `language/asi/S7.9.2_A1_T3.js [sloppy]` |
| exception:ThrownValue | 158 | `built-ins/Promise/all/capability-resolve-throws-reject.js [sloppy]` |
| exception:RangeError | 57 | `built-ins/Object/defineProperties/15.2.3.7-6-a-122.js [sloppy]` |
| exception:crash | 49 | `annexB/language/statements/labeled/function-declaration.js [sloppy]` |
| resource-limit | 23 | `language/types/number/S8.5_A10_T2.js [sloppy]` |
| exception:Error | 8 | `built-ins/Promise/exception-after-resolve-in-executor.js [sloppy]` |

## Frequent diagnostics

Exact per-mode evidence, source hashes, esid, includes and dependency labels are in `items.jsonl`.

| Diagnostic (numeric details normalized) | Modes |
| --- | ---: |
| ReferenceError: Temporal is not defined | 11920 |
| TypeError: value is not callable | 6270 |
| TypeError: cannot access a property of null or undefined | 2738 |
| Test262Error: throws failed | 2714 |
| Test262Error: sameValue failed | 1844 |
| ReferenceError: BigInt64Array is not defined | 1777 |
| ReferenceError: Date is not defined | 1647 |
| uncaught JavaScript value: Object(ObjectId { heap: #, serial: # }) | 1474 |
| ok | 1139 |
| ReferenceError: Iterator is not defined | 898 |
| expected a property key (found Invalid("unicode escape does not form a valid private identifier character")) | 884 |
| expected a property key (found Invalid("private identifier requires a name")) | 830 |
| TypeError: property helper requires an object | 574 |
| ReferenceError: SharedArrayBuffer is not defined | 490 |
| Test262Error: verifyProperty failed | 331 |
| Test262Error: verifyWritable failed | 286 |
| Test262Error: isConstructor failed | 282 |
| ReferenceError: Atomics is not defined | 212 |
| ReferenceError: WeakMap is not defined | 195 |
| ReferenceError: DisposableStack is not defined | 188 |
| expected RParen (found Punct(Comma)) | 171 |
| Test262Error: assert failed | 158 |
| expected a property key (found Invalid("unicode escape does not form a valid identifier character")) | 150 |
| uncaught JavaScript value: String(JsString([#, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #, #,  | 148 |
| ReferenceError: AsyncDisposableStack is not defined | 146 |
| expected ';' (found Identifier("x")) | 139 |
| ReferenceError: ShadowRealm is not defined | 118 |
| TypeError: Number conversion requires a non-Symbol primitive | 116 |
| ReferenceError: WeakSet is not defined | 104 |
| default import requires 'from' or ',' (found Punct(Star)) | 94 |

## Measurement limits

A passing result is the runner's observation, not proof of complete feature conformance. Unclassified parser rejections cannot establish required negative early errors. Existing adapter records may not distinguish a runtime exception in an include from one in the test body. Missing APIs, assertion mismatches and timeouts require reproduction before assigning a confirmed engine root cause. Staging, Annex B, Intl and host-dependent cases remain visible pending applicability audit.
