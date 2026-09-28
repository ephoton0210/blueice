# macOS Test262 Report

## Current complete inventory (2026-09-29)

The pinned, unfiltered Test262 snapshot was rerun on macOS 27.0 (build 26A428, Apple silicon) with Rust/Cargo 1.95.0 and Python 3.14.6, using source commit `dee6e8718` plus the uncommitted coverage changes below. The verified corpus revision is `72faf8ec1445c55149615e8b35187830783aba1a` and includes main, proposals, and staging. The complete command was `/tmp/bluejs-conformance-venv/bin/python backend/bluejs/test262/run.py --corpus development/browser_core/reference/test262 --jobs 8 --output target/test262-macos-20260929-batch1 --progress-interval 60`, after `cargo build -p blueice-bluejs --bins --offline`; it completed in 239.079 seconds. The runner verified the pinned corpus marker, manifest, and every file before execution. `analyze.py` reconciled all 53,582 files and 102,926 modes against `results.jsonl` and `summary.json`. All 102,926 per-mode statuses, expected outcomes, and actual outcome kinds and phases match the preceding run.

Adapter SHA-256: `01bde81372a2e20a126cb823bedfdae89f326b64055844a2461a2532c2d45183`. RegExp worker SHA-256: `d1222aade82c742bf98d631cc932740c596a48ba6d8668a539295f8b757e6473`. Runner SHA-256: `d56c75f03b0422f20fea1dcd8a10be3ea81905f4a79d7fead0abb1ca6986d0ba`. The [checked-in summary](test262-summary.json) contains the complete feature and top-level group counts; the full per-mode evidence is in `target/test262-macos-20260929-batch1/results.jsonl`.

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

The earlier macOS 26.6.2 run at `eaeb5c1` recorded 99,899 pass, 3,025 fail, and 2 timeout out of 102,926 modes (97.059%). Its three-platform comparison belongs to that earlier source revision. The current 2026-09-29 inventory above replaces it as the macOS Test262 status. The complete 2026-09-25 run at `e9c15268` had the same 102,921 pass, 1 stale-corpus, and 4 excluded outcomes. Exact path/mode/status/source-hash comparisons found zero changes across all 102,926 modes between that run and the first 2026-09-28 run, between the first and preceding final 2026-09-28 runs, between that final and complete73, between complete73 and complete74, between complete74 and complete75, and between complete75 and complete76. The latest rerun also matched the 2026-09-28 batch5 run in every status, expected outcome, actual kind, actual phase, and source hash. The latest Temporal receiver refactor therefore preserved every recorded outcome.

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


## Later BlueJS per-file coverage (2026-09-29)

This is a separate BlueJS coverage measurement at commit `6297fc57` with uncommitted changes on Darwin 27.0 (`arm64`), rustc 1.95.0 (59807616e 2026-04-14) and `cargo-llvm-cov 0.9.1`. It is independent of the Test262 status and historical verification above. With Python and PyYAML installed, reproduce it with `python backend/bluejs/coverage_file.py --update-macos-report --include-test262`. This measurement cleared prior LLVM execution profiles, reused instrumented Cargo build artifacts, ran the complete default BlueJS Rust test suite, then ran the full pinned Test262 inventory with the instrumented adapter, exported fresh per-file JSON and source-line text, and released raw profiles and incremental compilation caches afterward. On Linux, test-binary DWARF was removed while retaining the coverage maps. The opt-in Node oracle was not included. Workspace coverage was not remeasured at this revision.

Each measured cell shows LLVM JSON's raw covered / instrumented counts and the coverage rate. All 178 Rust files under `backend/bluejs/src/` are listed: 160 have LLVM counters; 18 use `-` with an individual reason in `Note`. A `0%` result requires a positive instrumented denominator and zero covered units. `☑` means **lines, functions and regions all reach 100%**; `☐` means at least one is below 100%. The total aggregates only instrumented files. LLVM can count separate compiled instances of the same source in different test binaries. `Note` shows the union across those binaries when its counts differ: each source location is counted once and considered covered if any binary executes it. The raw LLVM counts in the main columns determine completion. Region coverage is separate from branch coverage. To rerun any one file independently, use `python backend/bluejs/coverage_file.py ast.rs --include-test262` (replace `ast.rs` with its source path). Each invocation reruns the entire measured suite, since tests outside a file can still exercise it.

