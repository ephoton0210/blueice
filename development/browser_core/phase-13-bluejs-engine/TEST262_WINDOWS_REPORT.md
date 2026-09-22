# Windows Test262 Report

## Current complete inventory (2026-09-21)

The test host is **Windows 11 Pro (build 26100), zh-TW, on a QEMU/KVM VM** (`Ubuntu 24.04 PC (Q35 + ICH9, 2009)` model, CPU `Intel Core Processor (Skylake, IBRS)`, 6 logical CPUs, 12 GB of RAM), `x86_64-pc-windows-msvc`, with Rust/Cargo 1.95.0 (the pinned toolchain), Python 3.12.7 and Node v24.21.0. Windows Defender real-time protection is on, with exclusions added for the Cargo/rustup/Python paths under the build's service account so a scan does not compete with every compiler invocation. The command `python backend\bluejs\test262\run.py --corpus X:\blueice-test262-72faf8ec-verified --adapter X:\adapter-new\bluejs-test262.exe --output X:\blueice-inventory-final3 --progress-interval 60` completed the pinned, unfiltered inventory in 801.750 seconds with 6 jobs, run in isolation on an otherwise idle host (see "Root cause and fix" below for why that matters). Adapter SHA-256: `dd1e458f429b5aad429849d4576200755ddff3ec1805d24f94859c2cb5dc6d6f`. Regexp worker SHA-256: `f73fda3e2ac25f5b074dffb1a389f3505b30f96949b5f11010da51bd6a1d093e`. Test262 revision: `72faf8ec1445c55149615e8b35187830783aba1a`; the scope includes `main`, proposals and staging.

