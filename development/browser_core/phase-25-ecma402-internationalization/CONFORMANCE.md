# ECMA-402 Edition 13 conformance matrix

[← Phase 25 plan](PLAN.md)

## Normative baseline

This work is pinned to **ECMA-402, 13th edition, June 2026** —
*ECMAScript® 2026 Internationalization API Specification*. The normative text
is [the ECMA-402 edition 13 HTML](https://402.ecma-international.org/), not a
TC39 draft, an engine's current behaviour, or a remembered earlier edition.
The edition landing page is the authority for a later published edition:
[ECMA-402 publications](https://ecma-international.org/publications-and-standards/standards/ecma-402/).

An implementation may use implementation-dependent locale data as permitted by
the specification. That does not relax ECMAScript-observable algorithms,
property descriptors, coercion, error order, or the required service APIs.

## What “100%” means for this phase

Phase 25 can be called complete only when all of the following are true:

1. Every normative Edition 13 surface below is marked **complete**, with its
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

The starting host-crate measurement on 2026-09-14 is **1,054 / 1,287 lines
(81.90%)**, 129 / 165 functions and 80.81% regions. It is a baseline, not a
completion claim.

## Boundary ownership

| Boundary | Owns | Must not claim |
| --- | --- | --- |
| `blueice-ecma402` | Locale/data algorithms, typed option validation, formatting and UTF-16 service results | ECMAScript coercion, Realm/prototype machinery, property descriptors or GC behaviour |
| BlueJS `vm/intl` | ECMAScript values, observable option access order, constructor/prototype semantics, errors, bound functions and JS iterables | ICU4X types or host-only errors |
| Test262 runner | Exact upstream JS execution outcomes and explicit blockers | Passing an unexecuted/filtered test |

## Edition 13 implementation inventory

Status names are deliberately strict: **complete** means all four completion
criteria above; **partial** means a bounded, documented slice exists; **absent**
means no conforming public service exists yet. No row is currently complete.

| Edition 13 clauses | Required surface | Host service | BlueJS public surface | Status |
| --- | --- | --- | --- | --- |
| 6, 9 | Language-tag/currency/unit/time-zone identifiers; locale list canonicalization, lookup/best-fit resolution, options helpers | Canonical locale handling and per-service locale negotiation are partial | `getCanonicalLocales`, Collator/NumberFormat/Locale-specific paths are partial | partial |
| 8 | `Intl`, all constructors, `getCanonicalLocales`, `supportedValuesOf` | Shared data registry absent | `Intl` lacks most required constructors and `supportedValuesOf` | partial |
| 10 | `Intl.Collator`, `compare`, `resolvedOptions`, `supportedLocalesOf` | UTF-16 Collator and negotiation | Constructor/prototype adapter | partial |
| 11 | `Intl.DateTimeFormat`, styles/components, parts and ranges | No formatter yet | Allocation-only constructor; no formatting API | partial |
| 12 | `Intl.DisplayNames` | Absent | Absent | absent |
| 13 | `Intl.DurationFormat` | Absent | Absent | absent |
| 14 | `Intl.ListFormat`, parts | List/parts and negotiation | Constructor, iterable `format`/`formatToParts`, `resolvedOptions` and `supportedLocalesOf`; all 81 pinned upstream files / 162 strict+sloppy modes pass | partial |
| 15 | `Intl.Locale`, option update, accessors, information methods | Canonical locale/options/information | Constructor and listed accessors/information methods | partial |
| 16 | `Intl.NumberFormat`, all styles, rounding, parts/ranges | Finite decimal subset only | Finite decimal subset only | partial |
| 17 | `Intl.PluralRules`, rounding and `selectRange` | Finite cardinal/ordinal selection, no range/rounding options | Absent | partial |
| 18 | `Intl.RelativeTimeFormat` | Absent | Absent | absent |
| 19 | `Intl.Segmenter`, segment iterator and segments objects | UTF-16 segment boundaries | Absent | partial |

## Required execution order

The rows are implemented in dependency order, with a test added before each
public behaviour is advertised:

1. Finish the common Edition 13 locale/option/service-registry algorithms,
   including `supportedValuesOf`; make the existing negotiation APIs share them.
2. Complete the existing partial services end-to-end (first Collator and Locale,
   then NumberFormat, ListFormat, PluralRules and Segmenter), eliminating
   adapter-only or host-only claims.
3. Implement DateTimeFormat before using the already-declared BlueJS
   constructor; then DisplayNames, DurationFormat and RelativeTimeFormat.
4. Run the complete `intl402/` inventory against the pinned Test262 snapshot,
   triage every non-pass to a concrete missing dependency, and update this
   matrix with reproducible totals.
5. Enforce the no-exclusion 100% host line-coverage command above only after
   all implementation branches have public-boundary regression tests.

This document is the sole progress ledger for Phase 25. A checklist item moves
to **complete** only with the command output and upstream-test evidence recorded
in the Phase 25 plan.
