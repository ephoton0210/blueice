# macOS Test262 Report

**Measurement: 2026-10-06T11:11:26.555703+00:00. All numbers use this one verified source snapshot.**

## Current complete inventory

Source fingerprint: `a791618afcfab936be3ea79a37ef40dfcc5d86c9d1d56416341b9e9243372b95`. Adapter SHA-256: `a9ecc3154b5a698c9904a3c7f9dc3546daf2c114a69ccd956a19f5e58a2cb998`. Pinned Test262 revision: `72faf8ec1445c55149615e8b35187830783aba1a`.

**102,921 applicable modes passed**, across 102,926 scheduled modes and 53,582 test files, in 441.025 seconds. The scheduled inventory retains 4 host exclusions and 1 stale corpus modes. These modes are not counted as passes.

The runner returns 1 when any scheduled status is not `pass`. The command record retains that exit; semantic outcomes determine the gate.

Test262 has no official Core classification. This report defines ECMA-262 Core as `language/` plus `built-ins/`, the complete ECMA-262 scope as Core plus `annexB/` and `staging/`, and ECMA-402 as `intl402/`. All pass-rate denominators include the scheduled dispositions in their row.

| Scope | Scheduled | Pass | Fail | Unsupported | Excluded | Stale corpus | Timeout | Harness error | Raw pass rate |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| ECMA-262 Core (`language/` + `built-ins/`) | 91,820 | 91,816 | 0 | 0 | 4 | 0 | 0 | 0 | 99.996% |
| Complete ECMA-262 scope (Core + `annexB/` + `staging/`) | 95,980 | 95,975 | 0 | 0 | 4 | 1 | 0 | 0 | 99.995% |
| ECMA-402 (`intl402/`) | 6,714 | 6,714 | 0 | 0 | 0 | 0 | 0 | 0 | 100.000% |
| Test262 harness support (`harness/`) | 232 | 232 | 0 | 0 | 0 | 0 | 0 | 0 | 100.000% |
| **All Test262 runner modes** | 102,926 | 102,921 | 0 | 0 | 4 | 1 | 0 | 0 | 99.995% |

| Top-level Test262 group | Scheduled | Pass | Fail | Unsupported | Excluded | Stale corpus | Timeout | Harness error | Raw pass rate |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `language/` | 44,497 | 44,497 | 0 | 0 | 0 | 0 | 0 | 0 | 100.000% |
| `built-ins/` | 47,323 | 47,319 | 0 | 0 | 4 | 0 | 0 | 0 | 99.992% |
| `annexB/` | 1,377 | 1,376 | 0 | 0 | 0 | 1 | 0 | 0 | 99.927% |
| `staging/` | 2,783 | 2,783 | 0 | 0 | 0 | 0 | 0 | 0 | 100.000% |
| `intl402/` | 6,714 | 6,714 | 0 | 0 | 0 | 0 | 0 | 0 | 100.000% |
| `harness/` | 232 | 232 | 0 | 0 | 0 | 0 | 0 | 0 | 100.000% |

### Non-pass inventory dispositions

| Path | Modes | Status | Reason |
| --- | --- | --- | --- |
| `annexB/language/function-code/block-decl-func-skip-arguments.js` | 1 | stale_corpus | contradicts the current FunctionDeclarationInstantiation Annex B web-compat insertion point (verified against the live spec text); see tc39/test262#5113, fix pending in tc39/test262#5112 |
| `built-ins/Atomics/wait/bigint/cannot-suspend-throws.js` | 2 | excluded | host declares [[CanBlock]] = true, fixture requires CanBlockIsFalse |
| `built-ins/Atomics/wait/cannot-suspend-throws.js` | 2 | excluded | host declares [[CanBlock]] = true, fixture requires CanBlockIsFalse |

### Corrected crash regressions

| Crash regression | Scheduled | Pass | Fail | Unsupported | Excluded | Stale corpus | Timeout | Harness error | Raw pass rate |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `built-ins/Array/prototype/push/S15.4.4.7_A3.js` | 2 | 2 | 0 | 0 | 0 | 0 | 0 | 0 | 100.000% |
| `built-ins/Proxy/has/null-handler.js` | 2 | 2 | 0 | 0 | 0 | 0 | 0 | 0 | 100.000% |
| `built-ins/Proxy/has/null-handler-using-with.js` | 1 | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 100.000% |

## Selected results

| Selection | Scheduled | Pass | Fail | Unsupported | Excluded | Stale corpus | Timeout | Harness error | Raw pass rate |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `built-ins/Array/` | 6,117 | 6,117 | 0 | 0 | 0 | 0 | 0 | 0 | 100.000% |
| `built-ins/TypedArray/` + `built-ins/TypedArrayConstructors/` | 4,322 | 4,322 | 0 | 0 | 0 | 0 | 0 | 0 | 100.000% |
| `built-ins/ArrayBuffer/` | 442 | 442 | 0 | 0 | 0 | 0 | 0 | 0 | 100.000% |
| `built-ins/SharedArrayBuffer/` | 208 | 208 | 0 | 0 | 0 | 0 | 0 | 0 | 100.000% |
| `built-ins/DataView/` | 1,122 | 1,122 | 0 | 0 | 0 | 0 | 0 | 0 | 100.000% |
| `built-ins/Atomics/` | 778 | 774 | 0 | 0 | 4 | 0 | 0 | 0 | 99.486% |
| `built-ins/Iterator/` | 1,308 | 1,308 | 0 | 0 | 0 | 0 | 0 | 0 | 100.000% |
| `built-ins/Promise/` | 1,458 | 1,458 | 0 | 0 | 0 | 0 | 0 | 0 | 100.000% |
| `built-ins/ShadowRealm/` | 124 | 124 | 0 | 0 | 0 | 0 | 0 | 0 | 100.000% |
| `built-ins/AsyncDisposableStack/` | 208 | 208 | 0 | 0 | 0 | 0 | 0 | 0 | 100.000% |
| `built-ins/DisposableStack/` | 186 | 186 | 0 | 0 | 0 | 0 | 0 | 0 | 100.000% |
| `built-ins/Temporal/` | 9,210 | 9,210 | 0 | 0 | 0 | 0 | 0 | 0 | 100.000% |
| `language/import/` | 135 | 135 | 0 | 0 | 0 | 0 | 0 | 0 | 100.000% |
| `language/expressions/dynamic-import/` | 1,900 | 1,900 | 0 | 0 | 0 | 0 | 0 | 0 | 100.000% |
| `language/module-code/` | 602 | 602 | 0 | 0 | 0 | 0 | 0 | 0 | 100.000% |
| `language/statements/using/` | 154 | 154 | 0 | 0 | 0 | 0 | 0 | 0 | 100.000% |
| `language/statements/await-using/` | 188 | 188 | 0 | 0 | 0 | 0 | 0 | 0 | 100.000% |
| `language/statements/for-await-of/` | 2,431 | 2,431 | 0 | 0 | 0 | 0 | 0 | 0 | 100.000% |
| `language/eval-code/` | 454 | 454 | 0 | 0 | 0 | 0 | 0 | 0 | 100.000% |

## ECMA-402 and Temporal breakdown

| Intl group | Scheduled | Pass | Fail | Unsupported | Excluded | Stale corpus | Timeout | Harness error | Raw pass rate |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `intl402/Temporal/` | 4,058 | 4,058 | 0 | 0 | 0 | 0 | 0 | 0 | 100.000% |
| `intl402/NumberFormat/` | 498 | 498 | 0 | 0 | 0 | 0 | 0 | 0 | 100.000% |
| `intl402/DateTimeFormat/` | 488 | 488 | 0 | 0 | 0 | 0 | 0 | 0 | 100.000% |
| `intl402/Locale/` | 336 | 336 | 0 | 0 | 0 | 0 | 0 | 0 | 100.000% |
| `intl402/DurationFormat/` | 220 | 220 | 0 | 0 | 0 | 0 | 0 | 0 | 100.000% |
| `intl402/ListFormat/` | 162 | 162 | 0 | 0 | 0 | 0 | 0 | 0 | 100.000% |
| `intl402/RelativeTimeFormat/` | 160 | 160 | 0 | 0 | 0 | 0 | 0 | 0 | 100.000% |
| `intl402/Segmenter/` | 158 | 158 | 0 | 0 | 0 | 0 | 0 | 0 | 100.000% |
| `intl402/Intl/` | 132 | 132 | 0 | 0 | 0 | 0 | 0 | 0 | 100.000% |
| `intl402/Collator/` | 130 | 130 | 0 | 0 | 0 | 0 | 0 | 0 | 100.000% |
| `intl402/DisplayNames/` | 114 | 114 | 0 | 0 | 0 | 0 | 0 | 0 | 100.000% |
| `intl402/PluralRules/` | 106 | 106 | 0 | 0 | 0 | 0 | 0 | 0 | 100.000% |
| Top-level Intl fixtures and locale methods on other built-ins | 152 | 152 | 0 | 0 | 0 | 0 | 0 | 0 | 100.000% |
| **Total ECMA-402** | 6,714 | 6,714 | 0 | 0 | 0 | 0 | 0 | 0 | 100.000% |

