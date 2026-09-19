# Phase 25 — ECMA-402 TODO

[← Phase 25 plan](PLAN.md) · [Conformance matrix](CONFORMANCE.md)

A ranked, actionable worklist derived from the current-public implementation inventory and the 2026-09-17 coverage measurement in [`CONFORMANCE.md`](CONFORMANCE.md). This file tracks *what to do next*; `CONFORMANCE.md` remains the sole source of truth for *current status*. Update both together — closing an item here without updating the matching row/number in `CONFORMANCE.md` is not done.

## Denominator at a glance (coverage 2026-09-17; Test262 2026-09-19)

Two independent measurements, not interchangeable — see `CONFORMANCE.md`'s coverage section for why. Full per-service Test262 breakdown is in `CONFORMANCE.md`'s "Reproducible current inventory".

| Axis | Measurement |
| --- | --- |
| `blueice-ecma402` Rust line coverage | 9,657 / 10,237 (94.33%) |
| Test262 `intl402/` non-Temporal (what Phase 25 claims) | 2,656 / 2,656 (100%) |
| Test262 `intl402/` full corpus, incl. Temporal (unscoped) | 6,364 / 6,714 (94.787%) |
| — of which `Temporal/` alone | 3,708 / 4,058 (91.375%) |
| — every other `intl402/` group | 2,656 / 2,656 (100%) |

## Platform status (updated 2026-09-19)

The available local host is Ubuntu 24.04.3 LTS under WSL2, not Ubuntu 24.04.4. The Test262 values above are from the fresh 2026-09-19 complete Ubuntu inventory; the coverage value remains its separately dated measurement. Current Ubuntu checks also cover the workspace build plus focused ECMA-402 and BlueJS Intl suites; the complete details are in [`TEST262_LINUX_REPORT.md`](../phase-13-bluejs-engine/TEST262_LINUX_REPORT.md). macOS and Windows are pending later real runs and must not be filled from the Ubuntu measurements.

## Coverage gate (`cargo llvm-cov -p blueice-ecma402 --fail-under-lines 100`)

Current: 9,657 / 10,237 lines (94.33%). Ranked by missed-line count:

- [ ] `date_time_format.rs` — 243 / 1,820 lines missed (86.65%). Largest single gap in the crate, roughly two-fifths of the total. Triage per-function before writing tests: some missed branches are real gaps, others may be unreachable paths behind the known ICU4X raw-skeleton limitation (see below) and don't need direct coverage so much as a documented `unreachable!`/error path.
- [ ] `number_format.rs` — 125 / 1,751 lines missed (92.86%). Second-largest gap; likely more tractable as a single increment than DateTimeFormat.
- [ ] `locale_data.rs` — 46 lines missed (97.09%).
- [ ] `lib.rs` (facade) — 20 lines missed (96.87%).
- [ ] `locale_data/currency_patterns.rs` — 24 lines missed (93.91%).
- [ ] `locale_data/compact_patterns.rs` — 22 lines missed (94.40%).
- [ ] `locale_data/date_time_formats.rs` — 20 lines missed (93.67%).
- [ ] `list_format.rs` — 19 lines missed (93.17%).
- [ ] Remaining files (`duration.rs`, `display_names.rs` x2, `locale_information.rs` (+`_data.rs`), `relative_time_format.rs`, `segmenter.rs`, `plural_rules.rs`, `supported_values.rs`, `decimal_symbols.rs`, `unit_patterns/full_cldr_compound.rs`, `locale_data/list_patterns.rs`) — each under 10 missed lines; sweep these last, after the two large files above.
- [ ] Do not enforce the no-exclusion 100% gate in CI until every branch above has a genuine public-boundary test — per `CONFORMANCE.md`'s own completion criteria, this is step 5, not step 1.

## Known functional gaps (not just coverage)

