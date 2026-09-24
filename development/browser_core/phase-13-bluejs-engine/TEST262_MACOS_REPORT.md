# macOS Test262 Report

## Current complete inventory (2026-09-25)

The pinned, unfiltered Test262 snapshot was run on macOS 27.0 (build 26A428, Apple silicon) with Rust/Cargo 1.95.0 and Python 3.14.6, using source commit `e9c15268`. The verified corpus revision is `72faf8ec1445c55149615e8b35187830783aba1a` and includes main, proposals, and staging. The complete command was `/tmp/bluejs-conformance-venv/bin/python backend/bluejs/test262/run.py --corpus development/browser_core/reference/test262 --jobs 8 --output target/test262-macos-20260925 --progress-interval 60`, after `cargo build -p blueice-bluejs --bins --offline`; it completed in 235.768 seconds. The runner verified the pinned corpus marker, manifest, and every file before execution. `analyze.py` reconciled all 53,582 files and 102,926 modes against `results.jsonl` and `summary.json`.

Adapter SHA-256: `0b5d02b83748321a63c21f87d3392d111dd59e8948574e6228ce04331a9af4c2`. RegExp worker SHA-256: `d1e0cabbfa8ba438c6f301f7d3fdf070bda2fbd29a1333d7190d27fd68b45dd0`. Runner SHA-256: `d56c75f03b0422f20fea1dcd8a10be3ea81905f4a79d7fead0abb1ca6986d0ba`. The [checked-in summary](test262-summary.json) contains the complete feature and top-level group counts; the full per-mode evidence is in `target/test262-macos-20260925/results.jsonl`.

**Every dispatched, applicable mode passed: 102,921 / 102,921 (100%).** The raw scheduled inventory is **102,921 / 102,926 pass (99.995%)** because 4 modes are `excluded` by this host's declared `[[CanBlock]] = true` capability and 1 pinned fixture is classified `stale_corpus`. None of these 5 modes was dispatched or counted as a pass. There are **0 `fail`, 0 `unsupported`, 0 `timeout`, and 0 `harness_error`** outcomes. The runner intentionally exits 1 whenever any scheduled mode is not `pass`, so its exit code is 1 for this complete, reconciled run. This is a Test262 progress measurement, not proof of complete ECMAScript conformance. The exact five modes and their reasons are listed below and explained in the [analysis report](TEST262_ANALYSIS_REPORT.md).

Test262 has no official ‘Core’ classification. This report defines ECMA-262 Core as `language/` plus `built-ins/`, complete ECMA-262 Test262 scope as Core plus `annexB/` and `staging/`, and ECMA-402 as `intl402/`. `harness/` appears only in the full inventory total. All tables below are derived from this one unfiltered run; pass rates use every scheduled mode in each row as the denominator.

| Scope | Scheduled | Pass | Fail | Unsupported | Excluded | Stale corpus | Timeout | Harness error | Raw pass rate |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| ECMA-262 Core (`language/` + `built-ins/`) | 91,820 | 91,816 | 0 | 0 | 4 | 0 | 0 | 0 | 99.996% |
| Complete ECMA-262 Test262 scope (Core + `annexB/` + `staging/`) | 95,980 | 95,975 | 0 | 0 | 4 | 1 | 0 | 0 | 99.995% |
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
| `annexB/language/function-code/block-decl-func-skip-arguments.js` | 1 | `stale_corpus` | The pinned fixture contradicts the current Annex B `FunctionDeclarationInstantiation` behavior; the upstream correction is tracked in Test262 issue #5113 / PR #5112. |
| `built-ins/Atomics/wait/cannot-suspend-throws.js` | 2 | `excluded` | Fixture requires `CanBlockIsFalse`; this host declares `[[CanBlock]] = true`. |
| `built-ins/Atomics/wait/bigint/cannot-suspend-throws.js` | 2 | `excluded` | Fixture requires `CanBlockIsFalse`; this host declares `[[CanBlock]] = true`. |

## Selected results (same complete run)

These are subsets of the complete JSONL, grouped by exact path prefix. `built-ins/Array/` is that directory alone; an unanchored `--filter built-ins/Array/` would also match four `intl402/Array/` modes.

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

## ECMA-402 and Temporal breakdown (same complete run)

`intl402/` has 6,714 / 6,714 passing modes. This section breaks it down by service and combines its Temporal modes with `built-ins/Temporal/`. These groups are derived from the same complete JSONL.

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
| `intl402/*.js` (top level) and locale methods on `String/`, `Date/`, `BigInt/`, `Number/`, `Array/`, `FallbackSymbol/`, `TypedArray/` | 152 | 152 | 0 | 100% |
| **Total** | **6,714** | **6,714** | **0** | **100%** |

`Temporal/` is an ECMA-262 feature. Its 9,210 `built-ins/` modes plus 4,058 `intl402/` modes give **13,268 / 13,268 pass (100%)**:

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
| `Temporal/` root files | 10 | 10 | 100% |
| **Total** | **13,268** | **13,268** | **100%** |

## Historical complete inventory (2026-09-21)

The earlier macOS 26.6.2 run at `eaeb5c1` recorded 99,899 pass, 3,025 fail, and 2 timeout out of 102,926 modes (97.059%). Its three-platform comparison belongs to that earlier source revision. The current 2026-09-25 inventory above replaces it as the macOS Test262 status.

## Historical verification on this platform (2026-09-21 source revision)

These checks were performed for the earlier `eaeb5c1` source revision. They are retained as historical evidence, not as measurements of the current Test262 run.

| Check | Command | Result | Gate |
| --- | --- | --- | --- |
| Workspace tests | `cargo test --workspace --no-fail-fast` | **2,985 passed, 0 failed**, 5 ignored | all pass |
| Line coverage, workspace (CI `Coverage` job) | `cargo llvm-cov --workspace --ignore-filename-regex 'extension/src/main\.rs$\|frontend-reference/src/main\.rs$\|mcp-server/src/main\.rs$\|mcp-server/src/server\.rs$' --fail-under-lines 90 --summary-only` | 92.32% lines (90,668 / 98,207); functions 93.19%; regions 90.04% | ≥ 90% lines: met; wall 557 s |
| Line coverage, `blueice-bluejs` alone | `cargo llvm-cov -p blueice-bluejs --fail-under-lines 88 --summary-only` | 91.37% lines (58,892 / 64,456); functions 91.69%; regions 88.44% | ≥ 88% lines: met; wall 416 s |
| Line coverage, `blueice-ecma402` alone | `cargo llvm-cov -p blueice-ecma402 --summary-only` | 94.36% lines (9,559 / 10,130); functions 95.29%; regions 91.91% | informational |
| Node differential oracle | `cargo test -p blueice-bluejs --test node_differential -- --ignored` (Node v24.21.0) | **4 / 4 tests pass**: the 22,268-script main corpus plus the 10-script and 67-script matrices (Intl NumberFormat range/locale data) agree with Node | all pass |
| TypeScript compatibility oracle | `npm exec --yes --package typescript@5.9.3 -- env BLUEICE_BLUETSC_ORACLE=tsc cargo test -p blueice-bluets --test typescript_oracle -- --ignored` | **1 / 1 test passes**: all 68 cases (48 compile-and-run cases whose stdout is compared, 20 diagnostic-parity cases; 71 module sources) agree with TypeScript 5.9.3 | all pass |

