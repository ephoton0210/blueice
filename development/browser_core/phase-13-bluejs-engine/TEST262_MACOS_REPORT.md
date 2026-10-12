# macOS Test262 Report

**Measurement: 2026-10-12T05:48:12.469868+00:00. All numbers use this one verified source snapshot.**

## Current complete inventory

Source fingerprint: `f90cc40a8a0619a26b24acbf38ef5f0d63cbc0fd12eb239fb3970ce30cd4e992`. Adapter SHA-256: `43cf5d9361d5096b74b2f12eaa8e7d67e7f237cda164725c19e7f9298d4883f6`. Pinned Test262 revision: `72faf8ec1445c55149615e8b35187830783aba1a`.

**102,921 applicable modes passed**, across 102,926 scheduled modes and 53,582 test files, in 550.335 seconds. The scheduled inventory retains 4 host exclusions and 1 stale corpus modes. These modes are not counted as passes.

The runner returns 1 when any scheduled status is not `pass`. The command record retains that exit; semantic outcomes determine the gate.

Test262 has no official Core classification. This report defines ECMA-262 Core as `language/` plus `built-ins/`, the complete ECMA-262 scope as Core plus `annexB/` and `staging/`, and ECMA-402 as `intl402/`. All pass-rate denominators include the scheduled dispositions in their row.

| Scope | Scheduled | Pass | Fail | Unsupported | Excluded | Stale corpus | Timeout | Harness error | Raw pass rate |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| ECMA-262 Core (`language/` + `built-ins/`) | 91,820 | 91,816 | 0 | 0 | 4 | 0 | 0 | 0 | 99.996% |
| Complete ECMA-262 scope (Core + `annexB/` + `staging/`) | 95,980 | 95,975 | 0 | 0 | 4 | 1 | 0 | 0 | 99.995% |
| ECMA-402 (`intl402/`) | 6,714 | 6,714 | 0 | 0 | 0 | 0 | 0 | 0 | 100% |
| Test262 harness support (`harness/`) | 232 | 232 | 0 | 0 | 0 | 0 | 0 | 0 | 100% |
| **All Test262 runner modes** | 102,926 | 102,921 | 0 | 0 | 4 | 1 | 0 | 0 | 99.995% |

| Top-level Test262 group | Scheduled | Pass | Fail | Unsupported | Excluded | Stale corpus | Timeout | Harness error | Raw pass rate |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `language/` | 44,497 | 44,497 | 0 | 0 | 0 | 0 | 0 | 0 | 100% |
| `built-ins/` | 47,323 | 47,319 | 0 | 0 | 4 | 0 | 0 | 0 | 99.992% |
| `annexB/` | 1,377 | 1,376 | 0 | 0 | 0 | 1 | 0 | 0 | 99.927% |
| `staging/` | 2,783 | 2,783 | 0 | 0 | 0 | 0 | 0 | 0 | 100% |
| `intl402/` | 6,714 | 6,714 | 0 | 0 | 0 | 0 | 0 | 0 | 100% |
| `harness/` | 232 | 232 | 0 | 0 | 0 | 0 | 0 | 0 | 100% |

### Non-pass inventory dispositions

| Path | Modes | Status | Reason |
| --- | --- | --- | --- |
| `annexB/language/function-code/block-decl-func-skip-arguments.js` | 1 | stale_corpus | contradicts the current FunctionDeclarationInstantiation Annex B web-compat insertion point (verified against the live spec text); see tc39/test262#5113, fix pending in tc39/test262#5112 |
| `built-ins/Atomics/wait/bigint/cannot-suspend-throws.js` | 2 | excluded | host declares [[CanBlock]] = true, fixture requires CanBlockIsFalse |
| `built-ins/Atomics/wait/cannot-suspend-throws.js` | 2 | excluded | host declares [[CanBlock]] = true, fixture requires CanBlockIsFalse |

### Corrected crash regressions

| Crash regression | Scheduled | Pass | Fail | Unsupported | Excluded | Stale corpus | Timeout | Harness error | Raw pass rate |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `built-ins/Array/prototype/push/S15.4.4.7_A3.js` | 2 | 2 | 0 | 0 | 0 | 0 | 0 | 0 | 100% |
| `built-ins/Proxy/has/null-handler.js` | 2 | 2 | 0 | 0 | 0 | 0 | 0 | 0 | 100% |
| `built-ins/Proxy/has/null-handler-using-with.js` | 1 | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 100% |

## Selected results

| Selection | Scheduled | Pass | Fail | Unsupported | Excluded | Stale corpus | Timeout | Harness error | Raw pass rate |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `built-ins/Array/` | 6,117 | 6,117 | 0 | 0 | 0 | 0 | 0 | 0 | 100% |
| `built-ins/TypedArray/` + `built-ins/TypedArrayConstructors/` | 4,322 | 4,322 | 0 | 0 | 0 | 0 | 0 | 0 | 100% |
| `built-ins/ArrayBuffer/` | 442 | 442 | 0 | 0 | 0 | 0 | 0 | 0 | 100% |
| `built-ins/SharedArrayBuffer/` | 208 | 208 | 0 | 0 | 0 | 0 | 0 | 0 | 100% |
| `built-ins/DataView/` | 1,122 | 1,122 | 0 | 0 | 0 | 0 | 0 | 0 | 100% |
| `built-ins/Atomics/` | 778 | 774 | 0 | 0 | 4 | 0 | 0 | 0 | 99.486% |
| `built-ins/Iterator/` | 1,308 | 1,308 | 0 | 0 | 0 | 0 | 0 | 0 | 100% |
| `built-ins/Promise/` | 1,458 | 1,458 | 0 | 0 | 0 | 0 | 0 | 0 | 100% |
| `built-ins/ShadowRealm/` | 124 | 124 | 0 | 0 | 0 | 0 | 0 | 0 | 100% |
| `built-ins/AsyncDisposableStack/` | 208 | 208 | 0 | 0 | 0 | 0 | 0 | 0 | 100% |
| `built-ins/DisposableStack/` | 186 | 186 | 0 | 0 | 0 | 0 | 0 | 0 | 100% |
| `built-ins/Temporal/` | 9,210 | 9,210 | 0 | 0 | 0 | 0 | 0 | 0 | 100% |
| `language/import/` | 135 | 135 | 0 | 0 | 0 | 0 | 0 | 0 | 100% |
| `language/expressions/dynamic-import/` | 1,900 | 1,900 | 0 | 0 | 0 | 0 | 0 | 0 | 100% |
| `language/module-code/` | 602 | 602 | 0 | 0 | 0 | 0 | 0 | 0 | 100% |
| `language/statements/using/` | 154 | 154 | 0 | 0 | 0 | 0 | 0 | 0 | 100% |
| `language/statements/await-using/` | 188 | 188 | 0 | 0 | 0 | 0 | 0 | 0 | 100% |
| `language/statements/for-await-of/` | 2,431 | 2,431 | 0 | 0 | 0 | 0 | 0 | 0 | 100% |
| `language/eval-code/` | 454 | 454 | 0 | 0 | 0 | 0 | 0 | 0 | 100% |

## ECMA-402 and Temporal breakdown

