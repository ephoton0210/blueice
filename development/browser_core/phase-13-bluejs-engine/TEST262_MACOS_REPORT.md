# macOS Test262 Report

## Current complete inventory (2026-10-01)

The pinned, unfiltered Test262 snapshot was rerun on macOS 26.6.2 (build 25G83, Apple silicon) with Rust/Cargo 1.98.0 and Python 3.14.6, using source commit `3d7be209c` plus the local Array.push and revoked-Proxy crash corrections. The exact production changes and source hashes are retained in [source.patch](../../../target/test262-macos-20261001-crash-fix/source.patch) and [source-state.json](../../../target/test262-macos-20261001-crash-fix/source-state.json). The verified corpus revision is `72faf8ec1445c55149615e8b35187830783aba1a` and includes main, proposals, and staging. The verified archive in `/tmp/blueice-test262-72faf8ec-20261001` was reused. The complete command was `/tmp/bluejs-conformance-venv/bin/python backend/bluejs/test262/run.py --corpus /tmp/blueice-test262-72faf8ec-20261001 --jobs 8 --output target/test262-macos-20261001-crash-fix --progress-interval 60`, after `cargo build -p blueice-bluejs --bins --offline`; it completed in 193.718 seconds. The runner verified the pinned corpus marker, manifest, and every file before execution. `analyze.py` reconciled all 53,582 files and 102,926 modes against `results.jsonl` and `summary.json`.

Adapter SHA-256: `24a861ec8f6c94544e62a674948694bc9669b4171d191e9166472bd2fa629745`. RegExp worker SHA-256: `33e70af589f5fab57df07cdec41020444f4b0f066fc1781cca52f51fdce0ed01`. Runner SHA-256: `fe9356675cbc3cea1092dc4b94206be7098df1b0cf976d0e40a215c1ca5af5bc`. The [checked-in summary](test262-summary.json) contains the complete feature and top-level group counts; the full per-mode evidence is in `target/test262-macos-20261001-crash-fix/results.jsonl`, with the reconciled analysis in `target/test262-macos-20261001-crash-fix-analysis/`. [outcome-diff.json](../../../target/test262-macos-20261001-crash-fix/outcome-diff.json) verifies that exactly the five former crash modes changed from `fail` to `pass`; all other 102,921 modes retain their statuses, expected outcomes, actual kinds and phases, and source hashes. The original independent crash reproductions remain in `target/test262-macos-20261001/crash-diagnostics.json`.

**Dispatched, applicable modes: 102,921 / 102,921 pass (100.000%).** The raw scheduled inventory is **102,921 / 102,926 pass (99.995%)**. There are **0 `fail`, 0 `unsupported`, 0 `timeout`, and 0 `harness_error`** outcomes. Four modes are `excluded` by this host's declared `[[CanBlock]] = true` capability and one pinned fixture is classified `stale_corpus`; these five modes were not dispatched or counted as passes. The runner exits 1 whenever any scheduled mode is not `pass`, so its exit code is 1 for this complete, reconciled run. All five crashes identified in the earlier 2026-10-01 run are corrected. The remaining non-pass modes and reasons are listed below.

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
| `annexB/language/function-code/block-decl-func-skip-arguments.js` | 1 | `stale_corpus` | The runner classifies the pinned fixture as contradicting the current Annex B `FunctionDeclarationInstantiation` behavior; the upstream correction is tracked in Test262 issue #5113 / PR #5112. |
| `built-ins/Atomics/wait/cannot-suspend-throws.js` | 2 | `excluded` | Fixture requires `CanBlockIsFalse`; this host declares `[[CanBlock]] = true`. |
| `built-ins/Atomics/wait/bigint/cannot-suspend-throws.js` | 2 | `excluded` | Fixture requires `CanBlockIsFalse`; this host declares `[[CanBlock]] = true`. |

### Corrected crash regressions (2026-10-01)

| Path | Modes | Current status | Correction |
| --- | ---: | --- | --- |
| `built-ins/Array/prototype/push/S15.4.4.7_A3.js` | 2 | `pass` | Length overflow propagates a catchable RangeError. Overflowing push uses the generic algorithm, preserving all argument writes before the final length Set. |
| `built-ins/Proxy/has/null-handler.js` | 2 | `pass` | The revoked Proxy error propagates as a catchable TypeError from `in`. |
| `built-ins/Proxy/has/null-handler-using-with.js` | 1 | `pass` | The same TypeError propagation protects `with` environment lookups. |

Three new public-interface Rust regression tests reproduced the original panics before the corrections. All 119 tests across `arrays`, `conformance_edges`, `array_generic_methods`, `array_mutator_edges`, `proxy_trap_gc_rooting`, and `object_to_string_proxy` now pass. The regressions cover strict/sloppy execution, empty push at the maximum length, all writes before overflowing push throws (also with a one-object nursery), revocation during property-key coercion, and sloppy `with` lookup. `cargo clippy -p blueice-bluejs --all-targets --offline -- -D warnings` passes. Formatting checks pass for all changed Rust files. `cargo fmt --all -- --check` still reports existing formatting differences elsewhere in the workspace.

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

The earlier macOS 26.6.2 run at `eaeb5c1` recorded 99,899 pass, 3,025 fail, and 2 timeout out of 102,926 modes (97.059%). Its three-platform comparison belongs to that earlier source revision. The current 2026-10-01 inventory above replaces it as the macOS Test262 status. The complete 2026-09-25 run at `e9c15268` had the same 102,921 pass, 1 stale-corpus, and 4 excluded outcomes. Exact path/mode/status/source-hash comparisons found zero changes across all 102,926 modes between that run and the first 2026-09-28 run, between the first and preceding final 2026-09-28 runs, between that final and complete73, between complete73 and complete74, between complete74 and complete75, and between complete75 and complete76. The 2026-09-29 rerun at `dee6e8718` also matched the 2026-09-28 batch5 run in every status, expected outcome, actual kind, actual phase, and source hash, recording 102,921 pass, 1 stale-corpus, and 4 excluded outcomes. The pre-fix 2026-10-01 rerun at `741500476` recorded five crash failures; the current crash-fix rerun restores those five modes to pass.

## Historical verification on this platform (2026-09-21 source revision)

These checks were performed for the earlier `eaeb5c1` source revision. They are retained as historical evidence, not as measurements of the current Test262 run.

| Check | Command | Result | Gate |
| --- | --- | --- | --- |
| Workspace tests | `cargo test --workspace --no-fail-fast` | **2,992 passed, 0 failed**, 5 ignored | all pass |
| Line coverage, workspace (CI `Coverage` job) | `cargo llvm-cov --workspace --ignore-filename-regex 'extension/src/main\.rs$\|frontend-reference/src/main\.rs$\|mcp-server/src/main\.rs$\|mcp-server/src/server\.rs$' --fail-under-lines 90 --summary-only` | 92.30% lines (90,775 / 98,347); functions 93.12%; regions 90.03% | ≥ 90% lines: met |
| Line coverage, `blueice-bluejs` alone | `cargo llvm-cov -p blueice-bluejs --fail-under-lines 88 --summary-only` | 91.36% lines (58,943 / 64,517); functions 91.70%; regions 88.43% | ≥ 88% lines: met |
| Line coverage, `blueice-ecma402` alone | `cargo llvm-cov -p blueice-ecma402 --summary-only` | 93.55% lines (9,471 / 10,124); functions 92.66%; regions 91.31% | informational |
| Node differential oracle | `cargo test -p blueice-bluejs --test node_differential -- --ignored` (Node v24.21.0) | **4 / 4 tests pass**: the 22,268-script main corpus plus the 10-script and 67-script matrices (Intl NumberFormat range/locale data) agree with Node | all pass |
| TypeScript compatibility oracle | `npm exec --yes --package typescript@5.9.3 -- env BLUEICE_BLUETSC_ORACLE=tsc cargo test -p blueice-bluets --test typescript_oracle -- --ignored` | **1 / 1 test passes**: all 68 cases (48 compile-and-run cases whose stdout is compared, 20 diagnostic-parity cases; 71 module sources) agree with TypeScript 5.9.3 | all pass |

