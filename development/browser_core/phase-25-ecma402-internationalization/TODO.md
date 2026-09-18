# Phase 25 — ECMA-402 TODO

[← Phase 25 plan](PLAN.md) · [Conformance matrix](CONFORMANCE.md)

A ranked, actionable worklist derived from the current-public implementation
inventory and the 2026-09-17 coverage measurement in
[`CONFORMANCE.md`](CONFORMANCE.md). This file tracks *what to do next*;
`CONFORMANCE.md` remains the sole source of truth for *current status*. Update
both together — closing an item here without updating the matching row/number
in `CONFORMANCE.md` is not done.

## Denominator at a glance (2026-09-17)

Two independent measurements, not interchangeable — see `CONFORMANCE.md`'s
coverage section for why. Full per-service Test262 breakdown is in
`CONFORMANCE.md`'s "Reproducible current inventory".

| Axis | Measurement |
| --- | --- |
| `blueice-ecma402` Rust line coverage | 9,657 / 10,237 (94.33%) |
| Test262 `intl402/` non-Temporal (what Phase 25 claims) | 2,656 / 2,656 (100%) |
| Test262 `intl402/` full corpus, incl. Temporal (unscoped) | 2,922 / 6,714 (43.52%) |
| — of which `Temporal/` alone | 266 / 4,058 (6.55%) |
| — every other `intl402/` group | 2,656 / 2,656 (100%) |

## Coverage gate (`cargo llvm-cov -p blueice-ecma402 --fail-under-lines 100`)

Current: 9,657 / 10,237 lines (94.33%). Ranked by missed-line count:

- [ ] `date_time_format.rs` — 243 / 1,820 lines missed (86.65%). Largest
      single gap in the crate, roughly two-fifths of the total. Triage
      per-function before writing tests: some missed branches are real gaps,
      others may be unreachable paths behind the known ICU4X raw-skeleton
      limitation (see below) and don't need direct coverage so much as a
      documented `unreachable!`/error path.
- [ ] `number_format.rs` — 125 / 1,751 lines missed (92.86%). Second-largest
      gap; likely more tractable as a single increment than DateTimeFormat.
- [ ] `locale_data.rs` — 46 lines missed (97.09%).
- [ ] `lib.rs` (facade) — 20 lines missed (96.87%).
- [ ] `locale_data/currency_patterns.rs` — 24 lines missed (93.91%).
- [ ] `locale_data/compact_patterns.rs` — 22 lines missed (94.40%).
- [ ] `locale_data/date_time_formats.rs` — 20 lines missed (93.67%).
- [ ] `list_format.rs` — 19 lines missed (93.17%).
- [ ] Remaining files (`duration.rs`, `display_names.rs` x2,
      `locale_information.rs` (+`_data.rs`), `relative_time_format.rs`,
      `segmenter.rs`, `plural_rules.rs`, `supported_values.rs`,
      `decimal_symbols.rs`, `unit_patterns/full_cldr_compound.rs`,
      `locale_data/list_patterns.rs`) — each under 10 missed lines; sweep
      these last, after the two large files above.
- [ ] Do not enforce the no-exclusion 100% gate in CI until every branch above
      has a genuine public-boundary test — per `CONFORMANCE.md`'s own
      completion criteria, this is step 5, not step 1.

## Known functional gaps (not just coverage)

- [ ] **DateTimeFormat**: exact raw-skeleton width/extra-field rendering and
      byte-exact `appendItems` literal reproduction in *range* output remain
      blocked on ICU4X's public dynamic-skeleton API. Needs upstream
      investigation (or a documented, tested fallback), not just more tests
      against the current behavior.
- [ ] **NumberFormat**: compact/scientific notation and the complete
      sanctioned-unit data set are explicit future slices, not silently
      partial. Scope each as its own increment.
- [ ] **DurationFormat**: non-English unit-pattern data, locale digital
      metadata, and ISO/Temporal duration input are still pending per
      `PLAN.md` §5.
- [ ] **Locale negotiation / service registry**: `PLAN.md` §4 still says "the
      other adapter migration and service data remain to be moved" without
      naming which adapters — first task is to identify the concrete
      remaining ones.

## Documentation staleness (found 2026-09-17, not yet fixed)

- [ ] `PLAN.md` lines 267–270 still quote the old **9,085 / 10,058 (90.33%)**
      coverage measurement; `CONFORMANCE.md` has been updated to the current
      **9,657 / 10,237 (94.33%)** but `PLAN.md`'s own copy was left stale.
