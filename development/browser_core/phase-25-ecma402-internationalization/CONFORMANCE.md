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

## Pinned CLDR provider update (2026-09-17)

Locale provider payloads are checked-in source inputs, not build output. The
full CLDR 48.2.1 `DisplayNames`/`RelativeTimeFormat` table, full list-pattern
table, full decimal/unit data and Gregorian DateTimeFormat `availableFormats`
table are all generated from
`unicode-org/cldr-json@26a79cb42bfcc90def764102aa2af126d9ef3108`. Their
generators, source revisions, row counts and artifact hashes are tracked next
to the payloads. The macOS/Linux/Windows CI matrix checks out that exact CLDR
revision and regenerates every provider file before Rust compilation, while
the same matrix independently checks out the pinned Test262 fixture.

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
**9,085 / 10,058 lines (90.33%)**, 887 / 937 functions (94.66%) and 89.51%
regions, from `cargo llvm-cov -p blueice-ecma402 --fail-under-lines 100
--summary-only`. The raw command exits 1, as it must: the full pinned-provider
migration adds real decoder, fallback, and formatting branches that now belong
to the measured surface. The report has 973 uncovered denominator lines
(including six instrumented Rust-std thread-local lines), so this is no longer
the former 14-line mapping discrepancy. It must be improved with public-boundary
tests rather than rounded up or hidden by an exclusion; the service inventory
below is also incomplete.

## Reproducible current inventory

The Test262 pin is
`72faf8ec1445c55149615e8b35187830783aba1a` (2026-09-10), verified with both
the GitHub archive SHA-256 and a file manifest SHA-256 in
[`backend/bluejs/test262/snapshot.json`](../../../backend/bluejs/test262/snapshot.json).
It was the current public Test262 `main` revision fetched on 2026-09-14; it is
not a remembered Edition 12-era corpus.

The full `intl402/` selection contains 6,714 strict/sloppy modes. The
current non-Temporal execution is reproducible with
`python backend/bluejs/test262/run.py --filter intl402/ --exclude Temporal
--jobs 1`: on 2026-09-17 it scheduled **2,656 modes from 1,328 files and
passed all 2,656** in 216.270 seconds. That execution includes the full
DateTimeFormat (488), DurationFormat (220), `Intl` (132), NumberFormat (498)
and `supportedValuesOf` paths; its complete JSON report is an ephemeral test
artifact, not a source of truth checked into this repository. The remaining
4,058 modes are in `intl402/Temporal/`; they require the separate ECMA-262
Temporal implementation and must remain visible in a full-inventory report,
not be silently counted as passing. A filtered run is evidence only for the
non-Temporal service boundary and never a phase-completion claim.

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
| 17 | `Intl.PluralRules`, rounding and `selectRange` | Cardinal/ordinal selection and range boundary | Constructor, `select`, `selectRange`, `resolvedOptions` and `supportedLocalesOf`; all 106 current Test262 modes pass | partial |
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
