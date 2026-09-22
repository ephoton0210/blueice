# Ubuntu Test262 Report

## Current complete inventory (2026-09-22)

The test host is **Ubuntu 24.04.4 LTS under WSL2** (kernel `6.6.87.2-microsoft-standard-WSL2`) on an x86_64 host with 16 logical CPUs and 30 GB of RAM, `x86_64-unknown-linux-gnu`, with Rust/Cargo 1.95.0 and Python 3.12.3 (PyYAML 6.0.1). This is a WSL2 host again, unlike the native host the 2026-09-21 measurement below used, and the host was not otherwise idle during the run (it shared the machine with concurrent build/test work all session; the run still completed with zero timeouts). The command `python3 backend/bluejs/test262/run.py --corpus <pinned-corpus-dir> --adapter target/debug/bluejs-test262 --output <output-dir> --jobs 4 --progress-interval 0` completed the pinned, unfiltered inventory in 810.668 seconds with 4 jobs (explicitly below the host's CPU count, again because of concurrent load rather than to follow the README's single-job-per-worker guidance). Adapter SHA-256: `d9f56845c8c7263f18f2aff9084792674fdc0c9897fa50dd584c25de4ac8694b`. Regexp worker SHA-256: `3f35e9ced03287355b0efa067e5ed5961d23c965c153a77650b1f7f1fd3d5c20`. Test262 revision: `72faf8ec1445c55149615e8b35187830783aba1a`, unchanged; the scope includes `main`, proposals and staging. The tree under test is commit `9decbb3` of `feature/test262-remaining-failures` (not yet merged to `main`), which folds in this branch's work across several areas: general `language/`/`built-ins/`/`staging/` fixes, a `Promise` rebuild on `PromiseCapability` records, class/super/private-name/decorator support (`ClassDefinitionEvaluation`, auto-accessors, the Stage 3 TC39 Decorators proposal), cross-realm primitives, real `Function.prototype.toString` source text, a lossless encoding for unpaired UTF-16 surrogates in source text, general tail calls, GC-stress rooting fixes, and an ECMA-402 default-date-pattern fix.

**This branch has not yet been re-run on macOS or Windows.** [The macOS report](TEST262_MACOS_REPORT.md) and [the Windows report](TEST262_WINDOWS_REPORT.md) still reflect the `eaeb5c1` figures from 2026-09-21; the "Differences from the other platforms" section below is scoped accordingly, not re-verified for this commit. The [triage report](TEST262_ANALYSIS_REPORT.md) classifies the remaining failures.

Test262 has no official “Core” classification. This report defines **ECMA-262 Core** as `language/` + `built-ins/`; **complete ECMA-262 Test262 scope** as Core + `annexB/` + `staging/`; and **ECMA-402** as `intl402/`. `harness/` tests runner support code, so it appears only in the all-inventory total. Every scope is derived from this one unfiltered complete run, not a separately filtered invocation. The complete ECMA-262 scope is an inventory label, not an assertion that time-based staging/proposal tests belong to one published ECMA edition.

| Scope | Scheduled | Pass | Fail | Timeout | Pass rate |
| --- | ---: | ---: | ---: | ---: | ---: |
| ECMA-262 Core (`language/` + `built-ins/`) | 91,820 | 91,814 | 6 | 0 | 99.993% |
| Complete ECMA-262 Test262 scope (Core + `annexB/` + `staging/`) | 95,980 | 95,913 | 67 | 0 | 99.930% |
| ECMA-402 (`intl402/`) | 6,714 | 6,714 | 0 | 0 | 100.000% |
| Test262 harness support (`harness/`) | 232 | 232 | 0 | 0 | 100.000% |
| All Test262 runner modes | 102,926 | 102,859 | 67 | 0 | 99.935% |

| Top-level Test262 group | Scheduled | Pass | Fail | Timeout | Pass rate |
| --- | ---: | ---: | ---: | ---: | ---: |
| `language/` | 44,497 | 44,495 | 2 | 0 | 99.996% |
| `built-ins/` | 47,323 | 47,319 | 4 | 0 | 99.992% |
| `annexB/` | 1,377 | 1,376 | 1 | 0 | 99.927% |
| `staging/` | 2,783 | 2,723 | 60 | 0 | 97.844% |
| `intl402/` | 6,714 | 6,714 | 0 | 0 | 100.000% |
| `harness/` | 232 | 232 | 0 | 0 | 100.000% |

The run completed all 53,582 test files with 0 `timeout` and 0 `harness_error` records. The 67 retained failures are semantic outcomes; [the triage report](TEST262_ANALYSIS_REPORT.md) classifies all of them by root-cause class (instruction-budget/wall-time, corpus-only, cross-realm reverse-membrane, Atomics host flag, string-size policy, `regress`-crate limitation, call-depth limit, spec-version conflict, one grammar gap).

## Selected results (same complete run)

These rows are subsets of the one unfiltered run above (grouped by path prefix), listed because they cover the areas this branch's work changed most. `built-ins/Array/` here is the directory alone (6,117 modes); the filtered invocation `--filter built-ins/Array/` also matches the four `intl402/Array/` modes.