- [ ] `PLAN.md` §5 says "BlueJS has not yet adopted [the DurationFormat
      Duration Record boundary]" — verified false against current
      `backend/bluejs/src/vm/intl.rs` (`resolve_duration_format`/
      `create_duration_format` already adapt `blueice_ecma402::DurationFormat`
      end to end). Needs a correction, not a re-verification.

## Structural / cross-phase blockers (tracked here for visibility, not owned by Phase 25)

- [ ] `intl402/Temporal/` — **4,058 of the full 6,714 `intl402/` modes; only
      266 pass (6.55%), 3,792 fail.** This single group is the entire reason
      the *unfiltered* Test262 `intl402/` pass rate in
      [`TEST262_LINUX_REPORT.md`](../phase-13-bluejs-engine/TEST262_LINUX_REPORT.md)
      reads ~43%: **every other group in `intl402/` is at 100%** — see
      `CONFORMANCE.md`'s full per-service breakdown table in "Reproducible
      current inventory". It is entirely blocked on ECMA-262 Temporal, which
      does not exist in BlueJS yet. Do not treat this as an ECMA-402-crate
      coverage problem or attempt to close it from this crate; it is a
      separate phase's not-yet-started dependency, and the 266 passes that do
      occur are almost entirely feature-detection/negative-assertion tests
      that don't require a real Temporal implementation.
      Sub-divided by Temporal type (`CONFORMANCE.md` has the full table):
      `ZonedDateTime` 32/1,166, `PlainDate` 100/986, `PlainDateTime` 66/966,
      `PlainYearMonth` 18/654, `PlainMonthDay` 48/180, `Duration` 2/42,
      `Instant`/`PlainTime`/`Now` 0/34, 0/24, 0/6. No type clears 27%, so this
      breakdown does not surface a natural "smallest first slice" — a real
      Temporal effort would need its own design doc and phase, not a
      Test262-bucket-count-driven pick from here.
- [ ] **No Temporal phase or plan exists anywhere under
      `development/browser_core/`.** If Temporal work is ever prioritized,
      it needs a new phase directory (design-first, per this repo's own
      workflow) before any implementation TODO can be written for it —
      not a sub-bullet of Phase 25.
- [ ] `backend/bluejs/src/vm/intl.rs` (3,508 lines) is flagged in `PLAN.md`'s
      source-modularity audit as needing a service-by-service split, but only
      alongside matching host-service migrations — no mechanical split
      without adapter regressions.
- [ ] `blueice-bluejs` crate's own coverage gate (`cargo llvm-cov -p
      blueice-bluejs --fail-under-lines 88 --summary-only`) was last measured
      2026-09-14 at 87.84% — under the 88% floor, and now stale (code has
      moved since). Needs a fresh run; not re-measured in this pass because
      it covers BlueJS's entire surface, not just ECMA-402, and takes
      substantially longer than the ECMA-402-only gate.

## Recently closed

- [x] `Intl.PluralRules.prototype.selectRange` non-identity ranges — replaced
      the hardcoded `"other"` fallback with a real CLDR `pluralRanges`-table
      lookup via a second locale-negotiated `PluralRulesWithRanges` service.
      106 / 106 pinned `intl402/PluralRules` Test262 modes still pass.
      (`backend/ecma402/src/plural_rules.rs`, `backend/bluejs/src/vm/intl.rs`)
      **Follow-up regression found and fixed 2026-09-18**: a full
      `cargo test -p blueice-bluejs --no-fail-fast` run (only run for the
      first time while unrelated Phase 26/Temporal work was in progress —
      a normal `cargo test` stops at the first failing test *binary* and
      had been masking this) turned up
      `process_hosts.rs::adapter_executes_plural_rules_through_the_json_lines_interface`
      failing. Its hardcoded expectation,
      `new Intl.PluralRules('en',{type:'ordinal'}).selectRange(1,2) === 'other'`,
      was written against the *old, broken* stub (which always returned
      `"other"` for any non-identity range) and was never updated for the
      real fix above. Verified directly against the host crate that the
      correct value is `'two'` (English ordinal has no explicit CLDR
      `pluralRanges` override, so it falls back to the end category — `2`
      is `"two"` — matching the documented default). Updated the test's
      expectation; this was a stale-test issue, not a logic bug in the
      `selectRange` fix itself.
