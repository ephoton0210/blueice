# Test262 architecture triage

Snapshot: `72faf8ec1445c55149615e8b35187830783aba1a`.

Reconciled 53,582 files / 102,926 modes; complete inventory: True.

Targets are inferred from paths/metadata; blockers are first observed symptoms, not proven root causes. Feature/dependency counts overlap. Target counts are exclusive and reconcile to all modes. Priorities are dependency order, not failure-count order. Passed negatives remain passes; no outcomes are excluded.

| Order | Target | Pass | Fail | Unsupported | Timeout | Harness error | Prerequisites |
| --- | --- | ---: | ---: | ---: | ---: | ---: | --- |
| P0.1 | completion: Completion records, iterator lifetime, catch/finally, control transfer | 9049 | 610 | 0 | 0 | 0 | — |
| P0.2 | environments: Persistent realms, bindings, parameter environments, arguments, eval | 3771 | 176 | 0 | 0 | 0 | completion |
| P0.3 | references: Reference evaluation, coercion and observable evaluation order | 5818 | 265 | 0 | 0 | 0 | completion, environments |
| P0.4 | objects: Internal methods, descriptors, receiver and callable/constructor contracts | 7788 | 11 | 0 | 0 | 0 | references |
| P1.1 | grammar: Source grammar and classified strict/early errors | 3669 | 664 | 0 | 0 | 0 | environments, references |
| P1.2 | classes: Classes, private slots, super and derived construction | 16744 | 378 | 0 | 0 | 0 | objects, grammar |
| P1.3 | suspension: Resumable frames, generators, async functions and promise jobs | 4259 | 822 | 0 | 0 | 0 | completion, environments, objects, grammar |
| P1.4 | modules: Module linking, live bindings, evaluation and dynamic import | 1680 | 987 | 0 | 0 | 0 | environments, suspension, grammar |
| P1.5 | storage: BigInt, buffers, typed arrays, shared memory and GC weak slots | 6059 | 1592 | 0 | 0 | 0 | objects |
| P1.6 | host: Test262 realm, agent, GC, buffer and async host hooks | 206 | 26 | 0 | 0 | 0 | environments, suspension, modules, storage |
| P2.1 | library: Remaining standard builtin algorithms and descriptors | 18491 | 11054 | 0 | 0 | 0 | completion, references, objects |
| P2.2 | intl: ECMA-402 constructors, algorithms and locale data | 2922 | 3792 | 0 | 0 | 0 | library |
| P3.1 | review: Unmapped/staging targets requiring specification and applicability review | 1350 | 743 | 0 | 0 | 0 | — |

## Observed blockers

| Symptom | Modes | Representative test / mode |
| --- | ---: | --- |
| none | 81806 | `—` |
| exception:TypeError | 8424 | `annexB/built-ins/RegExp/legacy-accessors/index/prop-desc.js [sloppy]` |
| assertion | 7577 | `annexB/built-ins/RegExp/legacy-accessors/index/this-cross-realm-constructor.js [sloppy]` |
| exception:RangeError | 2227 | `built-ins/Array/prototype/slice/S15.4.4.10_A1.1_T3.js [sloppy]` |
| missing-expected-error | 831 | `annexB/language/expressions/template-literal/legacy-octal-escape-sequence-strict.js [strict]` |
| unresolved-name | 647 | `annexB/built-ins/escape/argument_bigint.js [sloppy]` |
| exception:SyntaxError | 577 | `annexB/language/function-code/function-redeclaration-block.js [sloppy]` |
| unclassified-parse | 564 | `language/asi/S7.9.2_A1_T3.js [sloppy]` |
| exception:ThrownValue | 152 | `built-ins/Promise/all/capability-resolve-throws-reject.js [sloppy]` |
| exception:crash | 55 | `annexB/language/statements/labeled/function-declaration.js [sloppy]` |
| resource-limit | 56 | `built-ins/Array/prototype/every/15.4.4.16-7-c-ii-2.js [sloppy]` |
| exception:Error | 10 | `built-ins/AsyncGeneratorPrototype/return/return-suspendedStart-broken-promise.js [sloppy]` |

## Frequent diagnostics

Exact per-mode evidence, source hashes, esid, includes and dependency labels are in `items.jsonl`.

| Diagnostic (numeric details normalized) | Modes |
| --- | ---: |
| TypeError: value is not callable | 5521 |
| Test262Error: throws failed | 3310 |
| Test262Error: sameValue failed | 2783 |
| RangeError: invalid Temporal date string | 1784 |
| uncaught JavaScript value: Object(ObjectId { heap: #, serial: # }) | 1611 |
| ok | 831 |
| TypeError: cannot access a property of null or undefined | 802 |
| TypeError: property helper requires an object | 562 |
| TypeError: value is not a constructor | 408 |
| Test262Error: isConstructor failed | 272 |
| Test262Error: verifyProperty failed | 237 |
| ReferenceError: DisposableStack is not defined | 188 |
| expected RParen (found Punct(Comma)) | 171 |
| RangeError: invalid Temporal.Duration string | 158 |
| Test262Error: assert failed | 147 |
| Test262Error: compareArray failed | 142 |
| ReferenceError: AsyncDisposableStack is not defined | 146 |
| expected ';' (found Identifier("x")) | 139 |
| ReferenceError: ShadowRealm is not defined | 118 |
| TypeError: class extends value is not a constructor or null | 108 |
| default import requires 'from' or ',' (found Punct(Star)) | 96 |
| RangeError: Temporal.toZonedDateTime currently supports UTC | 90 |
| expected ';' (found Identifier("_")) | 59 |
| adapter exited with code # | 54 |
| TypeError: TypedArray is out of bounds | 50 |
| RangeError: maximum call depth exceeded | 42 |
| expected an expression (found Punct(Ellipsis)) | 42 |
| TypeError: cannot convert null or undefined to Object | 39 |

## Measurement limits

A passing result is the runner's observation, not proof of complete feature conformance. Unclassified parser rejections cannot establish required negative early errors. Existing adapter records may not distinguish a runtime exception in an include from one in the test body. Missing APIs, assertion mismatches and timeouts require reproduction before assigning a confirmed engine root cause. Staging, Annex B, Intl and host-dependent cases remain visible pending applicability audit.

## Historical focused P0.4 audits

The following focused filters were measured against the prior `6eec1ac9…`
snapshot. They remain useful regression evidence but are not substitutes for
the complete `72faf8ec…` inventory above.

### Closure audit

The final targeted realm and Proxy audit records **607/607** passing modes for
`built-ins/Proxy`, **60/60** for `built-ins/Proxy/construct`, and **20/20** for
`built-ins/Reflect/construct`. The remaining Reflect modes required only that
`Date.now` exist as a callable, non-constructor; the Date baseline supplies
that contract without claiming complete Date object support. The public
`conformance_edges` regression suite passes **37/37**.

### Weak collections, Date and Array

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

These focused results are historical regression evidence for their respective slices. The current complete-inventory baseline is the reconciled 72faf8ec result recorded above and in the checked-in summary.