| Temporal type | Scheduled | Pass | Fail | Unsupported | Excluded | Stale corpus | Timeout | Harness error | Raw pass rate |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Duration | 1,122 | 1,122 | 0 | 0 | 0 | 0 | 0 | 0 | 100.000% |
| Instant | 968 | 968 | 0 | 0 | 0 | 0 | 0 | 0 | 100.000% |
| Now | 138 | 138 | 0 | 0 | 0 | 0 | 0 | 0 | 100.000% |
| PlainDate | 2,290 | 2,290 | 0 | 0 | 0 | 0 | 0 | 0 | 100.000% |
| PlainDateTime | 2,512 | 2,512 | 0 | 0 | 0 | 0 | 0 | 0 | 100.000% |
| PlainMonthDay | 578 | 578 | 0 | 0 | 0 | 0 | 0 | 0 | 100.000% |
| PlainTime | 1,010 | 1,010 | 0 | 0 | 0 | 0 | 0 | 0 | 100.000% |
| PlainYearMonth | 1,672 | 1,672 | 0 | 0 | 0 | 0 | 0 | 0 | 100.000% |
| ZonedDateTime | 2,968 | 2,968 | 0 | 0 | 0 | 0 | 0 | 0 | 100.000% |
| toStringTag | 4 | 4 | 0 | 0 | 0 | 0 | 0 | 0 | 100.000% |
| Temporal root files | 6 | 6 | 0 | 0 | 0 | 0 | 0 | 0 | 100.000% |
| **Total Temporal** | 13,268 | 13,268 | 0 | 0 | 0 | 0 | 0 | 0 | 100.000% |

## BlueJS per-file coverage

All counters come from fresh atomic LLVM profiles for the current full run. Completion requires 100% raw lines, functions and regions.

| Metric | Covered / instrumented |
| --- | --- |
| Lines | 80,840 / 81,168 (99.595900%) |
| Functions | 5,737 / 5,792 (99.050414%) |
| Regions | 135,076 / 135,792 (99.472723%) |

**131 / 162 instrumented files are complete; 31 remain incomplete.**

