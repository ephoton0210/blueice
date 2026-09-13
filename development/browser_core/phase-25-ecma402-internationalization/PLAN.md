# Phase 25 — ECMA-402 Internationalization

[← Back to plan](../BROWSER_CORE_PLAN.md)

**Status**: In progress — `blueice-ecma402` is now an independent workspace
crate. Its locale canonicalization and locale-data selection APIs are consumed
by BlueJS; the remaining ECMA-402 services are intentionally not claimed done.

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

## Delivery order

1. [x] Establish `backend/ecma402` (`blueice-ecma402`) in the workspace.
2. [x] Move locale identifier validation/canonicalization, Unicode-extension
   aliases, collation locale support/keywords, and collation preferences into
   the independent crate; BlueJS consumes these APIs.
3. [ ] Move collation construction/comparison behind a host-neutral UTF-16
   service interface, leaving BlueJS's `Intl.Collator` object as an adapter.
4. [ ] Implement locale negotiation, `Intl.Locale` information, and all
   `resolvedOptions` data APIs from a shared service registry.
5. [ ] Implement `NumberFormat`, `DateTimeFormat`, `PluralRules`,
   `RelativeTimeFormat`, `ListFormat`, `DisplayNames`, `Segmenter`, and
   `DurationFormat` as independent service algorithms before exposing each JS
   constructor.
6. [ ] Add ECMA-402 Test262 coverage by constructor/service and run it both
   through BlueJS and directly at the host-neutral crate boundary where a
   JavaScript Realm is unnecessary.

## Current acceptance boundary

The first slice preserves BlueJS behavior for canonical language tags including
`posix`, transformed extension aliases, calendar aliases, boolean Unicode keys,
and selected collation data. Its direct crate tests deliberately exercise that
semantic core without constructing a VM. The BlueJS Intl integration tests
remain the JavaScript-observable regression gate.
