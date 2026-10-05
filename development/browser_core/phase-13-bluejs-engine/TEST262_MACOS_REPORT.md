# macOS Test262 Report

**Measurement date: 2026-10-05. All results and coverage tables in this report use the same verified source snapshot.**

## Current complete inventory

The complete pinned Test262 inventory ran on macOS 26.6.2 (build 25G83, Apple silicon) with Rust/Cargo 1.98.0 and LLVM 22.1.8. The measured source is commit `1e6b3ab46653ddf010cc35f14ee9174e26241312` plus the [verified source patch](../../../target/test262-macos-20261005-corrections-batch-r28/prepared.patch); the [source hashes](../../../target/test262-macos-20261005-corrections-batch-r28/source-state.json) identify the exact snapshot. Corpus revision `72faf8ec1445c55149615e8b35187830783aba1a` includes main, proposals and staging without a path filter. All 53,582 test files and 102,926 scheduled modes were processed with 8 jobs in 274.207 seconds.

Adapter SHA-256: `1e6bb2f89c5abc8b93fc128f59496269cdbacdf0fc2def1434a7176a44d1f3fa`. The [complete mode results](../../../target/test262-macos-20261005-corrections-batch-r28/test262/results.jsonl), [run summary](../../../target/test262-macos-20261005-corrections-batch-r28/test262/summary.json) and [checked-in summary](test262-summary.json) contain this measurement.

**Applicable modes: 102,921 / 102,921 pass (100.000%).** The scheduled inventory has **102,921 / 102,926 pass (99.995%)**, with **zero failures, unsupported modes, timeouts or harness errors**. Four host exclusions and one stale fixture were not dispatched or counted as passes. The runner exit code is 1 because it treats any inventory status other than `pass` as nonzero; the [command record](../../../target/test262-macos-20261005-corrections-batch-r28/test262-command.json) retains that result.

Test262 has no official Core classification. This report defines ECMA-262 Core as `language/` plus `built-ins/`, the complete ECMA-262 scope as Core plus `annexB/` and `staging/`, and ECMA-402 as `intl402/`. Pass rates below use all scheduled modes in each row as their denominator.

| Scope | Scheduled | Pass | Fail | Unsupported | Excluded | Stale corpus | Timeout | Harness error | Raw pass rate |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| ECMA-262 Core (`language/` + `built-ins/`) | 91,820 | 91,816 | 0 | 0 | 4 | 0 | 0 | 0 | 99.996% |
| Complete ECMA-262 scope (Core + `annexB/` + `staging/`) | 95,980 | 95,975 | 0 | 0 | 4 | 1 | 0 | 0 | 99.995% |
| ECMA-402 (`intl402/`) | 6,714 | 6,714 | 0 | 0 | 0 | 0 | 0 | 0 | 100.000% |
| Test262 harness support (`harness/`) | 232 | 232 | 0 | 0 | 0 | 0 | 0 | 0 | 100.000% |
| **All Test262 runner modes** | 102,926 | 102,921 | 0 | 0 | 4 | 1 | 0 | 0 | 99.995% |

| Top-level Test262 group | Scheduled | Pass | Fail | Unsupported | Excluded | Stale corpus | Timeout | Harness error | Raw pass rate |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `language/` | 44,497 | 44,497 | 0 | 0 | 0 | 0 | 0 | 0 | 100.000% |
| `built-ins/` | 47,323 | 47,319 | 0 | 0 | 4 | 0 | 0 | 0 | 99.992% |
| `annexB/` | 1,377 | 1,376 | 0 | 0 | 0 | 1 | 0 | 0 | 99.927% |
| `staging/` | 2,783 | 2,783 | 0 | 0 | 0 | 0 | 0 | 0 | 100.000% |
| `intl402/` | 6,714 | 6,714 | 0 | 0 | 0 | 0 | 0 | 0 | 100.000% |
| `harness/` | 232 | 232 | 0 | 0 | 0 | 0 | 0 | 0 | 100.000% |

### Non-pass inventory dispositions

| Path | Modes | Status | Reason |
| --- | ---: | --- | --- |
| `annexB/language/function-code/block-decl-func-skip-arguments.js` | 1 | `stale_corpus` | The pinned fixture contradicts the runner's Annex B FunctionDeclarationInstantiation contract; classified as a stale corpus fixture. |
| `built-ins/Atomics/wait/bigint/cannot-suspend-throws.js` | 2 | `excluded` | Requires CanBlockIsFalse; this host declares [[CanBlock]] = true. |
| `built-ins/Atomics/wait/cannot-suspend-throws.js` | 2 | `excluded` | Requires CanBlockIsFalse; this host declares [[CanBlock]] = true. |

### Corrected crash regressions

All five crash regression modes pass in this complete run.

| Path | Modes | Current status | Verified behavior |
| --- | ---: | --- | --- |
| `built-ins/Array/prototype/push/S15.4.4.7_A3.js` | 2 | `pass` | Length overflow propagates a catchable RangeError and preserves writes before the final length Set. |
| `built-ins/Proxy/has/null-handler.js` | 2 | `pass` | A revoked Proxy propagates a catchable TypeError from `in`. |
| `built-ins/Proxy/has/null-handler-using-with.js` | 1 | `pass` | A revoked Proxy propagates a catchable TypeError during `with` lookup. |

## Selected results

Each selection is an exact path-prefix subset of the complete inventory.

| Selection | Scheduled | Pass | Fail | Unsupported | Excluded | Stale corpus | Timeout | Harness error | Raw pass rate |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
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

`intl402/` has **6,714 / 6,714 passing modes (100%)**.

| `intl402/` group | Modes | Pass | Fail | Pass rate |
| --- | ---: | ---: | ---: | ---: |
| `Temporal/` | 4,058 | 4,058 | 0 | 100% |
| `NumberFormat/` | 498 | 498 | 0 | 100% |
| `DateTimeFormat/` | 488 | 488 | 0 | 100% |
| `Locale/` | 336 | 336 | 0 | 100% |
| `DurationFormat/` | 220 | 220 | 0 | 100% |
| `ListFormat/` | 162 | 162 | 0 | 100% |
| `RelativeTimeFormat/` | 160 | 160 | 0 | 100% |
| `Segmenter/` | 158 | 158 | 0 | 100% |
| `Intl/` | 132 | 132 | 0 | 100% |
| `Collator/` | 130 | 130 | 0 | 100% |
| `DisplayNames/` | 114 | 114 | 0 | 100% |
| `PluralRules/` | 106 | 106 | 0 | 100% |
| Top-level Intl fixtures and locale methods on other built-ins | 152 | 152 | 0 | 100% |
| **Total** | 6,714 | 6,714 | 0 | 100% |

Temporal has **13,268 / 13,268 passing modes (100%)** across `built-ins/Temporal/` and `intl402/Temporal/`.

| Temporal type | Combined modes | Pass | Pass rate |
| --- | ---: | ---: | ---: |
| `ZonedDateTime` | 2,968 | 2,968 | 100% |
| `PlainDateTime` | 2,512 | 2,512 | 100% |
| `PlainDate` | 2,290 | 2,290 | 100% |
| `PlainYearMonth` | 1,672 | 1,672 | 100% |
| `Duration` | 1,122 | 1,122 | 100% |
| `PlainTime` | 1,010 | 1,010 | 100% |
| `Instant` | 968 | 968 | 100% |
| `PlainMonthDay` | 578 | 578 | 100% |
| `Now` | 138 | 138 | 100% |
| `Temporal root files` | 6 | 6 | 100% |
| `toStringTag` | 4 | 4 | 100% |
| **Total** | **13,268** | **13,268** | **100%** |

## BlueJS per-file coverage

Coverage uses fresh LLVM 22 profiles from all 334 BlueJS Rust harnesses and the complete Test262 run against the same source snapshot. Atomic profile counters are enabled. No earlier or cancelled-run profiles or executable maps contribute to these results. The [counter integrity audit](../../../target/test262-macos-20261005-corrections-batch-r28/counter-integrity.json) finds no unsigned-counter underflow.

Completion requires **100% raw LLVM lines, functions and regions**. Source-location unions do not determine completion. Test262 pass rate and Rust source coverage measure different things.

| Metric | Covered / instrumented | Coverage |
| --- | ---: | ---: |
| Lines | 80,667 / 81,027 | 99.555704% |
| Functions | 5,735 / 5,790 | 99.050086% |
| Regions | 134,679 / 135,518 | 99.380894% |

**119 / 162 instrumented files are complete; 43 remain incomplete.** All 179 Rust files under `backend/bluejs/src/` are listed below. The 17 files without executable counters have individual reasons. `Complete` requires exact covered and instrumented counts to match in all three columns.

