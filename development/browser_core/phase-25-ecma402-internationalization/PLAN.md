# Phase 25 — ECMA-402 Internationalization

[← Back to plan](../BROWSER_CORE_PLAN.md)

**Status**: In progress — `blueice-ecma402` is now an independent workspace
crate. BlueJS consumes its locale canonicalization, collation negotiation and
UTF-16 Collator service. Host-neutral `NumberFormat`, `PluralRules`,
`ListFormat`, `DisplayNames`, `RelativeTimeFormat` and `Segmenter` services
have direct test boundaries, and their JavaScript adapters are integrated with
the JSON-lines Test262 interface. This is not a completion claim: the current
current-public ledger records DateTimeFormat, DurationFormat, full NumberFormat,
the shared data registry and the no-exclusion 100% host coverage gate as open.

The normative target and completion gates are revision-locked in the
[ECMA-402 current-public conformance matrix](CONFORMANCE.md). In particular, the
Phase is not complete merely because a host service compiles or reaches a line
coverage threshold: every current-public surface needs its host-boundary and
JavaScript-observable Test262 evidence.

## Objective

Implement ECMA-402 as a reusable internationalization subsystem, separate from
both BlueJS's ECMAScript object model and `blueice-i18n`'s Fluent-based BlueIce
UI localization. The crate owns standards data algorithms and locale service
selection; JavaScript-visible constructors, Realm/prototype setup, coercion,
and UTF-16 adaptation stay in the BlueJS binding layer.

This boundary prevents `Intl.*` completeness from being blocked by unrelated VM
work and permits a non-JavaScript host to reuse the same locale semantics.

## Architecture

```text
ECMA-402 locale/service algorithms + ICU4X data  →  blueice-ecma402
                                                     ↑
BlueJS Realm, JS coercion, objects and UTF-16 adapter ┘

BlueIce UI Fluent catalogs / translated application text → blueice-i18n
```

## Source modularity audit (2026-09-14)

The initial ECMA-402 implementation collected independent locale services in
one file. That made a change to a service unnecessarily expensive to review:
the service's public types, ICU4X adapter, locale negotiation and tests were
not visually isolated. The public facade remains `blueice_ecma402`, but its
implementations are now extracted by service. `collator.rs`, `list_format.rs`,
`segmenter.rs`, `duration.rs`, `display_names.rs` and `plural_rules.rs` are
completed extractions. `number_format.rs`, `relative_time_format.rs` and the
`Intl.Locale` information service in `locale_information.rs` are also
independent service modules. They retain the exact public paths through
explicit facade re-exports and have direct tests as a no-change gate. The
facade `lib.rs` is now **822 lines** and contains only the shared locale
identifier, locale-option, negotiation and data-provider boundary plus its
unit tests.

This is a structural refactor, not a semantic completion claim. Each moved
service must retain an MPL-2.0 header, a narrowly-scoped `use super::*` bridge
only while common locale machinery remains in the facade, and focused host plus
BlueJS/Test262 verification after the move. The long-term endpoint is a small
facade containing shared identifiers, data-registry interfaces and re-exports;
services must not import BlueJS types.

The same audit enumerated every Rust source file over 2,000 lines at the time
of review. They are not all part of Phase 25, so this table records ownership
and safe follow-up rather than performing unrelated high-risk rewrites while
completing an internationalization standard.

| File | Lines at audit | Ownership / finding | Follow-up |
| --- | ---: | --- | --- |
| `backend/core/html/src/tree_builder.rs` | 3,516 | HTML tree-construction algorithm, parser insertion modes and recovery state | Extract stable insertion-mode families only with parser fixtures and browser integration coverage; defer. |
| `backend/bluejs/src/vm/intl.rs` | 3,508 | JavaScript-visible Intl constructors, slots and coercion adapters | Split by service only alongside matching host-service migrations; DurationFormat and `supportedValuesOf` increased the urgency, but no mechanical split without adapter regressions. |
| `backend/core/engine/src/session.rs` | 2,811 | Browser session dispatcher, navigation and frame lifecycle | Separate command dispatch from navigation state only with engine integration tests; defer. |
| `backend/bluets/src/checker.rs` | 2,654 | TypeScript checker with shared symbol/type-flow state | Identify stable checker passes and extract with compiler fixture coverage; defer. |
| `backend/bluejs/src/heap.rs` | 2,675 | GC heap, allocation, tracing and object storage invariants | Do not mechanically split; first isolate tracing/storage behind invariant tests. |
| `backend/bluejs/src/vm/builtins.rs` | 2,620 | Built-in dispatch plus existing focused submodules | Continue service-family extraction (as already done for promises/typed arrays); defer unrelated work. |
| `backend/bluejs/src/vm.rs` | 2,095 | VM facade, execution state and existing submodule boundary | Keep facade small as new VM features arrive; no immediate Phase 25-only split. |
| `backend/bluejs/src/vm/test262.rs` | 2,018 | Test262 host hooks, including non-standard test-only facilities | Partition by host capability only after preserving process-interface coverage; defer. |