- [ ] **DateTimeFormat**: exact raw-skeleton width/extra-field rendering and byte-exact `appendItems` literal reproduction in *range* output remain blocked on ICU4X's public dynamic-skeleton API. Needs upstream investigation (or a documented, tested fallback), not just more tests against the current behavior.
- [ ] **Service-completeness audit**: the current non-Temporal Test262 inventory is 2,656 / 2,656, including NumberFormat (498 / 498), DurationFormat (220 / 220) and Locale (336 / 336). Those results do not by themselves close their current-public specification rows: retain the explicit host limitations, data completeness and 100% no-exclusion coverage review in `CONFORMANCE.md` rather than reviving obsolete feature-slice TODOs.

## Structural / cross-phase blockers (tracked here for visibility, not owned by Phase 25)

- [ ] `intl402/Temporal/` — **4,058 of the full 6,714 `intl402/` modes; 3,708 pass (91.375%), 350 fail.** It is the entire remaining source of failures in the *unfiltered* Test262 `intl402/` pass rate in [`TEST262_LINUX_REPORT.md`](../phase-13-bluejs-engine/TEST262_LINUX_REPORT.md) (94.787%): **every other group in `intl402/` is at 100%** — see `CONFORMANCE.md`'s full per-service breakdown table in "Reproducible current inventory". Do not treat it as an ECMA-402-crate coverage problem or attempt to close it from this crate: Phase 26 owns the remaining implementation work. Current `intl402/Temporal/` results are `ZonedDateTime` 1,062 / 1,166, `PlainDate` 908 / 986, `PlainDateTime` 890 / 966, `PlainYearMonth` 600 / 654, `PlainMonthDay` 150 / 180, `Duration` 34 / 42, and `Instant` / `PlainTime` / `Now` 34 / 34, 24 / 24, 6 / 6. Prioritize through Phase 26's combined ECMA-262 + ECMA-402 Temporal analysis, not this ECMA-402-only bucket.
- [x] The Phase 26 Temporal plan now exists at `development/browser_core/phase-26-ecma262-temporal/PLAN.md`; Temporal work is a cross-phase dependency, not an unowned future placeholder.
- [x] `backend/bluejs/src/vm/intl.rs` was split at service boundaries; its stable facade is now 764 lines and the source-modularity audit records the resulting modules and validation.
- [x] The BlueJS no-exclusion coverage baseline is separately recorded at 88.38% lines on Rust 1.95 (2026-09-13), matching its 88% CI floor. It is not an ECMA-402 completion measure; any future full remeasurement belongs to Phase 13's coverage record.

## Recently closed

- [x] `PLAN.md`'s DurationFormat description now records the existing BlueJS `resolve_duration_format`/`create_duration_format` adapter rather than the obsolete "not yet adopted" statement.
- [x] `Intl.PluralRules.prototype.selectRange` non-identity ranges — replaced the hardcoded `"other"` fallback with a real CLDR `pluralRanges`-table lookup via a second locale-negotiated `PluralRulesWithRanges` service. 106 / 106 pinned `intl402/PluralRules` Test262 modes still pass. (`backend/ecma402/src/plural_rules.rs`, `backend/bluejs/src/vm/intl.rs`) **Follow-up regression found and fixed 2026-09-18**: a full `cargo test -p blueice-bluejs --no-fail-fast` run (only run for the first time while unrelated Phase 26/Temporal work was in progress — a normal `cargo test` stops at the first failing test *binary* and had been masking this) turned up `process_hosts.rs::adapter_executes_plural_rules_through_the_json_lines_interface` failing. Its hardcoded expectation, `new Intl.PluralRules('en',{type:'ordinal'}).selectRange(1,2) === 'other'`, was written against the *old, broken* stub (which always returned `"other"` for any non-identity range) and was never updated for the real fix above. Verified directly against the host crate that the correct value is `'two'` (English ordinal has no explicit CLDR `pluralRanges` override, so it falls back to the end category — `2` is `"two"` — matching the documented default). Updated the test's expectation; this was a stale-test issue, not a logic bug in the `selectRange` fix itself.