For that historical measurement, coverage used Homebrew `llvm@22` (LLVM 22.1.8, the same LLVM version as that `rustc`) through `LLVM_COV`/`LLVM_PROFDATA`, since Homebrew's Rust ships without `llvm-tools`. Node 24.21.0 was Homebrew's `node@24` (the default `node` on that machine was 26.7.0, so the oracles were run with `node@24` first in `PATH`, matching CI). The Node oracle initially disagreed with Node 24 on three scripts that assert Node's legacy behaviour for a hook installed on a primitive's prototype (`'a'.match(3)`, `'a'.search('b')`, `'a'.matchAll(true)`); BlueJS follows ECMA-262 and Test262 there, so those lines were removed from the oracle corpus in `fff18c4`.

Line coverage is a different measure from a Test262 pass rate and the two must not be quoted interchangeably: it is the fraction of the Rust source lines that execute during the crates' own test suites. The five ignored tests are the opt-in oracles (four Node differential tests and one TypeScript compatibility test), which the table's last two rows run explicitly.

Line coverage is a different measure from a Test262 pass rate and the two must not be quoted interchangeably: it is the fraction of the Rust source lines that execute during the crates' own test suites. The five ignored tests are the opt-in oracles (four Node differential tests and one TypeScript compatibility test), which the table's last two rows run explicitly. `blueice-ecma402`'s line coverage (93.55%) is a few points below the previously recorded 94.36%: the workspace `Cargo.toml` now builds every dependency (including `blueice-ecma402` itself, in dev/test profiles) at `opt-level = 3` to keep RegExp-heavy Test262 cases inside their wall-clock budget (see `Cargo.toml`'s own comment), and the optimiser eliminates or merges a handful of source lines that the unoptimised build separately instrumented; this is a real build-configuration effect, not a regression in what the crate's tests exercise.

## Later BlueJS per-file coverage (2026-10-01)

This is a separate BlueJS coverage measurement at source commit `741500476` on Darwin 26.6.2 (`arm64`), rustc 1.98.0 (88d9e12ae 2026-08-18), `cargo-llvm-cov 0.9.1`, and Homebrew LLVM 22.1.8 via `LLVM_COV`/`LLVM_PROFDATA`. The complete default BlueJS Rust suite recorded **3,736 passed, 0 failed, 4 ignored**. The opt-in Node oracle was not included, and workspace coverage was not remeasured at this revision.

The full pinned Test262 inventory was then run with the instrumented adapter: 102,916 `pass`, 5 `fail`, 4 `excluded`, 1 `stale_corpus`, 0 `unsupported`, 0 `timeout`, 0 `harness_error`, out of 102,926 scheduled modes, in 221.129 seconds. All 102,926 per-mode statuses, expected outcomes, source hashes, and actual outcome kinds and phases match the pre-fix ordinary run at `741500476`, retained in `target/test262-macos-20261001/`. This coverage measurement predates the crash corrections. Its exit code is 1 because the non-pass outcomes remain visible. This coverage measurement is separate from the ordinary Test262 pass rate and historical verification above.

The measurement cleared prior LLVM execution profiles, reused instrumented Cargo build artifacts, and released raw profiles and incremental compilation caches afterward. The final LLVM export uses only the 327 executables actually run by this suite and the instrumented inventory, identified from the Cargo transcript and adapter/RegExp-worker paths in [objects.json](../../../target/test262-macos-20261001-coverage/objects.json). Executables that were not part of this run are excluded. The final scope is `backend/bluejs/src/`, with test source files excluded as in the earlier table. This also removes the inlined Rust standard-library source that Rust 1.98 adds to Cargo's default export. The two newly listed debugger test files were audited as test sources, and LLVM text's `P`/`E` counter suffixes are accepted when computing the source-location union. Per-file LLVM counters are preserved; scoped totals are their sums.

The [scoped LLVM JSON](../../../target/test262-macos-20261001-coverage/raw/measured-bluejs-coverage.json), [source-line text](../../../target/test262-macos-20261001-coverage/raw/measured-bluejs-coverage.txt), [instrumented summary](../../../target/test262-macos-20261001-coverage/raw/test262/summary.json), merged `measured.profdata`, original Cargo exports, and reconciled analysis in `target/test262-macos-20261001-coverage/analysis/` are retained. The [measurement driver](../../../target/test262-macos-20261001-coverage/refresh.py) uses the existing coverage helper's complete Rust-plus-Test262 workflow with the verified temporary corpus; it replaces the helper's fixed historical pass-count assertion with full `analyze.py` reconciliation, preserving the five measured failures. The [export driver](../../../target/test262-macos-20261001-coverage/export_measured.py) scopes the retained evidence and regenerates the table. With the verified corpus and Python environment prepared as below, reproduce this measurement with:

```sh
LLVM_COV=/opt/homebrew/opt/llvm@22/bin/llvm-cov LLVM_PROFDATA=/opt/homebrew/opt/llvm@22/bin/llvm-profdata PYTHONDONTWRITEBYTECODE=1 /tmp/bluejs-conformance-venv/bin/python target/test262-macos-20261001-coverage/refresh.py > /tmp/bluejs-macos-report-coverage-20261001.log 2>&1
PYTHONDONTWRITEBYTECODE=1 /tmp/bluejs-conformance-venv/bin/python target/test262-macos-20261001-coverage/finalize_report.py
```

Each measured cell shows LLVM JSON's raw covered / instrumented counts and the coverage rate. All 182 Rust files under `backend/bluejs/src/` are listed: 162 have LLVM counters; 20 use `-` with an individual reason in `Note`. A `0%` result requires a positive instrumented denominator and zero covered units. `☑` means **lines, functions and regions all reach 100%**; `☐` means at least one is below 100%. The total aggregates only instrumented files. LLVM can count separate compiled instances of the same source in different test binaries. `Note` shows the union across those binaries when its counts differ: each source location is counted once and considered covered if any binary executes it. The raw LLVM counts in the main columns determine completion. Region coverage is separate from branch coverage. The retained measurement driver reruns the entire measured suite, since tests outside a file can still exercise it.