| Source file | Raw lines | Raw functions | Raw regions | Status | Note |
| --- | ---: | ---: | ---: | --- | --- |
| [`ast.rs`](../../../backend/bluejs/src/ast.rs) | 940 / 940 (100.000%) | 95 / 95 (100.000%) | 1,249 / 1,249 (100.000%) | Complete | |
| [`ast/retained_payload.rs`](../../../backend/bluejs/src/ast/retained_payload.rs) | 510 / 515 (99.029%) | 41 / 41 (100.000%) | 830 / 855 (97.076%) | Incomplete | |
| [`bin/bluejs-regexp-worker.rs`](../../../backend/bluejs/src/bin/bluejs-regexp-worker.rs) | 3 / 3 (100.000%) | 1 / 1 (100.000%) | 3 / 3 (100.000%) | Complete | |
| [`bin/bluejs-test262.rs`](../../../backend/bluejs/src/bin/bluejs-test262.rs) | 631 / 631 (100.000%) | 56 / 56 (100.000%) | 1,211 / 1,211 (100.000%) | Complete | |
| [`bytecode.rs`](../../../backend/bluejs/src/bytecode.rs) | 116 / 116 (100.000%) | 18 / 18 (100.000%) | 123 / 123 (100.000%) | Complete | |
| [`compiler.rs`](../../../backend/bluejs/src/compiler.rs) | 1,532 / 1,532 (100.000%) | 147 / 147 (100.000%) | 2,161 / 2,164 (99.861%) | Incomplete | |
| [`compiler/expressions.rs`](../../../backend/bluejs/src/compiler/expressions.rs) | 1,540 / 1,540 (100.000%) | 42 / 42 (100.000%) | 3,408 / 3,408 (100.000%) | Complete | |
| [`compiler/expressions/tests.rs`](../../../backend/bluejs/src/compiler/expressions/tests.rs) | - | - | - | No counters | Test source; not a coverage target |
| [`compiler/functions.rs`](../../../backend/bluejs/src/compiler/functions.rs) | 992 / 992 (100.000%) | 52 / 52 (100.000%) | 1,835 / 1,835 (100.000%) | Complete | |
| [`compiler/functions/tests.rs`](../../../backend/bluejs/src/compiler/functions/tests.rs) | - | - | - | No counters | Test source; not a coverage target |
| [`compiler/private_validation.rs`](../../../backend/bluejs/src/compiler/private_validation.rs) | 337 / 337 (100.000%) | 32 / 32 (100.000%) | 701 / 701 (100.000%) | Complete | |
| [`compiler/private_validation/tests.rs`](../../../backend/bluejs/src/compiler/private_validation/tests.rs) | - | - | - | No counters | Test source; not a coverage target |
| [`compiler/statements.rs`](../../../backend/bluejs/src/compiler/statements.rs) | 1,348 / 1,348 (100.000%) | 69 / 69 (100.000%) | 2,637 / 2,637 (100.000%) | Complete | |
| [`compiler/statements/tests.rs`](../../../backend/bluejs/src/compiler/statements/tests.rs) | - | - | - | No counters | Test source; not a coverage target |
| [`heap.rs`](../../../backend/bluejs/src/heap.rs) | 744 / 744 (100.000%) | 78 / 78 (100.000%) | 1,190 / 1,190 (100.000%) | Complete | |
| [`heap/binary_data.rs`](../../../backend/bluejs/src/heap/binary_data.rs) | 852 / 852 (100.000%) | 77 / 77 (100.000%) | 1,307 / 1,307 (100.000%) | Complete | |
| [`heap/collection_iteration.rs`](../../../backend/bluejs/src/heap/collection_iteration.rs) | 93 / 93 (100.000%) | 5 / 5 (100.000%) | 120 / 120 (100.000%) | Complete | |
| [`heap/core.rs`](../../../backend/bluejs/src/heap/core.rs) | 1,261 / 1,261 (100.000%) | 116 / 116 (100.000%) | 2,546 / 2,546 (100.000%) | Complete | |
| [`heap/debugger.rs`](../../../backend/bluejs/src/heap/debugger.rs) | 163 / 163 (100.000%) | 10 / 10 (100.000%) | 239 / 239 (100.000%) | Complete | |
| [`heap/exotic.rs`](../../../backend/bluejs/src/heap/exotic.rs) | 996 / 996 (100.000%) | 119 / 119 (100.000%) | 1,160 / 1,160 (100.000%) | Complete | |
| [`heap/lifecycle.rs`](../../../backend/bluejs/src/heap/lifecycle.rs) | 294 / 294 (100.000%) | 31 / 31 (100.000%) | 510 / 510 (100.000%) | Complete | |
| [`heap/object_storage.rs`](../../../backend/bluejs/src/heap/object_storage.rs) | 1,222 / 1,223 (99.918%) | 72 / 72 (100.000%) | 2,593 / 2,601 (99.692%) | Incomplete | |
| [`heap/tests.rs`](../../../backend/bluejs/src/heap/tests.rs) | - | - | - | No counters | Test source; not a coverage target |
| [`intl.rs`](../../../backend/bluejs/src/intl.rs) | 124 / 124 (100.000%) | 19 / 19 (100.000%) | 188 / 188 (100.000%) | Complete | |
| [`lib.rs`](../../../backend/bluejs/src/lib.rs) | - | - | - | No counters | Declarations/re-exports only; no executable code |
| [`native.rs`](../../../backend/bluejs/src/native.rs) | 684 / 684 (100.000%) | 44 / 44 (100.000%) | 1,231 / 1,231 (100.000%) | Complete | |
| [`page_runtime.rs`](../../../backend/bluejs/src/page_runtime.rs) | 1,155 / 1,156 (99.913%) | 78 / 78 (100.000%) | 1,338 / 1,342 (99.702%) | Incomplete | |
| [`parser.rs`](../../../backend/bluejs/src/parser.rs) | 568 / 568 (100.000%) | 77 / 77 (100.000%) | 779 / 779 (100.000%) | Complete | |
| [`parser/expressions.rs`](../../../backend/bluejs/src/parser/expressions.rs) | 946 / 946 (100.000%) | 41 / 41 (100.000%) | 1,670 / 1,670 (100.000%) | Complete | |
| [`parser/functions.rs`](../../../backend/bluejs/src/parser/functions.rs) | 666 / 666 (100.000%) | 41 / 41 (100.000%) | 1,048 / 1,048 (100.000%) | Complete | |
| [`parser/module.rs`](../../../backend/bluejs/src/parser/module.rs) | 143 / 143 (100.000%) | 8 / 8 (100.000%) | 243 / 243 (100.000%) | Complete | |
| [`parser/module_items.rs`](../../../backend/bluejs/src/parser/module_items.rs) | 339 / 339 (100.000%) | 13 / 13 (100.000%) | 635 / 635 (100.000%) | Complete | |
| [`parser/patterns.rs`](../../../backend/bluejs/src/parser/patterns.rs) | 175 / 175 (100.000%) | 10 / 10 (100.000%) | 279 / 279 (100.000%) | Complete | |
| [`parser/statements.rs`](../../../backend/bluejs/src/parser/statements.rs) | 589 / 616 (95.617%) | 21 / 21 (100.000%) | 1,137 / 1,177 (96.602%) | Incomplete | |
| [`parser/tests.rs`](../../../backend/bluejs/src/parser/tests.rs) | - | - | - | No counters | Test source; not a coverage target |
| [`primitive.rs`](../../../backend/bluejs/src/primitive.rs) | 277 / 277 (100.000%) | 25 / 25 (100.000%) | 518 / 518 (100.000%) | Complete | |
| [`program_abi.rs`](../../../backend/bluejs/src/program_abi.rs) | 29 / 29 (100.000%) | 4 / 4 (100.000%) | 53 / 53 (100.000%) | Complete | |
| [`program_debug.rs`](../../../backend/bluejs/src/program_debug.rs) | 347 / 347 (100.000%) | 43 / 43 (100.000%) | 398 / 398 (100.000%) | Complete | |
| [`program_debug/ast_ids.rs`](../../../backend/bluejs/src/program_debug/ast_ids.rs) | 484 / 484 (100.000%) | 31 / 31 (100.000%) | 703 / 703 (100.000%) | Complete | |
| [`property.rs`](../../../backend/bluejs/src/property.rs) | 91 / 91 (100.000%) | 22 / 22 (100.000%) | 117 / 117 (100.000%) | Complete | |
| [`regex_backrefs.rs`](../../../backend/bluejs/src/regex_backrefs.rs) | 98 / 98 (100.000%) | 12 / 12 (100.000%) | 186 / 186 (100.000%) | Complete | |
| [`regex_canonicalize.rs`](../../../backend/bluejs/src/regex_canonicalize.rs) | 636 / 636 (100.000%) | 66 / 66 (100.000%) | 1,354 / 1,354 (100.000%) | Complete | |
| [`regex_escapes.rs`](../../../backend/bluejs/src/regex_escapes.rs) | 228 / 228 (100.000%) | 25 / 25 (100.000%) | 484 / 484 (100.000%) | Complete | |
| [`regex_group_names.rs`](../../../backend/bluejs/src/regex_group_names.rs) | 323 / 323 (100.000%) | 39 / 39 (100.000%) | 600 / 600 (100.000%) | Complete | |
| [`regex_worker.rs`](../../../backend/bluejs/src/regex_worker.rs) | 641 / 643 (99.689%) | 76 / 76 (100.000%) | 950 / 955 (99.476%) | Incomplete | |
| [`regexp.rs`](../../../backend/bluejs/src/regexp.rs) | 304 / 304 (100.000%) | 27 / 27 (100.000%) | 468 / 468 (100.000%) | Complete | |
| [`source_encoding.rs`](../../../backend/bluejs/src/source_encoding.rs) | 153 / 153 (100.000%) | 20 / 20 (100.000%) | 295 / 295 (100.000%) | Complete | |
| [`string.rs`](../../../backend/bluejs/src/string.rs) | 107 / 107 (100.000%) | 23 / 23 (100.000%) | 174 / 174 (100.000%) | Complete | |
| [`token.rs`](../../../backend/bluejs/src/token.rs) | 1,381 / 1,381 (100.000%) | 115 / 115 (100.000%) | 2,200 / 2,200 (100.000%) | Complete | |
| [`value.rs`](../../../backend/bluejs/src/value.rs) | 27 / 27 (100.000%) | 5 / 5 (100.000%) | 38 / 38 (100.000%) | Complete | |
| [`vm.rs`](../../../backend/bluejs/src/vm.rs) | 628 / 628 (100.000%) | 59 / 59 (100.000%) | 910 / 910 (100.000%) | Complete | |
| [`vm/builtins.rs`](../../../backend/bluejs/src/vm/builtins.rs) | 320 / 320 (100.000%) | 20 / 20 (100.000%) | 587 / 587 (100.000%) | Complete | |
| [`vm/builtins/arguments.rs`](../../../backend/bluejs/src/vm/builtins/arguments.rs) | 784 / 784 (100.000%) | 73 / 73 (100.000%) | 1,292 / 1,292 (100.000%) | Complete | |
| [`vm/builtins/array_change_by_copy.rs`](../../../backend/bluejs/src/vm/builtins/array_change_by_copy.rs) | 197 / 197 (100.000%) | 14 / 14 (100.000%) | 455 / 456 (99.781%) | Incomplete | |
| [`vm/builtins/array_from_async.rs`](../../../backend/bluejs/src/vm/builtins/array_from_async.rs) | 366 / 366 (100.000%) | 29 / 29 (100.000%) | 889 / 889 (100.000%) | Complete | |
| [`vm/builtins/array_scan.rs`](../../../backend/bluejs/src/vm/builtins/array_scan.rs) | 123 / 123 (100.000%) | 13 / 13 (100.000%) | 185 / 185 (100.000%) | Complete | |
| [`vm/builtins/arrays.rs`](../../../backend/bluejs/src/vm/builtins/arrays.rs) | 1,334 / 1,334 (100.000%) | 80 / 80 (100.000%) | 2,886 / 2,898 (99.586%) | Incomplete | |
| [`vm/builtins/binary_data.rs`](../../../backend/bluejs/src/vm/builtins/binary_data.rs) | 1,659 / 1,659 (100.000%) | 116 / 116 (100.000%) | 2,751 / 2,751 (100.000%) | Complete | |
| [`vm/builtins/collection_iteration.rs`](../../../backend/bluejs/src/vm/builtins/collection_iteration.rs) | 116 / 116 (100.000%) | 8 / 8 (100.000%) | 196 / 200 (98.000%) | Incomplete | |
| [`vm/builtins/collections.rs`](../../../backend/bluejs/src/vm/builtins/collections.rs) | 372 / 372 (100.000%) | 27 / 27 (100.000%) | 750 / 755 (99.338%) | Incomplete | |
| [`vm/builtins/decorators.rs`](../../../backend/bluejs/src/vm/builtins/decorators.rs) | 558 / 558 (100.000%) | 43 / 43 (100.000%) | 1,121 / 1,121 (100.000%) | Complete | |
| [`vm/builtins/dynamic.rs`](../../../backend/bluejs/src/vm/builtins/dynamic.rs) | 717 / 717 (100.000%) | 40 / 40 (100.000%) | 1,308 / 1,308 (100.000%) | Complete | |
| [`vm/builtins/execution.rs`](../../../backend/bluejs/src/vm/builtins/execution.rs) | - | - | - | No counters | Declarations/re-exports only; no executable code |
| [`vm/builtins/execution/closures.rs`](../../../backend/bluejs/src/vm/builtins/execution/closures.rs) | 468 / 468 (100.000%) | 20 / 20 (100.000%) | 832 / 832 (100.000%) | Complete | |
| [`vm/builtins/execution/runtime.rs`](../../../backend/bluejs/src/vm/builtins/execution/runtime.rs) | 1,711 / 1,711 (100.000%) | 102 / 102 (100.000%) | 3,139 / 3,139 (100.000%) | Complete | |
| [`vm/builtins/execution/setup.rs`](../../../backend/bluejs/src/vm/builtins/execution/setup.rs) | 1,088 / 1,088 (100.000%) | 93 / 93 (100.000%) | 1,801 / 1,801 (100.000%) | Complete | |
| [`vm/builtins/general.rs`](../../../backend/bluejs/src/vm/builtins/general.rs) | 424 / 425 (99.765%) | 37 / 37 (100.000%) | 739 / 749 (98.665%) | Incomplete | |
| [`vm/builtins/generators.rs`](../../../backend/bluejs/src/vm/builtins/generators.rs) | 1,620 / 1,621 (99.938%) | 77 / 77 (100.000%) | 2,505 / 2,521 (99.365%) | Incomplete | |
| [`vm/builtins/globals.rs`](../../../backend/bluejs/src/vm/builtins/globals.rs) | 1,006 / 1,006 (100.000%) | 12 / 12 (100.000%) | 1,472 / 1,472 (100.000%) | Complete | |
| [`vm/builtins/immutable_arraybuffer.rs`](../../../backend/bluejs/src/vm/builtins/immutable_arraybuffer.rs) | 160 / 160 (100.000%) | 11 / 11 (100.000%) | 244 / 244 (100.000%) | Complete | |
| [`vm/builtins/math.rs`](../../../backend/bluejs/src/vm/builtins/math.rs) | 436 / 436 (100.000%) | 27 / 27 (100.000%) | 742 / 742 (100.000%) | Complete | |
| [`vm/builtins/native_dispatch.rs`](../../../backend/bluejs/src/vm/builtins/native_dispatch.rs) | - | - | - | No counters | Declarations/re-exports only; no executable code |
| [`vm/builtins/native_dispatch/date.rs`](../../../backend/bluejs/src/vm/builtins/native_dispatch/date.rs) | 687 / 687 (100.000%) | 55 / 55 (100.000%) | 1,230 / 1,232 (99.838%) | Incomplete | |
| [`vm/builtins/native_dispatch/dispatch.rs`](../../../backend/bluejs/src/vm/builtins/native_dispatch/dispatch.rs) | 1,240 / 1,261 (98.335%) | 35 / 37 (94.595%) | 3,062 / 3,150 (97.206%) | Incomplete | |
| [`vm/builtins/numbers.rs`](../../../backend/bluejs/src/vm/builtins/numbers.rs) | 244 / 246 (99.187%) | 15 / 15 (100.000%) | 424 / 431 (98.376%) | Incomplete | |
| [`vm/builtins/numbers/tests.rs`](../../../backend/bluejs/src/vm/builtins/numbers/tests.rs) | - | - | - | No counters | Test source; not a coverage target |
| [`vm/builtins/object.rs`](../../../backend/bluejs/src/vm/builtins/object.rs) | 1,395 / 1,397 (99.857%) | 78 / 78 (100.000%) | 2,390 / 2,396 (99.750%) | Incomplete | |
| [`vm/builtins/promise_combinators.rs`](../../../backend/bluejs/src/vm/builtins/promise_combinators.rs) | 415 / 415 (100.000%) | 34 / 34 (100.000%) | 854 / 854 (100.000%) | Complete | |
| [`vm/builtins/promise_core.rs`](../../../backend/bluejs/src/vm/builtins/promise_core.rs) | 799 / 801 (99.750%) | 67 / 67 (100.000%) | 1,329 / 1,339 (99.253%) | Incomplete | |
| [`vm/builtins/promises.rs`](../../../backend/bluejs/src/vm/builtins/promises.rs) | 986 / 986 (100.000%) | 59 / 59 (100.000%) | 1,639 / 1,641 (99.878%) | Incomplete | |
| [`vm/builtins/resource_management.rs`](../../../backend/bluejs/src/vm/builtins/resource_management.rs) | 546 / 558 (97.849%) | 32 / 34 (94.118%) | 806 / 829 (97.226%) | Incomplete | |
| [`vm/builtins/set_methods.rs`](../../../backend/bluejs/src/vm/builtins/set_methods.rs) | 314 / 314 (100.000%) | 33 / 33 (100.000%) | 627 / 629 (99.682%) | Incomplete | |
| [`vm/builtins/tests.rs`](../../../backend/bluejs/src/vm/builtins/tests.rs) | - | - | - | No counters | Test source; not a coverage target |
| [`vm/builtins/typed_arrays.rs`](../../../backend/bluejs/src/vm/builtins/typed_arrays.rs) | 915 / 917 (99.782%) | 43 / 43 (100.000%) | 1,780 / 1,791 (99.386%) | Incomplete | |
| [`vm/builtins/uint8array.rs`](../../../backend/bluejs/src/vm/builtins/uint8array.rs) | 493 / 508 (97.047%) | 34 / 35 (97.143%) | 743 / 768 (96.745%) | Incomplete | |
| [`vm/completion.rs`](../../../backend/bluejs/src/vm/completion.rs) | 113 / 113 (100.000%) | 9 / 9 (100.000%) | 162 / 162 (100.000%) | Complete | |
| [`vm/debugger.rs`](../../../backend/bluejs/src/vm/debugger.rs) | 773 / 773 (100.000%) | 38 / 38 (100.000%) | 946 / 946 (100.000%) | Complete | |
| [`vm/debugger/inspection.rs`](../../../backend/bluejs/src/vm/debugger/inspection.rs) | 189 / 191 (98.953%) | 6 / 6 (100.000%) | 182 / 192 (94.792%) | Incomplete | |
| [`vm/errors.rs`](../../../backend/bluejs/src/vm/errors.rs) | 406 / 406 (100.000%) | 24 / 24 (100.000%) | 744 / 747 (99.598%) | Incomplete | |
| [`vm/execution.rs`](../../../backend/bluejs/src/vm/execution.rs) | 1,567 / 1,573 (99.619%) | 142 / 142 (100.000%) | 2,543 / 2,558 (99.414%) | Incomplete | |
| [`vm/functions.rs`](../../../backend/bluejs/src/vm/functions.rs) | 434 / 434 (100.000%) | 46 / 46 (100.000%) | 551 / 557 (98.923%) | Incomplete | |
| [`vm/host_objects.rs`](../../../backend/bluejs/src/vm/host_objects.rs) | 525 / 620 (84.677%) | 40 / 58 (68.966%) | 597 / 713 (83.731%) | Incomplete | |
| [`vm/interpreter.rs`](../../../backend/bluejs/src/vm/interpreter.rs) | 1,474 / 1,488 (99.059%) | 60 / 60 (100.000%) | 3,267 / 3,317 (98.493%) | Incomplete | |
| [`vm/intl.rs`](../../../backend/bluejs/src/vm/intl.rs) | 597 / 597 (100.000%) | 18 / 18 (100.000%) | 779 / 779 (100.000%) | Complete | |
| [`vm/intl/collator_locale.rs`](../../../backend/bluejs/src/vm/intl/collator_locale.rs) | 444 / 444 (100.000%) | 41 / 41 (100.000%) | 711 / 711 (100.000%) | Complete | |
| [`vm/intl/date_time.rs`](../../../backend/bluejs/src/vm/intl/date_time.rs) | 827 / 827 (100.000%) | 56 / 56 (100.000%) | 1,226 / 1,226 (100.000%) | Complete | |
| [`vm/intl/list_duration.rs`](../../../backend/bluejs/src/vm/intl/list_duration.rs) | 593 / 618 (95.955%) | 47 / 55 (85.455%) | 891 / 957 (93.103%) | Incomplete | |
| [`vm/intl/number_options.rs`](../../../backend/bluejs/src/vm/intl/number_options.rs) | 446 / 446 (100.000%) | 32 / 32 (100.000%) | 632 / 633 (99.842%) | Incomplete | |
| [`vm/intl/number_runtime.rs`](../../../backend/bluejs/src/vm/intl/number_runtime.rs) | 438 / 438 (100.000%) | 26 / 26 (100.000%) | 586 / 586 (100.000%) | Complete | |
| [`vm/intl/plural_segmenter.rs`](../../../backend/bluejs/src/vm/intl/plural_segmenter.rs) | 585 / 585 (100.000%) | 45 / 45 (100.000%) | 843 / 843 (100.000%) | Complete | |
| [`vm/intl/shared.rs`](../../../backend/bluejs/src/vm/intl/shared.rs) | 719 / 719 (100.000%) | 65 / 65 (100.000%) | 1,168 / 1,168 (100.000%) | Complete | |
| [`vm/intrinsics.rs`](../../../backend/bluejs/src/vm/intrinsics.rs) | 454 / 454 (100.000%) | 5 / 5 (100.000%) | 600 / 600 (100.000%) | Complete | |
| [`vm/json.rs`](../../../backend/bluejs/src/vm/json.rs) | 806 / 818 (98.533%) | 61 / 61 (100.000%) | 1,608 / 1,649 (97.514%) | Incomplete | |
| [`vm/lifecycle.rs`](../../../backend/bluejs/src/vm/lifecycle.rs) | 213 / 213 (100.000%) | 13 / 13 (100.000%) | 228 / 228 (100.000%) | Complete | |
| [`vm/modules.rs`](../../../backend/bluejs/src/vm/modules.rs) | 1,916 / 1,918 (99.896%) | 101 / 101 (100.000%) | 2,965 / 2,969 (99.865%) | Incomplete | |
| [`vm/modules/deferred.rs`](../../../backend/bluejs/src/vm/modules/deferred.rs) | 303 / 303 (100.000%) | 27 / 27 (100.000%) | 475 / 475 (100.000%) | Complete | |
| [`vm/modules/namespace.rs`](../../../backend/bluejs/src/vm/modules/namespace.rs) | 282 / 282 (100.000%) | 20 / 20 (100.000%) | 450 / 450 (100.000%) | Complete | |
| [`vm/modules/synthetic.rs`](../../../backend/bluejs/src/vm/modules/synthetic.rs) | 211 / 211 (100.000%) | 12 / 12 (100.000%) | 317 / 317 (100.000%) | Complete | |
| [`vm/native_stack.rs`](../../../backend/bluejs/src/vm/native_stack.rs) | 97 / 97 (100.000%) | 19 / 19 (100.000%) | 160 / 160 (100.000%) | Complete | |
| [`vm/operations.rs`](../../../backend/bluejs/src/vm/operations.rs) | 728 / 728 (100.000%) | 58 / 58 (100.000%) | 1,341 / 1,341 (100.000%) | Complete | |
| [`vm/properties.rs`](../../../backend/bluejs/src/vm/properties.rs) | 668 / 705 (94.752%) | 65 / 70 (92.857%) | 1,207 / 1,295 (93.205%) | Incomplete | |
| [`vm/realm_reentrancy.rs`](../../../backend/bluejs/src/vm/realm_reentrancy.rs) | 32 / 32 (100.000%) | 11 / 11 (100.000%) | 42 / 42 (100.000%) | Complete | |
| [`vm/regexp.rs`](../../../backend/bluejs/src/vm/regexp.rs) | 1,140 / 1,140 (100.000%) | 65 / 65 (100.000%) | 2,114 / 2,115 (99.953%) | Incomplete | |
| [`vm/shadow_realm.rs`](../../../backend/bluejs/src/vm/shadow_realm.rs) | 446 / 446 (100.000%) | 34 / 34 (100.000%) | 746 / 746 (100.000%) | Complete | |
| [`vm/temporal.rs`](../../../backend/bluejs/src/vm/temporal.rs) | 578 / 578 (100.000%) | 11 / 11 (100.000%) | 568 / 568 (100.000%) | Complete | |
| [`vm/temporal/calendar.rs`](../../../backend/bluejs/src/vm/temporal/calendar.rs) | 147 / 147 (100.000%) | 14 / 14 (100.000%) | 215 / 215 (100.000%) | Complete | |
| [`vm/temporal/conversion.rs`](../../../backend/bluejs/src/vm/temporal/conversion.rs) | - | - | - | No counters | Declarations/re-exports only; no executable code |
| [`vm/temporal/conversion/calendar_fields.rs`](../../../backend/bluejs/src/vm/temporal/conversion/calendar_fields.rs) | 305 / 313 (97.444%) | 25 / 27 (92.593%) | 475 / 485 (97.938%) | Incomplete | |
| [`vm/temporal/conversion/from_value.rs`](../../../backend/bluejs/src/vm/temporal/conversion/from_value.rs) | 369 / 369 (100.000%) | 21 / 21 (100.000%) | 600 / 600 (100.000%) | Complete | |
| [`vm/temporal/conversion/getters.rs`](../../../backend/bluejs/src/vm/temporal/conversion/getters.rs) | 114 / 114 (100.000%) | 4 / 4 (100.000%) | 151 / 151 (100.000%) | Complete | |
| [`vm/temporal/conversion/numeric.rs`](../../../backend/bluejs/src/vm/temporal/conversion/numeric.rs) | 195 / 195 (100.000%) | 24 / 24 (100.000%) | 251 / 251 (100.000%) | Complete | |
| [`vm/temporal/conversion/options.rs`](../../../backend/bluejs/src/vm/temporal/conversion/options.rs) | 130 / 130 (100.000%) | 12 / 12 (100.000%) | 158 / 158 (100.000%) | Complete | |
| [`vm/temporal/conversion/zoned_conversion.rs`](../../../backend/bluejs/src/vm/temporal/conversion/zoned_conversion.rs) | 107 / 131 (81.679%) | 6 / 11 (54.545%) | 182 / 206 (88.350%) | Incomplete | |
| [`vm/temporal/dates.rs`](../../../backend/bluejs/src/vm/temporal/dates.rs) | - | - | - | No counters | Declarations/re-exports only; no executable code |
| [`vm/temporal/dates/arithmetic.rs`](../../../backend/bluejs/src/vm/temporal/dates/arithmetic.rs) | 206 / 206 (100.000%) | 8 / 8 (100.000%) | 281 / 281 (100.000%) | Complete | |
| [`vm/temporal/dates/comparison.rs`](../../../backend/bluejs/src/vm/temporal/dates/comparison.rs) | 49 / 49 (100.000%) | 3 / 3 (100.000%) | 65 / 65 (100.000%) | Complete | |
| [`vm/temporal/dates/construction.rs`](../../../backend/bluejs/src/vm/temporal/dates/construction.rs) | 214 / 214 (100.000%) | 10 / 10 (100.000%) | 231 / 233 (99.142%) | Incomplete | |
| [`vm/temporal/dates/conversions.rs`](../../../backend/bluejs/src/vm/temporal/dates/conversions.rs) | 66 / 66 (100.000%) | 3 / 3 (100.000%) | 108 / 108 (100.000%) | Complete | |
| [`vm/temporal/dates/date_time.rs`](../../../backend/bluejs/src/vm/temporal/dates/date_time.rs) | 124 / 124 (100.000%) | 8 / 8 (100.000%) | 198 / 198 (100.000%) | Complete | |
| [`vm/temporal/dates/formatting.rs`](../../../backend/bluejs/src/vm/temporal/dates/formatting.rs) | 166 / 166 (100.000%) | 5 / 5 (100.000%) | 216 / 216 (100.000%) | Complete | |
| [`vm/temporal/dates/now_and_zone.rs`](../../../backend/bluejs/src/vm/temporal/dates/now_and_zone.rs) | 192 / 192 (100.000%) | 20 / 20 (100.000%) | 266 / 266 (100.000%) | Complete | |
| [`vm/temporal/dates/with.rs`](../../../backend/bluejs/src/vm/temporal/dates/with.rs) | 144 / 144 (100.000%) | 6 / 6 (100.000%) | 262 / 262 (100.000%) | Complete | |
| [`vm/temporal/duration_conversion.rs`](../../../backend/bluejs/src/vm/temporal/duration_conversion.rs) | 48 / 48 (100.000%) | 4 / 4 (100.000%) | 73 / 73 (100.000%) | Complete | |
| [`vm/temporal/duration_math.rs`](../../../backend/bluejs/src/vm/temporal/duration_math.rs) | 305 / 305 (100.000%) | 29 / 29 (100.000%) | 361 / 361 (100.000%) | Complete | |
| [`vm/temporal/duration_operations.rs`](../../../backend/bluejs/src/vm/temporal/duration_operations.rs) | 611 / 611 (100.000%) | 27 / 27 (100.000%) | 874 / 874 (100.000%) | Complete | |
| [`vm/temporal/duration_relative.rs`](../../../backend/bluejs/src/vm/temporal/duration_relative.rs) | 523 / 523 (100.000%) | 42 / 42 (100.000%) | 659 / 659 (100.000%) | Complete | |
| [`vm/temporal/epoch.rs`](../../../backend/bluejs/src/vm/temporal/epoch.rs) | 136 / 136 (100.000%) | 13 / 13 (100.000%) | 237 / 237 (100.000%) | Complete | |
| [`vm/temporal/instant.rs`](../../../backend/bluejs/src/vm/temporal/instant.rs) | 449 / 462 (97.186%) | 25 / 32 (78.125%) | 635 / 667 (95.202%) | Incomplete | |
| [`vm/temporal/iso.rs`](../../../backend/bluejs/src/vm/temporal/iso.rs) | 17 / 17 (100.000%) | 3 / 3 (100.000%) | 24 / 24 (100.000%) | Complete | |
| [`vm/temporal/iso/annotations.rs`](../../../backend/bluejs/src/vm/temporal/iso/annotations.rs) | 271 / 271 (100.000%) | 28 / 28 (100.000%) | 452 / 452 (100.000%) | Complete | |
| [`vm/temporal/iso/datetime.rs`](../../../backend/bluejs/src/vm/temporal/iso/datetime.rs) | 145 / 145 (100.000%) | 15 / 15 (100.000%) | 259 / 259 (100.000%) | Complete | |
| [`vm/temporal/iso/duration.rs`](../../../backend/bluejs/src/vm/temporal/iso/duration.rs) | 117 / 117 (100.000%) | 5 / 5 (100.000%) | 183 / 183 (100.000%) | Complete | |
| [`vm/temporal/iso/offset.rs`](../../../backend/bluejs/src/vm/temporal/iso/offset.rs) | 182 / 182 (100.000%) | 12 / 12 (100.000%) | 349 / 349 (100.000%) | Complete | |
| [`vm/temporal/iso/scan.rs`](../../../backend/bluejs/src/vm/temporal/iso/scan.rs) | 349 / 349 (100.000%) | 38 / 38 (100.000%) | 628 / 628 (100.000%) | Complete | |
| [`vm/temporal/iso/tests.rs`](../../../backend/bluejs/src/vm/temporal/iso/tests.rs) | - | - | - | No counters | Test source; not a coverage target |
| [`vm/temporal/plain_date.rs`](../../../backend/bluejs/src/vm/temporal/plain_date.rs) | - | - | - | No counters | Declarations/re-exports only; no executable code |
| [`vm/temporal/plain_date/calendar_add.rs`](../../../backend/bluejs/src/vm/temporal/plain_date/calendar_add.rs) | 181 / 181 (100.000%) | 6 / 6 (100.000%) | 303 / 303 (100.000%) | Complete | |
| [`vm/temporal/plain_date/calendar_difference.rs`](../../../backend/bluejs/src/vm/temporal/plain_date/calendar_difference.rs) | 210 / 210 (100.000%) | 8 / 8 (100.000%) | 369 / 369 (100.000%) | Complete | |
| [`vm/temporal/plain_date/format.rs`](../../../backend/bluejs/src/vm/temporal/plain_date/format.rs) | 30 / 30 (100.000%) | 4 / 4 (100.000%) | 56 / 56 (100.000%) | Complete | |
| [`vm/temporal/plain_date/iso_date.rs`](../../../backend/bluejs/src/vm/temporal/plain_date/iso_date.rs) | 116 / 116 (100.000%) | 17 / 17 (100.000%) | 207 / 207 (100.000%) | Complete | |
| [`vm/temporal/plain_date/month_structure.rs`](../../../backend/bluejs/src/vm/temporal/plain_date/month_structure.rs) | 161 / 161 (100.000%) | 14 / 14 (100.000%) | 243 / 243 (100.000%) | Complete | |
| [`vm/temporal/plain_date/round_duration.rs`](../../../backend/bluejs/src/vm/temporal/plain_date/round_duration.rs) | 155 / 155 (100.000%) | 10 / 10 (100.000%) | 204 / 204 (100.000%) | Complete | |
| [`vm/temporal/plain_date/tests.rs`](../../../backend/bluejs/src/vm/temporal/plain_date/tests.rs) | - | - | - | No counters | Test source; not a coverage target |
| [`vm/temporal/plain_date_time_difference.rs`](../../../backend/bluejs/src/vm/temporal/plain_date_time_difference.rs) | 454 / 454 (100.000%) | 25 / 25 (100.000%) | 594 / 594 (100.000%) | Complete | |
| [`vm/temporal/plain_month_day.rs`](../../../backend/bluejs/src/vm/temporal/plain_month_day.rs) | 282 / 282 (100.000%) | 28 / 28 (100.000%) | 386 / 386 (100.000%) | Complete | |
| [`vm/temporal/plain_time.rs`](../../../backend/bluejs/src/vm/temporal/plain_time.rs) | 627 / 649 (96.610%) | 39 / 44 (88.636%) | 888 / 922 (96.312%) | Incomplete | |
| [`vm/temporal/plain_year_month.rs`](../../../backend/bluejs/src/vm/temporal/plain_year_month.rs) | 145 / 145 (100.000%) | 13 / 13 (100.000%) | 193 / 193 (100.000%) | Complete | |
| [`vm/temporal/receiver.rs`](../../../backend/bluejs/src/vm/temporal/receiver.rs) | 42 / 42 (100.000%) | 3 / 3 (100.000%) | 43 / 43 (100.000%) | Complete | |
| [`vm/temporal/rounding.rs`](../../../backend/bluejs/src/vm/temporal/rounding.rs) | 405 / 405 (100.000%) | 33 / 33 (100.000%) | 666 / 666 (100.000%) | Complete | |
| [`vm/temporal/time_zone.rs`](../../../backend/bluejs/src/vm/temporal/time_zone.rs) | 519 / 519 (100.000%) | 47 / 47 (100.000%) | 1,031 / 1,031 (100.000%) | Complete | |
| [`vm/temporal/time_zone_id.rs`](../../../backend/bluejs/src/vm/temporal/time_zone_id.rs) | 248 / 248 (100.000%) | 32 / 32 (100.000%) | 546 / 546 (100.000%) | Complete | |
| [`vm/temporal/year_month.rs`](../../../backend/bluejs/src/vm/temporal/year_month.rs) | 992 / 992 (100.000%) | 71 / 71 (100.000%) | 1,553 / 1,553 (100.000%) | Complete | |
| [`vm/temporal/zoned.rs`](../../../backend/bluejs/src/vm/temporal/zoned.rs) | - | - | - | No counters | Declarations/re-exports only; no executable code |
| [`vm/temporal/zoned/arithmetic.rs`](../../../backend/bluejs/src/vm/temporal/zoned/arithmetic.rs) | 177 / 177 (100.000%) | 8 / 8 (100.000%) | 280 / 280 (100.000%) | Complete | |
| [`vm/temporal/zoned/conversions.rs`](../../../backend/bluejs/src/vm/temporal/zoned/conversions.rs) | 121 / 121 (100.000%) | 8 / 8 (100.000%) | 160 / 160 (100.000%) | Complete | |
| [`vm/temporal/zoned/formatting.rs`](../../../backend/bluejs/src/vm/temporal/zoned/formatting.rs) | 170 / 170 (100.000%) | 8 / 8 (100.000%) | 264 / 264 (100.000%) | Complete | |
| [`vm/temporal/zoned/from_value.rs`](../../../backend/bluejs/src/vm/temporal/zoned/from_value.rs) | 190 / 190 (100.000%) | 13 / 13 (100.000%) | 250 / 250 (100.000%) | Complete | |
| [`vm/temporal/zoned/resolution.rs`](../../../backend/bluejs/src/vm/temporal/zoned/resolution.rs) | 112 / 112 (100.000%) | 8 / 8 (100.000%) | 154 / 154 (100.000%) | Complete | |
| [`vm/temporal/zoned/since_until.rs`](../../../backend/bluejs/src/vm/temporal/zoned/since_until.rs) | 146 / 146 (100.000%) | 8 / 8 (100.000%) | 212 / 212 (100.000%) | Complete | |
| [`vm/temporal/zoned/with.rs`](../../../backend/bluejs/src/vm/temporal/zoned/with.rs) | 215 / 215 (100.000%) | 9 / 9 (100.000%) | 371 / 371 (100.000%) | Complete | |
| [`vm/temporal/zoned_date_time.rs`](../../../backend/bluejs/src/vm/temporal/zoned_date_time.rs) | 245 / 245 (100.000%) | 17 / 17 (100.000%) | 405 / 405 (100.000%) | Complete | |
| [`vm/temporal/zoned_difference.rs`](../../../backend/bluejs/src/vm/temporal/zoned_difference.rs) | 409 / 409 (100.000%) | 21 / 21 (100.000%) | 574 / 574 (100.000%) | Complete | |
| [`vm/test262.rs`](../../../backend/bluejs/src/vm/test262.rs) | 65 / 65 (100.000%) | 7 / 7 (100.000%) | 107 / 107 (100.000%) | Complete | |
| [`vm/test262/assertions.rs`](../../../backend/bluejs/src/vm/test262/assertions.rs) | 297 / 299 (99.331%) | 17 / 17 (100.000%) | 825 / 838 (98.449%) | Incomplete | |
| [`vm/test262/cases.rs`](../../../backend/bluejs/src/vm/test262/cases.rs) | 817 / 817 (100.000%) | 59 / 59 (100.000%) | 1,381 / 1,384 (99.783%) | Incomplete | |
| [`vm/test262/foreign.rs`](../../../backend/bluejs/src/vm/test262/foreign.rs) | 1,802 / 1,805 (99.834%) | 149 / 149 (100.000%) | 2,752 / 2,761 (99.674%) | Incomplete | |
| [`vm/test262/harness.rs`](../../../backend/bluejs/src/vm/test262/harness.rs) | 271 / 271 (100.000%) | 12 / 12 (100.000%) | 405 / 405 (100.000%) | Complete | |
| [`vm/test262/reverse.rs`](../../../backend/bluejs/src/vm/test262/reverse.rs) | 681 / 681 (100.000%) | 52 / 52 (100.000%) | 936 / 936 (100.000%) | Complete | |
| [`vm/test262_agents.rs`](../../../backend/bluejs/src/vm/test262_agents.rs) | 487 / 488 (99.795%) | 50 / 50 (100.000%) | 682 / 684 (99.708%) | Incomplete | |
| **Total (162 instrumented files)** | **80,667 / 81,027 (99.556%)** | **5,735 / 5,790 (99.050%)** | **134,679 / 135,518 (99.381%)** | Incomplete | |

