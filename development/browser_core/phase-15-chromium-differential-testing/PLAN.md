# Phase 15 — Chromium Differential Testing

[← Back to plan](../BROWSER_CORE_PLAN.md)

**Status**: In progress — the full local pipeline (BlueIce capture, Chromium capture, diff/report) works end to end (`npm run differential`); CI wiring and corpus curation are what's left. See "Staged build-out" and "First real results" below.

## Objective

Give BlueIce's rendering pipeline an independent correctness oracle — a real Chromium, driven by Puppeteer, rendering the same input BlueIce does, with the two outputs diffed — the same "don't trust your own test suite to grade itself" principle Phase 13 already established for BlueJS against Node.js, applied to rendering instead of script execution. Full design: `testing/TEST_PLAN.md`'s "Chromium differential testing" section; this document tracks the concrete build-out.

Raised out of the numbered sequence (like Phase 14 before it): needed *now*, once Phase 5's API made BlueIce drivable at all, rather than waiting for this phase's original place after Phases 6-11 — the earlier BlueIce is checked against a real browser, the more of the remaining work benefits from that signal, not less.

## Why this needed two new capabilities first

A Puppeteer harness comparing BlueIce against Chromium needs to get two things out of BlueIce: a screenshot and the DOM tree. Screenshots already existed (`blueice-raster`'s `Pixmap::save_png`); the DOM did not have an *external* interface — `blueice_dom::dump`'s canonical text format existed only inside `blueice-testing`, a crate explicitly scoped as test-only infrastructure, unreachable from a live `core` process. Closing that gap, done as part of this phase:

- **`blueice_dom::dump`** (moved out of `blueice-testing`, which now re-exports it as `dump_dom` for its existing fixture-corpus call sites) — the DOM crate producing its own canonical dump is a general capability of the crate that owns the data structure, the same relationship `blueice-paint`'s public `dump_frame` already has to *its* data structure, not something that belonged bolted onto a test-only crate.
- **`ClientMessage::GetDom` / `ServerMessage::Dom(String)`** (`blueice-ipc`), wired through `Page::dom_dump` and `blueice_engine::session`.
- **`get_dom` and `screenshot` MCP tools** (`backend/mcp-server`) — the harness's actual entry points, verified manually against the real compiled `blueice-mcp-server` binary: `get_dom` on `https://example.com` returns the full tree including the wrapping `<div>` that `get_page_representation`'s AI-facing snapshot correctly excludes (proving the two are genuinely different views, not the same data twice); `screenshot` returns a real, valid PNG of the live-fetched page.

**Why `GetDom` is a separate message from `GetRepresentation`, not a mode of it**: `phase-1-ai-representation-layer/spike.md`'s exclusion rule (purely decorative/non-semantic nodes absent from the AI-facing tree) is *correct* for that tree's purpose but would make a structural DOM diff against Chromium meaningless for exactly the content most likely to differ (generic wrapper `<div>`s, `<span>`s) — differential testing needs the unfiltered tree.

## Design (settled, see `TEST_PLAN.md` for the full writeup)

- **Corpus**: the existing shared fixture corpus (`development/browser_core/testing/fixtures/*.dat`) — no separately curated page set. Every fixture's `#data` HTML is fed to both Chromium and BlueIce.
- **Comparison, not exact match**: BlueIce's MVP scope is deliberately narrower than Chromium's, so the harness tracks a diff trend (screenshot pixel/perceptual diff, DOM structural diff) rather than gating on exact equality — mirrors Phase 13's own explicit non-goal for BlueJS-vs-Node performance parity.
- **Talks to BlueIce over MCP**, not a bespoke IPC client — `blueice-mcp-server`'s `navigate`/`screenshot`/`get_dom` tools, which doubles as an automated integration check of the MCP surface itself (beyond the manual verification Phase 12 has done so far).
- **Node.js**, not Rust, for the harness itself — Puppeteer needs a real Node runtime; this is the same exception CI's planned `bluejs-differential` job already carves out.

## Staged build-out

Built in two stages, on request — the BlueIce-driving half proven working on its own before adding Chromium/Puppeteer on top of it, rather than debugging both sides of a comparison at once:

**Stage 1 (done): the Node.js harness, BlueIce side only.** `differential-testing/` — a Node.js project separate from the Cargo workspace (Puppeteer needs a real Node runtime, same exception `testing/TEST_PLAN.md`'s planned `bluejs-differential`/`chromium-differential` CI jobs already carve out). Built on the official `@modelcontextprotocol/sdk` TypeScript/JS package for the MCP client side, the same "reuse proven infrastructure" reasoning as `rmcp` on the Rust side:

- `src/fixtures.js` — a JS port of `blueice-testing`'s `.dat` corpus parser, kept in exact lockstep with the Rust parser's rules (section-marker regex, blank-line stripping, one fixture per `#data` marker) so this harness reads the identical corpus BlueIce's own fixture tests do, not a subtly different one.
- `src/serve-fixture.js` — a throwaway local HTTP server per fixture, since `blueice-net` only fetches `http://`/`https://` (no `file://`/`data:`) and a real Chromium tab will need an actual URL too, for the same comparison on both sides.
- `src/blueice-client.js` — spawns `blueice-mcp-server` and wraps its `navigate`/`get_dom`/`screenshot` tools.
- `src/capture-blueice.js` (`npm run capture:blueice`) — runs every `#data`-bearing fixture in the shared corpus through the above and writes `differential-testing/output/blueice/<fixture>/{dom.txt,screenshot.png}`.

Run and verified against all 28 current fixtures: every one captured a real DOM dump and a real, correctly-rendered PNG (spot-checked by eye) with zero failures.

**Stage 2 (done): Puppeteer capture + the actual diff.**

- `src/dump-dom-in-page.js` — a JS port of `blueice_dom::dump`'s exact algorithm (same traversal order, indentation, attribute sorting, comment/doctype exclusion, text quoting), run inside a live Chromium tab via `page.evaluate()`. Both sides produce output from *the same dump function design*, not two structurally different serializations needing their own normalization pass before they're even comparable.
- `src/capture-chromium.js` (`npm run capture:chromium`) — mirrors `capture-blueice.js` exactly (same corpus, same per-fixture local HTTP server, same output-directory shape), driving a real headless Chromium instead. Viewport fixed at 800×600 to match `blueice-mcp-server`'s `CoreProcess::spawn(800, 600)`, so the two sides' screenshots are pixel-comparable at all.
- `src/compare.js` (`npm run compare`) — diffs `output/blueice/` against `output/chromium/` per fixture: DOM via `diff`'s `diffLines` (an exact-match flag plus a line-similarity fraction for the cases that differ), screenshots via `pixelmatch` (a match-fraction plus a written diff image highlighting exactly which pixels differ). Writes `output/report.json` (full per-fixture data) and a console summary table. Deliberately never fails/exits nonzero on a low similarity score — per "not exact parity" above, this is a report, not a pass/fail test.
- `npm run differential` runs all three in sequence.

## First real results

Running `npm run differential` against all 28 current fixtures: **27/28 DOM-identical, 99.9% average DOM line similarity, 99.8% average pixel match.** The one fixture that differs (`demo.dat`, the richest one — headings, a list, a form) shows real, legible, already-understood gaps in the diff image (`output/diff/demo.dat_0/screenshot-diff.png`): Chromium renders list bullets and a visible input-box border BlueIce's MVP scope doesn't yet, uses its default serif body font against BlueIce's bundled DejaVu Sans (sans-serif), and applies `:link` blue/underline styling BlueIce's MVP selector list doesn't include — every one of these is an already-known, already-decided MVP scope boundary (`phase-2-mvp-scope/PLAN.md`'s deferred selectors list explicitly excludes `:link`/`:visited`), not a bug this pass discovered. This is exactly the kind of signal the phase exists to produce: a real, quantified baseline instead of an assumption.

## Checklist

- [x] Design the comparison strategy (what's compared, corpus source, not-exact-match philosophy) — see `TEST_PLAN.md`
- [x] Add the DOM-tree capability the harness needs but didn't exist externally — `blueice_dom::dump`, `ClientMessage::GetDom`, `get_dom` MCP tool
- [x] Confirm the screenshot capability the harness needs — `screenshot` MCP tool, verified against a real live-fetched page
- [x] Write the Node.js harness's BlueIce-driving half (stage 1) — `differential-testing/`, verified against all 28 current fixtures
- [x] Add the Puppeteer/Chromium-driving half (stage 2) — `capture-chromium.js`, same corpus and output shape as stage 1
- [x] Implement the screenshot diff (pixel/perceptual) and the DOM structural diff — `compare.js`, `pixelmatch` + `diff`, with a written visual diff image per fixture
- [ ] Decide and implement how the diff trend is tracked *across runs* (a baseline file committed to the repo, so a regression is visible in a PR diff? a job-summary-only report with no persisted history? — `report.json` exists per-run today, but nothing compares one run's report to a prior one yet)
- [ ] Wire the `chromium-differential` CI job (`testing/TEST_PLAN.md`'s CI section) — needs Node.js plus a headless-Chromium-capable runner image; reports to the job summary, not gating the build
- [ ] Curate/extend the fixture corpus if differential testing surfaces gaps the current corpus doesn't exercise (none found yet — the one real divergence found, `demo.dat`, was already-known MVP scope, not a new gap)