| Source file (relative to `backend/bluejs/src/`) | Lines | Functions | Regions | Complete | Note |
| --- | ---: | ---: | ---: | :---: | --- |
| [`ast.rs`](../../../backend/bluejs/src/ast.rs) | 940 / 940 (100.00%) | 95 / 95 (100.00%) | 1,249 / 1,249 (100.00%) | ☑ | Unique source-location union: lines 890/890, functions 95/95, regions 1,249/1,249 |
| [`ast/retained_payload.rs`](../../../backend/bluejs/src/ast/retained_payload.rs) | 510 / 515 (99.03%) | 41 / 41 (100.00%) | 830 / 855 (97.08%) | ☐ | Unique source-location union: lines 509/514, functions 41/41, regions 830/855 |
| [`bin/bluejs-regexp-worker.rs`](../../../backend/bluejs/src/bin/bluejs-regexp-worker.rs) | 3 / 3 (100.00%) | 1 / 1 (100.00%) | 3 / 3 (100.00%) | ☑ | Unique source-location union: lines 3/3, functions 1/1, regions 3/3 |
| [`bin/bluejs-test262.rs`](../../../backend/bluejs/src/bin/bluejs-test262.rs) | 631 / 631 (100.00%) | 56 / 56 (100.00%) | 1,211 / 1,211 (100.00%) | ☑ | Unique source-location union: lines 613/613, functions 56/56, regions 1,211/1,211 |
| [`bytecode.rs`](../../../backend/bluejs/src/bytecode.rs) | 116 / 116 (100.00%) | 18 / 18 (100.00%) | 123 / 123 (100.00%) | ☑ | Unique source-location union: lines 115/115, functions 18/18, regions 123/123 |
| [`compiler.rs`](../../../backend/bluejs/src/compiler.rs) | 1,532 / 1,532 (100.00%) | 147 / 147 (100.00%) | 2,161 / 2,164 (99.86%) | ☐ | Unique source-location union: lines 1,460/1,460, functions 147/147, regions 2,161/2,164 |
| [`compiler/expressions.rs`](../../../backend/bluejs/src/compiler/expressions.rs) | 1,540 / 1,540 (100.00%) | 42 / 42 (100.00%) | 3,408 / 3,408 (100.00%) | ☑ | Unique source-location union: lines 1,528/1,528, functions 42/42, regions 3,408/3,408 |
| [`compiler/expressions/tests.rs`](../../../backend/bluejs/src/compiler/expressions/tests.rs) | - | - | - | - | Test source; not a coverage target |
| [`compiler/functions.rs`](../../../backend/bluejs/src/compiler/functions.rs) | 993 / 993 (100.00%) | 52 / 52 (100.00%) | 1,836 / 1,836 (100.00%) | ☑ | Unique source-location union: lines 964/964, functions 52/52, regions 1,836/1,836 |
| [`compiler/functions/tests.rs`](../../../backend/bluejs/src/compiler/functions/tests.rs) | - | - | - | - | Test source; not a coverage target |
| [`compiler/private_validation.rs`](../../../backend/bluejs/src/compiler/private_validation.rs) | 337 / 337 (100.00%) | 32 / 32 (100.00%) | 701 / 701 (100.00%) | ☑ | Unique source-location union: lines 318/318, functions 32/32, regions 701/701 |
| [`compiler/private_validation/tests.rs`](../../../backend/bluejs/src/compiler/private_validation/tests.rs) | - | - | - | - | Test source; not a coverage target |
| [`compiler/statements.rs`](../../../backend/bluejs/src/compiler/statements.rs) | 1,348 / 1,348 (100.00%) | 69 / 69 (100.00%) | 2,637 / 2,637 (100.00%) | ☑ | Unique source-location union: lines 1,322/1,322, functions 69/69, regions 2,637/2,637 |
| [`compiler/statements/tests.rs`](../../../backend/bluejs/src/compiler/statements/tests.rs) | - | - | - | - | Test source; not a coverage target |
| [`heap.rs`](../../../backend/bluejs/src/heap.rs) | 744 / 744 (100.00%) | 78 / 78 (100.00%) | 1,190 / 1,190 (100.00%) | ☑ | Unique source-location union: lines 723/723, functions 78/78, regions 1,190/1,190 |
| [`heap/binary_data.rs`](../../../backend/bluejs/src/heap/binary_data.rs) | 850 / 850 (100.00%) | 77 / 77 (100.00%) | 1,302 / 1,302 (100.00%) | ☑ | Unique source-location union: lines 832/832, functions 77/77, regions 1,302/1,302 |
| [`heap/collection_iteration.rs`](../../../backend/bluejs/src/heap/collection_iteration.rs) | 93 / 93 (100.00%) | 5 / 5 (100.00%) | 120 / 120 (100.00%) | ☑ | Unique source-location union: lines 93/93, functions 5/5, regions 120/120 |
| [`heap/core.rs`](../../../backend/bluejs/src/heap/core.rs) | 1,249 / 1,249 (100.00%) | 115 / 115 (100.00%) | 2,531 / 2,531 (100.00%) | ☑ | Unique source-location union: lines 1,222/1,222, functions 115/115, regions 2,531/2,531 |
| [`heap/debugger.rs`](../../../backend/bluejs/src/heap/debugger.rs) | 163 / 163 (100.00%) | 10 / 10 (100.00%) | 239 / 239 (100.00%) | ☑ | Unique source-location union: lines 161/161, functions 10/10, regions 239/239 |
| [`heap/exotic.rs`](../../../backend/bluejs/src/heap/exotic.rs) | 996 / 996 (100.00%) | 119 / 119 (100.00%) | 1,160 / 1,160 (100.00%) | ☑ | Unique source-location union: lines 966/966, functions 119/119, regions 1,160/1,160 |
| [`heap/lifecycle.rs`](../../../backend/bluejs/src/heap/lifecycle.rs) | 294 / 294 (100.00%) | 31 / 31 (100.00%) | 504 / 504 (100.00%) | ☑ | Unique source-location union: lines 279/279, functions 31/31, regions 504/504 |
| [`heap/object_storage.rs`](../../../backend/bluejs/src/heap/object_storage.rs) | 1,222 / 1,223 (99.92%) | 72 / 72 (100.00%) | 2,592 / 2,601 (99.65%) | ☐ | Unique source-location union: lines 1,200/1,201, functions 72/72, regions 2,598/2,601 |
| [`heap/tests.rs`](../../../backend/bluejs/src/heap/tests.rs) | - | - | - | - | Test source; not a coverage target |
| [`intl.rs`](../../../backend/bluejs/src/intl.rs) | 124 / 124 (100.00%) | 19 / 19 (100.00%) | 188 / 188 (100.00%) | ☑ | Unique source-location union: lines 118/118, functions 19/19, regions 188/188 |
| [`lib.rs`](../../../backend/bluejs/src/lib.rs) | - | - | - | - | Declarations/re-exports only; no executable code |
| [`native.rs`](../../../backend/bluejs/src/native.rs) | 684 / 684 (100.00%) | 44 / 44 (100.00%) | 1,216 / 1,231 (98.78%) | ☐ | Unique source-location union: lines 673/673, functions 44/44, regions 1,231/1,231 |
| [`page_runtime.rs`](../../../backend/bluejs/src/page_runtime.rs) | 2,623 / 2,648 (99.06%) | 149 / 150 (99.33%) | 3,717 / 3,789 (98.10%) | ☐ | Unique source-location union: lines 2,594/2,616, functions 149/150, regions 3,721/3,789 |
| [`parser.rs`](../../../backend/bluejs/src/parser.rs) | 568 / 568 (100.00%) | 77 / 77 (100.00%) | 779 / 779 (100.00%) | ☑ | Unique source-location union: lines 557/557, functions 77/77, regions 779/779 |
| [`parser/expressions.rs`](../../../backend/bluejs/src/parser/expressions.rs) | 946 / 946 (100.00%) | 41 / 41 (100.00%) | 1,670 / 1,670 (100.00%) | ☑ | Unique source-location union: lines 942/942, functions 41/41, regions 1,670/1,670 |
| [`parser/functions.rs`](../../../backend/bluejs/src/parser/functions.rs) | 666 / 666 (100.00%) | 41 / 41 (100.00%) | 1,048 / 1,048 (100.00%) | ☑ | Unique source-location union: lines 653/653, functions 41/41, regions 1,048/1,048 |
| [`parser/module.rs`](../../../backend/bluejs/src/parser/module.rs) | 143 / 143 (100.00%) | 8 / 8 (100.00%) | 243 / 243 (100.00%) | ☑ | Unique source-location union: lines 138/138, functions 8/8, regions 243/243 |
| [`parser/module_items.rs`](../../../backend/bluejs/src/parser/module_items.rs) | 339 / 339 (100.00%) | 13 / 13 (100.00%) | 635 / 635 (100.00%) | ☑ | Unique source-location union: lines 334/334, functions 13/13, regions 635/635 |
| [`parser/patterns.rs`](../../../backend/bluejs/src/parser/patterns.rs) | 175 / 175 (100.00%) | 10 / 10 (100.00%) | 279 / 279 (100.00%) | ☑ | Unique source-location union: lines 173/173, functions 10/10, regions 279/279 |
| [`parser/statements.rs`](../../../backend/bluejs/src/parser/statements.rs) | 589 / 616 (95.62%) | 21 / 21 (100.00%) | 1,137 / 1,177 (96.60%) | ☐ | Unique source-location union: lines 587/614, functions 21/21, regions 1,137/1,177 |
| [`parser/tests.rs`](../../../backend/bluejs/src/parser/tests.rs) | - | - | - | - | Test source; not a coverage target |
| [`primitive.rs`](../../../backend/bluejs/src/primitive.rs) | 277 / 277 (100.00%) | 25 / 25 (100.00%) | 518 / 518 (100.00%) | ☑ | Unique source-location union: lines 268/268, functions 25/25, regions 518/518 |
| [`program_abi.rs`](../../../backend/bluejs/src/program_abi.rs) | 29 / 29 (100.00%) | 4 / 4 (100.00%) | 53 / 53 (100.00%) | ☑ | Unique source-location union: lines 29/29, functions 4/4, regions 53/53 |
| [`program_debug.rs`](../../../backend/bluejs/src/program_debug.rs) | 347 / 347 (100.00%) | 43 / 43 (100.00%) | 398 / 398 (100.00%) | ☑ | Unique source-location union: lines 343/343, functions 43/43, regions 398/398 |
| [`program_debug/ast_ids.rs`](../../../backend/bluejs/src/program_debug/ast_ids.rs) | 484 / 484 (100.00%) | 31 / 31 (100.00%) | 703 / 703 (100.00%) | ☑ | Unique source-location union: lines 478/478, functions 31/31, regions 703/703 |
| [`property.rs`](../../../backend/bluejs/src/property.rs) | 86 / 86 (100.00%) | 21 / 21 (100.00%) | 111 / 111 (100.00%) | ☑ | Unique source-location union: lines 84/84, functions 21/21, regions 111/111 |
| [`regex_backrefs.rs`](../../../backend/bluejs/src/regex_backrefs.rs) | 98 / 98 (100.00%) | 12 / 12 (100.00%) | 186 / 186 (100.00%) | ☑ | Unique source-location union: lines 94/94, functions 12/12, regions 186/186 |
| [`regex_canonicalize.rs`](../../../backend/bluejs/src/regex_canonicalize.rs) | 636 / 636 (100.00%) | 66 / 66 (100.00%) | 1,354 / 1,354 (100.00%) | ☑ | Unique source-location union: lines 612/612, functions 66/66, regions 1,354/1,354 |
| [`regex_escapes.rs`](../../../backend/bluejs/src/regex_escapes.rs) | 228 / 228 (100.00%) | 25 / 25 (100.00%) | 484 / 484 (100.00%) | ☑ | Unique source-location union: lines 217/217, functions 25/25, regions 484/484 |
| [`regex_group_names.rs`](../../../backend/bluejs/src/regex_group_names.rs) | 323 / 323 (100.00%) | 39 / 39 (100.00%) | 600 / 600 (100.00%) | ☑ | Unique source-location union: lines 302/302, functions 39/39, regions 600/600 |
| [`regex_worker.rs`](../../../backend/bluejs/src/regex_worker.rs) | 642 / 643 (99.84%) | 76 / 76 (100.00%) | 951 / 955 (99.58%) | ☐ | Unique source-location union: lines 606/607, functions 76/76, regions 953/955 |
| [`regexp.rs`](../../../backend/bluejs/src/regexp.rs) | 304 / 304 (100.00%) | 27 / 27 (100.00%) | 468 / 468 (100.00%) | ☑ | Unique source-location union: lines 292/292, functions 27/27, regions 468/468 |
| [`source_encoding.rs`](../../../backend/bluejs/src/source_encoding.rs) | 153 / 153 (100.00%) | 20 / 20 (100.00%) | 295 / 295 (100.00%) | ☑ | Unique source-location union: lines 151/151, functions 20/20, regions 295/295 |
| [`string.rs`](../../../backend/bluejs/src/string.rs) | 107 / 107 (100.00%) | 23 / 23 (100.00%) | 174 / 174 (100.00%) | ☑ | Unique source-location union: lines 105/105, functions 23/23, regions 174/174 |
| [`token.rs`](../../../backend/bluejs/src/token.rs) | 1,381 / 1,381 (100.00%) | 115 / 115 (100.00%) | 2,200 / 2,200 (100.00%) | ☑ | Unique source-location union: lines 1,350/1,350, functions 115/115, regions 2,200/2,200 |
| [`value.rs`](../../../backend/bluejs/src/value.rs) | 12 / 12 (100.00%) | 2 / 2 (100.00%) | 20 / 20 (100.00%) | ☑ | Unique source-location union: lines 12/12, functions 2/2, regions 20/20 |
| [`vm.rs`](../../../backend/bluejs/src/vm.rs) | 599 / 599 (100.00%) | 57 / 57 (100.00%) | 883 / 907 (97.35%) | ☐ | Unique source-location union: lines 568/568, functions 57/57, regions 885/907 |
| [`vm/builtins.rs`](../../../backend/bluejs/src/vm/builtins.rs) | 320 / 320 (100.00%) | 20 / 20 (100.00%) | 587 / 587 (100.00%) | ☑ | Unique source-location union: lines 311/311, functions 20/20, regions 587/587 |
| [`vm/builtins/arguments.rs`](../../../backend/bluejs/src/vm/builtins/arguments.rs) | 779 / 779 (100.00%) | 72 / 72 (100.00%) | 1,279 / 1,279 (100.00%) | ☑ | Unique source-location union: lines 729/729, functions 72/72, regions 1,279/1,279 |
| [`vm/builtins/array_change_by_copy.rs`](../../../backend/bluejs/src/vm/builtins/array_change_by_copy.rs) | 197 / 197 (100.00%) | 14 / 14 (100.00%) | 454 / 456 (99.56%) | ☐ | Unique source-location union: lines 190/190, functions 14/14, regions 456/456 |
| [`vm/builtins/array_from_async.rs`](../../../backend/bluejs/src/vm/builtins/array_from_async.rs) | 329 / 338 (97.34%) | 25 / 26 (96.15%) | 836 / 891 (93.83%) | ☐ | Unique source-location union: lines 322/330, functions 25/26, regions 836/891 |
| [`vm/builtins/array_scan.rs`](../../../backend/bluejs/src/vm/builtins/array_scan.rs) | 123 / 123 (100.00%) | 13 / 13 (100.00%) | 185 / 185 (100.00%) | ☑ | Unique source-location union: lines 120/120, functions 13/13, regions 185/185 |
| [`vm/builtins/arrays.rs`](../../../backend/bluejs/src/vm/builtins/arrays.rs) | 1,331 / 1,331 (100.00%) | 80 / 80 (100.00%) | 2,882 / 2,894 (99.59%) | ☐ | Unique source-location union: lines 1,290/1,290, functions 80/80, regions 2,882/2,894 |
| [`vm/builtins/binary_data.rs`](../../../backend/bluejs/src/vm/builtins/binary_data.rs) | 1,516 / 1,561 (97.12%) | 122 / 131 (93.13%) | 2,633 / 2,798 (94.10%) | ☐ | Unique source-location union: lines 1,430/1,463, functions 122/131, regions 2,633/2,798 |
| [`vm/builtins/collection_iteration.rs`](../../../backend/bluejs/src/vm/builtins/collection_iteration.rs) | 116 / 116 (100.00%) | 8 / 8 (100.00%) | 196 / 200 (98.00%) | ☐ | Unique source-location union: lines 111/111, functions 8/8, regions 196/200 |
| [`vm/builtins/collections.rs`](../../../backend/bluejs/src/vm/builtins/collections.rs) | 372 / 372 (100.00%) | 27 / 27 (100.00%) | 750 / 755 (99.34%) | ☐ | Unique source-location union: lines 361/361, functions 27/27, regions 750/755 |
| [`vm/builtins/decorators.rs`](../../../backend/bluejs/src/vm/builtins/decorators.rs) | 558 / 558 (100.00%) | 43 / 43 (100.00%) | 1,121 / 1,121 (100.00%) | ☑ | Unique source-location union: lines 544/544, functions 43/43, regions 1,121/1,121 |
| [`vm/builtins/dynamic.rs`](../../../backend/bluejs/src/vm/builtins/dynamic.rs) | 627 / 645 (97.21%) | 37 / 37 (100.00%) | 1,196 / 1,260 (94.92%) | ☐ | Unique source-location union: lines 600/617, functions 37/37, regions 1,196/1,260 |
| [`vm/builtins/execution.rs`](../../../backend/bluejs/src/vm/builtins/execution.rs) | - | - | - | - | Declarations/re-exports only; no executable code |
| [`vm/builtins/execution/closures.rs`](../../../backend/bluejs/src/vm/builtins/execution/closures.rs) | 447 / 511 (87.48%) | 23 / 23 (100.00%) | 774 / 873 (88.66%) | ☐ | Unique source-location union: lines 480/496, functions 23/23, regions 839/873 |
| [`vm/builtins/execution/runtime.rs`](../../../backend/bluejs/src/vm/builtins/execution/runtime.rs) | 1,538 / 1,572 (97.84%) | 142 / 143 (99.30%) | 2,982 / 3,118 (95.64%) | ☐ | Unique source-location union: lines 1,446/1,476, functions 142/143, regions 2,982/3,118 |
| [`vm/builtins/execution/setup.rs`](../../../backend/bluejs/src/vm/builtins/execution/setup.rs) | 1,090 / 1,092 (99.82%) | 92 / 92 (100.00%) | 1,777 / 1,790 (99.27%) | ☐ | Unique source-location union: lines 1,018/1,019, functions 92/92, regions 1,777/1,790 |
| [`vm/builtins/general.rs`](../../../backend/bluejs/src/vm/builtins/general.rs) | 424 / 425 (99.76%) | 37 / 37 (100.00%) | 739 / 749 (98.66%) | ☐ | Unique source-location union: lines 413/414, functions 37/37, regions 739/749 |
| [`vm/builtins/generators.rs`](../../../backend/bluejs/src/vm/builtins/generators.rs) | 1,356 / 1,483 (91.44%) | 66 / 76 (86.84%) | 2,143 / 2,372 (90.35%) | ☐ | Unique source-location union: lines 1,318/1,432, functions 66/76, regions 2,143/2,372 |
| [`vm/builtins/globals.rs`](../../../backend/bluejs/src/vm/builtins/globals.rs) | 976 / 985 (99.09%) | 12 / 12 (100.00%) | 1,446 / 1,465 (98.70%) | ☐ | Unique source-location union: lines 963/971, functions 12/12, regions 1,446/1,465 |
| [`vm/builtins/immutable_arraybuffer.rs`](../../../backend/bluejs/src/vm/builtins/immutable_arraybuffer.rs) | 160 / 160 (100.00%) | 11 / 11 (100.00%) | 244 / 244 (100.00%) | ☑ | Unique source-location union: lines 155/155, functions 11/11, regions 244/244 |
| [`vm/builtins/math.rs`](../../../backend/bluejs/src/vm/builtins/math.rs) | 439 / 439 (100.00%) | 27 / 27 (100.00%) | 743 / 743 (100.00%) | ☑ | Unique source-location union: lines 427/427, functions 27/27, regions 743/743 |
| [`vm/builtins/native_dispatch.rs`](../../../backend/bluejs/src/vm/builtins/native_dispatch.rs) | - | - | - | - | Declarations/re-exports only; no executable code |
| [`vm/builtins/native_dispatch/date.rs`](../../../backend/bluejs/src/vm/builtins/native_dispatch/date.rs) | 687 / 687 (100.00%) | 55 / 55 (100.00%) | 1,230 / 1,232 (99.84%) | ☐ | Unique source-location union: lines 661/661, functions 55/55, regions 1,230/1,232 |
| [`vm/builtins/native_dispatch/dispatch.rs`](../../../backend/bluejs/src/vm/builtins/native_dispatch/dispatch.rs) | 1,235 / 1,261 (97.94%) | 35 / 37 (94.59%) | 3,043 / 3,150 (96.60%) | ☐ | Unique source-location union: lines 1,193/1,212, functions 35/37, regions 3,062/3,150 |
| [`vm/builtins/numbers.rs`](../../../backend/bluejs/src/vm/builtins/numbers.rs) | 244 / 246 (99.19%) | 15 / 15 (100.00%) | 424 / 431 (98.38%) | ☐ | Unique source-location union: lines 241/243, functions 15/15, regions 425/431 |
| [`vm/builtins/numbers/tests.rs`](../../../backend/bluejs/src/vm/builtins/numbers/tests.rs) | - | - | - | - | Test source; not a coverage target |
| [`vm/builtins/object.rs`](../../../backend/bluejs/src/vm/builtins/object.rs) | 1,263 / 1,304 (96.86%) | 83 / 88 (94.32%) | 2,215 / 2,461 (90.00%) | ☐ | Unique source-location union: lines 1,211/1,245, functions 83/88, regions 2,222/2,461 |
| [`vm/builtins/promise_combinators.rs`](../../../backend/bluejs/src/vm/builtins/promise_combinators.rs) | 415 / 415 (100.00%) | 34 / 34 (100.00%) | 854 / 854 (100.00%) | ☑ | Unique source-location union: lines 403/403, functions 34/34, regions 854/854 |
| [`vm/builtins/promise_core.rs`](../../../backend/bluejs/src/vm/builtins/promise_core.rs) | 799 / 801 (99.75%) | 67 / 67 (100.00%) | 1,329 / 1,339 (99.25%) | ☐ | Unique source-location union: lines 771/773, functions 67/67, regions 1,329/1,339 |
| [`vm/builtins/promises.rs`](../../../backend/bluejs/src/vm/builtins/promises.rs) | 927 / 939 (98.72%) | 59 / 59 (100.00%) | 1,568 / 1,638 (95.73%) | ☐ | Unique source-location union: lines 884/893, functions 59/59, regions 1,569/1,638 |
| [`vm/builtins/resource_management.rs`](../../../backend/bluejs/src/vm/builtins/resource_management.rs) | 546 / 558 (97.85%) | 32 / 34 (94.12%) | 806 / 829 (97.23%) | ☐ | Unique source-location union: lines 537/547, functions 32/34, regions 806/829 |
| [`vm/builtins/set_methods.rs`](../../../backend/bluejs/src/vm/builtins/set_methods.rs) | 314 / 314 (100.00%) | 33 / 33 (100.00%) | 627 / 629 (99.68%) | ☐ | Unique source-location union: lines 297/297, functions 33/33, regions 627/629 |
| [`vm/builtins/tests.rs`](../../../backend/bluejs/src/vm/builtins/tests.rs) | - | - | - | - | Test source; not a coverage target |
| [`vm/builtins/typed_arrays.rs`](../../../backend/bluejs/src/vm/builtins/typed_arrays.rs) | 854 / 860 (99.30%) | 41 / 42 (97.62%) | 1,665 / 1,733 (96.08%) | ☐ | Unique source-location union: lines 836/840, functions 41/42, regions 1,665/1,733 |
| [`vm/builtins/uint8array.rs`](../../../backend/bluejs/src/vm/builtins/uint8array.rs) | 493 / 508 (97.05%) | 34 / 35 (97.14%) | 743 / 768 (96.74%) | ☐ | Unique source-location union: lines 481/494, functions 34/35, regions 743/768 |
| [`vm/completion.rs`](../../../backend/bluejs/src/vm/completion.rs) | 113 / 113 (100.00%) | 9 / 9 (100.00%) | 162 / 162 (100.00%) | ☑ | Unique source-location union: lines 111/111, functions 9/9, regions 162/162 |
| [`vm/debugger.rs`](../../../backend/bluejs/src/vm/debugger.rs) | 674 / 798 (84.46%) | 34 / 42 (80.95%) | 826 / 972 (84.98%) | ☐ | Unique source-location union: lines 666/782, functions 34/42, regions 826/972 |
| [`vm/debugger/inspection.rs`](../../../backend/bluejs/src/vm/debugger/inspection.rs) | 189 / 191 (98.95%) | 6 / 6 (100.00%) | 182 / 192 (94.79%) | ☐ | Unique source-location union: lines 189/191, functions 6/6, regions 182/192 |
| [`vm/debugger/tests.rs`](../../../backend/bluejs/src/vm/debugger/tests.rs) | - | - | - | - | Test source; not a coverage target |
| [`vm/debugger/tests/nested_modules.rs`](../../../backend/bluejs/src/vm/debugger/tests/nested_modules.rs) | - | - | - | - | Test source; not a coverage target |
| [`vm/errors.rs`](../../../backend/bluejs/src/vm/errors.rs) | 369 / 389 (94.86%) | 21 / 24 (87.50%) | 688 / 764 (90.05%) | ☐ | Unique source-location union: lines 360/376, functions 21/24, regions 688/764 |
| [`vm/execution.rs`](../../../backend/bluejs/src/vm/execution.rs) | 1,419 / 1,522 (93.23%) | 133 / 139 (95.68%) | 2,322 / 2,568 (90.42%) | ☐ | Unique source-location union: lines 1,349/1,439, functions 133/139, regions 2,331/2,568 |
| [`vm/functions.rs`](../../../backend/bluejs/src/vm/functions.rs) | 434 / 434 (100.00%) | 46 / 46 (100.00%) | 550 / 557 (98.74%) | ☐ | Unique source-location union: lines 427/427, functions 46/46, regions 551/557 |
| [`vm/host_objects.rs`](../../../backend/bluejs/src/vm/host_objects.rs) | 525 / 620 (84.68%) | 40 / 58 (68.97%) | 597 / 713 (83.73%) | ☐ | Unique source-location union: lines 511/582, functions 40/58, regions 599/713 |
| [`vm/interpreter.rs`](../../../backend/bluejs/src/vm/interpreter.rs) | 1,309 / 1,391 (94.10%) | 60 / 63 (95.24%) | 3,022 / 3,225 (93.71%) | ☐ | Unique source-location union: lines 1,257/1,316, functions 60/63, regions 3,039/3,225 |
| [`vm/intl.rs`](../../../backend/bluejs/src/vm/intl.rs) | 585 / 589 (99.32%) | 17 / 17 (100.00%) | 785 / 798 (98.37%) | ☐ | Unique source-location union: lines 576/580, functions 17/17, regions 785/798 |
| [`vm/intl/collator_locale.rs`](../../../backend/bluejs/src/vm/intl/collator_locale.rs) | 450 / 450 (100.00%) | 41 / 41 (100.00%) | 717 / 717 (100.00%) | ☑ | Unique source-location union: lines 422/422, functions 41/41, regions 717/717 |
| [`vm/intl/date_time.rs`](../../../backend/bluejs/src/vm/intl/date_time.rs) | 794 / 840 (94.52%) | 53 / 56 (94.64%) | 1,213 / 1,275 (95.14%) | ☐ | Unique source-location union: lines 770/811, functions 53/56, regions 1,213/1,275 |
| [`vm/intl/list_duration.rs`](../../../backend/bluejs/src/vm/intl/list_duration.rs) | 593 / 618 (95.95%) | 47 / 55 (85.45%) | 891 / 957 (93.10%) | ☐ | Unique source-location union: lines 569/584, functions 47/55, regions 891/957 |
| [`vm/intl/number_options.rs`](../../../backend/bluejs/src/vm/intl/number_options.rs) | 446 / 446 (100.00%) | 32 / 32 (100.00%) | 632 / 633 (99.84%) | ☐ | Unique source-location union: lines 433/433, functions 32/32, regions 632/633 |
| [`vm/intl/number_runtime.rs`](../../../backend/bluejs/src/vm/intl/number_runtime.rs) | 444 / 448 (99.11%) | 26 / 26 (100.00%) | 585 / 590 (99.15%) | ☐ | Unique source-location union: lines 432/436, functions 26/26, regions 585/590 |
| [`vm/intl/plural_segmenter.rs`](../../../backend/bluejs/src/vm/intl/plural_segmenter.rs) | 585 / 585 (100.00%) | 45 / 45 (100.00%) | 843 / 843 (100.00%) | ☑ | Unique source-location union: lines 564/564, functions 45/45, regions 843/843 |
| [`vm/intl/shared.rs`](../../../backend/bluejs/src/vm/intl/shared.rs) | 723 / 723 (100.00%) | 65 / 65 (100.00%) | 1,165 / 1,165 (100.00%) | ☑ | Unique source-location union: lines 688/688, functions 65/65, regions 1,165/1,165 |
| [`vm/intrinsics.rs`](../../../backend/bluejs/src/vm/intrinsics.rs) | 443 / 443 (100.00%) | 5 / 5 (100.00%) | 589 / 594 (99.16%) | ☐ | Unique source-location union: lines 436/436, functions 5/5, regions 589/594 |
| [`vm/json.rs`](../../../backend/bluejs/src/vm/json.rs) | 805 / 818 (98.41%) | 61 / 61 (100.00%) | 1,606 / 1,649 (97.39%) | ☐ | Unique source-location union: lines 776/789, functions 61/61, regions 1,606/1,649 |
| [`vm/lifecycle.rs`](../../../backend/bluejs/src/vm/lifecycle.rs) | 211 / 211 (100.00%) | 13 / 13 (100.00%) | 226 / 226 (100.00%) | ☑ | Unique source-location union: lines 211/211, functions 13/13, regions 226/226 |
| [`vm/modules.rs`](../../../backend/bluejs/src/vm/modules.rs) | 2,071 / 2,093 (98.95%) | 117 / 117 (100.00%) | 3,134 / 3,179 (98.58%) | ☐ | Unique source-location union: lines 2,017/2,036, functions 117/117, regions 3,145/3,179 |
| [`vm/modules/deferred.rs`](../../../backend/bluejs/src/vm/modules/deferred.rs) | 297 / 307 (96.74%) | 27 / 28 (96.43%) | 456 / 489 (93.25%) | ☐ | Unique source-location union: lines 282/289, functions 27/28, regions 456/489 |
| [`vm/modules/namespace.rs`](../../../backend/bluejs/src/vm/modules/namespace.rs) | 203 / 224 (90.62%) | 14 / 17 (82.35%) | 330 / 365 (90.41%) | ☐ | Unique source-location union: lines 191/206, functions 14/17, regions 330/365 |
| [`vm/modules/synthetic.rs`](../../../backend/bluejs/src/vm/modules/synthetic.rs) | 211 / 211 (100.00%) | 12 / 12 (100.00%) | 317 / 317 (100.00%) | ☑ | Unique source-location union: lines 209/209, functions 12/12, regions 317/317 |
| [`vm/native_stack.rs`](../../../backend/bluejs/src/vm/native_stack.rs) | 97 / 97 (100.00%) | 19 / 19 (100.00%) | 160 / 160 (100.00%) | ☑ | Unique source-location union: lines 89/89, functions 19/19, regions 160/160 |
| [`vm/operations.rs`](../../../backend/bluejs/src/vm/operations.rs) | 732 / 732 (100.00%) | 58 / 58 (100.00%) | 1,341 / 1,341 (100.00%) | ☑ | Unique source-location union: lines 714/714, functions 58/58, regions 1,341/1,341 |
| [`vm/properties.rs`](../../../backend/bluejs/src/vm/properties.rs) | 668 / 705 (94.75%) | 65 / 70 (92.86%) | 1,206 / 1,295 (93.13%) | ☐ | Unique source-location union: lines 630/657, functions 65/70, regions 1,206/1,295 |
| [`vm/realm_reentrancy.rs`](../../../backend/bluejs/src/vm/realm_reentrancy.rs) | 27 / 27 (100.00%) | 8 / 8 (100.00%) | 34 / 34 (100.00%) | ☑ | Unique source-location union: lines 22/22, functions 8/8, regions 34/34 |
| [`vm/regexp.rs`](../../../backend/bluejs/src/vm/regexp.rs) | 1,061 / 1,097 (96.72%) | 60 / 60 (100.00%) | 1,979 / 2,088 (94.78%) | ☐ | Unique source-location union: lines 1,023/1,054, functions 60/60, regions 1,979/2,088 |
| [`vm/shadow_realm.rs`](../../../backend/bluejs/src/vm/shadow_realm.rs) | 335 / 380 (88.16%) | 25 / 28 (89.29%) | 562 / 630 (89.21%) | ☐ | Unique source-location union: lines 322/362, functions 25/28, regions 562/630 |
| [`vm/temporal.rs`](../../../backend/bluejs/src/vm/temporal.rs) | 562 / 569 (98.77%) | 11 / 11 (100.00%) | 551 / 567 (97.18%) | ☐ | Unique source-location union: lines 557/564, functions 11/11, regions 551/567 |
| [`vm/temporal/calendar.rs`](../../../backend/bluejs/src/vm/temporal/calendar.rs) | 147 / 147 (100.00%) | 14 / 14 (100.00%) | 215 / 215 (100.00%) | ☑ | Unique source-location union: lines 147/147, functions 14/14, regions 215/215 |
| [`vm/temporal/conversion.rs`](../../../backend/bluejs/src/vm/temporal/conversion.rs) | - | - | - | - | Declarations/re-exports only; no executable code |
| [`vm/temporal/conversion/calendar_fields.rs`](../../../backend/bluejs/src/vm/temporal/conversion/calendar_fields.rs) | 305 / 313 (97.44%) | 25 / 27 (92.59%) | 475 / 485 (97.94%) | ☐ | Unique source-location union: lines 286/290, functions 25/27, regions 475/485 |
| [`vm/temporal/conversion/from_value.rs`](../../../backend/bluejs/src/vm/temporal/conversion/from_value.rs) | 354 / 368 (96.20%) | 21 / 21 (100.00%) | 578 / 600 (96.33%) | ☐ | Unique source-location union: lines 337/351, functions 21/21, regions 578/600 |
| [`vm/temporal/conversion/getters.rs`](../../../backend/bluejs/src/vm/temporal/conversion/getters.rs) | 114 / 114 (100.00%) | 4 / 4 (100.00%) | 151 / 151 (100.00%) | ☑ | Unique source-location union: lines 110/110, functions 4/4, regions 151/151 |
| [`vm/temporal/conversion/numeric.rs`](../../../backend/bluejs/src/vm/temporal/conversion/numeric.rs) | 195 / 195 (100.00%) | 24 / 24 (100.00%) | 251 / 251 (100.00%) | ☑ | Unique source-location union: lines 183/183, functions 24/24, regions 251/251 |
| [`vm/temporal/conversion/options.rs`](../../../backend/bluejs/src/vm/temporal/conversion/options.rs) | 130 / 130 (100.00%) | 12 / 12 (100.00%) | 158 / 158 (100.00%) | ☑ | Unique source-location union: lines 125/125, functions 12/12, regions 158/158 |
| [`vm/temporal/conversion/zoned_conversion.rs`](../../../backend/bluejs/src/vm/temporal/conversion/zoned_conversion.rs) | 107 / 131 (81.68%) | 6 / 11 (54.55%) | 182 / 206 (88.35%) | ☐ | Unique source-location union: lines 103/118, functions 6/11, regions 182/206 |
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
| [`vm/temporal/duration_relative.rs`](../../../backend/bluejs/src/vm/temporal/duration_relative.rs) | 519 / 521 (99.62%) | 41 / 42 (97.62%) | 654 / 659 (99.24%) | ☐ | Unique source-location union: lines 489/490, functions 41/42, regions 654/659 |
| [`vm/temporal/epoch.rs`](../../../backend/bluejs/src/vm/temporal/epoch.rs) | 136 / 136 (100.00%) | 13 / 13 (100.00%) | 237 / 237 (100.00%) | ☑ | Unique source-location union: lines 136/136, functions 13/13, regions 237/237 |
| [`vm/temporal/instant.rs`](../../../backend/bluejs/src/vm/temporal/instant.rs) | 449 / 462 (97.19%) | 25 / 32 (78.12%) | 635 / 667 (95.20%) | ☐ | Unique source-location union: lines 438/443, functions 25/32, regions 635/667 |
| [`vm/temporal/iso.rs`](../../../backend/bluejs/src/vm/temporal/iso.rs) | 17 / 17 (100.00%) | 3 / 3 (100.00%) | 24 / 24 (100.00%) | ☑ | Unique source-location union: lines 17/17, functions 3/3, regions 24/24 |
| [`vm/temporal/iso/annotations.rs`](../../../backend/bluejs/src/vm/temporal/iso/annotations.rs) | 271 / 271 (100.00%) | 28 / 28 (100.00%) | 452 / 452 (100.00%) | ☑ | Unique source-location union: lines 255/255, functions 28/28, regions 452/452 |
| [`vm/temporal/iso/datetime.rs`](../../../backend/bluejs/src/vm/temporal/iso/datetime.rs) | 145 / 145 (100.00%) | 15 / 15 (100.00%) | 259 / 259 (100.00%) | ☑ | Unique source-location union: lines 143/143, functions 15/15, regions 259/259 |
| [`vm/temporal/iso/duration.rs`](../../../backend/bluejs/src/vm/temporal/iso/duration.rs) | 117 / 117 (100.00%) | 5 / 5 (100.00%) | 183 / 183 (100.00%) | ☑ | Unique source-location union: lines 114/114, functions 5/5, regions 183/183 |
| [`vm/temporal/iso/offset.rs`](../../../backend/bluejs/src/vm/temporal/iso/offset.rs) | 182 / 182 (100.00%) | 12 / 12 (100.00%) | 349 / 349 (100.00%) | ☑ | Unique source-location union: lines 181/181, functions 12/12, regions 349/349 |
| [`vm/temporal/iso/scan.rs`](../../../backend/bluejs/src/vm/temporal/iso/scan.rs) | 349 / 349 (100.00%) | 38 / 38 (100.00%) | 628 / 628 (100.00%) | ☑ | Unique source-location union: lines 336/336, functions 38/38, regions 628/628 |
| [`vm/temporal/iso/tests.rs`](../../../backend/bluejs/src/vm/temporal/iso/tests.rs) | - | - | - | - | Test source; not a coverage target |
| [`vm/temporal/plain_date.rs`](../../../backend/bluejs/src/vm/temporal/plain_date.rs) | - | - | - | - | Declarations/re-exports only; no executable code |
| [`vm/temporal/plain_date/calendar_add.rs`](../../../backend/bluejs/src/vm/temporal/plain_date/calendar_add.rs) | 181 / 181 (100.00%) | 6 / 6 (100.00%) | 303 / 303 (100.00%) | ☑ | Unique source-location union: lines 180/180, functions 6/6, regions 303/303 |
| [`vm/temporal/plain_date/calendar_difference.rs`](../../../backend/bluejs/src/vm/temporal/plain_date/calendar_difference.rs) | 210 / 210 (100.00%) | 8 / 8 (100.00%) | 369 / 369 (100.00%) | ☑ | Unique source-location union: lines 208/208, functions 8/8, regions 369/369 |
| [`vm/temporal/plain_date/format.rs`](../../../backend/bluejs/src/vm/temporal/plain_date/format.rs) | 30 / 30 (100.00%) | 4 / 4 (100.00%) | 56 / 56 (100.00%) | ☑ | Unique source-location union: lines 30/30, functions 4/4, regions 56/56 |
| [`vm/temporal/plain_date/iso_date.rs`](../../../backend/bluejs/src/vm/temporal/plain_date/iso_date.rs) | 116 / 116 (100.00%) | 17 / 17 (100.00%) | 207 / 207 (100.00%) | ☑ | Unique source-location union: lines 115/115, functions 17/17, regions 207/207 |
| [`vm/temporal/plain_date/month_structure.rs`](../../../backend/bluejs/src/vm/temporal/plain_date/month_structure.rs) | 161 / 161 (100.00%) | 14 / 14 (100.00%) | 243 / 243 (100.00%) | ☑ | Unique source-location union: lines 161/161, functions 14/14, regions 243/243 |
| [`vm/temporal/plain_date/round_duration.rs`](../../../backend/bluejs/src/vm/temporal/plain_date/round_duration.rs) | 155 / 155 (100.00%) | 10 / 10 (100.00%) | 204 / 204 (100.00%) | ☑ | Unique source-location union: lines 151/151, functions 10/10, regions 204/204 |
| [`vm/temporal/plain_date/tests.rs`](../../../backend/bluejs/src/vm/temporal/plain_date/tests.rs) | - | - | - | - | Test source; not a coverage target |
| [`vm/temporal/plain_date_time_difference.rs`](../../../backend/bluejs/src/vm/temporal/plain_date_time_difference.rs) | 784 / 785 (99.87%) | 54 / 54 (100.00%) | 1,089 / 1,105 (98.55%) | ☐ | Unique source-location union: lines 775/776, functions 54/54, regions 1,090/1,105 |
| [`vm/temporal/plain_month_day.rs`](../../../backend/bluejs/src/vm/temporal/plain_month_day.rs) | 282 / 282 (100.00%) | 28 / 28 (100.00%) | 386 / 386 (100.00%) | ☑ | Unique source-location union: lines 279/279, functions 28/28, regions 386/386 |
| [`vm/temporal/plain_time.rs`](../../../backend/bluejs/src/vm/temporal/plain_time.rs) | 627 / 649 (96.61%) | 39 / 44 (88.64%) | 888 / 922 (96.31%) | ☐ | Unique source-location union: lines 612/626, functions 39/44, regions 888/922 |
| [`vm/temporal/plain_year_month.rs`](../../../backend/bluejs/src/vm/temporal/plain_year_month.rs) | 145 / 145 (100.00%) | 13 / 13 (100.00%) | 193 / 193 (100.00%) | ☑ | Unique source-location union: lines 143/143, functions 13/13, regions 193/193 |
| [`vm/temporal/receiver.rs`](../../../backend/bluejs/src/vm/temporal/receiver.rs) | 42 / 42 (100.00%) | 3 / 3 (100.00%) | 43 / 43 (100.00%) | ☑ | Unique source-location union: lines 41/41, functions 3/3, regions 43/43 |
| [`vm/temporal/rounding.rs`](../../../backend/bluejs/src/vm/temporal/rounding.rs) | 405 / 405 (100.00%) | 33 / 33 (100.00%) | 666 / 666 (100.00%) | ☑ | Unique source-location union: lines 405/405, functions 33/33, regions 666/666 |
| [`vm/temporal/time_zone.rs`](../../../backend/bluejs/src/vm/temporal/time_zone.rs) | 519 / 519 (100.00%) | 47 / 47 (100.00%) | 1,031 / 1,031 (100.00%) | ☑ | Unique source-location union: lines 510/510, functions 47/47, regions 1,031/1,031 |
| [`vm/temporal/time_zone_id.rs`](../../../backend/bluejs/src/vm/temporal/time_zone_id.rs) | 248 / 248 (100.00%) | 32 / 32 (100.00%) | 546 / 546 (100.00%) | ☑ | Unique source-location union: lines 238/238, functions 32/32, regions 546/546 |
| [`vm/temporal/year_month.rs`](../../../backend/bluejs/src/vm/temporal/year_month.rs) | 992 / 992 (100.00%) | 71 / 71 (100.00%) | 1,553 / 1,553 (100.00%) | ☑ | Unique source-location union: lines 940/940, functions 71/71, regions 1,553/1,553 |
| [`vm/temporal/zoned.rs`](../../../backend/bluejs/src/vm/temporal/zoned.rs) | - | - | - | - | Declarations/re-exports only; no executable code |
| [`vm/temporal/zoned/arithmetic.rs`](../../../backend/bluejs/src/vm/temporal/zoned/arithmetic.rs) | 177 / 177 (100.00%) | 8 / 8 (100.00%) | 280 / 280 (100.00%) | ☑ | Unique source-location union: lines 168/168, functions 8/8, regions 280/280 |
| [`vm/temporal/zoned/conversions.rs`](../../../backend/bluejs/src/vm/temporal/zoned/conversions.rs) | 121 / 121 (100.00%) | 8 / 8 (100.00%) | 160 / 160 (100.00%) | ☑ | Unique source-location union: lines 120/120, functions 8/8, regions 160/160 |
| [`vm/temporal/zoned/formatting.rs`](../../../backend/bluejs/src/vm/temporal/zoned/formatting.rs) | 170 / 170 (100.00%) | 8 / 8 (100.00%) | 264 / 264 (100.00%) | ☑ | Unique source-location union: lines 167/167, functions 8/8, regions 264/264 |
| [`vm/temporal/zoned/from_value.rs`](../../../backend/bluejs/src/vm/temporal/zoned/from_value.rs) | 190 / 190 (100.00%) | 13 / 13 (100.00%) | 250 / 250 (100.00%) | ☑ | Unique source-location union: lines 179/179, functions 13/13, regions 250/250 |
| [`vm/temporal/zoned/resolution.rs`](../../../backend/bluejs/src/vm/temporal/zoned/resolution.rs) | 112 / 112 (100.00%) | 8 / 8 (100.00%) | 154 / 154 (100.00%) | ☑ | Unique source-location union: lines 112/112, functions 8/8, regions 154/154 |
| [`vm/temporal/zoned/since_until.rs`](../../../backend/bluejs/src/vm/temporal/zoned/since_until.rs) | 146 / 146 (100.00%) | 8 / 8 (100.00%) | 212 / 212 (100.00%) | ☑ | Unique source-location union: lines 142/142, functions 8/8, regions 212/212 |
| [`vm/temporal/zoned/with.rs`](../../../backend/bluejs/src/vm/temporal/zoned/with.rs) | 215 / 215 (100.00%) | 9 / 9 (100.00%) | 371 / 371 (100.00%) | ☑ | Unique source-location union: lines 209/209, functions 9/9, regions 371/371 |
| [`vm/temporal/zoned_date_time.rs`](../../../backend/bluejs/src/vm/temporal/zoned_date_time.rs) | 245 / 245 (100.00%) | 17 / 17 (100.00%) | 405 / 405 (100.00%) | ☑ | Unique source-location union: lines 243/243, functions 17/17, regions 405/405 |
| [`vm/temporal/zoned_difference.rs`](../../../backend/bluejs/src/vm/temporal/zoned_difference.rs) | 767 / 771 (99.48%) | 43 / 43 (100.00%) | 1,056 / 1,090 (96.88%) | ☐ | Unique source-location union: lines 763/767, functions 43/43, regions 1,056/1,090 |
| [`vm/test262.rs`](../../../backend/bluejs/src/vm/test262.rs) | 65 / 65 (100.00%) | 7 / 7 (100.00%) | 107 / 107 (100.00%) | ☑ | Unique source-location union: lines 62/62, functions 7/7, regions 107/107 |
| [`vm/test262/assertions.rs`](../../../backend/bluejs/src/vm/test262/assertions.rs) | 297 / 299 (99.33%) | 17 / 17 (100.00%) | 825 / 838 (98.45%) | ☐ | Unique source-location union: lines 295/297, functions 17/17, regions 825/838 |
| [`vm/test262/cases.rs`](../../../backend/bluejs/src/vm/test262/cases.rs) | 817 / 817 (100.00%) | 59 / 59 (100.00%) | 1,381 / 1,384 (99.78%) | ☐ | Unique source-location union: lines 779/779, functions 59/59, regions 1,381/1,384 |
| [`vm/test262/foreign.rs`](../../../backend/bluejs/src/vm/test262/foreign.rs) | 1,479 / 1,581 (93.55%) | 128 / 143 (89.51%) | 2,345 / 2,640 (88.83%) | ☐ | Unique source-location union: lines 1,396/1,474, functions 128/143, regions 2,345/2,640 |
| [`vm/test262/harness.rs`](../../../backend/bluejs/src/vm/test262/harness.rs) | 282 / 283 (99.65%) | 12 / 12 (100.00%) | 416 / 417 (99.76%) | ☐ | Unique source-location union: lines 274/274, functions 12/12, regions 416/417 |
| [`vm/test262/reverse.rs`](../../../backend/bluejs/src/vm/test262/reverse.rs) | 600 / 600 (100.00%) | 47 / 47 (100.00%) | 825 / 825 (100.00%) | ☑ | Unique source-location union: lines 578/578, functions 47/47, regions 825/825 |
| [`vm/test262_agents.rs`](../../../backend/bluejs/src/vm/test262_agents.rs) | 462 / 481 (96.05%) | 45 / 51 (88.24%) | 645 / 687 (93.89%) | ☐ | Unique source-location union: lines 452/461, functions 45/51, regions 647/687 |
| [`vm/tests.rs`](../../../backend/bluejs/src/vm/tests.rs) | - | - | - | - | Test source; not a coverage target |
| **Total (162 instrumented files)** | **80,678 / 82,070 (98.30%)** | **5,823 / 5,958 (97.73%)** | **135,084 / 138,580 (97.48%)** | ☐ |  |

