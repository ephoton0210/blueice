# BlueIce

[![CI](https://github.com/ephoton0210/blueice/actions/workflows/ci.yml/badge.svg)](https://github.com/ephoton0210/blueice/actions/workflows/ci.yml)

An AI-driven web browser built from scratch in Rust. The goal is a single rendering engine where a human user and an AI agent perceive the same page state from the same render pass, rather than driving a separate browser instance through external automation (e.g. Puppeteer/CDP) — which is subject to bot-detection differentiation and state drift between the two instances.

BlueIce references Firefox (Gecko) and Chromium (Blink/V8) as technical references for the rendering pipeline.

## Project status

The core rendering pipeline is complete end to end — HTML in, real pixels out — with `core` and a reference `frontend` running as separate OS processes over a Unix-socket IPC protocol (plus a shared-memory frame-plane for pixel delivery). The AI-facing representation (an accessibility-tree-shaped schema, addressed by stable node IDs) is extracted from the exact same render pass a human sees, reachable both through the reference `frontend` and through an MCP server (`blueice-mcp-server`) that lets any MCP-compatible AI agent drive BlueIce directly; a `blueice-launcher` process lets a human's `frontend` and an AI's `mcp-server` observe and act on *one* running `core` instance simultaneously — the literal goal this project exists for.

Also implemented: multi-tab support (`core`-side and MCP-exposed), a fleet-wide process/memory supervisor, i18n/localization, a Chromium differential-testing pipeline, the minimal-slice mechanism for a local AI safety gatekeeper — every navigation is reviewed by a separate always-resident process before it takes effect, non-blocking with respect to other tabs/clients, fail-closed if that process is unreachable; the gatekeeper's real review logic (rule-base + AI) is still to come — the minimal-slice mechanism for the extension protocol's server-side capability enforcement (a hardcoded single extension proven end to end over a real process boundary; the WASM runtime and manifest parser are still to come), a minimal-slice live `core` hot-swap: `blueice-launcher` can swap the running `core` for a freshly-spawned one at runtime, replaying open tabs into it, with no already-connected client ever dropped (automatic update-detection/scheduling is still to come); and, for BlueJS (the project's own from-scratch JavaScript engine, chosen over embedding an existing one so the safety gatekeeper can eventually hook script execution directly), design is fully resolved — execution model, GC, event loop, out-of-process placement, and the MVP language subset — and the first execution path is real: a hand-written tokenizer and recursive-descent parser (`backend/bluejs`) produce an AST, then a compiler emits stack-machine bytecode for a bounded VM. This slice executes primitive expressions, variables/block scopes, conditions/loops, ordinary objects and sparse arrays (including holes, indexed writes and length truncation) backed by stable handles, prototype lookup, and nursery/tenured GC with a managed-data budget. Public-pipeline tests and an opt-in Node.js differential corpus check execution, coercion and error behavior. Compiled functions/closures, String protocols, descriptors and iteration/spread now execute; remaining language semantics, general Array methods, event loop and browser integration are still to come. Still design-only or not started: a download manager. See [`BROWSER_CORE_PLAN.md`](development/browser_core/BROWSER_CORE_PLAN.md) for the full phase-by-phase progress table, or [`CLAUDE.md`](CLAUDE.md) for a denser, continuously-updated status summary. See [`development/browser_core/testing/TEST_PLAN.md`](development/browser_core/testing/TEST_PLAN.md) for the test/coverage policy the CI badge above enforces.

BlueJS targets [ECMAScript 2026 edition 17](development/browser_core/phase-13-bluejs-engine/ECMASCRIPT_2026.md), beyond its original MVP subset. Its [String implementation](development/browser_core/phase-13-bluejs-engine/STRING_BUILTINS.md) now includes all 35 core prototype methods, three statics and Annex B extensions, backed by lossless UTF-16, Unicode 17 normalization/casing, RegExp/Symbol protocols, iterators, compiled and bound replacement callbacks, object coercion and descriptors. RegExp.escape, bound constructors, instanceof/Symbol.hasInstance, Math constants/functions, abstract equality and property-presence are also implemented. String locale methods now use ICU-backed Intl.Collator, locale canonicalization and locale-sensitive casing; Intl.Locale supplies canonical locale objects, Unicode options, likely-subtag transforms and Locale-info queries. Test262 harness scripts can share classic-script globals and receive native descriptor/constructibility helpers. Regex compilation/matching run in a terminable helper process. The [Intl/deadline/Test262 report](development/browser_core/phase-13-bluejs-engine/INTL_CONFORMANCE.md) records the complete 102,926-mode inventory; the current platform-verification status is documented below. Rust 1.95's current no-exclusion BlueJS coverage measurement is 88.38% lines (an 88% CI floor), not 100%. The String inventory records resource limits and remaining language dependencies; neither coverage nor the Node oracle is a full Test262/ECMAScript conformance claim.

## Ubuntu Test262 baseline and deferred platform validation

The locally verified environment on **2026-09-19** is **Ubuntu 24.04.3 LTS
under WSL2** (`x86_64-unknown-linux-gnu`, Rust/Cargo 1.95.0, 12 logical CPUs).
It is not Ubuntu 24.04.4. On 2026-09-19,
`python3 backend/bluejs/test262/run.py --jobs 8` completed the unfiltered
53,582-file / 102,926-mode inventory in 685.257 seconds. macOS and Windows
are deliberately deferred until those environments are available; Ubuntu
results are never used to infer their parity.

Test262 does not provide an official "Core" switch, so the reports use explicit top-level-directory scopes: **ECMA-262 Core** is `language/` + `built-ins/` (91,820 modes); **complete ECMA-262 Test262 scope** adds `annexB/` and `staging/` (95,980 modes); and **ECMA-402** is `intl402/` (6,714 modes). `harness/` (232 modes) validates Test262 support code and is retained only in the all-inventory total. Every scope is calculated from the same unfiltered complete run, not from separately filtered invocations. The complete ECMA-262 scope is an inventory label, not an assertion that time-based staging/proposal tests belong to one published ECMA edition.

| Platform / evidence | ECMA-262 Core | Complete ECMA-262 scope | ECMA-402 | All modes: pass / fail / timeout |
| --- | ---: | ---: | ---: | ---: |
| Ubuntu 24.04.3 LTS WSL2 (2026-09-19; 8 jobs) | 86,304 / 91,820 (93.993%) | 89,631 / 95,980 (93.385%) | 6,364 / 6,714 (94.787%) | 96,203 / 6,714 / 9 (93.468%) |
| macOS | deferred — no current run | deferred — no current run | deferred — no current run | deferred — no current run |
| Windows | deferred — no current run | deferred — no current run | deferred — no current run | deferred — no current run |

The complete run recorded nine `timeout` records, zero `harness_error`
records, and 6,714 semantic failures. The 33-test BlueJS Intl integration
suite and 102-test Test262-host suite also pass. These are conformance progress
measurements, not a claim of full ECMAScript conformance. Exact scope,
provenance and rerun commands are in the [Ubuntu report](development/browser_core/phase-13-bluejs-engine/TEST262_LINUX_REPORT.md).

### ECMA-402 breakdown and Temporal

The ECMA-402 column above (`intl402/`) is not evenly distributed: every service other than the `Temporal/` subtree is at 100%; `Temporal/` alone (4,058 of the 6,714 `intl402/` modes) is what drags the aggregate down to ~43%. `Temporal/` is ECMA-262 (a core language built-in, like `Date`), not an ECMA-402 service, and is scoped as its own effort — [Phase 26](development/browser_core/phase-26-ecma262-temporal/PLAN.md) — separate from ECMA-402's [Phase 25](development/browser_core/phase-25-ecma402-internationalization/PLAN.md). Test262 also has a larger, separate `built-ins/Temporal/` tree (9,210 modes) that is correctly excluded from the `intl402/`-scoped column above but is Temporal's actual primary test surface.

| Platform | `intl402/` non-`Temporal` (11 services) | `intl402/Temporal/` | Combined Temporal (`built-ins/` + `intl402/`) |
| --- | ---: | ---: | ---: |
| Ubuntu 24.04.3 LTS WSL2, 2026-09-19 | 2,656 / 2,656 (100%) | 3,708 / 4,058 (91.375%) | 12,682 / 13,268 (95.583%) |
| macOS | deferred — no current run | deferred — no current run | deferred — no current run |
| Windows | deferred — no current run | deferred — no current run | deferred — no current run |

The breakdown comes from the same unfiltered complete inventory as the first
table; the non-Temporal ECMA-402 services remain 100%, while the current
Temporal implementation passes 95.583% of its combined surface. macOS and
Windows cells must be filled only by real runs on those platforms. The full
per-service and per-Temporal-type breakdown, provenance and reproduction
commands are in the [Ubuntu report](development/browser_core/phase-13-bluejs-engine/TEST262_LINUX_REPORT.md).

Design and planning documents live under [`development/`](development/); it is not source code. Each subdirectory covers one major component of the project, following the same design-first workflow: a plan is drafted before implementation starts, and updated as the design evolves.

- **[`development/browser_core/`](development/browser_core/)** — the browser engine itself. See [`BROWSER_CORE_PLAN.md`](development/browser_core/BROWSER_CORE_PLAN.md) for the current plan, open design decisions, and progress tracking.

## License

BlueIce is licensed under the [Mozilla Public License 2.0](LICENSE) (MPL-2.0).

Code adapted from Gecko stays MPL-2.0 (an inherent MPL obligation). Code adapted from Chromium/Blink (BSD-3-Clause) is relicensed to MPL-2.0 with the original BSD notice preserved. See the plan document above for details on how this applies, and on the project's position on patents and trademarks of the projects it references.
