# Ubuntu Test262 Report

## Current complete inventory (2026-09-25)

The pinned, unfiltered Test262 snapshot was run on Ubuntu 24.04.3 LTS under WSL2 (kernel `6.6.87.2-microsoft-standard-WSL2`, x86_64, 12 logical CPUs) with Rust/Cargo 1.95.0 and Python 3.13.13 (PyYAML 6.0.3), from commit `68150757` plus uncommitted coverage-test changes. The verified corpus revision is `72faf8ec1445c55149615e8b35187830783aba1a` and includes main, proposals, and staging. After `cargo build -p blueice-bluejs --bins --offline`, the command `python3 backend/bluejs/test262/run.py --corpus development/browser_core/reference/test262 --jobs 4 --output target/test262-linux-20260925-current --progress-interval 60` completed in 972.944 seconds. The runner verified the pinned corpus marker, manifest, and every file before execution. `analyze.py` reconciled all 53,582 files and 102,926 modes against `results.jsonl` and `summary.json`.

Adapter SHA-256: `b43a180b6fd9e8816b05bc4397febd1c9539bfbdb5ae1ec7d7db453fb047464e`. RegExp worker SHA-256: `151faa2fdf8c5686b7e87044f02cf0b8207fdb49e1f79d36c8fa4301b222e0ae`. Runner SHA-256: `d56c75f03b0422f20fea1dcd8a10be3ea81905f4a79d7fead0abb1ca6986d0ba`. The [checked-in Linux summary](test262-linux-summary.json) contains the complete feature and top-level group counts; the full per-mode evidence is in `target/test262-linux-20260925-current/results.jsonl`.

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

## Historical complete inventory (2026-09-22)

The earlier Ubuntu 24.04.4 WSL2 run at `9decbb3` recorded 102,859 pass, 67 fail, and 0 timeout out of 102,926 modes (99.935%). Its triage and three-platform comparison belong to that earlier source revision. The current 2026-09-25 inventory above replaces it as the Linux Test262 status.

## Historical verification on this platform (2026-09-22 source revision)

These checks were performed for the earlier `9decbb3` source revision. They are retained as historical evidence, not as measurements of the current Test262 run.

| Check | Command | Result | Gate |
| --- | --- | --- | --- |
| Workspace tests | `cargo test --workspace --no-fail-fast` | **3,640 passed, 0 failed** (141 s) | all pass |
| `blueice-bluejs` tests | `cargo test -p blueice-bluejs --no-fail-fast` | **2,432 passed, 0 failed** | all pass |
| Line coverage, workspace (CI `Coverage` job) | `cargo llvm-cov --workspace --ignore-filename-regex 'extension/src/main\.rs$\|frontend-reference/src/main\.rs$\|mcp-server/src/main\.rs$\|mcp-server/src/server\.rs$' --fail-under-lines 90 --summary-only` | 90.58% lines (155,626 / 171,819); functions 93.74%; regions 92.88% | ≥ 90% lines: met |
| Line coverage, `blueice-bluejs` alone | `cargo llvm-cov clean --workspace && cargo llvm-cov -p blueice-bluejs --fail-under-lines 88 --summary-only` | 89.53% lines (108,834 / 121,558); functions 92.84%; regions 92.43% | ≥ 88% lines: met |
| Line coverage, `blueice-ecma402` alone (from the workspace run's per-file breakdown) | — | 95.57% lines (9,732 / 10,183); functions 96.67%; regions 93.79% | informational |
| Node differential oracle | `cargo test -p blueice-bluejs --test node_differential -- --ignored` (Node v24.15.0) | **4 / 4 tests pass** (26.06 s): the main corpus plus the NumberFormat/NumberFormat-range/locale-data matrices agree with Node | all pass |
| TypeScript compatibility oracle | `npm exec --yes --package typescript@5.9.3 -- env BLUEICE_BLUETSC_ORACLE=tsc cargo test -p blueice-bluets --test typescript_oracle -- --ignored` | **1 / 1 test passes** (59.09 s) | all pass |

Coverage uses `cargo-llvm-cov` 0.9.0; Node here is v24.15.0 (not the v24.21.0 the 2026-09-21 measurement recorded — this is whatever Node the WSL2 host has on `PATH`, not independently pinned). The BlueJS crate itself roughly doubled in line count this pass (Promise/classes/decorators/tail-calls/source-text/cross-realm/surrogate-encoding work landed together), which is why its coverage percentage moved down from 91.38% to 89.53% even though the absolute covered-line count grew by about 50,000 lines; both the 90%/88% floors are still comfortably met. The `blueice-ecma402` figure above is computed from the workspace run's own per-file breakdown (the same numbers a standalone `-p blueice-ecma402` run would report, since that crate isn't excluded from the workspace measurement), not a separate invocation.

Line coverage is a different measure from a Test262 pass rate and the two must not be quoted interchangeably: it is the fraction of the Rust source lines that execute during the crates' own test suites. The Node differential and TypeScript oracle tests are `#[ignore]`d by default and run only with the explicit commands above.

The compile break the source-text feature caused in `blueice-bluets-bluejs` (a `bluejs::Function` literal missing the new `source_text` field) was caught by this platform's workspace-level build, not by `cargo build -p blueice-bluejs` alone — the sibling crate isn't in that package's own dependency closure. This is the reason a per-crate build is never treated as sufficient before a push; see the coverage gate command above for the same principle applied to tests.

## Later BlueJS per-file coverage (2026-09-26)

This is a separate BlueJS coverage measurement at commit `d0027d32` with uncommitted changes on Linux 6.6.87.2-microsoft-standard-WSL2 (`x86_64`), rustc 1.95.0 (59807616e 2026-04-14) and `cargo-llvm-cov 0.9.1`. It measures the Rust test suite independently of the Test262 inventory and historical verification above. `python3 backend/bluejs/coverage_file.py --update-linux-report` cleaned prior LLVM artifacts, ran the complete default BlueJS Rust test suite, and exported fresh per-file JSON and source-line text. The opt-in Node oracle and external full Test262 runner were not included. Workspace coverage was not remeasured at this revision.

Each measured cell shows LLVM JSON's raw covered / instrumented counts and the coverage rate. All 177 Rust files under `backend/bluejs/src/` are listed: 160 have LLVM counters; 17 use `-` with an individual reason in `Note`. A `0%` result requires a positive instrumented denominator and zero covered units. `☑` means **lines, functions and regions all reach 100%**; `☐` means at least one is below 100%. The total aggregates only instrumented files. LLVM can count separate compiled instances of the same source in different test binaries. `Note` shows the union across those binaries when its counts differ: each source location is counted once and considered covered if any binary executes it. The raw LLVM counts in the main columns determine completion. Region coverage is separate from branch coverage. To rerun any one file independently, use `python3 backend/bluejs/coverage_file.py ast.rs` (replace `ast.rs` with its source path). Each invocation reruns the entire test suite, since tests outside a file can still exercise it.

