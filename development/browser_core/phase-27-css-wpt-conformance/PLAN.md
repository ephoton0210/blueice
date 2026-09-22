# Phase 27 — CSS WPT Conformance Testing

[← Back to plan](../BROWSER_CORE_PLAN.md)

**Status**: In progress — the harness works end to end against a first pilot corpus (`css/css-color/`, 253 reftests); see "First real results" below.

## Objective

Give `blueice-css` the same kind of independent, real-corpus correctness oracle Phase 13 built for BlueJS against Node.js and `backend/core/html` already has against the official WPT tree-construction corpus (`backend/core/html/tests/wpt_corpus.rs`): the real, upstream **Web Platform Tests CSS suite**, not just the hand-written MVP fixture corpus (`development/browser_core/testing/fixtures/*.dat`) that already exists. Raised out of the numbered sequence for the same reason Phases 14/15/25/26 were: needed now that `blueice-css` is substantial enough (cascade, selectors, box model, basic flexbox) for a real conformance measurement to be worth taking, not planned from the start.

Explicitly not "CSS3 conformance" as a single number — per the HTML/CSS scope research this phase's own investigation surfaced, `phase-2-mvp-scope/PLAN.md`'s "MVP CSS scope" is an ad-hoc subset cross-checked against Stylo/Blink precedent, not a claim to any one numbered CSS spec level. This phase measures how much of the *real* WPT CSS corpus that subset already satisfies, and tracks the gap the same scope-normalized way Phase 2/`wpt_corpus.rs` already tracks HTML5 tree-construction: a raw pass rate (expected low, since WPT's CSS suite tests far more than BlueIce's MVP scope covers) plus a triage distinguishing "known MVP scope gap" from "real bug in what BlueIce already claims to support."

## Why this needs no second engine (unlike Phase 15)

Phase 15's Chromium differential harness compares BlueIce's rendering against a *different* engine's rendering of the *same* input — it needs Puppeteer and a real Chromium because the whole point is cross-engine agreement. WPT's CSS suite instead uses the **reftest** methodology: a test page and one or more reference pages that a spec-compliant engine must render *identically* (`<link rel="match" href="...">`) or *differently* (`rel="mismatch"`), designed so a single engine's own rendering of the two can prove or disprove a claimed CSS behavior without needing a second, independent implementation to compare against. `background-color-rgb-001.html`'s reference page uses the eventual computed color directly (`background-color: green`), so if BlueIce's `rgb()` parsing (it doesn't have any yet — see "First real results") is correct, rendering the test and its reference produces pixel-identical output *by BlueIce's own renderer alone*. This means the harness needs exactly the capability Phase 4/15 already built — navigate a URL, take a screenshot — plus the same `pixelmatch` comparison Phase 15's `compare.js` already uses, and nothing else new.

## Design