Raw LLVM lines, functions, and regions are all complete in **95 of 162** instrumented files; **67** remain incomplete.

## Historical differences from the other platforms (2026-09-21)

- **Ubuntu**: 2 of 102,926 modes differ (1 file): `staging/sm/Math/acosh-approx.js` (this platform: pass; Ubuntu: fail).
- **Windows**: 2 of 102,926 modes differ (1 file): `staging/sm/Math/acosh-approx.js` (this platform: pass; Windows: fail).

## Reproduce the current Test262 inventory

Use a verified copy of the pinned snapshot. For a fresh corpus destination, add `--fetch` to the runner command below; omit it when that destination already contains the verified snapshot. The existing reference checkout is not overwritten.

```sh
python3.14 -m venv /tmp/bluejs-conformance-venv
/tmp/bluejs-conformance-venv/bin/pip install -r backend/bluejs/test262/requirements.txt
cargo build -p blueice-bluejs --bins --offline
/tmp/bluejs-conformance-venv/bin/python -m unittest discover -s backend/bluejs/test262 -v
/tmp/bluejs-conformance-venv/bin/python backend/bluejs/test262/run.py --corpus /tmp/blueice-test262-72faf8ec-20261001 --jobs 8 --output target/test262-macos-20261001-crash-fix --progress-interval 60
/tmp/bluejs-conformance-venv/bin/python backend/bluejs/test262/analyze.py --run target/test262-macos-20261001-crash-fix --corpus /tmp/blueice-test262-72faf8ec-20261001 --output target/test262-macos-20261001-crash-fix-analysis
```

The runner validates the pinned corpus marker, manifest hash, and every corpus file before execution. It returns 1 for this run because the 1 `stale_corpus` and 4 `excluded` modes remain non-pass statuses; inspect `summary.json` and use `analyze.py` to reconcile all records. Keep the host otherwise idle during this inventory because ordinary cases have a two-second wall deadline.