| Source file (relative to `backend/bluejs/src/`) | Lines | Functions | Regions | Complete | Note |
| --- | ---: | ---: | ---: | :---: | --- |
| [`ast.rs`](../../../backend/bluejs/src/ast.rs) | 903 / 903 (100.00%) | 90 / 90 (100.00%) | 1,184 / 1,184 (100.00%) | ☑ | Unique source-location union: lines 854/854, functions 90/90, regions 1,184/1,184 |
| [`bin/bluejs-regexp-worker.rs`](../../../backend/bluejs/src/bin/bluejs-regexp-worker.rs) | 3 / 3 (100.00%) | 1 / 1 (100.00%) | 3 / 3 (100.00%) | ☑ | Unique source-location union: lines 3/3, functions 1/1, regions 3/3 |
| [`bin/bluejs-test262.rs`](../../../backend/bluejs/src/bin/bluejs-test262.rs) | 631 / 631 (100.00%) | 56 / 56 (100.00%) | 1,211 / 1,211 (100.00%) | ☑ | Unique source-location union: lines 613/613, functions 56/56, regions 1,211/1,211 |
| [`bytecode.rs`](../../../backend/bluejs/src/bytecode.rs) | 95 / 95 (100.00%) | 13 / 13 (100.00%) | 98 / 98 (100.00%) | ☑ | Unique source-location union: lines 94/94, functions 13/13, regions 98/98 |
| [`compiler.rs`](../../../backend/bluejs/src/compiler.rs) | 1,514 / 1,514 (100.00%) | 146 / 146 (100.00%) | 2,131 / 2,131 (100.00%) | ☑ | Unique source-location union: lines 1,443/1,443, functions 146/146, regions 2,131/2,131 |
| [`compiler/expressions.rs`](../../../backend/bluejs/src/compiler/expressions.rs) | 1,532 / 1,541 (99.42%) | 42 / 42 (100.00%) | 3,417 / 3,427 (99.71%) | ☐ | Unique source-location union: lines 1,530/1,530, functions 42/42, regions 3,427/3,427 |
| [`compiler/expressions/tests.rs`](../../../backend/bluejs/src/compiler/expressions/tests.rs) | - | - | - | - | Test source; not a coverage target |
| [`compiler/functions.rs`](../../../backend/bluejs/src/compiler/functions.rs) | 1,006 / 1,010 (99.60%) | 54 / 54 (100.00%) | 1,847 / 1,853 (99.68%) | ☐ | Unique source-location union: lines 980/980, functions 54/54, regions 1,853/1,853 |
| [`compiler/functions/tests.rs`](../../../backend/bluejs/src/compiler/functions/tests.rs) | - | - | - | - | Test source; not a coverage target |
| [`compiler/private_validation.rs`](../../../backend/bluejs/src/compiler/private_validation.rs) | 334 / 340 (98.24%) | 32 / 32 (100.00%) | 672 / 701 (95.86%) | ☐ | Unique source-location union: lines 321/321, functions 32/32, regions 701/701 |
| [`compiler/private_validation/tests.rs`](../../../backend/bluejs/src/compiler/private_validation/tests.rs) | - | - | - | - | Test source; not a coverage target |
| [`compiler/statements.rs`](../../../backend/bluejs/src/compiler/statements.rs) | 1,326 / 1,357 (97.72%) | 70 / 70 (100.00%) | 2,568 / 2,643 (97.16%) | ☐ | Unique source-location union: lines 1,330/1,330, functions 70/70, regions 2,643/2,643 |
| [`compiler/statements/tests.rs`](../../../backend/bluejs/src/compiler/statements/tests.rs) | - | - | - | - | Test source; not a coverage target |
| [`heap.rs`](../../../backend/bluejs/src/heap.rs) | 727 / 744 (97.72%) | 78 / 78 (100.00%) | 1,168 / 1,190 (98.15%) | ☐ | Unique source-location union: lines 723/723, functions 78/78, regions 1,190/1,190 |
| [`heap/binary_data.rs`](../../../backend/bluejs/src/heap/binary_data.rs) | 836 / 850 (98.35%) | 77 / 77 (100.00%) | 1,270 / 1,302 (97.54%) | ☐ | Unique source-location union: lines 832/832, functions 77/77, regions 1,302/1,302 |
| [`heap/collection_iteration.rs`](../../../backend/bluejs/src/heap/collection_iteration.rs) | 93 / 93 (100.00%) | 5 / 5 (100.00%) | 118 / 120 (98.33%) | ☐ | Unique source-location union: lines 93/93, functions 5/5, regions 120/120 |
| [`heap/core.rs`](../../../backend/bluejs/src/heap/core.rs) | 792 / 807 (98.14%) | 72 / 72 (100.00%) | 1,161 / 1,206 (96.27%) | ☐ | Unique source-location union: lines 791/791, functions 72/72, regions 1,206/1,206 |
| [`heap/debugger.rs`](../../../backend/bluejs/src/heap/debugger.rs) | 163 / 163 (100.00%) | 10 / 10 (100.00%) | 239 / 239 (100.00%) | ☑ | Unique source-location union: lines 161/161, functions 10/10, regions 239/239 |
| [`heap/exotic.rs`](../../../backend/bluejs/src/heap/exotic.rs) | 902 / 925 (97.51%) | 91 / 91 (100.00%) | 987 / 1,079 (91.47%) | ☐ | Unique source-location union: lines 897/920, functions 91/91, regions 988/1,079 |
| [`heap/lifecycle.rs`](../../../backend/bluejs/src/heap/lifecycle.rs) | 281 / 298 (94.30%) | 30 / 31 (96.77%) | 474 / 517 (91.68%) | ☐ | Unique source-location union: lines 272/283, functions 30/31, regions 480/517 |
| [`heap/object_storage.rs`](../../../backend/bluejs/src/heap/object_storage.rs) | 713 / 744 (95.83%) | 56 / 57 (98.25%) | 1,198 / 1,327 (90.28%) | ☐ | Unique source-location union: lines 684/714, functions 56/57, regions 1,199/1,327 |
| [`heap/tests.rs`](../../../backend/bluejs/src/heap/tests.rs) | - | - | - | - | Test source; not a coverage target |
| [`intl.rs`](../../../backend/bluejs/src/intl.rs) | 100 / 101 (99.01%) | 17 / 17 (100.00%) | 154 / 159 (96.86%) | ☐ | Unique source-location union: lines 94/95, functions 17/17, regions 154/159 |
| [`lib.rs`](../../../backend/bluejs/src/lib.rs) | - | - | - | - | Declarations/re-exports only; no executable code |
| [`native.rs`](../../../backend/bluejs/src/native.rs) | 515 / 526 (97.91%) | 40 / 40 (100.00%) | 913 / 967 (94.42%) | ☐ | Unique source-location union: lines 496/507, functions 40/40, regions 913/967 |
| [`page_runtime.rs`](../../../backend/bluejs/src/page_runtime.rs) | 1,670 / 1,803 (92.62%) | 99 / 108 (91.67%) | 2,376 / 2,596 (91.53%) | ☐ | Unique source-location union: lines 1,645/1,777, functions 99/108, regions 2,377/2,596 |
| [`parser.rs`](../../../backend/bluejs/src/parser.rs) | 536 / 548 (97.81%) | 71 / 73 (97.26%) | 733 / 758 (96.70%) | ☐ | Unique source-location union: lines 528/537, functions 71/73, regions 735/758 |
| [`parser/expressions.rs`](../../../backend/bluejs/src/parser/expressions.rs) | 927 / 952 (97.37%) | 41 / 41 (100.00%) | 1,617 / 1,700 (95.12%) | ☐ | Unique source-location union: lines 926/948, functions 41/41, regions 1,619/1,700 |
| [`parser/functions.rs`](../../../backend/bluejs/src/parser/functions.rs) | 644 / 659 (97.72%) | 39 / 40 (97.50%) | 1,009 / 1,046 (96.46%) | ☐ | Unique source-location union: lines 634/647, functions 39/40, regions 1,009/1,046 |
| [`parser/module.rs`](../../../backend/bluejs/src/parser/module.rs) | 139 / 143 (97.20%) | 7 / 8 (87.50%) | 235 / 243 (96.71%) | ☐ | Unique source-location union: lines 135/138, functions 7/8, regions 235/243 |
| [`parser/module_items.rs`](../../../backend/bluejs/src/parser/module_items.rs) | 318 / 336 (94.64%) | 12 / 12 (100.00%) | 564 / 642 (87.85%) | ☐ | Unique source-location union: lines 323/331, functions 12/12, regions 587/642 |
| [`parser/patterns.rs`](../../../backend/bluejs/src/parser/patterns.rs) | 174 / 178 (97.75%) | 10 / 10 (100.00%) | 273 / 292 (93.49%) | ☐ | Unique source-location union: lines 172/176, functions 10/10, regions 273/292 |
| [`parser/statements.rs`](../../../backend/bluejs/src/parser/statements.rs) | 558 / 616 (90.58%) | 21 / 21 (100.00%) | 1,041 / 1,177 (88.45%) | ☐ | Unique source-location union: lines 569/614, functions 21/21, regions 1,055/1,177 |
| [`parser/tests.rs`](../../../backend/bluejs/src/parser/tests.rs) | - | - | - | - | Test source; not a coverage target |
| [`primitive.rs`](../../../backend/bluejs/src/primitive.rs) | 277 / 281 (98.58%) | 26 / 27 (96.30%) | 516 / 524 (98.47%) | ☐ | Unique source-location union: lines 267/269, functions 26/27, regions 516/524 |
| [`program_abi.rs`](../../../backend/bluejs/src/program_abi.rs) | 12 / 12 (100.00%) | 2 / 2 (100.00%) | 22 / 22 (100.00%) | ☑ | Unique source-location union: lines 12/12, functions 2/2, regions 22/22 |
| [`program_debug.rs`](../../../backend/bluejs/src/program_debug.rs) | 506 / 536 (94.40%) | 53 / 56 (94.64%) | 790 / 862 (91.65%) | ☐ | Unique source-location union: lines 498/528, functions 53/56, regions 790/862 |
| [`program_debug/ast_ids.rs`](../../../backend/bluejs/src/program_debug/ast_ids.rs) | 160 / 344 (46.51%) | 13 / 18 (72.22%) | 250 / 589 (42.44%) | ☐ | Unique source-location union: lines 160/344, functions 13/18, regions 250/589 |
| [`property.rs`](../../../backend/bluejs/src/property.rs) | 86 / 86 (100.00%) | 21 / 21 (100.00%) | 111 / 111 (100.00%) | ☑ | Unique source-location union: lines 84/84, functions 21/21, regions 111/111 |
| [`regex_backrefs.rs`](../../../backend/bluejs/src/regex_backrefs.rs) | 98 / 98 (100.00%) | 12 / 12 (100.00%) | 186 / 186 (100.00%) | ☑ | Unique source-location union: lines 94/94, functions 12/12, regions 186/186 |
| [`regex_canonicalize.rs`](../../../backend/bluejs/src/regex_canonicalize.rs) | 610 / 610 (100.00%) | 64 / 64 (100.00%) | 1,295 / 1,304 (99.31%) | ☐ | Unique source-location union: lines 586/586, functions 64/64, regions 1,295/1,304 |
| [`regex_escapes.rs`](../../../backend/bluejs/src/regex_escapes.rs) | 215 / 215 (100.00%) | 22 / 22 (100.00%) | 458 / 459 (99.78%) | ☐ | Unique source-location union: lines 205/205, functions 22/22, regions 458/459 |
| [`regex_group_names.rs`](../../../backend/bluejs/src/regex_group_names.rs) | 303 / 303 (100.00%) | 38 / 38 (100.00%) | 585 / 586 (99.83%) | ☐ | Unique source-location union: lines 282/282, functions 38/38, regions 585/586 |
| [`regex_worker.rs`](../../../backend/bluejs/src/regex_worker.rs) | 412 / 412 (100.00%) | 50 / 50 (100.00%) | 587 / 599 (98.00%) | ☐ | Unique source-location union: lines 383/383, functions 50/50, regions 588/599 |
| [`regexp.rs`](../../../backend/bluejs/src/regexp.rs) | 292 / 301 (97.01%) | 27 / 27 (100.00%) | 452 / 463 (97.62%) | ☐ | Unique source-location union: lines 280/289, functions 27/27, regions 452/463 |
| [`source_encoding.rs`](../../../backend/bluejs/src/source_encoding.rs) | 153 / 153 (100.00%) | 20 / 20 (100.00%) | 293 / 294 (99.66%) | ☐ | Unique source-location union: lines 151/151, functions 20/20, regions 293/294 |
| [`string.rs`](../../../backend/bluejs/src/string.rs) | 96 / 97 (98.97%) | 20 / 20 (100.00%) | 153 / 155 (98.71%) | ☐ | Unique source-location union: lines 94/95, functions 20/20, regions 153/155 |
| [`token.rs`](../../../backend/bluejs/src/token.rs) | 1,249 / 1,257 (99.36%) | 104 / 104 (100.00%) | 1,972 / 1,989 (99.15%) | ☐ | Unique source-location union: lines 1,220/1,226, functions 104/104, regions 1,976/1,989 |
| [`value.rs`](../../../backend/bluejs/src/value.rs) | 12 / 12 (100.00%) | 2 / 2 (100.00%) | 20 / 20 (100.00%) | ☑ | Unique source-location union: lines 12/12, functions 2/2, regions 20/20 |
| [`vm.rs`](../../../backend/bluejs/src/vm.rs) | 599 / 599 (100.00%) | 57 / 57 (100.00%) | 881 / 907 (97.13%) | ☐ | Unique source-location union: lines 568/568, functions 57/57, regions 881/907 |
| [`vm/builtins.rs`](../../../backend/bluejs/src/vm/builtins.rs) | 269 / 316 (85.13%) | 14 / 18 (77.78%) | 499 / 573 (87.09%) | ☐ | Unique source-location union: lines 265/307, functions 14/18, regions 499/573 |
| [`vm/builtins/arguments.rs`](../../../backend/bluejs/src/vm/builtins/arguments.rs) | 622 / 696 (89.37%) | 62 / 63 (98.41%) | 1,058 / 1,232 (85.88%) | ☐ | Unique source-location union: lines 581/649, functions 62/63, regions 1,059/1,232 |
| [`vm/builtins/array_change_by_copy.rs`](../../../backend/bluejs/src/vm/builtins/array_change_by_copy.rs) | 194 / 197 (98.48%) | 14 / 14 (100.00%) | 422 / 456 (92.54%) | ☐ | Unique source-location union: lines 187/190, functions 14/14, regions 422/456 |
| [`vm/builtins/array_from_async.rs`](../../../backend/bluejs/src/vm/builtins/array_from_async.rs) | 313 / 338 (92.60%) | 25 / 26 (96.15%) | 753 / 891 (84.51%) | ☐ | Unique source-location union: lines 306/330, functions 25/26, regions 753/891 |
| [`vm/builtins/array_scan.rs`](../../../backend/bluejs/src/vm/builtins/array_scan.rs) | 117 / 117 (100.00%) | 12 / 12 (100.00%) | 163 / 174 (93.68%) | ☐ | Unique source-location union: lines 114/114, functions 12/12, regions 163/174 |
| [`vm/builtins/arrays.rs`](../../../backend/bluejs/src/vm/builtins/arrays.rs) | 1,239 / 1,305 (94.94%) | 78 / 81 (96.30%) | 2,549 / 2,868 (88.88%) | ☐ | Unique source-location union: lines 1,200/1,263, functions 78/81, regions 2,549/2,868 |
| [`vm/builtins/binary_data.rs`](../../../backend/bluejs/src/vm/builtins/binary_data.rs) | 1,261 / 1,511 (83.45%) | 101 / 123 (82.11%) | 2,299 / 2,728 (84.27%) | ☐ | Unique source-location union: lines 1,208/1,415, functions 101/123, regions 2,299/2,728 |
| [`vm/builtins/collection_iteration.rs`](../../../backend/bluejs/src/vm/builtins/collection_iteration.rs) | 110 / 116 (94.83%) | 8 / 8 (100.00%) | 182 / 200 (91.00%) | ☐ | Unique source-location union: lines 106/111, functions 8/8, regions 182/200 |
| [`vm/builtins/collections.rs`](../../../backend/bluejs/src/vm/builtins/collections.rs) | 312 / 355 (87.89%) | 19 / 23 (82.61%) | 678 / 752 (90.16%) | ☐ | Unique source-location union: lines 299/336, functions 19/23, regions 678/752 |
| [`vm/builtins/decorators.rs`](../../../backend/bluejs/src/vm/builtins/decorators.rs) | 498 / 523 (95.22%) | 41 / 41 (100.00%) | 1,011 / 1,123 (90.03%) | ☐ | Unique source-location union: lines 484/508, functions 41/41, regions 1,011/1,123 |
| [`vm/builtins/dynamic.rs`](../../../backend/bluejs/src/vm/builtins/dynamic.rs) | 615 / 645 (95.35%) | 36 / 37 (97.30%) | 1,139 / 1,260 (90.40%) | ☐ | Unique source-location union: lines 589/617, functions 36/37, regions 1,139/1,260 |
| [`vm/builtins/execution.rs`](../../../backend/bluejs/src/vm/builtins/execution.rs) | - | - | - | - | Declarations/re-exports only; no executable code |
| [`vm/builtins/execution/closures.rs`](../../../backend/bluejs/src/vm/builtins/execution/closures.rs) | 354 / 431 (82.13%) | 16 / 16 (100.00%) | 661 / 796 (83.04%) | ☐ | Unique source-location union: lines 390/418, functions 16/16, regions 725/796 |
| [`vm/builtins/execution/runtime.rs`](../../../backend/bluejs/src/vm/builtins/execution/runtime.rs) | 1,253 / 1,483 (84.49%) | 121 / 134 (90.30%) | 2,370 / 2,870 (82.58%) | ☐ | Unique source-location union: lines 1,171/1,388, functions 121/134, regions 2,371/2,870 |
| [`vm/builtins/execution/setup.rs`](../../../backend/bluejs/src/vm/builtins/execution/setup.rs) | 916 / 1,052 (87.07%) | 90 / 92 (97.83%) | 1,550 / 1,776 (87.27%) | ☐ | Unique source-location union: lines 857/979, functions 90/92, regions 1,552/1,776 |
| [`vm/builtins/general.rs`](../../../backend/bluejs/src/vm/builtins/general.rs) | 420 / 425 (98.82%) | 35 / 37 (94.59%) | 701 / 750 (93.47%) | ☐ | Unique source-location union: lines 411/414, functions 35/37, regions 701/750 |
| [`vm/builtins/generators.rs`](../../../backend/bluejs/src/vm/builtins/generators.rs) | 1,310 / 1,483 (88.33%) | 64 / 76 (84.21%) | 2,067 / 2,372 (87.14%) | ☐ | Unique source-location union: lines 1,274/1,432, functions 64/76, regions 2,067/2,372 |
| [`vm/builtins/globals.rs`](../../../backend/bluejs/src/vm/builtins/globals.rs) | 882 / 985 (89.54%) | 12 / 12 (100.00%) | 1,306 / 1,465 (89.15%) | ☐ | Unique source-location union: lines 872/971, functions 12/12, regions 1,310/1,465 |
| [`vm/builtins/immutable_arraybuffer.rs`](../../../backend/bluejs/src/vm/builtins/immutable_arraybuffer.rs) | 116 / 120 (96.67%) | 12 / 12 (100.00%) | 210 / 233 (90.13%) | ☐ | Unique source-location union: lines 110/114, functions 12/12, regions 210/233 |
| [`vm/builtins/math.rs`](../../../backend/bluejs/src/vm/builtins/math.rs) | 314 / 333 (94.29%) | 15 / 15 (100.00%) | 538 / 605 (88.93%) | ☐ | Unique source-location union: lines 306/325, functions 15/15, regions 538/605 |
| [`vm/builtins/native_dispatch.rs`](../../../backend/bluejs/src/vm/builtins/native_dispatch.rs) | - | - | - | - | Declarations/re-exports only; no executable code |
| [`vm/builtins/native_dispatch/date.rs`](../../../backend/bluejs/src/vm/builtins/native_dispatch/date.rs) | 559 / 674 (82.94%) | 47 / 54 (87.04%) | 1,014 / 1,231 (82.37%) | ☐ | Unique source-location union: lines 541/646, functions 47/54, regions 1,014/1,231 |
| [`vm/builtins/native_dispatch/dispatch.rs`](../../../backend/bluejs/src/vm/builtins/native_dispatch/dispatch.rs) | 1,122 / 1,258 (89.19%) | 28 / 37 (75.68%) | 2,822 / 3,132 (90.10%) | ☐ | Unique source-location union: lines 1,108/1,209, functions 28/37, regions 2,869/3,132 |
| [`vm/builtins/numbers.rs`](../../../backend/bluejs/src/vm/builtins/numbers.rs) | 239 / 244 (97.95%) | 14 / 15 (93.33%) | 407 / 430 (94.65%) | ☐ | Unique source-location union: lines 236/240, functions 14/15, regions 407/430 |
| [`vm/builtins/object.rs`](../../../backend/bluejs/src/vm/builtins/object.rs) | 1,100 / 1,304 (84.36%) | 77 / 88 (87.50%) | 2,051 / 2,461 (83.34%) | ☐ | Unique source-location union: lines 1,056/1,245, functions 77/88, regions 2,053/2,461 |
| [`vm/builtins/promise_combinators.rs`](../../../backend/bluejs/src/vm/builtins/promise_combinators.rs) | 401 / 412 (97.33%) | 33 / 34 (97.06%) | 790 / 876 (90.18%) | ☐ | Unique source-location union: lines 389/399, functions 33/34, regions 790/876 |
| [`vm/builtins/promise_core.rs`](../../../backend/bluejs/src/vm/builtins/promise_core.rs) | 658 / 700 (94.00%) | 56 / 57 (98.25%) | 1,094 / 1,202 (91.01%) | ☐ | Unique source-location union: lines 632/674, functions 56/57, regions 1,094/1,202 |
| [`vm/builtins/promises.rs`](../../../backend/bluejs/src/vm/builtins/promises.rs) | 828 / 939 (88.18%) | 56 / 59 (94.92%) | 1,454 / 1,638 (88.77%) | ☐ | Unique source-location union: lines 792/893, functions 56/59, regions 1,454/1,638 |
| [`vm/builtins/resource_management.rs`](../../../backend/bluejs/src/vm/builtins/resource_management.rs) | 511 / 558 (91.58%) | 32 / 34 (94.12%) | 746 / 829 (89.99%) | ☐ | Unique source-location union: lines 502/547, functions 32/34, regions 746/829 |
| [`vm/builtins/set_methods.rs`](../../../backend/bluejs/src/vm/builtins/set_methods.rs) | 254 / 261 (97.32%) | 28 / 28 (100.00%) | 512 / 575 (89.04%) | ☐ | Unique source-location union: lines 240/244, functions 28/28, regions 512/575 |
| [`vm/builtins/tests.rs`](../../../backend/bluejs/src/vm/builtins/tests.rs) | - | - | - | - | Test source; not a coverage target |
| [`vm/builtins/typed_arrays.rs`](../../../backend/bluejs/src/vm/builtins/typed_arrays.rs) | 696 / 860 (80.93%) | 35 / 42 (83.33%) | 1,374 / 1,733 (79.28%) | ☐ | Unique source-location union: lines 686/840, functions 35/42, regions 1,374/1,733 |
| [`vm/builtins/uint8array.rs`](../../../backend/bluejs/src/vm/builtins/uint8array.rs) | 291 / 508 (57.28%) | 26 / 35 (74.29%) | 454 / 768 (59.11%) | ☐ | Unique source-location union: lines 286/494, functions 26/35, regions 454/768 |
| [`vm/completion.rs`](../../../backend/bluejs/src/vm/completion.rs) | 113 / 113 (100.00%) | 9 / 9 (100.00%) | 162 / 162 (100.00%) | ☑ | Unique source-location union: lines 111/111, functions 9/9, regions 162/162 |
| [`vm/debugger.rs`](../../../backend/bluejs/src/vm/debugger.rs) | 1,837 / 1,980 (92.78%) | 101 / 109 (92.66%) | 2,826 / 3,011 (93.86%) | ☐ | Unique source-location union: lines 1,797/1,932, functions 101/109, regions 2,826/3,011 |
| [`vm/errors.rs`](../../../backend/bluejs/src/vm/errors.rs) | 350 / 389 (89.97%) | 21 / 24 (87.50%) | 640 / 764 (83.77%) | ☐ | Unique source-location union: lines 342/376, functions 21/24, regions 640/764 |
| [`vm/execution.rs`](../../../backend/bluejs/src/vm/execution.rs) | 1,373 / 1,514 (90.69%) | 131 / 139 (94.24%) | 2,258 / 2,558 (88.27%) | ☐ | Unique source-location union: lines 1,305/1,431, functions 131/139, regions 2,264/2,558 |
| [`vm/functions.rs`](../../../backend/bluejs/src/vm/functions.rs) | 105 / 106 (99.06%) | 4 / 4 (100.00%) | 198 / 213 (92.96%) | ☐ | Unique source-location union: lines 103/104, functions 4/4, regions 198/213 |
| [`vm/host_objects.rs`](../../../backend/bluejs/src/vm/host_objects.rs) | 525 / 620 (84.68%) | 40 / 58 (68.97%) | 597 / 713 (83.73%) | ☐ | Unique source-location union: lines 511/582, functions 40/58, regions 599/713 |
| [`vm/interpreter.rs`](../../../backend/bluejs/src/vm/interpreter.rs) | 1,234 / 1,356 (91.00%) | 56 / 62 (90.32%) | 2,847 / 3,178 (89.58%) | ☐ | Unique source-location union: lines 1,191/1,281, functions 56/62, regions 2,865/3,178 |
| [`vm/intl.rs`](../../../backend/bluejs/src/vm/intl.rs) | 489 / 589 (83.02%) | 16 / 17 (94.12%) | 668 / 798 (83.71%) | ☐ | Unique source-location union: lines 480/580, functions 16/17, regions 668/798 |
| [`vm/intl/collator_locale.rs`](../../../backend/bluejs/src/vm/intl/collator_locale.rs) | 423 / 431 (98.14%) | 41 / 41 (100.00%) | 674 / 722 (93.35%) | ☐ | Unique source-location union: lines 396/403, functions 41/41, regions 674/722 |
| [`vm/intl/date_time.rs`](../../../backend/bluejs/src/vm/intl/date_time.rs) | 751 / 840 (89.40%) | 51 / 56 (91.07%) | 1,133 / 1,275 (88.86%) | ☐ | Unique source-location union: lines 729/811, functions 51/56, regions 1,133/1,275 |
| [`vm/intl/list_duration.rs`](../../../backend/bluejs/src/vm/intl/list_duration.rs) | 584 / 618 (94.50%) | 46 / 55 (83.64%) | 867 / 957 (90.60%) | ☐ | Unique source-location union: lines 561/584, functions 46/55, regions 867/957 |
| [`vm/intl/number_options.rs`](../../../backend/bluejs/src/vm/intl/number_options.rs) | 421 / 456 (92.32%) | 31 / 32 (96.88%) | 570 / 653 (87.29%) | ☐ | Unique source-location union: lines 409/443, functions 31/32, regions 570/653 |
| [`vm/intl/number_runtime.rs`](../../../backend/bluejs/src/vm/intl/number_runtime.rs) | 354 / 404 (87.62%) | 23 / 28 (82.14%) | 477 / 555 (85.95%) | ☐ | Unique source-location union: lines 344/388, functions 23/28, regions 477/555 |
| [`vm/intl/plural_segmenter.rs`](../../../backend/bluejs/src/vm/intl/plural_segmenter.rs) | 563 / 588 (95.75%) | 49 / 54 (90.74%) | 796 / 875 (90.97%) | ☐ | Unique source-location union: lines 537/557, functions 49/54, regions 796/875 |
| [`vm/intl/shared.rs`](../../../backend/bluejs/src/vm/intl/shared.rs) | 683 / 737 (92.67%) | 61 / 68 (89.71%) | 1,086 / 1,248 (87.02%) | ☐ | Unique source-location union: lines 652/699, functions 61/68, regions 1,086/1,248 |
| [`vm/intrinsics.rs`](../../../backend/bluejs/src/vm/intrinsics.rs) | 437 / 443 (98.65%) | 5 / 5 (100.00%) | 582 / 594 (97.98%) | ☐ | Unique source-location union: lines 431/436, functions 5/5, regions 582/594 |
| [`vm/json.rs`](../../../backend/bluejs/src/vm/json.rs) | 721 / 786 (91.73%) | 58 / 60 (96.67%) | 1,397 / 1,590 (87.86%) | ☐ | Unique source-location union: lines 694/757, functions 58/60, regions 1,397/1,590 |
| [`vm/lifecycle.rs`](../../../backend/bluejs/src/vm/lifecycle.rs) | 200 / 200 (100.00%) | 13 / 13 (100.00%) | 214 / 217 (98.62%) | ☐ | Unique source-location union: lines 200/200, functions 13/13, regions 214/217 |
| [`vm/modules.rs`](../../../backend/bluejs/src/vm/modules.rs) | 1,617 / 1,780 (90.84%) | 77 / 85 (90.59%) | 2,516 / 2,846 (88.40%) | ☐ | Unique source-location union: lines 1,577/1,719, functions 77/85, regions 2,527/2,846 |
| [`vm/modules/deferred.rs`](../../../backend/bluejs/src/vm/modules/deferred.rs) | 290 / 307 (94.46%) | 27 / 28 (96.43%) | 444 / 489 (90.80%) | ☐ | Unique source-location union: lines 275/289, functions 27/28, regions 444/489 |
| [`vm/modules/namespace.rs`](../../../backend/bluejs/src/vm/modules/namespace.rs) | 184 / 224 (82.14%) | 11 / 17 (64.71%) | 294 / 365 (80.55%) | ☐ | Unique source-location union: lines 176/206, functions 11/17, regions 294/365 |
| [`vm/modules/synthetic.rs`](../../../backend/bluejs/src/vm/modules/synthetic.rs) | 99 / 102 (97.06%) | 3 / 3 (100.00%) | 132 / 137 (96.35%) | ☐ | Unique source-location union: lines 97/100, functions 3/3, regions 132/137 |
| [`vm/native_stack.rs`](../../../backend/bluejs/src/vm/native_stack.rs) | 98 / 99 (98.99%) | 15 / 15 (100.00%) | 173 / 175 (98.86%) | ☐ | Unique source-location union: lines 93/94, functions 15/15, regions 173/175 |
| [`vm/operations.rs`](../../../backend/bluejs/src/vm/operations.rs) | 588 / 637 (92.31%) | 44 / 47 (93.62%) | 1,031 / 1,169 (88.20%) | ☐ | Unique source-location union: lines 573/620, functions 44/47, regions 1,031/1,169 |
| [`vm/properties.rs`](../../../backend/bluejs/src/vm/properties.rs) | 639 / 705 (90.64%) | 62 / 70 (88.57%) | 1,127 / 1,295 (87.03%) | ☐ | Unique source-location union: lines 606/657, functions 62/70, regions 1,129/1,295 |
| [`vm/realm_reentrancy.rs`](../../../backend/bluejs/src/vm/realm_reentrancy.rs) | 27 / 27 (100.00%) | 8 / 8 (100.00%) | 34 / 34 (100.00%) | ☑ | Unique source-location union: lines 22/22, functions 8/8, regions 34/34 |
| [`vm/regexp.rs`](../../../backend/bluejs/src/vm/regexp.rs) | 1,056 / 1,097 (96.26%) | 60 / 60 (100.00%) | 1,905 / 2,089 (91.19%) | ☐ | Unique source-location union: lines 1,019/1,054, functions 60/60, regions 1,905/2,089 |
| [`vm/shadow_realm.rs`](../../../backend/bluejs/src/vm/shadow_realm.rs) | 315 / 380 (82.89%) | 23 / 28 (82.14%) | 538 / 630 (85.40%) | ☐ | Unique source-location union: lines 305/362, functions 23/28, regions 538/630 |
| [`vm/temporal.rs`](../../../backend/bluejs/src/vm/temporal.rs) | 539 / 569 (94.73%) | 11 / 11 (100.00%) | 518 / 567 (91.36%) | ☐ | Unique source-location union: lines 534/564, functions 11/11, regions 518/567 |
| [`vm/temporal/calendar.rs`](../../../backend/bluejs/src/vm/temporal/calendar.rs) | 147 / 147 (100.00%) | 14 / 14 (100.00%) | 215 / 215 (100.00%) | ☑ | Unique source-location union: lines 147/147, functions 14/14, regions 215/215 |
| [`vm/temporal/conversion.rs`](../../../backend/bluejs/src/vm/temporal/conversion.rs) | - | - | - | - | Declarations/re-exports only; no executable code |
| [`vm/temporal/conversion/calendar_fields.rs`](../../../backend/bluejs/src/vm/temporal/conversion/calendar_fields.rs) | 302 / 313 (96.49%) | 22 / 27 (81.48%) | 456 / 485 (94.02%) | ☐ | Unique source-location union: lines 286/290, functions 22/27, regions 456/485 |
| [`vm/temporal/conversion/from_value.rs`](../../../backend/bluejs/src/vm/temporal/conversion/from_value.rs) | 340 / 368 (92.39%) | 20 / 21 (95.24%) | 554 / 600 (92.33%) | ☐ | Unique source-location union: lines 324/351, functions 20/21, regions 554/600 |
| [`vm/temporal/conversion/getters.rs`](../../../backend/bluejs/src/vm/temporal/conversion/getters.rs) | 107 / 120 (89.17%) | 4 / 7 (57.14%) | 148 / 164 (90.24%) | ☐ | Unique source-location union: lines 103/111, functions 4/7, regions 148/164 |
| [`vm/temporal/conversion/numeric.rs`](../../../backend/bluejs/src/vm/temporal/conversion/numeric.rs) | 194 / 198 (97.98%) | 21 / 24 (87.50%) | 241 / 259 (93.05%) | ☐ | Unique source-location union: lines 185/186, functions 21/24, regions 241/259 |
| [`vm/temporal/conversion/options.rs`](../../../backend/bluejs/src/vm/temporal/conversion/options.rs) | 127 / 130 (97.69%) | 10 / 12 (83.33%) | 145 / 158 (91.77%) | ☐ | Unique source-location union: lines 124/125, functions 10/12, regions 145/158 |
| [`vm/temporal/conversion/zoned_conversion.rs`](../../../backend/bluejs/src/vm/temporal/conversion/zoned_conversion.rs) | 105 / 131 (80.15%) | 4 / 11 (36.36%) | 175 / 206 (84.95%) | ☐ | Unique source-location union: lines 103/118, functions 4/11, regions 175/206 |
| [`vm/temporal/dates.rs`](../../../backend/bluejs/src/vm/temporal/dates.rs) | - | - | - | - | Declarations/re-exports only; no executable code |
| [`vm/temporal/dates/arithmetic.rs`](../../../backend/bluejs/src/vm/temporal/dates/arithmetic.rs) | 206 / 206 (100.00%) | 8 / 8 (100.00%) | 277 / 283 (97.88%) | ☐ | Unique source-location union: lines 197/197, functions 8/8, regions 277/283 |
| [`vm/temporal/dates/comparison.rs`](../../../backend/bluejs/src/vm/temporal/dates/comparison.rs) | 49 / 49 (100.00%) | 3 / 3 (100.00%) | 65 / 66 (98.48%) | ☐ | Unique source-location union: lines 48/48, functions 3/3, regions 65/66 |
| [`vm/temporal/dates/construction.rs`](../../../backend/bluejs/src/vm/temporal/dates/construction.rs) | 211 / 231 (91.34%) | 10 / 12 (83.33%) | 233 / 252 (92.46%) | ☐ | Unique source-location union: lines 208/224, functions 10/12, regions 233/252 |
| [`vm/temporal/dates/conversions.rs`](../../../backend/bluejs/src/vm/temporal/dates/conversions.rs) | 66 / 69 (95.65%) | 3 / 6 (50.00%) | 105 / 119 (88.24%) | ☐ | Unique source-location union: lines 66/66, functions 3/6, regions 105/119 |
| [`vm/temporal/dates/date_time.rs`](../../../backend/bluejs/src/vm/temporal/dates/date_time.rs) | 128 / 133 (96.24%) | 8 / 9 (88.89%) | 202 / 214 (94.39%) | ☐ | Unique source-location union: lines 122/125, functions 8/9, regions 202/214 |
| [`vm/temporal/dates/formatting.rs`](../../../backend/bluejs/src/vm/temporal/dates/formatting.rs) | 167 / 171 (97.66%) | 5 / 6 (83.33%) | 222 / 229 (96.94%) | ☐ | Unique source-location union: lines 165/167, functions 5/6, regions 222/229 |
| [`vm/temporal/dates/now_and_zone.rs`](../../../backend/bluejs/src/vm/temporal/dates/now_and_zone.rs) | 182 / 192 (94.79%) | 16 / 20 (80.00%) | 247 / 266 (92.86%) | ☐ | Unique source-location union: lines 178/183, functions 16/20, regions 247/266 |
| [`vm/temporal/dates/with.rs`](../../../backend/bluejs/src/vm/temporal/dates/with.rs) | 140 / 140 (100.00%) | 6 / 6 (100.00%) | 254 / 264 (96.21%) | ☐ | Unique source-location union: lines 135/135, functions 6/6, regions 254/264 |
| [`vm/temporal/duration_conversion.rs`](../../../backend/bluejs/src/vm/temporal/duration_conversion.rs) | 47 / 49 (95.92%) | 3 / 4 (75.00%) | 70 / 77 (90.91%) | ☐ | Unique source-location union: lines 45/46, functions 3/4, regions 70/77 |
| [`vm/temporal/duration_math.rs`](../../../backend/bluejs/src/vm/temporal/duration_math.rs) | 305 / 305 (100.00%) | 29 / 29 (100.00%) | 361 / 361 (100.00%) | ☑ | Unique source-location union: lines 305/305, functions 29/29, regions 361/361 |
| [`vm/temporal/duration_operations.rs`](../../../backend/bluejs/src/vm/temporal/duration_operations.rs) | 620 / 621 (99.84%) | 26 / 27 (96.30%) | 900 / 923 (97.51%) | ☐ | Unique source-location union: lines 600/600, functions 26/27, regions 900/923 |
| [`vm/temporal/duration_relative.rs`](../../../backend/bluejs/src/vm/temporal/duration_relative.rs) | 515 / 532 (96.80%) | 37 / 44 (84.09%) | 639 / 677 (94.39%) | ☐ | Unique source-location union: lines 491/499, functions 37/44, regions 639/677 |
| [`vm/temporal/epoch.rs`](../../../backend/bluejs/src/vm/temporal/epoch.rs) | 136 / 136 (100.00%) | 13 / 13 (100.00%) | 237 / 237 (100.00%) | ☑ | Unique source-location union: lines 136/136, functions 13/13, regions 237/237 |
| [`vm/temporal/instant.rs`](../../../backend/bluejs/src/vm/temporal/instant.rs) | 402 / 462 (87.01%) | 20 / 32 (62.50%) | 574 / 667 (86.06%) | ☐ | Unique source-location union: lines 399/443, functions 20/32, regions 574/667 |
| [`vm/temporal/iso.rs`](../../../backend/bluejs/src/vm/temporal/iso.rs) | 17 / 17 (100.00%) | 3 / 3 (100.00%) | 24 / 24 (100.00%) | ☑ | Unique source-location union: lines 17/17, functions 3/3, regions 24/24 |
| [`vm/temporal/iso/annotations.rs`](../../../backend/bluejs/src/vm/temporal/iso/annotations.rs) | 154 / 155 (99.35%) | 21 / 21 (100.00%) | 274 / 285 (96.14%) | ☐ | Unique source-location union: lines 140/140, functions 21/21, regions 275/285 |
| [`vm/temporal/iso/datetime.rs`](../../../backend/bluejs/src/vm/temporal/iso/datetime.rs) | 129 / 129 (100.00%) | 13 / 13 (100.00%) | 216 / 219 (98.63%) | ☐ | Unique source-location union: lines 127/127, functions 13/13, regions 217/219 |
| [`vm/temporal/iso/duration.rs`](../../../backend/bluejs/src/vm/temporal/iso/duration.rs) | 104 / 104 (100.00%) | 4 / 4 (100.00%) | 164 / 168 (97.62%) | ☐ | Unique source-location union: lines 101/101, functions 4/4, regions 165/168 |
| [`vm/temporal/iso/offset.rs`](../../../backend/bluejs/src/vm/temporal/iso/offset.rs) | 124 / 125 (99.20%) | 7 / 7 (100.00%) | 225 / 233 (96.57%) | ☐ | Unique source-location union: lines 123/124, functions 7/7, regions 227/233 |
| [`vm/temporal/iso/scan.rs`](../../../backend/bluejs/src/vm/temporal/iso/scan.rs) | 236 / 236 (100.00%) | 31 / 31 (100.00%) | 451 / 473 (95.35%) | ☐ | Unique source-location union: lines 225/225, functions 31/31, regions 452/473 |
| [`vm/temporal/iso/tests.rs`](../../../backend/bluejs/src/vm/temporal/iso/tests.rs) | - | - | - | - | Test source; not a coverage target |
| [`vm/temporal/plain_date.rs`](../../../backend/bluejs/src/vm/temporal/plain_date.rs) | - | - | - | - | Declarations/re-exports only; no executable code |
| [`vm/temporal/plain_date/calendar_add.rs`](../../../backend/bluejs/src/vm/temporal/plain_date/calendar_add.rs) | 131 / 131 (100.00%) | 3 / 3 (100.00%) | 230 / 241 (95.44%) | ☐ | Unique source-location union: lines 131/131, functions 3/3, regions 230/241 |
| [`vm/temporal/plain_date/calendar_difference.rs`](../../../backend/bluejs/src/vm/temporal/plain_date/calendar_difference.rs) | 185 / 188 (98.40%) | 7 / 7 (100.00%) | 335 / 339 (98.82%) | ☐ | Unique source-location union: lines 183/186, functions 7/7, regions 335/339 |
| [`vm/temporal/plain_date/format.rs`](../../../backend/bluejs/src/vm/temporal/plain_date/format.rs) | 21 / 22 (95.45%) | 3 / 3 (100.00%) | 38 / 39 (97.44%) | ☐ | Unique source-location union: lines 21/22, functions 3/3, regions 38/39 |
| [`vm/temporal/plain_date/iso_date.rs`](../../../backend/bluejs/src/vm/temporal/plain_date/iso_date.rs) | 112 / 113 (99.12%) | 16 / 16 (100.00%) | 203 / 204 (99.51%) | ☐ | Unique source-location union: lines 111/112, functions 16/16, regions 203/204 |
| [`vm/temporal/plain_date/month_structure.rs`](../../../backend/bluejs/src/vm/temporal/plain_date/month_structure.rs) | 110 / 110 (100.00%) | 11 / 11 (100.00%) | 168 / 177 (94.92%) | ☐ | Unique source-location union: lines 110/110, functions 11/11, regions 168/177 |
| [`vm/temporal/plain_date/round_duration.rs`](../../../backend/bluejs/src/vm/temporal/plain_date/round_duration.rs) | 122 / 126 (96.83%) | 5 / 5 (100.00%) | 182 / 188 (96.81%) | ☐ | Unique source-location union: lines 120/124, functions 5/5, regions 182/188 |
| [`vm/temporal/plain_date/tests.rs`](../../../backend/bluejs/src/vm/temporal/plain_date/tests.rs) | - | - | - | - | Test source; not a coverage target |
| [`vm/temporal/plain_date_time_difference.rs`](../../../backend/bluejs/src/vm/temporal/plain_date_time_difference.rs) | 784 / 785 (99.87%) | 54 / 54 (100.00%) | 1,088 / 1,105 (98.46%) | ☐ | Unique source-location union: lines 775/776, functions 54/54, regions 1,089/1,105 |
| [`vm/temporal/plain_month_day.rs`](../../../backend/bluejs/src/vm/temporal/plain_month_day.rs) | 282 / 282 (100.00%) | 28 / 28 (100.00%) | 386 / 386 (100.00%) | ☑ | Unique source-location union: lines 279/279, functions 28/28, regions 386/386 |
| [`vm/temporal/plain_time.rs`](../../../backend/bluejs/src/vm/temporal/plain_time.rs) | 596 / 649 (91.83%) | 37 / 44 (84.09%) | 844 / 922 (91.54%) | ☐ | Unique source-location union: lines 584/626, functions 37/44, regions 844/922 |
| [`vm/temporal/plain_year_month.rs`](../../../backend/bluejs/src/vm/temporal/plain_year_month.rs) | 145 / 145 (100.00%) | 13 / 13 (100.00%) | 193 / 193 (100.00%) | ☑ | Unique source-location union: lines 143/143, functions 13/13, regions 193/193 |
| [`vm/temporal/receiver.rs`](../../../backend/bluejs/src/vm/temporal/receiver.rs) | 40 / 40 (100.00%) | 2 / 2 (100.00%) | 39 / 40 (97.50%) | ☐ | Unique source-location union: lines 40/40, functions 2/2, regions 39/40 |
| [`vm/temporal/rounding.rs`](../../../backend/bluejs/src/vm/temporal/rounding.rs) | 395 / 395 (100.00%) | 33 / 33 (100.00%) | 643 / 644 (99.84%) | ☐ | Unique source-location union: lines 395/395, functions 33/33, regions 643/644 |
| [`vm/temporal/time_zone.rs`](../../../backend/bluejs/src/vm/temporal/time_zone.rs) | 512 / 513 (99.81%) | 47 / 48 (97.92%) | 1,006 / 1,013 (99.31%) | ☐ | Unique source-location union: lines 503/503, functions 47/48, regions 1,007/1,013 |
| [`vm/temporal/time_zone_id.rs`](../../../backend/bluejs/src/vm/temporal/time_zone_id.rs) | 202 / 202 (100.00%) | 28 / 28 (100.00%) | 457 / 465 (98.28%) | ☐ | Unique source-location union: lines 192/192, functions 28/28, regions 457/465 |
| [`vm/temporal/year_month.rs`](../../../backend/bluejs/src/vm/temporal/year_month.rs) | 971 / 1,017 (95.48%) | 68 / 82 (82.93%) | 1,537 / 1,635 (94.01%) | ☐ | Unique source-location union: lines 921/949, functions 68/82, regions 1,537/1,635 |
| [`vm/temporal/zoned.rs`](../../../backend/bluejs/src/vm/temporal/zoned.rs) | - | - | - | - | Declarations/re-exports only; no executable code |
| [`vm/temporal/zoned/arithmetic.rs`](../../../backend/bluejs/src/vm/temporal/zoned/arithmetic.rs) | 172 / 180 (95.56%) | 8 / 9 (88.89%) | 275 / 288 (95.49%) | ☐ | Unique source-location union: lines 163/169, functions 8/9, regions 275/288 |
| [`vm/temporal/zoned/conversions.rs`](../../../backend/bluejs/src/vm/temporal/zoned/conversions.rs) | 115 / 117 (98.29%) | 8 / 9 (88.89%) | 159 / 172 (92.44%) | ☐ | Unique source-location union: lines 114/115, functions 8/9, regions 159/172 |
| [`vm/temporal/zoned/formatting.rs`](../../../backend/bluejs/src/vm/temporal/zoned/formatting.rs) | 152 / 157 (96.82%) | 6 / 8 (75.00%) | 241 / 255 (94.51%) | ☐ | Unique source-location union: lines 149/152, functions 6/8, regions 241/255 |
| [`vm/temporal/zoned/from_value.rs`](../../../backend/bluejs/src/vm/temporal/zoned/from_value.rs) | 184 / 196 (93.88%) | 13 / 15 (86.67%) | 250 / 265 (94.34%) | ☐ | Unique source-location union: lines 173/181, functions 13/15, regions 250/265 |
| [`vm/temporal/zoned/resolution.rs`](../../../backend/bluejs/src/vm/temporal/zoned/resolution.rs) | 99 / 99 (100.00%) | 7 / 7 (100.00%) | 126 / 127 (99.21%) | ☐ | Unique source-location union: lines 99/99, functions 7/7, regions 126/127 |
| [`vm/temporal/zoned/since_until.rs`](../../../backend/bluejs/src/vm/temporal/zoned/since_until.rs) | 141 / 141 (100.00%) | 8 / 8 (100.00%) | 202 / 209 (96.65%) | ☐ | Unique source-location union: lines 137/137, functions 8/8, regions 202/209 |
| [`vm/temporal/zoned/with.rs`](../../../backend/bluejs/src/vm/temporal/zoned/with.rs) | 205 / 205 (100.00%) | 9 / 9 (100.00%) | 352 / 369 (95.39%) | ☐ | Unique source-location union: lines 199/199, functions 9/9, regions 352/369 |
| [`vm/temporal/zoned_date_time.rs`](../../../backend/bluejs/src/vm/temporal/zoned_date_time.rs) | 218 / 222 (98.20%) | 15 / 15 (100.00%) | 361 / 368 (98.10%) | ☐ | Unique source-location union: lines 217/221, functions 15/15, regions 361/368 |
| [`vm/temporal/zoned_difference.rs`](../../../backend/bluejs/src/vm/temporal/zoned_difference.rs) | 767 / 771 (99.48%) | 43 / 43 (100.00%) | 1,056 / 1,090 (96.88%) | ☐ | Unique source-location union: lines 763/767, functions 43/43, regions 1,056/1,090 |
| [`vm/test262.rs`](../../../backend/bluejs/src/vm/test262.rs) | 60 / 60 (100.00%) | 6 / 6 (100.00%) | 96 / 97 (98.97%) | ☐ | Unique source-location union: lines 57/57, functions 6/6, regions 96/97 |
| [`vm/test262/assertions.rs`](../../../backend/bluejs/src/vm/test262/assertions.rs) | 288 / 299 (96.32%) | 16 / 17 (94.12%) | 750 / 838 (89.50%) | ☐ | Unique source-location union: lines 287/297, functions 16/17, regions 750/838 |
| [`vm/test262/cases.rs`](../../../backend/bluejs/src/vm/test262/cases.rs) | 647 / 686 (94.31%) | 39 / 45 (86.67%) | 1,129 / 1,263 (89.39%) | ☐ | Unique source-location union: lines 620/648, functions 39/45, regions 1,129/1,263 |
| [`vm/test262/foreign.rs`](../../../backend/bluejs/src/vm/test262/foreign.rs) | 1,413 / 1,581 (89.37%) | 124 / 143 (86.71%) | 2,217 / 2,640 (83.98%) | ☐ | Unique source-location union: lines 1,335/1,474, functions 124/143, regions 2,217/2,640 |
| [`vm/test262/harness.rs`](../../../backend/bluejs/src/vm/test262/harness.rs) | 230 / 257 (89.49%) | 12 / 12 (100.00%) | 369 / 418 (88.28%) | ☐ | Unique source-location union: lines 223/248, functions 12/12, regions 369/418 |
| [`vm/test262/reverse.rs`](../../../backend/bluejs/src/vm/test262/reverse.rs) | 260 / 270 (96.30%) | 24 / 26 (92.31%) | 427 / 464 (92.03%) | ☐ | Unique source-location union: lines 249/256, functions 24/26, regions 427/464 |
| [`vm/test262_agents.rs`](../../../backend/bluejs/src/vm/test262_agents.rs) | 421 / 462 (91.13%) | 42 / 49 (85.71%) | 593 / 664 (89.31%) | ☐ | Unique source-location union: lines 412/443, functions 42/49, regions 593/664 |
| [`vm/tests.rs`](../../../backend/bluejs/src/vm/tests.rs) | - | - | - | - | Test source; not a coverage target |
| **Total (160 instrumented files)** | **72,141 / 77,332 (93.29%)** | **5,211 / 5,576 (93.45%)** | **120,436 / 132,001 (91.24%)** | ☐ |  |