### Preservation of previously complete files

**95 / 95 previously complete files remain at 100% raw lines, functions and regions.** The current comparison finds **zero per-file coverage percentage regressions**. The [raw coverage audit](../../../target/test262-macos-20261005-corrections-batch-r28/coverage-audit.json) retains these checks.

## Nine most difficult and thirteen difficult files

**14 / 22 selected files are complete: 4 / 9 D5 files and 10 / 13 D4 files.** **36 / 48 modified production files are complete.** The requested 100% coverage target remains unmet.

### Verification evidence

| Check | Latest result | Evidence |
| --- | --- | --- |
| Complete BlueJS Rust suite | 334 targets; 4,069 passed; zero failed; four existing ignored; 1,303.024 seconds | [Rust results](../../../target/test262-macos-20261005-corrections-batch-r28/rust-results.json) |
| Critical Rust targets | 18 targets; 1,442 passed; zero failed or ignored | [Rust results](../../../target/test262-macos-20261005-corrections-batch-r28/rust-results.json) |
| Complete Test262 inventory | 102,921 applicable modes passed; zero failures; four exclusions; one stale fixture; 274.207 seconds | [Test262 summary](../../../target/test262-macos-20261005-corrections-batch-r28/test262/summary.json) |
| Test262 semantic comparison | 102,926 mode contracts checked; zero changes | [Outcome contract audit](../../../target/test262-macos-20261005-corrections-batch-r28/outcome-contract-audit.json) |
| Workspace Clippy | Passed for all targets with warnings denied | [Static gates](../../../target/test262-macos-20261005-corrections-batch-r28/static-gates.json) |
| Modified Rust formatting and diff checks | All 82 task Rust files formatted; diff check passed | [Static gates](../../../target/test262-macos-20261005-corrections-batch-r28/static-gates.json) |
| Workspace formatting | 20 unchanged files retain existing formatting differences | [Formatting audit](../../../target/test262-macos-20261005-corrections-batch-r28/workspace-format-baseline.json) |
| Remaining workspace runtime tests | Not verified for this source snapshot | Pending |
| BlueJS Rustdoc tests | Not verified for this source snapshot | Pending |

