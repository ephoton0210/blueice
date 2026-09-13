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

The current no-exclusion host-crate measurement on 2026-09-14 is **2,815 /
2,829 lines (99.51%)**, 305 / 309 functions (98.71%) and 98.01% regions.
The raw `--fail-under-lines 100` command therefore still fails: it must not be
rounded up or replaced by an exclusion. LLVM's LCOV export reports an execution
count for every source line it can map, while the summary retains 14 unmapped
generic/closure instantiation counters in its line denominator. That toolchain
discrepancy is recorded as an open coverage-gate task, not treated as proof of
completion; the service inventory below is also incomplete.

## Reproducible current inventory

The Test262 pin is
`72faf8ec1445c55149615e8b35187830783aba1a` (2026-09-10), verified with both
the GitHub archive SHA-256 and a file manifest SHA-256 in
[`backend/bluejs/test262/snapshot.json`](../../../backend/bluejs/test262/snapshot.json).
It was the current public Test262 `main` revision fetched on 2026-09-14; it is
not a remembered Edition 12-era corpus.

Running `python backend/bluejs/test262/run.py --filter intl402/` against that
pin scheduled **6,714** strict/sloppy modes: **1,786 pass, 4,922 fail, 6
timeout**. The `Temporal/` subtree accounts for 4,058 failures and is reported
separately because it also requires unimplemented ECMA-262 Temporal support.
The remaining ECMA-402-facing inventory is **1,786 pass, 864 fail, 6
timeout**. Failures stay in the denominator; neither the runner nor this
matrix treats filtered or unsupported tests as passes.

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
| 8 | `Intl`, all constructors, `getCanonicalLocales`, `supportedValuesOf` | Host-neutral registry currently carries only DurationFormat's `latn` numbering system | Constructors are exposed for every implemented service; `supportedValuesOf("numberingSystem")` returns `latn`, while the other required registry categories remain unavailable | partial |
| 10 | `Intl.Collator`, `compare`, `resolvedOptions`, `supportedLocalesOf` | UTF-16 Collator and negotiation | Constructor/prototype adapter; all 130 current Test262 modes pass | partial |
| 11 | `Intl.DateTimeFormat`, styles/components, parts and ranges | No formatter yet | Allocation-only constructor; 50 / 488 current Test262 modes pass | partial |
| 12 | `Intl.DisplayNames` | Typed host service, locale negotiation and deterministic data/fallback boundary | Constructor, `of`, `resolvedOptions` and `supportedLocalesOf`; all 114 current Test262 modes pass | partial |
| 13 | `Intl.DurationFormat` | Duration Record/options plus English host formatter and parts | Constructor, supported locales, options, format and parts; 192 / 220 current Test262 modes pass | partial |
| 14 | `Intl.ListFormat`, parts | List/parts and negotiation | Constructor, iterable `format`/`formatToParts`, `resolvedOptions` and `supportedLocalesOf`; all 162 current Test262 modes pass | partial |
| 15 | `Intl.Locale`, option update, accessors, information methods | Canonical locale/options/information, including `RegionPreference` | Constructor and listed accessors/information methods; all 336 current Test262 modes pass | partial |
| 16 | `Intl.NumberFormat`, all styles, rounding, parts/ranges | Decimal/unit subset with non-finite values and all `signDisplay` values | 200 pass, 292 fail and 6 timeout modes; currency, percent, notation, ranges and full locale data remain absent | partial |
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