For that historical measurement, coverage used Homebrew `llvm@22` (LLVM 22.1.8, the same LLVM version as that `rustc`) through `LLVM_COV`/`LLVM_PROFDATA`, since Homebrew's Rust ships without `llvm-tools`. Node 24.21.0 was Homebrew's `node@24` (the default `node` on that machine was 26.7.0, so the oracles were run with `node@24` first in `PATH`, matching CI). The Node oracle initially disagreed with Node 24 on three scripts that assert Node's legacy behaviour for a hook installed on a primitive's prototype (`'a'.match(3)`, `'a'.search('b')`, `'a'.matchAll(true)`); BlueJS follows ECMA-262 and Test262 there, so those lines were removed from the oracle corpus in `fff18c4`.

Line coverage is a different measure from a Test262 pass rate and the two must not be quoted interchangeably: it is the fraction of the Rust source lines that execute during the crates' own test suites. The five ignored tests are the opt-in oracles (four Node differential tests and one TypeScript compatibility test), which the table's last two rows run explicitly.


## Later BlueJS per-file coverage (2026-09-25)

This is a separate BlueJS coverage measurement at commit `b32076f4` with uncommitted changes on Darwin 27.0 (`arm64`), rustc 1.95.0 (59807616e 2026-04-14) and `cargo-llvm-cov 0.9.1`. It measures the Rust test suite independently of the Test262 inventory and historical verification above. `python3 backend/bluejs/coverage_file.py --update-macos-report` cleaned prior LLVM artifacts, ran the complete default BlueJS Rust test suite, and exported fresh per-file JSON. The opt-in Node oracle and external full Test262 runner were not included. Workspace coverage was not remeasured at this revision.

Each measured cell shows covered / instrumented and the coverage rate. All 171 Rust files under `backend/bluejs/src/` are listed: 158 have LLVM counters; 13 use `-` with an individual reason in `Note`. A `0%` result requires a positive instrumented denominator and zero covered units. `☑` means **lines, functions and regions all reach 100%**; `☐` means at least one is below 100%. The total aggregates only instrumented files. Region coverage is separate from branch coverage. To rerun any one file independently, use `python3 backend/bluejs/coverage_file.py ast.rs` (replace `ast.rs` with its source path). Each invocation reruns the entire test suite, since tests outside a file can still exercise it.

