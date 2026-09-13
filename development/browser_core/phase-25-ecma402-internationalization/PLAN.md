# Phase 25 — ECMA-402 Internationalization

[← Back to plan](../BROWSER_CORE_PLAN.md)

**Status**: In progress — `blueice-ecma402` is now an independent workspace
crate. BlueJS consumes its locale canonicalization, collation negotiation and
UTF-16 Collator service. Host-neutral `NumberFormat`, `PluralRules`,
`ListFormat` and `Segmenter` services also have direct test boundaries. The
finite-decimal `NumberFormat` slice is additionally wired through BlueJS and
its JSON-lines Test262 interface; the other adapters and remaining ECMA-402
services are intentionally not claimed done.

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
not require `sudo` in a normal developer shell. On 2026-09-14 the component was
installed and the complete no-exclusion BlueJS coverage gate passed:

```sh
cargo llvm-cov -p blueice-bluejs --fail-under-lines 88 --summary-only
```

It executes every default BlueJS unit and integration test, including the
direct `Intl.NumberFormat` VM boundary and the `bluejs-test262` JSON-lines
process interface. The measured result was **31,655 / 35,794 lines (88.44%)**,
2,247 / 2,486 functions (90.39%), and 84.89% regions, exceeding the 88% line
floor. This is the real crate coverage gate, not a two-target proxy.

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
   now has typed option application, likely-subtag transforms and deterministic
   information data. BlueJS adapts the finite-decimal NumberFormat service
   through an internal slot, `format`, `resolvedOptions` and
   `supportedLocalesOf`; the other adapter migration and service data remain
   to be moved.
5. [ ] Implement `DateTimeFormat`, `RelativeTimeFormat`, `DisplayNames` and
   `DurationFormat` as independent service algorithms before exposing each JS
   constructor.
6. [ ] Add ECMA-402 Test262 coverage by constructor/service and run it both
   through BlueJS and directly at the host-neutral crate boundary where a
   JavaScript Realm is unnecessary. Collator, decimal NumberFormat,
   PluralRules, ListFormat and Segmenter service boundaries now have standalone
   `backend/ecma402/tests/` coverage. The decimal NumberFormat slice also has
   direct-VM and JSON-lines Test262-interface regression tests; Test262-derived
   service coverage and the broader BlueJS integration gate remain pending.

## Current acceptance boundary

The current slice preserves BlueJS behavior for canonical language tags
including `posix`, transformed extension aliases, calendar aliases, boolean
Unicode keys, selected collation data, lookup/best-fit collation negotiation,
and UTF-16 comparison. Its direct crate tests deliberately exercise that
semantic core without constructing a VM. The BlueJS Intl integration tests
remain the JavaScript-observable regression gate.

The independent decimal NumberFormat slice accepts finite base-10 decimal
strings or finite IEEE-754 values, applies the `nu` Unicode extension,
CLDR separators/digits and grouping, and resolves the ECMA-402 decimal
fraction-digit defaults with half-expand rounding. Currency/unit, range,
compact/scientific notation and non-finite symbols remain explicit future
slices rather than silent partial implementations.

BlueJS now owns JavaScript coercion and Realm/prototype semantics for that
finite-decimal slice, then stores the resolved host formatter in an internal
slot. `Intl.NumberFormat` supports construction/call allocation, a cached bound
`format` getter, `resolvedOptions` and `supportedLocalesOf`; its direct VM test
and `bluejs-test262` JSON-lines process test cover locale negotiation, Thai
digits, fraction rounding, custom constructor prototypes and GC reachability.
This is deliberately not a claim of the full JavaScript constructor: currency,
unit, compact/scientific notation, range formatting, non-finite symbols and
the remaining NumberFormat options still require their own host-neutral slices
before they are advertised as supported.

The independent PluralRules slice selects cardinal or ordinal CLDR categories
from a finite base-10 decimal or IEEE-754 value, preserving visible fractional
zeros for the former. Its host-neutral decimal boundary deliberately exposes
the operand distinction needed by CLDR (`1` versus `1.0`), while a future
BlueJS adapter supplies ECMAScript Number coercion and digit-option rounding.
`selectRange` remains a later service slice.

The independent ListFormat slice formats already-coerced strings with CLDR
conjunction, disjunction and unit patterns at wide, short and narrow widths.
It includes locale negotiation, resolved option data and host-neutral
`formatToParts` element/literal output before any BlueJS adapter migration.

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