The line counts above are an audit threshold, not a quality metric by
themselves. A file is split only where a stable ownership boundary exists and
where its relevant regression suite demonstrates unchanged observable
behaviour.

- `blueice-ecma402` exposes host-neutral Rust values and typed errors. It must
  not depend on `blueice-bluejs`, GC objects, `Value`, a Realm, or a UI catalog.
- BlueJS converts JS inputs and errors at its public boundary, then delegates
  ECMA-402 data operations to this crate.
- ICU4X is a data/provider implementation detail; BlueJS does not expose its
  types through JavaScript-visible objects.

## Coverage toolchain

The repository pins Rust `1.95.0-aarch64-apple-darwin` through
`rust-toolchain.toml`. Its coverage component is named `llvm-tools` (reported
by Rustup as `llvm-tools-aarch64-apple-darwin`), not the older
`llvm-tools-preview` spelling that older `cargo-llvm-cov` releases may suggest.
Install it against the pinned toolchain:

```sh
rustup component add llvm-tools --toolchain 1.95.0-aarch64-apple-darwin
```

It installs `llvm-cov` and `llvm-profdata` below that toolchain's sysroot. In a
sandbox where `/Users/ephoton/.rustup` is outside the workspace's writable
roots, this bootstrap needs explicit external-write/network approval; it does
not require `sudo` in a normal developer shell. The component is installed.
The following remains the required no-exclusion BlueJS gate:

```sh
cargo llvm-cov -p blueice-bluejs --fail-under-lines 88 --summary-only
```

It executes every default BlueJS unit and integration test, including the
direct `Intl.NumberFormat` VM boundary, the ListFormat/DurationFormat adapters
and the `bluejs-test262` JSON-lines process interface. The fresh 2026-09-14
measurement after the modular service extraction and supported-values adapter
is **33,297 / 37,908 lines (87.84%)**, 2,395 / 2,673 functions (89.60%), and
84.20% regions. It is below
the 88% line floor, so the command correctly remains an open gate rather than
a pass claim. This is the real crate coverage gate, not a two-target proxy.

## Delivery order

1. [x] Establish `backend/ecma402` (`blueice-ecma402`) in the workspace.
2. [x] Move locale identifier validation/canonicalization, Unicode-extension
   aliases, collation locale support/keywords, and collation preferences into
   the independent crate; BlueJS consumes these APIs.
3. [x] Move collation construction/comparison behind a host-neutral UTF-16
   service interface, leaving BlueJS's `Intl.Collator` object as an adapter.
4. [ ] Implement locale negotiation, `Intl.Locale` information, and all
   `resolvedOptions` data APIs from a shared service registry. Collation and
   decimal NumberFormat, PluralRules, ListFormat and Segmenter now have shared
   lookup/best-fit negotiation, direct crate tests and resolved data; Locale
   now has typed option application, likely-subtag transforms and host-neutral
   information data selected with current ECMA-402 `RegionPreference` ordering.
   BlueJS adapts the decimal/unit NumberFormat service
   through an internal slot, `format`, `resolvedOptions` and
   `supportedLocalesOf`, and adapts ListFormat through its own internal slot,
   iterable `format`/`formatToParts`, `resolvedOptions` and
   `supportedLocalesOf`; the other adapter migration and service data remain
   to be moved.
5. [ ] Implement `DateTimeFormat` and `DurationFormat` as independent service
   algorithms before exposing their JS formatting methods. `DisplayNames` and
   `RelativeTimeFormat` are now host-neutral services with BlueJS adapters,
   but remain partial until their data and phase-wide coverage gates are met.
   DurationFormat now has the current-public host-neutral Duration Record boundary:
   ten integral fields, common-sign validation, the `2^32` calendar-unit
   limits and exact `2^53` normalized-seconds limit. BlueJS has not yet
   adopted it. The host layer also resolves the table-ordered unit
   style/display records, global `digital` defaults, numeric-style ordering
   conflicts and `fractionalDigits` range. It now also partitions English
   long/short/narrow/digital output and `formatToParts` records with exact
   integer arithmetic, grouping suppression, zero-display and one-negative-
   sign handling. BlueJS now adapts construction, `supportedLocalesOf`,
   resolved options, record conversion, `format` and `formatToParts`, including
   foreign-Realm default prototypes. Non-English unit-pattern data, locale
   digital metadata, ISO/Temporal duration input and full NumberFormat unit
   patterns are still pending.
