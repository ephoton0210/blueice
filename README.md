# BlueIce

[![CI](https://github.com/ephoton0210/blueice/actions/workflows/ci.yml/badge.svg)](https://github.com/ephoton0210/blueice/actions/workflows/ci.yml)

An AI-driven web browser built from scratch in Rust. The goal is a single rendering engine where a human user and an AI agent perceive the same page state from the same render pass, rather than driving a separate browser instance through external automation (e.g. Puppeteer/CDP) — which is subject to bot-detection differentiation and state drift between the two instances.

BlueIce references Firefox (Gecko) and Chromium (Blink/V8) as technical references for the rendering pipeline.

## Project status

The core rendering pipeline is complete end to end — HTML in, real pixels out — with `core` and a reference `frontend` running as separate OS processes over a Unix-socket IPC protocol (plus a shared-memory frame-plane for pixel delivery). The AI-facing representation (an accessibility-tree-shaped schema, addressed by stable node IDs) is extracted from the exact same render pass a human sees, reachable both through the reference `frontend` and through an MCP server (`blueice-mcp-server`) that lets any MCP-compatible AI agent drive BlueIce directly; a `blueice-launcher` process lets a human's `frontend` and an AI's `mcp-server` observe and act on *one* running `core` instance simultaneously — the literal goal this project exists for.

Also implemented: multi-tab support (`core`-side and MCP-exposed), a fleet-wide process/memory supervisor, i18n/localization, a Chromium differential-testing pipeline, the minimal-slice mechanism for a local AI safety gatekeeper — every navigation is reviewed by a separate always-resident process before it takes effect, non-blocking with respect to other tabs/clients, fail-closed if that process is unreachable; the gatekeeper's real review logic (rule-base + AI) is still to come — the minimal-slice mechanism for the extension protocol's server-side capability enforcement (a hardcoded single extension proven end to end over a real process boundary; the WASM runtime and manifest parser are still to come), a minimal-slice live `core` hot-swap: `blueice-launcher` can swap the running `core` for a freshly-spawned one at runtime, replaying open tabs into it, with no already-connected client ever dropped (automatic update-detection/scheduling is still to come); and, for BlueJS (the project's own from-scratch JavaScript engine, chosen over embedding an existing one so the safety gatekeeper can eventually hook script execution directly), design is fully resolved — execution model, GC, event loop, out-of-process placement, and the MVP language subset — and the first execution path is real: a hand-written tokenizer and recursive-descent parser (`backend/bluejs`) produce an AST, then a compiler emits stack-machine bytecode for a bounded VM. This slice executes primitive expressions, variables/block scopes, conditions/loops, ordinary objects and sparse arrays (including holes, indexed writes and length truncation) backed by stable handles, prototype lookup, and nursery/tenured GC with a managed-data budget. Public-pipeline tests and an opt-in Node.js differential corpus check execution, coercion and error behavior. Compiled functions/closures, String protocols, descriptors and iteration/spread now execute; remaining language semantics, general Array methods, event loop and browser integration are still to come. Still design-only or not started: a download manager. See [`BROWSER_CORE_PLAN.md`](development/browser_core/BROWSER_CORE_PLAN.md) for the full phase-by-phase progress table, or [`CLAUDE.md`](CLAUDE.md) for a denser, continuously-updated status summary. See [`development/browser_core/testing/TEST_PLAN.md`](development/browser_core/testing/TEST_PLAN.md) for the test/coverage policy the CI badge above enforces.

BlueJS targets [ECMAScript 2026 edition 17](development/browser_core/phase-13-bluejs-engine/ECMASCRIPT_2026.md), beyond its original MVP subset. Its [String implementation](development/browser_core/phase-13-bluejs-engine/STRING_BUILTINS.md) now includes all 35 core prototype methods, three statics and Annex B extensions, backed by lossless UTF-16, Unicode 17 normalization/casing, RegExp/Symbol protocols, iterators, compiled and bound replacement callbacks, object coercion and descriptors. RegExp.escape, bound constructors, instanceof/Symbol.hasInstance, Math constants/functions, abstract equality and property-presence are also implemented. String locale methods now use ICU-backed Intl.Collator, locale canonicalization and locale-sensitive casing; Intl.Locale supplies canonical locale objects, Unicode options, likely-subtag transforms and Locale-info queries. Test262 harness scripts can share classic-script globals and receive native descriptor/constructibility helpers. Regex compilation/matching run in a terminable helper process. The [Intl/deadline/Test262 report](development/browser_core/phase-13-bluejs-engine/INTL_CONFORMANCE.md) records the complete 102,926-mode inventory; the current platform-verification status is documented below. The latest BlueJS line, function and region coverage is recorded in the current coverage section below; CI enforces an 88% BlueJS line floor. The String inventory records resource limits and remaining language dependencies; neither coverage nor the Node oracle is a full Test262/ECMAScript conformance claim.

## Test262 results by platform

The same source revision (commit `eaeb5c1` of `feature/bluejs-object-heap`) was run through the unfiltered, pinned Test262 inventory (53,582 files / 102,926 modes, snapshot `72faf8ec…`) on **macOS and Ubuntu on 2026-09-21 (the Windows run is still in progress and its row is filled from its own result when it completes)**. Every row below is a real complete run on that platform's own hardware and toolchain; no row is inferred from another. The Ubuntu run used the repository's pinned Rust 1.95.0; the macOS run used Homebrew's Rust 1.98.0 (there is no `rustup` there, so the pin was not in effect).

Test262 does not provide an official "Core" switch, so the reports use explicit top-level-directory scopes: **ECMA-262 Core** is `language/` + `built-ins/` (91,820 modes); **complete ECMA-262 Test262 scope** adds `annexB/` and `staging/` (95,980 modes); and **ECMA-402** is `intl402/` (6,714 modes). `harness/` (232 modes) validates Test262 support code and is retained only in the all-inventory total. Every scope is calculated from the same unfiltered complete run, not from separately filtered invocations. The complete ECMA-262 scope is an inventory label, not an assertion that time-based staging/proposal tests belong to one published ECMA edition.

| Platform / evidence | ECMA-262 Core | Complete ECMA-262 scope | ECMA-402 | All modes: pass / fail / timeout |
| --- | ---: | ---: | ---: | ---: |
| macOS 26.6.2, Apple M4 (arm64); Rust 1.98.0; 8 jobs; 324.9 s | 89,444 / 91,820 (97.412%) | 92,975 / 95,980 (96.869%) | 6,714 / 6,714 (100.000%) | 99,899 / 3,025 / 2 (97.059%) |
| Ubuntu 24.04.4 LTS (native), Core i5-9400T (x86_64); Rust 1.95.0; 6 jobs; 542.3 s | 89,444 / 91,820 (97.412%) | 92,973 / 95,980 (96.867%) | 6,714 / 6,714 (100.000%) | 99,897 / 3,027 / 2 (97.057%) |
| Windows 11 (build 26100) VM (x86_64, 6 vCPU); Rust 1.95.0 | run in progress | run in progress | run in progress | run in progress |

Every run recorded zero `harness_error` records; macOS recorded 2 `timeout` records and Ubuntu 2 (both are the two modes of `staging/explicit-resource-management/async-disposal-from-sync-method-returning-a-promise.js`). These are conformance progress measurements, not a claim of full ECMAScript conformance: the remaining failures are classified in the [triage report](development/browser_core/phase-13-bluejs-engine/TEST262_ANALYSIS_REPORT.md). Between macOS and Ubuntu only `staging/sm/Math/acosh-approx.js` differs (it passes on macOS and fails on Ubuntu; the cause is not yet isolated). Exact scope, provenance and rerun commands are in the per-platform reports: [macOS](development/browser_core/phase-13-bluejs-engine/TEST262_MACOS_REPORT.md), [Ubuntu](development/browser_core/phase-13-bluejs-engine/TEST262_LINUX_REPORT.md) and [Windows](development/browser_core/phase-13-bluejs-engine/TEST262_WINDOWS_REPORT.md).

### Test suites and coverage on the same revision

| Platform | Workspace tests | Line coverage: workspace (gate 90%) | `blueice-bluejs` (gate 88%) | `blueice-ecma402` | Node oracle (22,268 scripts) | TypeScript oracle (68 cases) |
| --- | --- | --- | --- | --- | --- | --- |
| macOS | 2,985 pass / 0 fail / 5 ignored | 92.32% (90,668 / 98,207) | 91.37% (58,892 / 64,456) | 94.36% (9,559 / 10,130) | 4 / 4 pass | pass |
| Ubuntu | 2,985 pass / 0 fail / 5 ignored | 92.20% (90,541 / 98,201) | 91.38% (58,892 / 64,450) | 94.42% (9,559 / 10,124) | 4 / 4 pass | pass |
| Windows 11 VM | in progress | in progress | in progress | in progress | in progress | in progress |

Line coverage is a different measure from a Test262 pass rate and the two are not interchangeable. The BlueJS figure was 91.37% (58,892 / 64,456) on macOS and 91.38% (58,892 / 64,450) on Ubuntu, up from the previously recorded 88.38%; the workspace and BlueJS gates in CI are 90% and 88%. The Node oracle compares 22,268 scripts with Node 24; the TypeScript oracle runs BlueTSC's 68-case fixture matrix against TypeScript 5.9.3. BlueTS's own results are in the [BlueTS test report](development/browser_core/phase-18-bluets/TEST_REPORT.md).

### Current BlueJS coverage (2026-09-24)

At commit `06724eed` on macOS 27.0, Rust 1.95.0 and `cargo-llvm-cov` 0.9.1, `cargo llvm-cov -p blueice-bluejs --summary-only --quiet` completed successfully with the default BlueJS test suite and no source-file exclusions. The opt-in Node oracle and the external full Test262 runner are not part of this measurement. The BlueJS CI gate is 88% line coverage; workspace coverage was not remeasured at this commit.

| Measure | Covered / instrumented | Coverage |
| --- | ---: | ---: |
| Lines | 67,872 / 73,153 | 92.78% |
| Functions | 4,971 / 5,329 | 93.28% |
| Regions | 113,547 / 126,212 | 89.97% |

The [macOS report's per-file table](development/browser_core/phase-13-bluejs-engine/TEST262_MACOS_REPORT.md#later-bluejs-per-file-coverage-2026-09-24) lists the line, function and region counts for every BlueJS source file in this measurement.

### ECMA-402 breakdown and Temporal

Every `intl402/` service passes in full on the platforms measured, including `Temporal/` (4,058 of the 6,714 `intl402/` modes). `Temporal/` is ECMA-262 (a core language built-in, like `Date`), not an ECMA-402 service, and is scoped as its own effort — [Phase 26](development/browser_core/phase-26-ecma262-temporal/PLAN.md) — separate from ECMA-402's [Phase 25](development/browser_core/phase-25-ecma402-internationalization/PLAN.md). Test262 also has a larger, separate `built-ins/Temporal/` tree (9,210 modes) that is excluded from the `intl402/`-scoped column above but is Temporal's primary test surface.

| Platform | `intl402/` non-`Temporal` (11 services) | `intl402/Temporal/` | Combined Temporal (`built-ins/` + `intl402/`) |
| --- | ---: | ---: | ---: |
| macOS, 2026-09-21 | 2,656 / 2,656 (100.000%) | 4,058 / 4,058 (100.000%) | 13,268 / 13,268 (100.000%) |
| Ubuntu 24.04.4 LTS, 2026-09-21 | 2,656 / 2,656 (100.000%) | 4,058 / 4,058 (100.000%) | 13,268 / 13,268 (100.000%) |
| Windows 11 VM, 2026-09-21 | in progress | in progress | in progress |

The breakdown comes from the same unfiltered complete inventory as the first table. The full per-service and per-Temporal-type breakdown, provenance and reproduction commands are in the [Ubuntu report](development/browser_core/phase-13-bluejs-engine/TEST262_LINUX_REPORT.md), the [macOS report](development/browser_core/phase-13-bluejs-engine/TEST262_MACOS_REPORT.md) and the [Windows report](development/browser_core/phase-13-bluejs-engine/TEST262_WINDOWS_REPORT.md).

Design and planning documents live under [`development/`](development/); it is not source code. Each subdirectory covers one major component of the project, following the same design-first workflow: a plan is drafted before implementation starts, and updated as the design evolves.

- **[`development/browser_core/`](development/browser_core/)** — the browser engine itself. See [`BROWSER_CORE_PLAN.md`](development/browser_core/BROWSER_CORE_PLAN.md) for the current plan, open design decisions, and progress tracking.

## License

BlueIce is licensed under the [Mozilla Public License 2.0](LICENSE) (MPL-2.0).

Code adapted from Gecko stays MPL-2.0 (an inherent MPL obligation). Code adapted from Chromium/Blink (BSD-3-Clause) is relicensed to MPL-2.0 with the original BSD notice preserved. See the plan document above for details on how this applies, and on the project's position on patents and trademarks of the projects it references.
