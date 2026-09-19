# ECMA-402 current-public conformance matrix

[← Phase 25 plan](PLAN.md)

## Normative baseline

This work tracks the **current public TC39 ECMA-402 text**, whose 2026-09-14
publication identifies itself as the *ECMAScript® 2027 Internationalization
API Specification*. The normative source is [the current ECMA-402
specification](https://tc39.es/ecma402/), checked on that date, rather than a
remembered prior edition or an engine's behaviour. The latest published
edition remains separately discoverable through [ECMA-402
publications](https://ecma-international.org/publications-and-standards/standards/ecma-402/).
The pinned Test262 revision below is the executable compatibility corpus for
that check; a later public-spec or Test262 revision must trigger a fresh
inventory rather than inheriting an older completion claim.

## Platform verification status (2026-09-19)

The only currently available test host is **Ubuntu 24.04.3 LTS under WSL2**
(`x86_64-unknown-linux-gnu`, Rust/Cargo 1.95.0). Although a requested refresh
named Ubuntu 24.04.4, that is not the actual local release. On 2026-09-19,
the unfiltered `python3 backend/bluejs/test262/run.py --jobs 8` inventory
completed all 53,582 files / 102,926 modes in 685.257 seconds. The Test262
values later in this document are therefore current Ubuntu 24.04.3 evidence,
not a 24.04.4 result. Focused local checks are also recorded in the
[Ubuntu Test262 report](../phase-13-bluejs-engine/TEST262_LINUX_REPORT.md).

macOS and Windows validation is **deferred** until those platforms are
available. Do not infer cross-platform parity from Ubuntu outcomes.

## Pinned CLDR provider update (2026-09-17)

Locale provider payloads are checked-in source inputs, not build output. The
full CLDR 48.2.1 `DisplayNames`/`RelativeTimeFormat` table, full list-pattern
table, full decimal/unit data and Gregorian DateTimeFormat `availableFormats`
table are all generated from
`unicode-org/cldr-json@26a79cb42bfcc90def764102aa2af126d9ef3108`. Their
generators, source revisions, row counts and artifact hashes are tracked next
to the payloads. The configured macOS/Linux/Windows CI matrix checks out that
exact CLDR revision and regenerates every provider file before Rust
compilation, while the same matrix independently checks out the pinned Test262
fixture. This document update validates only the Ubuntu host described above;
it does not claim fresh macOS or Windows execution.

`Intl.Locale` regional information now comes from that same pin rather than
hand-written country cases: 52 calendar-preference regions, 276 hour-cycle
rules, 151 week-data regions, and 419 BCP-47 time zones across 247 regions.
The regional time-zone list uses CLDR's first canonical BCP-47 IANA alias;
regions with no applicable time zone return an empty list, never a fabricated
`Etc/UTC` default. Rust and BlueJS tests cover full US data, multi-zone
Germany, likely-subtag defaults, `rg`/`sd` overrides, and a runtime count gate
over the complete matrix.

The provider now supplies 766 resolved locales for `DisplayNames`,
`ListFormat`, `DurationFormat`, `RelativeTimeFormat` and `DateTimeFormat`;
DateTimeFormat's BasicFormatMatcher consumes its 38,792 locale-ordered
Gregorian `availableFormats` records and verifies every locale's 8,426 raw
`appendItems` entries, including each pattern's localized CLDR `dateFields`
label for `{2}`, before advertising it. A fully covering selected record sets
the dynamic formatter's semantic component set. When it lacks a requested
component, single-value `format`/`formatToParts` renders complete typed values,
filters the selected base fields, and expands the raw CLDR `{0}`/`{1}`/`{2}`
append literal at the part boundary. Ranges retain ICU4X's complete dynamic
interval skeleton, so they preserve all requested fields and never invoke the
incomplete raw renderer, but they do not yet reproduce an `appendItems` literal
as a byte-exact range pattern. A 766-locale matrix covers weekday, era, date,
clock, fractional-second and zone components; a pinned `de-CH` regression
proves the localized `Stunde` append label and range field retention. The
selected raw record's exact width and extra-field rendering remain limited by
ICU4X's public dynamic skeleton API, so this is not a completion claim for
DateTimeFormat or the phase.

An implementation may use implementation-dependent locale data as permitted by
the specification. That does not relax ECMAScript-observable algorithms,
property descriptors, coercion, error order, or the required service APIs.

## `hourCycle: "h24"` rendering bug (fixed 2026-09-18, found via Phase 26)

Found while closing `Temporal.PlainTime`/`Temporal.Instant`'s own
`toLocaleString/hourcycle.js` Test262 fixtures (Phase 26,
`development/browser_core/phase-26-ecma262-temporal/PLAN.md`) — an ordinary
`Intl.DateTimeFormat` bug, not a Temporal one, so it is recorded here rather
than in the Temporal plan. `hourCycle: "h24"` rendered midnight (hour 0) as
`"00"` instead of `"24"`. `resolve_date_time_locale`
(`backend/ecma402/src/date_time_format.rs`) substitutes ICU4X's `h23`
skeleton for `h24` when building the formatting locale — ICU4X's dynamic
semantic skeleton has no `h24` field-set preference of its own — while
keeping `h24` as the ECMA-402-visible resolved `hourCycle`, but nothing
converted the resulting `h23`-range (`0`-`23`) rendered digits back to
`h24`'s range (`1`-`24`) at midnight. Fixed with
`DateTimeFormat::apply_h24_hour_cycle`, a typed-part-boundary rewriter in the
same style as the existing `apply_flexible_day_period` (reusing
`trim_numeric_date_part_padding`'s locale-digit lookup rather than assuming
ASCII), run at both the single-value and range-endpoint formatting call
sites: whenever the resolved `hour_cycle` is `"h24"` and the underlying ICU
hour is `0`, the `hour` part's text is replaced with the locale digits for
`"24"`. `hourCycle: "h11"` needed no corresponding fix — it was already
correct; it only *looked* broken in the Temporal fixtures because both
`hourcycle.js` files run every `hourCycle` value in one script in ascending
order and the `h24` assertion's failure aborted the script before its `h11`
assertion ever ran. A new host-neutral `blueice-ecma402` test,
`h24_and_h11_hour_cycles_render_midnight_correctly`
(`backend/ecma402/tests/date_time_format.rs`), covers all four `hourCycle`
values (`h23`/`h12`/`h24`/`h11`) independently and would have caught this on
its own. No non-`Temporal` Test262 fixture happens to check the rendered
digits at `h24`/`h11` midnight (the existing `hourCycle`-adjacent
`intl402/DateTimeFormat/` fixtures only check `resolvedOptions()` reporting,
which this bug never affected), so the "every group other than `Temporal/`
is at 100%" table above did not previously expose it; re-running
`intl402/DateTimeFormat/` after the fix still measures 488/488, and a full
`intl402/` run excluding `Temporal/`/`DateTimeFormat/` stays at 2,168/2,168 —
zero regressions.

## What “100%” means for this phase

Phase 25 can be called complete only when all of the following are true:

1. Every normative current-public surface below is marked **complete**, with its
   applicable abstract operations implemented at the correct boundary.
2. Every applicable upstream Test262 `intl402/` test in the pinned corpus passes
   through BlueJS. Tests depending on still-unimplemented ECMA-262 facilities
   remain visible as blocked, never omitted or counted as passing.
3. Each host-neutral algorithm has focused Rust tests for normal, override,
   fallback and error paths; every JavaScript-visible operation has a BlueJS
   integration test for call/construct, coercion, descriptors and receiver
   validation where the specification requires them.
4. `cargo llvm-cov -p blueice-ecma402 --fail-under-lines 100 --summary-only`
   passes with no source exclusions. This is a real line-coverage gate, but it
   is **not** a substitute for points 1–3 or branch/specification coverage.

The current standalone no-exclusion host-crate measurement on 2026-09-17 is
**9,657 / 10,237 lines (94.33%)**, 897 / 942 functions (95.22%) and 91.90%
regions, from `cargo llvm-cov -p blueice-ecma402 --fail-under-lines 100
--summary-only`. The raw command still exits 1, as it must until the gate
genuinely passes. The two largest remaining files by missed-line count are
`date_time_format.rs` (243 of 1,820 lines missed, 86.65%) and
`number_format.rs` (125 of 1,751 lines missed, 92.86%); together they account
for roughly two-thirds of the crate's entire coverage gap. Every other source
file is at or above 90%, and `collator.rs` and `locale_data/range_patterns.rs`
are already at 100%. It must be improved with public-boundary tests rather
than rounded up or hidden by an exclusion; the service inventory below is also
incomplete. See [`TODO.md`](TODO.md) for the current ranked worklist.

**This line-coverage number is not a Test262 pass rate and the two must not be
quoted interchangeably.** It measures what fraction of `blueice-ecma402`'s own
Rust source lines execute during the crate's own test suite — a measure of how
thoroughly the *already-written* implementation is exercised, independent of
how much of the ECMA-402 specification that implementation actually covers.
[`development/browser_core/phase-13-bluejs-engine/TEST262_LINUX_REPORT.md`](../phase-13-bluejs-engine/TEST262_LINUX_REPORT.md)
reports a separate, unrelated number: the unfiltered upstream Test262
`intl402/` pass rate through BlueJS end-to-end, **94.787% (6,364 / 6,714
modes)** on the 2026-09-19 Ubuntu complete run. The remaining 350 failures
are all in `intl402/Temporal/`; Phase 26 owns their follow-up rather than
making them an ECMA-402 host-crate coverage gap.
The full per-service breakdown of this denominator is in "Reproducible
current inventory" below. The filtered, non-Temporal Test262 run this
document already tracks (2,656 / 2,656 passing) is the correct scoped
conformance figure for what Phase 25 actually claims to support; the
unfiltered report's 94.787% is the honest, unscoped figure across the complete
current-public `intl402/` corpus. Both numbers are real and answer
different questions.

## Reproducible current inventory

The Test262 pin is
`72faf8ec1445c55149615e8b35187830783aba1a` (2026-09-10), verified with both
the GitHub archive SHA-256 and a file manifest SHA-256 in
[`backend/bluejs/test262/snapshot.json`](../../../backend/bluejs/test262/snapshot.json).
It was the current public Test262 `main` revision fetched on 2026-09-14; it is
not a remembered Edition 12-era corpus.

The full `intl402/` selection contains 6,714 strict/sloppy modes. It is
reproducible with `python3 backend/bluejs/test262/run.py --jobs 8`, writing
one `{path, status, ...}` JSON line per mode to
`<output>/results.jsonl`. Grouping that file by the path segment directly
under `intl402/` (`path.split("/")[1]`) gives the complete denominator,
broken out by service, from the 2026-09-19 complete run:

| `intl402/` group | Modes | Pass | Fail | Pass rate |
| --- | ---: | ---: | ---: | ---: |
| `Temporal/` | 4,058 | 3,708 | 350 | 91.375% |
| `NumberFormat/` | 498 | 498 | 0 | 100% |
| `DateTimeFormat/` | 488 | 488 | 0 | 100% |
| `Locale/` | 336 | 336 | 0 | 100% |
| `DurationFormat/` | 220 | 220 | 0 | 100% |
| `ListFormat/` | 162 | 162 | 0 | 100% |
| `RelativeTimeFormat/` | 160 | 160 | 0 | 100% |
| `Segmenter/` | 158 | 158 | 0 | 100% |
| `Intl/` | 132 | 132 | 0 | 100% |
| `Collator/` | 130 | 130 | 0 | 100% |
| `DisplayNames/` | 114 | 114 | 0 | 100% |
| `PluralRules/` | 106 | 106 | 0 | 100% |
| `intl402/*.js` (top-level, e.g. `fallback-locales-are-supported.js`) | 44 | 44 | 0 | 100% |
| `String/` (`localeCompare`, `toLocaleUpperCase`/`LowerCase`) | 38 | 38 | 0 | 100% |
| `Date/` (`toLocale*String`) | 24 | 24 | 0 | 100% |
| `BigInt/` (`toLocaleString`) | 22 | 22 | 0 | 100% |
| `Number/` (`toLocaleString`) | 14 | 14 | 0 | 100% |
| `Array/` (`toLocaleString`) | 4 | 4 | 0 | 100% |
| `FallbackSymbol/` | 4 | 4 | 0 | 100% |
| `TypedArray/` (`toLocaleString`) | 2 | 2 | 0 | 100% |
| **Total** | **6,714** | **6,364** | **350** | **94.787%** |

Every group other than `Temporal/` is at **100%**; the 2,656 non-Temporal
modes summed above match the non-Temporal service total exactly.
`intl402/Temporal/` now passes 3,708 modes; its 350 remaining failures stay
visible in the full denominator and are owned by Phase 26. The complete
per-mode JSON report is an ephemeral test artifact, not a source of truth
checked into this repository — regenerate it with the command above rather
than trusting a stale copy. A non-Temporal-only run is evidence only for the
service boundary and never a phase-completion claim by itself.

`intl402/Temporal/` itself further subdivides by Temporal type
(`path.split("/")[2]`), from the same 2026-09-19 run. The
[Phase 26 Temporal plan](../phase-26-ecma262-temporal/PLAN.md) owns the
implementation status behind these results:

| `Temporal/` type | Modes | Pass | Fail | Pass rate |
| --- | ---: | ---: | ---: | ---: |
| `ZonedDateTime/` | 1,166 | 1,062 | 104 | 91.080% |
| `PlainDate/` | 986 | 908 | 78 | 92.089% |
| `PlainDateTime/` | 966 | 890 | 76 | 92.133% |
| `PlainYearMonth/` | 654 | 600 | 54 | 91.743% |
| `PlainMonthDay/` | 180 | 150 | 30 | 83.333% |
| `Duration/` | 42 | 34 | 8 | 80.952% |
| `Instant/` | 34 | 34 | 0 | 100% |
| `PlainTime/` | 24 | 24 | 0 | 100% |
| `Now/` | 6 | 6 | 0 | 100% |
| **Total** | **4,058** | **3,708** | **350** | **91.375%** |

`Instant/`, `PlainTime/` and `Now/` are now complete in this `intl402/`
subtree. The remaining failures are concentrated in calendar-aware
`ZonedDateTime`, `PlainDate`, `PlainDateTime`, `PlainYearMonth`,
`PlainMonthDay`, and `Duration` operations; their method-level triage belongs
in Phase 26 rather than the ECMA-402 host-service backlog.

## Boundary ownership

| Boundary | Owns | Must not claim |
| --- | --- | --- |
| `blueice-ecma402` | Locale/data algorithms, typed option validation, formatting and UTF-16 service results | ECMAScript coercion, Realm/prototype machinery, property descriptors or GC behaviour |
| BlueJS `vm/intl` | ECMAScript values, observable option access order, constructor/prototype semantics, errors, bound functions and JS iterables | ICU4X types or host-only errors |
| Test262 runner | Exact upstream JS execution outcomes and explicit blockers | Passing an unexecuted/filtered test |

## Current-public implementation inventory

Status names are deliberately strict: **complete** means all four completion
criteria above; **partial** means a bounded, documented slice exists; **absent**
means no conforming public service exists yet. No row is currently complete.

| Current clauses | Required surface | Host service | BlueJS public surface | Status |
| --- | --- | --- | --- | --- |
| 6, 9 | Language-tag/currency/unit/time-zone identifiers; locale list canonicalization, lookup/best-fit resolution, options helpers | Canonical locale handling and per-service locale negotiation are partial | `getCanonicalLocales`, Collator/NumberFormat/Locale-specific paths are partial | partial |
| 8 | `Intl`, all constructors, `getCanonicalLocales`, `supportedValuesOf` | Host-neutral registry carries the required canonical calendars, numbering systems, currencies, units, collations and IANA zones; deprecated `islamic`/`islamic-rgsa` DateTimeFormat requests resolve to the advertised `islamic-civil` fallback | Constructors are exposed for every implemented service; `supportedValuesOf` returns fresh sorted/de-duplicated data-backed values for every standard category, with IANA canonicalization applied to zones | partial |
| 10 | `Intl.Collator`, `compare`, `resolvedOptions`, `supportedLocalesOf` | UTF-16 Collator and negotiation | Constructor/prototype adapter; all 130 current Test262 modes pass | partial |
| 11 | `Intl.DateTimeFormat`, styles/components, parts and ranges | Locale/key resolution, TimeClip, fixed/IANA zones, components/styles, parts and ranges; full 766-locale raw Gregorian `availableFormats`/`appendItems` provider | Constructor/prototype adapter, coercion, parts and ranges; 488 / 488 current direct Test262 modes pass. Single-value Basic output synthesizes raw append literals, including localized `{2}` field labels; ranges retain complete dynamic interval fields, while exact raw-skeleton width/extra-field and append-literal range rendering remain ICU4X host limitations. | partial |
| 12 | `Intl.DisplayNames` | Typed host service, locale negotiation and deterministic data/fallback boundary | Constructor, `of`, `resolvedOptions` and `supportedLocalesOf`; all 114 current Test262 modes pass | partial |
| 13 | `Intl.DurationFormat` | Duration Record/options, localized CLDR unit/list patterns and typed parts | Constructor, supported locales, options, format and parts; 220 / 220 current direct Test262 modes pass | partial |
| 14 | `Intl.ListFormat`, parts | List/parts and negotiation | Constructor, iterable `format`/`formatToParts`, `resolvedOptions` and `supportedLocalesOf`; all 162 current Test262 modes pass | partial |
| 15 | `Intl.Locale`, option update, accessors, information methods | Canonical locale/options/information, including `RegionPreference` | Constructor and listed accessors/information methods; all 336 current Test262 modes pass | partial |
| 16 | `Intl.NumberFormat`, all styles, rounding, parts/ranges | Full pinned decimal/currency/unit/compact provider with typed ranges and parts | Constructor/prototype adapter; 498 / 498 current direct Test262 modes pass | partial |
| 17 | `Intl.PluralRules`, rounding and `selectRange` | Cardinal/ordinal selection and a real CLDR `pluralRanges`-table range boundary | Constructor, `select`, `selectRange` (resolves each endpoint's category against the bundled CLDR range table, not an `"other"` stub), `resolvedOptions` and `supportedLocalesOf`; all 106 current Test262 modes pass | partial |
| 18 | `Intl.RelativeTimeFormat` | Host-neutral patterns, numeric parts and locale/numbering-system resolution | Constructor, `format`, `formatToParts`, `resolvedOptions` and `supportedLocalesOf`; all 160 current Test262 modes pass | partial |
| 19 | `Intl.Segmenter`, segment iterator and segments objects | UTF-16 segment boundaries | Full constructor/segments iterator adapter; all 158 current Test262 modes pass | partial |

## Required execution order

The rows are implemented in dependency order, with a test added before each
public behaviour is advertised:

1. Finish the common current-public locale/option/service-registry algorithms,
   including `supportedValuesOf`; make every constructor use those exact data
   sets.
2. Complete Locale and NumberFormat end-to-end, then turn the full current
   Test262 failure buckets into focused regressions rather than broad slices.
3. Implement DateTimeFormat and DurationFormat as host-neutral services before
   advertising formatting methods. DisplayNames and RelativeTimeFormat are
   wired end-to-end but still require full locale-data and coverage completion.
4. Run the complete `intl402/` inventory against the pinned Test262 snapshot,
   triage every non-pass to a concrete missing dependency, and update this
   matrix with reproducible totals.
5. Enforce the no-exclusion 100% host line-coverage command above only after
   all implementation branches have public-boundary regression tests.

This document is the sole progress ledger for Phase 25. A checklist item moves
to **complete** only with the command output and upstream-test evidence recorded
in the Phase 25 plan.