The semantic comparison includes status, expected result, actual error kind and phase, flags, features and fixture hashes. Separately retained diagnostic text differences do not change these outcome contracts.

### Completion of the selected files

| Difficulty | Selected source file | Raw lines | Raw functions | Raw regions | Status |
| --- | --- | ---: | ---: | ---: | --- |
| D5 | [`page_runtime.rs`](../../../backend/bluejs/src/page_runtime.rs) | 1,155 / 1,156 (99.913%) | 78 / 78 (100.000%) | 1,338 / 1,342 (99.702%) | Incomplete |
| D5 | [`vm/builtins/binary_data.rs`](../../../backend/bluejs/src/vm/builtins/binary_data.rs) | 1,659 / 1,659 (100.000%) | 116 / 116 (100.000%) | 2,751 / 2,751 (100.000%) | Complete |
| D5 | [`vm/builtins/execution/closures.rs`](../../../backend/bluejs/src/vm/builtins/execution/closures.rs) | 468 / 468 (100.000%) | 20 / 20 (100.000%) | 832 / 832 (100.000%) | Complete |
| D5 | [`vm/builtins/generators.rs`](../../../backend/bluejs/src/vm/builtins/generators.rs) | 1,620 / 1,621 (99.938%) | 77 / 77 (100.000%) | 2,505 / 2,521 (99.365%) | Incomplete |
| D5 | [`vm/debugger.rs`](../../../backend/bluejs/src/vm/debugger.rs) | 773 / 773 (100.000%) | 38 / 38 (100.000%) | 946 / 946 (100.000%) | Complete |
| D5 | [`vm/execution.rs`](../../../backend/bluejs/src/vm/execution.rs) | 1,567 / 1,573 (99.619%) | 142 / 142 (100.000%) | 2,543 / 2,558 (99.414%) | Incomplete |
| D5 | [`vm/interpreter.rs`](../../../backend/bluejs/src/vm/interpreter.rs) | 1,474 / 1,488 (99.059%) | 60 / 60 (100.000%) | 3,267 / 3,317 (98.493%) | Incomplete |
| D5 | [`vm/shadow_realm.rs`](../../../backend/bluejs/src/vm/shadow_realm.rs) | 446 / 446 (100.000%) | 34 / 34 (100.000%) | 746 / 746 (100.000%) | Complete |
| D5 | [`vm/test262/foreign.rs`](../../../backend/bluejs/src/vm/test262/foreign.rs) | 1,802 / 1,805 (99.834%) | 149 / 149 (100.000%) | 2,752 / 2,761 (99.674%) | Incomplete |
| D4 | [`vm.rs`](../../../backend/bluejs/src/vm.rs) | 628 / 628 (100.000%) | 59 / 59 (100.000%) | 910 / 910 (100.000%) | Complete |
| D4 | [`vm/builtins/array_from_async.rs`](../../../backend/bluejs/src/vm/builtins/array_from_async.rs) | 366 / 366 (100.000%) | 29 / 29 (100.000%) | 889 / 889 (100.000%) | Complete |
| D4 | [`vm/builtins/execution/runtime.rs`](../../../backend/bluejs/src/vm/builtins/execution/runtime.rs) | 1,711 / 1,711 (100.000%) | 102 / 102 (100.000%) | 3,139 / 3,139 (100.000%) | Complete |
| D4 | [`vm/builtins/execution/setup.rs`](../../../backend/bluejs/src/vm/builtins/execution/setup.rs) | 1,088 / 1,088 (100.000%) | 93 / 93 (100.000%) | 1,801 / 1,801 (100.000%) | Complete |
| D4 | [`vm/builtins/object.rs`](../../../backend/bluejs/src/vm/builtins/object.rs) | 1,395 / 1,397 (99.857%) | 78 / 78 (100.000%) | 2,390 / 2,396 (99.750%) | Incomplete |
| D4 | [`vm/modules.rs`](../../../backend/bluejs/src/vm/modules.rs) | 1,916 / 1,918 (99.896%) | 101 / 101 (100.000%) | 2,965 / 2,969 (99.865%) | Incomplete |
| D4 | [`vm/modules/deferred.rs`](../../../backend/bluejs/src/vm/modules/deferred.rs) | 303 / 303 (100.000%) | 27 / 27 (100.000%) | 475 / 475 (100.000%) | Complete |
| D4 | [`vm/modules/namespace.rs`](../../../backend/bluejs/src/vm/modules/namespace.rs) | 282 / 282 (100.000%) | 20 / 20 (100.000%) | 450 / 450 (100.000%) | Complete |
| D4 | [`vm/regexp.rs`](../../../backend/bluejs/src/vm/regexp.rs) | 1,140 / 1,140 (100.000%) | 65 / 65 (100.000%) | 2,114 / 2,115 (99.953%) | Incomplete |
| D4 | [`vm/temporal/conversion/from_value.rs`](../../../backend/bluejs/src/vm/temporal/conversion/from_value.rs) | 369 / 369 (100.000%) | 21 / 21 (100.000%) | 600 / 600 (100.000%) | Complete |
| D4 | [`vm/temporal/duration_relative.rs`](../../../backend/bluejs/src/vm/temporal/duration_relative.rs) | 523 / 523 (100.000%) | 42 / 42 (100.000%) | 659 / 659 (100.000%) | Complete |
| D4 | [`vm/temporal/plain_date_time_difference.rs`](../../../backend/bluejs/src/vm/temporal/plain_date_time_difference.rs) | 454 / 454 (100.000%) | 25 / 25 (100.000%) | 594 / 594 (100.000%) | Complete |
| D4 | [`vm/temporal/zoned_difference.rs`](../../../backend/bluejs/src/vm/temporal/zoned_difference.rs) | 409 / 409 (100.000%) | 21 / 21 (100.000%) | 574 / 574 (100.000%) | Complete |

