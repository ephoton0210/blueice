# Source-size audit

Reviewed: 2026-09-21

## Scope and decision

This audit covers every Rust source and integration-test file below `backend/` at or above 1,200 physical lines. It is a review threshold, not an automatic reason to split cohesive code. The maintenance target is **at most 2,200 physical lines per Rust file**. A file may exceed that target only when an identified responsibility cannot reasonably be separated; no current file needs that exception.

The inventory is reproducible with:

```sh
rg --files backend -g '*.rs' | xargs -r wc -l
```

It excludes `target/`, vendored dependencies, generated output, and non-Rust assets. On this review there are **44 files at or above 1,200 lines** and **zero above 2,200 lines**. The earlier audit's large monolith paths and counts are historical: the extractions now present in the source tree are reflected below.

## Current inventory

| Area | Files at or above 1,200 lines | Review finding |
| --- | --- | --- |
| ECMA-402 implementation | `ecma402/src/date_time_format.rs` (2,101), `locale_data.rs` (1,908), `duration.rs` (1,377) | Provider data is already partitioned beneath `locale_data/`; retain that boundary. Extract a new independently evolving formatter concern before adding it to `date_time_format.rs`. |
| ECMA-402 integration tests | `ecma402/tests/number_format/core.rs` (1,975) | Number-format tests are split by concern; new locale or option families belong in a focused child rather than expanding `core.rs` indiscriminately. |
| BlueJS VM shell and execution | `bluejs/src/vm.rs` (2,098), `vm/modules.rs` (2,140), `vm/interpreter.rs` (1,541), `vm/execution.rs` (1,498), `native.rs` (1,632), `heap.rs` (1,572) | Module loading, execution, native bindings and storage are distinct responsibilities. Keep feature-specific logic in their existing children. |
| BlueJS Temporal | `vm/temporal/iso.rs` (2,067), `plain_date.rs` (1,792), `conversion.rs` (1,790), `zoned.rs` (1,745), `dates.rs` (1,629), `year_month.rs` (1,378), `duration_operations.rs` (1,308), `duration_relative.rs` (1,274) | The former shared Temporal implementation is partitioned by value type and conversion/operation concern. Further work should use those modules, not recreate a common Temporal monolith. |
| BlueJS built-ins | `vm/builtins/promises.rs` (2,003), `native_dispatch/dispatch.rs` (1,764), `execution/runtime.rs` (1,577), `binary_data.rs` (1,458), `arrays.rs` (1,361), `object.rs` (1,354), `generators.rs` (1,265) | Promise, dispatch, runtime and built-in families have separate ownership. Add a new intrinsic family in its own focused module where it does not belong to an existing family. |
| BlueJS parser/compiler | `compiler.rs` (1,712), `compiler/expressions.rs` (1,598), `compiler/statements.rs` (1,315), `token.rs` (1,613), `ast.rs` (1,407), `parser/tests.rs` (1,565) | Statement and expression compilation are now separate. Keep new grammar and test fixtures at the matching syntactic seam. |
| BlueJS integration tests | `tests/intl.rs` (2,067), `tests/test262_host/modules.rs` (1,238) | Host and Intl cases are already separated from the library implementation. Split by service or protocol only when a new independent test family is introduced. |
| HTML and engine | `core/html/src/tree_builder.rs` (2,140), `tokenizer.rs` (1,589), `tree_builder/tests.rs` (1,378), `core/engine/src/session/tests.rs` (2,074) | Tree-building and session tests have dedicated children; insertion-mode, tokenizer-state and session-lifecycle work must stay at those seams. |
| BlueTS | `bluets/src/checker.rs` (1,339), `checker/tests.rs` (1,361), `parser/declarations.rs` (1,215) | Project-wide type surfaces, per-module binding/expression checks, declaration parsing, type/cursor parsing, runtime-token boundaries, direct-expression lowering, and regression suites now have focused children. Every BlueTS/BlueTSC production and test source file is at or below 1,500 lines; retain that tighter boundary for future Phase 18 work. |
| Process/protocol services | `launcher/src/lib.rs` (1,866), `mcp-server/src/lib.rs` (1,717), `mcp-server/src/server.rs` (1,572) | Process lifecycle and MCP server layers are separated. New transport or protocol families require a focused module. |

## Boundary checks

The following former pressure points are now below the 2,200-line target and have explicit module boundaries:

| Former large responsibility | Current boundary | Largest relevant file |
| --- | --- | ---: |
| ECMA-402 locale provider | `locale_data/` data families and provider entry points | `locale_data.rs`, 1,908 |
| BlueJS internationalization/Temporal | `vm/temporal/` by value type and operation | `iso.rs`, 2,067 |
| VM built-in registration and dispatch | `vm/builtins/{native_dispatch,execution}/` and family modules | `promises.rs`, 2,003 |
| HTML tree construction | `tree_builder/` production/test seams | `tree_builder.rs`, 2,140 |
| Engine sessions | `session/` tests and focused session code | `session/tests.rs`, 2,074 |
| ECMA-402 NumberFormat tests | `tests/number_format/` concern-specific files | `core.rs`, 1,975 |
| BlueTS checker/parser/direct lowerer | `checker/{project,module/}`, `parser/{declarations,type_syntax,runtime_syntax}`, and `bluets-bluejs/{expression,tests/}` | `checker/tests.rs`, 1,361 |

## Enforcement for subsequent work

1. Re-run this inventory whenever a changed Rust file reaches 1,200 lines or gains an independently evolving responsibility.
2. Before a file would exceed 2,200 lines, extract the smallest cohesive child module and add focused public-boundary tests. Record the seam in the relevant phase plan or conformance document.
3. Preserve public APIs in parent modules; child modules own data tables, parsing/rendering flows, feature-specific behavior, or focused tests.
4. Do not split only to reduce a count. A localized correction in a coherent module is preferable to a gratuitous abstraction; an exception above 2,200 requires a written rationale and cannot be assumed.