| Source file (relative to `backend/bluejs/src/`) | Lines | Functions | Regions | Complete | Note |
| --- | ---: | ---: | ---: | :---: | --- |
| [`ast.rs`](../../../backend/bluejs/src/ast.rs) | 903 / 903 (100.00%) | 90 / 90 (100.00%) | 1,184 / 1,184 (100.00%) | ☑ |  |
| [`bin/bluejs-regexp-worker.rs`](../../../backend/bluejs/src/bin/bluejs-regexp-worker.rs) | 3 / 3 (100.00%) | 1 / 1 (100.00%) | 3 / 3 (100.00%) | ☑ |  |
| [`bin/bluejs-test262.rs`](../../../backend/bluejs/src/bin/bluejs-test262.rs) | 631 / 631 (100.00%) | 56 / 56 (100.00%) | 1,211 / 1,211 (100.00%) | ☑ |  |
| [`bytecode.rs`](../../../backend/bluejs/src/bytecode.rs) | 85 / 85 (100.00%) | 11 / 11 (100.00%) | 88 / 88 (100.00%) | ☑ |  |
| [`compiler.rs`](../../../backend/bluejs/src/compiler.rs) | 1,440 / 1,441 (99.93%) | 137 / 137 (100.00%) | 2,051 / 2,058 (99.66%) | ☐ |  |
| [`compiler/expressions.rs`](../../../backend/bluejs/src/compiler/expressions.rs) | 1,355 / 1,487 (91.12%) | 36 / 36 (100.00%) | 2,869 / 3,480 (82.44%) | ☐ |  |
| [`compiler/functions.rs`](../../../backend/bluejs/src/compiler/functions.rs) | 966 / 984 (98.17%) | 51 / 51 (100.00%) | 1,660 / 1,816 (91.41%) | ☐ |  |
| [`compiler/private_validation.rs`](../../../backend/bluejs/src/compiler/private_validation.rs) | 334 / 340 (98.24%) | 32 / 32 (100.00%) | 643 / 701 (91.73%) | ☐ |  |
| [`compiler/statements.rs`](../../../backend/bluejs/src/compiler/statements.rs) | 1,294 / 1,351 (95.78%) | 68 / 70 (97.14%) | 2,354 / 2,612 (90.12%) | ☐ |  |
| [`heap.rs`](../../../backend/bluejs/src/heap.rs) | 698 / 746 (93.57%) | 77 / 78 (98.72%) | 1,113 / 1,193 (93.29%) | ☐ |  |
| [`heap/binary_data.rs`](../../../backend/bluejs/src/heap/binary_data.rs) | 757 / 839 (90.23%) | 71 / 74 (95.95%) | 1,129 / 1,311 (86.12%) | ☐ |  |
| [`heap/collection_iteration.rs`](../../../backend/bluejs/src/heap/collection_iteration.rs) | 82 / 83 (98.80%) | 4 / 4 (100.00%) | 103 / 110 (93.64%) | ☐ |  |
| [`heap/core.rs`](../../../backend/bluejs/src/heap/core.rs) | 761 / 793 (95.96%) | 69 / 70 (98.57%) | 1,088 / 1,194 (91.12%) | ☐ |  |
| [`heap/exotic.rs`](../../../backend/bluejs/src/heap/exotic.rs) | 902 / 925 (97.51%) | 91 / 91 (100.00%) | 987 / 1,079 (91.47%) | ☐ |  |
| [`heap/lifecycle.rs`](../../../backend/bluejs/src/heap/lifecycle.rs) | 280 / 298 (93.96%) | 30 / 31 (96.77%) | 470 / 517 (90.91%) | ☐ |  |
| [`heap/object_storage.rs`](../../../backend/bluejs/src/heap/object_storage.rs) | 705 / 737 (95.66%) | 54 / 56 (96.43%) | 1,190 / 1,322 (90.02%) | ☐ |  |
| [`heap/tests.rs`](../../../backend/bluejs/src/heap/tests.rs) | - | - | - | - | Test source; not a coverage target |
| [`intl.rs`](../../../backend/bluejs/src/intl.rs) | 100 / 101 (99.01%) | 17 / 17 (100.00%) | 154 / 159 (96.86%) | ☐ |  |
| [`lib.rs`](../../../backend/bluejs/src/lib.rs) | - | - | - | - | Declarations/re-exports only; no executable code |
| [`native.rs`](../../../backend/bluejs/src/native.rs) | 515 / 526 (97.91%) | 40 / 40 (100.00%) | 913 / 967 (94.42%) | ☐ |  |
| [`page_runtime.rs`](../../../backend/bluejs/src/page_runtime.rs) | 761 / 859 (88.59%) | 50 / 57 (87.72%) | 1,084 / 1,245 (87.07%) | ☐ |  |
| [`parser.rs`](../../../backend/bluejs/src/parser.rs) | 536 / 548 (97.81%) | 71 / 73 (97.26%) | 733 / 758 (96.70%) | ☐ |  |
| [`parser/expressions.rs`](../../../backend/bluejs/src/parser/expressions.rs) | 927 / 952 (97.37%) | 41 / 41 (100.00%) | 1,617 / 1,700 (95.12%) | ☐ |  |
| [`parser/functions.rs`](../../../backend/bluejs/src/parser/functions.rs) | 644 / 659 (97.72%) | 39 / 40 (97.50%) | 1,009 / 1,046 (96.46%) | ☐ |  |
| [`parser/module.rs`](../../../backend/bluejs/src/parser/module.rs) | 139 / 143 (97.20%) | 7 / 8 (87.50%) | 235 / 243 (96.71%) | ☐ |  |
| [`parser/module_items.rs`](../../../backend/bluejs/src/parser/module_items.rs) | 318 / 336 (94.64%) | 12 / 12 (100.00%) | 564 / 642 (87.85%) | ☐ |  |
| [`parser/patterns.rs`](../../../backend/bluejs/src/parser/patterns.rs) | 174 / 178 (97.75%) | 10 / 10 (100.00%) | 273 / 292 (93.49%) | ☐ |  |
| [`parser/statements.rs`](../../../backend/bluejs/src/parser/statements.rs) | 555 / 616 (90.10%) | 21 / 21 (100.00%) | 1,036 / 1,177 (88.02%) | ☐ |  |
| [`parser/tests.rs`](../../../backend/bluejs/src/parser/tests.rs) | - | - | - | - | Test source; not a coverage target |
| [`primitive.rs`](../../../backend/bluejs/src/primitive.rs) | 277 / 281 (98.58%) | 26 / 27 (96.30%) | 516 / 524 (98.47%) | ☐ |  |
| [`program_abi.rs`](../../../backend/bluejs/src/program_abi.rs) | 12 / 12 (100.00%) | 2 / 2 (100.00%) | 22 / 22 (100.00%) | ☑ |  |
| [`program_debug.rs`](../../../backend/bluejs/src/program_debug.rs) | 444 / 477 (93.08%) | 50 / 54 (92.59%) | 660 / 735 (89.80%) | ☐ |  |
| [`program_debug/ast_ids.rs`](../../../backend/bluejs/src/program_debug/ast_ids.rs) | 92 / 344 (26.74%) | 9 / 18 (50.00%) | 129 / 589 (21.90%) | ☐ |  |
| [`property.rs`](../../../backend/bluejs/src/property.rs) | 86 / 86 (100.00%) | 21 / 21 (100.00%) | 111 / 111 (100.00%) | ☑ |  |
| [`regex_backrefs.rs`](../../../backend/bluejs/src/regex_backrefs.rs) | 98 / 98 (100.00%) | 12 / 12 (100.00%) | 186 / 186 (100.00%) | ☑ |  |
| [`regex_canonicalize.rs`](../../../backend/bluejs/src/regex_canonicalize.rs) | 610 / 610 (100.00%) | 64 / 64 (100.00%) | 1,295 / 1,304 (99.31%) | ☐ |  |
| [`regex_escapes.rs`](../../../backend/bluejs/src/regex_escapes.rs) | 215 / 215 (100.00%) | 22 / 22 (100.00%) | 458 / 459 (99.78%) | ☐ |  |
| [`regex_group_names.rs`](../../../backend/bluejs/src/regex_group_names.rs) | 303 / 303 (100.00%) | 38 / 38 (100.00%) | 585 / 586 (99.83%) | ☐ |  |
| [`regex_worker.rs`](../../../backend/bluejs/src/regex_worker.rs) | 412 / 412 (100.00%) | 50 / 50 (100.00%) | 587 / 599 (98.00%) | ☐ |  |
| [`regexp.rs`](../../../backend/bluejs/src/regexp.rs) | 292 / 301 (97.01%) | 27 / 27 (100.00%) | 452 / 463 (97.62%) | ☐ |  |
| [`source_encoding.rs`](../../../backend/bluejs/src/source_encoding.rs) | 153 / 153 (100.00%) | 20 / 20 (100.00%) | 293 / 294 (99.66%) | ☐ |  |
| [`string.rs`](../../../backend/bluejs/src/string.rs) | 96 / 97 (98.97%) | 20 / 20 (100.00%) | 153 / 155 (98.71%) | ☐ |  |
| [`token.rs`](../../../backend/bluejs/src/token.rs) | 1,249 / 1,257 (99.36%) | 104 / 104 (100.00%) | 1,972 / 1,989 (99.15%) | ☐ |  |
| [`value.rs`](../../../backend/bluejs/src/value.rs) | 12 / 12 (100.00%) | 2 / 2 (100.00%) | 20 / 20 (100.00%) | ☑ |  |
| [`vm.rs`](../../../backend/bluejs/src/vm.rs) | 591 / 591 (100.00%) | 56 / 56 (100.00%) | 872 / 898 (97.10%) | ☐ |  |
| [`vm/builtins.rs`](../../../backend/bluejs/src/vm/builtins.rs) | 269 / 316 (85.13%) | 14 / 18 (77.78%) | 499 / 573 (87.09%) | ☐ |  |
| [`vm/builtins/arguments.rs`](../../../backend/bluejs/src/vm/builtins/arguments.rs) | 620 / 694 (89.34%) | 62 / 63 (98.41%) | 1,056 / 1,230 (85.85%) | ☐ |  |
| [`vm/builtins/array_change_by_copy.rs`](../../../backend/bluejs/src/vm/builtins/array_change_by_copy.rs) | 194 / 197 (98.48%) | 14 / 14 (100.00%) | 422 / 456 (92.54%) | ☐ |  |
| [`vm/builtins/array_from_async.rs`](../../../backend/bluejs/src/vm/builtins/array_from_async.rs) | 313 / 338 (92.60%) | 25 / 26 (96.15%) | 753 / 891 (84.51%) | ☐ |  |
| [`vm/builtins/array_scan.rs`](../../../backend/bluejs/src/vm/builtins/array_scan.rs) | 117 / 117 (100.00%) | 12 / 12 (100.00%) | 163 / 174 (93.68%) | ☐ |  |
| [`vm/builtins/arrays.rs`](../../../backend/bluejs/src/vm/builtins/arrays.rs) | 1,239 / 1,305 (94.94%) | 78 / 81 (96.30%) | 2,551 / 2,868 (88.95%) | ☐ |  |
| [`vm/builtins/binary_data.rs`](../../../backend/bluejs/src/vm/builtins/binary_data.rs) | 1,261 / 1,511 (83.45%) | 101 / 123 (82.11%) | 2,299 / 2,728 (84.27%) | ☐ |  |
| [`vm/builtins/collection_iteration.rs`](../../../backend/bluejs/src/vm/builtins/collection_iteration.rs) | 110 / 116 (94.83%) | 8 / 8 (100.00%) | 182 / 200 (91.00%) | ☐ |  |
| [`vm/builtins/collections.rs`](../../../backend/bluejs/src/vm/builtins/collections.rs) | 312 / 355 (87.89%) | 19 / 23 (82.61%) | 678 / 752 (90.16%) | ☐ |  |
| [`vm/builtins/decorators.rs`](../../../backend/bluejs/src/vm/builtins/decorators.rs) | 498 / 523 (95.22%) | 41 / 41 (100.00%) | 1,011 / 1,123 (90.03%) | ☐ |  |
| [`vm/builtins/dynamic.rs`](../../../backend/bluejs/src/vm/builtins/dynamic.rs) | 615 / 645 (95.35%) | 36 / 37 (97.30%) | 1,139 / 1,260 (90.40%) | ☐ |  |
| [`vm/builtins/execution.rs`](../../../backend/bluejs/src/vm/builtins/execution.rs) | - | - | - | - | Declarations/re-exports only; no executable code |
| [`vm/builtins/execution/closures.rs`](../../../backend/bluejs/src/vm/builtins/execution/closures.rs) | 346 / 359 (96.38%) | 15 / 15 (100.00%) | 650 / 688 (94.48%) | ☐ |  |
| [`vm/builtins/execution/runtime.rs`](../../../backend/bluejs/src/vm/builtins/execution/runtime.rs) | 1,253 / 1,483 (84.49%) | 121 / 134 (90.30%) | 2,370 / 2,870 (82.58%) | ☐ |  |
| [`vm/builtins/execution/setup.rs`](../../../backend/bluejs/src/vm/builtins/execution/setup.rs) | 916 / 1,052 (87.07%) | 90 / 92 (97.83%) | 1,550 / 1,776 (87.27%) | ☐ |  |
| [`vm/builtins/general.rs`](../../../backend/bluejs/src/vm/builtins/general.rs) | 420 / 425 (98.82%) | 35 / 37 (94.59%) | 701 / 750 (93.47%) | ☐ |  |
| [`vm/builtins/generators.rs`](../../../backend/bluejs/src/vm/builtins/generators.rs) | 1,308 / 1,481 (88.32%) | 64 / 76 (84.21%) | 2,067 / 2,372 (87.14%) | ☐ |  |
| [`vm/builtins/globals.rs`](../../../backend/bluejs/src/vm/builtins/globals.rs) | 882 / 985 (89.54%) | 12 / 12 (100.00%) | 1,306 / 1,465 (89.15%) | ☐ |  |
| [`vm/builtins/immutable_arraybuffer.rs`](../../../backend/bluejs/src/vm/builtins/immutable_arraybuffer.rs) | 116 / 120 (96.67%) | 12 / 12 (100.00%) | 210 / 233 (90.13%) | ☐ |  |
| [`vm/builtins/math.rs`](../../../backend/bluejs/src/vm/builtins/math.rs) | 314 / 333 (94.29%) | 15 / 15 (100.00%) | 538 / 605 (88.93%) | ☐ |  |
| [`vm/builtins/native_dispatch.rs`](../../../backend/bluejs/src/vm/builtins/native_dispatch.rs) | - | - | - | - | Declarations/re-exports only; no executable code |
| [`vm/builtins/native_dispatch/date.rs`](../../../backend/bluejs/src/vm/builtins/native_dispatch/date.rs) | 559 / 674 (82.94%) | 47 / 54 (87.04%) | 1,014 / 1,231 (82.37%) | ☐ |  |
| [`vm/builtins/native_dispatch/dispatch.rs`](../../../backend/bluejs/src/vm/builtins/native_dispatch/dispatch.rs) | 1,122 / 1,247 (89.98%) | 28 / 37 (75.68%) | 2,822 / 3,092 (91.27%) | ☐ |  |
| [`vm/builtins/numbers.rs`](../../../backend/bluejs/src/vm/builtins/numbers.rs) | 239 / 244 (97.95%) | 14 / 15 (93.33%) | 407 / 430 (94.65%) | ☐ |  |
| [`vm/builtins/object.rs`](../../../backend/bluejs/src/vm/builtins/object.rs) | 1,100 / 1,304 (84.36%) | 77 / 88 (87.50%) | 2,051 / 2,461 (83.34%) | ☐ |  |
| [`vm/builtins/promise_combinators.rs`](../../../backend/bluejs/src/vm/builtins/promise_combinators.rs) | 401 / 412 (97.33%) | 33 / 34 (97.06%) | 790 / 876 (90.18%) | ☐ |  |
| [`vm/builtins/promise_core.rs`](../../../backend/bluejs/src/vm/builtins/promise_core.rs) | 658 / 700 (94.00%) | 56 / 57 (98.25%) | 1,094 / 1,202 (91.01%) | ☐ |  |
| [`vm/builtins/promises.rs`](../../../backend/bluejs/src/vm/builtins/promises.rs) | 818 / 928 (88.15%) | 55 / 58 (94.83%) | 1,438 / 1,620 (88.77%) | ☐ |  |
| [`vm/builtins/resource_management.rs`](../../../backend/bluejs/src/vm/builtins/resource_management.rs) | 510 / 557 (91.56%) | 32 / 34 (94.12%) | 745 / 828 (89.98%) | ☐ |  |
| [`vm/builtins/set_methods.rs`](../../../backend/bluejs/src/vm/builtins/set_methods.rs) | 254 / 261 (97.32%) | 28 / 28 (100.00%) | 512 / 575 (89.04%) | ☐ |  |
| [`vm/builtins/tests.rs`](../../../backend/bluejs/src/vm/builtins/tests.rs) | - | - | - | - | Test source; not a coverage target |
| [`vm/builtins/typed_arrays.rs`](../../../backend/bluejs/src/vm/builtins/typed_arrays.rs) | 696 / 860 (80.93%) | 35 / 42 (83.33%) | 1,374 / 1,733 (79.28%) | ☐ |  |
| [`vm/builtins/uint8array.rs`](../../../backend/bluejs/src/vm/builtins/uint8array.rs) | 291 / 508 (57.28%) | 26 / 35 (74.29%) | 454 / 768 (59.11%) | ☐ |  |
| [`vm/completion.rs`](../../../backend/bluejs/src/vm/completion.rs) | 113 / 113 (100.00%) | 9 / 9 (100.00%) | 162 / 162 (100.00%) | ☑ |  |
| [`vm/debugger.rs`](../../../backend/bluejs/src/vm/debugger.rs) | 163 / 217 (75.12%) | 18 / 19 (94.74%) | 237 / 295 (80.34%) | ☐ |  |
| [`vm/errors.rs`](../../../backend/bluejs/src/vm/errors.rs) | 350 / 389 (89.97%) | 21 / 24 (87.50%) | 640 / 764 (83.77%) | ☐ |  |
| [`vm/execution.rs`](../../../backend/bluejs/src/vm/execution.rs) | 1,371 / 1,514 (90.55%) | 130 / 139 (93.53%) | 2,255 / 2,558 (88.15%) | ☐ |  |
| [`vm/functions.rs`](../../../backend/bluejs/src/vm/functions.rs) | 105 / 106 (99.06%) | 4 / 4 (100.00%) | 198 / 213 (92.96%) | ☐ |  |
| [`vm/interpreter.rs`](../../../backend/bluejs/src/vm/interpreter.rs) | 1,219 / 1,329 (91.72%) | 56 / 62 (90.32%) | 2,822 / 3,136 (89.99%) | ☐ |  |
| [`vm/intl.rs`](../../../backend/bluejs/src/vm/intl.rs) | 489 / 589 (83.02%) | 16 / 17 (94.12%) | 668 / 798 (83.71%) | ☐ |  |
| [`vm/intl/collator_locale.rs`](../../../backend/bluejs/src/vm/intl/collator_locale.rs) | 423 / 431 (98.14%) | 41 / 41 (100.00%) | 674 / 722 (93.35%) | ☐ |  |
| [`vm/intl/date_time.rs`](../../../backend/bluejs/src/vm/intl/date_time.rs) | 751 / 840 (89.40%) | 51 / 56 (91.07%) | 1,133 / 1,275 (88.86%) | ☐ |  |
| [`vm/intl/list_duration.rs`](../../../backend/bluejs/src/vm/intl/list_duration.rs) | 584 / 618 (94.50%) | 46 / 55 (83.64%) | 867 / 957 (90.60%) | ☐ |  |
| [`vm/intl/number_options.rs`](../../../backend/bluejs/src/vm/intl/number_options.rs) | 421 / 456 (92.32%) | 31 / 32 (96.88%) | 570 / 653 (87.29%) | ☐ |  |
| [`vm/intl/number_runtime.rs`](../../../backend/bluejs/src/vm/intl/number_runtime.rs) | 354 / 404 (87.62%) | 23 / 28 (82.14%) | 477 / 555 (85.95%) | ☐ |  |
| [`vm/intl/plural_segmenter.rs`](../../../backend/bluejs/src/vm/intl/plural_segmenter.rs) | 563 / 588 (95.75%) | 49 / 54 (90.74%) | 796 / 875 (90.97%) | ☐ |  |
| [`vm/intl/shared.rs`](../../../backend/bluejs/src/vm/intl/shared.rs) | 683 / 737 (92.67%) | 61 / 68 (89.71%) | 1,086 / 1,248 (87.02%) | ☐ |  |
| [`vm/intrinsics.rs`](../../../backend/bluejs/src/vm/intrinsics.rs) | 437 / 443 (98.65%) | 5 / 5 (100.00%) | 582 / 594 (97.98%) | ☐ |  |
| [`vm/json.rs`](../../../backend/bluejs/src/vm/json.rs) | 721 / 786 (91.73%) | 58 / 60 (96.67%) | 1,397 / 1,590 (87.86%) | ☐ |  |
| [`vm/lifecycle.rs`](../../../backend/bluejs/src/vm/lifecycle.rs) | 187 / 187 (100.00%) | 13 / 13 (100.00%) | 203 / 206 (98.54%) | ☐ |  |
| [`vm/modules.rs`](../../../backend/bluejs/src/vm/modules.rs) | 1,398 / 1,547 (90.37%) | 69 / 77 (89.61%) | 2,233 / 2,533 (88.16%) | ☐ |  |
| [`vm/modules/deferred.rs`](../../../backend/bluejs/src/vm/modules/deferred.rs) | 290 / 307 (94.46%) | 27 / 28 (96.43%) | 444 / 489 (90.80%) | ☐ |  |
| [`vm/modules/namespace.rs`](../../../backend/bluejs/src/vm/modules/namespace.rs) | 184 / 224 (82.14%) | 11 / 17 (64.71%) | 294 / 365 (80.55%) | ☐ |  |
| [`vm/modules/synthetic.rs`](../../../backend/bluejs/src/vm/modules/synthetic.rs) | 99 / 102 (97.06%) | 3 / 3 (100.00%) | 132 / 137 (96.35%) | ☐ |  |
| [`vm/native_stack.rs`](../../../backend/bluejs/src/vm/native_stack.rs) | 65 / 65 (100.00%) | 14 / 14 (100.00%) | 105 / 106 (99.06%) | ☐ |  |
| [`vm/operations.rs`](../../../backend/bluejs/src/vm/operations.rs) | 588 / 637 (92.31%) | 44 / 47 (93.62%) | 1,031 / 1,169 (88.20%) | ☐ |  |
| [`vm/properties.rs`](../../../backend/bluejs/src/vm/properties.rs) | 639 / 705 (90.64%) | 62 / 70 (88.57%) | 1,127 / 1,295 (87.03%) | ☐ |  |
| [`vm/realm_reentrancy.rs`](../../../backend/bluejs/src/vm/realm_reentrancy.rs) | 27 / 27 (100.00%) | 8 / 8 (100.00%) | 34 / 34 (100.00%) | ☑ |  |
| [`vm/regexp.rs`](../../../backend/bluejs/src/vm/regexp.rs) | 1,056 / 1,097 (96.26%) | 60 / 60 (100.00%) | 1,905 / 2,089 (91.19%) | ☐ |  |
| [`vm/shadow_realm.rs`](../../../backend/bluejs/src/vm/shadow_realm.rs) | 315 / 380 (82.89%) | 23 / 28 (82.14%) | 538 / 630 (85.40%) | ☐ |  |
| [`vm/temporal.rs`](../../../backend/bluejs/src/vm/temporal.rs) | 539 / 569 (94.73%) | 11 / 11 (100.00%) | 518 / 567 (91.36%) | ☐ |  |
| [`vm/temporal/calendar.rs`](../../../backend/bluejs/src/vm/temporal/calendar.rs) | 147 / 147 (100.00%) | 14 / 14 (100.00%) | 215 / 215 (100.00%) | ☑ |  |
| [`vm/temporal/conversion.rs`](../../../backend/bluejs/src/vm/temporal/conversion.rs) | - | - | - | - | Declarations/re-exports only; no executable code |
| [`vm/temporal/conversion/calendar_fields.rs`](../../../backend/bluejs/src/vm/temporal/conversion/calendar_fields.rs) | 302 / 313 (96.49%) | 22 / 27 (81.48%) | 456 / 485 (94.02%) | ☐ |  |
| [`vm/temporal/conversion/from_value.rs`](../../../backend/bluejs/src/vm/temporal/conversion/from_value.rs) | 340 / 368 (92.39%) | 20 / 21 (95.24%) | 554 / 600 (92.33%) | ☐ |  |
| [`vm/temporal/conversion/getters.rs`](../../../backend/bluejs/src/vm/temporal/conversion/getters.rs) | 107 / 120 (89.17%) | 4 / 7 (57.14%) | 148 / 164 (90.24%) | ☐ |  |
| [`vm/temporal/conversion/numeric.rs`](../../../backend/bluejs/src/vm/temporal/conversion/numeric.rs) | 194 / 198 (97.98%) | 21 / 24 (87.50%) | 241 / 259 (93.05%) | ☐ |  |
| [`vm/temporal/conversion/options.rs`](../../../backend/bluejs/src/vm/temporal/conversion/options.rs) | 127 / 130 (97.69%) | 10 / 12 (83.33%) | 145 / 158 (91.77%) | ☐ |  |
| [`vm/temporal/conversion/zoned_conversion.rs`](../../../backend/bluejs/src/vm/temporal/conversion/zoned_conversion.rs) | 105 / 131 (80.15%) | 4 / 11 (36.36%) | 175 / 206 (84.95%) | ☐ |  |
| [`vm/temporal/dates.rs`](../../../backend/bluejs/src/vm/temporal/dates.rs) | - | - | - | - | Declarations/re-exports only; no executable code |
| [`vm/temporal/dates/arithmetic.rs`](../../../backend/bluejs/src/vm/temporal/dates/arithmetic.rs) | 206 / 206 (100.00%) | 8 / 8 (100.00%) | 277 / 283 (97.88%) | ☐ |  |
| [`vm/temporal/dates/comparison.rs`](../../../backend/bluejs/src/vm/temporal/dates/comparison.rs) | 49 / 49 (100.00%) | 3 / 3 (100.00%) | 65 / 66 (98.48%) | ☐ |  |
| [`vm/temporal/dates/construction.rs`](../../../backend/bluejs/src/vm/temporal/dates/construction.rs) | 211 / 231 (91.34%) | 10 / 12 (83.33%) | 233 / 252 (92.46%) | ☐ |  |
| [`vm/temporal/dates/conversions.rs`](../../../backend/bluejs/src/vm/temporal/dates/conversions.rs) | 66 / 69 (95.65%) | 3 / 6 (50.00%) | 105 / 119 (88.24%) | ☐ |  |
| [`vm/temporal/dates/date_time.rs`](../../../backend/bluejs/src/vm/temporal/dates/date_time.rs) | 128 / 133 (96.24%) | 8 / 9 (88.89%) | 202 / 214 (94.39%) | ☐ |  |
| [`vm/temporal/dates/formatting.rs`](../../../backend/bluejs/src/vm/temporal/dates/formatting.rs) | 167 / 171 (97.66%) | 5 / 6 (83.33%) | 222 / 229 (96.94%) | ☐ |  |
| [`vm/temporal/dates/now_and_zone.rs`](../../../backend/bluejs/src/vm/temporal/dates/now_and_zone.rs) | 182 / 192 (94.79%) | 16 / 20 (80.00%) | 247 / 266 (92.86%) | ☐ |  |
| [`vm/temporal/dates/with.rs`](../../../backend/bluejs/src/vm/temporal/dates/with.rs) | 140 / 140 (100.00%) | 6 / 6 (100.00%) | 254 / 264 (96.21%) | ☐ |  |
| [`vm/temporal/duration_conversion.rs`](../../../backend/bluejs/src/vm/temporal/duration_conversion.rs) | 47 / 49 (95.92%) | 3 / 4 (75.00%) | 70 / 77 (90.91%) | ☐ |  |
| [`vm/temporal/duration_math.rs`](../../../backend/bluejs/src/vm/temporal/duration_math.rs) | 305 / 305 (100.00%) | 29 / 29 (100.00%) | 361 / 361 (100.00%) | ☑ |  |
| [`vm/temporal/duration_operations.rs`](../../../backend/bluejs/src/vm/temporal/duration_operations.rs) | 620 / 621 (99.84%) | 26 / 27 (96.30%) | 900 / 923 (97.51%) | ☐ |  |
| [`vm/temporal/duration_relative.rs`](../../../backend/bluejs/src/vm/temporal/duration_relative.rs) | 515 / 532 (96.80%) | 37 / 44 (84.09%) | 639 / 677 (94.39%) | ☐ |  |
| [`vm/temporal/epoch.rs`](../../../backend/bluejs/src/vm/temporal/epoch.rs) | 136 / 136 (100.00%) | 13 / 13 (100.00%) | 237 / 237 (100.00%) | ☑ |  |
| [`vm/temporal/instant.rs`](../../../backend/bluejs/src/vm/temporal/instant.rs) | 402 / 462 (87.01%) | 20 / 32 (62.50%) | 574 / 667 (86.06%) | ☐ |  |
| [`vm/temporal/iso.rs`](../../../backend/bluejs/src/vm/temporal/iso.rs) | 17 / 17 (100.00%) | 3 / 3 (100.00%) | 24 / 24 (100.00%) | ☑ |  |
| [`vm/temporal/iso/annotations.rs`](../../../backend/bluejs/src/vm/temporal/iso/annotations.rs) | 154 / 155 (99.35%) | 21 / 21 (100.00%) | 274 / 285 (96.14%) | ☐ |  |
| [`vm/temporal/iso/datetime.rs`](../../../backend/bluejs/src/vm/temporal/iso/datetime.rs) | 129 / 129 (100.00%) | 13 / 13 (100.00%) | 216 / 219 (98.63%) | ☐ |  |
| [`vm/temporal/iso/duration.rs`](../../../backend/bluejs/src/vm/temporal/iso/duration.rs) | 104 / 104 (100.00%) | 4 / 4 (100.00%) | 164 / 168 (97.62%) | ☐ |  |
| [`vm/temporal/iso/offset.rs`](../../../backend/bluejs/src/vm/temporal/iso/offset.rs) | 124 / 125 (99.20%) | 7 / 7 (100.00%) | 225 / 233 (96.57%) | ☐ |  |
| [`vm/temporal/iso/scan.rs`](../../../backend/bluejs/src/vm/temporal/iso/scan.rs) | 236 / 236 (100.00%) | 31 / 31 (100.00%) | 451 / 473 (95.35%) | ☐ |  |
| [`vm/temporal/iso/tests.rs`](../../../backend/bluejs/src/vm/temporal/iso/tests.rs) | - | - | - | - | Test source; not a coverage target |
| [`vm/temporal/plain_date.rs`](../../../backend/bluejs/src/vm/temporal/plain_date.rs) | - | - | - | - | Declarations/re-exports only; no executable code |
| [`vm/temporal/plain_date/calendar_add.rs`](../../../backend/bluejs/src/vm/temporal/plain_date/calendar_add.rs) | 131 / 131 (100.00%) | 3 / 3 (100.00%) | 230 / 241 (95.44%) | ☐ |  |
| [`vm/temporal/plain_date/calendar_difference.rs`](../../../backend/bluejs/src/vm/temporal/plain_date/calendar_difference.rs) | 185 / 188 (98.40%) | 7 / 7 (100.00%) | 335 / 339 (98.82%) | ☐ |  |
| [`vm/temporal/plain_date/format.rs`](../../../backend/bluejs/src/vm/temporal/plain_date/format.rs) | 21 / 22 (95.45%) | 3 / 3 (100.00%) | 38 / 39 (97.44%) | ☐ |  |
| [`vm/temporal/plain_date/iso_date.rs`](../../../backend/bluejs/src/vm/temporal/plain_date/iso_date.rs) | 112 / 113 (99.12%) | 16 / 16 (100.00%) | 203 / 204 (99.51%) | ☐ |  |
| [`vm/temporal/plain_date/month_structure.rs`](../../../backend/bluejs/src/vm/temporal/plain_date/month_structure.rs) | 110 / 110 (100.00%) | 11 / 11 (100.00%) | 168 / 177 (94.92%) | ☐ |  |
| [`vm/temporal/plain_date/round_duration.rs`](../../../backend/bluejs/src/vm/temporal/plain_date/round_duration.rs) | 122 / 126 (96.83%) | 5 / 5 (100.00%) | 182 / 188 (96.81%) | ☐ |  |
| [`vm/temporal/plain_date/tests.rs`](../../../backend/bluejs/src/vm/temporal/plain_date/tests.rs) | - | - | - | - | Test source; not a coverage target |
| [`vm/temporal/plain_date_time_difference.rs`](../../../backend/bluejs/src/vm/temporal/plain_date_time_difference.rs) | 784 / 785 (99.87%) | 54 / 54 (100.00%) | 1,088 / 1,105 (98.46%) | ☐ |  |
| [`vm/temporal/plain_month_day.rs`](../../../backend/bluejs/src/vm/temporal/plain_month_day.rs) | 282 / 282 (100.00%) | 28 / 28 (100.00%) | 386 / 386 (100.00%) | ☑ |  |
| [`vm/temporal/plain_time.rs`](../../../backend/bluejs/src/vm/temporal/plain_time.rs) | 596 / 649 (91.83%) | 37 / 44 (84.09%) | 844 / 922 (91.54%) | ☐ |  |
| [`vm/temporal/plain_year_month.rs`](../../../backend/bluejs/src/vm/temporal/plain_year_month.rs) | 145 / 145 (100.00%) | 13 / 13 (100.00%) | 193 / 193 (100.00%) | ☑ |  |
| [`vm/temporal/receiver.rs`](../../../backend/bluejs/src/vm/temporal/receiver.rs) | 40 / 40 (100.00%) | 2 / 2 (100.00%) | 39 / 40 (97.50%) | ☐ |  |
| [`vm/temporal/rounding.rs`](../../../backend/bluejs/src/vm/temporal/rounding.rs) | 395 / 395 (100.00%) | 33 / 33 (100.00%) | 643 / 644 (99.84%) | ☐ |  |
| [`vm/temporal/time_zone.rs`](../../../backend/bluejs/src/vm/temporal/time_zone.rs) | 512 / 513 (99.81%) | 47 / 48 (97.92%) | 1,006 / 1,013 (99.31%) | ☐ |  |
| [`vm/temporal/time_zone_id.rs`](../../../backend/bluejs/src/vm/temporal/time_zone_id.rs) | 202 / 202 (100.00%) | 28 / 28 (100.00%) | 457 / 465 (98.28%) | ☐ |  |
| [`vm/temporal/year_month.rs`](../../../backend/bluejs/src/vm/temporal/year_month.rs) | 971 / 1,017 (95.48%) | 68 / 82 (82.93%) | 1,537 / 1,635 (94.01%) | ☐ |  |
| [`vm/temporal/zoned.rs`](../../../backend/bluejs/src/vm/temporal/zoned.rs) | - | - | - | - | Declarations/re-exports only; no executable code |
| [`vm/temporal/zoned/arithmetic.rs`](../../../backend/bluejs/src/vm/temporal/zoned/arithmetic.rs) | 172 / 180 (95.56%) | 8 / 9 (88.89%) | 275 / 288 (95.49%) | ☐ |  |
| [`vm/temporal/zoned/conversions.rs`](../../../backend/bluejs/src/vm/temporal/zoned/conversions.rs) | 115 / 117 (98.29%) | 8 / 9 (88.89%) | 159 / 172 (92.44%) | ☐ |  |
| [`vm/temporal/zoned/formatting.rs`](../../../backend/bluejs/src/vm/temporal/zoned/formatting.rs) | 152 / 157 (96.82%) | 6 / 8 (75.00%) | 241 / 255 (94.51%) | ☐ |  |
| [`vm/temporal/zoned/from_value.rs`](../../../backend/bluejs/src/vm/temporal/zoned/from_value.rs) | 184 / 196 (93.88%) | 13 / 15 (86.67%) | 250 / 265 (94.34%) | ☐ |  |
| [`vm/temporal/zoned/resolution.rs`](../../../backend/bluejs/src/vm/temporal/zoned/resolution.rs) | 99 / 99 (100.00%) | 7 / 7 (100.00%) | 126 / 127 (99.21%) | ☐ |  |
| [`vm/temporal/zoned/since_until.rs`](../../../backend/bluejs/src/vm/temporal/zoned/since_until.rs) | 141 / 141 (100.00%) | 8 / 8 (100.00%) | 202 / 209 (96.65%) | ☐ |  |
| [`vm/temporal/zoned/with.rs`](../../../backend/bluejs/src/vm/temporal/zoned/with.rs) | 205 / 205 (100.00%) | 9 / 9 (100.00%) | 352 / 369 (95.39%) | ☐ |  |
| [`vm/temporal/zoned_date_time.rs`](../../../backend/bluejs/src/vm/temporal/zoned_date_time.rs) | 218 / 222 (98.20%) | 15 / 15 (100.00%) | 361 / 368 (98.10%) | ☐ |  |
| [`vm/temporal/zoned_difference.rs`](../../../backend/bluejs/src/vm/temporal/zoned_difference.rs) | 767 / 771 (99.48%) | 43 / 43 (100.00%) | 1,056 / 1,090 (96.88%) | ☐ |  |
| [`vm/test262.rs`](../../../backend/bluejs/src/vm/test262.rs) | 60 / 60 (100.00%) | 6 / 6 (100.00%) | 96 / 97 (98.97%) | ☐ |  |
| [`vm/test262/assertions.rs`](../../../backend/bluejs/src/vm/test262/assertions.rs) | 288 / 299 (96.32%) | 16 / 17 (94.12%) | 750 / 838 (89.50%) | ☐ |  |
| [`vm/test262/cases.rs`](../../../backend/bluejs/src/vm/test262/cases.rs) | 647 / 686 (94.31%) | 39 / 45 (86.67%) | 1,129 / 1,263 (89.39%) | ☐ |  |
| [`vm/test262/foreign.rs`](../../../backend/bluejs/src/vm/test262/foreign.rs) | 1,413 / 1,581 (89.37%) | 124 / 143 (86.71%) | 2,217 / 2,640 (83.98%) | ☐ |  |
| [`vm/test262/harness.rs`](../../../backend/bluejs/src/vm/test262/harness.rs) | 230 / 257 (89.49%) | 12 / 12 (100.00%) | 369 / 418 (88.28%) | ☐ |  |
| [`vm/test262/reverse.rs`](../../../backend/bluejs/src/vm/test262/reverse.rs) | 260 / 270 (96.30%) | 24 / 26 (92.31%) | 427 / 464 (92.03%) | ☐ |  |
| [`vm/test262_agents.rs`](../../../backend/bluejs/src/vm/test262_agents.rs) | 421 / 462 (91.13%) | 42 / 49 (85.71%) | 593 / 664 (89.31%) | ☐ |  |
| [`vm/tests.rs`](../../../backend/bluejs/src/vm/tests.rs) | - | - | - | - | Test source; not a coverage target |
| **Total (158 instrumented files)** | **67,932 / 73,160 (92.85%)** | **4,974 / 5,326 (93.39%)** | **113,672 / 126,129 (90.12%)** | ☐ |  |