6. [ ] Add ECMA-402 Test262 coverage by constructor/service and run it both
   through BlueJS and directly at the host-neutral crate boundary where a
   JavaScript Realm is unnecessary. Collator, decimal NumberFormat,
   PluralRules, ListFormat and Segmenter service boundaries now have standalone
   `backend/ecma402/tests/` coverage. DisplayNames and RelativeTimeFormat now
   also have direct-host, direct-VM, process-interface and foreign-Realm
   regressions; Test262-derived service coverage and the broader BlueJS
   integration gate remain pending.

## Current acceptance boundary

The current slice preserves BlueJS behavior for canonical language tags
including `posix`, transformed extension aliases, calendar aliases, boolean
Unicode keys, selected collation data, lookup/best-fit collation negotiation,
and UTF-16 comparison. Its direct crate tests deliberately exercise that
semantic core without constructing a VM. The BlueJS Intl integration tests
remain the JavaScript-observable regression gate.

The independent NumberFormat slice accepts finite base-10 decimal strings or
IEEE-754 values (including `NaN` and infinities), applies the `nu` Unicode extension, CLDR
separators/digits and grouping, resolves ECMA-402 fraction-digit defaults and
all current rounding-mode and `signDisplay` names, and partitions decimal and currently
supported duration-unit output through `formatToParts`. Currency and percent
now cover their finite-decimal defaults, `formatToParts`, accounting signs and
the bundled locale table's prefix/suffix patterns (including French trailing
currency and non-breaking percent spacing); they are not a claim of complete
CLDR currency-name, unit-pattern or range support. Range,
compact/scientific notation and the complete sanctioned-unit data set remain
explicit future slices rather than silent partial implementations.

BlueJS now owns JavaScript coercion and Realm/prototype semantics for that
finite-decimal slice, then stores the resolved host formatter in an internal
slot. `Intl.NumberFormat` supports construction/call allocation, a cached bound
`format` getter, `resolvedOptions` and `supportedLocalesOf`; its direct VM test
and `bluejs-test262` JSON-lines process test cover locale negotiation, Thai
digits, fraction rounding, custom constructor prototypes and GC reachability.
This is deliberately not a claim of the full JavaScript constructor: the
complete currency-name and locale-pattern data, complete unit data,
compact/scientific notation, range formatting and the remaining NumberFormat
options still require their own host-neutral slices before they are advertised
as supported.

The independent PluralRules slice selects cardinal or ordinal CLDR categories
from a finite base-10 decimal or IEEE-754 value, preserving visible fractional
zeros for the former. Its host-neutral decimal boundary deliberately exposes
the operand distinction needed by CLDR (`1` versus `1.0`), while a future
BlueJS adapter supplies ECMAScript Number coercion and digit-option rounding.
`selectRange` remains a later service slice.

The independent ListFormat slice formats already-coerced strings with CLDR
conjunction, disjunction and unit patterns at wide, short and narrow widths.
It includes locale negotiation, resolved option data and host-neutral
`formatToParts` element/literal output. BlueJS now exposes the service through
the required constructor/prototype boundary, validates that each iterable item
is a String, preserves original UTF-16 element code units while ICU4X selects
the literals, closes a live iterator on an abrupt item failure, and returns
ordinary `{ type, value }` part records. This remains a partial service until
the current-public matrix's phase-wide 100% host-coverage and full-service gates
pass. Its own pinned-upstream ListFormat inventory is now clean.

The independent Segmenter slice returns grapheme, word and sentence boundaries
for already-coerced Unicode strings. Every boundary is exposed as a UTF-16
index; word results additionally retain `isWordLike`. Its locale negotiation
is passed into ICU4X word/sentence tailoring, so it affects data rather than
only `resolvedOptions` output.