| Source file (relative to `backend/bluejs/src/`) | Lines | Functions | Regions | Complete | Note |
| --- | ---: | ---: | ---: | :---: | --- |
| [`ast.rs`](../../../backend/bluejs/src/ast.rs) | 903 / 903 (100.00%) | 90 / 90 (100.00%) | 1,184 / 1,184 (100.00%) | ☑ | Unique source-location union: lines 854/854, functions 90/90, regions 1,184/1,184 |
| [`bin/bluejs-regexp-worker.rs`](../../../backend/bluejs/src/bin/bluejs-regexp-worker.rs) | 3 / 3 (100.00%) | 1 / 1 (100.00%) | 3 / 3 (100.00%) | ☑ | Unique source-location union: lines 3/3, functions 1/1, regions 3/3 |
| [`bin/bluejs-test262.rs`](../../../backend/bluejs/src/bin/bluejs-test262.rs) | 631 / 631 (100.00%) | 56 / 56 (100.00%) | 1,211 / 1,211 (100.00%) | ☑ | Unique source-location union: lines 613/613, functions 56/56, regions 1,211/1,211 |
| [`bytecode.rs`](../../../backend/bluejs/src/bytecode.rs) | 104 / 104 (100.00%) | 16 / 16 (100.00%) | 111 / 111 (100.00%) | ☑ | Unique source-location union: lines 103/103, functions 16/16, regions 111/111 |
| [`compiler.rs`](../../../backend/bluejs/src/compiler.rs) | 1,514 / 1,514 (100.00%) | 146 / 146 (100.00%) | 2,131 / 2,131 (100.00%) | ☑ | Unique source-location union: lines 1,443/1,443, functions 146/146, regions 2,131/2,131 |
| [`compiler/expressions.rs`](../../../backend/bluejs/src/compiler/expressions.rs) | 1,540 / 1,540 (100.00%) | 42 / 42 (100.00%) | 3,408 / 3,408 (100.00%) | ☑ | Unique source-location union: lines 1,528/1,528, functions 42/42, regions 3,408/3,408 |
| [`compiler/expressions/tests.rs`](../../../backend/bluejs/src/compiler/expressions/tests.rs) | - | - | - | - | Test source; not a coverage target |
| [`compiler/functions.rs`](../../../backend/bluejs/src/compiler/functions.rs) | 993 / 993 (100.00%) | 52 / 52 (100.00%) | 1,836 / 1,836 (100.00%) | ☑ | Unique source-location union: lines 964/964, functions 52/52, regions 1,836/1,836 |
| [`compiler/functions/tests.rs`](../../../backend/bluejs/src/compiler/functions/tests.rs) | - | - | - | - | Test source; not a coverage target |
| [`compiler/private_validation.rs`](../../../backend/bluejs/src/compiler/private_validation.rs) | 340 / 340 (100.00%) | 32 / 32 (100.00%) | 701 / 701 (100.00%) | ☑ | Unique source-location union: lines 321/321, functions 32/32, regions 701/701 |
| [`compiler/private_validation/tests.rs`](../../../backend/bluejs/src/compiler/private_validation/tests.rs) | - | - | - | - | Test source; not a coverage target |
| [`compiler/statements.rs`](../../../backend/bluejs/src/compiler/statements.rs) | 1,348 / 1,348 (100.00%) | 69 / 69 (100.00%) | 2,637 / 2,637 (100.00%) | ☑ | Unique source-location union: lines 1,322/1,322, functions 69/69, regions 2,637/2,637 |
| [`compiler/statements/tests.rs`](../../../backend/bluejs/src/compiler/statements/tests.rs) | - | - | - | - | Test source; not a coverage target |
| [`heap.rs`](../../../backend/bluejs/src/heap.rs) | 744 / 744 (100.00%) | 78 / 78 (100.00%) | 1,190 / 1,190 (100.00%) | ☑ | Unique source-location union: lines 723/723, functions 78/78, regions 1,190/1,190 |
| [`heap/binary_data.rs`](../../../backend/bluejs/src/heap/binary_data.rs) | 850 / 850 (100.00%) | 77 / 77 (100.00%) | 1,302 / 1,302 (100.00%) | ☑ | Unique source-location union: lines 832/832, functions 77/77, regions 1,302/1,302 |
| [`heap/collection_iteration.rs`](../../../backend/bluejs/src/heap/collection_iteration.rs) | 93 / 93 (100.00%) | 5 / 5 (100.00%) | 120 / 120 (100.00%) | ☑ | Unique source-location union: lines 93/93, functions 5/5, regions 120/120 |
| [`heap/core.rs`](../../../backend/bluejs/src/heap/core.rs) | 808 / 808 (100.00%) | 72 / 72 (100.00%) | 1,211 / 1,211 (100.00%) | ☑ | Unique source-location union: lines 792/792, functions 72/72, regions 1,211/1,211 |
| [`heap/debugger.rs`](../../../backend/bluejs/src/heap/debugger.rs) | 163 / 163 (100.00%) | 10 / 10 (100.00%) | 239 / 239 (100.00%) | ☑ | Unique source-location union: lines 161/161, functions 10/10, regions 239/239 |
| [`heap/exotic.rs`](../../../backend/bluejs/src/heap/exotic.rs) | 947 / 947 (100.00%) | 94 / 94 (100.00%) | 1,112 / 1,112 (100.00%) | ☑ | Unique source-location union: lines 942/942, functions 94/94, regions 1,112/1,112 |
| [`heap/lifecycle.rs`](../../../backend/bluejs/src/heap/lifecycle.rs) | 294 / 294 (100.00%) | 31 / 31 (100.00%) | 504 / 504 (100.00%) | ☑ | Unique source-location union: lines 279/279, functions 31/31, regions 504/504 |
| [`heap/object_storage.rs`](../../../backend/bluejs/src/heap/object_storage.rs) | 742 / 750 (98.93%) | 47 / 47 (100.00%) | 1,286 / 1,319 (97.50%) | ☐ | Unique source-location union: lines 732/732, functions 47/47, regions 1,319/1,319 |
| [`heap/tests.rs`](../../../backend/bluejs/src/heap/tests.rs) | - | - | - | - | Test source; not a coverage target |
| [`intl.rs`](../../../backend/bluejs/src/intl.rs) | 124 / 124 (100.00%) | 19 / 19 (100.00%) | 188 / 188 (100.00%) | ☑ | Unique source-location union: lines 118/118, functions 19/19, regions 188/188 |
| [`lib.rs`](../../../backend/bluejs/src/lib.rs) | - | - | - | - | Declarations/re-exports only; no executable code |
| [`native.rs`](../../../backend/bluejs/src/native.rs) | 530 / 532 (99.62%) | 27 / 27 (100.00%) | 953 / 983 (96.95%) | ☐ | Unique source-location union: lines 525/525, functions 27/27, regions 983/983 |
| [`page_runtime.rs`](../../../backend/bluejs/src/page_runtime.rs) | 1,772 / 1,803 (98.28%) | 107 / 108 (99.07%) | 2,503 / 2,596 (96.42%) | ☐ | Unique source-location union: lines 1,764/1,777, functions 107/108, regions 2,521/2,596 |
| [`parser.rs`](../../../backend/bluejs/src/parser.rs) | 540 / 540 (100.00%) | 72 / 72 (100.00%) | 734 / 734 (100.00%) | ☑ | Unique source-location union: lines 530/530, functions 72/72, regions 734/734 |
| [`parser/expressions.rs`](../../../backend/bluejs/src/parser/expressions.rs) | 946 / 946 (100.00%) | 41 / 41 (100.00%) | 1,670 / 1,670 (100.00%) | ☑ | Unique source-location union: lines 942/942, functions 41/41, regions 1,670/1,670 |
| [`parser/functions.rs`](../../../backend/bluejs/src/parser/functions.rs) | 666 / 666 (100.00%) | 41 / 41 (100.00%) | 1,048 / 1,048 (100.00%) | ☑ | Unique source-location union: lines 653/653, functions 41/41, regions 1,048/1,048 |
| [`parser/module.rs`](../../../backend/bluejs/src/parser/module.rs) | 143 / 143 (100.00%) | 8 / 8 (100.00%) | 243 / 243 (100.00%) | ☑ | Unique source-location union: lines 138/138, functions 8/8, regions 243/243 |
| [`parser/module_items.rs`](../../../backend/bluejs/src/parser/module_items.rs) | 339 / 339 (100.00%) | 13 / 13 (100.00%) | 635 / 635 (100.00%) | ☑ | Unique source-location union: lines 334/334, functions 13/13, regions 635/635 |
| [`parser/patterns.rs`](../../../backend/bluejs/src/parser/patterns.rs) | 175 / 175 (100.00%) | 10 / 10 (100.00%) | 279 / 279 (100.00%) | ☑ | Unique source-location union: lines 173/173, functions 10/10, regions 279/279 |
| [`parser/statements.rs`](../../../backend/bluejs/src/parser/statements.rs) | 586 / 616 (95.13%) | 21 / 21 (100.00%) | 1,110 / 1,177 (94.31%) | ☐ | Unique source-location union: lines 584/614, functions 21/21, regions 1,111/1,177 |
| [`parser/tests.rs`](../../../backend/bluejs/src/parser/tests.rs) | - | - | - | - | Test source; not a coverage target |
| [`primitive.rs`](../../../backend/bluejs/src/primitive.rs) | 277 / 277 (100.00%) | 25 / 25 (100.00%) | 518 / 518 (100.00%) | ☑ | Unique source-location union: lines 268/268, functions 25/25, regions 518/518 |
| [`program_abi.rs`](../../../backend/bluejs/src/program_abi.rs) | 12 / 12 (100.00%) | 2 / 2 (100.00%) | 22 / 22 (100.00%) | ☑ | Unique source-location union: lines 12/12, functions 2/2, regions 22/22 |
| [`program_debug.rs`](../../../backend/bluejs/src/program_debug.rs) | 531 / 536 (99.07%) | 56 / 56 (100.00%) | 842 / 862 (97.68%) | ☐ | Unique source-location union: lines 523/528, functions 56/56, regions 845/862 |
| [`program_debug/ast_ids.rs`](../../../backend/bluejs/src/program_debug/ast_ids.rs) | 160 / 344 (46.51%) | 13 / 18 (72.22%) | 250 / 589 (42.44%) | ☐ | Unique source-location union: lines 160/344, functions 13/18, regions 250/589 |
| [`property.rs`](../../../backend/bluejs/src/property.rs) | 86 / 86 (100.00%) | 21 / 21 (100.00%) | 111 / 111 (100.00%) | ☑ | Unique source-location union: lines 84/84, functions 21/21, regions 111/111 |
| [`regex_backrefs.rs`](../../../backend/bluejs/src/regex_backrefs.rs) | 98 / 98 (100.00%) | 12 / 12 (100.00%) | 186 / 186 (100.00%) | ☑ | Unique source-location union: lines 94/94, functions 12/12, regions 186/186 |
| [`regex_canonicalize.rs`](../../../backend/bluejs/src/regex_canonicalize.rs) | 625 / 625 (100.00%) | 65 / 65 (100.00%) | 1,329 / 1,329 (100.00%) | ☑ | Unique source-location union: lines 601/601, functions 65/65, regions 1,329/1,329 |
| [`regex_escapes.rs`](../../../backend/bluejs/src/regex_escapes.rs) | 220 / 220 (100.00%) | 23 / 23 (100.00%) | 465 / 465 (100.00%) | ☑ | Unique source-location union: lines 210/210, functions 23/23, regions 465/465 |
| [`regex_group_names.rs`](../../../backend/bluejs/src/regex_group_names.rs) | 312 / 312 (100.00%) | 38 / 38 (100.00%) | 591 / 591 (100.00%) | ☑ | Unique source-location union: lines 291/291, functions 38/38, regions 591/591 |
| [`regex_worker.rs`](../../../backend/bluejs/src/regex_worker.rs) | 428 / 428 (100.00%) | 54 / 54 (100.00%) | 618 / 620 (99.68%) | ☐ | Unique source-location union: lines 397/397, functions 54/54, regions 620/620 |
| [`regexp.rs`](../../../backend/bluejs/src/regexp.rs) | 304 / 304 (100.00%) | 27 / 27 (100.00%) | 468 / 468 (100.00%) | ☑ | Unique source-location union: lines 292/292, functions 27/27, regions 468/468 |
| [`source_encoding.rs`](../../../backend/bluejs/src/source_encoding.rs) | 153 / 153 (100.00%) | 20 / 20 (100.00%) | 295 / 295 (100.00%) | ☑ | Unique source-location union: lines 151/151, functions 20/20, regions 295/295 |
| [`string.rs`](../../../backend/bluejs/src/string.rs) | 104 / 104 (100.00%) | 22 / 22 (100.00%) | 168 / 168 (100.00%) | ☑ | Unique source-location union: lines 102/102, functions 22/22, regions 168/168 |
| [`token.rs`](../../../backend/bluejs/src/token.rs) | 1,277 / 1,277 (100.00%) | 106 / 106 (100.00%) | 2,023 / 2,023 (100.00%) | ☑ | Unique source-location union: lines 1,246/1,246, functions 106/106, regions 2,023/2,023 |
| [`value.rs`](../../../backend/bluejs/src/value.rs) | 12 / 12 (100.00%) | 2 / 2 (100.00%) | 20 / 20 (100.00%) | ☑ | Unique source-location union: lines 12/12, functions 2/2, regions 20/20 |
| [`vm.rs`](../../../backend/bluejs/src/vm.rs) | 599 / 599 (100.00%) | 57 / 57 (100.00%) | 881 / 907 (97.13%) | ☐ | Unique source-location union: lines 568/568, functions 57/57, regions 881/907 |
| [`vm/builtins.rs`](../../../backend/bluejs/src/vm/builtins.rs) | 300 / 316 (94.94%) | 18 / 18 (100.00%) | 552 / 573 (96.34%) | ☐ | Unique source-location union: lines 291/307, functions 18/18, regions 552/573 |
| [`vm/builtins/arguments.rs`](../../../backend/bluejs/src/vm/builtins/arguments.rs) | 637 / 696 (91.52%) | 63 / 63 (100.00%) | 1,085 / 1,232 (88.07%) | ☐ | Unique source-location union: lines 595/649, functions 63/63, regions 1,086/1,232 |
| [`vm/builtins/array_change_by_copy.rs`](../../../backend/bluejs/src/vm/builtins/array_change_by_copy.rs) | 197 / 197 (100.00%) | 14 / 14 (100.00%) | 454 / 456 (99.56%) | ☐ | Unique source-location union: lines 190/190, functions 14/14, regions 456/456 |
| [`vm/builtins/array_from_async.rs`](../../../backend/bluejs/src/vm/builtins/array_from_async.rs) | 319 / 338 (94.38%) | 25 / 26 (96.15%) | 774 / 891 (86.87%) | ☐ | Unique source-location union: lines 312/330, functions 25/26, regions 774/891 |
| [`vm/builtins/array_scan.rs`](../../../backend/bluejs/src/vm/builtins/array_scan.rs) | 120 / 120 (100.00%) | 12 / 12 (100.00%) | 174 / 174 (100.00%) | ☑ | Unique source-location union: lines 117/117, functions 12/12, regions 174/174 |
| [`vm/builtins/arrays.rs`](../../../backend/bluejs/src/vm/builtins/arrays.rs) | 1,293 / 1,305 (99.08%) | 80 / 81 (98.77%) | 2,745 / 2,868 (95.71%) | ☐ | Unique source-location union: lines 1,252/1,263, functions 80/81, regions 2,745/2,868 |
| [`vm/builtins/binary_data.rs`](../../../backend/bluejs/src/vm/builtins/binary_data.rs) | 1,422 / 1,511 (94.11%) | 112 / 123 (91.06%) | 2,488 / 2,728 (91.20%) | ☐ | Unique source-location union: lines 1,342/1,415, functions 112/123, regions 2,488/2,728 |
| [`vm/builtins/collection_iteration.rs`](../../../backend/bluejs/src/vm/builtins/collection_iteration.rs) | 111 / 116 (95.69%) | 8 / 8 (100.00%) | 184 / 200 (92.00%) | ☐ | Unique source-location union: lines 106/111, functions 8/8, regions 184/200 |
| [`vm/builtins/collections.rs`](../../../backend/bluejs/src/vm/builtins/collections.rs) | 327 / 355 (92.11%) | 20 / 23 (86.96%) | 710 / 752 (94.41%) | ☐ | Unique source-location union: lines 313/336, functions 20/23, regions 710/752 |
| [`vm/builtins/decorators.rs`](../../../backend/bluejs/src/vm/builtins/decorators.rs) | 498 / 523 (95.22%) | 41 / 41 (100.00%) | 1,011 / 1,123 (90.03%) | ☐ | Unique source-location union: lines 484/508, functions 41/41, regions 1,011/1,123 |
| [`vm/builtins/dynamic.rs`](../../../backend/bluejs/src/vm/builtins/dynamic.rs) | 623 / 645 (96.59%) | 37 / 37 (100.00%) | 1,181 / 1,260 (93.73%) | ☐ | Unique source-location union: lines 596/617, functions 37/37, regions 1,181/1,260 |
| [`vm/builtins/execution.rs`](../../../backend/bluejs/src/vm/builtins/execution.rs) | - | - | - | - | Declarations/re-exports only; no executable code |
| [`vm/builtins/execution/closures.rs`](../../../backend/bluejs/src/vm/builtins/execution/closures.rs) | 354 / 431 (82.13%) | 16 / 16 (100.00%) | 667 / 796 (83.79%) | ☐ | Unique source-location union: lines 390/418, functions 16/16, regions 731/796 |
| [`vm/builtins/execution/runtime.rs`](../../../backend/bluejs/src/vm/builtins/execution/runtime.rs) | 1,435 / 1,484 (96.70%) | 133 / 134 (99.25%) | 2,678 / 2,871 (93.28%) | ☐ | Unique source-location union: lines 1,344/1,389, functions 133/134, regions 2,678/2,871 |
| [`vm/builtins/execution/setup.rs`](../../../backend/bluejs/src/vm/builtins/execution/setup.rs) | 1,010 / 1,052 (96.01%) | 91 / 92 (98.91%) | 1,661 / 1,776 (93.52%) | ☐ | Unique source-location union: lines 947/979, functions 91/92, regions 1,661/1,776 |
| [`vm/builtins/general.rs`](../../../backend/bluejs/src/vm/builtins/general.rs) | 422 / 425 (99.29%) | 35 / 37 (94.59%) | 730 / 749 (97.46%) | ☐ | Unique source-location union: lines 413/414, functions 35/37, regions 731/749 |
| [`vm/builtins/generators.rs`](../../../backend/bluejs/src/vm/builtins/generators.rs) | 1,356 / 1,483 (91.44%) | 66 / 76 (86.84%) | 2,137 / 2,372 (90.09%) | ☐ | Unique source-location union: lines 1,318/1,432, functions 66/76, regions 2,137/2,372 |
| [`vm/builtins/globals.rs`](../../../backend/bluejs/src/vm/builtins/globals.rs) | 935 / 985 (94.92%) | 12 / 12 (100.00%) | 1,384 / 1,465 (94.47%) | ☐ | Unique source-location union: lines 923/971, functions 12/12, regions 1,384/1,465 |
| [`vm/builtins/immutable_arraybuffer.rs`](../../../backend/bluejs/src/vm/builtins/immutable_arraybuffer.rs) | 120 / 120 (100.00%) | 12 / 12 (100.00%) | 214 / 233 (91.85%) | ☐ | Unique source-location union: lines 114/114, functions 12/12, regions 214/233 |
| [`vm/builtins/math.rs`](../../../backend/bluejs/src/vm/builtins/math.rs) | 326 / 333 (97.90%) | 15 / 15 (100.00%) | 597 / 605 (98.68%) | ☐ | Unique source-location union: lines 318/325, functions 15/15, regions 597/605 |
| [`vm/builtins/native_dispatch.rs`](../../../backend/bluejs/src/vm/builtins/native_dispatch.rs) | - | - | - | - | Declarations/re-exports only; no executable code |
| [`vm/builtins/native_dispatch/date.rs`](../../../backend/bluejs/src/vm/builtins/native_dispatch/date.rs) | 660 / 674 (97.92%) | 54 / 54 (100.00%) | 1,169 / 1,231 (94.96%) | ☐ | Unique source-location union: lines 632/646, functions 54/54, regions 1,169/1,231 |
| [`vm/builtins/native_dispatch/dispatch.rs`](../../../backend/bluejs/src/vm/builtins/native_dispatch/dispatch.rs) | 1,223 / 1,261 (96.99%) | 35 / 37 (94.59%) | 3,013 / 3,150 (95.65%) | ☐ | Unique source-location union: lines 1,185/1,212, functions 35/37, regions 3,032/3,150 |
| [`vm/builtins/numbers.rs`](../../../backend/bluejs/src/vm/builtins/numbers.rs) | 241 / 244 (98.77%) | 14 / 15 (93.33%) | 421 / 430 (97.91%) | ☐ | Unique source-location union: lines 238/240, functions 14/15, regions 422/430 |
| [`vm/builtins/numbers/tests.rs`](../../../backend/bluejs/src/vm/builtins/numbers/tests.rs) | - | - | - | - | Test source; not a coverage target |
| [`vm/builtins/object.rs`](../../../backend/bluejs/src/vm/builtins/object.rs) | 1,261 / 1,304 (96.70%) | 83 / 88 (94.32%) | 2,202 / 2,461 (89.48%) | ☐ | Unique source-location union: lines 1,210/1,245, functions 83/88, regions 2,202/2,461 |
| [`vm/builtins/promise_combinators.rs`](../../../backend/bluejs/src/vm/builtins/promise_combinators.rs) | 406 / 412 (98.54%) | 33 / 34 (97.06%) | 809 / 876 (92.35%) | ☐ | Unique source-location union: lines 394/399, functions 33/34, regions 809/876 |
| [`vm/builtins/promise_core.rs`](../../../backend/bluejs/src/vm/builtins/promise_core.rs) | 675 / 700 (96.43%) | 56 / 57 (98.25%) | 1,113 / 1,202 (92.60%) | ☐ | Unique source-location union: lines 649/674, functions 56/57, regions 1,113/1,202 |
| [`vm/builtins/promises.rs`](../../../backend/bluejs/src/vm/builtins/promises.rs) | 915 / 939 (97.44%) | 59 / 59 (100.00%) | 1,539 / 1,638 (93.96%) | ☐ | Unique source-location union: lines 873/893, functions 59/59, regions 1,539/1,638 |
| [`vm/builtins/resource_management.rs`](../../../backend/bluejs/src/vm/builtins/resource_management.rs) | 534 / 558 (95.70%) | 32 / 34 (94.12%) | 778 / 829 (93.85%) | ☐ | Unique source-location union: lines 525/547, functions 32/34, regions 778/829 |
| [`vm/builtins/set_methods.rs`](../../../backend/bluejs/src/vm/builtins/set_methods.rs) | 257 / 261 (98.47%) | 28 / 28 (100.00%) | 526 / 575 (91.48%) | ☐ | Unique source-location union: lines 243/244, functions 28/28, regions 526/575 |
| [`vm/builtins/tests.rs`](../../../backend/bluejs/src/vm/builtins/tests.rs) | - | - | - | - | Test source; not a coverage target |
| [`vm/builtins/typed_arrays.rs`](../../../backend/bluejs/src/vm/builtins/typed_arrays.rs) | 831 / 860 (96.63%) | 39 / 42 (92.86%) | 1,585 / 1,733 (91.46%) | ☐ | Unique source-location union: lines 816/840, functions 39/42, regions 1,585/1,733 |
| [`vm/builtins/uint8array.rs`](../../../backend/bluejs/src/vm/builtins/uint8array.rs) | 483 / 508 (95.08%) | 34 / 35 (97.14%) | 726 / 768 (94.53%) | ☐ | Unique source-location union: lines 472/494, functions 34/35, regions 726/768 |
| [`vm/completion.rs`](../../../backend/bluejs/src/vm/completion.rs) | 113 / 113 (100.00%) | 9 / 9 (100.00%) | 162 / 162 (100.00%) | ☑ | Unique source-location union: lines 111/111, functions 9/9, regions 162/162 |
| [`vm/debugger.rs`](../../../backend/bluejs/src/vm/debugger.rs) | 1,837 / 1,980 (92.78%) | 101 / 109 (92.66%) | 2,826 / 3,011 (93.86%) | ☐ | Unique source-location union: lines 1,797/1,932, functions 101/109, regions 2,826/3,011 |
| [`vm/errors.rs`](../../../backend/bluejs/src/vm/errors.rs) | 365 / 389 (93.83%) | 21 / 24 (87.50%) | 677 / 764 (88.61%) | ☐ | Unique source-location union: lines 356/376, functions 21/24, regions 678/764 |
| [`vm/execution.rs`](../../../backend/bluejs/src/vm/execution.rs) | 1,401 / 1,514 (92.54%) | 133 / 139 (95.68%) | 2,290 / 2,558 (89.52%) | ☐ | Unique source-location union: lines 1,331/1,431, functions 133/139, regions 2,296/2,558 |
| [`vm/functions.rs`](../../../backend/bluejs/src/vm/functions.rs) | 106 / 106 (100.00%) | 4 / 4 (100.00%) | 203 / 213 (95.31%) | ☐ | Unique source-location union: lines 104/104, functions 4/4, regions 205/213 |
| [`vm/host_objects.rs`](../../../backend/bluejs/src/vm/host_objects.rs) | 525 / 620 (84.68%) | 40 / 58 (68.97%) | 597 / 713 (83.73%) | ☐ | Unique source-location union: lines 511/582, functions 40/58, regions 599/713 |
| [`vm/interpreter.rs`](../../../backend/bluejs/src/vm/interpreter.rs) | 1,258 / 1,360 (92.50%) | 57 / 62 (91.94%) | 2,915 / 3,184 (91.55%) | ☐ | Unique source-location union: lines 1,211/1,285, functions 57/62, regions 2,933/3,184 |
| [`vm/intl.rs`](../../../backend/bluejs/src/vm/intl.rs) | 512 / 589 (86.93%) | 17 / 17 (100.00%) | 696 / 798 (87.22%) | ☐ | Unique source-location union: lines 503/580, functions 17/17, regions 696/798 |
| [`vm/intl/collator_locale.rs`](../../../backend/bluejs/src/vm/intl/collator_locale.rs) | 427 / 431 (99.07%) | 41 / 41 (100.00%) | 698 / 722 (96.68%) | ☐ | Unique source-location union: lines 400/403, functions 41/41, regions 698/722 |
| [`vm/intl/date_time.rs`](../../../backend/bluejs/src/vm/intl/date_time.rs) | 791 / 840 (94.17%) | 53 / 56 (94.64%) | 1,209 / 1,275 (94.82%) | ☐ | Unique source-location union: lines 767/811, functions 53/56, regions 1,209/1,275 |
| [`vm/intl/list_duration.rs`](../../../backend/bluejs/src/vm/intl/list_duration.rs) | 593 / 618 (95.95%) | 47 / 55 (85.45%) | 890 / 957 (93.00%) | ☐ | Unique source-location union: lines 569/584, functions 47/55, regions 890/957 |
| [`vm/intl/number_options.rs`](../../../backend/bluejs/src/vm/intl/number_options.rs) | 441 / 456 (96.71%) | 31 / 32 (96.88%) | 629 / 653 (96.32%) | ☐ | Unique source-location union: lines 429/443, functions 31/32, regions 629/653 |
| [`vm/intl/number_runtime.rs`](../../../backend/bluejs/src/vm/intl/number_runtime.rs) | 386 / 404 (95.54%) | 25 / 28 (89.29%) | 523 / 555 (94.23%) | ☐ | Unique source-location union: lines 374/388, functions 25/28, regions 523/555 |
| [`vm/intl/plural_segmenter.rs`](../../../backend/bluejs/src/vm/intl/plural_segmenter.rs) | 571 / 586 (97.44%) | 50 / 54 (92.59%) | 828 / 873 (94.85%) | ☐ | Unique source-location union: lines 544/555, functions 50/54, regions 828/873 |
| [`vm/intl/shared.rs`](../../../backend/bluejs/src/vm/intl/shared.rs) | 690 / 737 (93.62%) | 62 / 68 (91.18%) | 1,131 / 1,248 (90.62%) | ☐ | Unique source-location union: lines 658/699, functions 62/68, regions 1,131/1,248 |
| [`vm/intrinsics.rs`](../../../backend/bluejs/src/vm/intrinsics.rs) | 443 / 443 (100.00%) | 5 / 5 (100.00%) | 589 / 594 (99.16%) | ☐ | Unique source-location union: lines 436/436, functions 5/5, regions 589/594 |
| [`vm/json.rs`](../../../backend/bluejs/src/vm/json.rs) | 771 / 786 (98.09%) | 59 / 60 (98.33%) | 1,509 / 1,590 (94.91%) | ☐ | Unique source-location union: lines 743/757, functions 59/60, regions 1,509/1,590 |
| [`vm/lifecycle.rs`](../../../backend/bluejs/src/vm/lifecycle.rs) | 204 / 204 (100.00%) | 13 / 13 (100.00%) | 220 / 220 (100.00%) | ☑ | Unique source-location union: lines 204/204, functions 13/13, regions 220/220 |
| [`vm/modules.rs`](../../../backend/bluejs/src/vm/modules.rs) | 1,657 / 1,787 (92.73%) | 77 / 85 (90.59%) | 2,570 / 2,850 (90.18%) | ☐ | Unique source-location union: lines 1,618/1,726, functions 77/85, regions 2,584/2,850 |
| [`vm/modules/deferred.rs`](../../../backend/bluejs/src/vm/modules/deferred.rs) | 296 / 307 (96.42%) | 27 / 28 (96.43%) | 448 / 489 (91.62%) | ☐ | Unique source-location union: lines 281/289, functions 27/28, regions 448/489 |
| [`vm/modules/namespace.rs`](../../../backend/bluejs/src/vm/modules/namespace.rs) | 197 / 224 (87.95%) | 13 / 17 (76.47%) | 324 / 365 (88.77%) | ☐ | Unique source-location union: lines 187/206, functions 13/17, regions 324/365 |
| [`vm/modules/synthetic.rs`](../../../backend/bluejs/src/vm/modules/synthetic.rs) | 117 / 117 (100.00%) | 4 / 4 (100.00%) | 149 / 149 (100.00%) | ☑ | Unique source-location union: lines 115/115, functions 4/4, regions 149/149 |
| [`vm/native_stack.rs`](../../../backend/bluejs/src/vm/native_stack.rs) | 88 / 88 (100.00%) | 16 / 16 (100.00%) | 146 / 146 (100.00%) | ☑ | Unique source-location union: lines 82/82, functions 16/16, regions 146/146 |
| [`vm/operations.rs`](../../../backend/bluejs/src/vm/operations.rs) | 601 / 637 (94.35%) | 45 / 47 (95.74%) | 1,086 / 1,168 (92.98%) | ☐ | Unique source-location union: lines 585/620, functions 45/47, regions 1,086/1,168 |
| [`vm/properties.rs`](../../../backend/bluejs/src/vm/properties.rs) | 650 / 705 (92.20%) | 63 / 70 (90.00%) | 1,149 / 1,295 (88.73%) | ☐ | Unique source-location union: lines 615/657, functions 63/70, regions 1,149/1,295 |
| [`vm/realm_reentrancy.rs`](../../../backend/bluejs/src/vm/realm_reentrancy.rs) | 27 / 27 (100.00%) | 8 / 8 (100.00%) | 34 / 34 (100.00%) | ☑ | Unique source-location union: lines 22/22, functions 8/8, regions 34/34 |
| [`vm/regexp.rs`](../../../backend/bluejs/src/vm/regexp.rs) | 1,061 / 1,097 (96.72%) | 60 / 60 (100.00%) | 1,978 / 2,088 (94.73%) | ☐ | Unique source-location union: lines 1,023/1,054, functions 60/60, regions 1,978/2,088 |
| [`vm/shadow_realm.rs`](../../../backend/bluejs/src/vm/shadow_realm.rs) | 335 / 380 (88.16%) | 25 / 28 (89.29%) | 562 / 630 (89.21%) | ☐ | Unique source-location union: lines 322/362, functions 25/28, regions 562/630 |
| [`vm/temporal.rs`](../../../backend/bluejs/src/vm/temporal.rs) | 542 / 569 (95.25%) | 11 / 11 (100.00%) | 525 / 567 (92.59%) | ☐ | Unique source-location union: lines 537/564, functions 11/11, regions 525/567 |
| [`vm/temporal/calendar.rs`](../../../backend/bluejs/src/vm/temporal/calendar.rs) | 147 / 147 (100.00%) | 14 / 14 (100.00%) | 215 / 215 (100.00%) | ☑ | Unique source-location union: lines 147/147, functions 14/14, regions 215/215 |
| [`vm/temporal/conversion.rs`](../../../backend/bluejs/src/vm/temporal/conversion.rs) | - | - | - | - | Declarations/re-exports only; no executable code |
| [`vm/temporal/conversion/calendar_fields.rs`](../../../backend/bluejs/src/vm/temporal/conversion/calendar_fields.rs) | 302 / 313 (96.49%) | 22 / 27 (81.48%) | 462 / 485 (95.26%) | ☐ | Unique source-location union: lines 286/290, functions 22/27, regions 462/485 |
| [`vm/temporal/conversion/from_value.rs`](../../../backend/bluejs/src/vm/temporal/conversion/from_value.rs) | 354 / 368 (96.20%) | 21 / 21 (100.00%) | 578 / 600 (96.33%) | ☐ | Unique source-location union: lines 337/351, functions 21/21, regions 578/600 |
| [`vm/temporal/conversion/getters.rs`](../../../backend/bluejs/src/vm/temporal/conversion/getters.rs) | 107 / 120 (89.17%) | 4 / 7 (57.14%) | 148 / 163 (90.80%) | ☐ | Unique source-location union: lines 103/111, functions 4/7, regions 148/163 |
| [`vm/temporal/conversion/numeric.rs`](../../../backend/bluejs/src/vm/temporal/conversion/numeric.rs) | 195 / 195 (100.00%) | 24 / 24 (100.00%) | 251 / 251 (100.00%) | ☑ | Unique source-location union: lines 183/183, functions 24/24, regions 251/251 |
| [`vm/temporal/conversion/options.rs`](../../../backend/bluejs/src/vm/temporal/conversion/options.rs) | 130 / 130 (100.00%) | 12 / 12 (100.00%) | 158 / 158 (100.00%) | ☑ | Unique source-location union: lines 125/125, functions 12/12, regions 158/158 |
| [`vm/temporal/conversion/zoned_conversion.rs`](../../../backend/bluejs/src/vm/temporal/conversion/zoned_conversion.rs) | 105 / 131 (80.15%) | 4 / 11 (36.36%) | 176 / 206 (85.44%) | ☐ | Unique source-location union: lines 103/118, functions 4/11, regions 176/206 |
| [`vm/temporal/dates.rs`](../../../backend/bluejs/src/vm/temporal/dates.rs) | - | - | - | - | Declarations/re-exports only; no executable code |
| [`vm/temporal/dates/arithmetic.rs`](../../../backend/bluejs/src/vm/temporal/dates/arithmetic.rs) | 206 / 206 (100.00%) | 8 / 8 (100.00%) | 281 / 281 (100.00%) | ☑ | Unique source-location union: lines 197/197, functions 8/8, regions 281/281 |
| [`vm/temporal/dates/comparison.rs`](../../../backend/bluejs/src/vm/temporal/dates/comparison.rs) | 49 / 49 (100.00%) | 3 / 3 (100.00%) | 65 / 65 (100.00%) | ☑ | Unique source-location union: lines 48/48, functions 3/3, regions 65/65 |
| [`vm/temporal/dates/construction.rs`](../../../backend/bluejs/src/vm/temporal/dates/construction.rs) | 214 / 214 (100.00%) | 10 / 10 (100.00%) | 231 / 233 (99.14%) | ☐ | Unique source-location union: lines 211/211, functions 10/10, regions 233/233 |
| [`vm/temporal/dates/conversions.rs`](../../../backend/bluejs/src/vm/temporal/dates/conversions.rs) | 66 / 66 (100.00%) | 3 / 3 (100.00%) | 108 / 108 (100.00%) | ☑ | Unique source-location union: lines 66/66, functions 3/3, regions 108/108 |
| [`vm/temporal/dates/date_time.rs`](../../../backend/bluejs/src/vm/temporal/dates/date_time.rs) | 124 / 124 (100.00%) | 8 / 8 (100.00%) | 198 / 198 (100.00%) | ☑ | Unique source-location union: lines 118/118, functions 8/8, regions 198/198 |
| [`vm/temporal/dates/formatting.rs`](../../../backend/bluejs/src/vm/temporal/dates/formatting.rs) | 166 / 166 (100.00%) | 5 / 5 (100.00%) | 216 / 216 (100.00%) | ☑ | Unique source-location union: lines 164/164, functions 5/5, regions 216/216 |
| [`vm/temporal/dates/now_and_zone.rs`](../../../backend/bluejs/src/vm/temporal/dates/now_and_zone.rs) | 192 / 192 (100.00%) | 20 / 20 (100.00%) | 262 / 266 (98.50%) | ☐ | Unique source-location union: lines 183/183, functions 20/20, regions 266/266 |
| [`vm/temporal/dates/with.rs`](../../../backend/bluejs/src/vm/temporal/dates/with.rs) | 144 / 144 (100.00%) | 6 / 6 (100.00%) | 262 / 262 (100.00%) | ☑ | Unique source-location union: lines 139/139, functions 6/6, regions 262/262 |
| [`vm/temporal/duration_conversion.rs`](../../../backend/bluejs/src/vm/temporal/duration_conversion.rs) | 48 / 48 (100.00%) | 4 / 4 (100.00%) | 73 / 73 (100.00%) | ☑ | Unique source-location union: lines 45/45, functions 4/4, regions 73/73 |
| [`vm/temporal/duration_math.rs`](../../../backend/bluejs/src/vm/temporal/duration_math.rs) | 305 / 305 (100.00%) | 29 / 29 (100.00%) | 361 / 361 (100.00%) | ☑ | Unique source-location union: lines 305/305, functions 29/29, regions 361/361 |
| [`vm/temporal/duration_operations.rs`](../../../backend/bluejs/src/vm/temporal/duration_operations.rs) | 611 / 611 (100.00%) | 27 / 27 (100.00%) | 874 / 874 (100.00%) | ☑ | Unique source-location union: lines 590/590, functions 27/27, regions 874/874 |
| [`vm/temporal/duration_relative.rs`](../../../backend/bluejs/src/vm/temporal/duration_relative.rs) | 526 / 532 (98.87%) | 39 / 44 (88.64%) | 651 / 677 (96.16%) | ☐ | Unique source-location union: lines 498/499, functions 39/44, regions 651/677 |
| [`vm/temporal/epoch.rs`](../../../backend/bluejs/src/vm/temporal/epoch.rs) | 136 / 136 (100.00%) | 13 / 13 (100.00%) | 237 / 237 (100.00%) | ☑ | Unique source-location union: lines 136/136, functions 13/13, regions 237/237 |
| [`vm/temporal/instant.rs`](../../../backend/bluejs/src/vm/temporal/instant.rs) | 438 / 462 (94.81%) | 22 / 32 (68.75%) | 617 / 667 (92.50%) | ☐ | Unique source-location union: lines 431/443, functions 22/32, regions 617/667 |
| [`vm/temporal/iso.rs`](../../../backend/bluejs/src/vm/temporal/iso.rs) | 17 / 17 (100.00%) | 3 / 3 (100.00%) | 24 / 24 (100.00%) | ☑ | Unique source-location union: lines 17/17, functions 3/3, regions 24/24 |
| [`vm/temporal/iso/annotations.rs`](../../../backend/bluejs/src/vm/temporal/iso/annotations.rs) | 206 / 206 (100.00%) | 24 / 24 (100.00%) | 341 / 341 (100.00%) | ☑ | Unique source-location union: lines 191/191, functions 24/24, regions 341/341 |
| [`vm/temporal/iso/datetime.rs`](../../../backend/bluejs/src/vm/temporal/iso/datetime.rs) | 135 / 135 (100.00%) | 14 / 14 (100.00%) | 233 / 233 (100.00%) | ☑ | Unique source-location union: lines 133/133, functions 14/14, regions 233/233 |
| [`vm/temporal/iso/duration.rs`](../../../backend/bluejs/src/vm/temporal/iso/duration.rs) | 117 / 117 (100.00%) | 5 / 5 (100.00%) | 183 / 183 (100.00%) | ☑ | Unique source-location union: lines 114/114, functions 5/5, regions 183/183 |
| [`vm/temporal/iso/offset.rs`](../../../backend/bluejs/src/vm/temporal/iso/offset.rs) | 135 / 135 (100.00%) | 8 / 8 (100.00%) | 256 / 256 (100.00%) | ☑ | Unique source-location union: lines 134/134, functions 8/8, regions 256/256 |
| [`vm/temporal/iso/scan.rs`](../../../backend/bluejs/src/vm/temporal/iso/scan.rs) | 244 / 244 (100.00%) | 32 / 32 (100.00%) | 475 / 475 (100.00%) | ☑ | Unique source-location union: lines 232/232, functions 32/32, regions 475/475 |
| [`vm/temporal/iso/tests.rs`](../../../backend/bluejs/src/vm/temporal/iso/tests.rs) | - | - | - | - | Test source; not a coverage target |
| [`vm/temporal/plain_date.rs`](../../../backend/bluejs/src/vm/temporal/plain_date.rs) | - | - | - | - | Declarations/re-exports only; no executable code |
| [`vm/temporal/plain_date/calendar_add.rs`](../../../backend/bluejs/src/vm/temporal/plain_date/calendar_add.rs) | 141 / 141 (100.00%) | 3 / 3 (100.00%) | 230 / 230 (100.00%) | ☑ | Unique source-location union: lines 141/141, functions 3/3, regions 230/230 |
| [`vm/temporal/plain_date/calendar_difference.rs`](../../../backend/bluejs/src/vm/temporal/plain_date/calendar_difference.rs) | 186 / 186 (100.00%) | 7 / 7 (100.00%) | 341 / 341 (100.00%) | ☑ | Unique source-location union: lines 184/184, functions 7/7, regions 341/341 |
| [`vm/temporal/plain_date/format.rs`](../../../backend/bluejs/src/vm/temporal/plain_date/format.rs) | 30 / 30 (100.00%) | 4 / 4 (100.00%) | 56 / 56 (100.00%) | ☑ | Unique source-location union: lines 30/30, functions 4/4, regions 56/56 |
| [`vm/temporal/plain_date/iso_date.rs`](../../../backend/bluejs/src/vm/temporal/plain_date/iso_date.rs) | 116 / 116 (100.00%) | 17 / 17 (100.00%) | 207 / 207 (100.00%) | ☑ | Unique source-location union: lines 115/115, functions 17/17, regions 207/207 |
| [`vm/temporal/plain_date/month_structure.rs`](../../../backend/bluejs/src/vm/temporal/plain_date/month_structure.rs) | 131 / 131 (100.00%) | 12 / 12 (100.00%) | 204 / 204 (100.00%) | ☑ | Unique source-location union: lines 131/131, functions 12/12, regions 204/204 |
| [`vm/temporal/plain_date/round_duration.rs`](../../../backend/bluejs/src/vm/temporal/plain_date/round_duration.rs) | 111 / 111 (100.00%) | 6 / 6 (100.00%) | 157 / 157 (100.00%) | ☑ | Unique source-location union: lines 107/107, functions 6/6, regions 157/157 |
| [`vm/temporal/plain_date/tests.rs`](../../../backend/bluejs/src/vm/temporal/plain_date/tests.rs) | - | - | - | - | Test source; not a coverage target |
| [`vm/temporal/plain_date_time_difference.rs`](../../../backend/bluejs/src/vm/temporal/plain_date_time_difference.rs) | 784 / 785 (99.87%) | 54 / 54 (100.00%) | 1,088 / 1,105 (98.46%) | ☐ | Unique source-location union: lines 775/776, functions 54/54, regions 1,089/1,105 |
| [`vm/temporal/plain_month_day.rs`](../../../backend/bluejs/src/vm/temporal/plain_month_day.rs) | 282 / 282 (100.00%) | 28 / 28 (100.00%) | 386 / 386 (100.00%) | ☑ | Unique source-location union: lines 279/279, functions 28/28, regions 386/386 |
| [`vm/temporal/plain_time.rs`](../../../backend/bluejs/src/vm/temporal/plain_time.rs) | 625 / 649 (96.30%) | 38 / 44 (86.36%) | 883 / 922 (95.77%) | ☐ | Unique source-location union: lines 611/626, functions 38/44, regions 883/922 |
| [`vm/temporal/plain_year_month.rs`](../../../backend/bluejs/src/vm/temporal/plain_year_month.rs) | 145 / 145 (100.00%) | 13 / 13 (100.00%) | 193 / 193 (100.00%) | ☑ | Unique source-location union: lines 143/143, functions 13/13, regions 193/193 |
| [`vm/temporal/receiver.rs`](../../../backend/bluejs/src/vm/temporal/receiver.rs) | 42 / 42 (100.00%) | 3 / 3 (100.00%) | 43 / 43 (100.00%) | ☑ | Unique source-location union: lines 41/41, functions 3/3, regions 43/43 |
| [`vm/temporal/rounding.rs`](../../../backend/bluejs/src/vm/temporal/rounding.rs) | 399 / 399 (100.00%) | 33 / 33 (100.00%) | 652 / 652 (100.00%) | ☑ | Unique source-location union: lines 399/399, functions 33/33, regions 652/652 |
| [`vm/temporal/time_zone.rs`](../../../backend/bluejs/src/vm/temporal/time_zone.rs) | 519 / 519 (100.00%) | 47 / 47 (100.00%) | 1,030 / 1,030 (100.00%) | ☑ | Unique source-location union: lines 510/510, functions 47/47, regions 1,030/1,030 |
| [`vm/temporal/time_zone_id.rs`](../../../backend/bluejs/src/vm/temporal/time_zone_id.rs) | 225 / 225 (100.00%) | 29 / 29 (100.00%) | 495 / 495 (100.00%) | ☑ | Unique source-location union: lines 215/215, functions 29/29, regions 495/495 |
| [`vm/temporal/year_month.rs`](../../../backend/bluejs/src/vm/temporal/year_month.rs) | 971 / 1,017 (95.48%) | 68 / 82 (82.93%) | 1,538 / 1,626 (94.59%) | ☐ | Unique source-location union: lines 921/949, functions 68/82, regions 1,538/1,626 |
| [`vm/temporal/zoned.rs`](../../../backend/bluejs/src/vm/temporal/zoned.rs) | - | - | - | - | Declarations/re-exports only; no executable code |
| [`vm/temporal/zoned/arithmetic.rs`](../../../backend/bluejs/src/vm/temporal/zoned/arithmetic.rs) | 177 / 177 (100.00%) | 8 / 8 (100.00%) | 280 / 280 (100.00%) | ☑ | Unique source-location union: lines 168/168, functions 8/8, regions 280/280 |
| [`vm/temporal/zoned/conversions.rs`](../../../backend/bluejs/src/vm/temporal/zoned/conversions.rs) | 121 / 121 (100.00%) | 8 / 8 (100.00%) | 160 / 160 (100.00%) | ☑ | Unique source-location union: lines 120/120, functions 8/8, regions 160/160 |
| [`vm/temporal/zoned/formatting.rs`](../../../backend/bluejs/src/vm/temporal/zoned/formatting.rs) | 162 / 162 (100.00%) | 7 / 7 (100.00%) | 254 / 254 (100.00%) | ☑ | Unique source-location union: lines 159/159, functions 7/7, regions 254/254 |
| [`vm/temporal/zoned/from_value.rs`](../../../backend/bluejs/src/vm/temporal/zoned/from_value.rs) | 184 / 196 (93.88%) | 13 / 15 (86.67%) | 251 / 265 (94.72%) | ☐ | Unique source-location union: lines 173/181, functions 13/15, regions 251/265 |
| [`vm/temporal/zoned/resolution.rs`](../../../backend/bluejs/src/vm/temporal/zoned/resolution.rs) | 112 / 112 (100.00%) | 8 / 8 (100.00%) | 154 / 154 (100.00%) | ☑ | Unique source-location union: lines 112/112, functions 8/8, regions 154/154 |
| [`vm/temporal/zoned/since_until.rs`](../../../backend/bluejs/src/vm/temporal/zoned/since_until.rs) | 146 / 146 (100.00%) | 8 / 8 (100.00%) | 212 / 212 (100.00%) | ☑ | Unique source-location union: lines 142/142, functions 8/8, regions 212/212 |
| [`vm/temporal/zoned/with.rs`](../../../backend/bluejs/src/vm/temporal/zoned/with.rs) | 215 / 215 (100.00%) | 9 / 9 (100.00%) | 371 / 371 (100.00%) | ☑ | Unique source-location union: lines 209/209, functions 9/9, regions 371/371 |
| [`vm/temporal/zoned_date_time.rs`](../../../backend/bluejs/src/vm/temporal/zoned_date_time.rs) | 245 / 245 (100.00%) | 17 / 17 (100.00%) | 405 / 405 (100.00%) | ☑ | Unique source-location union: lines 243/243, functions 17/17, regions 405/405 |
| [`vm/temporal/zoned_difference.rs`](../../../backend/bluejs/src/vm/temporal/zoned_difference.rs) | 767 / 771 (99.48%) | 43 / 43 (100.00%) | 1,056 / 1,090 (96.88%) | ☐ | Unique source-location union: lines 763/767, functions 43/43, regions 1,056/1,090 |
| [`vm/test262.rs`](../../../backend/bluejs/src/vm/test262.rs) | 65 / 65 (100.00%) | 7 / 7 (100.00%) | 107 / 107 (100.00%) | ☑ | Unique source-location union: lines 62/62, functions 7/7, regions 107/107 |
| [`vm/test262/assertions.rs`](../../../backend/bluejs/src/vm/test262/assertions.rs) | 288 / 299 (96.32%) | 16 / 17 (94.12%) | 750 / 838 (89.50%) | ☐ | Unique source-location union: lines 287/297, functions 16/17, regions 750/838 |
| [`vm/test262/cases.rs`](../../../backend/bluejs/src/vm/test262/cases.rs) | 647 / 686 (94.31%) | 39 / 45 (86.67%) | 1,130 / 1,263 (89.47%) | ☐ | Unique source-location union: lines 620/648, functions 39/45, regions 1,130/1,263 |
| [`vm/test262/foreign.rs`](../../../backend/bluejs/src/vm/test262/foreign.rs) | 1,471 / 1,581 (93.04%) | 127 / 143 (88.81%) | 2,320 / 2,640 (87.88%) | ☐ | Unique source-location union: lines 1,390/1,474, functions 127/143, regions 2,320/2,640 |
| [`vm/test262/harness.rs`](../../../backend/bluejs/src/vm/test262/harness.rs) | 242 / 257 (94.16%) | 12 / 12 (100.00%) | 385 / 418 (92.11%) | ☐ | Unique source-location union: lines 234/248, functions 12/12, regions 385/418 |
| [`vm/test262/reverse.rs`](../../../backend/bluejs/src/vm/test262/reverse.rs) | 260 / 270 (96.30%) | 24 / 26 (92.31%) | 427 / 464 (92.03%) | ☐ | Unique source-location union: lines 249/256, functions 24/26, regions 427/464 |
| [`vm/test262_agents.rs`](../../../backend/bluejs/src/vm/test262_agents.rs) | 461 / 490 (94.08%) | 45 / 52 (86.54%) | 659 / 719 (91.66%) | ☐ | Unique source-location union: lines 451/469, functions 45/52, regions 661/719 |
| [`vm/tests.rs`](../../../backend/bluejs/src/vm/tests.rs) | - | - | - | - | Test source; not a coverage target |
| **Total (160 instrumented files)** | **75,021 / 77,650 (96.61%)** | **5,351 / 5,578 (95.93%)** | **125,566 / 132,229 (94.96%)** | ☐ |  |

