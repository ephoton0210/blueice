# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project status

**Phase 3 (Rust core engine skeleton) is done.** Every pipeline crate under `backend/core/` is a real implementation, end to end — HTML in, a paint-command display list out:

- `backend/core/dom` — the DOM tree (node identity, tree structure).
- `backend/core/html` — HTML tokenizer and tree builder (parses into `blueice-dom` trees).
- `backend/core/css` — CSS tokenizer, selector matching, and cascade (DOM + stylesheets -> per-element `ComputedStyle`, including a built-in UA stylesheet).
- `backend/core/layout` — block/inline layout (DOM + `ComputedStyle` -> an immutable `Fragment` tree with real box-model/line-breaking geometry).
- `backend/core/paint` — a `Fragment` tree + `ComputedStyle` -> a flat, ordered paint-command display list (background/border rectangles, text runs) — not pixels yet; actual rasterization is a per-platform Phase 4 `frontend` concern.
- `backend/core/engine` — wires the above into one `render(html, css, viewport_width) -> Frame` entry point, plus (Phase 4) a stateful `Page` type and `session` message loop for interactive use.
- `backend/testing` — shared cross-stage test interface (fixture format + DOM dump serializer); see "Rendering-correctness fixtures" and "UI testing strategy" in `TEST_PLAN.md`.
- `backend/extension` — still a stub/placeholder, per plan §1's process-architecture scope.

**Phase 4 (human-visible rendering path) is done** (see `phase-4-human-rendering-path/PLAN.md`). `core` and `frontend` are real, separate OS processes:

- `backend/ipc` — the control-plane protocol (`ClientMessage`/`ServerMessage` over a length-prefixed-JSON Unix domain socket) and the frame-plane (`blueice_ipc::shm`, real `mmap`-backed frame files).
- `backend/net` — HTTP fetching for navigation (`ureq`-based).
- `backend/core/font` — the one place fonts are loaded and measured (bundled DejaVu Sans, plus a Noto Sans TC fallback face for CJK glyphs DejaVu lacks — `font_for_char`), shared by `blueice-layout` (real word-width measurement) and `blueice-raster` (glyph rasterization), so the two never disagree about how wide a run of text is.
- `backend/i18n` — `blueice-i18n`'s namespace/key UI-text lookup (Fluent-backed, `en` default + `zh-TW` today), per `phase-14-i18n-localization/PLAN.md`. Every UI-facing string (the credits screen, `frontend`'s window title) goes through this rather than being a hardcoded literal.
- `backend/core/raster` — `blueice-paint`'s display list -> actual RGBA8 pixels (`fontdue`-based glyph rendering).
- `backend/core/engine/src/bin/blueice-core.rs` — the `core` process binary: a Unix-socket server owning one `Page`, driven by `blueice_engine::session`.
- `backend/frontend-reference` — the reference `frontend` binary (`winit` + `softbuffer`), verified manually in a real window (resize, click-driven navigation, no rendering defects) since GUI event loops can't run in headless CI.

**Phase 5 (AI representation output path) is done** (see `phase-5-ai-representation-output/PLAN.md`; the `protocol_version` handshake `phase-1-ai-representation-layer/PLAN.md` also decided is a recorded, deferred follow-up, not yet needed with only one `core`/`frontend` pair). `blueice_engine::ai_snapshot` extracts the Phase 1 accessibility-tree-shaped schema (`blueice_ipc::{AiSnapshot, AiNode, Role, ...}`) from the exact same `Page` state Phase 4's `render()` paints from, reachable over the same `blueice-ipc` control-plane protocol `frontend` already uses (`ClientMessage::GetRepresentation`/`ActOn`/`Highlight`/`Hover`, `ServerMessage::Representation`) rather than a separate channel. Elements are addressed by their stable `blueice_dom::NodeId` (`ActOn` resolves an ID to current bounds internally, never raw coordinates); a `Representation` and the `FrameReady` from the state change immediately before it always share the same `generation` number, the checkable version of "human and AI perceive the same render pass."

Build/lint/test commands (see `development/browser_core/testing/TEST_PLAN.md` for the full testing policy):

