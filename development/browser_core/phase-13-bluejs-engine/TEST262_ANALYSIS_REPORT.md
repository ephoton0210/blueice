# Test262 architecture triage

Snapshot: `6eec1ac9ee144dafd8f344d73a21f36bfc9f6755`.

Reconciled 53,404 files / 102,578 modes; complete inventory: True.

Targets are inferred from paths/metadata; blockers are first observed symptoms, not proven root causes. Feature/dependency counts overlap. Target counts are exclusive and reconcile to all modes. Priorities are dependency order, not failure-count order. Passed negatives remain passes; no outcomes are excluded.

| Order | Target | Pass | Fail | Unsupported | Timeout | Harness error | Prerequisites |
| --- | --- | ---: | ---: | ---: | ---: | ---: | --- |
| P0.1 | completion: Completion records, iterator lifetime, catch/finally, control transfer | 2703 | 5476 | 1480 | 0 | 0 | — |
| P0.2 | environments: Persistent realms, bindings, parameter environments, arguments, eval | 790 | 2362 | 793 | 2 | 0 | completion |
| P0.3 | references: Reference evaluation, coercion and observable evaluation order | 1820 | 2831 | 1432 | 0 | 0 | completion, environments |
| P0.4 | objects: Internal methods, descriptors, receiver and callable/constructor contracts | 1976 | 4778 | 1045 | 0 | 0 | references |
| P1.1 | grammar: Source grammar and classified strict/early errors | 1394 | 1202 | 1714 | 5 | 0 | environments, references |
| P1.2 | classes: Classes, private slots, super and derived construction | 4 | 14582 | 2536 | 0 | 0 | objects, grammar |
| P1.3 | suspension: Resumable frames, generators, async functions and promise jobs | 11 | 3697 | 1367 | 0 | 0 | completion, environments, objects, grammar |
| P1.4 | modules: Module linking, live bindings, evaluation and dynamic import | 72 | 1125 | 1466 | 0 | 0 | environments, suspension, grammar |
| P1.5 | storage: BigInt, buffers, typed arrays, shared memory and GC weak slots | 50 | 3813 | 3788 | 0 | 0 | objects |
| P1.6 | host: Test262 realm, agent, GC, buffer and async host hooks | 74 | 60 | 98 | 0 | 0 | environments, suspension, modules, storage |
| P2.1 | library: Remaining standard builtin algorithms and descriptors | 6487 | 18181 | 3683 | 908 | 0 | completion, references, objects |
| P2.2 | intl: ECMA-402 constructors, algorithms and locale data | 434 | 3296 | 2952 | 0 | 0 | library |
| P3.1 | review: Unmapped/staging targets requiring specification and applicability review | 411 | 1294 | 382 | 4 | 0 | — |

## Observed blockers

| Symptom | Modes | Representative test / mode |
| --- | ---: | --- |
| unclassified-parse | 39447 | `annexB/built-ins/RegExp/RegExp-control-escape-russian-letter.js [sloppy]` |
| none | 16226 | `—` |
| unresolved-name | 14768 | `annexB/built-ins/Date/prototype/getYear/B.2.4.js [sloppy]` |
| exception:TypeError | 10160 | `annexB/built-ins/RegExp/legacy-accessors/index/prop-desc.js [sloppy]` |
| harness-prerequisite | 9166 | `annexB/built-ins/TypedArrayConstructors/from/iterator-method-emulates-undefined.js [sloppy]` |
| assertion | 4467 | `annexB/built-ins/Function/createdynfn-no-line-terminator-html-close-comment-params.js [sloppy]` |
| compiler-unsupported | 3955 | `annexB/language/eval-code/direct/block-decl-nostrict.js [sloppy]` |
| async-host | 1015 | `built-ins/Atomics/waitAsync/false-for-timeout.js [sloppy]` |
| instruction-limit | 906 | `built-ins/RegExp/CharacterClassEscapes/character-class-digit-class-escape-negative-cases.js [sloppy]` |
| missing-expected-error | 866 | `annexB/language/expressions/template-literal/legacy-octal-escape-sequence-strict.js [strict]` |
| module-host | 835 | `built-ins/AbstractModuleSource/length.js [module]` |
| host-hook | 450 | `annexB/built-ins/Array/from/iterator-method-emulates-undefined.js [sloppy]` |
| exception:ThrownValue | 248 | `built-ins/Array/prototype/join/S15.4.4.5_A2_T2.js [sloppy]` |
| exception:SyntaxError | 47 | `annexB/language/function-code/block-decl-func-skip-early-err-block.js [sloppy]` |
| deadline | 13 | `built-ins/RegExp/property-escapes/generated/strings/RGI_Emoji.js [sloppy]` |
| exception:RangeError | 7 | `intl402/Intl/getCanonicalLocales/non-iana-canon.js [sloppy]` |
| resource-limit | 2 | `staging/sm/JSON/parse-mega-huge-array.js [sloppy]` |