| Intl group | Scheduled | Pass | Fail | Unsupported | Excluded | Stale corpus | Timeout | Harness error | Raw pass rate |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `intl402/Temporal/` | 4,058 | 4,058 | 0 | 0 | 0 | 0 | 0 | 0 | 100% |
| `intl402/NumberFormat/` | 498 | 498 | 0 | 0 | 0 | 0 | 0 | 0 | 100% |
| `intl402/DateTimeFormat/` | 488 | 488 | 0 | 0 | 0 | 0 | 0 | 0 | 100% |
| `intl402/Locale/` | 336 | 336 | 0 | 0 | 0 | 0 | 0 | 0 | 100% |
| `intl402/DurationFormat/` | 220 | 220 | 0 | 0 | 0 | 0 | 0 | 0 | 100% |
| `intl402/ListFormat/` | 162 | 162 | 0 | 0 | 0 | 0 | 0 | 0 | 100% |
| `intl402/RelativeTimeFormat/` | 160 | 160 | 0 | 0 | 0 | 0 | 0 | 0 | 100% |
| `intl402/Segmenter/` | 158 | 158 | 0 | 0 | 0 | 0 | 0 | 0 | 100% |
| `intl402/Intl/` | 132 | 132 | 0 | 0 | 0 | 0 | 0 | 0 | 100% |
| `intl402/Collator/` | 130 | 130 | 0 | 0 | 0 | 0 | 0 | 0 | 100% |
| `intl402/DisplayNames/` | 114 | 114 | 0 | 0 | 0 | 0 | 0 | 0 | 100% |
| `intl402/PluralRules/` | 106 | 106 | 0 | 0 | 0 | 0 | 0 | 0 | 100% |
| Top-level Intl fixtures and locale methods on other built-ins | 152 | 152 | 0 | 0 | 0 | 0 | 0 | 0 | 100% |
| **Total ECMA-402** | 6,714 | 6,714 | 0 | 0 | 0 | 0 | 0 | 0 | 100% |

| Temporal type | Scheduled | Pass | Fail | Unsupported | Excluded | Stale corpus | Timeout | Harness error | Raw pass rate |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Duration | 1,122 | 1,122 | 0 | 0 | 0 | 0 | 0 | 0 | 100% |
| Instant | 968 | 968 | 0 | 0 | 0 | 0 | 0 | 0 | 100% |
| Now | 138 | 138 | 0 | 0 | 0 | 0 | 0 | 0 | 100% |
| PlainDate | 2,290 | 2,290 | 0 | 0 | 0 | 0 | 0 | 0 | 100% |
| PlainDateTime | 2,512 | 2,512 | 0 | 0 | 0 | 0 | 0 | 0 | 100% |
| PlainMonthDay | 578 | 578 | 0 | 0 | 0 | 0 | 0 | 0 | 100% |
| PlainTime | 1,010 | 1,010 | 0 | 0 | 0 | 0 | 0 | 0 | 100% |
| PlainYearMonth | 1,672 | 1,672 | 0 | 0 | 0 | 0 | 0 | 0 | 100% |
| ZonedDateTime | 2,968 | 2,968 | 0 | 0 | 0 | 0 | 0 | 0 | 100% |
| toStringTag | 4 | 4 | 0 | 0 | 0 | 0 | 0 | 0 | 100% |
| Temporal root files | 6 | 6 | 0 | 0 | 0 | 0 | 0 | 0 | 100% |
| **Total Temporal** | 13,268 | 13,268 | 0 | 0 | 0 | 0 | 0 | 0 | 100% |

## BlueJS per-file coverage

All counters come from fresh atomic LLVM profiles for the current full run. Completion requires 100% raw lines, functions and regions.

| Metric | Current result |
| --- | --- |
| Lines | 81,480 / 81,480 (100%) |
| Functions | 5,780 / 5,780 (100%) |
| Regions | 135,695 / 135,722 (99.980%) |
| Complete instrumented files | 161 / 169 |
| Incomplete instrumented files | 8 |
| Originally complete files preserved | 95 / 95 |
| Per-file coverage percentage regressions | 0 |
| Modified production files complete | 83 / 83 |

`parser/statements.rs` is complete: **579 / 579 lines, 21 / 21 functions and 1,124 / 1,124 regions (100% each)**. The smaller denominators, previously 616 lines and 1,177 regions, reflect removal of redundant checks whose preconditions were already established by statement dispatch, binding validation or semicolon lookahead. [Public parser regressions](../../../backend/bluejs/tests/parser_statements_coverage.rs) cover reserved labels, static-block bindings, malformed declaration heads including a second lexical `let` binding, automatic semicolon insertion and AST forms; [unit regressions](../../../backend/bluejs/src/parser/tests.rs) cover nested and truncated iteration lookahead.

`vm/test262/assertions.rs` and `heap/object_storage.rs` are complete in all three raw metrics in the main table. [Assertion boundary contracts](../../../backend/bluejs/tests/fixtures/test262_assertion_boundaries.rs) cover foreign handles, caller-root restoration and failed Test262Error construction followed by retry. [Object storage boundary contracts](../../../backend/bluejs/tests/fixtures/heap_object_storage_boundaries.rs) cover Symbol receiver validation, virtual Array/String properties, non-enumerable and accessor descriptors, and malformed namespace/arguments/backing-buffer links in both library instantiations. [Public harness checks](../../../backend/bluejs/tests/test262_host/harness.rs) compare replaced JSON globals with upstream assert.js under normal and one-object nurseries; [public helper regressions](../../../backend/bluejs/tests/coverage_test262_cases.rs) also check replaced String results and HTMLDDA classification. Exhaustive helper dispatch removes its impossible fallback, and pure receiver/value lookups reuse validation already established by preceding operations. Mapped-arguments allocation tests now verify both failed writes and successful cell updates.

All **167 other instrumented files retain exactly the same raw line, function and region counts** as the earlier 2026-10-12 measurement. The [per-file preservation audit](../../../target/bluejs-selftest/runs/20261012-123317-51478599/coverage-preservation-audit.json) records every before/after comparison.

A [four-test linked-frame supplement](../../../target/bluejs-selftest/runs/20261012-123317-51478599/getter-supplement/execution.json), compiled from the same snapshot with package codegen units fixed to 256, recovers a public getter's ordinary-library coverage mapping under Rust 1.95. Its fresh profiles are included in the full LLVM merge, and the [original adapter/worker executables](../../../target/bluejs-selftest/runs/20261012-123317-51478599/measured-binaries.json) are retained with hashes matching the Test262 measurement.

