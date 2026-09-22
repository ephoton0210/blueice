# Phase 25 — ECMA-402 TODO

[← Phase 25 plan](PLAN.md) · [Conformance matrix](CONFORMANCE.md)

A ranked, actionable worklist derived from the current-public implementation inventory and the 2026-09-21 coverage measurement in [`CONFORMANCE.md`](CONFORMANCE.md). This file tracks *what to do next*; `CONFORMANCE.md` remains the sole source of truth for *current status*. Update both together — closing an item here without updating the matching row/number in `CONFORMANCE.md` is not done.

## Denominator at a glance (coverage and Test262 2026-09-21)

Two independent measurements, not interchangeable — see `CONFORMANCE.md`'s coverage section for why. Full per-service Test262 breakdown is in `CONFORMANCE.md`'s "Reproducible current inventory".

| Axis | Measurement |
| --- | --- |
| `blueice-ecma402` Rust line coverage | 9,471 / 10,124 (93.55%) |
| Test262 `intl402/` non-Temporal (what Phase 25 claims) | 2,656 / 2,656 (100.000%) |
| Test262 `intl402/` full corpus, incl. Temporal (unscoped) | 6,714 / 6,714 (100.000%) |
| — of which `Temporal/` alone | 4,058 / 4,058 (100.000%) |
| — every other `intl402/` group | 2,656 / 2,656 (100.000%) |

## Platform status (updated 2026-09-21)

The Test262 values above and the coverage value are from the 2026-09-21 measurements on commit `eaeb5c1`: the complete inventory ran on macOS, Ubuntu and Windows, and `cargo llvm-cov -p blueice-ecma402 --summary-only` gives 93.55% (9,471 / 10,124) on macOS and 93.28% (9,444 / 10,124) on Ubuntu. The complete details are in [`TEST262_LINUX_REPORT.md`](../phase-13-bluejs-engine/TEST262_LINUX_REPORT.md), [`TEST262_MACOS_REPORT.md`](../phase-13-bluejs-engine/TEST262_MACOS_REPORT.md) and [`TEST262_WINDOWS_REPORT.md`](../phase-13-bluejs-engine/TEST262_WINDOWS_REPORT.md); no platform is inferred from another.

## Coverage gate (`cargo llvm-cov -p blueice-ecma402 --fail-under-lines 100`)

Current: 9,471 / 10,124 lines (93.55%, macOS). Ranked by missed-line count:

- [ ] `date_time_format.rs` — 242 / 1,859 lines missed (86.98%; the `date_time_format.rs` family, 232 of them in `date_time_format.rs` itself). Largest single gap in the crate, about 37% of the total (the `date_time_format.rs` family). Triage per-function before writing tests: some missed branches are real gaps, others may be unreachable paths behind the known ICU4X raw-skeleton limitation (see below) and don't need direct coverage so much as a documented `unreachable!`/error path.
- [ ] `number_format.rs` — 146 / 1,757 lines missed (91.69%; the `number_format.rs` family, mostly `number_format/implementation.rs` (79) and `number_format/formatting.rs` (40)). Second-largest gap; likely more tractable as a single increment than DateTimeFormat.
- [ ] `locale_data.rs` — 55 lines missed (94.89%).
- [ ] `lib.rs` (facade) — 23 lines missed (96.39%).
- [ ] `locale_data/currency_patterns.rs` — 24 lines missed (93.91%).
- [ ] `locale_data/compact_patterns.rs` — 22 lines missed (94.40%).
- [ ] `locale_data/date_time_formats.rs` — 20 lines missed (93.67%).
- [ ] `list_format.rs` — 25 lines missed (91.01%).
- [ ] Remaining files (`duration.rs`, `display_names.rs` x2, `locale_information.rs` (+`_data.rs`), `relative_time_format.rs`, `segmenter.rs`, `plural_rules.rs`, `supported_values.rs`, `decimal_symbols.rs`, `unit_patterns/full_cldr_compound.rs`, `locale_data/list_patterns.rs`) — each under 10 missed lines; sweep these last, after the two large files above.
- [ ] Do not enforce the no-exclusion 100% gate in CI until every branch above has a genuine public-boundary test — per `CONFORMANCE.md`'s own completion criteria, this is step 5, not step 1.

