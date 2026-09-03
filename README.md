# BlueIce

[![CI](https://github.com/ephoton0210/blueice/actions/workflows/ci.yml/badge.svg)](https://github.com/ephoton0210/blueice/actions/workflows/ci.yml)

An AI-driven web browser built from scratch in Rust. The goal is a single rendering engine where a human user and an AI agent perceive the same page state from the same render pass, rather than driving a separate browser instance through external automation (e.g. Puppeteer/CDP) — which is subject to bot-detection differentiation and state drift between the two instances.

BlueIce references Firefox (Gecko) and Chromium (Blink/V8) as technical references for the rendering pipeline.

## Project status

Early implementation. The Cargo workspace is scaffolded under [`backend/`](backend/) per the process architecture in the plan below (`core`/`extension` split, `frontend` to follow); `backend/core/dom` has a real, tested implementation, the rest of the pipeline (`html`/`css`/`layout`/`paint`) is still stubbed. See [`development/browser_core/testing/TEST_PLAN.md`](development/browser_core/testing/TEST_PLAN.md) for the test/coverage policy the CI badge above enforces.

Design and planning documents live under [`development/`](development/); it is not source code. Each subdirectory covers one major component of the project, following the same design-first workflow: a plan is drafted before implementation starts, and updated as the design evolves.

- **[`development/browser_core/`](development/browser_core/)** — the browser engine itself. See [`BROWSER_CORE_PLAN.md`](development/browser_core/BROWSER_CORE_PLAN.md) for the current plan, open design decisions, and progress tracking.

## License

BlueIce is licensed under the [Mozilla Public License 2.0](LICENSE) (MPL-2.0).

Code adapted from Gecko stays MPL-2.0 (an inherent MPL obligation). Code adapted from Chromium/Blink (BSD-3-Clause) is relicensed to MPL-2.0 with the original BSD notice preserved. See the plan document above for details on how this applies, and on the project's position on patents and trademarks of the projects it references.
