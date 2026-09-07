# Phase 15 — Chromium Differential Testing

[← Back to plan](../BROWSER_CORE_PLAN.md)

**Status**: In progress (the BlueIce-side capabilities the harness needs are built and verified; the actual Puppeteer harness is not written yet)

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

## Checklist

- [x] Design the comparison strategy (what's compared, corpus source, not-exact-match philosophy) — see `TEST_PLAN.md`
- [x] Add the DOM-tree capability the harness needs but didn't exist externally — `blueice_dom::dump`, `ClientMessage::GetDom`, `get_dom` MCP tool
- [x] Confirm the screenshot capability the harness needs — `screenshot` MCP tool, verified against a real live-fetched page
- [ ] Write the actual Node.js/Puppeteer harness (per-fixture: load in Chromium, load via `blueice-mcp-server`, capture both artifacts)
- [ ] Implement the screenshot diff (pixel/perceptual, with a tolerance) and the DOM structural diff
- [ ] Decide and implement how the diff trend is tracked across runs (a baseline file committed to the repo? a job-summary-only report with no persisted history? — not yet decided)
- [ ] Wire the `chromium-differential` CI job (`testing/TEST_PLAN.md`'s CI section), reporting to the job summary, not gating the build
- [ ] Curate/extend the fixture corpus if differential testing surfaces gaps the current corpus doesn't exercise
