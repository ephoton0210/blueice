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

This is a separate BlueJS coverage measurement at commit `8835e9e3` with uncommitted changes on Darwin 27.0 (`arm64`), rustc 1.95.0 (59807616e 2026-04-14) and `cargo-llvm-cov 0.9.1`. It measures the Rust test suite independently of the Test262 inventory and historical verification above. `python3 backend/bluejs/coverage_file.py --update-macos-report` cleaned prior LLVM artifacts, ran the complete default BlueJS Rust test suite, and exported fresh per-file JSON and source-line text. The opt-in Node oracle and external full Test262 runner were not included. Workspace coverage was not remeasured at this revision.

Each measured cell shows covered / instrumented and the coverage rate. All 173 Rust files under `backend/bluejs/src/` are listed: 159 have LLVM counters; 14 use `-` with an individual reason in `Note`. A `0%` result requires a positive instrumented denominator and zero covered units. `☑` means **lines, functions and regions all reach 100%**; `☐` means at least one is below 100%. The total aggregates only instrumented files. Source lines and regions are counted once per file location: a location is covered when any unit or integration test binary executes it. Function and region denominators are checked against LLVM JSON; line hits come from LLVM's source-line view. These per-source counts can differ from LLVM's raw summary for multiply compiled source files; they are not the CI raw summary coverage metric. Any file that reaches 100% with combined results while its raw LLVM summary is lower includes the raw counts in `Note`. Region coverage is separate from branch coverage. To rerun any one file independently, use `python3 backend/bluejs/coverage_file.py ast.rs` (replace `ast.rs` with its source path). Each invocation reruns the entire test suite, since tests outside a file can still exercise it.