### Remaining work and limits

**Eight selected files and twelve modified production files remain incomplete.** The modified-file gaps total **32 lines, zero functions and 123 regions**. Passing Rust and Test262 results do not establish the remaining coverage or workspace runtime gates.

| Modified production file | Missing lines | Missing functions | Missing regions | Selection |
| --- | ---: | ---: | ---: | --- |
| [`vm/interpreter.rs`](../../../backend/bluejs/src/vm/interpreter.rs) | 14 | 0 | 50 | D5 |
| [`vm/builtins/generators.rs`](../../../backend/bluejs/src/vm/builtins/generators.rs) | 1 | 0 | 16 | D5 |
| [`vm/execution.rs`](../../../backend/bluejs/src/vm/execution.rs) | 6 | 0 | 15 | D5 |
| [`vm/test262/foreign.rs`](../../../backend/bluejs/src/vm/test262/foreign.rs) | 3 | 0 | 9 | D5 |
| [`page_runtime.rs`](../../../backend/bluejs/src/page_runtime.rs) | 1 | 0 | 4 | D5 |
| [`vm/builtins/object.rs`](../../../backend/bluejs/src/vm/builtins/object.rs) | 2 | 0 | 6 | D4 |
| [`vm/modules.rs`](../../../backend/bluejs/src/vm/modules.rs) | 2 | 0 | 4 | D4 |
| [`vm/regexp.rs`](../../../backend/bluejs/src/vm/regexp.rs) | 0 | 0 | 1 | D4 |
| [`vm/builtins/typed_arrays.rs`](../../../backend/bluejs/src/vm/builtins/typed_arrays.rs) | 2 | 0 | 11 | Shared dependency |
| [`vm/builtins/promises.rs`](../../../backend/bluejs/src/vm/builtins/promises.rs) | 0 | 0 | 2 | Shared dependency |
| [`vm/errors.rs`](../../../backend/bluejs/src/vm/errors.rs) | 0 | 0 | 3 | Shared dependency |
| [`vm/test262_agents.rs`](../../../backend/bluejs/src/vm/test262_agents.rs) | 1 | 0 | 2 | Shared dependency |