## Known functional gaps (not just coverage)

- [ ] **DateTimeFormat**: exact raw-skeleton width/extra-field rendering and byte-exact `appendItems` literal reproduction in *range* output remain blocked on ICU4X's public dynamic-skeleton API. Needs upstream investigation (or a documented, tested fallback), not just more tests against the current behavior.
- [ ] **Service-completeness audit**: the current non-Temporal Test262 inventory is 2,656 / 2,656, including NumberFormat (498 / 498), DurationFormat (220 / 220) and Locale (336 / 336). Those results do not by themselves close their current-public specification rows: retain the explicit host limitations, data completeness and 100% no-exclusion coverage review in `CONFORMANCE.md` rather than reviving obsolete feature-slice TODOs.

## Structural / cross-phase blockers (tracked here for visibility, not owned by Phase 25)

- [x] `intl402/Temporal/` — **4,058 of the full 6,714 `intl402/` modes; all 4,058 pass (100%)** as of the 2026-09-21 complete run, so the unfiltered Test262 `intl402/` pass rate in [`TEST262_LINUX_REPORT.md`](../phase-13-bluejs-engine/TEST262_LINUX_REPORT.md) is now 100% on every platform measured. It remains a cross-phase dependency owned by Phase 26 (see `CONFORMANCE.md`'s per-service breakdown), not an ECMA-402-crate coverage problem.
- [x] The Phase 26 Temporal plan now exists at `development/browser_core/phase-26-ecma262-temporal/PLAN.md`; Temporal work is a cross-phase dependency, not an unowned future placeholder.
- [x] `backend/bluejs/src/vm/intl.rs` was split at service boundaries; its stable facade is now 764 lines and the source-modularity audit records the resulting modules and validation.
- [x] The BlueJS no-exclusion coverage is separately recorded at 91.37% lines (58,944 / 64,511, Ubuntu, Rust 1.95, 2026-09-21; macOS 91.36%) against its 88% CI floor, up from 88.38% on 2026-09-13. It is not an ECMA-402 completion measure; any future full remeasurement belongs to Phase 13's coverage record.

## Recently closed

- [x] `PLAN.md`'s DurationFormat description now records the existing BlueJS `resolve_duration_format`/`create_duration_format` adapter rather than the obsolete "not yet adopted" statement.
- [x] `Intl.PluralRules.prototype.selectRange` non-identity ranges — replaced the hardcoded `"other"` fallback with a real CLDR `pluralRanges`-table lookup via a second locale-negotiated `PluralRulesWithRanges` service. 106 / 106 pinned `intl402/PluralRules` Test262 modes still pass. (`backend/ecma402/src/plural_rules.rs`, `backend/bluejs/src/vm/intl.rs`) **Follow-up regression found and fixed 2026-09-18**: a full `cargo test -p blueice-bluejs --no-fail-fast` run (only run for the first time while unrelated Phase 26/Temporal work was in progress — a normal `cargo test` stops at the first failing test *binary* and had been masking this) turned up `process_hosts.rs::adapter_executes_plural_rules_through_the_json_lines_interface` failing. Its hardcoded expectation, `new Intl.PluralRules('en',{type:'ordinal'}).selectRange(1,2) === 'other'`, was written against the *old, broken* stub (which always returned `"other"` for any non-identity range) and was never updated for the real fix above. Verified directly against the host crate that the correct value is `'two'` (English ordinal has no explicit CLDR `pluralRanges` override, so it falls back to the end category — `2` is `"two"` — matching the documented default). Updated the test's expectation; this was a stale-test issue, not a logic bug in the `selectRange` fix itself.