| Source file | Raw lines | Raw functions | Raw regions | Status | Note |
| --- | --- | --- | --- | --- | --- |
| [`ast.rs`](../../../backend/bluejs/src/ast.rs) | 940 / 940 (100%) | 95 / 95 (100%) | 1,249 / 1,249 (100%) | Complete |  |
| [`ast/retained_payload.rs`](../../../backend/bluejs/src/ast/retained_payload.rs) | 523 / 523 (100%) | 42 / 42 (100%) | 852 / 852 (100%) | Complete |  |
| [`bin/bluejs-regexp-worker.rs`](../../../backend/bluejs/src/bin/bluejs-regexp-worker.rs) | 3 / 3 (100%) | 1 / 1 (100%) | 3 / 3 (100%) | Complete |  |
| [`bin/bluejs-test262.rs`](../../../backend/bluejs/src/bin/bluejs-test262.rs) | 631 / 631 (100%) | 56 / 56 (100%) | 1,211 / 1,211 (100%) | Complete |  |
| [`bytecode.rs`](../../../backend/bluejs/src/bytecode.rs) | 116 / 116 (100%) | 18 / 18 (100%) | 123 / 123 (100%) | Complete |  |
| [`compiler.rs`](../../../backend/bluejs/src/compiler.rs) | 1,532 / 1,532 (100%) | 147 / 147 (100%) | 2,161 / 2,164 (99.861%) | Incomplete |  |
| [`compiler/expressions.rs`](../../../backend/bluejs/src/compiler/expressions.rs) | 1,544 / 1,544 (100%) | 42 / 42 (100%) | 3,426 / 3,426 (100%) | Complete |  |
| [`compiler/expressions/tests.rs`](../../../backend/bluejs/src/compiler/expressions/tests.rs) | — | — | — | No executable counters | Test source; not a coverage target |
| [`compiler/functions.rs`](../../../backend/bluejs/src/compiler/functions.rs) | 992 / 992 (100%) | 52 / 52 (100%) | 1,835 / 1,835 (100%) | Complete |  |
| [`compiler/functions/tests.rs`](../../../backend/bluejs/src/compiler/functions/tests.rs) | — | — | — | No executable counters | Test source; not a coverage target |
| [`compiler/private_validation.rs`](../../../backend/bluejs/src/compiler/private_validation.rs) | 337 / 337 (100%) | 32 / 32 (100%) | 701 / 701 (100%) | Complete |  |
| [`compiler/private_validation/tests.rs`](../../../backend/bluejs/src/compiler/private_validation/tests.rs) | — | — | — | No executable counters | Test source; not a coverage target |
| [`compiler/statements.rs`](../../../backend/bluejs/src/compiler/statements.rs) | 1,348 / 1,348 (100%) | 69 / 69 (100%) | 2,637 / 2,637 (100%) | Complete |  |
| [`compiler/statements/tests.rs`](../../../backend/bluejs/src/compiler/statements/tests.rs) | — | — | — | No executable counters | Test source; not a coverage target |
| [`heap.rs`](../../../backend/bluejs/src/heap.rs) | 744 / 744 (100%) | 78 / 78 (100%) | 1,190 / 1,190 (100%) | Complete |  |
| [`heap/binary_data.rs`](../../../backend/bluejs/src/heap/binary_data.rs) | 852 / 852 (100%) | 77 / 77 (100%) | 1,307 / 1,307 (100%) | Complete |  |
| [`heap/capabilities.rs`](../../../backend/bluejs/src/heap/capabilities.rs) | 23 / 23 (100%) | 2 / 2 (100%) | 26 / 26 (100%) | Complete |  |
| [`heap/collection_iteration.rs`](../../../backend/bluejs/src/heap/collection_iteration.rs) | 93 / 93 (100%) | 5 / 5 (100%) | 120 / 120 (100%) | Complete |  |
| [`heap/core.rs`](../../../backend/bluejs/src/heap/core.rs) | 1,261 / 1,261 (100%) | 116 / 116 (100%) | 2,546 / 2,546 (100%) | Complete |  |
| [`heap/debugger.rs`](../../../backend/bluejs/src/heap/debugger.rs) | 163 / 163 (100%) | 10 / 10 (100%) | 239 / 239 (100%) | Complete |  |
| [`heap/exotic.rs`](../../../backend/bluejs/src/heap/exotic.rs) | 984 / 984 (100%) | 117 / 117 (100%) | 1,148 / 1,148 (100%) | Complete |  |
| [`heap/lifecycle.rs`](../../../backend/bluejs/src/heap/lifecycle.rs) | 294 / 294 (100%) | 31 / 31 (100%) | 510 / 510 (100%) | Complete |  |
| [`heap/object_storage.rs`](../../../backend/bluejs/src/heap/object_storage.rs) | 1,231 / 1,231 (100%) | 72 / 72 (100%) | 2,614 / 2,614 (100%) | Complete |  |
| [`heap/tests.rs`](../../../backend/bluejs/src/heap/tests.rs) | — | — | — | No executable counters | Test source; not a coverage target |
| [`intl.rs`](../../../backend/bluejs/src/intl.rs) | 124 / 124 (100%) | 19 / 19 (100%) | 188 / 188 (100%) | Complete |  |
| [`lib.rs`](../../../backend/bluejs/src/lib.rs) | — | — | — | No executable counters | Declarations/re-exports only; no executable code |
| [`native.rs`](../../../backend/bluejs/src/native.rs) | 684 / 684 (100%) | 44 / 44 (100%) | 1,231 / 1,231 (100%) | Complete |  |
| [`page_runtime.rs`](../../../backend/bluejs/src/page_runtime.rs) | 1,156 / 1,156 (100%) | 78 / 78 (100%) | 1,342 / 1,342 (100%) | Complete |  |
| [`parser.rs`](../../../backend/bluejs/src/parser.rs) | 568 / 568 (100%) | 77 / 77 (100%) | 779 / 779 (100%) | Complete |  |
| [`parser/expressions.rs`](../../../backend/bluejs/src/parser/expressions.rs) | 946 / 946 (100%) | 41 / 41 (100%) | 1,670 / 1,670 (100%) | Complete |  |
| [`parser/functions.rs`](../../../backend/bluejs/src/parser/functions.rs) | 666 / 666 (100%) | 41 / 41 (100%) | 1,048 / 1,048 (100%) | Complete |  |
| [`parser/module.rs`](../../../backend/bluejs/src/parser/module.rs) | 143 / 143 (100%) | 8 / 8 (100%) | 243 / 243 (100%) | Complete |  |
| [`parser/module_items.rs`](../../../backend/bluejs/src/parser/module_items.rs) | 339 / 339 (100%) | 13 / 13 (100%) | 635 / 635 (100%) | Complete |  |
| [`parser/patterns.rs`](../../../backend/bluejs/src/parser/patterns.rs) | 175 / 175 (100%) | 10 / 10 (100%) | 279 / 279 (100%) | Complete |  |
| [`parser/statements.rs`](../../../backend/bluejs/src/parser/statements.rs) | 579 / 579 (100%) | 21 / 21 (100%) | 1,124 / 1,124 (100%) | Complete |  |
| [`parser/tests.rs`](../../../backend/bluejs/src/parser/tests.rs) | — | — | — | No executable counters | Test source; not a coverage target |
| [`primitive.rs`](../../../backend/bluejs/src/primitive.rs) | 277 / 277 (100%) | 25 / 25 (100%) | 518 / 518 (100%) | Complete |  |
| [`program_abi.rs`](../../../backend/bluejs/src/program_abi.rs) | 29 / 29 (100%) | 4 / 4 (100%) | 53 / 53 (100%) | Complete |  |
| [`program_debug.rs`](../../../backend/bluejs/src/program_debug.rs) | 347 / 347 (100%) | 43 / 43 (100%) | 398 / 398 (100%) | Complete |  |
| [`program_debug/ast_ids.rs`](../../../backend/bluejs/src/program_debug/ast_ids.rs) | 484 / 484 (100%) | 31 / 31 (100%) | 703 / 703 (100%) | Complete |  |
| [`property.rs`](../../../backend/bluejs/src/property.rs) | 91 / 91 (100%) | 22 / 22 (100%) | 117 / 117 (100%) | Complete |  |
| [`regex_backrefs.rs`](../../../backend/bluejs/src/regex_backrefs.rs) | 98 / 98 (100%) | 12 / 12 (100%) | 186 / 186 (100%) | Complete |  |
| [`regex_canonicalize.rs`](../../../backend/bluejs/src/regex_canonicalize.rs) | 636 / 636 (100%) | 66 / 66 (100%) | 1,354 / 1,354 (100%) | Complete |  |
| [`regex_escapes.rs`](../../../backend/bluejs/src/regex_escapes.rs) | 228 / 228 (100%) | 25 / 25 (100%) | 484 / 484 (100%) | Complete |  |
| [`regex_group_names.rs`](../../../backend/bluejs/src/regex_group_names.rs) | 323 / 323 (100%) | 39 / 39 (100%) | 600 / 600 (100%) | Complete |  |
| [`regex_worker.rs`](../../../backend/bluejs/src/regex_worker.rs) | 657 / 657 (100%) | 77 / 77 (100%) | 967 / 967 (100%) | Complete |  |
| [`regexp.rs`](../../../backend/bluejs/src/regexp.rs) | 304 / 304 (100%) | 27 / 27 (100%) | 468 / 468 (100%) | Complete |  |
| [`source_encoding.rs`](../../../backend/bluejs/src/source_encoding.rs) | 153 / 153 (100%) | 20 / 20 (100%) | 295 / 295 (100%) | Complete |  |
| [`string.rs`](../../../backend/bluejs/src/string.rs) | 107 / 107 (100%) | 23 / 23 (100%) | 174 / 174 (100%) | Complete |  |
| [`token.rs`](../../../backend/bluejs/src/token.rs) | 1,381 / 1,381 (100%) | 115 / 115 (100%) | 2,200 / 2,200 (100%) | Complete |  |
| [`value.rs`](../../../backend/bluejs/src/value.rs) | 27 / 27 (100%) | 5 / 5 (100%) | 38 / 38 (100%) | Complete |  |
| [`vm.rs`](../../../backend/bluejs/src/vm.rs) | 624 / 624 (100%) | 57 / 57 (100%) | 903 / 903 (100%) | Complete |  |
| [`vm/builtins.rs`](../../../backend/bluejs/src/vm/builtins.rs) | 320 / 320 (100%) | 20 / 20 (100%) | 587 / 587 (100%) | Complete |  |
| [`vm/builtins/arguments.rs`](../../../backend/bluejs/src/vm/builtins/arguments.rs) | 784 / 784 (100%) | 73 / 73 (100%) | 1,292 / 1,292 (100%) | Complete |  |
| [`vm/builtins/array_change_by_copy.rs`](../../../backend/bluejs/src/vm/builtins/array_change_by_copy.rs) | 197 / 197 (100%) | 14 / 14 (100%) | 456 / 456 (100%) | Complete |  |
| [`vm/builtins/array_from_async.rs`](../../../backend/bluejs/src/vm/builtins/array_from_async.rs) | 366 / 366 (100%) | 29 / 29 (100%) | 889 / 889 (100%) | Complete |  |
| [`vm/builtins/array_scan.rs`](../../../backend/bluejs/src/vm/builtins/array_scan.rs) | 123 / 123 (100%) | 13 / 13 (100%) | 185 / 185 (100%) | Complete |  |
| [`vm/builtins/arrays.rs`](../../../backend/bluejs/src/vm/builtins/arrays.rs) | 1,348 / 1,348 (100%) | 81 / 81 (100%) | 2,915 / 2,915 (100%) | Complete |  |
| [`vm/builtins/binary_data.rs`](../../../backend/bluejs/src/vm/builtins/binary_data.rs) | 1,659 / 1,659 (100%) | 116 / 116 (100%) | 2,751 / 2,751 (100%) | Complete |  |
| [`vm/builtins/collection_iteration.rs`](../../../backend/bluejs/src/vm/builtins/collection_iteration.rs) | 116 / 116 (100%) | 8 / 8 (100%) | 196 / 200 (98.000%) | Incomplete |  |
| [`vm/builtins/collections.rs`](../../../backend/bluejs/src/vm/builtins/collections.rs) | 372 / 372 (100%) | 27 / 27 (100%) | 750 / 755 (99.338%) | Incomplete |  |
| [`vm/builtins/decorators.rs`](../../../backend/bluejs/src/vm/builtins/decorators.rs) | 558 / 558 (100%) | 43 / 43 (100%) | 1,121 / 1,121 (100%) | Complete |  |
| [`vm/builtins/dynamic.rs`](../../../backend/bluejs/src/vm/builtins/dynamic.rs) | 717 / 717 (100%) | 40 / 40 (100%) | 1,308 / 1,308 (100%) | Complete |  |
| [`vm/builtins/execution.rs`](../../../backend/bluejs/src/vm/builtins/execution.rs) | — | — | — | No executable counters | Declarations/re-exports only; no executable code |
| [`vm/builtins/execution/closures.rs`](../../../backend/bluejs/src/vm/builtins/execution/closures.rs) | 468 / 468 (100%) | 20 / 20 (100%) | 832 / 832 (100%) | Complete |  |
| [`vm/builtins/execution/runtime.rs`](../../../backend/bluejs/src/vm/builtins/execution/runtime.rs) | 1,711 / 1,711 (100%) | 102 / 102 (100%) | 3,139 / 3,139 (100%) | Complete |  |
| [`vm/builtins/execution/setup.rs`](../../../backend/bluejs/src/vm/builtins/execution/setup.rs) | 1,088 / 1,088 (100%) | 93 / 93 (100%) | 1,801 / 1,801 (100%) | Complete |  |
| [`vm/builtins/general.rs`](../../../backend/bluejs/src/vm/builtins/general.rs) | 424 / 424 (100%) | 37 / 37 (100%) | 718 / 718 (100%) | Complete |  |
| [`vm/builtins/generators.rs`](../../../backend/bluejs/src/vm/builtins/generators.rs) | 1,653 / 1,653 (100%) | 77 / 77 (100%) | 2,567 / 2,567 (100%) | Complete |  |
| [`vm/builtins/globals.rs`](../../../backend/bluejs/src/vm/builtins/globals.rs) | 1,006 / 1,006 (100%) | 12 / 12 (100%) | 1,472 / 1,472 (100%) | Complete |  |
| [`vm/builtins/immutable_arraybuffer.rs`](../../../backend/bluejs/src/vm/builtins/immutable_arraybuffer.rs) | 160 / 160 (100%) | 11 / 11 (100%) | 244 / 244 (100%) | Complete |  |
| [`vm/builtins/math.rs`](../../../backend/bluejs/src/vm/builtins/math.rs) | 436 / 436 (100%) | 27 / 27 (100%) | 742 / 742 (100%) | Complete |  |
| [`vm/builtins/native_dispatch.rs`](../../../backend/bluejs/src/vm/builtins/native_dispatch.rs) | — | — | — | No executable counters | Declarations/re-exports only; no executable code |
| [`vm/builtins/native_dispatch/date.rs`](../../../backend/bluejs/src/vm/builtins/native_dispatch/date.rs) | 687 / 687 (100%) | 55 / 55 (100%) | 1,230 / 1,232 (99.838%) | Incomplete |  |
| [`vm/builtins/native_dispatch/dispatch.rs`](../../../backend/bluejs/src/vm/builtins/native_dispatch/dispatch.rs) | 1,396 / 1,396 (100%) | 36 / 36 (100%) | 3,185 / 3,185 (100%) | Complete |  |
| [`vm/builtins/numbers.rs`](../../../backend/bluejs/src/vm/builtins/numbers.rs) | 252 / 252 (100%) | 16 / 16 (100%) | 435 / 435 (100%) | Complete |  |
| [`vm/builtins/numbers/tests.rs`](../../../backend/bluejs/src/vm/builtins/numbers/tests.rs) | — | — | — | No executable counters | Test source; not a coverage target |
| [`vm/builtins/object.rs`](../../../backend/bluejs/src/vm/builtins/object.rs) | 1,381 / 1,381 (100%) | 77 / 77 (100%) | 2,381 / 2,381 (100%) | Complete |  |
| [`vm/builtins/promise_combinators.rs`](../../../backend/bluejs/src/vm/builtins/promise_combinators.rs) | 415 / 415 (100%) | 34 / 34 (100%) | 854 / 854 (100%) | Complete |  |
| [`vm/builtins/promise_core.rs`](../../../backend/bluejs/src/vm/builtins/promise_core.rs) | 828 / 828 (100%) | 70 / 70 (100%) | 1,377 / 1,377 (100%) | Complete |  |
| [`vm/builtins/promises.rs`](../../../backend/bluejs/src/vm/builtins/promises.rs) | 987 / 987 (100%) | 59 / 59 (100%) | 1,642 / 1,642 (100%) | Complete |  |
| [`vm/builtins/resource_management.rs`](../../../backend/bluejs/src/vm/builtins/resource_management.rs) | 544 / 544 (100%) | 32 / 32 (100%) | 810 / 810 (100%) | Complete |  |
| [`vm/builtins/set_methods.rs`](../../../backend/bluejs/src/vm/builtins/set_methods.rs) | 314 / 314 (100%) | 33 / 33 (100%) | 627 / 629 (99.682%) | Incomplete |  |
| [`vm/builtins/tests.rs`](../../../backend/bluejs/src/vm/builtins/tests.rs) | — | — | — | No executable counters | Test source; not a coverage target |
| [`vm/builtins/typed_arrays.rs`](../../../backend/bluejs/src/vm/builtins/typed_arrays.rs) | 948 / 948 (100%) | 44 / 44 (100%) | 1,836 / 1,836 (100%) | Complete |  |
| [`vm/builtins/uint8array.rs`](../../../backend/bluejs/src/vm/builtins/uint8array.rs) | 515 / 515 (100%) | 34 / 34 (100%) | 762 / 762 (100%) | Complete |  |
| [`vm/classification.rs`](../../../backend/bluejs/src/vm/classification.rs) | 46 / 46 (100%) | 8 / 8 (100%) | 69 / 69 (100%) | Complete |  |
| [`vm/completion.rs`](../../../backend/bluejs/src/vm/completion.rs) | 113 / 113 (100%) | 9 / 9 (100%) | 162 / 162 (100%) | Complete |  |
| [`vm/debugger.rs`](../../../backend/bluejs/src/vm/debugger.rs) | 773 / 773 (100%) | 38 / 38 (100%) | 946 / 946 (100%) | Complete |  |
| [`vm/debugger/inspection.rs`](../../../backend/bluejs/src/vm/debugger/inspection.rs) | 188 / 188 (100%) | 6 / 6 (100%) | 187 / 187 (100%) | Complete |  |
| [`vm/errors.rs`](../../../backend/bluejs/src/vm/errors.rs) | 387 / 387 (100%) | 22 / 22 (100%) | 717 / 717 (100%) | Complete |  |
| [`vm/execution.rs`](../../../backend/bluejs/src/vm/execution.rs) | 1,562 / 1,562 (100%) | 142 / 142 (100%) | 2,543 / 2,543 (100%) | Complete |  |
| [`vm/for_in.rs`](../../../backend/bluejs/src/vm/for_in.rs) | 183 / 183 (100%) | 23 / 23 (100%) | 364 / 364 (100%) | Complete |  |
| [`vm/functions.rs`](../../../backend/bluejs/src/vm/functions.rs) | 434 / 434 (100%) | 46 / 46 (100%) | 551 / 557 (98.923%) | Incomplete |  |
| [`vm/host_objects.rs`](../../../backend/bluejs/src/vm/host_objects.rs) | 662 / 662 (100%) | 52 / 52 (100%) | 707 / 707 (100%) | Complete |  |
| [`vm/host_registration.rs`](../../../backend/bluejs/src/vm/host_registration.rs) | 15 / 15 (100%) | 4 / 4 (100%) | 22 / 22 (100%) | Complete |  |
| [`vm/interpreter.rs`](../../../backend/bluejs/src/vm/interpreter.rs) | 1,516 / 1,516 (100%) | 61 / 61 (100%) | 3,359 / 3,359 (100%) | Complete |  |
| [`vm/intl.rs`](../../../backend/bluejs/src/vm/intl.rs) | 597 / 597 (100%) | 18 / 18 (100%) | 779 / 779 (100%) | Complete |  |
| [`vm/intl/collator_locale.rs`](../../../backend/bluejs/src/vm/intl/collator_locale.rs) | 444 / 444 (100%) | 41 / 41 (100%) | 711 / 711 (100%) | Complete |  |
| [`vm/intl/date_time.rs`](../../../backend/bluejs/src/vm/intl/date_time.rs) | 827 / 827 (100%) | 56 / 56 (100%) | 1,226 / 1,226 (100%) | Complete |  |
| [`vm/intl/duration_format.rs`](../../../backend/bluejs/src/vm/intl/duration_format.rs) | 391 / 391 (100%) | 29 / 29 (100%) | 586 / 586 (100%) | Complete |  |
| [`vm/intl/list_format.rs`](../../../backend/bluejs/src/vm/intl/list_format.rs) | 251 / 251 (100%) | 26 / 26 (100%) | 395 / 395 (100%) | Complete |  |
| [`vm/intl/number_options.rs`](../../../backend/bluejs/src/vm/intl/number_options.rs) | 446 / 446 (100%) | 32 / 32 (100%) | 633 / 633 (100%) | Complete |  |
| [`vm/intl/number_runtime.rs`](../../../backend/bluejs/src/vm/intl/number_runtime.rs) | 438 / 438 (100%) | 26 / 26 (100%) | 586 / 586 (100%) | Complete |  |
| [`vm/intl/plural_segmenter.rs`](../../../backend/bluejs/src/vm/intl/plural_segmenter.rs) | 585 / 585 (100%) | 45 / 45 (100%) | 843 / 843 (100%) | Complete |  |
| [`vm/intl/shared.rs`](../../../backend/bluejs/src/vm/intl/shared.rs) | 719 / 719 (100%) | 65 / 65 (100%) | 1,168 / 1,168 (100%) | Complete |  |
| [`vm/intrinsics.rs`](../../../backend/bluejs/src/vm/intrinsics.rs) | 454 / 454 (100%) | 5 / 5 (100%) | 600 / 600 (100%) | Complete |  |
| [`vm/json.rs`](../../../backend/bluejs/src/vm/json.rs) | 833 / 833 (100%) | 60 / 60 (100%) | 1,616 / 1,616 (100%) | Complete |  |
| [`vm/lifecycle.rs`](../../../backend/bluejs/src/vm/lifecycle.rs) | 214 / 214 (100%) | 13 / 13 (100%) | 229 / 229 (100%) | Complete |  |
| [`vm/modules.rs`](../../../backend/bluejs/src/vm/modules.rs) | 1,920 / 1,920 (100%) | 101 / 101 (100%) | 2,970 / 2,970 (100%) | Complete |  |
| [`vm/modules/deferred.rs`](../../../backend/bluejs/src/vm/modules/deferred.rs) | 303 / 303 (100%) | 27 / 27 (100%) | 475 / 475 (100%) | Complete |  |
| [`vm/modules/namespace.rs`](../../../backend/bluejs/src/vm/modules/namespace.rs) | 282 / 282 (100%) | 20 / 20 (100%) | 450 / 450 (100%) | Complete |  |
| [`vm/modules/synthetic.rs`](../../../backend/bluejs/src/vm/modules/synthetic.rs) | 211 / 211 (100%) | 12 / 12 (100%) | 317 / 317 (100%) | Complete |  |
| [`vm/native_stack.rs`](../../../backend/bluejs/src/vm/native_stack.rs) | 97 / 97 (100%) | 19 / 19 (100%) | 160 / 160 (100%) | Complete |  |
| [`vm/operations.rs`](../../../backend/bluejs/src/vm/operations.rs) | 758 / 758 (100%) | 58 / 58 (100%) | 1,448 / 1,448 (100%) | Complete |  |
| [`vm/promise_reactions.rs`](../../../backend/bluejs/src/vm/promise_reactions.rs) | 33 / 33 (100%) | 2 / 2 (100%) | 37 / 37 (100%) | Complete |  |
| [`vm/properties.rs`](../../../backend/bluejs/src/vm/properties.rs) | 614 / 614 (100%) | 45 / 45 (100%) | 952 / 952 (100%) | Complete |  |
| [`vm/realm_reentrancy.rs`](../../../backend/bluejs/src/vm/realm_reentrancy.rs) | 32 / 32 (100%) | 11 / 11 (100%) | 42 / 42 (100%) | Complete |  |
| [`vm/regexp.rs`](../../../backend/bluejs/src/vm/regexp.rs) | 1,140 / 1,140 (100%) | 65 / 65 (100%) | 2,115 / 2,115 (100%) | Complete |  |
| [`vm/shadow_realm.rs`](../../../backend/bluejs/src/vm/shadow_realm.rs) | 446 / 446 (100%) | 34 / 34 (100%) | 746 / 746 (100%) | Complete |  |
| [`vm/temporal.rs`](../../../backend/bluejs/src/vm/temporal.rs) | 578 / 578 (100%) | 11 / 11 (100%) | 568 / 568 (100%) | Complete |  |
| [`vm/temporal/calendar.rs`](../../../backend/bluejs/src/vm/temporal/calendar.rs) | 147 / 147 (100%) | 14 / 14 (100%) | 215 / 215 (100%) | Complete |  |
| [`vm/temporal/conversion.rs`](../../../backend/bluejs/src/vm/temporal/conversion.rs) | — | — | — | No executable counters | Declarations/re-exports only; no executable code |
| [`vm/temporal/conversion/calendar_fields.rs`](../../../backend/bluejs/src/vm/temporal/conversion/calendar_fields.rs) | 304 / 304 (100%) | 25 / 25 (100%) | 470 / 470 (100%) | Complete |  |
| [`vm/temporal/conversion/from_value.rs`](../../../backend/bluejs/src/vm/temporal/conversion/from_value.rs) | 369 / 369 (100%) | 21 / 21 (100%) | 600 / 600 (100%) | Complete |  |
| [`vm/temporal/conversion/getters.rs`](../../../backend/bluejs/src/vm/temporal/conversion/getters.rs) | 114 / 114 (100%) | 4 / 4 (100%) | 151 / 151 (100%) | Complete |  |
| [`vm/temporal/conversion/numeric.rs`](../../../backend/bluejs/src/vm/temporal/conversion/numeric.rs) | 195 / 195 (100%) | 24 / 24 (100%) | 251 / 251 (100%) | Complete |  |
| [`vm/temporal/conversion/options.rs`](../../../backend/bluejs/src/vm/temporal/conversion/options.rs) | 241 / 241 (100%) | 24 / 24 (100%) | 287 / 287 (100%) | Complete |  |
| [`vm/temporal/conversion/zoned_conversion.rs`](../../../backend/bluejs/src/vm/temporal/conversion/zoned_conversion.rs) | 99 / 99 (100%) | 6 / 6 (100%) | 165 / 165 (100%) | Complete |  |
| [`vm/temporal/dates.rs`](../../../backend/bluejs/src/vm/temporal/dates.rs) | — | — | — | No executable counters | Declarations/re-exports only; no executable code |
| [`vm/temporal/dates/arithmetic.rs`](../../../backend/bluejs/src/vm/temporal/dates/arithmetic.rs) | 206 / 206 (100%) | 8 / 8 (100%) | 281 / 281 (100%) | Complete |  |
| [`vm/temporal/dates/comparison.rs`](../../../backend/bluejs/src/vm/temporal/dates/comparison.rs) | 49 / 49 (100%) | 3 / 3 (100%) | 65 / 65 (100%) | Complete |  |
| [`vm/temporal/dates/construction.rs`](../../../backend/bluejs/src/vm/temporal/dates/construction.rs) | 214 / 214 (100%) | 10 / 10 (100%) | 231 / 233 (99.142%) | Incomplete |  |
| [`vm/temporal/dates/conversions.rs`](../../../backend/bluejs/src/vm/temporal/dates/conversions.rs) | 66 / 66 (100%) | 3 / 3 (100%) | 108 / 108 (100%) | Complete |  |
| [`vm/temporal/dates/date_time.rs`](../../../backend/bluejs/src/vm/temporal/dates/date_time.rs) | 124 / 124 (100%) | 8 / 8 (100%) | 198 / 198 (100%) | Complete |  |
| [`vm/temporal/dates/formatting.rs`](../../../backend/bluejs/src/vm/temporal/dates/formatting.rs) | 166 / 166 (100%) | 5 / 5 (100%) | 216 / 216 (100%) | Complete |  |
| [`vm/temporal/dates/now_and_zone.rs`](../../../backend/bluejs/src/vm/temporal/dates/now_and_zone.rs) | 192 / 192 (100%) | 20 / 20 (100%) | 264 / 264 (100%) | Complete |  |
| [`vm/temporal/dates/with.rs`](../../../backend/bluejs/src/vm/temporal/dates/with.rs) | 144 / 144 (100%) | 6 / 6 (100%) | 262 / 262 (100%) | Complete |  |
| [`vm/temporal/duration_conversion.rs`](../../../backend/bluejs/src/vm/temporal/duration_conversion.rs) | 48 / 48 (100%) | 4 / 4 (100%) | 73 / 73 (100%) | Complete |  |
| [`vm/temporal/duration_math.rs`](../../../backend/bluejs/src/vm/temporal/duration_math.rs) | 305 / 305 (100%) | 29 / 29 (100%) | 361 / 361 (100%) | Complete |  |
| [`vm/temporal/duration_operations.rs`](../../../backend/bluejs/src/vm/temporal/duration_operations.rs) | 611 / 611 (100%) | 27 / 27 (100%) | 874 / 874 (100%) | Complete |  |
| [`vm/temporal/duration_relative.rs`](../../../backend/bluejs/src/vm/temporal/duration_relative.rs) | 523 / 523 (100%) | 42 / 42 (100%) | 659 / 659 (100%) | Complete |  |
| [`vm/temporal/epoch.rs`](../../../backend/bluejs/src/vm/temporal/epoch.rs) | 146 / 146 (100%) | 15 / 15 (100%) | 249 / 249 (100%) | Complete |  |
| [`vm/temporal/instant.rs`](../../../backend/bluejs/src/vm/temporal/instant.rs) | 401 / 401 (100%) | 21 / 21 (100%) | 565 / 565 (100%) | Complete |  |
| [`vm/temporal/iso.rs`](../../../backend/bluejs/src/vm/temporal/iso.rs) | 17 / 17 (100%) | 3 / 3 (100%) | 24 / 24 (100%) | Complete |  |
| [`vm/temporal/iso/annotations.rs`](../../../backend/bluejs/src/vm/temporal/iso/annotations.rs) | 271 / 271 (100%) | 28 / 28 (100%) | 452 / 452 (100%) | Complete |  |
| [`vm/temporal/iso/datetime.rs`](../../../backend/bluejs/src/vm/temporal/iso/datetime.rs) | 145 / 145 (100%) | 15 / 15 (100%) | 259 / 259 (100%) | Complete |  |
| [`vm/temporal/iso/duration.rs`](../../../backend/bluejs/src/vm/temporal/iso/duration.rs) | 117 / 117 (100%) | 5 / 5 (100%) | 183 / 183 (100%) | Complete |  |
| [`vm/temporal/iso/offset.rs`](../../../backend/bluejs/src/vm/temporal/iso/offset.rs) | 182 / 182 (100%) | 12 / 12 (100%) | 349 / 349 (100%) | Complete |  |
| [`vm/temporal/iso/scan.rs`](../../../backend/bluejs/src/vm/temporal/iso/scan.rs) | 349 / 349 (100%) | 38 / 38 (100%) | 628 / 628 (100%) | Complete |  |
| [`vm/temporal/iso/tests.rs`](../../../backend/bluejs/src/vm/temporal/iso/tests.rs) | — | — | — | No executable counters | Test source; not a coverage target |
| [`vm/temporal/plain_date.rs`](../../../backend/bluejs/src/vm/temporal/plain_date.rs) | — | — | — | No executable counters | Declarations/re-exports only; no executable code |
| [`vm/temporal/plain_date/calendar_add.rs`](../../../backend/bluejs/src/vm/temporal/plain_date/calendar_add.rs) | 181 / 181 (100%) | 6 / 6 (100%) | 303 / 303 (100%) | Complete |  |
| [`vm/temporal/plain_date/calendar_difference.rs`](../../../backend/bluejs/src/vm/temporal/plain_date/calendar_difference.rs) | 210 / 210 (100%) | 8 / 8 (100%) | 369 / 369 (100%) | Complete |  |
| [`vm/temporal/plain_date/format.rs`](../../../backend/bluejs/src/vm/temporal/plain_date/format.rs) | 30 / 30 (100%) | 4 / 4 (100%) | 56 / 56 (100%) | Complete |  |
| [`vm/temporal/plain_date/iso_date.rs`](../../../backend/bluejs/src/vm/temporal/plain_date/iso_date.rs) | 116 / 116 (100%) | 17 / 17 (100%) | 207 / 207 (100%) | Complete |  |
| [`vm/temporal/plain_date/month_structure.rs`](../../../backend/bluejs/src/vm/temporal/plain_date/month_structure.rs) | 161 / 161 (100%) | 14 / 14 (100%) | 243 / 243 (100%) | Complete |  |
| [`vm/temporal/plain_date/round_duration.rs`](../../../backend/bluejs/src/vm/temporal/plain_date/round_duration.rs) | 155 / 155 (100%) | 10 / 10 (100%) | 204 / 204 (100%) | Complete |  |
| [`vm/temporal/plain_date/tests.rs`](../../../backend/bluejs/src/vm/temporal/plain_date/tests.rs) | — | — | — | No executable counters | Test source; not a coverage target |
| [`vm/temporal/plain_date_time_difference.rs`](../../../backend/bluejs/src/vm/temporal/plain_date_time_difference.rs) | 454 / 454 (100%) | 25 / 25 (100%) | 594 / 594 (100%) | Complete |  |
| [`vm/temporal/plain_month_day.rs`](../../../backend/bluejs/src/vm/temporal/plain_month_day.rs) | 282 / 282 (100%) | 28 / 28 (100%) | 386 / 386 (100%) | Complete |  |
| [`vm/temporal/plain_time.rs`](../../../backend/bluejs/src/vm/temporal/plain_time.rs) | 533 / 533 (100%) | 30 / 30 (100%) | 783 / 783 (100%) | Complete |  |
| [`vm/temporal/plain_year_month.rs`](../../../backend/bluejs/src/vm/temporal/plain_year_month.rs) | 145 / 145 (100%) | 13 / 13 (100%) | 193 / 193 (100%) | Complete |  |
| [`vm/temporal/receiver.rs`](../../../backend/bluejs/src/vm/temporal/receiver.rs) | 43 / 43 (100%) | 5 / 5 (100%) | 43 / 43 (100%) | Complete |  |
| [`vm/temporal/receiver_kind.rs`](../../../backend/bluejs/src/vm/temporal/receiver_kind.rs) | 24 / 24 (100%) | 1 / 1 (100%) | 25 / 25 (100%) | Complete |  |
| [`vm/temporal/rounding.rs`](../../../backend/bluejs/src/vm/temporal/rounding.rs) | 405 / 405 (100%) | 33 / 33 (100%) | 666 / 666 (100%) | Complete |  |
| [`vm/temporal/time_zone.rs`](../../../backend/bluejs/src/vm/temporal/time_zone.rs) | 519 / 519 (100%) | 47 / 47 (100%) | 1,031 / 1,031 (100%) | Complete |  |
| [`vm/temporal/time_zone_id.rs`](../../../backend/bluejs/src/vm/temporal/time_zone_id.rs) | 248 / 248 (100%) | 32 / 32 (100%) | 546 / 546 (100%) | Complete |  |
| [`vm/temporal/year_month.rs`](../../../backend/bluejs/src/vm/temporal/year_month.rs) | 992 / 992 (100%) | 71 / 71 (100%) | 1,553 / 1,553 (100%) | Complete |  |
| [`vm/temporal/zoned.rs`](../../../backend/bluejs/src/vm/temporal/zoned.rs) | — | — | — | No executable counters | Declarations/re-exports only; no executable code |
| [`vm/temporal/zoned/arithmetic.rs`](../../../backend/bluejs/src/vm/temporal/zoned/arithmetic.rs) | 177 / 177 (100%) | 8 / 8 (100%) | 280 / 280 (100%) | Complete |  |
| [`vm/temporal/zoned/conversions.rs`](../../../backend/bluejs/src/vm/temporal/zoned/conversions.rs) | 121 / 121 (100%) | 8 / 8 (100%) | 160 / 160 (100%) | Complete |  |
| [`vm/temporal/zoned/formatting.rs`](../../../backend/bluejs/src/vm/temporal/zoned/formatting.rs) | 170 / 170 (100%) | 8 / 8 (100%) | 264 / 264 (100%) | Complete |  |
| [`vm/temporal/zoned/from_value.rs`](../../../backend/bluejs/src/vm/temporal/zoned/from_value.rs) | 190 / 190 (100%) | 13 / 13 (100%) | 250 / 250 (100%) | Complete |  |
| [`vm/temporal/zoned/resolution.rs`](../../../backend/bluejs/src/vm/temporal/zoned/resolution.rs) | 112 / 112 (100%) | 8 / 8 (100%) | 154 / 154 (100%) | Complete |  |
| [`vm/temporal/zoned/since_until.rs`](../../../backend/bluejs/src/vm/temporal/zoned/since_until.rs) | 146 / 146 (100%) | 8 / 8 (100%) | 212 / 212 (100%) | Complete |  |
| [`vm/temporal/zoned/with.rs`](../../../backend/bluejs/src/vm/temporal/zoned/with.rs) | 215 / 215 (100%) | 9 / 9 (100%) | 371 / 371 (100%) | Complete |  |
| [`vm/temporal/zoned_date_time.rs`](../../../backend/bluejs/src/vm/temporal/zoned_date_time.rs) | 245 / 245 (100%) | 17 / 17 (100%) | 405 / 405 (100%) | Complete |  |
| [`vm/temporal/zoned_difference.rs`](../../../backend/bluejs/src/vm/temporal/zoned_difference.rs) | 409 / 409 (100%) | 21 / 21 (100%) | 574 / 574 (100%) | Complete |  |
| [`vm/test262.rs`](../../../backend/bluejs/src/vm/test262.rs) | 65 / 65 (100%) | 7 / 7 (100%) | 107 / 107 (100%) | Complete |  |
| [`vm/test262/assertions.rs`](../../../backend/bluejs/src/vm/test262/assertions.rs) | 308 / 308 (100%) | 17 / 17 (100%) | 836 / 836 (100%) | Complete |  |
| [`vm/test262/cases.rs`](../../../backend/bluejs/src/vm/test262/cases.rs) | 817 / 817 (100%) | 59 / 59 (100%) | 1,381 / 1,384 (99.783%) | Incomplete |  |
| [`vm/test262/foreign.rs`](../../../backend/bluejs/src/vm/test262/foreign.rs) | 1,800 / 1,800 (100%) | 149 / 149 (100%) | 2,756 / 2,756 (100%) | Complete |  |
| [`vm/test262/harness.rs`](../../../backend/bluejs/src/vm/test262/harness.rs) | 271 / 271 (100%) | 12 / 12 (100%) | 405 / 405 (100%) | Complete |  |
| [`vm/test262/reverse.rs`](../../../backend/bluejs/src/vm/test262/reverse.rs) | 681 / 681 (100%) | 52 / 52 (100%) | 936 / 936 (100%) | Complete |  |
| [`vm/test262_agents.rs`](../../../backend/bluejs/src/vm/test262_agents.rs) | 488 / 488 (100%) | 50 / 50 (100%) | 684 / 684 (100%) | Complete |  |

