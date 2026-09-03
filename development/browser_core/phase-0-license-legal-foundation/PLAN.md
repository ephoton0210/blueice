# Phase 0 — License & Legal Foundation

[← Back to plan](../BROWSER_CORE_PLAN.md)

**Status**: Done

## Objective

Put the legal/licensing groundwork in place before any code that touches Gecko or Blink/Chromium source is written, so provenance and license obligations are tracked from the first ported file onward rather than reconstructed after the fact. See plan §2 for the settled licensing decisions this phase implements.

## Plan

- The repository-level `LICENSE` (MPL-2.0, official text) already exists at the repo root — that part of this phase is done.
- **Decision**: per-file header only, no centralized `NOTICE`/`THIRD_PARTY_LICENSES` file. Every file's licensing history is self-contained in its own header, so it survives being copied, moved, or extracted on its own — at the cost of the full BSD-3-Clause block being repeated in every Chromium/Blink-derived file rather than referenced once centrally. See [`CONTRIBUTING.md`](../../../CONTRIBUTING.md) for the three concrete header templates (original code, Gecko-derived, Chromium/Blink-derived).
- Trademark usage was checked across all current project docs — every mention of Firefox/Mozilla/Chrome/Chromium is either a descriptive reference or the license's own name, never used as BlueIce's own branding.
- **Additional obligation identified**: BSD-3-Clause's binary-distribution clause ("reproduce the above copyright notice... in the documentation and/or other materials provided with the distribution") is separate from, and not satisfied by, the per-file source headers above. It requires a Help/About/Credits screen crediting Chromium (and, for disclosure consistency, Gecko) in any distributed binary. There's no UI to put that screen in yet, so the obligation is recorded here and the actual screen is tracked as a checklist item under [Phase 4](../phase-4-human-rendering-path/PLAN.md).

## Checklist

- [x] Add root `LICENSE` (MPL-2.0, official text)
- [x] Decide: per-file header notice, a centralized `NOTICE`/`THIRD_PARTY_LICENSES` file, or both — **per-file header only**
- [x] Define the per-file header convention for Gecko-derived files (upstream file path, upstream revision/commit, MPL notice)
- [x] Define the per-file header convention for Blink/Chromium-derived files (upstream file path, upstream revision/commit, original BSD-3-Clause copyright + disclaimer text preserved verbatim)
- [x] ~~Create the `NOTICE`/`THIRD_PARTY_LICENSES` file~~ — not applicable; per-file-header-only decision means no centralized file
- [x] Document the convention somewhere contributors will actually see it before porting code — [`CONTRIBUTING.md`](../../../CONTRIBUTING.md)
- [x] Confirm no project branding uses "Firefox"/"Mozilla"/"Chrome"/"Google Chrome"/"Chromium" names or logos (plan §2 trademark note) — verified, no violations
- [x] Identify and record the BSD-3-Clause binary-distribution notice obligation (Help/About/Credits screen) — recorded here; implementation tracked under [Phase 4](../phase-4-human-rendering-path/PLAN.md)
