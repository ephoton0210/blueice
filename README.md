# BlueIce

[![CI](https://github.com/ephoton0210/blueice/actions/workflows/ci.yml/badge.svg)](https://github.com/ephoton0210/blueice/actions/workflows/ci.yml)

An AI-driven web browser built from scratch in Rust. The goal is a single rendering engine where a human user and an AI agent perceive the same page state from the same render pass, rather than driving a separate browser instance through external automation (e.g. Puppeteer/CDP) — which is subject to bot-detection differentiation and state drift between the two instances.

BlueIce references Firefox (Gecko) and Chromium (Blink/V8) as technical references for the rendering pipeline.

## Project status

The core rendering pipeline is complete end to end — HTML in, real pixels out — with `core` and a reference `frontend` running as separate OS processes over a Unix-socket IPC protocol (plus a shared-memory frame-plane for pixel delivery). The AI-facing representation (an accessibility-tree-shaped schema, addressed by stable node IDs) is extracted from the exact same render pass a human sees, reachable both through the reference `frontend` and through an MCP server (`blueice-mcp-server`) that lets any MCP-compatible AI agent drive BlueIce directly; a `blueice-launcher` process lets a human's `frontend` and an AI's `mcp-server` observe and act on *one* running `core` instance simultaneously — the literal goal this project exists for.

Also implemented: multi-tab support (`core`-side and MCP-exposed), a fleet-wide process/memory supervisor, i18n/localization, a Chromium differential-testing pipeline, the minimal-slice mechanism for a local AI safety gatekeeper — every navigation is reviewed by a separate always-resident process before it takes effect, non-blocking with respect to other tabs/clients, fail-closed if that process is unreachable; the gatekeeper's real review logic (rule-base + AI) is still to come — the minimal-slice mechanism for the extension protocol's server-side capability enforcement (a hardcoded single extension proven end to end over a real process boundary; the WASM runtime and manifest parser are still to come), a minimal-slice live `core` hot-swap: `blueice-launcher` can swap the running `core` for a freshly-spawned one at runtime, replaying open tabs into it, with no already-connected client ever dropped (automatic update-detection/scheduling is still to come); and, for BlueJS (the project's own from-scratch JavaScript engine, chosen over embedding an existing one so the safety gatekeeper can eventually hook script execution directly), design is fully resolved — execution model, GC, event loop, out-of-process placement, and the MVP language subset — and the first execution path is real: a hand-written tokenizer and recursive-descent parser (`backend/bluejs`) produce an AST, then a compiler emits stack-machine bytecode for a bounded VM. This slice executes primitive expressions, variables/block scopes, conditions/loops, ordinary objects and sparse arrays (including holes, indexed writes and length truncation) backed by stable handles, prototype lookup, and nursery/tenured GC with a managed-data budget. Public-pipeline tests and an opt-in Node.js differential corpus check execution, coercion and error behavior. Compiled functions/closures, String protocols, descriptors and iteration/spread now execute; the current language/built-in inventory and validation results are documented below. Still design-only or not started: a download manager. See [`BROWSER_CORE_PLAN.md`](development/browser_core/BROWSER_CORE_PLAN.md) for the full phase-by-phase progress table, or [`CLAUDE.md`](CLAUDE.md) for a denser, continuously-updated status summary. See [`development/browser_core/testing/TEST_PLAN.md`](development/browser_core/testing/TEST_PLAN.md) for the test/coverage policy the CI badge above enforces.

BlueJS targets [ECMAScript 2026 edition 17](development/browser_core/phase-13-bluejs-engine/ECMASCRIPT_2026.md), beyond its original MVP subset. Its [String implementation](development/browser_core/phase-13-bluejs-engine/STRING_BUILTINS.md) now includes all 35 core prototype methods, three statics and Annex B extensions, backed by lossless UTF-16, Unicode 17 normalization/casing, RegExp/Symbol protocols, iterators, compiled and bound replacement callbacks, object coercion and descriptors. RegExp.escape, bound constructors, instanceof/Symbol.hasInstance, Math constants/functions, abstract equality and property-presence are also implemented. String locale methods now use ICU-backed Intl.Collator, locale canonicalization and locale-sensitive casing; Intl.Locale supplies canonical locale objects, Unicode options, likely-subtag transforms and Locale-info queries. Test262 harness scripts can share classic-script globals and receive native descriptor/constructibility helpers. Regex compilation/matching run in a terminable helper process. The [Intl/deadline/Test262 report](development/browser_core/phase-13-bluejs-engine/INTL_CONFORMANCE.md) records the complete 102,926-mode inventory; the current platform-verification status is documented below. The latest BlueJS line, function and region coverage is recorded in the current coverage section below; CI enforces an 88% BlueJS line floor. The String inventory records resource limits and remaining language dependencies; neither coverage nor the Node oracle is a full Test262/ECMAScript conformance claim.

## Test262 results by platform

The latest complete native measurements ran on macOS on 2026-10-12 and on Ubuntu and Windows on 2026-10-13, all with Rust 1.95.0. Each platform ran the same pinned inventory: **53,582 files / 102,926 scheduled modes**, Test262 revision `72faf8ec1445c55149615e8b35187830783aba1a`. Every applicable mode passed, with zero failures, unsupported cases, timeouts or harness errors. Each row links its platform report and JSON summary.

The macOS measurement uses source fingerprint `8b938660…`; Ubuntu and Windows use `d6c141d0…`. Their production Rust inputs are byte-identical; native collection support and the portable process-host tests account for the source differences. The reports retain the complete source hashes, executable hashes, commands and coverage audits. Windows ran in the Windows 11 build 26100 VM on `pb60g.bravotekcorp.cc`; its test checkout, caches, profiles and temporary files all use the attached B: disk.

Test262 does not provide an official "Core" switch, so the reports use explicit top-level-directory scopes: **ECMA-262 Core** is `language/` + `built-ins/` (91,820 modes); **complete ECMA-262 Test262 scope** adds `annexB/` and `staging/` (95,980 modes); and **ECMA-402** is `intl402/` (6,714 modes). `harness/` (232 modes) validates Test262 support code and is retained only in the all-inventory total. Every scope is calculated from the same unfiltered complete run, not from separately filtered invocations. The complete ECMA-262 scope is an inventory label, not an assertion that time-based staging/proposal tests belong to one published ECMA edition.

| Platform / evidence | ECMA-262 Core | Complete ECMA-262 scope | ECMA-402 | All modes: pass / fail / timeout |
| --- | ---: | ---: | ---: | ---: |
| [macOS](development/browser_core/phase-13-bluejs-engine/TEST262_MACOS_REPORT.md), 2026-10-12; [JSON](development/browser_core/phase-13-bluejs-engine/test262-macos-summary.json); 4 jobs; 450.104 s | 91,816 / 91,820 (99.996%) | 95,975 / 95,980 (99.995%) | 6,714 / 6,714 (100%) | 102,921 / 0 / 0 (99.995%) |
| [Ubuntu 24.04.4 LTS](development/browser_core/phase-13-bluejs-engine/TEST262_LINUX_REPORT.md), 2026-10-13; [JSON](development/browser_core/phase-13-bluejs-engine/test262-linux-summary.json); 2 jobs; 2,037.104 s | 91,816 / 91,820 (99.996%) | 95,975 / 95,980 (99.995%) | 6,714 / 6,714 (100%) | 102,921 / 0 / 0 (99.995%) |
| [Windows 11 VM](development/browser_core/phase-13-bluejs-engine/TEST262_WINDOWS_REPORT.md), 2026-10-13; [JSON](development/browser_core/phase-13-bluejs-engine/test262-windows-summary.json); 2 jobs; 2,583.921 s | 91,816 / 91,820 (99.996%) | 95,975 / 95,980 (99.995%) | 6,714 / 6,714 (100%) | 102,921 / 0 / 0 (99.995%) |

Each platform retains **4 host-capability exclusions** (the two modes of each Atomics.wait fixture that requires `CanBlockIsFalse`) and **1 stale corpus mode** (`annexB/language/function-code/block-decl-func-skip-arguments.js`). These five dispositions remain in the raw pass-rate denominator and are not counted as passes. Ubuntu and Windows each match all **102,926 macOS outcome contracts with zero changes**. Passing this pinned inventory and its coverage gates does not establish complete ECMAScript conformance; the [edition track](development/browser_core/phase-13-bluejs-engine/ECMASCRIPT_2026.md) records that broader scope.

### Current test suites (2026-10-12–13)

| Platform | BlueJS Rust: pass / fail / ignored (335 targets) | Workspace runtime: pass / fail / ignored | BlueJS Rustdoc: pass / fail | Self-test tooling: pass / fail |
| --- | ---: | ---: | ---: | ---: |
| macOS | 4,169 / 0 / 4 | 2,170 / 0 / 60 | 2 / 0 | 85 / 0 |
| Ubuntu | 4,170 / 0 / 4 | 2,170 / 0 / 60 | 2 / 0 | 85 / 0 |
| Windows | 4,169 / 0 / 4 | 1,385 / 0 / 60 | 2 / 0 | 85 / 0 |

All three runs passed task Rust formatting, whitespace checks and workspace Clippy with `-D warnings`. Windows now runs both process-host boundary tests, so every BlueJS Rust target has the same pass/fail/ignored counts as macOS. Ubuntu's extra case checks its native stack initialization-failure path.

### Current BlueJS coverage

Fresh atomic LLVM profiles from each complete Rust and Test262 run establish **100% raw line, function and region coverage for all 169 instrumented BlueJS files**, with zero per-file coverage percentage regressions.

| Platform | Raw lines: covered / instrumented | Raw functions: covered / instrumented | Raw regions: covered / instrumented | Complete instrumented files |
| --- | ---: | ---: | ---: | ---: |
| macOS | 81,516 / 81,516 (100%) | 5,780 / 5,780 (100%) | 135,725 / 135,725 (100%) | 169 / 169 |
| Ubuntu | 81,546 / 81,546 (100%) | 5,784 / 5,784 (100%) | 135,781 / 135,781 (100%) | 169 / 169 |
| Windows | 81,523 / 81,523 (100%) | 5,781 / 5,781 (100%) | 135,735 / 135,735 (100%) | 169 / 169 |

Each platform report retains one complete 186-row source table, including the 17 declaration/test sources in the reviewed no-counter manifest: [macOS](development/browser_core/phase-13-bluejs-engine/TEST262_MACOS_REPORT.md#bluejs-per-file-coverage), [Ubuntu](development/browser_core/phase-13-bluejs-engine/TEST262_LINUX_REPORT.md#bluejs-per-file-coverage) and [Windows](development/browser_core/phase-13-bluejs-engine/TEST262_WINDOWS_REPORT.md#bluejs-per-file-coverage). Completion uses raw summaries and maximum-per-definition region reconciliation. Platform compilation accounts for the counter-denominator differences; the audits confirm zero coverage percentage regressions.

Coverage and Test262 pass rates measure different things. The CI line floors remain 90% for the workspace and 88% for BlueJS; full workspace coverage was not remeasured in these October runs. The [testing policy](development/browser_core/testing/TEST_PLAN.md) records the gate scope.

### ECMA-402 breakdown and Temporal

Every `intl402/` service passes in full on the platforms measured, including `Temporal/` (4,058 of the 6,714 `intl402/` modes). `Temporal/` is ECMA-262 (a core language built-in, like `Date`), not an ECMA-402 service, and is scoped as its own effort — [Phase 26](development/browser_core/phase-26-ecma262-temporal/PLAN.md) — separate from ECMA-402's [Phase 25](development/browser_core/phase-25-ecma402-internationalization/PLAN.md). Test262 also has a larger, separate `built-ins/Temporal/` tree (9,210 modes) that is excluded from the `intl402/`-scoped column above but is Temporal's primary test surface.

| Platform | `intl402/` non-`Temporal` (11 services) | `intl402/Temporal/` | Combined Temporal (`built-ins/` + `intl402/`) |
| --- | ---: | ---: | ---: |
| macOS, 2026-10-12 | 2,656 / 2,656 (100%) | 4,058 / 4,058 (100%) | 13,268 / 13,268 (100%) |
| Ubuntu 24.04.4 LTS, 2026-10-13 | 2,656 / 2,656 (100%) | 4,058 / 4,058 (100%) | 13,268 / 13,268 (100%) |
| Windows 11 VM, 2026-10-13 | 2,656 / 2,656 (100%) | 4,058 / 4,058 (100%) | 13,268 / 13,268 (100%) |

The breakdown comes from the same unfiltered complete inventory as the first table. The full per-service and per-Temporal-type breakdown, provenance and reproduction commands are in the [Ubuntu report](development/browser_core/phase-13-bluejs-engine/TEST262_LINUX_REPORT.md), the [macOS report](development/browser_core/phase-13-bluejs-engine/TEST262_MACOS_REPORT.md) and the [Windows report](development/browser_core/phase-13-bluejs-engine/TEST262_WINDOWS_REPORT.md).

Design and planning documents live under [`development/`](development/); it is not source code. Each subdirectory covers one major component of the project, following the same design-first workflow: a plan is drafted before implementation starts, and updated as the design evolves.

- **[`development/browser_core/`](development/browser_core/)** — the browser engine itself. See [`BROWSER_CORE_PLAN.md`](development/browser_core/BROWSER_CORE_PLAN.md) for the current plan, open design decisions, and progress tracking.

## License

BlueIce is licensed under the [Mozilla Public License 2.0](LICENSE) (MPL-2.0).

Code adapted from Gecko stays MPL-2.0 (an inherent MPL obligation). Code adapted from Chromium/Blink (BSD-3-Clause) is relicensed to MPL-2.0 with the original BSD notice preserved. See the plan document above for details on how this applies, and on the project's position on patents and trademarks of the projects it references.