- Build: `cargo build --workspace --all-targets`
- Test: `cargo test --workspace`
- Lint: `cargo clippy --workspace --all-targets -- -D warnings`
- Coverage gate (crates with a real implementation must hold ≥90% line coverage; the placeholder `extension` process and the windowed reference frontend's `main.rs` -- GUI wiring with no headless-CI display to run against, its pure helpers are still unit tested -- are excluded and only reported for visibility): `cargo llvm-cov --workspace --ignore-filename-regex 'extension/src/main\.rs$|frontend-reference/src/main\.rs$' --fail-under-lines 90 --summary-only`

All of the above run in CI on every push/PR to `main` (`.github/workflows/ci.yml`). Design documentation for both implemented and not-yet-implemented parts still lives under `development/` — keep it in sync as each phase's own Definition of Done, not as an afterthought.

**Definition of Done** (full policy: `development/browser_core/testing/TEST_PLAN.md`): no design or feature counts as done merely because it's settled or compiles.

- **Develop test-first (TDD)**: write the failing test before the implementation that makes it pass — not tests bolted on after the feature is already written.
- It needs passing tests; where an end-to-end path through the feature's real public interface exists, that path must pass too — not just tests of internal modules.
- The public interface itself must be complete enough for tests to drive without reaching into `pub(crate)`/private internals.
- Real-implementation crates must clear the ≥90% line-coverage gate.
- **Once implementation is otherwise complete, do a dedicated test-review pass**: re-read the suite's actual content (not just pass/fail or the coverage number), fill in cases TDD's incremental cycles didn't surface, update any test whose expectation no longer matches intended behavior, and delete tests that no longer check anything meaningful.

This applies to every phase's checklist, not only to crates that already meet it.

## Core goal

BlueIce is a browser engine, written from scratch in Rust, built around one idea: a human user and an AI agent should perceive the same page state from the **same render pass**, instead of the common dual-track setup where a human uses a real browser and an AI drives a separate headless instance via external automation (e.g. Puppeteer/CDP). That dual-track approach is rejected for architectural reasons, not just performance:

- Anti-bot systems (Cloudflare, Akamai, PerimeterX) fingerprint headless/CDP Chromium and can serve it different content than a human sees.
- A separate driven instance has its own viewport/timing/JS state, so "what the AI saw" is only ever an approximation of "what the human saw."
- Any AI-facing capability (e.g. semantic tags alongside human-facing highlights) has to be a post-hoc hack in an externally-driven architecture; owning the render pipeline makes it a first-class feature instead.

This is not a clean-room implementation — Gecko (Firefox) and Blink/V8 (Chromium) source are read directly as technical reference and porting basis. See `development/browser_core/BROWSER_CORE_PLAN.md` for full detail.

Two concrete requirements follow from the core goal (plan §1) and apply across all phases:

- The AI must be able to show/hide the human-facing window at runtime without restarting the engine — same instance, same render pass, same JS state, regardless of window visibility.
- Every DOM node has an explicit, stable ID assigned at creation (not an array index or ephemeral pointer), so the AI-facing representation can reference elements reliably across mutations.

## Design-first workflow

Plans are drafted under `development/` *before* implementation starts, and updated as the design evolves — treat these as living documents, not historical records. Each major component gets its own subdirectory with a plan doc. Currently the only component is `development/browser_core/BROWSER_CORE_PLAN.md` (the engine itself).

**Settled**: which layer the human and AI representations are shared at (plan §3) — an accessibility-tree-shaped schema (role, state, provenance-tagged name, bounds) extended with `opacity`, `animating`, `occluded`/`occludedBy`/`occludedFraction`, and per-node `hovered`/`focused`, keyed by BlueIce's own stable `NodeId`. Resolved directly from how Gecko/Blink already do this in production (`research/accessibility-tree.md`), not from an independent validation exercise — check plan §3 for the current schema rather than assuming one of the original four candidates in isolation.

**MVP scope** (per plan §4) is deliberately narrow: a minimal HTML parse → DOM → CSS cascade → layout → paint pipeline, plus the shared human/AI representation layer. Explicitly out of scope for now: extension ecosystem, multi-tab state sync, full JS engine optimization, DevTools.

## Licensing (load-bearing, not boilerplate)

Project-wide license is MPL-2.0, chosen specifically because the project reads and adapts Gecko/Chromium source rather than clean-rooming:

- Files derived from Gecko **must** stay MPL-2.0 — this is an inherent MPL file-level copyleft obligation, not a project choice.
- Files derived from Chromium/Blink (BSD-3-Clause) are relicensed to MPL-2.0, but the original BSD copyright/disclaimer notice must be preserved verbatim in that file's own header — the project uses per-file headers only, no centralized `NOTICE`/`THIRD_PARTY_LICENSES` file. See `CONTRIBUTING.md` for the exact header templates (original code, Gecko-derived, Chromium/Blink-derived) before porting any file.
- BSD-3-Clause's binary-distribution clause is separate from the source-header requirement above and isn't satisfied by it: any distributed binary needs the Chromium notice reproduced in "documentation and/or other materials provided with the distribution" — i.e. a Help/About/Credits screen. Tracked as a Phase 4 checklist item (`development/browser_core/phase-4-human-rendering-path/PLAN.md`), since there's no UI to put it in yet.
- Wholly original code (the AI representation layer, project glue) uses MPL-2.0 for consistency only, with no derivative-work obligation.
- The license places no field-of-use/commercial restriction (that would violate the OSI Open Source Definition) — "no commercial activity" is a project stance, not something encoded in the license terms.
- MPL §2.1's patent grant only covers contributors' own contributions to this project, not unrelated third-party patents (codecs, GPU rendering, JIT techniques). This risk doesn't disappear at non-commercial stage.
- Do not use "Firefox", "Mozilla", "Chrome", "Google Chrome", or "Chromium" names/logos as BlueIce branding. Descriptive references ("BlueIce references Gecko's approach to X") are fine as nominative fair use.

## Naming

The brand name is written **BlueIce** (capital I) in prose and docs. The lowercase `blueice` is reserved for technical identifiers only — the GitHub repo slug (`ephoton0210/blueice`) and filesystem paths — not for prose.