## Historical differences from the other platforms (2026-09-21)

- **Ubuntu**: 2 of 102,926 modes differ (1 file): `staging/sm/Math/acosh-approx.js` (this platform: pass; Ubuntu: fail).

## Reproduce the current Test262 inventory

```sh
python3 -m venv /tmp/bluejs-conformance-venv
/tmp/bluejs-conformance-venv/bin/pip install -r backend/bluejs/test262/requirements.txt
cargo build -p blueice-bluejs --bins --offline
/tmp/bluejs-conformance-venv/bin/python -m unittest discover -s backend/bluejs/test262 -v
/tmp/bluejs-conformance-venv/bin/python backend/bluejs/test262/run.py --corpus development/browser_core/reference/test262 --jobs 8 --output target/test262-macos-20260925 --progress-interval 60
/tmp/bluejs-conformance-venv/bin/python backend/bluejs/test262/analyze.py --run target/test262-macos-20260925 --corpus development/browser_core/reference/test262 --output target/test262-macos-20260925-analysis
```

The runner validates the pinned corpus marker, manifest hash, and every corpus file before execution. It returns 1 for this run because the 1 `stale_corpus` and 4 `excluded` modes remain non-pass statuses; inspect `summary.json` and use `analyze.py` to reconcile all records. Keep the host otherwise idle during this inventory because ordinary cases have a two-second wall deadline.
