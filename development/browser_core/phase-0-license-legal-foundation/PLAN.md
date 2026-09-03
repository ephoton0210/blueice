# Phase 0 — License & Legal Foundation

[← Back to plan](../BROWSER_CORE_PLAN.md)

**Status**: In progress

## Objective

Put the legal/licensing groundwork in place before any code that touches Gecko or Blink/Chromium source is written, so provenance and license obligations are tracked from the first ported file onward rather than reconstructed after the fact. See plan §2 for the settled licensing decisions this phase implements.

## Plan

- The repository-level `LICENSE` (MPL-2.0, official text) already exists at the repo root — that part of this phase is done.
- What's still missing is the machinery for the two derivative-code cases described in plan §2:
  - Gecko-derived files: MPL-2.0 is mandatory (file-level copyleft), but there's no convention yet for recording *which upstream file/revision* a given file was ported from.
  - Chromium/Blink-derived files (BSD-3-Clause → relicensed to MPL-2.0): the original BSD copyright/disclaimer notice must be preserved somewhere — either inline per-file or centrally. Neither exists yet.
- Decide inline-header vs. centralized-file (or both) before the first ported file lands, since retrofitting provenance notes across many files later is much more expensive than establishing the convention up front.

## Checklist

- [x] Add root `LICENSE` (MPL-2.0, official text)
- [ ] Decide: per-file header notice, a centralized `NOTICE`/`THIRD_PARTY_LICENSES` file, or both
- [ ] Define the per-file header convention for Gecko-derived files (upstream file path, upstream revision/commit, MPL notice)
- [ ] Define the per-file header convention for Blink/Chromium-derived files (upstream file path, upstream revision/commit, original BSD-3-Clause copyright + disclaimer text preserved verbatim)
- [ ] Create the `NOTICE`/`THIRD_PARTY_LICENSES` file (if the centralized approach is chosen) with a template entry ready for the first ported file
- [ ] Document the convention somewhere contributors will actually see it before porting code (e.g. `CONTRIBUTING.md`)
- [ ] Confirm no project branding uses "Firefox"/"Mozilla"/"Chrome"/"Google Chrome"/"Chromium" names or logos (plan §2 trademark note)