| Source file | Raw lines | Raw functions | Raw regions | Status | Note |
| --- | --- | --- | --- | --- | --- |
| [`ast.rs`](../../../backend/bluejs/src/ast.rs) | 940 / 940 (100.000000%) | 95 / 95 (100.000000%) | 1,249 / 1,249 (100.000000%) | Complete |  |
| [`ast/retained_payload.rs`](../../../backend/bluejs/src/ast/retained_payload.rs) | 510 / 515 (99.029126%) | 41 / 41 (100.000000%) | 830 / 855 (97.076023%) | Incomplete |  |
| [`bin/bluejs-regexp-worker.rs`](../../../backend/bluejs/src/bin/bluejs-regexp-worker.rs) | 3 / 3 (100.000000%) | 1 / 1 (100.000000%) | 3 / 3 (100.000000%) | Complete |  |
| [`bin/bluejs-test262.rs`](../../../backend/bluejs/src/bin/bluejs-test262.rs) | 631 / 631 (100.000000%) | 56 / 56 (100.000000%) | 1,211 / 1,211 (100.000000%) | Complete |  |
| [`bytecode.rs`](../../../backend/bluejs/src/bytecode.rs) | 116 / 116 (100.000000%) | 18 / 18 (100.000000%) | 123 / 123 (100.000000%) | Complete |  |
| [`compiler.rs`](../../../backend/bluejs/src/compiler.rs) | 1,532 / 1,532 (100.000000%) | 147 / 147 (100.000000%) | 2,161 / 2,164 (99.861368%) | Incomplete |  |
| [`compiler/expressions.rs`](../../../backend/bluejs/src/compiler/expressions.rs) | 1,544 / 1,544 (100.000000%) | 42 / 42 (100.000000%) | 3,426 / 3,426 (100.000000%) | Complete |  |
| [`compiler/expressions/tests.rs`](../../../backend/bluejs/src/compiler/expressions/tests.rs) | — | — | — | No executable counters | Test source; not a coverage target |
| [`compiler/functions.rs`](../../../backend/bluejs/src/compiler/functions.rs) | 992 / 992 (100.000000%) | 52 / 52 (100.000000%) | 1,835 / 1,835 (100.000000%) | Complete |  |
| [`compiler/functions/tests.rs`](../../../backend/bluejs/src/compiler/functions/tests.rs) | — | — | — | No executable counters | Test source; not a coverage target |
| [`compiler/private_validation.rs`](../../../backend/bluejs/src/compiler/private_validation.rs) | 337 / 337 (100.000000%) | 32 / 32 (100.000000%) | 701 / 701 (100.000000%) | Complete |  |
| [`compiler/private_validation/tests.rs`](../../../backend/bluejs/src/compiler/private_validation/tests.rs) | — | — | — | No executable counters | Test source; not a coverage target |
| [`compiler/statements.rs`](../../../backend/bluejs/src/compiler/statements.rs) | 1,348 / 1,348 (100.000000%) | 69 / 69 (100.000000%) | 2,637 / 2,637 (100.000000%) | Complete |  |
| [`compiler/statements/tests.rs`](../../../backend/bluejs/src/compiler/statements/tests.rs) | — | — | — | No executable counters | Test source; not a coverage target |
| [`heap.rs`](../../../backend/bluejs/src/heap.rs) | 744 / 744 (100.000000%) | 78 / 78 (100.000000%) | 1,190 / 1,190 (100.000000%) | Complete |  |
| [`heap/binary_data.rs`](../../../backend/bluejs/src/heap/binary_data.rs) | 852 / 852 (100.000000%) | 77 / 77 (100.000000%) | 1,307 / 1,307 (100.000000%) | Complete |  |
| [`heap/collection_iteration.rs`](../../../backend/bluejs/src/heap/collection_iteration.rs) | 93 / 93 (100.000000%) | 5 / 5 (100.000000%) | 120 / 120 (100.000000%) | Complete |  |
| [`heap/core.rs`](../../../backend/bluejs/src/heap/core.rs) | 1,261 / 1,261 (100.000000%) | 116 / 116 (100.000000%) | 2,546 / 2,546 (100.000000%) | Complete |  |
| [`heap/debugger.rs`](../../../backend/bluejs/src/heap/debugger.rs) | 163 / 163 (100.000000%) | 10 / 10 (100.000000%) | 239 / 239 (100.000000%) | Complete |  |
| [`heap/exotic.rs`](../../../backend/bluejs/src/heap/exotic.rs) | 996 / 996 (100.000000%) | 119 / 119 (100.000000%) | 1,160 / 1,160 (100.000000%) | Complete |  |
| [`heap/lifecycle.rs`](../../../backend/bluejs/src/heap/lifecycle.rs) | 294 / 294 (100.000000%) | 31 / 31 (100.000000%) | 510 / 510 (100.000000%) | Complete |  |
| [`heap/object_storage.rs`](../../../backend/bluejs/src/heap/object_storage.rs) | 1,222 / 1,223 (99.918234%) | 72 / 72 (100.000000%) | 2,593 / 2,601 (99.692426%) | Incomplete |  |
| [`heap/tests.rs`](../../../backend/bluejs/src/heap/tests.rs) | — | — | — | No executable counters | Test source; not a coverage target |
| [`intl.rs`](../../../backend/bluejs/src/intl.rs) | 124 / 124 (100.000000%) | 19 / 19 (100.000000%) | 188 / 188 (100.000000%) | Complete |  |
| [`lib.rs`](../../../backend/bluejs/src/lib.rs) | — | — | — | No executable counters | Declarations/re-exports only; no executable code |
| [`native.rs`](../../../backend/bluejs/src/native.rs) | 684 / 684 (100.000000%) | 44 / 44 (100.000000%) | 1,231 / 1,231 (100.000000%) | Complete |  |
| [`page_runtime.rs`](../../../backend/bluejs/src/page_runtime.rs) | 1,156 / 1,156 (100.000000%) | 78 / 78 (100.000000%) | 1,342 / 1,342 (100.000000%) | Complete |  |
| [`parser.rs`](../../../backend/bluejs/src/parser.rs) | 568 / 568 (100.000000%) | 77 / 77 (100.000000%) | 779 / 779 (100.000000%) | Complete |  |
| [`parser/expressions.rs`](../../../backend/bluejs/src/parser/expressions.rs) | 946 / 946 (100.000000%) | 41 / 41 (100.000000%) | 1,670 / 1,670 (100.000000%) | Complete |  |
| [`parser/functions.rs`](../../../backend/bluejs/src/parser/functions.rs) | 666 / 666 (100.000000%) | 41 / 41 (100.000000%) | 1,048 / 1,048 (100.000000%) | Complete |  |
| [`parser/module.rs`](../../../backend/bluejs/src/parser/module.rs) | 143 / 143 (100.000000%) | 8 / 8 (100.000000%) | 243 / 243 (100.000000%) | Complete |  |
| [`parser/module_items.rs`](../../../backend/bluejs/src/parser/module_items.rs) | 339 / 339 (100.000000%) | 13 / 13 (100.000000%) | 635 / 635 (100.000000%) | Complete |  |
| [`parser/patterns.rs`](../../../backend/bluejs/src/parser/patterns.rs) | 175 / 175 (100.000000%) | 10 / 10 (100.000000%) | 279 / 279 (100.000000%) | Complete |  |
| [`parser/statements.rs`](../../../backend/bluejs/src/parser/statements.rs) | 589 / 616 (95.616883%) | 21 / 21 (100.000000%) | 1,137 / 1,177 (96.601529%) | Incomplete |  |
| [`parser/tests.rs`](../../../backend/bluejs/src/parser/tests.rs) | — | — | — | No executable counters | Test source; not a coverage target |
| [`primitive.rs`](../../../backend/bluejs/src/primitive.rs) | 277 / 277 (100.000000%) | 25 / 25 (100.000000%) | 518 / 518 (100.000000%) | Complete |  |
| [`program_abi.rs`](../../../backend/bluejs/src/program_abi.rs) | 29 / 29 (100.000000%) | 4 / 4 (100.000000%) | 53 / 53 (100.000000%) | Complete |  |
| [`program_debug.rs`](../../../backend/bluejs/src/program_debug.rs) | 347 / 347 (100.000000%) | 43 / 43 (100.000000%) | 398 / 398 (100.000000%) | Complete |  |
| [`program_debug/ast_ids.rs`](../../../backend/bluejs/src/program_debug/ast_ids.rs) | 484 / 484 (100.000000%) | 31 / 31 (100.000000%) | 703 / 703 (100.000000%) | Complete |  |
| [`property.rs`](../../../backend/bluejs/src/property.rs) | 91 / 91 (100.000000%) | 22 / 22 (100.000000%) | 117 / 117 (100.000000%) | Complete |  |
| [`regex_backrefs.rs`](../../../backend/bluejs/src/regex_backrefs.rs) | 98 / 98 (100.000000%) | 12 / 12 (100.000000%) | 186 / 186 (100.000000%) | Complete |  |
| [`regex_canonicalize.rs`](../../../backend/bluejs/src/regex_canonicalize.rs) | 636 / 636 (100.000000%) | 66 / 66 (100.000000%) | 1,354 / 1,354 (100.000000%) | Complete |  |
| [`regex_escapes.rs`](../../../backend/bluejs/src/regex_escapes.rs) | 228 / 228 (100.000000%) | 25 / 25 (100.000000%) | 484 / 484 (100.000000%) | Complete |  |
| [`regex_group_names.rs`](../../../backend/bluejs/src/regex_group_names.rs) | 323 / 323 (100.000000%) | 39 / 39 (100.000000%) | 600 / 600 (100.000000%) | Complete |  |
| [`regex_worker.rs`](../../../backend/bluejs/src/regex_worker.rs) | 641 / 643 (99.688958%) | 76 / 76 (100.000000%) | 950 / 955 (99.476440%) | Incomplete |  |
| [`regexp.rs`](../../../backend/bluejs/src/regexp.rs) | 304 / 304 (100.000000%) | 27 / 27 (100.000000%) | 468 / 468 (100.000000%) | Complete |  |
| [`source_encoding.rs`](../../../backend/bluejs/src/source_encoding.rs) | 153 / 153 (100.000000%) | 20 / 20 (100.000000%) | 295 / 295 (100.000000%) | Complete |  |
| [`string.rs`](../../../backend/bluejs/src/string.rs) | 107 / 107 (100.000000%) | 23 / 23 (100.000000%) | 174 / 174 (100.000000%) | Complete |  |
| [`token.rs`](../../../backend/bluejs/src/token.rs) | 1,381 / 1,381 (100.000000%) | 115 / 115 (100.000000%) | 2,200 / 2,200 (100.000000%) | Complete |  |
| [`value.rs`](../../../backend/bluejs/src/value.rs) | 27 / 27 (100.000000%) | 5 / 5 (100.000000%) | 38 / 38 (100.000000%) | Complete |  |
| [`vm.rs`](../../../backend/bluejs/src/vm.rs) | 628 / 628 (100.000000%) | 59 / 59 (100.000000%) | 910 / 910 (100.000000%) | Complete |  |
| [`vm/builtins.rs`](../../../backend/bluejs/src/vm/builtins.rs) | 320 / 320 (100.000000%) | 20 / 20 (100.000000%) | 587 / 587 (100.000000%) | Complete |  |
| [`vm/builtins/arguments.rs`](../../../backend/bluejs/src/vm/builtins/arguments.rs) | 784 / 784 (100.000000%) | 73 / 73 (100.000000%) | 1,292 / 1,292 (100.000000%) | Complete |  |
| [`vm/builtins/array_change_by_copy.rs`](../../../backend/bluejs/src/vm/builtins/array_change_by_copy.rs) | 197 / 197 (100.000000%) | 14 / 14 (100.000000%) | 455 / 456 (99.780702%) | Incomplete |  |
| [`vm/builtins/array_from_async.rs`](../../../backend/bluejs/src/vm/builtins/array_from_async.rs) | 366 / 366 (100.000000%) | 29 / 29 (100.000000%) | 889 / 889 (100.000000%) | Complete |  |
| [`vm/builtins/array_scan.rs`](../../../backend/bluejs/src/vm/builtins/array_scan.rs) | 123 / 123 (100.000000%) | 13 / 13 (100.000000%) | 185 / 185 (100.000000%) | Complete |  |
| [`vm/builtins/arrays.rs`](../../../backend/bluejs/src/vm/builtins/arrays.rs) | 1,334 / 1,334 (100.000000%) | 80 / 80 (100.000000%) | 2,886 / 2,898 (99.585921%) | Incomplete |  |
| [`vm/builtins/binary_data.rs`](../../../backend/bluejs/src/vm/builtins/binary_data.rs) | 1,659 / 1,659 (100.000000%) | 116 / 116 (100.000000%) | 2,751 / 2,751 (100.000000%) | Complete |  |
| [`vm/builtins/collection_iteration.rs`](../../../backend/bluejs/src/vm/builtins/collection_iteration.rs) | 116 / 116 (100.000000%) | 8 / 8 (100.000000%) | 196 / 200 (98.000000%) | Incomplete |  |
| [`vm/builtins/collections.rs`](../../../backend/bluejs/src/vm/builtins/collections.rs) | 372 / 372 (100.000000%) | 27 / 27 (100.000000%) | 750 / 755 (99.337748%) | Incomplete |  |
| [`vm/builtins/decorators.rs`](../../../backend/bluejs/src/vm/builtins/decorators.rs) | 558 / 558 (100.000000%) | 43 / 43 (100.000000%) | 1,121 / 1,121 (100.000000%) | Complete |  |
| [`vm/builtins/dynamic.rs`](../../../backend/bluejs/src/vm/builtins/dynamic.rs) | 717 / 717 (100.000000%) | 40 / 40 (100.000000%) | 1,308 / 1,308 (100.000000%) | Complete |  |
| [`vm/builtins/execution.rs`](../../../backend/bluejs/src/vm/builtins/execution.rs) | — | — | — | No executable counters | Declarations/re-exports only; no executable code |
| [`vm/builtins/execution/closures.rs`](../../../backend/bluejs/src/vm/builtins/execution/closures.rs) | 468 / 468 (100.000000%) | 20 / 20 (100.000000%) | 832 / 832 (100.000000%) | Complete |  |
| [`vm/builtins/execution/runtime.rs`](../../../backend/bluejs/src/vm/builtins/execution/runtime.rs) | 1,711 / 1,711 (100.000000%) | 102 / 102 (100.000000%) | 3,139 / 3,139 (100.000000%) | Complete |  |
| [`vm/builtins/execution/setup.rs`](../../../backend/bluejs/src/vm/builtins/execution/setup.rs) | 1,088 / 1,088 (100.000000%) | 93 / 93 (100.000000%) | 1,801 / 1,801 (100.000000%) | Complete |  |
| [`vm/builtins/general.rs`](../../../backend/bluejs/src/vm/builtins/general.rs) | 424 / 425 (99.764706%) | 37 / 37 (100.000000%) | 739 / 749 (98.664887%) | Incomplete |  |
| [`vm/builtins/generators.rs`](../../../backend/bluejs/src/vm/builtins/generators.rs) | 1,653 / 1,653 (100.000000%) | 77 / 77 (100.000000%) | 2,567 / 2,567 (100.000000%) | Complete |  |
| [`vm/builtins/globals.rs`](../../../backend/bluejs/src/vm/builtins/globals.rs) | 1,006 / 1,006 (100.000000%) | 12 / 12 (100.000000%) | 1,472 / 1,472 (100.000000%) | Complete |  |
| [`vm/builtins/immutable_arraybuffer.rs`](../../../backend/bluejs/src/vm/builtins/immutable_arraybuffer.rs) | 160 / 160 (100.000000%) | 11 / 11 (100.000000%) | 244 / 244 (100.000000%) | Complete |  |
| [`vm/builtins/math.rs`](../../../backend/bluejs/src/vm/builtins/math.rs) | 436 / 436 (100.000000%) | 27 / 27 (100.000000%) | 742 / 742 (100.000000%) | Complete |  |
| [`vm/builtins/native_dispatch.rs`](../../../backend/bluejs/src/vm/builtins/native_dispatch.rs) | — | — | — | No executable counters | Declarations/re-exports only; no executable code |
| [`vm/builtins/native_dispatch/date.rs`](../../../backend/bluejs/src/vm/builtins/native_dispatch/date.rs) | 687 / 687 (100.000000%) | 55 / 55 (100.000000%) | 1,230 / 1,232 (99.837662%) | Incomplete |  |
| [`vm/builtins/native_dispatch/dispatch.rs`](../../../backend/bluejs/src/vm/builtins/native_dispatch/dispatch.rs) | 1,240 / 1,261 (98.334655%) | 35 / 37 (94.594595%) | 3,062 / 3,150 (97.206349%) | Incomplete |  |
| [`vm/builtins/numbers.rs`](../../../backend/bluejs/src/vm/builtins/numbers.rs) | 244 / 246 (99.186992%) | 15 / 15 (100.000000%) | 424 / 431 (98.375870%) | Incomplete |  |
| [`vm/builtins/numbers/tests.rs`](../../../backend/bluejs/src/vm/builtins/numbers/tests.rs) | — | — | — | No executable counters | Test source; not a coverage target |
| [`vm/builtins/object.rs`](../../../backend/bluejs/src/vm/builtins/object.rs) | 1,419 / 1,419 (100.000000%) | 78 / 78 (100.000000%) | 2,427 / 2,427 (100.000000%) | Complete |  |
| [`vm/builtins/promise_combinators.rs`](../../../backend/bluejs/src/vm/builtins/promise_combinators.rs) | 415 / 415 (100.000000%) | 34 / 34 (100.000000%) | 854 / 854 (100.000000%) | Complete |  |
| [`vm/builtins/promise_core.rs`](../../../backend/bluejs/src/vm/builtins/promise_core.rs) | 799 / 801 (99.750312%) | 67 / 67 (100.000000%) | 1,329 / 1,339 (99.253174%) | Incomplete |  |
| [`vm/builtins/promises.rs`](../../../backend/bluejs/src/vm/builtins/promises.rs) | 987 / 987 (100.000000%) | 59 / 59 (100.000000%) | 1,642 / 1,642 (100.000000%) | Complete |  |
| [`vm/builtins/resource_management.rs`](../../../backend/bluejs/src/vm/builtins/resource_management.rs) | 546 / 558 (97.849462%) | 32 / 34 (94.117647%) | 806 / 829 (97.225573%) | Incomplete |  |
| [`vm/builtins/set_methods.rs`](../../../backend/bluejs/src/vm/builtins/set_methods.rs) | 314 / 314 (100.000000%) | 33 / 33 (100.000000%) | 627 / 629 (99.682035%) | Incomplete |  |
| [`vm/builtins/tests.rs`](../../../backend/bluejs/src/vm/builtins/tests.rs) | — | — | — | No executable counters | Test source; not a coverage target |
| [`vm/builtins/typed_arrays.rs`](../../../backend/bluejs/src/vm/builtins/typed_arrays.rs) | 948 / 948 (100.000000%) | 44 / 44 (100.000000%) | 1,836 / 1,836 (100.000000%) | Complete |  |
| [`vm/builtins/uint8array.rs`](../../../backend/bluejs/src/vm/builtins/uint8array.rs) | 493 / 508 (97.047244%) | 34 / 35 (97.142857%) | 743 / 768 (96.744792%) | Incomplete |  |
| [`vm/completion.rs`](../../../backend/bluejs/src/vm/completion.rs) | 113 / 113 (100.000000%) | 9 / 9 (100.000000%) | 162 / 162 (100.000000%) | Complete |  |
| [`vm/debugger.rs`](../../../backend/bluejs/src/vm/debugger.rs) | 773 / 773 (100.000000%) | 38 / 38 (100.000000%) | 946 / 946 (100.000000%) | Complete |  |
| [`vm/debugger/inspection.rs`](../../../backend/bluejs/src/vm/debugger/inspection.rs) | 189 / 191 (98.952880%) | 6 / 6 (100.000000%) | 182 / 192 (94.791667%) | Incomplete |  |
| [`vm/errors.rs`](../../../backend/bluejs/src/vm/errors.rs) | 406 / 406 (100.000000%) | 24 / 24 (100.000000%) | 747 / 747 (100.000000%) | Complete |  |
| [`vm/execution.rs`](../../../backend/bluejs/src/vm/execution.rs) | 1,562 / 1,562 (100.000000%) | 142 / 142 (100.000000%) | 2,543 / 2,543 (100.000000%) | Complete |  |
| [`vm/functions.rs`](../../../backend/bluejs/src/vm/functions.rs) | 434 / 434 (100.000000%) | 46 / 46 (100.000000%) | 551 / 557 (98.922801%) | Incomplete |  |
| [`vm/host_objects.rs`](../../../backend/bluejs/src/vm/host_objects.rs) | 525 / 620 (84.677419%) | 40 / 58 (68.965517%) | 597 / 713 (83.730715%) | Incomplete |  |
| [`vm/interpreter.rs`](../../../backend/bluejs/src/vm/interpreter.rs) | 1,517 / 1,517 (100.000000%) | 61 / 61 (100.000000%) | 3,360 / 3,360 (100.000000%) | Complete |  |
| [`vm/intl.rs`](../../../backend/bluejs/src/vm/intl.rs) | 597 / 597 (100.000000%) | 18 / 18 (100.000000%) | 779 / 779 (100.000000%) | Complete |  |
| [`vm/intl/collator_locale.rs`](../../../backend/bluejs/src/vm/intl/collator_locale.rs) | 444 / 444 (100.000000%) | 41 / 41 (100.000000%) | 711 / 711 (100.000000%) | Complete |  |
| [`vm/intl/date_time.rs`](../../../backend/bluejs/src/vm/intl/date_time.rs) | 827 / 827 (100.000000%) | 56 / 56 (100.000000%) | 1,226 / 1,226 (100.000000%) | Complete |  |
| [`vm/intl/list_duration.rs`](../../../backend/bluejs/src/vm/intl/list_duration.rs) | 593 / 618 (95.954693%) | 47 / 55 (85.454545%) | 891 / 957 (93.103448%) | Incomplete |  |
| [`vm/intl/number_options.rs`](../../../backend/bluejs/src/vm/intl/number_options.rs) | 446 / 446 (100.000000%) | 32 / 32 (100.000000%) | 632 / 633 (99.842022%) | Incomplete |  |
| [`vm/intl/number_runtime.rs`](../../../backend/bluejs/src/vm/intl/number_runtime.rs) | 438 / 438 (100.000000%) | 26 / 26 (100.000000%) | 586 / 586 (100.000000%) | Complete |  |
| [`vm/intl/plural_segmenter.rs`](../../../backend/bluejs/src/vm/intl/plural_segmenter.rs) | 585 / 585 (100.000000%) | 45 / 45 (100.000000%) | 843 / 843 (100.000000%) | Complete |  |
| [`vm/intl/shared.rs`](../../../backend/bluejs/src/vm/intl/shared.rs) | 719 / 719 (100.000000%) | 65 / 65 (100.000000%) | 1,168 / 1,168 (100.000000%) | Complete |  |
| [`vm/intrinsics.rs`](../../../backend/bluejs/src/vm/intrinsics.rs) | 454 / 454 (100.000000%) | 5 / 5 (100.000000%) | 600 / 600 (100.000000%) | Complete |  |
| [`vm/json.rs`](../../../backend/bluejs/src/vm/json.rs) | 806 / 818 (98.533007%) | 61 / 61 (100.000000%) | 1,608 / 1,649 (97.513645%) | Incomplete |  |
| [`vm/lifecycle.rs`](../../../backend/bluejs/src/vm/lifecycle.rs) | 214 / 214 (100.000000%) | 13 / 13 (100.000000%) | 229 / 229 (100.000000%) | Complete |  |
| [`vm/modules.rs`](../../../backend/bluejs/src/vm/modules.rs) | 1,920 / 1,920 (100.000000%) | 101 / 101 (100.000000%) | 2,970 / 2,970 (100.000000%) | Complete |  |
| [`vm/modules/deferred.rs`](../../../backend/bluejs/src/vm/modules/deferred.rs) | 303 / 303 (100.000000%) | 27 / 27 (100.000000%) | 475 / 475 (100.000000%) | Complete |  |
| [`vm/modules/namespace.rs`](../../../backend/bluejs/src/vm/modules/namespace.rs) | 282 / 282 (100.000000%) | 20 / 20 (100.000000%) | 450 / 450 (100.000000%) | Complete |  |
| [`vm/modules/synthetic.rs`](../../../backend/bluejs/src/vm/modules/synthetic.rs) | 211 / 211 (100.000000%) | 12 / 12 (100.000000%) | 317 / 317 (100.000000%) | Complete |  |
| [`vm/native_stack.rs`](../../../backend/bluejs/src/vm/native_stack.rs) | 97 / 97 (100.000000%) | 19 / 19 (100.000000%) | 160 / 160 (100.000000%) | Complete |  |
| [`vm/operations.rs`](../../../backend/bluejs/src/vm/operations.rs) | 763 / 763 (100.000000%) | 58 / 58 (100.000000%) | 1,449 / 1,449 (100.000000%) | Complete |  |
| [`vm/properties.rs`](../../../backend/bluejs/src/vm/properties.rs) | 668 / 705 (94.751773%) | 65 / 70 (92.857143%) | 1,207 / 1,295 (93.204633%) | Incomplete |  |
| [`vm/realm_reentrancy.rs`](../../../backend/bluejs/src/vm/realm_reentrancy.rs) | 32 / 32 (100.000000%) | 11 / 11 (100.000000%) | 42 / 42 (100.000000%) | Complete |  |
| [`vm/regexp.rs`](../../../backend/bluejs/src/vm/regexp.rs) | 1,140 / 1,140 (100.000000%) | 65 / 65 (100.000000%) | 2,115 / 2,115 (100.000000%) | Complete |  |
| [`vm/shadow_realm.rs`](../../../backend/bluejs/src/vm/shadow_realm.rs) | 446 / 446 (100.000000%) | 34 / 34 (100.000000%) | 746 / 746 (100.000000%) | Complete |  |
| [`vm/temporal.rs`](../../../backend/bluejs/src/vm/temporal.rs) | 578 / 578 (100.000000%) | 11 / 11 (100.000000%) | 568 / 568 (100.000000%) | Complete |  |
| [`vm/temporal/calendar.rs`](../../../backend/bluejs/src/vm/temporal/calendar.rs) | 147 / 147 (100.000000%) | 14 / 14 (100.000000%) | 215 / 215 (100.000000%) | Complete |  |
| [`vm/temporal/conversion.rs`](../../../backend/bluejs/src/vm/temporal/conversion.rs) | — | — | — | No executable counters | Declarations/re-exports only; no executable code |
| [`vm/temporal/conversion/calendar_fields.rs`](../../../backend/bluejs/src/vm/temporal/conversion/calendar_fields.rs) | 305 / 313 (97.444089%) | 25 / 27 (92.592593%) | 475 / 485 (97.938144%) | Incomplete |  |
| [`vm/temporal/conversion/from_value.rs`](../../../backend/bluejs/src/vm/temporal/conversion/from_value.rs) | 369 / 369 (100.000000%) | 21 / 21 (100.000000%) | 600 / 600 (100.000000%) | Complete |  |
| [`vm/temporal/conversion/getters.rs`](../../../backend/bluejs/src/vm/temporal/conversion/getters.rs) | 114 / 114 (100.000000%) | 4 / 4 (100.000000%) | 151 / 151 (100.000000%) | Complete |  |
| [`vm/temporal/conversion/numeric.rs`](../../../backend/bluejs/src/vm/temporal/conversion/numeric.rs) | 195 / 195 (100.000000%) | 24 / 24 (100.000000%) | 251 / 251 (100.000000%) | Complete |  |
| [`vm/temporal/conversion/options.rs`](../../../backend/bluejs/src/vm/temporal/conversion/options.rs) | 130 / 130 (100.000000%) | 12 / 12 (100.000000%) | 158 / 158 (100.000000%) | Complete |  |
| [`vm/temporal/conversion/zoned_conversion.rs`](../../../backend/bluejs/src/vm/temporal/conversion/zoned_conversion.rs) | 107 / 131 (81.679389%) | 6 / 11 (54.545455%) | 182 / 206 (88.349515%) | Incomplete |  |
| [`vm/temporal/dates.rs`](../../../backend/bluejs/src/vm/temporal/dates.rs) | — | — | — | No executable counters | Declarations/re-exports only; no executable code |
| [`vm/temporal/dates/arithmetic.rs`](../../../backend/bluejs/src/vm/temporal/dates/arithmetic.rs) | 206 / 206 (100.000000%) | 8 / 8 (100.000000%) | 281 / 281 (100.000000%) | Complete |  |
| [`vm/temporal/dates/comparison.rs`](../../../backend/bluejs/src/vm/temporal/dates/comparison.rs) | 49 / 49 (100.000000%) | 3 / 3 (100.000000%) | 65 / 65 (100.000000%) | Complete |  |
| [`vm/temporal/dates/construction.rs`](../../../backend/bluejs/src/vm/temporal/dates/construction.rs) | 214 / 214 (100.000000%) | 10 / 10 (100.000000%) | 231 / 233 (99.141631%) | Incomplete |  |
| [`vm/temporal/dates/conversions.rs`](../../../backend/bluejs/src/vm/temporal/dates/conversions.rs) | 66 / 66 (100.000000%) | 3 / 3 (100.000000%) | 108 / 108 (100.000000%) | Complete |  |
| [`vm/temporal/dates/date_time.rs`](../../../backend/bluejs/src/vm/temporal/dates/date_time.rs) | 124 / 124 (100.000000%) | 8 / 8 (100.000000%) | 198 / 198 (100.000000%) | Complete |  |
| [`vm/temporal/dates/formatting.rs`](../../../backend/bluejs/src/vm/temporal/dates/formatting.rs) | 166 / 166 (100.000000%) | 5 / 5 (100.000000%) | 216 / 216 (100.000000%) | Complete |  |
| [`vm/temporal/dates/now_and_zone.rs`](../../../backend/bluejs/src/vm/temporal/dates/now_and_zone.rs) | 192 / 192 (100.000000%) | 20 / 20 (100.000000%) | 266 / 266 (100.000000%) | Complete |  |
| [`vm/temporal/dates/with.rs`](../../../backend/bluejs/src/vm/temporal/dates/with.rs) | 144 / 144 (100.000000%) | 6 / 6 (100.000000%) | 262 / 262 (100.000000%) | Complete |  |
| [`vm/temporal/duration_conversion.rs`](../../../backend/bluejs/src/vm/temporal/duration_conversion.rs) | 48 / 48 (100.000000%) | 4 / 4 (100.000000%) | 73 / 73 (100.000000%) | Complete |  |
| [`vm/temporal/duration_math.rs`](../../../backend/bluejs/src/vm/temporal/duration_math.rs) | 305 / 305 (100.000000%) | 29 / 29 (100.000000%) | 361 / 361 (100.000000%) | Complete |  |
| [`vm/temporal/duration_operations.rs`](../../../backend/bluejs/src/vm/temporal/duration_operations.rs) | 611 / 611 (100.000000%) | 27 / 27 (100.000000%) | 874 / 874 (100.000000%) | Complete |  |
| [`vm/temporal/duration_relative.rs`](../../../backend/bluejs/src/vm/temporal/duration_relative.rs) | 523 / 523 (100.000000%) | 42 / 42 (100.000000%) | 659 / 659 (100.000000%) | Complete |  |
| [`vm/temporal/epoch.rs`](../../../backend/bluejs/src/vm/temporal/epoch.rs) | 136 / 136 (100.000000%) | 13 / 13 (100.000000%) | 237 / 237 (100.000000%) | Complete |  |
| [`vm/temporal/instant.rs`](../../../backend/bluejs/src/vm/temporal/instant.rs) | 449 / 462 (97.186147%) | 25 / 32 (78.125000%) | 635 / 667 (95.202399%) | Incomplete |  |
| [`vm/temporal/iso.rs`](../../../backend/bluejs/src/vm/temporal/iso.rs) | 17 / 17 (100.000000%) | 3 / 3 (100.000000%) | 24 / 24 (100.000000%) | Complete |  |
| [`vm/temporal/iso/annotations.rs`](../../../backend/bluejs/src/vm/temporal/iso/annotations.rs) | 271 / 271 (100.000000%) | 28 / 28 (100.000000%) | 452 / 452 (100.000000%) | Complete |  |
| [`vm/temporal/iso/datetime.rs`](../../../backend/bluejs/src/vm/temporal/iso/datetime.rs) | 145 / 145 (100.000000%) | 15 / 15 (100.000000%) | 259 / 259 (100.000000%) | Complete |  |
| [`vm/temporal/iso/duration.rs`](../../../backend/bluejs/src/vm/temporal/iso/duration.rs) | 117 / 117 (100.000000%) | 5 / 5 (100.000000%) | 183 / 183 (100.000000%) | Complete |  |
| [`vm/temporal/iso/offset.rs`](../../../backend/bluejs/src/vm/temporal/iso/offset.rs) | 182 / 182 (100.000000%) | 12 / 12 (100.000000%) | 349 / 349 (100.000000%) | Complete |  |
| [`vm/temporal/iso/scan.rs`](../../../backend/bluejs/src/vm/temporal/iso/scan.rs) | 349 / 349 (100.000000%) | 38 / 38 (100.000000%) | 628 / 628 (100.000000%) | Complete |  |
| [`vm/temporal/iso/tests.rs`](../../../backend/bluejs/src/vm/temporal/iso/tests.rs) | — | — | — | No executable counters | Test source; not a coverage target |
| [`vm/temporal/plain_date.rs`](../../../backend/bluejs/src/vm/temporal/plain_date.rs) | — | — | — | No executable counters | Declarations/re-exports only; no executable code |
| [`vm/temporal/plain_date/calendar_add.rs`](../../../backend/bluejs/src/vm/temporal/plain_date/calendar_add.rs) | 181 / 181 (100.000000%) | 6 / 6 (100.000000%) | 303 / 303 (100.000000%) | Complete |  |
| [`vm/temporal/plain_date/calendar_difference.rs`](../../../backend/bluejs/src/vm/temporal/plain_date/calendar_difference.rs) | 210 / 210 (100.000000%) | 8 / 8 (100.000000%) | 369 / 369 (100.000000%) | Complete |  |
| [`vm/temporal/plain_date/format.rs`](../../../backend/bluejs/src/vm/temporal/plain_date/format.rs) | 30 / 30 (100.000000%) | 4 / 4 (100.000000%) | 56 / 56 (100.000000%) | Complete |  |
| [`vm/temporal/plain_date/iso_date.rs`](../../../backend/bluejs/src/vm/temporal/plain_date/iso_date.rs) | 116 / 116 (100.000000%) | 17 / 17 (100.000000%) | 207 / 207 (100.000000%) | Complete |  |
| [`vm/temporal/plain_date/month_structure.rs`](../../../backend/bluejs/src/vm/temporal/plain_date/month_structure.rs) | 161 / 161 (100.000000%) | 14 / 14 (100.000000%) | 243 / 243 (100.000000%) | Complete |  |
| [`vm/temporal/plain_date/round_duration.rs`](../../../backend/bluejs/src/vm/temporal/plain_date/round_duration.rs) | 155 / 155 (100.000000%) | 10 / 10 (100.000000%) | 204 / 204 (100.000000%) | Complete |  |
| [`vm/temporal/plain_date/tests.rs`](../../../backend/bluejs/src/vm/temporal/plain_date/tests.rs) | — | — | — | No executable counters | Test source; not a coverage target |
| [`vm/temporal/plain_date_time_difference.rs`](../../../backend/bluejs/src/vm/temporal/plain_date_time_difference.rs) | 454 / 454 (100.000000%) | 25 / 25 (100.000000%) | 594 / 594 (100.000000%) | Complete |  |
| [`vm/temporal/plain_month_day.rs`](../../../backend/bluejs/src/vm/temporal/plain_month_day.rs) | 282 / 282 (100.000000%) | 28 / 28 (100.000000%) | 386 / 386 (100.000000%) | Complete |  |
| [`vm/temporal/plain_time.rs`](../../../backend/bluejs/src/vm/temporal/plain_time.rs) | 627 / 649 (96.610169%) | 39 / 44 (88.636364%) | 888 / 922 (96.312364%) | Incomplete |  |
| [`vm/temporal/plain_year_month.rs`](../../../backend/bluejs/src/vm/temporal/plain_year_month.rs) | 145 / 145 (100.000000%) | 13 / 13 (100.000000%) | 193 / 193 (100.000000%) | Complete |  |
| [`vm/temporal/receiver.rs`](../../../backend/bluejs/src/vm/temporal/receiver.rs) | 42 / 42 (100.000000%) | 3 / 3 (100.000000%) | 43 / 43 (100.000000%) | Complete |  |
| [`vm/temporal/rounding.rs`](../../../backend/bluejs/src/vm/temporal/rounding.rs) | 405 / 405 (100.000000%) | 33 / 33 (100.000000%) | 666 / 666 (100.000000%) | Complete |  |
| [`vm/temporal/time_zone.rs`](../../../backend/bluejs/src/vm/temporal/time_zone.rs) | 519 / 519 (100.000000%) | 47 / 47 (100.000000%) | 1,031 / 1,031 (100.000000%) | Complete |  |
| [`vm/temporal/time_zone_id.rs`](../../../backend/bluejs/src/vm/temporal/time_zone_id.rs) | 248 / 248 (100.000000%) | 32 / 32 (100.000000%) | 546 / 546 (100.000000%) | Complete |  |
| [`vm/temporal/year_month.rs`](../../../backend/bluejs/src/vm/temporal/year_month.rs) | 992 / 992 (100.000000%) | 71 / 71 (100.000000%) | 1,553 / 1,553 (100.000000%) | Complete |  |
| [`vm/temporal/zoned.rs`](../../../backend/bluejs/src/vm/temporal/zoned.rs) | — | — | — | No executable counters | Declarations/re-exports only; no executable code |
| [`vm/temporal/zoned/arithmetic.rs`](../../../backend/bluejs/src/vm/temporal/zoned/arithmetic.rs) | 177 / 177 (100.000000%) | 8 / 8 (100.000000%) | 280 / 280 (100.000000%) | Complete |  |
| [`vm/temporal/zoned/conversions.rs`](../../../backend/bluejs/src/vm/temporal/zoned/conversions.rs) | 121 / 121 (100.000000%) | 8 / 8 (100.000000%) | 160 / 160 (100.000000%) | Complete |  |
| [`vm/temporal/zoned/formatting.rs`](../../../backend/bluejs/src/vm/temporal/zoned/formatting.rs) | 170 / 170 (100.000000%) | 8 / 8 (100.000000%) | 264 / 264 (100.000000%) | Complete |  |
| [`vm/temporal/zoned/from_value.rs`](../../../backend/bluejs/src/vm/temporal/zoned/from_value.rs) | 190 / 190 (100.000000%) | 13 / 13 (100.000000%) | 250 / 250 (100.000000%) | Complete |  |
| [`vm/temporal/zoned/resolution.rs`](../../../backend/bluejs/src/vm/temporal/zoned/resolution.rs) | 112 / 112 (100.000000%) | 8 / 8 (100.000000%) | 154 / 154 (100.000000%) | Complete |  |
| [`vm/temporal/zoned/since_until.rs`](../../../backend/bluejs/src/vm/temporal/zoned/since_until.rs) | 146 / 146 (100.000000%) | 8 / 8 (100.000000%) | 212 / 212 (100.000000%) | Complete |  |
| [`vm/temporal/zoned/with.rs`](../../../backend/bluejs/src/vm/temporal/zoned/with.rs) | 215 / 215 (100.000000%) | 9 / 9 (100.000000%) | 371 / 371 (100.000000%) | Complete |  |
| [`vm/temporal/zoned_date_time.rs`](../../../backend/bluejs/src/vm/temporal/zoned_date_time.rs) | 245 / 245 (100.000000%) | 17 / 17 (100.000000%) | 405 / 405 (100.000000%) | Complete |  |
| [`vm/temporal/zoned_difference.rs`](../../../backend/bluejs/src/vm/temporal/zoned_difference.rs) | 409 / 409 (100.000000%) | 21 / 21 (100.000000%) | 574 / 574 (100.000000%) | Complete |  |
| [`vm/test262.rs`](../../../backend/bluejs/src/vm/test262.rs) | 65 / 65 (100.000000%) | 7 / 7 (100.000000%) | 107 / 107 (100.000000%) | Complete |  |
| [`vm/test262/assertions.rs`](../../../backend/bluejs/src/vm/test262/assertions.rs) | 297 / 299 (99.331104%) | 17 / 17 (100.000000%) | 825 / 838 (98.448687%) | Incomplete |  |
| [`vm/test262/cases.rs`](../../../backend/bluejs/src/vm/test262/cases.rs) | 817 / 817 (100.000000%) | 59 / 59 (100.000000%) | 1,381 / 1,384 (99.783237%) | Incomplete |  |
| [`vm/test262/foreign.rs`](../../../backend/bluejs/src/vm/test262/foreign.rs) | 1,800 / 1,800 (100.000000%) | 149 / 149 (100.000000%) | 2,756 / 2,756 (100.000000%) | Complete |  |
| [`vm/test262/harness.rs`](../../../backend/bluejs/src/vm/test262/harness.rs) | 271 / 271 (100.000000%) | 12 / 12 (100.000000%) | 405 / 405 (100.000000%) | Complete |  |
| [`vm/test262/reverse.rs`](../../../backend/bluejs/src/vm/test262/reverse.rs) | 681 / 681 (100.000000%) | 52 / 52 (100.000000%) | 936 / 936 (100.000000%) | Complete |  |
| [`vm/test262_agents.rs`](../../../backend/bluejs/src/vm/test262_agents.rs) | 488 / 488 (100.000000%) | 50 / 50 (100.000000%) | 684 / 684 (100.000000%) | Complete |  |