## Historical differences from the other platforms (2026-09-21)

The 2026-09-21 `eaeb5c1` measurement recorded one Ubuntu/macOS difference, `staging/sm/Math/acosh-approx.js` (Ubuntu fail, macOS pass). The current 2026-09-25 Linux inventory has the same status for every scheduled mode as the current macOS report, though the two runs used different source commits.

## Reproduce the current Test262 inventory

```sh
python3 -m pip install -r backend/bluejs/test262/requirements.txt
cargo build -p blueice-bluejs --bins --offline
python3 -m unittest discover -s backend/bluejs/test262 -v
python3 backend/bluejs/test262/run.py --corpus development/browser_core/reference/test262 --jobs 4 --output target/test262-linux-20260925-current --progress-interval 60
python3 backend/bluejs/test262/analyze.py --run target/test262-linux-20260925-current --corpus development/browser_core/reference/test262 --output target/test262-linux-20260925-current-analysis
python3 backend/bluejs/coverage_file.py compiler/statements.rs --update-linux-report
```

The runner validates the pinned corpus marker, manifest hash, and every corpus file. It returns 1 for this run because 1 `stale_corpus` and 4 `excluded` modes remain non-pass statuses; inspect `summary.json` and use `analyze.py` to reconcile all records. Keep the host otherwise idle during the inventory because ordinary cases have a two-second wall deadline.
