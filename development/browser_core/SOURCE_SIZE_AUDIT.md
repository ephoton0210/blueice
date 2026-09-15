# Source-size audit

Reviewed: 2026-09-15

## Scope and decision

This audit covers every Rust source and integration-test source below
`backend/` exceeding 1,200 physical lines. It is a review threshold, not a
reason to split a cohesive implementation merely to lower a count. Before a
reviewed file receives a new independently evolving responsibility, its
existing seam must be extracted into a focused child module. Mechanical or
localized bug fixes may remain in place.

The inventory was obtained with:

```sh
find backend -name '*.rs' -type f -print0 | xargs -0 wc -l
```

The counts exclude `target/`, vendored dependencies, generated build output,
and non-Rust assets.

## Active ECMA-402 work

| Source | Lines | Audit finding and required seam before further expansion |
| --- | ---: | --- |
| `ecma402/src/locale_data.rs` | 5,777 | Stable provider entry points remain here; raw CLDR range records are in `locale_data/range_patterns.rs`, while raw locale unit families and their dispatch live in `locale_data/unit_patterns/` by linguistic/data concern. New raw unit or range families must not be added to this root file. |
| `ecma402/tests/number_format.rs` | 3,227 | The integration target remains useful for service-level coverage. Locale-specific raw-CLDR regressions enter `tests/number_format/locale_units.rs`; North Germanic cases already live in its `locale_units/north_germanic.rs` child. Extract corresponding `options`, `ranges`, or linguistic children before their next substantial families are added. |
| `ecma402/src/number_format.rs` | 2,683 | Its current host-neutral formatting flow is cohesive. Extract `number_format/{options,rendering,ranges}.rs` before a new option family, notation renderer, or range-collapse rule. |
| `ecma402/src/date_time_format.rs` | 1,965 | Keep the present pipeline intact; extract locale resolution and interval-part normalization before calendar or skeleton breadth is added. |
| `ecma402/src/duration.rs` | 1,305 | Extract typed option resolution from rendering before another style-data family is introduced. |

The current NumberFormat locale expansion follows this decision: raw range
selection is isolated, and raw simple, generic-compound, and newer
denominator-specific `perUnitPattern` records are co-located in their
linguistic modules. The 181-line `unit_patterns/mod.rs` owns child-family
dispatch, so adding a locale cannot grow the provider root;
`unit_patterns/per.rs` is a 585-line compatibility router and legacy-table
owner, not the destination for a new locale family. Austronesian, Greek,
Indic, Iranian, North Germanic, Romance, Semitic, Slavic, Tai, Turkic,
Uralic, Vietic, and West Slavic families therefore have focused ownership
boundaries. The Turkish family was moved out of `locale_data.rs` during this
review, so new data no longer grows the provider root.

## Other reviewed production sources

| Area | Sources over 1,200 lines | Required seam before continued feature expansion |
| --- | --- | --- |
| BlueJS VM and runtime | `bluejs/src/vm/intl.rs` (5,308), `vm.rs` (2,123), `vm/modules.rs` (1,810), `vm/interpreter.rs` (1,476), `vm/execution.rs` (1,450), `native.rs` (1,429), `vm/temporal.rs` (1,286) | Split by intrinsic or execution subsystem; do not add another independent built-in family to the common VM files. In particular, split `vm/intl.rs` by ECMA-402 constructor family before its next major expansion. |
| BlueJS built-ins and heap | `bluejs/src/heap.rs` (3,304), `vm/builtins.rs` (2,642), `vm/builtins/native_dispatch.rs` (2,072), `vm/builtins/promises.rs` (1,850), `vm/builtins/object.rs` (1,308), `vm/builtins/binary_data.rs` (1,274), `vm/builtins/generators.rs` (1,265), `vm/builtins/arrays.rs` (1,240) | Extract registry/dispatch, storage/GC, and per-built-in behaviour at their existing ownership boundaries before expanding those respective families. |
| BlueJS parser and compiler | `bluejs/src/compiler.rs` (1,618), `compiler/expressions.rs` (1,556), `token.rs` (1,613), `ast.rs` (1,382) | Split new grammar, AST, and code-generation concerns by syntactic family rather than extending central representations. |
| HTML and engine | `core/html/src/tree_builder.rs` (3,516), `core/html/src/tokenizer.rs` (1,589), `core/engine/src/session.rs` (2,811) | Extract insertion-mode groups, tokenizer states, and session lifecycle/transport concerns before new standards or browser-session features land. |
| BlueTS | `bluets/src/checker.rs` (2,716), `bluets/src/parser.rs` (1,461) | Separate checker passes and grammar productions before type-system or syntax breadth is extended. |
| Process and protocol services | `launcher/src/lib.rs` (1,840), `mcp-server/src/lib.rs` (1,695), `mcp-server/src/server.rs` (1,572) | Isolate process lifecycle, protocol handlers, and transport/server layers before each service receives a new protocol family. |

## Reviewed test and conformance infrastructure

| Sources over 1,200 lines | Required seam before expanded coverage |
| --- | --- |
| `bluejs/src/vm/test262.rs` (2,326), `bluejs/tests/test262_host.rs` (1,987), `bluejs/src/parser/tests.rs` (1,565) | Split test selection/runner logic from assertions and group fixtures by language feature before adding another broad Test262 suite. |

## Enforcement for subsequent work

1. Re-run the inventory when a changed Rust file crosses 1,200 lines, or when
   a reviewed file gains a distinct responsibility.
2. Include the selected extraction seam and focused tests in the associated
   phase-plan or conformance update before implementing the new family.
3. Preserve public APIs in the parent module; child modules own data tables,
   parsing/rendering subflows, or feature-specific tests. This keeps review
   and future removal/migration local.

No broad size-only refactor is scheduled: the inventory identifies expansion
boundaries without mixing unrelated behavior changes into the NumberFormat
work.