The pure Collator service also exposes a read-only negotiation trace (every
candidate's support decision, the selected fallback, and resolved options),
and the Locale service exposes typed option application, canonical data
selection and likely-subtag transforms. The decimal NumberFormat, PluralRules,
ListFormat and Segmenter services likewise expose requested-locale decisions
and resolved data. `blueice-mcp-server` presents them through
`debug_collator` (including an optional exact UTF-16 comparison for
investigating lone surrogates), `debug_number_format`, `debug_plural_rules`,
`debug_list_format`, `debug_segmenter` and `debug_locale`. They have no Realm,
page, IPC or browser-state authority; the later BlueJS debugger remains the
separately scoped Phase 12 adapter.

## Latest verification

On 2026-09-14, the Test262 pin was advanced from the June snapshot to public
`main` revision `72faf8ec1445c55149615e8b35187830783aba1a` (2026-09-10). The
archive and every unpacked file are SHA-256 verified by
`backend/bluejs/test262/run.py`; the fixed hashes are in its `snapshot.json`.
The full `intl402/` inventory contains 6,714 modes, of which 4,058 sit below
the separately blocked `intl402/Temporal/` subtree. The current non-Temporal
inventory was rerun from the current workspace build on 2026-09-17 with
`python backend/bluejs/test262/run.py --filter intl402/ --exclude Temporal
--jobs 1`: it scheduled **2,656 modes from 1,328 files and passed 2,656/2,656**
in 216.270 seconds. This includes DateTimeFormat (488), DurationFormat (220),
NumberFormat (498), Locale (336), `Intl` (132, including
`supportedValuesOf`), DisplayNames (114), RelativeTimeFormat (160), Collator
(130), ListFormat (162), PluralRules (106) and Segmenter (158). This focused
success does not erase the Temporal subtree from the full denominator or make
the phase complete. Collator, ListFormat, Segmenter, DurationFormat,
DisplayNames and PluralRules are physical host-service modules re-exported by
the stable facade; the remaining service extractions are tracked in the
source-modularity audit above.
On 2026-09-17, the required standalone `cargo llvm-cov -p blueice-ecma402
--fail-under-lines 100 --summary-only` command measured **9,085 / 10,058
lines (90.33%)**, 887 / 937 functions (94.66%) and 89.51% regions, then exited
1 as required. The regenerated complete provider adds real decoder, fallback,
and formatting branches, and the report has 973 uncovered denominator lines
(including six instrumented Rust-std thread-local lines). This supersedes the
old 14-line mapping discrepancy: the no-exclusion gate is now a substantive
public-boundary coverage task, not a toolchain exception, and cannot be called
100% or hidden through an exclusion.

The complete `intl402/DurationFormat` filter now schedules **220 modes, all of
which pass** in the 2026-09-17 non-Temporal inventory. It uses the actual
upstream `testIntl.js` DurationFormat pattern helper: BlueJS executes the
helper's BigInt addition, multiplication, division, remainder and
BigInt/Number relational operations, while the host-neutral NumberFormat
service supplies the duration-unit formatting and parts boundary. The
leading-zero negative-zero case, exact Number-visible fraction conversion and
`BigInt(Number)` above the `i64` range are covered by direct regressions. This
is Test262 evidence for DurationFormat's current public non-Temporal surface,
not a completion claim for the phase or its 100% coverage gate.

The latest `intl402/Locale` filter scheduled **168 files and 336 modes**, all
of which pass. `locale_information.rs` now owns `Intl.Locale` information
lookup at the host-neutral boundary; its `RegionPreference` implementation
prioritizes a valid `rg` override, explicit region, `sd` subdivision, CLDR
likely subtags and then `001`. BlueJS has no duplicate locale-data branch: it
only adapts the returned typed data to ECMAScript objects and arrays.

On 2026-09-14, the ListFormat adapter's direct BlueJS regression test covered
locale/type/style resolution, array-iterator input, `formatToParts`,
`supportedLocalesOf`, `Symbol.toStringTag`, custom `Reflect.construct`
prototypes, invalid receivers/options/items and `IteratorClose` on an abrupt
element. It also passed every ListFormat case from the current pinned official
Test262 revision `72faf8ec1445c55149615e8b35187830783aba1a`: **81 files, 162 scheduled
strict/sloppy modes, 162 passes, zero failures**. This includes constructor
primitive-option rejection, ToObject coercion for `supportedLocalesOf`, string
and `undefined` `StringListFromIterable` inputs, GC-safe `formatToParts` result
construction and foreign-Realm fallback prototypes. `cargo test -p
blueice-bluejs --quiet` passed its complete default suite after this change.
This confirms the implemented ListFormat slice only; it does not change the
current-public completion status recorded in [CONFORMANCE.md](CONFORMANCE.md).