Raw LLVM lines, functions, and regions are all complete in **80 of 160** instrumented files; **80** remain incomplete.

## Historical differences from the other platforms (2026-09-21)

- **Ubuntu**: 2 of 102,926 modes differ (1 file): `staging/sm/Math/acosh-approx.js` (this platform: pass; Ubuntu: fail).

## Reproduce the current Test262 inventory

```sh
python3 -m venv /tmp/bluejs-conformance-venv
/tmp/bluejs-conformance-venv/bin/pip install -r backend/bluejs/test262/requirements.txt
cargo build -p blueice-bluejs --bins --offline
/tmp/bluejs-conformance-venv/bin/python -m unittest discover -s backend/bluejs/test262 -v
/tmp/bluejs-conformance-venv/bin/python backend/bluejs/test262/run.py --corpus development/browser_core/reference/test262 --jobs 8 --output target/test262-macos-20260929-batch1 --progress-interval 60
/tmp/bluejs-conformance-venv/bin/python backend/bluejs/test262/analyze.py --run target/test262-macos-20260929-batch1 --corpus development/browser_core/reference/test262 --output target/test262-macos-20260929-batch1-analysis
```

The runner validates the pinned corpus marker, manifest hash, and every corpus file before execution. It returns 1 for this run because the 1 `stale_corpus` and 4 `excluded` modes remain non-pass statuses; inspect `summary.json` and use `analyze.py` to reconcile all records. Keep the host otherwise idle during this inventory because ordinary cases have a two-second wall deadline.