| Source file (relative to `backend/bluejs/src/`) | Lines | Functions | Regions | Complete | Note |
| --- | ---: | ---: | ---: | :---: | --- |
| [`ast.rs`](../../../backend/bluejs/src/ast.rs) | 854 / 854 (100.00%) | 90 / 90 (100.00%) | 1,184 / 1,184 (100.00%) | ☑ |  |
| [`bin/bluejs-regexp-worker.rs`](../../../backend/bluejs/src/bin/bluejs-regexp-worker.rs) | 3 / 3 (100.00%) | 1 / 1 (100.00%) | 3 / 3 (100.00%) | ☑ |  |
| [`bin/bluejs-test262.rs`](../../../backend/bluejs/src/bin/bluejs-test262.rs) | 613 / 613 (100.00%) | 56 / 56 (100.00%) | 1,211 / 1,211 (100.00%) | ☑ |  |
| [`bytecode.rs`](../../../backend/bluejs/src/bytecode.rs) | 84 / 84 (100.00%) | 11 / 11 (100.00%) | 88 / 88 (100.00%) | ☑ |  |
| [`compiler.rs`](../../../backend/bluejs/src/compiler.rs) | 1,426 / 1,426 (100.00%) | 146 / 146 (100.00%) | 2,110 / 2,110 (100.00%) | ☑ |  |
| [`compiler/expressions.rs`](../../../backend/bluejs/src/compiler/expressions.rs) | 1,523 / 1,523 (100.00%) | 41 / 41 (100.00%) | 3,414 / 3,414 (100.00%) | ☑ | Combined test coverage is complete; raw LLVM summary: lines 1,522/1,534, functions 41/41, regions 3,396/3,414 |
| [`compiler/expressions/tests.rs`](../../../backend/bluejs/src/compiler/expressions/tests.rs) | - | - | - | - | Test source; not a coverage target |
| [`compiler/functions.rs`](../../../backend/bluejs/src/compiler/functions.rs) | 943 / 955 (98.74%) | 51 / 51 (100.00%) | 1,706 / 1,824 (93.53%) | ☐ |  |
| [`compiler/private_validation.rs`](../../../backend/bluejs/src/compiler/private_validation.rs) | 315 / 321 (98.13%) | 32 / 32 (100.00%) | 643 / 701 (91.73%) | ☐ |  |
| [`compiler/statements.rs`](../../../backend/bluejs/src/compiler/statements.rs) | 1,280 / 1,325 (96.60%) | 68 / 70 (97.14%) | 2,413 / 2,613 (92.35%) | ☐ |  |
| [`heap.rs`](../../../backend/bluejs/src/heap.rs) | 681 / 725 (93.93%) | 77 / 78 (98.72%) | 1,119 / 1,193 (93.80%) | ☐ |  |
| [`heap/binary_data.rs`](../../../backend/bluejs/src/heap/binary_data.rs) | 745 / 823 (90.52%) | 71 / 74 (95.95%) | 1,134 / 1,311 (86.50%) | ☐ |  |
| [`heap/collection_iteration.rs`](../../../backend/bluejs/src/heap/collection_iteration.rs) | 82 / 83 (98.80%) | 4 / 4 (100.00%) | 104 / 110 (94.55%) | ☐ |  |
| [`heap/core.rs`](../../../backend/bluejs/src/heap/core.rs) | 746 / 776 (96.13%) | 69 / 70 (98.57%) | 1,089 / 1,194 (91.21%) | ☐ |  |
| [`heap/exotic.rs`](../../../backend/bluejs/src/heap/exotic.rs) | 897 / 920 (97.50%) | 91 / 91 (100.00%) | 988 / 1,079 (91.57%) | ☐ |  |
| [`heap/lifecycle.rs`](../../../backend/bluejs/src/heap/lifecycle.rs) | 272 / 283 (96.11%) | 30 / 31 (96.77%) | 480 / 517 (92.84%) | ☐ |  |
| [`heap/object_storage.rs`](../../../backend/bluejs/src/heap/object_storage.rs) | 677 / 707 (95.76%) | 54 / 56 (96.43%) | 1,191 / 1,322 (90.09%) | ☐ |  |
| [`heap/tests.rs`](../../../backend/bluejs/src/heap/tests.rs) | - | - | - | - | Test source; not a coverage target |
| [`intl.rs`](../../../backend/bluejs/src/intl.rs) | 94 / 95 (98.95%) | 17 / 17 (100.00%) | 154 / 159 (96.86%) | ☐ |  |
| [`lib.rs`](../../../backend/bluejs/src/lib.rs) | - | - | - | - | Declarations/re-exports only; no executable code |
| [`native.rs`](../../../backend/bluejs/src/native.rs) | 496 / 507 (97.83%) | 40 / 40 (100.00%) | 913 / 967 (94.42%) | ☐ |  |
| [`page_runtime.rs`](../../../backend/bluejs/src/page_runtime.rs) | 899 / 1,013 (88.75%) | 62 / 71 (87.32%) | 1,302 / 1,473 (88.39%) | ☐ |  |
| [`parser.rs`](../../../backend/bluejs/src/parser.rs) | 528 / 537 (98.32%) | 71 / 73 (97.26%) | 735 / 758 (96.97%) | ☐ |  |
| [`parser/expressions.rs`](../../../backend/bluejs/src/parser/expressions.rs) | 926 / 948 (97.68%) | 41 / 41 (100.00%) | 1,619 / 1,700 (95.24%) | ☐ |  |
| [`parser/functions.rs`](../../../backend/bluejs/src/parser/functions.rs) | 634 / 647 (97.99%) | 39 / 40 (97.50%) | 1,009 / 1,046 (96.46%) | ☐ |  |
| [`parser/module.rs`](../../../backend/bluejs/src/parser/module.rs) | 135 / 138 (97.83%) | 7 / 8 (87.50%) | 235 / 243 (96.71%) | ☐ |  |
| [`parser/module_items.rs`](../../../backend/bluejs/src/parser/module_items.rs) | 323 / 331 (97.58%) | 12 / 12 (100.00%) | 587 / 642 (91.43%) | ☐ |  |
| [`parser/patterns.rs`](../../../backend/bluejs/src/parser/patterns.rs) | 172 / 176 (97.73%) | 10 / 10 (100.00%) | 273 / 292 (93.49%) | ☐ |  |
| [`parser/statements.rs`](../../../backend/bluejs/src/parser/statements.rs) | 569 / 614 (92.67%) | 21 / 21 (100.00%) | 1,055 / 1,177 (89.63%) | ☐ |  |
| [`parser/tests.rs`](../../../backend/bluejs/src/parser/tests.rs) | - | - | - | - | Test source; not a coverage target |
| [`primitive.rs`](../../../backend/bluejs/src/primitive.rs) | 267 / 269 (99.26%) | 26 / 27 (96.30%) | 516 / 524 (98.47%) | ☐ |  |
| [`program_abi.rs`](../../../backend/bluejs/src/program_abi.rs) | 12 / 12 (100.00%) | 2 / 2 (100.00%) | 22 / 22 (100.00%) | ☑ |  |
| [`program_debug.rs`](../../../backend/bluejs/src/program_debug.rs) | 437 / 470 (92.98%) | 50 / 54 (92.59%) | 660 / 735 (89.80%) | ☐ |  |
| [`program_debug/ast_ids.rs`](../../../backend/bluejs/src/program_debug/ast_ids.rs) | 103 / 344 (29.94%) | 10 / 18 (55.56%) | 151 / 589 (25.64%) | ☐ |  |
| [`property.rs`](../../../backend/bluejs/src/property.rs) | 84 / 84 (100.00%) | 21 / 21 (100.00%) | 111 / 111 (100.00%) | ☑ |  |
| [`regex_backrefs.rs`](../../../backend/bluejs/src/regex_backrefs.rs) | 94 / 94 (100.00%) | 12 / 12 (100.00%) | 186 / 186 (100.00%) | ☑ |  |
| [`regex_canonicalize.rs`](../../../backend/bluejs/src/regex_canonicalize.rs) | 586 / 586 (100.00%) | 64 / 64 (100.00%) | 1,295 / 1,304 (99.31%) | ☐ |  |
| [`regex_escapes.rs`](../../../backend/bluejs/src/regex_escapes.rs) | 205 / 205 (100.00%) | 22 / 22 (100.00%) | 458 / 459 (99.78%) | ☐ |  |
| [`regex_group_names.rs`](../../../backend/bluejs/src/regex_group_names.rs) | 282 / 282 (100.00%) | 38 / 38 (100.00%) | 585 / 586 (99.83%) | ☐ |  |
| [`regex_worker.rs`](../../../backend/bluejs/src/regex_worker.rs) | 383 / 383 (100.00%) | 50 / 50 (100.00%) | 588 / 599 (98.16%) | ☐ |  |
| [`regexp.rs`](../../../backend/bluejs/src/regexp.rs) | 280 / 289 (96.89%) | 27 / 27 (100.00%) | 452 / 463 (97.62%) | ☐ |  |
| [`source_encoding.rs`](../../../backend/bluejs/src/source_encoding.rs) | 151 / 151 (100.00%) | 20 / 20 (100.00%) | 293 / 294 (99.66%) | ☐ |  |
| [`string.rs`](../../../backend/bluejs/src/string.rs) | 94 / 95 (98.95%) | 20 / 20 (100.00%) | 153 / 155 (98.71%) | ☐ |  |
| [`token.rs`](../../../backend/bluejs/src/token.rs) | 1,220 / 1,226 (99.51%) | 104 / 104 (100.00%) | 1,976 / 1,989 (99.35%) | ☐ |  |
| [`value.rs`](../../../backend/bluejs/src/value.rs) | 12 / 12 (100.00%) | 2 / 2 (100.00%) | 20 / 20 (100.00%) | ☑ |  |
| [`vm.rs`](../../../backend/bluejs/src/vm.rs) | 568 / 568 (100.00%) | 57 / 57 (100.00%) | 881 / 907 (97.13%) | ☐ |  |
| [`vm/builtins.rs`](../../../backend/bluejs/src/vm/builtins.rs) | 265 / 307 (86.32%) | 14 / 18 (77.78%) | 499 / 573 (87.09%) | ☐ |  |
| [`vm/builtins/arguments.rs`](../../../backend/bluejs/src/vm/builtins/arguments.rs) | 581 / 649 (89.52%) | 62 / 63 (98.41%) | 1,059 / 1,232 (85.96%) | ☐ |  |
| [`vm/builtins/array_change_by_copy.rs`](../../../backend/bluejs/src/vm/builtins/array_change_by_copy.rs) | 187 / 190 (98.42%) | 14 / 14 (100.00%) | 422 / 456 (92.54%) | ☐ |  |
| [`vm/builtins/array_from_async.rs`](../../../backend/bluejs/src/vm/builtins/array_from_async.rs) | 306 / 330 (92.73%) | 25 / 26 (96.15%) | 753 / 891 (84.51%) | ☐ |  |
| [`vm/builtins/array_scan.rs`](../../../backend/bluejs/src/vm/builtins/array_scan.rs) | 114 / 114 (100.00%) | 12 / 12 (100.00%) | 163 / 174 (93.68%) | ☐ |  |
| [`vm/builtins/arrays.rs`](../../../backend/bluejs/src/vm/builtins/arrays.rs) | 1,200 / 1,263 (95.01%) | 78 / 81 (96.30%) | 2,549 / 2,868 (88.88%) | ☐ |  |
| [`vm/builtins/binary_data.rs`](../../../backend/bluejs/src/vm/builtins/binary_data.rs) | 1,208 / 1,415 (85.37%) | 101 / 123 (82.11%) | 2,299 / 2,728 (84.27%) | ☐ |  |
| [`vm/builtins/collection_iteration.rs`](../../../backend/bluejs/src/vm/builtins/collection_iteration.rs) | 106 / 111 (95.50%) | 8 / 8 (100.00%) | 182 / 200 (91.00%) | ☐ |  |
| [`vm/builtins/collections.rs`](../../../backend/bluejs/src/vm/builtins/collections.rs) | 299 / 336 (88.99%) | 19 / 23 (82.61%) | 678 / 752 (90.16%) | ☐ |  |
| [`vm/builtins/decorators.rs`](../../../backend/bluejs/src/vm/builtins/decorators.rs) | 484 / 508 (95.28%) | 41 / 41 (100.00%) | 1,011 / 1,123 (90.03%) | ☐ |  |
| [`vm/builtins/dynamic.rs`](../../../backend/bluejs/src/vm/builtins/dynamic.rs) | 589 / 617 (95.46%) | 36 / 37 (97.30%) | 1,139 / 1,260 (90.40%) | ☐ |  |
| [`vm/builtins/execution.rs`](../../../backend/bluejs/src/vm/builtins/execution.rs) | - | - | - | - | Declarations/re-exports only; no executable code |
| [`vm/builtins/execution/closures.rs`](../../../backend/bluejs/src/vm/builtins/execution/closures.rs) | 335 / 347 (96.54%) | 15 / 15 (100.00%) | 650 / 688 (94.48%) | ☐ |  |
| [`vm/builtins/execution/runtime.rs`](../../../backend/bluejs/src/vm/builtins/execution/runtime.rs) | 1,171 / 1,388 (84.37%) | 121 / 134 (90.30%) | 2,371 / 2,870 (82.61%) | ☐ |  |
| [`vm/builtins/execution/setup.rs`](../../../backend/bluejs/src/vm/builtins/execution/setup.rs) | 857 / 979 (87.54%) | 90 / 92 (97.83%) | 1,552 / 1,776 (87.39%) | ☐ |  |
| [`vm/builtins/general.rs`](../../../backend/bluejs/src/vm/builtins/general.rs) | 411 / 414 (99.28%) | 35 / 37 (94.59%) | 701 / 750 (93.47%) | ☐ |  |
| [`vm/builtins/generators.rs`](../../../backend/bluejs/src/vm/builtins/generators.rs) | 1,274 / 1,432 (88.97%) | 64 / 76 (84.21%) | 2,067 / 2,372 (87.14%) | ☐ |  |
| [`vm/builtins/globals.rs`](../../../backend/bluejs/src/vm/builtins/globals.rs) | 872 / 971 (89.80%) | 12 / 12 (100.00%) | 1,310 / 1,465 (89.42%) | ☐ |  |
| [`vm/builtins/immutable_arraybuffer.rs`](../../../backend/bluejs/src/vm/builtins/immutable_arraybuffer.rs) | 110 / 114 (96.49%) | 12 / 12 (100.00%) | 210 / 233 (90.13%) | ☐ |  |
| [`vm/builtins/math.rs`](../../../backend/bluejs/src/vm/builtins/math.rs) | 306 / 325 (94.15%) | 15 / 15 (100.00%) | 538 / 605 (88.93%) | ☐ |  |
| [`vm/builtins/native_dispatch.rs`](../../../backend/bluejs/src/vm/builtins/native_dispatch.rs) | - | - | - | - | Declarations/re-exports only; no executable code |
| [`vm/builtins/native_dispatch/date.rs`](../../../backend/bluejs/src/vm/builtins/native_dispatch/date.rs) | 541 / 646 (83.75%) | 47 / 54 (87.04%) | 1,014 / 1,231 (82.37%) | ☐ |  |
| [`vm/builtins/native_dispatch/dispatch.rs`](../../../backend/bluejs/src/vm/builtins/native_dispatch/dispatch.rs) | 1,108 / 1,209 (91.65%) | 28 / 37 (75.68%) | 2,869 / 3,132 (91.60%) | ☐ |  |
| [`vm/builtins/numbers.rs`](../../../backend/bluejs/src/vm/builtins/numbers.rs) | 236 / 240 (98.33%) | 14 / 15 (93.33%) | 407 / 430 (94.65%) | ☐ |  |
| [`vm/builtins/object.rs`](../../../backend/bluejs/src/vm/builtins/object.rs) | 1,056 / 1,245 (84.82%) | 77 / 88 (87.50%) | 2,053 / 2,461 (83.42%) | ☐ |  |
| [`vm/builtins/promise_combinators.rs`](../../../backend/bluejs/src/vm/builtins/promise_combinators.rs) | 389 / 399 (97.49%) | 33 / 34 (97.06%) | 790 / 876 (90.18%) | ☐ |  |
| [`vm/builtins/promise_core.rs`](../../../backend/bluejs/src/vm/builtins/promise_core.rs) | 632 / 674 (93.77%) | 56 / 57 (98.25%) | 1,094 / 1,202 (91.01%) | ☐ |  |
| [`vm/builtins/promises.rs`](../../../backend/bluejs/src/vm/builtins/promises.rs) | 782 / 882 (88.66%) | 55 / 58 (94.83%) | 1,438 / 1,620 (88.77%) | ☐ |  |
| [`vm/builtins/resource_management.rs`](../../../backend/bluejs/src/vm/builtins/resource_management.rs) | 502 / 547 (91.77%) | 32 / 34 (94.12%) | 746 / 829 (89.99%) | ☐ |  |
| [`vm/builtins/set_methods.rs`](../../../backend/bluejs/src/vm/builtins/set_methods.rs) | 240 / 244 (98.36%) | 28 / 28 (100.00%) | 512 / 575 (89.04%) | ☐ |  |
| [`vm/builtins/tests.rs`](../../../backend/bluejs/src/vm/builtins/tests.rs) | - | - | - | - | Test source; not a coverage target |
| [`vm/builtins/typed_arrays.rs`](../../../backend/bluejs/src/vm/builtins/typed_arrays.rs) | 686 / 840 (81.67%) | 35 / 42 (83.33%) | 1,374 / 1,733 (79.28%) | ☐ |  |
| [`vm/builtins/uint8array.rs`](../../../backend/bluejs/src/vm/builtins/uint8array.rs) | 286 / 494 (57.89%) | 26 / 35 (74.29%) | 454 / 768 (59.11%) | ☐ |  |
| [`vm/completion.rs`](../../../backend/bluejs/src/vm/completion.rs) | 111 / 111 (100.00%) | 9 / 9 (100.00%) | 162 / 162 (100.00%) | ☑ |  |
| [`vm/debugger.rs`](../../../backend/bluejs/src/vm/debugger.rs) | 243 / 293 (82.94%) | 23 / 24 (95.83%) | 348 / 406 (85.71%) | ☐ |  |
| [`vm/errors.rs`](../../../backend/bluejs/src/vm/errors.rs) | 342 / 376 (90.96%) | 21 / 24 (87.50%) | 640 / 764 (83.77%) | ☐ |  |
| [`vm/execution.rs`](../../../backend/bluejs/src/vm/execution.rs) | 1,304 / 1,431 (91.13%) | 130 / 139 (93.53%) | 2,262 / 2,558 (88.43%) | ☐ |  |
| [`vm/functions.rs`](../../../backend/bluejs/src/vm/functions.rs) | 103 / 104 (99.04%) | 4 / 4 (100.00%) | 198 / 213 (92.96%) | ☐ |  |
| [`vm/host_objects.rs`](../../../backend/bluejs/src/vm/host_objects.rs) | 507 / 576 (88.02%) | 40 / 59 (67.80%) | 595 / 708 (84.04%) | ☐ |  |
| [`vm/interpreter.rs`](../../../backend/bluejs/src/vm/interpreter.rs) | 1,169 / 1,258 (92.93%) | 56 / 62 (90.32%) | 2,834 / 3,145 (90.11%) | ☐ |  |
| [`vm/intl.rs`](../../../backend/bluejs/src/vm/intl.rs) | 480 / 580 (82.76%) | 16 / 17 (94.12%) | 668 / 798 (83.71%) | ☐ |  |
| [`vm/intl/collator_locale.rs`](../../../backend/bluejs/src/vm/intl/collator_locale.rs) | 396 / 403 (98.26%) | 41 / 41 (100.00%) | 674 / 722 (93.35%) | ☐ |  |
| [`vm/intl/date_time.rs`](../../../backend/bluejs/src/vm/intl/date_time.rs) | 729 / 811 (89.89%) | 51 / 56 (91.07%) | 1,133 / 1,275 (88.86%) | ☐ |  |
| [`vm/intl/list_duration.rs`](../../../backend/bluejs/src/vm/intl/list_duration.rs) | 561 / 584 (96.06%) | 46 / 55 (83.64%) | 867 / 957 (90.60%) | ☐ |  |
| [`vm/intl/number_options.rs`](../../../backend/bluejs/src/vm/intl/number_options.rs) | 409 / 443 (92.33%) | 31 / 32 (96.88%) | 570 / 653 (87.29%) | ☐ |  |
| [`vm/intl/number_runtime.rs`](../../../backend/bluejs/src/vm/intl/number_runtime.rs) | 344 / 388 (88.66%) | 23 / 28 (82.14%) | 477 / 555 (85.95%) | ☐ |  |
| [`vm/intl/plural_segmenter.rs`](../../../backend/bluejs/src/vm/intl/plural_segmenter.rs) | 537 / 557 (96.41%) | 49 / 54 (90.74%) | 796 / 875 (90.97%) | ☐ |  |
| [`vm/intl/shared.rs`](../../../backend/bluejs/src/vm/intl/shared.rs) | 652 / 699 (93.28%) | 61 / 68 (89.71%) | 1,086 / 1,248 (87.02%) | ☐ |  |
| [`vm/intrinsics.rs`](../../../backend/bluejs/src/vm/intrinsics.rs) | 431 / 436 (98.85%) | 5 / 5 (100.00%) | 582 / 594 (97.98%) | ☐ |  |
| [`vm/json.rs`](../../../backend/bluejs/src/vm/json.rs) | 694 / 757 (91.68%) | 58 / 60 (96.67%) | 1,397 / 1,590 (87.86%) | ☐ |  |
| [`vm/lifecycle.rs`](../../../backend/bluejs/src/vm/lifecycle.rs) | 193 / 193 (100.00%) | 13 / 13 (100.00%) | 209 / 212 (98.58%) | ☐ |  |
| [`vm/modules.rs`](../../../backend/bluejs/src/vm/modules.rs) | 1,366 / 1,497 (91.25%) | 69 / 77 (89.61%) | 2,236 / 2,533 (88.27%) | ☐ |  |
| [`vm/modules/deferred.rs`](../../../backend/bluejs/src/vm/modules/deferred.rs) | 275 / 289 (95.16%) | 27 / 28 (96.43%) | 444 / 489 (90.80%) | ☐ |  |
| [`vm/modules/namespace.rs`](../../../backend/bluejs/src/vm/modules/namespace.rs) | 176 / 206 (85.44%) | 11 / 17 (64.71%) | 294 / 365 (80.55%) | ☐ |  |
| [`vm/modules/synthetic.rs`](../../../backend/bluejs/src/vm/modules/synthetic.rs) | 97 / 100 (97.00%) | 3 / 3 (100.00%) | 132 / 137 (96.35%) | ☐ |  |
| [`vm/native_stack.rs`](../../../backend/bluejs/src/vm/native_stack.rs) | 60 / 60 (100.00%) | 14 / 14 (100.00%) | 105 / 106 (99.06%) | ☐ |  |
| [`vm/operations.rs`](../../../backend/bluejs/src/vm/operations.rs) | 573 / 620 (92.42%) | 44 / 47 (93.62%) | 1,031 / 1,169 (88.20%) | ☐ |  |
| [`vm/properties.rs`](../../../backend/bluejs/src/vm/properties.rs) | 606 / 657 (92.24%) | 62 / 70 (88.57%) | 1,129 / 1,295 (87.18%) | ☐ |  |
| [`vm/realm_reentrancy.rs`](../../../backend/bluejs/src/vm/realm_reentrancy.rs) | 22 / 22 (100.00%) | 8 / 8 (100.00%) | 34 / 34 (100.00%) | ☑ |  |
| [`vm/regexp.rs`](../../../backend/bluejs/src/vm/regexp.rs) | 1,019 / 1,054 (96.68%) | 60 / 60 (100.00%) | 1,905 / 2,089 (91.19%) | ☐ |  |
| [`vm/shadow_realm.rs`](../../../backend/bluejs/src/vm/shadow_realm.rs) | 305 / 362 (84.25%) | 23 / 28 (82.14%) | 538 / 630 (85.40%) | ☐ |  |
| [`vm/temporal.rs`](../../../backend/bluejs/src/vm/temporal.rs) | 534 / 564 (94.68%) | 11 / 11 (100.00%) | 518 / 567 (91.36%) | ☐ |  |
| [`vm/temporal/calendar.rs`](../../../backend/bluejs/src/vm/temporal/calendar.rs) | 147 / 147 (100.00%) | 14 / 14 (100.00%) | 215 / 215 (100.00%) | ☑ |  |
| [`vm/temporal/conversion.rs`](../../../backend/bluejs/src/vm/temporal/conversion.rs) | - | - | - | - | Declarations/re-exports only; no executable code |
| [`vm/temporal/conversion/calendar_fields.rs`](../../../backend/bluejs/src/vm/temporal/conversion/calendar_fields.rs) | 286 / 290 (98.62%) | 22 / 27 (81.48%) | 456 / 485 (94.02%) | ☐ |  |
| [`vm/temporal/conversion/from_value.rs`](../../../backend/bluejs/src/vm/temporal/conversion/from_value.rs) | 324 / 351 (92.31%) | 20 / 21 (95.24%) | 554 / 600 (92.33%) | ☐ |  |
| [`vm/temporal/conversion/getters.rs`](../../../backend/bluejs/src/vm/temporal/conversion/getters.rs) | 103 / 111 (92.79%) | 4 / 7 (57.14%) | 148 / 164 (90.24%) | ☐ |  |
| [`vm/temporal/conversion/numeric.rs`](../../../backend/bluejs/src/vm/temporal/conversion/numeric.rs) | 185 / 186 (99.46%) | 21 / 24 (87.50%) | 241 / 259 (93.05%) | ☐ |  |
| [`vm/temporal/conversion/options.rs`](../../../backend/bluejs/src/vm/temporal/conversion/options.rs) | 124 / 125 (99.20%) | 10 / 12 (83.33%) | 145 / 158 (91.77%) | ☐ |  |
| [`vm/temporal/conversion/zoned_conversion.rs`](../../../backend/bluejs/src/vm/temporal/conversion/zoned_conversion.rs) | 103 / 118 (87.29%) | 4 / 11 (36.36%) | 175 / 206 (84.95%) | ☐ |  |
| [`vm/temporal/dates.rs`](../../../backend/bluejs/src/vm/temporal/dates.rs) | - | - | - | - | Declarations/re-exports only; no executable code |
| [`vm/temporal/dates/arithmetic.rs`](../../../backend/bluejs/src/vm/temporal/dates/arithmetic.rs) | 197 / 197 (100.00%) | 8 / 8 (100.00%) | 277 / 283 (97.88%) | ☐ |  |
| [`vm/temporal/dates/comparison.rs`](../../../backend/bluejs/src/vm/temporal/dates/comparison.rs) | 48 / 48 (100.00%) | 3 / 3 (100.00%) | 65 / 66 (98.48%) | ☐ |  |
| [`vm/temporal/dates/construction.rs`](../../../backend/bluejs/src/vm/temporal/dates/construction.rs) | 208 / 224 (92.86%) | 10 / 12 (83.33%) | 233 / 252 (92.46%) | ☐ |  |
| [`vm/temporal/dates/conversions.rs`](../../../backend/bluejs/src/vm/temporal/dates/conversions.rs) | 66 / 66 (100.00%) | 3 / 6 (50.00%) | 105 / 119 (88.24%) | ☐ |  |
| [`vm/temporal/dates/date_time.rs`](../../../backend/bluejs/src/vm/temporal/dates/date_time.rs) | 122 / 125 (97.60%) | 8 / 9 (88.89%) | 202 / 214 (94.39%) | ☐ |  |
| [`vm/temporal/dates/formatting.rs`](../../../backend/bluejs/src/vm/temporal/dates/formatting.rs) | 165 / 167 (98.80%) | 5 / 6 (83.33%) | 222 / 229 (96.94%) | ☐ |  |
| [`vm/temporal/dates/now_and_zone.rs`](../../../backend/bluejs/src/vm/temporal/dates/now_and_zone.rs) | 178 / 183 (97.27%) | 16 / 20 (80.00%) | 247 / 266 (92.86%) | ☐ |  |
| [`vm/temporal/dates/with.rs`](../../../backend/bluejs/src/vm/temporal/dates/with.rs) | 135 / 135 (100.00%) | 6 / 6 (100.00%) | 254 / 264 (96.21%) | ☐ |  |
| [`vm/temporal/duration_conversion.rs`](../../../backend/bluejs/src/vm/temporal/duration_conversion.rs) | 45 / 46 (97.83%) | 3 / 4 (75.00%) | 70 / 77 (90.91%) | ☐ |  |
| [`vm/temporal/duration_math.rs`](../../../backend/bluejs/src/vm/temporal/duration_math.rs) | 305 / 305 (100.00%) | 29 / 29 (100.00%) | 361 / 361 (100.00%) | ☑ |  |
| [`vm/temporal/duration_operations.rs`](../../../backend/bluejs/src/vm/temporal/duration_operations.rs) | 600 / 600 (100.00%) | 26 / 27 (96.30%) | 900 / 923 (97.51%) | ☐ |  |
| [`vm/temporal/duration_relative.rs`](../../../backend/bluejs/src/vm/temporal/duration_relative.rs) | 491 / 499 (98.40%) | 37 / 44 (84.09%) | 639 / 677 (94.39%) | ☐ |  |
| [`vm/temporal/epoch.rs`](../../../backend/bluejs/src/vm/temporal/epoch.rs) | 136 / 136 (100.00%) | 13 / 13 (100.00%) | 237 / 237 (100.00%) | ☑ |  |
| [`vm/temporal/instant.rs`](../../../backend/bluejs/src/vm/temporal/instant.rs) | 399 / 443 (90.07%) | 20 / 32 (62.50%) | 574 / 667 (86.06%) | ☐ |  |
| [`vm/temporal/iso.rs`](../../../backend/bluejs/src/vm/temporal/iso.rs) | 17 / 17 (100.00%) | 3 / 3 (100.00%) | 24 / 24 (100.00%) | ☑ |  |
| [`vm/temporal/iso/annotations.rs`](../../../backend/bluejs/src/vm/temporal/iso/annotations.rs) | 140 / 140 (100.00%) | 21 / 21 (100.00%) | 275 / 285 (96.49%) | ☐ |  |
| [`vm/temporal/iso/datetime.rs`](../../../backend/bluejs/src/vm/temporal/iso/datetime.rs) | 127 / 127 (100.00%) | 13 / 13 (100.00%) | 217 / 219 (99.09%) | ☐ |  |
| [`vm/temporal/iso/duration.rs`](../../../backend/bluejs/src/vm/temporal/iso/duration.rs) | 101 / 101 (100.00%) | 4 / 4 (100.00%) | 165 / 168 (98.21%) | ☐ |  |
| [`vm/temporal/iso/offset.rs`](../../../backend/bluejs/src/vm/temporal/iso/offset.rs) | 123 / 124 (99.19%) | 7 / 7 (100.00%) | 227 / 233 (97.42%) | ☐ |  |
| [`vm/temporal/iso/scan.rs`](../../../backend/bluejs/src/vm/temporal/iso/scan.rs) | 225 / 225 (100.00%) | 31 / 31 (100.00%) | 452 / 473 (95.56%) | ☐ |  |
| [`vm/temporal/iso/tests.rs`](../../../backend/bluejs/src/vm/temporal/iso/tests.rs) | - | - | - | - | Test source; not a coverage target |
| [`vm/temporal/plain_date.rs`](../../../backend/bluejs/src/vm/temporal/plain_date.rs) | - | - | - | - | Declarations/re-exports only; no executable code |
| [`vm/temporal/plain_date/calendar_add.rs`](../../../backend/bluejs/src/vm/temporal/plain_date/calendar_add.rs) | 131 / 131 (100.00%) | 3 / 3 (100.00%) | 230 / 241 (95.44%) | ☐ |  |
| [`vm/temporal/plain_date/calendar_difference.rs`](../../../backend/bluejs/src/vm/temporal/plain_date/calendar_difference.rs) | 183 / 186 (98.39%) | 7 / 7 (100.00%) | 335 / 339 (98.82%) | ☐ |  |
| [`vm/temporal/plain_date/format.rs`](../../../backend/bluejs/src/vm/temporal/plain_date/format.rs) | 21 / 22 (95.45%) | 3 / 3 (100.00%) | 38 / 39 (97.44%) | ☐ |  |
| [`vm/temporal/plain_date/iso_date.rs`](../../../backend/bluejs/src/vm/temporal/plain_date/iso_date.rs) | 111 / 112 (99.11%) | 16 / 16 (100.00%) | 203 / 204 (99.51%) | ☐ |  |
| [`vm/temporal/plain_date/month_structure.rs`](../../../backend/bluejs/src/vm/temporal/plain_date/month_structure.rs) | 110 / 110 (100.00%) | 11 / 11 (100.00%) | 168 / 177 (94.92%) | ☐ |  |
| [`vm/temporal/plain_date/round_duration.rs`](../../../backend/bluejs/src/vm/temporal/plain_date/round_duration.rs) | 120 / 124 (96.77%) | 5 / 5 (100.00%) | 182 / 188 (96.81%) | ☐ |  |
| [`vm/temporal/plain_date/tests.rs`](../../../backend/bluejs/src/vm/temporal/plain_date/tests.rs) | - | - | - | - | Test source; not a coverage target |
| [`vm/temporal/plain_date_time_difference.rs`](../../../backend/bluejs/src/vm/temporal/plain_date_time_difference.rs) | 775 / 776 (99.87%) | 54 / 54 (100.00%) | 1,089 / 1,105 (98.55%) | ☐ |  |
| [`vm/temporal/plain_month_day.rs`](../../../backend/bluejs/src/vm/temporal/plain_month_day.rs) | 279 / 279 (100.00%) | 28 / 28 (100.00%) | 386 / 386 (100.00%) | ☑ |  |
| [`vm/temporal/plain_time.rs`](../../../backend/bluejs/src/vm/temporal/plain_time.rs) | 584 / 626 (93.29%) | 37 / 44 (84.09%) | 844 / 922 (91.54%) | ☐ |  |
| [`vm/temporal/plain_year_month.rs`](../../../backend/bluejs/src/vm/temporal/plain_year_month.rs) | 143 / 143 (100.00%) | 13 / 13 (100.00%) | 193 / 193 (100.00%) | ☑ |  |
| [`vm/temporal/receiver.rs`](../../../backend/bluejs/src/vm/temporal/receiver.rs) | 40 / 40 (100.00%) | 2 / 2 (100.00%) | 39 / 40 (97.50%) | ☐ |  |
| [`vm/temporal/rounding.rs`](../../../backend/bluejs/src/vm/temporal/rounding.rs) | 395 / 395 (100.00%) | 33 / 33 (100.00%) | 643 / 644 (99.84%) | ☐ |  |
| [`vm/temporal/time_zone.rs`](../../../backend/bluejs/src/vm/temporal/time_zone.rs) | 503 / 503 (100.00%) | 47 / 48 (97.92%) | 1,007 / 1,013 (99.41%) | ☐ |  |
| [`vm/temporal/time_zone_id.rs`](../../../backend/bluejs/src/vm/temporal/time_zone_id.rs) | 192 / 192 (100.00%) | 28 / 28 (100.00%) | 457 / 465 (98.28%) | ☐ |  |
| [`vm/temporal/year_month.rs`](../../../backend/bluejs/src/vm/temporal/year_month.rs) | 921 / 949 (97.05%) | 68 / 82 (82.93%) | 1,537 / 1,635 (94.01%) | ☐ |  |
| [`vm/temporal/zoned.rs`](../../../backend/bluejs/src/vm/temporal/zoned.rs) | - | - | - | - | Declarations/re-exports only; no executable code |
| [`vm/temporal/zoned/arithmetic.rs`](../../../backend/bluejs/src/vm/temporal/zoned/arithmetic.rs) | 163 / 169 (96.45%) | 8 / 9 (88.89%) | 275 / 288 (95.49%) | ☐ |  |
| [`vm/temporal/zoned/conversions.rs`](../../../backend/bluejs/src/vm/temporal/zoned/conversions.rs) | 114 / 115 (99.13%) | 8 / 9 (88.89%) | 159 / 172 (92.44%) | ☐ |  |
| [`vm/temporal/zoned/formatting.rs`](../../../backend/bluejs/src/vm/temporal/zoned/formatting.rs) | 149 / 152 (98.03%) | 6 / 8 (75.00%) | 241 / 255 (94.51%) | ☐ |  |
| [`vm/temporal/zoned/from_value.rs`](../../../backend/bluejs/src/vm/temporal/zoned/from_value.rs) | 173 / 181 (95.58%) | 13 / 15 (86.67%) | 250 / 265 (94.34%) | ☐ |  |
| [`vm/temporal/zoned/resolution.rs`](../../../backend/bluejs/src/vm/temporal/zoned/resolution.rs) | 99 / 99 (100.00%) | 7 / 7 (100.00%) | 126 / 127 (99.21%) | ☐ |  |
| [`vm/temporal/zoned/since_until.rs`](../../../backend/bluejs/src/vm/temporal/zoned/since_until.rs) | 137 / 137 (100.00%) | 8 / 8 (100.00%) | 202 / 209 (96.65%) | ☐ |  |
| [`vm/temporal/zoned/with.rs`](../../../backend/bluejs/src/vm/temporal/zoned/with.rs) | 199 / 199 (100.00%) | 9 / 9 (100.00%) | 352 / 369 (95.39%) | ☐ |  |
| [`vm/temporal/zoned_date_time.rs`](../../../backend/bluejs/src/vm/temporal/zoned_date_time.rs) | 217 / 221 (98.19%) | 15 / 15 (100.00%) | 361 / 368 (98.10%) | ☐ |  |
| [`vm/temporal/zoned_difference.rs`](../../../backend/bluejs/src/vm/temporal/zoned_difference.rs) | 763 / 767 (99.48%) | 43 / 43 (100.00%) | 1,056 / 1,090 (96.88%) | ☐ |  |
| [`vm/test262.rs`](../../../backend/bluejs/src/vm/test262.rs) | 57 / 57 (100.00%) | 6 / 6 (100.00%) | 96 / 97 (98.97%) | ☐ |  |
| [`vm/test262/assertions.rs`](../../../backend/bluejs/src/vm/test262/assertions.rs) | 287 / 297 (96.63%) | 16 / 17 (94.12%) | 750 / 838 (89.50%) | ☐ |  |
| [`vm/test262/cases.rs`](../../../backend/bluejs/src/vm/test262/cases.rs) | 620 / 648 (95.68%) | 39 / 45 (86.67%) | 1,129 / 1,263 (89.39%) | ☐ |  |
| [`vm/test262/foreign.rs`](../../../backend/bluejs/src/vm/test262/foreign.rs) | 1,335 / 1,474 (90.57%) | 124 / 143 (86.71%) | 2,217 / 2,640 (83.98%) | ☐ |  |
| [`vm/test262/harness.rs`](../../../backend/bluejs/src/vm/test262/harness.rs) | 223 / 248 (89.92%) | 12 / 12 (100.00%) | 369 / 418 (88.28%) | ☐ |  |
| [`vm/test262/reverse.rs`](../../../backend/bluejs/src/vm/test262/reverse.rs) | 249 / 256 (97.27%) | 24 / 26 (92.31%) | 427 / 464 (92.03%) | ☐ |  |
| [`vm/test262_agents.rs`](../../../backend/bluejs/src/vm/test262_agents.rs) | 412 / 443 (93.00%) | 42 / 49 (85.71%) | 593 / 664 (89.31%) | ☐ |  |
| [`vm/tests.rs`](../../../backend/bluejs/src/vm/tests.rs) | - | - | - | - | Test source; not a coverage target |
| **Total (159 instrumented files)** | **66,871 / 71,445 (93.60%)** | **5,047 / 5,419 (93.14%)** | **115,508 / 127,238 (90.78%)** | ☐ |  |

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