| Selection | Modes | Pass | Fail | Timeout | Pass rate |
| --- | ---: | ---: | ---: | ---: | ---: |
| `built-ins/Array/` | 6,117 | 6,117 | 0 | 0 | 100.000% |
| `built-ins/TypedArray/` + `TypedArrayConstructors/` | 4,322 | 4,322 | 0 | 0 | 100.000% |
| `built-ins/ArrayBuffer/` | 442 | 442 | 0 | 0 | 100.000% |
| `built-ins/SharedArrayBuffer/` | 208 | 208 | 0 | 0 | 100.000% |
| `built-ins/DataView/` | 1,122 | 1,122 | 0 | 0 | 100.000% |
| `built-ins/Atomics/` | 778 | 774 | 4 | 0 | 99.486% |
| `built-ins/Iterator/` | 1,308 | 1,308 | 0 | 0 | 100.000% |
| `built-ins/Promise/` | 1,458 | 1,458 | 0 | 0 | 100.000% |
| `built-ins/ShadowRealm/` | 124 | 124 | 0 | 0 | 100.000% |
| `built-ins/AsyncDisposableStack/` | 208 | 208 | 0 | 0 | 100.000% |
| `built-ins/DisposableStack/` | 186 | 186 | 0 | 0 | 100.000% |
| `built-ins/Temporal/` | 9,210 | 9,210 | 0 | 0 | 100.000% |
| `language/statements/class/` | 8,662 | 8,662 | 0 | 0 | 100.000% |
| `language/expressions/class/` | 8,027 | 8,027 | 0 | 0 | 100.000% |
| `language/expressions/super/` | 184 | 184 | 0 | 0 | 100.000% |
| `language/statements/class/decorator/` | 21 | 21 | 0 | 0 | 100.000% |
| `staging/decorators/` | 6 | 6 | 0 | 0 | 100.000% |
| `language/import/` | 135 | 135 | 0 | 0 | 100.000% |
| `language/expressions/dynamic-import/` | 1,900 | 1,900 | 0 | 0 | 100.000% |
| `language/module-code/` | 602 | 602 | 0 | 0 | 100.000% |
| `language/statements/using/` | 154 | 154 | 0 | 0 | 100.000% |
| `language/statements/await-using/` | 188 | 188 | 0 | 0 | 100.000% |
| `language/statements/for-await-of/` | 2,431 | 2,431 | 0 | 0 | 100.000% |
| `language/eval-code/` | 454 | 454 | 0 | 0 | 100.000% |

## ECMA-402 and Temporal breakdown (same complete run)

The tables above report `intl402/` as one aggregate row (100.000%). This section breaks it down by service and adds ECMA-262 Temporal's own test surface, which `built-ins/` also includes above without a separate line. These figures are derived from the same complete JSONL, not a filtered rerun; group `results.jsonl` by `path.split("/")[1]` (or `[2]` for the Temporal sub-breakdown).

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
| `intl402/*.js` (top-level) + `String/`/`Date/`/`BigInt/`/`Number/`/`Array/`/`FallbackSymbol/`/`TypedArray/` (`toLocale*`/`localeCompare`) | 152 | 152 | 0 | 100% |
| **Total** | **6,714** | **6,714** | **0** | **100.000%** |

`Temporal/` is ECMA-262, not an ECMA-402 service — see [Phase 26's plan](../phase-26-ecma262-temporal/PLAN.md) rather than [Phase 25's](../phase-25-ecma402-internationalization/PLAN.md). `built-ins/Temporal/` (9,210 modes) is Temporal's primary test surface; combined with `intl402/Temporal/` the Temporal denominator is **13,268 modes, 13,268 passing (100.000%)**, per type:

| Temporal type | Combined modes (`built-ins/` + `intl402/`) | Combined pass | Pass rate |
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
| **Total** | **13,268** | **13,268** | **100.000%** |

## Additional verification on this platform (same source revision)

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

## Differences from the other platforms

Not assessed for this commit: see the provenance note above. The 2026-09-21 `eaeb5c1` measurement recorded exactly one difference, `staging/sm/Math/acosh-approx.js` (Ubuntu fail, macOS pass); that fixture is unrelated to this branch's changes, but it has not been re-verified here.

## Reproduce

```sh
export RUSTUP_TOOLCHAIN=1.95.0
cargo build --locked -p blueice-bluejs --bins
python3 backend/bluejs/test262/run.py --corpus /tmp/blueice-test262-72faf8ec-verified --output /tmp/blueice-test262-linux --progress-interval 60
python3 backend/bluejs/test262/analyze.py --run /tmp/blueice-test262-linux --corpus /tmp/blueice-test262-72faf8ec-verified --output /tmp/blueice-test262-linux-analysis
```

The runner rejects a corpus that does not match `snapshot.json`; `--fetch --corpus <dir>` downloads and verifies it. Keep the host otherwise idle during the run (the per-case wall deadline is 2 seconds).
