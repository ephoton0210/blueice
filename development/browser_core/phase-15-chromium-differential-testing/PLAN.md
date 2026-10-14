# Phase 15 — Chromium Differential Testing

[← Back to plan](../BROWSER_CORE_PLAN.md)

**Status**: Complete — the local and CI pipelines capture BlueIce and Chromium, compare DOM/screenshots, publish an informational cross-run trend against the committed baseline, and retain artifacts. The shared corpus includes author stylesheets, a script-bearing page, and comment-adjacent text coverage. See "Staged build-out" and "First real results" below.

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
- `src/blueice-client.js` — spawns `blueice-mcp-server`, unwraps its stable untrusted-page-content envelope before parsing machine data, and wraps its `navigate`/`get_dom`/`screenshot` tools.
- `src/blueice-runtime.js` — reuses an already-running gatekeeper/launcher if present; otherwise starts the fail-closed gatekeeper and a launcher (therefore `core` plus its BlueJS sibling) as one owned process group and reaps only that group after capture. This makes a clean checkout run just as fully as an existing user session, including document-script execution.
- `src/capture-blueice.js` (`npm run capture:blueice`) — runs every `#data`-bearing fixture in the shared corpus through the above and writes `differential-testing/output/blueice/<fixture>/{dom.txt,screenshot.png}`.

Run and verified against all 30 current fixtures: every one captured a real DOM dump and a real, correctly-rendered PNG with zero failures.

**Stage 2 (done): Puppeteer capture + the actual diff.**

- `src/dump-dom-in-page.js` — a JS port of `blueice_dom::dump`'s exact algorithm (same traversal order, indentation, attribute sorting, comment/doctype exclusion, text quoting), run inside a live Chromium tab via `page.evaluate()`. Both sides produce output from *the same dump function design*, not two structurally different serializations needing their own normalization pass before they're even comparable.
- `src/capture-chromium.js` (`npm run capture:chromium`) — mirrors `capture-blueice.js` exactly (same corpus, same per-fixture local HTTP server, same output-directory shape), driving a real headless Chromium instead. Viewport fixed at 800×600 to match the launcher's `core`, so the two sides' screenshots are pixel-comparable at all.
- `src/fixtures.js`'s `htmlForFixture` feeds both browsers the same complete input: `#data` plus any shared-fixture `#css` section embedded as an author `<style>` element. A Rust fixture's stylesheet is therefore no longer silently absent from the independent comparison.
- `src/compare.js` (`npm run compare`) — diffs `output/blueice/` against `output/chromium/` per fixture: DOM via `diff`'s `diffLines` (an exact-match flag plus a line-similarity fraction for the cases that differ), screenshots via `pixelmatch` (a match-fraction plus a written diff image highlighting exactly which pixels differ). Writes `output/report.json` (including platform/architecture) and a console summary table. Deliberately never fails/exits nonzero on a low similarity score — per "not exact parity" above, this is a report, not a pass/fail test.
- `npm run differential` runs capture, comparison, and trend rendering in sequence.

## Cross-run trend and CI

`differential-testing/baseline.json` is a versioned, committed snapshot of the full report. `npm run trend` compares a fresh `output/report.json` to it and writes human-readable `output/trend.md`: aggregate DOM/pixel metrics, corpus additions/removals, and material per-fixture changes. A handful of antialiased pixels is intentionally omitted from the per-fixture table; exact metrics remain in JSON. Similarity never sets a failing exit code.

Updating that snapshot is deliberately explicit: after reviewing an intended change, run `npm run baseline:update` and commit the resulting `baseline.json`; an ordinary differential run never moves its own reference point. The current committed baseline is 30/30 DOM-identical with 100.00% average DOM similarity and 99.68% average pixel match on the recorded macOS/arm64 run.

`.github/workflows/ci.yml` now has a separate `chromium-differential` job. It installs the lockfile, explicitly downloads Chrome for Testing (rather than relying on an npm lifecycle hook), builds the BlueIce subprocesses, runs the Node helper tests and `npm run differential`, writes `trend.md` to the GitHub job summary, and uploads the generated report/diff artifacts. A broken harness still fails its own job; a lower rendering similarity remains informational rather than a build gate.

## First real results

Running `npm run differential` against all 28 current fixtures initially found **27/28 DOM-identical, 99.9% average DOM line similarity, 99.8% average pixel match** — and the one exception was a genuine `blueice-html` bug, not an MVP-scope gap: whitespace text either side of `</body>` (and, more generally, any two character-token runs separated only by a dropped comment token) landed as two adjacent sibling Text nodes instead of merging into one, per HTML5's "insert a character" algorithm. Invisible in any rendered output (pure whitespace either way), which is exactly why no fixture's `#paint`/`#layout` section had caught it — found specifically because this phase does a byte-level structural DOM comparison against a real, spec-compliant engine. **Fixed**: `blueice-html`'s `insert_text` (and `blueice-dom` gained `last_child`/`prev_sibling` accessors to support it) now checks whether the insertion point already ends with a Text node and appends to it rather than always creating a new one, on both the ordinary and foster-parenting insertion paths. Re-running the harness after the fix: **28/28 DOM-identical.**

The committed 30-fixture baseline is **30/30 DOM-identical, 100.00% average DOM similarity, and 99.68% average pixel match**. The largest current visual deltas are `paint.dat#0` (96.2%), the `currentColor` stylesheet case in `css.dat#3` (96.8%), and `demo.dat#0` (98.4%). `demo` still represents understood MVP differences — list markers/input chrome, default font, and `:link` styling — while the first two now have a durable independent signal for future rendering work. This is exactly the kind of signal the phase exists to produce: a real, quantified baseline distinguishing a scope boundary from a regression instead of assuming either.

## Checklist

- [x] Design the comparison strategy (what's compared, corpus source, not-exact-match philosophy) — see `TEST_PLAN.md`
- [x] Add the DOM-tree capability the harness needs but didn't exist externally — `blueice_dom::dump`, `ClientMessage::GetDom`, `get_dom` MCP tool
- [x] Confirm the screenshot capability the harness needs — `screenshot` MCP tool, verified against a real live-fetched page
- [x] Write the Node.js harness's BlueIce-driving half (stage 1) — `differential-testing/`, including clean-run gatekeeper/launcher/BlueJS lifecycle, verified against all 30 current fixtures
- [x] Add the Puppeteer/Chromium-driving half (stage 2) — `capture-chromium.js`, same corpus and output shape as stage 1
- [x] Implement the screenshot diff (pixel/perceptual) and the DOM structural diff — `compare.js`, `pixelmatch` + `diff`, with a written visual diff image per fixture
- [x] Track the diff trend across runs — committed `baseline.json`, explicit `baseline:update`, and informational `trend.md` with focused Node tests
- [x] Wire the `chromium-differential` CI job — lockfile Node setup, explicit Chrome for Testing download, job summary, and downloadable artifacts; similarity does not gate the build
- [x] Curate/extend the fixture corpus for gaps the original run did not exercise — `differential.dat` checks first-paint inline-script execution and comment-adjacent text merging; the harness now includes every fixture's `#css` author stylesheet