### Preservation of previously complete files

95 / 95 original complete files remain complete. The raw comparison records 0 per-file coverage percentage regressions.

## Nine most difficult and thirteen difficult files

| Difficulty | Selected source | Raw lines | Raw functions | Raw regions | Status |
| --- | --- | --- | --- | --- | --- |
| D5 | [`page_runtime.rs`](../../../backend/bluejs/src/page_runtime.rs) | 1,156 / 1,156 (100.000000%) | 78 / 78 (100.000000%) | 1,342 / 1,342 (100.000000%) | Complete |
| D5 | [`vm/builtins/binary_data.rs`](../../../backend/bluejs/src/vm/builtins/binary_data.rs) | 1,659 / 1,659 (100.000000%) | 116 / 116 (100.000000%) | 2,751 / 2,751 (100.000000%) | Complete |
| D5 | [`vm/builtins/execution/closures.rs`](../../../backend/bluejs/src/vm/builtins/execution/closures.rs) | 468 / 468 (100.000000%) | 20 / 20 (100.000000%) | 832 / 832 (100.000000%) | Complete |
| D5 | [`vm/builtins/generators.rs`](../../../backend/bluejs/src/vm/builtins/generators.rs) | 1,653 / 1,653 (100.000000%) | 77 / 77 (100.000000%) | 2,567 / 2,567 (100.000000%) | Complete |
| D5 | [`vm/debugger.rs`](../../../backend/bluejs/src/vm/debugger.rs) | 773 / 773 (100.000000%) | 38 / 38 (100.000000%) | 946 / 946 (100.000000%) | Complete |
| D5 | [`vm/execution.rs`](../../../backend/bluejs/src/vm/execution.rs) | 1,562 / 1,562 (100.000000%) | 142 / 142 (100.000000%) | 2,543 / 2,543 (100.000000%) | Complete |
| D5 | [`vm/interpreter.rs`](../../../backend/bluejs/src/vm/interpreter.rs) | 1,517 / 1,517 (100.000000%) | 61 / 61 (100.000000%) | 3,360 / 3,360 (100.000000%) | Complete |
| D5 | [`vm/shadow_realm.rs`](../../../backend/bluejs/src/vm/shadow_realm.rs) | 446 / 446 (100.000000%) | 34 / 34 (100.000000%) | 746 / 746 (100.000000%) | Complete |
| D5 | [`vm/test262/foreign.rs`](../../../backend/bluejs/src/vm/test262/foreign.rs) | 1,800 / 1,800 (100.000000%) | 149 / 149 (100.000000%) | 2,756 / 2,756 (100.000000%) | Complete |
| D4 | [`vm.rs`](../../../backend/bluejs/src/vm.rs) | 628 / 628 (100.000000%) | 59 / 59 (100.000000%) | 910 / 910 (100.000000%) | Complete |
| D4 | [`vm/builtins/array_from_async.rs`](../../../backend/bluejs/src/vm/builtins/array_from_async.rs) | 366 / 366 (100.000000%) | 29 / 29 (100.000000%) | 889 / 889 (100.000000%) | Complete |
| D4 | [`vm/builtins/execution/runtime.rs`](../../../backend/bluejs/src/vm/builtins/execution/runtime.rs) | 1,711 / 1,711 (100.000000%) | 102 / 102 (100.000000%) | 3,139 / 3,139 (100.000000%) | Complete |
| D4 | [`vm/builtins/execution/setup.rs`](../../../backend/bluejs/src/vm/builtins/execution/setup.rs) | 1,088 / 1,088 (100.000000%) | 93 / 93 (100.000000%) | 1,801 / 1,801 (100.000000%) | Complete |
| D4 | [`vm/builtins/object.rs`](../../../backend/bluejs/src/vm/builtins/object.rs) | 1,419 / 1,419 (100.000000%) | 78 / 78 (100.000000%) | 2,427 / 2,427 (100.000000%) | Complete |
| D4 | [`vm/modules.rs`](../../../backend/bluejs/src/vm/modules.rs) | 1,920 / 1,920 (100.000000%) | 101 / 101 (100.000000%) | 2,970 / 2,970 (100.000000%) | Complete |
| D4 | [`vm/modules/deferred.rs`](../../../backend/bluejs/src/vm/modules/deferred.rs) | 303 / 303 (100.000000%) | 27 / 27 (100.000000%) | 475 / 475 (100.000000%) | Complete |
| D4 | [`vm/modules/namespace.rs`](../../../backend/bluejs/src/vm/modules/namespace.rs) | 282 / 282 (100.000000%) | 20 / 20 (100.000000%) | 450 / 450 (100.000000%) | Complete |
| D4 | [`vm/regexp.rs`](../../../backend/bluejs/src/vm/regexp.rs) | 1,140 / 1,140 (100.000000%) | 65 / 65 (100.000000%) | 2,115 / 2,115 (100.000000%) | Complete |
| D4 | [`vm/temporal/conversion/from_value.rs`](../../../backend/bluejs/src/vm/temporal/conversion/from_value.rs) | 369 / 369 (100.000000%) | 21 / 21 (100.000000%) | 600 / 600 (100.000000%) | Complete |
| D4 | [`vm/temporal/duration_relative.rs`](../../../backend/bluejs/src/vm/temporal/duration_relative.rs) | 523 / 523 (100.000000%) | 42 / 42 (100.000000%) | 659 / 659 (100.000000%) | Complete |
| D4 | [`vm/temporal/plain_date_time_difference.rs`](../../../backend/bluejs/src/vm/temporal/plain_date_time_difference.rs) | 454 / 454 (100.000000%) | 25 / 25 (100.000000%) | 594 / 594 (100.000000%) | Complete |
| D4 | [`vm/temporal/zoned_difference.rs`](../../../backend/bluejs/src/vm/temporal/zoned_difference.rs) | 409 / 409 (100.000000%) | 21 / 21 (100.000000%) | 574 / 574 (100.000000%) | Complete |

