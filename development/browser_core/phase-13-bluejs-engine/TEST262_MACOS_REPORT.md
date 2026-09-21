# macOS Test262 Report

## Current complete inventory (2026-09-21)

The test host is **macOS 26.6.2 (build 25G83) on Apple silicon** (Apple M4, model Mac16,10), `aarch64-apple-darwin`, with Rust/Cargo 1.98.0 (a Homebrew build: there is no `rustup`, so `rust-toolchain.toml`'s 1.95.0 pin was **not** in effect), 10 logical CPUs, 24 GB of RAM and Python 3.9.6. The command `python3 backend/bluejs/test262/run.py --corpus /tmp/blueice-test262-72faf8ec --jobs 8 --output <dir> --progress-interval 60` completed the pinned, unfiltered inventory in 324.897 seconds with 8 jobs. Adapter SHA-256: `1e206fac066340a8e2b796e900c6f9c54e33b9060586e86c24da9c159fdae4ec`. Regexp worker SHA-256: `24a83ec119bed9dac32564ef5a02fac435cfd74810cda38f244d4d9e91e552b3`. Test262 revision: `72faf8ec1445c55149615e8b35187830783aba1a`; the scope includes `main`, proposals and staging. The tree under test is commit `eaeb5c1` of `feature/bluejs-object-heap`; the two later commits (`4ef9a44`, a `.gitattributes` for two generated fixtures, and `fff18c4`, three Node-oracle corpus lines) do not change any Test262 outcome.

The same source revision was run unfiltered on all three platforms on 2026-09-21; the [macOS](TEST262_MACOS_REPORT.md), [Ubuntu](TEST262_LINUX_REPORT.md) and [Windows](TEST262_WINDOWS_REPORT.md) reports each contain the complete tables, and no platform's result is used as a substitute for another's. The [triage report](TEST262_ANALYSIS_REPORT.md) classifies the remaining failures.

Test262 has no official “Core” classification. This report defines **ECMA-262 Core** as `language/` + `built-ins/`; **complete ECMA-262 Test262 scope** as Core + `annexB/` + `staging/`; and **ECMA-402** as `intl402/`. `harness/` tests runner support code, so it appears only in the all-inventory total. Every scope is derived from this one unfiltered complete run, not a separately filtered invocation. The complete ECMA-262 scope is an inventory label, not an assertion that time-based staging/proposal tests belong to one published ECMA edition.

| Scope | Scheduled | Pass | Fail | Timeout | Pass rate |
| --- | ---: | ---: | ---: | ---: | ---: |
| ECMA-262 Core (`language/` + `built-ins/`) | 91,820 | 89,444 | 2,376 | 0 | 97.412% |
| Complete ECMA-262 Test262 scope (Core + `annexB/` + `staging/`) | 95,980 | 92,975 | 3,003 | 2 | 96.869% |
| ECMA-402 (`intl402/`) | 6,714 | 6,714 | 0 | 0 | 100.000% |
| Test262 harness support (`harness/`) | 232 | 210 | 22 | 0 | 90.517% |
| All Test262 runner modes | 102,926 | 99,899 | 3,025 | 2 | 97.059% |

| Top-level Test262 group | Scheduled | Pass | Fail | Timeout | Pass rate |
| --- | ---: | ---: | ---: | ---: | ---: |
| `language/` | 44,497 | 43,553 | 944 | 0 | 97.879% |
| `built-ins/` | 47,323 | 45,891 | 1,432 | 0 | 96.974% |
| `annexB/` | 1,377 | 1,176 | 201 | 0 | 85.403% |
| `staging/` | 2,783 | 2,355 | 426 | 2 | 84.621% |
| `intl402/` | 6,714 | 6,714 | 0 | 0 | 100.000% |
| `harness/` | 232 | 210 | 22 | 0 | 90.517% |

The run completed all 53,582 test files. Its JSONL contains 2 `timeout` records and 0 `harness_error` records. The 3,025 retained failures are semantic outcomes, not filtered or recategorized timeouts. The `timeout` records are `staging/explicit-resource-management/async-disposal-from-sync-method-returning-a-promise.js` (both modes).

## Selected results (same complete run)

These rows are subsets of the one unfiltered run above (grouped by path prefix), listed because they cover the areas changed most recently. `built-ins/Array/` here is the directory alone (6,117 modes); the filtered invocation `--filter built-ins/Array/` also matches the four `intl402/Array/` modes.

| Selection | Modes | Pass | Fail | Timeout | Pass rate |
| --- | ---: | ---: | ---: | ---: | ---: |
| `built-ins/Array/` | 6,117 | 6,117 | 0 | 0 | 100.000% |
| `built-ins/TypedArray/` + `TypedArrayConstructors/` | 4,322 | 4,322 | 0 | 0 | 100.000% |
| `built-ins/ArrayBuffer/` | 442 | 442 | 0 | 0 | 100.000% |
| `built-ins/SharedArrayBuffer/` | 208 | 208 | 0 | 0 | 100.000% |
| `built-ins/DataView/` | 1,122 | 1,122 | 0 | 0 | 100.000% |
| `built-ins/Atomics/` | 778 | 774 | 4 | 0 | 99.486% |
| `built-ins/Iterator/` | 1,308 | 1,308 | 0 | 0 | 100.000% |
| `built-ins/Promise/` | 1,458 | 806 | 652 | 0 | 55.281% |
| `built-ins/ShadowRealm/` | 124 | 124 | 0 | 0 | 100.000% |
| `built-ins/AsyncDisposableStack/` | 208 | 208 | 0 | 0 | 100.000% |
| `built-ins/DisposableStack/` | 186 | 186 | 0 | 0 | 100.000% |
| `built-ins/Temporal/` | 9,210 | 9,210 | 0 | 0 | 100.000% |
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
| Workspace tests | `cargo test --workspace --no-fail-fast` | **2,985 passed, 0 failed**, 5 ignored | all pass |
| Line coverage, workspace (CI `Coverage` job) | `cargo llvm-cov --workspace --ignore-filename-regex 'extension/src/main\.rs$\|frontend-reference/src/main\.rs$\|mcp-server/src/main\.rs$\|mcp-server/src/server\.rs$' --fail-under-lines 90 --summary-only` | 92.32% lines (90,668 / 98,207); functions 93.19%; regions 90.04% | ≥ 90% lines: met; wall 557 s |
| Line coverage, `blueice-bluejs` alone | `cargo llvm-cov -p blueice-bluejs --fail-under-lines 88 --summary-only` | 91.37% lines (58,892 / 64,456); functions 91.69%; regions 88.44% | ≥ 88% lines: met; wall 416 s |
| Line coverage, `blueice-ecma402` alone | `cargo llvm-cov -p blueice-ecma402 --summary-only` | 94.36% lines (9,559 / 10,130); functions 95.29%; regions 91.91% | informational |
| Node differential oracle | `cargo test -p blueice-bluejs --test node_differential -- --ignored` (Node v24.21.0) | **4 / 4 tests pass**: the 22,268-script main corpus plus the 10-script and 67-script matrices (Intl NumberFormat range/locale data) agree with Node | all pass |
| TypeScript compatibility oracle | `npm exec --yes --package typescript@5.9.3 -- env BLUEICE_BLUETSC_ORACLE=tsc cargo test -p blueice-bluets --test typescript_oracle -- --ignored` | **1 / 1 test passes**: all 68 cases (48 compile-and-run cases whose stdout is compared, 20 diagnostic-parity cases; 71 module sources) agree with TypeScript 5.9.3 | all pass |

Coverage on this host uses Homebrew `llvm@22` (LLVM 22.1.8, the same LLVM version as this `rustc`) through `LLVM_COV`/`LLVM_PROFDATA`, since Homebrew's Rust ships without `llvm-tools`. Node 24.21.0 is Homebrew's `node@24` (the default `node` on this machine is 26.7.0, so the oracles were run with `node@24` first in `PATH`, matching CI). The Node oracle initially disagreed with Node 24 on three scripts that assert Node's legacy behaviour for a hook installed on a primitive's prototype (`'a'.match(3)`, `'a'.search('b')`, `'a'.matchAll(true)`); BlueJS follows ECMA-262 and Test262 there, so those lines were removed from the oracle corpus in `fff18c4`.

Line coverage is a different measure from a Test262 pass rate and the two must not be quoted interchangeably: it is the fraction of the Rust source lines that execute during the crates' own test suites. The five ignored tests are the opt-in oracles (four Node differential tests and one TypeScript compatibility test), which the table's last two rows run explicitly.


## Differences from the other platforms

- **Ubuntu**: 2 of 102,926 modes differ (1 file): `staging/sm/Math/acosh-approx.js` (this platform: pass; Ubuntu: fail).

## Reproduce

```sh
cargo build -p blueice-bluejs --bins
python3 backend/bluejs/test262/run.py --corpus /tmp/blueice-test262-72faf8ec --jobs 8 --output /tmp/blueice-test262-macos
python3 backend/bluejs/test262/analyze.py --run /tmp/blueice-test262-macos --corpus /tmp/blueice-test262-72faf8ec --output /tmp/blueice-test262-macos-analysis
```

Fetch the pinned corpus once with `--fetch --corpus /tmp/blueice-test262-72faf8ec` (the runner verifies it against `snapshot.json`). `analyze.py` needs the same `--corpus` because the default reference directory has no snapshot metadata. Do not run other heavy jobs while the inventory runs: the per-case wall deadline is 2 seconds, and a loaded machine produces spurious `timeout` records.