Changed sources in the reviewed no-counter manifest remain separately audited:

| Source file | Reviewed classification |
| --- | --- |
| [`heap/tests.rs`](../../../backend/bluejs/src/heap/tests.rs) | Test source; not a coverage target |
| [`parser/tests.rs`](../../../backend/bluejs/src/parser/tests.rs) | Test source; not a coverage target |

### Verification evidence

| Gate | Current result |
| --- | --- |
| BlueJS Rust | 335 targets; 4,163 passed; 0 failed; 4 ignored |
| Full Test262 | 102,921 passed |
| Semantic comparison | 102,926 mode contracts; 0 changes |
| Workspace runtime and Rustdoc | Passed |

| Current gate | Pass | Fail | Ignored | Wall seconds |
| --- | --- | --- | --- | --- |
| Complete BlueJS Rust | 4,163 | 0 | 4 | 1835.748 |
| Critical Rust targets | 1,527 | 0 | 0 | 310.993 |
| Self-test tooling contracts | 85 | 0 | 0 | 6.419 |
| Workspace runtime tests | 2,170 | 0 | 60 | 757.515 |
| BlueJS Rustdoc tests | 2 | 0 | 0 | 5.620 |

| Static gate | Current result |
| --- | --- |
| Task Rust formatting | Passed |
| Diff whitespace | Passed |
| Workspace Clippy, warnings denied | Passed |