Selected files complete: **22 / 22**. Modified production files complete: **50 / 50**.

### Verification evidence

| Gate | Current result |
| --- | --- |
| BlueJS Rust | 334 targets; 4,099 passed; 0 failed; 4 ignored |
| Full Test262 | 102,921 passed |
| Semantic comparison | 102,926 mode contracts; 0 changes |
| Workspace runtime and Rustdoc | Passed |

| Current gate | Pass | Fail | Ignored | Seconds |
| --- | --- | --- | --- | --- |
| Complete BlueJS Rust | 4,099 | 0 | 4 | 1346.614 |
| Critical Rust targets | 1,470 | 0 | 0 | 297.897 |
| Self-test tooling contracts | 73 | 0 | 0 | 10.058 |
| Workspace runtime tests | 2,170 | 0 | 60 | 337.668 |
| BlueJS Rustdoc tests | 2 | 0 | 0 | 4.685 |

| Static gate | Current result |
| --- | --- |
| Task Rust formatting | Passed |
| Diff whitespace | Passed |
| Workspace Clippy, warnings denied | Passed |

### Remaining modified production files

| Source file | Missing lines | Missing functions | Missing regions |
| --- | --- | --- | --- |

### Coverage percentage regressions

Zero per-file percentage regressions against the retained verified comparison thresholds.