- **Corpus**: a real, unmodified subset of `web-platform-tests/wpt`'s `css/` tree, fetched the same sparse-partial-clone way `development/browser_core/reference/wpt/`'s existing HTML tree-construction corpus already is (see that directory's `README.md`, updated by this phase). Pilot suite: `css/css-color/` (253 real `rel="match"`/`rel="mismatch"` reftests, excluding testharness.js-driven script tests and the reference files themselves), plus the shared `css/support/` and root `fonts/` directories a handful of those reftests need via root-absolute paths (`/css/support/...`, `/fonts/ahem.css`). Widening to further suites (`css/css-backgrounds/`, `css/css-box/`, `css/CSS2/`) is future work once the pilot's own findings are triaged, not a scope commitment of this document.
- **Explicitly out of scope for this phase**: any WPT CSS test driven by `testharness.js` (`test()`/`assert_*` calling `getComputedStyle`, CSSOM property access, etc.) — these need real DOM/CSSOM JavaScript bindings BlueJS does not have yet (`backend/core/dom` is a pure internal Rust tree with no script-facing API at all), which is Phase 20's job, not this one's. Running those tests today would either hang waiting on APIs that don't exist or need a parallel, throwaway scripting shim Phase 20 would later replace. This mirrors the same "genuinely unblocked vs. actually blocked on an earlier phase" distinction Phase 24 (PDF viewer, blocked on Phases 20/22) surfaced.
- **Serving the corpus**: unlike Phase 15's per-fixture throwaway HTTP server (one string of HTML at `/`), WPT reftests reference sibling files and root-absolute shared resources (`/css/support/...`, `/fonts/...`), so the harness runs one static file server rooted at the whole `development/browser_core/reference/wpt/` checkout, matching how the upstream `wpt.py` test runner itself serves the repo from a virtual root.
- **Comparison mechanism**: for each reftest, resolve every `rel="match"`/`rel="mismatch"` link from the test file's own `<head>`, navigate BlueIce to the test URL and each reference URL in turn, screenshot each, and `pixelmatch` the test's screenshot against each reference's. A `match` reference must be pixel-identical (0 differing pixels — no fuzzy-match tolerance in this first slice; WPT's `<meta name=fuzzy>` per-test tolerance metadata is a known future refinement, not implemented yet); a `mismatch` reference must differ by at least one pixel.
- **Reuses, not rebuilds**: `blueice-mcp-server`'s existing `navigate`/`screenshot` tools (no new IPC surface needed, unlike Phase 15's `GetDom` addition — a reftest never needs the DOM tree, only pixels) and the `pixelmatch`/`pngjs` dependencies `differential-testing/` already has. New harness code lives in `differential-testing/src/css-wpt/` (same Node project, since it needs no Puppeteer for this phase's own comparisons — Puppeteer stays a `differential-testing` dependency for Phase 15's unrelated cross-engine work) rather than a second Node project.
- **Pass-rate methodology**: mirrors `backend/core/html/tests/wpt_corpus.rs`'s already-established pattern (`testing/TEST_PLAN.md`'s "WPT tree-construction corpus" section) — report the raw pass rate over the full pilot corpus, then classify every failure into a known MVP-CSS-scope gap (a property/value/selector `phase-2-mvp-scope/PLAN.md` already documents as deferred) or an unclassified failure (something BlueIce claims to support that the corpus shows is actually wrong). Zero unclassified failures is the target for a given corpus slice, the same bar `wpt_corpus.rs` holds itself to; a nonzero unclassified count is real signal to fix, not something to filter out of the report.

## First real results

Running the harness against the full `css/css-color/` pilot corpus (253 reftests) found **[fill in after the harness runs]**.

## Explicit non-goals

- A single "CSS3 pass rate" headline number — CSS has no such unified conformance suite even upstream; WPT's `css/` tree is dozens of independently versioned suites (`css-color`, `css-backgrounds`, `css-flexbox`, ...), each tracked and reported on its own, the same way this document reports `css-color` on its own rather than inventing an aggregate.
- Script-driven (`testharness.js`) CSS conformance, CSSOM (`getComputedStyle`, `element.style`), and anything else needing a live DOM/script binding — deferred to whenever Phase 20 exists; adding a throwaway scripting shim here to force those tests to run would duplicate work Phase 20 will replace.
- Fuzzy-match tolerance (`<meta name=fuzzy>`), `@font-face`/custom web font loading (the Ahem test font WPT commonly relies on for exact pixel measurements is out of scope until BlueIce's font system loads fonts other than its bundled DejaVu Sans/Noto Sans TC), and per-`.headers`-file HTTP header overrides the static server does not yet honor — each is a known simplification of this first slice, not a silent gap.

## Checklist

- [x] Design the comparison strategy (reftest self-comparison, no second engine needed, corpus source, pass-rate methodology) — this document
- [x] Widen the existing WPT sparse checkout to a real CSS suite plus its shared resources — `css/css-color/`, `css/support/`, `fonts/` (`reference/README.md`)
- [ ] Build the static-file-server + navigate/screenshot/pixelmatch harness (`differential-testing/src/css-wpt/`)
- [ ] Run the pilot corpus, record the raw pass rate and the known-scope-gap/unclassified failure split — "First real results" above
- [ ] Fix any unclassified failures the pilot run finds (a real bug in a property/value BlueIce already claims to support), the same way every prior WPT-corpus effort in this project has
- [ ] Decide whether/how to wire a `css-wpt-conformance` CI job (reporting only, not gating, matching Phase 15's own still-open CI item)
- [ ] Widen the corpus to additional CSS suites once the pilot's own findings are triaged and closed