### Remaining modified production files

None. All 83 modified instrumented production files are complete.

**The requested completion gates are satisfied.**

[Frozen source hashes](../../../target/bluejs-selftest/runs/20261012-123317-51478599/source-state.json), [complete run record](../../../target/bluejs-selftest/runs/20261012-123317-51478599/run.json), [raw coverage data](../../../target/bluejs-selftest/runs/20261012-123317-51478599/report-data.json) and [semantic contract audit](../../../target/bluejs-selftest/runs/20261012-123317-51478599/outcome-contract-audit.json) retain this measurement.

## Difficulty ranking of the remaining incomplete files

| Rank | Difficulty | Source file | Missing lines | Missing functions | Missing regions |
| --- | --- | --- | --- | --- | --- |
| 1 | D2 | [`vm/functions.rs`](../../../backend/bluejs/src/vm/functions.rs) | 0 | 0 | 6 |
| 2 | D2 | [`vm/builtins/collections.rs`](../../../backend/bluejs/src/vm/builtins/collections.rs) | 0 | 0 | 5 |
| 3 | D2 | [`vm/builtins/collection_iteration.rs`](../../../backend/bluejs/src/vm/builtins/collection_iteration.rs) | 0 | 0 | 4 |
| 4 | D2 | [`compiler.rs`](../../../backend/bluejs/src/compiler.rs) | 0 | 0 | 3 |
| 5 | D2 | [`vm/test262/cases.rs`](../../../backend/bluejs/src/vm/test262/cases.rs) | 0 | 0 | 3 |
| 6 | D2 | [`vm/builtins/native_dispatch/date.rs`](../../../backend/bluejs/src/vm/builtins/native_dispatch/date.rs) | 0 | 0 | 2 |
| 7 | D2 | [`vm/builtins/set_methods.rs`](../../../backend/bluejs/src/vm/builtins/set_methods.rs) | 0 | 0 | 2 |
| 8 | D2 | [`vm/temporal/dates/construction.rs`](../../../backend/bluejs/src/vm/temporal/dates/construction.rs) | 0 | 0 | 2 |

## Reproduce the current inventory

```sh
python3 -m backend.bluejs.selftest run --mode pipeline --workspace
```

The pipeline runs affected tests first, then the full current inventory and fresh coverage. A partial or stale run cannot publish this report.