The tree under test is commit `fff18c4` of `feature/bluejs-object-heap`, exported with `git archive` and CRLF-converted to emulate `core.autocrlf=true`, with three files updated to their `b0b0021` state: `backend/bluejs/src/regex_worker.rs`, `backend/bluejs/tests/regex_worker_reuse.rs` and the root `Cargo.toml` (the RegExp-request memo and dependency-optimisation profiles that this platform's fix below needed). The further commit `1947afb` (a socket-connect race fix) changes only `#[cfg(unix)]`-gated code in `blueice-launcher`, `blueice-mcp-server` and `backend/core/engine/tests/core_binary.rs`, none of which this Windows build compiles, so its tree is behaviourally equivalent to `1947afb` for every number in this report. macOS and Ubuntu ran the literal `1947afb` tree; see those reports.

The same source revision was run unfiltered on all three platforms on 2026-09-21; the [macOS](TEST262_MACOS_REPORT.md), [Ubuntu](TEST262_LINUX_REPORT.md) and [Windows](TEST262_WINDOWS_REPORT.md) reports each contain the complete tables, and no platform's result is used as a substitute for another's. The [triage report](TEST262_ANALYSIS_REPORT.md) classifies the remaining failures.

Test262 has no official “Core” classification. This report defines **ECMA-262 Core** as `language/` + `built-ins/`; **complete ECMA-262 Test262 scope** as Core + `annexB/` + `staging/`; and **ECMA-402** as `intl402/`. `harness/` tests runner support code, so it appears only in the all-inventory total. Every scope is derived from this one unfiltered complete run, not a separately filtered invocation. The complete ECMA-262 scope is an inventory label, not an assertion that time-based staging/proposal tests belong to one published ECMA edition.

| Scope | Scheduled | Pass | Fail | Timeout | Pass rate |
| --- | ---: | ---: | ---: | ---: | ---: |
| ECMA-262 Core (`language/` + `built-ins/`) | 91,820 | 89,444 | 2,376 | 0 | 97.412% |
| Complete ECMA-262 Test262 scope (Core + `annexB/` + `staging/`) | 95,980 | 92,973 | 3,005 | 2 | 96.867% |
| ECMA-402 (`intl402/`) | 6,714 | 6,714 | 0 | 0 | 100.000% |
| Test262 harness support (`harness/`) | 232 | 210 | 22 | 0 | 90.517% |
| All Test262 runner modes | 102,926 | 99,897 | 3,027 | 2 | 97.057% |

| Top-level Test262 group | Scheduled | Pass | Fail | Timeout | Pass rate |
| --- | ---: | ---: | ---: | ---: | ---: |
| `language/` | 44,497 | 43,553 | 944 | 0 | 97.879% |
| `built-ins/` | 47,323 | 45,891 | 1,432 | 0 | 96.974% |
| `annexB/` | 1,377 | 1,176 | 201 | 0 | 85.403% |
| `staging/` | 2,783 | 2,353 | 428 | 2 | 84.549% |
| `intl402/` | 6,714 | 6,714 | 0 | 0 | 100.000% |
| `harness/` | 232 | 210 | 22 | 0 | 90.517% |

The run completed all 53,582 test files. Its JSONL contains 2 `timeout` records and 0 `harness_error` records. The 3,027 retained failures are semantic outcomes, not filtered or recategorized timeouts. The `timeout` records are `staging/explicit-resource-management/async-disposal-from-sync-method-returning-a-promise.js` (both modes) — the same finite fixture that also times out on macOS and Ubuntu.

## Root cause and fix: why this platform needed real changes, not a longer deadline (2026-09-21)

An initial, honestly-recorded Windows run of an earlier tree (before this session's fixes) recorded **288 `timeout` outcomes** out of 102,926 modes at the runner's default 2-second per-case wall deadline — RegExp-heavy cases in particular (`built-ins/Function/prototype/toString/built-in-function-object.js`, the `intl402` NumberFormat matrices, several `staging/sm/RegExp/*` files) that complete in well under a second on macOS/Ubuntu. Lengthening the deadline was rejected as a fix: a slower deadline hides a real performance defect rather than closing it, so the two causes were found and fixed instead.

**Cause 1 — a helper process started for every single RegExp operation.** `backend/bluejs/src/regex_worker.rs` retired its `bluejs-regexp-worker` helper process after every `compile`/`find`/`validate` call and started a fresh one for the next, to avoid a Windows-specific hang where joining a failed transaction's I/O thread from its own error path could block forever. Starting a process is milliseconds of host work everywhere, but far more so on Windows (each new process is also scanned), and a Test262 harness matrix can call into RegExp thousands of times in one file — one case alone sent 167,000 `find` requests, and only 263 of them were for a distinct pattern/subject/start-index combination. The fix (commit `b0b0021`) keeps a small pool of idle, healthy workers (bounded at 8) instead of retiring one after every operation — the original Windows-only hang is instead avoided by never joining a *failed* worker's I/O thread from ordinary code, only detaching it — **and** adds a bounded, size-limited memo of the helper's own answers to `compile` and `find` requests in the parent process, keyed on the exact pattern/flags/subject/start-index (a rejected pattern is never memoized, since a syntax-error message must always come from the helper). Matching is a pure function of its inputs, so a script that asks the same question thousands of times now reaches the helper process once. A new test, `backend/bluejs/tests/regex_worker_reuse.rs`, asserts both that repeated operations reuse one worker and that identical repeats are answered without another round trip. This alone measured 73.6 s → 19.3 s for the heaviest single Test262 case on one development machine, and combined with unrelated dependency-optimisation profile changes (commit `948aa4d`, `opt-level = 3` for every workspace dependency in dev/test builds) reduced the microbenchmarked per-operation Windows RegExp round-trip from roughly 17–20 ms to about **0.54 ms** (versus 20–40 μs on macOS/Ubuntu — the remaining gap is IPC thread-hop/marshalling overhead, not further investigated once it stopped causing timeouts).

**Cause 2 — the VM's data disk was attached over emulated USB.** After the RegExp fix, a full inventory run on this VM still recorded intermittent multi-second stalls unrelated to the engine: Windows' System event log recorded a `disk` provider event 153 ("reset to device, \\Device\\... during a paging operation") roughly every 20 seconds against the attached data volume, each one costing the run 15–37 seconds of wall time on an ordinary file read. The volume (`X:`, holding the checked-out tree, target directory and Test262 corpus) was attached as a `qemu-xhci` USB disk; the guest's own `Get-Disk`/event-log evidence pointed to the emulated USB storage stack, not to CPU, RAM or the engine. The fix was operational, not code: the VM was shut down, its data `.qcow2` was reattached on the existing SATA/AHCI controller (unit 4) instead of USB, and the VM was restarted — no VM setting beyond the disk's bus changed. The full `blueice-bluejs` suite and a full Test262 inventory both immediately confirmed the fix: the inventory's wall time roughly halved (2,411 s → 1,168 s for a run sharing the host with a concurrent Linux verification pass, 801.75 s for the isolated run recorded in this report), and a targeted rerun of the 146 files that had timed out before the RegExp fix passed 144 of them at the runner's **default, unmodified 2-second deadline** (the remaining 2 are the same finite-fixture timeout every platform records, see above).

A run of the fixed adapter taken *while* a full Linux verification pass shared the same physical host recorded 8 timeouts instead of 2 — confirmed to be host CPU contention between the two concurrent workloads, not a Windows-specific regression, by rerunning the identical adapter in isolation once the host was quiet: the isolated rerun (recorded as this report's authoritative numbers above) reproduces Ubuntu's own outcome exactly, including the same single `staging/sm/Math/acosh-approx.js` difference from macOS (see "Differences from the other platforms" below).

The full `blueice-bluejs` test suite (which includes `regex_worker_reuse`, `regex_deadlines` and `process_hosts`) was run to completion on Windows at every stage of this investigation with **no hang**: 1,779–1,782 tests pass depending on which intermediate adapter was tested, 0 failed.

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
| Full `blueice-bluejs` suite | `cargo test -p blueice-bluejs --no-fail-fast` | **1,782 passed, 0 failed**, 4 ignored (439 s, isolated) | all pass |
| Workspace tests | `cargo test --workspace --no-fail-fast` | **2,747 passed, 0 failed**, 5 ignored (1,457 s) | all pass |
| Line coverage, workspace (CI `Coverage` job) | `cargo llvm-cov --workspace --ignore-filename-regex 'extension/src/main\.rs$\|frontend-reference/src/main\.rs$\|mcp-server/src/main\.rs$\|mcp-server/src/server\.rs$' --fail-under-lines 90 --summary-only` | 91.28% lines (86,461 / 94,723); functions 92.15%; regions 89.17% | ≥ 90% lines: met; wall 920 s |
| Line coverage, `blueice-bluejs` alone | `cargo llvm-cov -p blueice-bluejs --fail-under-lines 88 --summary-only` | 91.33% lines (58,919 / 64,514); functions 91.72%; regions 88.40% | ≥ 88% lines: met; wall 673 s |
| Line coverage, `blueice-ecma402` alone | `cargo llvm-cov -p blueice-ecma402 --summary-only` | 93.54% lines (9,470 / 10,124); functions 92.56%; regions 91.31% | informational; wall 86 s |
| Node differential oracle | `cargo test -p blueice-bluejs --test node_differential -- --ignored` (Node v24.21.0) | **4 / 4 tests pass**: the 22,268-script main corpus plus the 10-script and 67-script matrices (Intl NumberFormat range/locale data) agree with Node | all pass; wall 133 s |
| TypeScript compatibility oracle | `$env:BLUEICE_BLUETSC_ORACLE="tsc.cmd"; npm exec --yes --package typescript@5.9.3 -- cargo test -p blueice-bluets --test typescript_oracle -- --ignored` | **1 / 1 test passes**: all 68 cases (48 compile-and-run cases whose stdout is compared, 20 diagnostic-parity cases; 71 module sources) agree with TypeScript 5.9.3 | all pass; wall 327 s |

The workspace test/coverage/oracle numbers above are lower for `workspace tests`/`workspace coverage` than macOS/Ubuntu's ~2,992/92%: `backend/launcher`, `backend/mcp-server` and `backend/core/engine/tests/core_binary.rs` gate their Unix-domain-socket code behind `#[cfg(unix)]` (`blueice-launcher`/`blueice-mcp-server` have no Windows transport yet), so this platform compiles and measures a smaller set of workspace tests by design, not by omission — `blueice-bluejs` and `blueice-ecma402` themselves are measured in full and match macOS/Ubuntu within normal per-platform variance. The TypeScript oracle needs `tsc.cmd` rather than the bare `tsc` shim `npm exec` provides on Windows.

Line coverage is a different measure from a Test262 pass rate and the two must not be quoted interchangeably: it is the fraction of the Rust source lines that execute during the crates' own test suites. `blueice-ecma402`'s line coverage (93.54%) matches macOS/Ubuntu's post-optimisation-profile figures (93.55%/93.28%) rather than the older pre-profile 94.36%, for the same `opt-level = 3` dependency-build reason described in the other two reports.

## Differences from the other platforms

- **macOS**: 2 of 102,926 modes differ (1 file): `staging/sm/Math/acosh-approx.js` (this platform: fail; macOS: pass).
- **Ubuntu**: identical outcome for every one of the 102,926 modes.

The single difference from macOS (`staging/sm/Math/acosh-approx.js`) is the same one Ubuntu records against macOS — both this platform and Ubuntu are `x86_64`, while macOS is `aarch64`; the case's fixed-point approximation is architecture-sensitive, not Windows-specific, and its root cause is not yet isolated (tracked the same as the Ubuntu report's own note).

## Reproduce

```powershell
$env:RUSTUP_TOOLCHAIN = "1.95.0"
cargo build --locked -p blueice-bluejs --bins
python backend\bluejs\test262\run.py --corpus <pinned-corpus> --output <dir> --progress-interval 60
python backend\bluejs\test262\analyze.py --run <dir> --corpus <pinned-corpus> --output <dir>-analysis
```

The runner rejects a corpus that does not match `snapshot.json`; `--fetch --corpus <dir>` downloads and verifies it. A root `.gitattributes` pins the generated host-typing artifacts (`lib.blueice.d.ts`, `lib.blueice.manifest.json`, `backend/core/engine/tests/fixtures/host_typings/**`) to `eol=lf`, needed because a Windows checkout with `core.autocrlf=true` otherwise converts them to CRLF and fails their hash-identity test. Keep the host otherwise idle during the run (the per-case wall deadline is 2 seconds; this report's own experience above is a case study in why that matters more, not less, once the engine itself is fast).