**The requested completion gates are satisfied.**

[Frozen source hashes](../../../target/bluejs-selftest/runs/20261006-183231-de935772/source-state.json), [complete run record](../../../target/bluejs-selftest/runs/20261006-183231-de935772/run.json), [raw coverage data](../../../target/bluejs-selftest/runs/20261006-183231-de935772/report-data.json) and [semantic contract audit](../../../target/bluejs-selftest/runs/20261006-183231-de935772/outcome-contract-audit.json) retain this measurement.

## Difficulty ranking of the remaining incomplete files

| Rank | Difficulty | Source file | Missing lines | Missing functions | Missing regions |
| --- | --- | --- | --- | --- | --- |
| 1 | D3 | [`vm/host_objects.rs`](../../../backend/bluejs/src/vm/host_objects.rs) | 95 | 18 | 116 |
| 2 | D3 | [`vm/builtins/native_dispatch/dispatch.rs`](../../../backend/bluejs/src/vm/builtins/native_dispatch/dispatch.rs) | 21 | 2 | 88 |
| 3 | D3 | [`vm/properties.rs`](../../../backend/bluejs/src/vm/properties.rs) | 37 | 5 | 88 |
| 4 | D3 | [`vm/intl/list_duration.rs`](../../../backend/bluejs/src/vm/intl/list_duration.rs) | 25 | 8 | 66 |
| 5 | D3 | [`vm/builtins/promise_core.rs`](../../../backend/bluejs/src/vm/builtins/promise_core.rs) | 2 | 0 | 10 |
| 6 | D3 | [`vm/debugger/inspection.rs`](../../../backend/bluejs/src/vm/debugger/inspection.rs) | 2 | 0 | 10 |
| 7 | D2 | [`vm/temporal/plain_time.rs`](../../../backend/bluejs/src/vm/temporal/plain_time.rs) | 22 | 5 | 34 |
| 8 | D2 | [`vm/temporal/instant.rs`](../../../backend/bluejs/src/vm/temporal/instant.rs) | 13 | 7 | 32 |
| 9 | D2 | [`ast/retained_payload.rs`](../../../backend/bluejs/src/ast/retained_payload.rs) | 5 | 0 | 25 |
| 10 | D2 | [`vm/temporal/conversion/zoned_conversion.rs`](../../../backend/bluejs/src/vm/temporal/conversion/zoned_conversion.rs) | 24 | 5 | 24 |
| 11 | D2 | [`vm/builtins/arrays.rs`](../../../backend/bluejs/src/vm/builtins/arrays.rs) | 0 | 0 | 12 |
| 12 | D2 | [`vm/builtins/general.rs`](../../../backend/bluejs/src/vm/builtins/general.rs) | 1 | 0 | 10 |
| 13 | D2 | [`vm/temporal/conversion/calendar_fields.rs`](../../../backend/bluejs/src/vm/temporal/conversion/calendar_fields.rs) | 8 | 2 | 10 |
| 14 | D2 | [`vm/functions.rs`](../../../backend/bluejs/src/vm/functions.rs) | 0 | 0 | 6 |
| 15 | D2 | [`vm/builtins/collections.rs`](../../../backend/bluejs/src/vm/builtins/collections.rs) | 0 | 0 | 5 |
| 16 | D2 | [`vm/builtins/collection_iteration.rs`](../../../backend/bluejs/src/vm/builtins/collection_iteration.rs) | 0 | 0 | 4 |
| 17 | D2 | [`compiler.rs`](../../../backend/bluejs/src/compiler.rs) | 0 | 0 | 3 |
| 18 | D2 | [`vm/test262/cases.rs`](../../../backend/bluejs/src/vm/test262/cases.rs) | 0 | 0 | 3 |
| 19 | D2 | [`vm/builtins/native_dispatch/date.rs`](../../../backend/bluejs/src/vm/builtins/native_dispatch/date.rs) | 0 | 0 | 2 |
| 20 | D2 | [`vm/builtins/set_methods.rs`](../../../backend/bluejs/src/vm/builtins/set_methods.rs) | 0 | 0 | 2 |
| 21 | D2 | [`vm/intl/number_options.rs`](../../../backend/bluejs/src/vm/intl/number_options.rs) | 0 | 0 | 1 |
| 22 | D1 | [`vm/json.rs`](../../../backend/bluejs/src/vm/json.rs) | 12 | 0 | 41 |
| 23 | D1 | [`parser/statements.rs`](../../../backend/bluejs/src/parser/statements.rs) | 27 | 0 | 40 |
| 24 | D1 | [`vm/builtins/uint8array.rs`](../../../backend/bluejs/src/vm/builtins/uint8array.rs) | 15 | 1 | 25 |
| 25 | D1 | [`vm/builtins/resource_management.rs`](../../../backend/bluejs/src/vm/builtins/resource_management.rs) | 12 | 2 | 23 |
| 26 | D1 | [`vm/test262/assertions.rs`](../../../backend/bluejs/src/vm/test262/assertions.rs) | 2 | 0 | 13 |
| 27 | D1 | [`heap/object_storage.rs`](../../../backend/bluejs/src/heap/object_storage.rs) | 1 | 0 | 8 |
| 28 | D1 | [`vm/builtins/numbers.rs`](../../../backend/bluejs/src/vm/builtins/numbers.rs) | 2 | 0 | 7 |
| 29 | D1 | [`regex_worker.rs`](../../../backend/bluejs/src/regex_worker.rs) | 2 | 0 | 5 |
| 30 | D0 | [`vm/temporal/dates/construction.rs`](../../../backend/bluejs/src/vm/temporal/dates/construction.rs) | 0 | 0 | 2 |
| 31 | D0 | [`vm/builtins/array_change_by_copy.rs`](../../../backend/bluejs/src/vm/builtins/array_change_by_copy.rs) | 0 | 0 | 1 |

## Reproduce the current inventory

```sh
python3 -m backend.bluejs.selftest run --mode pipeline --workspace
```

The pipeline runs affected tests first, then the full current inventory and fresh coverage. A partial or stale run cannot publish this report.