## Frequent diagnostics

Exact per-mode evidence, source hashes, esid, includes and dependency labels are in `items.jsonl`.

| Diagnostic (numeric details normalized) | Modes |
| --- | ---: |
| expected ';' (found Identifier("C")) | 7862 |
| expected ';' (found Punct(LBrace)) | 7827 |
| TypeError: value is not callable | 7161 |
| harness source parse unsupported | 5398 |
| ReferenceError: Temporal is not defined | 4820 |
| expected ';' (found Keyword(Function)) | 4444 |
| identifier or digit immediately after numeric literal (found Invalid("identifier or digit immediately after numeric literal")) | 4333 |
| expected LParen (found Punct(Star)) | 4323 |
| harness source compile unsupported | 3768 |
| this statement kind (functions, for-in/of, switch, return or exceptions) | 3747 |
| Test262Error: throws failed | 2651 |
| TypeError: cannot access a property of null or undefined | 2478 |
| ReferenceError: Date is not defined | 1536 |
| ReferenceError: eval is not defined | 1292 |
| Test262Error: sameValue failed | 1138 |
| ReferenceError: ArrayBuffer is not defined | 1050 |
| async jobs and $DONE host | 1015 |
| BlueJS instruction budget exhausted | 906 |
| ok | 866 |
| module host | 835 |
| ReferenceError: Proxy is not defined | 813 |
| expected RParen (found Keyword(Function)) | 803 |
| ReferenceError: arguments is not defined | 724 |
| expected Comma (found Punct(Star)) | 554 |
| expected Comma (found Keyword(Function)) | 519 |
| expected a property key (found Punct(Star)) | 513 |
| ReferenceError: Set is not defined | 504 |
| ReferenceError: Promise is not defined | 481 |
| only a plain identifier is supported as a for-in/for-of target when no declaration keyword precedes it | 481 |
| missing Test262 host hook | 450 |

## Measurement limits

A passing result is the runner's observation, not proof of complete feature conformance. Unclassified parser rejections cannot establish required negative early errors. Existing adapter records may not distinguish a runtime exception in an include from one in the test body. Missing APIs, assertion mismatches and timeouts require reproduction before assigning a confirmed engine root cause. Staging, Annex B, Intl and host-dependent cases remain visible pending applicability audit.

## Recorded run

Measured 2026-09-10 after the first P0.1 iterator slice. See [architecture backlog](TEST262_ARCHITECTURE.md) for implementation status, exact before/after changes and remaining work.

- Adapter SHA-256: `d362d653a6caf73197b156be3c010b0fb43641fc0d6378a3cfdb4ac4898e661b`
- Regex worker SHA-256: `2e25f5dcab5e875327777c041d07f7cb4c786da6bcfb8bbd159676432181d56a`
- Runner SHA-256: `9b141ccfc900dee9620b33d731f3efb9db89f638a2a530bad3e5ae77de7cec5f`
- Mode instruction budget: 100000; case deadline: 2 seconds; workers: 8.
- Full results: `target/test262-architecture-after/results.jsonl`.
- Per-mode classifications: `target/test262-architecture-after/analysis/items.jsonl`.
- Machine-readable classifications: `target/test262-architecture-after/analysis/analysis.json`.

Regenerate using the commands in the [runner README](../../../backend/bluejs/test262/README.md). The tracked [runner summary](test262-summary.json) retains feature/group counts and snapshot provenance. Generated full records are local artifacts, not checked into source control.