Exact uncovered locations and compiled-function contexts are retained in the [raw gap analysis](../../../target/test262-macos-20261005-corrections-batch-r28/best-instantiation-gaps.json). Implementation details are in [COVERAGE_BATCH_ANALYSIS.md](COVERAGE_BATCH_ANALYSIS.md).

## Difficulty ranking of the remaining incomplete files

All **43 currently incomplete files** are ranked from most difficult to easiest. Difficulty estimates reflect the state, realms, resource limits and counter analysis involved; within each level, files are ordered by current missing regions. Every gap count comes from this report's current raw measurement.

### Difficulty groups

| Level | Work type | Incomplete files |
| --- | --- | ---: |
| D5 | Frames and realms: most difficult | 5 |
| D4 | State and algorithms: difficult | 3 |
| D3 | Host behavior and resources | 8 |
| D2 | Multiple helpers and validation | 16 |
| D1 | Local behavior and boundaries | 9 |
| D0 | Compiled-counter reconciliation | 2 |

### Complete ranking

| Rank | Difficulty | Source file | Missing lines | Missing functions | Missing regions |
| ---: | --- | --- | ---: | ---: | ---: |
| 1 | D5 | [`vm/interpreter.rs`](../../../backend/bluejs/src/vm/interpreter.rs) | 14 | 0 | 50 |
| 2 | D5 | [`vm/builtins/generators.rs`](../../../backend/bluejs/src/vm/builtins/generators.rs) | 1 | 0 | 16 |
| 3 | D5 | [`vm/execution.rs`](../../../backend/bluejs/src/vm/execution.rs) | 6 | 0 | 15 |
| 4 | D5 | [`vm/test262/foreign.rs`](../../../backend/bluejs/src/vm/test262/foreign.rs) | 3 | 0 | 9 |
| 5 | D5 | [`page_runtime.rs`](../../../backend/bluejs/src/page_runtime.rs) | 1 | 0 | 4 |
| 6 | D4 | [`vm/builtins/object.rs`](../../../backend/bluejs/src/vm/builtins/object.rs) | 2 | 0 | 6 |
| 7 | D4 | [`vm/modules.rs`](../../../backend/bluejs/src/vm/modules.rs) | 2 | 0 | 4 |
| 8 | D4 | [`vm/regexp.rs`](../../../backend/bluejs/src/vm/regexp.rs) | 0 | 0 | 1 |
| 9 | D3 | [`vm/host_objects.rs`](../../../backend/bluejs/src/vm/host_objects.rs) | 95 | 18 | 116 |
| 10 | D3 | [`vm/builtins/native_dispatch/dispatch.rs`](../../../backend/bluejs/src/vm/builtins/native_dispatch/dispatch.rs) | 21 | 2 | 88 |
| 11 | D3 | [`vm/properties.rs`](../../../backend/bluejs/src/vm/properties.rs) | 37 | 5 | 88 |
| 12 | D3 | [`vm/intl/list_duration.rs`](../../../backend/bluejs/src/vm/intl/list_duration.rs) | 25 | 8 | 66 |
| 13 | D3 | [`vm/builtins/typed_arrays.rs`](../../../backend/bluejs/src/vm/builtins/typed_arrays.rs) | 2 | 0 | 11 |
| 14 | D3 | [`vm/builtins/promise_core.rs`](../../../backend/bluejs/src/vm/builtins/promise_core.rs) | 2 | 0 | 10 |
| 15 | D3 | [`vm/debugger/inspection.rs`](../../../backend/bluejs/src/vm/debugger/inspection.rs) | 2 | 0 | 10 |
| 16 | D3 | [`vm/builtins/promises.rs`](../../../backend/bluejs/src/vm/builtins/promises.rs) | 0 | 0 | 2 |
| 17 | D2 | [`vm/temporal/plain_time.rs`](../../../backend/bluejs/src/vm/temporal/plain_time.rs) | 22 | 5 | 34 |
| 18 | D2 | [`vm/temporal/instant.rs`](../../../backend/bluejs/src/vm/temporal/instant.rs) | 13 | 7 | 32 |
| 19 | D2 | [`ast/retained_payload.rs`](../../../backend/bluejs/src/ast/retained_payload.rs) | 5 | 0 | 25 |
| 20 | D2 | [`vm/temporal/conversion/zoned_conversion.rs`](../../../backend/bluejs/src/vm/temporal/conversion/zoned_conversion.rs) | 24 | 5 | 24 |
| 21 | D2 | [`vm/builtins/arrays.rs`](../../../backend/bluejs/src/vm/builtins/arrays.rs) | 0 | 0 | 12 |
| 22 | D2 | [`vm/builtins/general.rs`](../../../backend/bluejs/src/vm/builtins/general.rs) | 1 | 0 | 10 |
| 23 | D2 | [`vm/temporal/conversion/calendar_fields.rs`](../../../backend/bluejs/src/vm/temporal/conversion/calendar_fields.rs) | 8 | 2 | 10 |
| 24 | D2 | [`vm/functions.rs`](../../../backend/bluejs/src/vm/functions.rs) | 0 | 0 | 6 |
| 25 | D2 | [`vm/builtins/collections.rs`](../../../backend/bluejs/src/vm/builtins/collections.rs) | 0 | 0 | 5 |
| 26 | D2 | [`vm/builtins/collection_iteration.rs`](../../../backend/bluejs/src/vm/builtins/collection_iteration.rs) | 0 | 0 | 4 |
| 27 | D2 | [`compiler.rs`](../../../backend/bluejs/src/compiler.rs) | 0 | 0 | 3 |
| 28 | D2 | [`vm/errors.rs`](../../../backend/bluejs/src/vm/errors.rs) | 0 | 0 | 3 |
| 29 | D2 | [`vm/test262/cases.rs`](../../../backend/bluejs/src/vm/test262/cases.rs) | 0 | 0 | 3 |
| 30 | D2 | [`vm/builtins/native_dispatch/date.rs`](../../../backend/bluejs/src/vm/builtins/native_dispatch/date.rs) | 0 | 0 | 2 |
| 31 | D2 | [`vm/builtins/set_methods.rs`](../../../backend/bluejs/src/vm/builtins/set_methods.rs) | 0 | 0 | 2 |
| 32 | D2 | [`vm/intl/number_options.rs`](../../../backend/bluejs/src/vm/intl/number_options.rs) | 0 | 0 | 1 |
| 33 | D1 | [`vm/json.rs`](../../../backend/bluejs/src/vm/json.rs) | 12 | 0 | 41 |
| 34 | D1 | [`parser/statements.rs`](../../../backend/bluejs/src/parser/statements.rs) | 27 | 0 | 40 |
| 35 | D1 | [`vm/builtins/uint8array.rs`](../../../backend/bluejs/src/vm/builtins/uint8array.rs) | 15 | 1 | 25 |
| 36 | D1 | [`vm/builtins/resource_management.rs`](../../../backend/bluejs/src/vm/builtins/resource_management.rs) | 12 | 2 | 23 |
| 37 | D1 | [`vm/test262/assertions.rs`](../../../backend/bluejs/src/vm/test262/assertions.rs) | 2 | 0 | 13 |
| 38 | D1 | [`heap/object_storage.rs`](../../../backend/bluejs/src/heap/object_storage.rs) | 1 | 0 | 8 |
| 39 | D1 | [`vm/builtins/numbers.rs`](../../../backend/bluejs/src/vm/builtins/numbers.rs) | 2 | 0 | 7 |
| 40 | D1 | [`regex_worker.rs`](../../../backend/bluejs/src/regex_worker.rs) | 2 | 0 | 5 |
| 41 | D1 | [`vm/test262_agents.rs`](../../../backend/bluejs/src/vm/test262_agents.rs) | 1 | 0 | 2 |
| 42 | D0 | [`vm/temporal/dates/construction.rs`](../../../backend/bluejs/src/vm/temporal/dates/construction.rs) | 0 | 0 | 2 |
| 43 | D0 | [`vm/builtins/array_change_by_copy.rs`](../../../backend/bluejs/src/vm/builtins/array_change_by_copy.rs) | 0 | 0 | 1 |

## Reproduce the current Test262 inventory

Use the pinned corpus and the binaries built from the measured source snapshot. Add `--fetch` only for a fresh corpus destination.

```sh
cargo build -p blueice-bluejs --bins --offline
python3 backend/bluejs/test262/run.py \
  --corpus /tmp/blueice-test262-72faf8ec-20261001 \
  --adapter target/debug/bluejs-test262 \
  --jobs 8 --output target/test262-macos-current --progress-interval 60
python3 backend/bluejs/test262/analyze.py \
  --run target/test262-macos-current \
  --corpus /tmp/blueice-test262-72faf8ec-20261001 \
  --output target/test262-macos-current-analysis
```

The runner validates the corpus marker, manifest and every fixture hash. Inspect the summary and inventory dispositions when its exit code is 1. Keep the host otherwise idle because ordinary modes have a two-second wall deadline.

For the full Rust, Test262 and coverage measurement, the retained [verification driver](../../../target/test262-macos-20261005-corrections-batch-r28/verify_batch.py) and [coverage export driver](../../../target/test262-macos-20261005-corrections-batch-r28/export_batch.py) record the commands and atomic-profile configuration.
