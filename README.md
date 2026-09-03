# BlueIce

An AI-driven web browser built from scratch in Rust. The goal is a single rendering engine where a human user and an AI agent perceive the same page state from the same render pass, rather than driving a separate browser instance through external automation (e.g. Puppeteer/CDP) — which is subject to bot-detection differentiation and state drift between the two instances.

BlueIce references Firefox (Gecko) and Chromium (Blink/V8) as technical references for the rendering pipeline.

## Project status

Pre-implementation. The plan below is a first draft, meant to be refined as work begins — expect it to change.

Design and planning documents live under [`development/`](development/); it is not source code. Each subdirectory covers one major component of the project, following the same design-first workflow: a plan is drafted before implementation starts, and updated as the design evolves.

- **[`development/browser_core/`](development/browser_core/)** — the browser engine itself. See [`BROWSER_CORE_PLAN.md`](development/browser_core/BROWSER_CORE_PLAN.md) for the current plan, open design decisions, and progress tracking.

## License

BlueIce is licensed under the [Mozilla Public License 2.0](LICENSE) (MPL-2.0).

Code adapted from Gecko stays MPL-2.0 (an inherent MPL obligation). Code adapted from Chromium/Blink (BSD-3-Clause) is relicensed to MPL-2.0 with the original BSD notice preserved. See the plan document above for details on how this applies, and on the project's position on patents and trademarks of the projects it references.
