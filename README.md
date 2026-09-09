# BlueIce

[![CI](https://github.com/ephoton0210/blueice/actions/workflows/ci.yml/badge.svg)](https://github.com/ephoton0210/blueice/actions/workflows/ci.yml)

An AI-driven web browser built from scratch in Rust. The goal is a single rendering engine where a human user and an AI agent perceive the same page state from the same render pass, rather than driving a separate browser instance through external automation (e.g. Puppeteer/CDP) — which is subject to bot-detection differentiation and state drift between the two instances.

BlueIce references Firefox (Gecko) and Chromium (Blink/V8) as technical references for the rendering pipeline.

## Project status

The core rendering pipeline is complete end to end — HTML in, real pixels out — with `core` and a reference `frontend` running as separate OS processes over a Unix-socket IPC protocol (plus a shared-memory frame-plane for pixel delivery). The AI-facing representation (an accessibility-tree-shaped schema, addressed by stable node IDs) is extracted from the exact same render pass a human sees, reachable both through the reference `frontend` and through an MCP server (`blueice-mcp-server`) that lets any MCP-compatible AI agent drive BlueIce directly; a `blueice-launcher` process lets a human's `frontend` and an AI's `mcp-server` observe and act on *one* running `core` instance simultaneously — the literal goal this project exists for.

Also implemented: multi-tab support (`core`-side and MCP-exposed), a fleet-wide process/memory supervisor, i18n/localization, a Chromium differential-testing pipeline, the minimal-slice mechanism for a local AI safety gatekeeper — every navigation is reviewed by a separate always-resident process before it takes effect, non-blocking with respect to other tabs/clients, fail-closed if that process is unreachable; the gatekeeper's real review logic (rule-base + AI) is still to come — the minimal-slice mechanism for the extension protocol's server-side capability enforcement (a hardcoded single extension proven end to end over a real process boundary; the WASM runtime and manifest parser are still to come), a minimal-slice live `core` hot-swap: `blueice-launcher` can swap the running `core` for a freshly-spawned one at runtime, replaying open tabs into it, with no already-connected client ever dropped (automatic update-detection/scheduling is still to come); and, for BlueJS (the project's own from-scratch JavaScript engine, chosen over embedding an existing one so the safety gatekeeper can eventually hook script execution directly), design is fully resolved — execution model, GC, event loop, out-of-process placement, and the MVP language subset — and the front end is real: a hand-written tokenizer and recursive-descent parser (`backend/bluejs`) producing an AST for that language subset, alongside the first runtime-storage slice: ordinary data-property objects with stable handles, explicit GC roots, prototype lookup, and nursery/tenured garbage collection with a managed-data budget. Arrays/functions, the bytecode compiler/interpreter, and event loop are still to come. Still design-only or not started: a download manager. See [`BROWSER_CORE_PLAN.md`](development/browser_core/BROWSER_CORE_PLAN.md) for the full phase-by-phase progress table, or [`CLAUDE.md`](CLAUDE.md) for a denser, continuously-updated status summary. See [`development/browser_core/testing/TEST_PLAN.md`](development/browser_core/testing/TEST_PLAN.md) for the test/coverage policy the CI badge above enforces.

Design and planning documents live under [`development/`](development/); it is not source code. Each subdirectory covers one major component of the project, following the same design-first workflow: a plan is drafted before implementation starts, and updated as the design evolves.

- **[`development/browser_core/`](development/browser_core/)** — the browser engine itself. See [`BROWSER_CORE_PLAN.md`](development/browser_core/BROWSER_CORE_PLAN.md) for the current plan, open design decisions, and progress tracking.

## License

BlueIce is licensed under the [Mozilla Public License 2.0](LICENSE) (MPL-2.0).

Code adapted from Gecko stays MPL-2.0 (an inherent MPL obligation). Code adapted from Chromium/Blink (BSD-3-Clause) is relicensed to MPL-2.0 with the original BSD notice preserved. See the plan document above for details on how this applies, and on the project's position on patents and trademarks of the projects it references.
