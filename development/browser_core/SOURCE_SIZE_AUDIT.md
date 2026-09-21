# Source-size audit

Reviewed: 2026-09-21

## Scope and decision

This audit covers every Rust source and integration-test file below `backend/` at or above 1,200 physical lines. It is a review threshold, not an automatic reason to split cohesive code. The maintenance target is **at most 2,200 physical lines per Rust file**, with a stricter **1,500-line limit for Temporal code under `backend/bluejs/src/vm/temporal/`** (see the enforcement section). A file may exceed that target only when an identified responsibility cannot reasonably be separated; no current file needs that exception.

The inventory is reproducible with:

```sh
rg --files backend -g '*.rs' | xargs -r wc -l
```

It excludes `target/`, vendored dependencies, generated output, and non-Rust assets. On this review there are **40 files at or above 1,200 lines** and **zero above 2,200 lines**. The earlier audit's large monolith paths and counts are historical: the extractions now present in the source tree are reflected below.

## Current inventory

| Area | Files at or above 1,200 lines | Review finding |
| --- | --- | --- |
| ECMA-402 implementation | `ecma402/src/date_time_format.rs` (2,105), `locale_data.rs` (1,908), `duration.rs` (1,377) | Provider data is already partitioned beneath `locale_data/`; retain that boundary. Extract a new independently evolving formatter concern before adding it to `date_time_format.rs`. |
| ECMA-402 integration tests | `ecma402/tests/number_format/core.rs` (1,975) | Number-format tests are split by concern; new locale or option families belong in a focused child rather than expanding `core.rs` indiscriminately. |
| BlueJS VM shell and execution | `bluejs/src/vm.rs` (2,196), `vm/modules.rs` (2,058), `vm/interpreter.rs` (1,596), `vm/execution.rs` (1,497), `native.rs` (1,674), `heap.rs` (1,585), `vm/test262/foreign.rs` (1,597) | Module loading, execution, native bindings and storage are distinct responsibilities. Keep feature-specific logic in their existing children: `vm/modules.rs` keeps graph linking and evaluation, while phase imports and deferred namespaces (`modules/deferred.rs`) and export/namespace resolution (`modules/namespace.rs`) are children. `vm.rs` is 17 lines under the target, so the next addition there must first move into a child. |
| BlueJS Temporal | `vm/temporal/year_month.rs` (1,420), `duration_operations.rs` (756), `zoned_difference.rs` (1,119), `plain_date_time_difference.rs` (1,247) | Every Temporal file is at or below the 1,500-line limit. The former monoliths are facades over child directories: `iso.rs` -> `iso/{scan,annotations,offset,datetime,duration,tests}`, `conversion.rs` -> `conversion/*`, `dates.rs` -> `dates/*`, `plain_date.rs` -> `plain_date/*`, `zoned.rs` -> `zoned/*`. Add new behavior to the matching child, not the facade. |
| BlueJS built-ins | `vm/builtins/promises.rs` (1,996), `native_dispatch/dispatch.rs` (1,870), `execution/runtime.rs` (1,599), `binary_data.rs` (1,693), `arrays.rs` (1,631), `object.rs` (1,411), `generators.rs` (1,279), `globals.rs` (1,213) | Promise, dispatch, runtime and built-in families have separate ownership. Add a new intrinsic family in its own focused module where it does not belong to an existing family, as the Immutable ArrayBuffer surface did with `immutable_arraybuffer.rs`. `binary_data.rs` and `arrays.rs` are the next files to split when their families grow again. |
| BlueJS parser/compiler | `compiler.rs` (1,736), `compiler/expressions.rs` (1,617), `compiler/statements.rs` (1,414), `token.rs` (1,663), `ast.rs` (1,457), `parser/expressions.rs` (1,226), `parser/tests.rs` (1,572) | Statement and expression compilation are now separate. Keep new grammar and test fixtures at the matching syntactic seam. |
| BlueJS integration tests | `tests/intl.rs` (2,067), `tests/test262_host/modules.rs` (1,238) | Host and Intl cases are already separated from the library implementation. Split by service or protocol only when a new independent test family is introduced. |
| HTML and engine | `core/html/src/tree_builder.rs` (2,140), `tokenizer.rs` (1,589), `tree_builder/tests.rs` (1,378), `core/engine/src/session/tests.rs` (2,074) | Tree-building and session tests have dedicated children; insertion-mode, tokenizer-state and session-lifecycle work must stay at those seams. |
| BlueTS / BlueTSC / direct bridge | `bluets/src/checker/tests.rs` (1,361), `checker.rs` (1,339), `bluets-bluejs/src/lib.rs` (1,249), `parser/declarations.rs` (1,215) | Project-wide type surfaces, per-module binding/expression checks, declaration parsing, type/cursor parsing, runtime-token boundaries, direct-expression lowering, page-realm ownership, and regression suites have focused children. Every BlueTS, BlueTSC, and `bluets-bluejs` production/test source is at or below the requested 1,500-line boundary; the largest is `checker/tests.rs` at 1,361. |
| Process/protocol services | `launcher/src/lib.rs` (1,866), `mcp-server/src/lib.rs` (1,717), `mcp-server/src/server.rs` (1,572) | Process lifecycle and MCP server layers are separated. New transport or protocol families require a focused module. |

## Boundary checks

The following former pressure points are now below the 2,200-line target and have explicit module boundaries:

| Former large responsibility | Current boundary | Largest relevant file |
| --- | --- | ---: |
| ECMA-402 locale provider | `locale_data/` data families and provider entry points | `locale_data.rs`, 1,908 |
| BlueJS internationalization/Temporal | `vm/temporal/` by value type and operation | `year_month.rs`, 1,378 |
| VM built-in registration and dispatch | `vm/builtins/{native_dispatch,execution}/` and family modules | `promises.rs`, 2,011 |
| BlueJS module graph | `vm/modules/` phase-import/deferred-namespace and export/namespace children | `modules.rs`, 2,058 |
| HTML tree construction | `tree_builder/` production/test seams | `tree_builder.rs`, 2,140 |
| Engine sessions | `session/` tests and focused session code | `session/tests.rs`, 2,074 |
| ECMA-402 NumberFormat tests | `tests/number_format/` concern-specific files | `core.rs`, 1,975 |
| BlueTS checker/parser/direct lowerer/page bridge | `checker/{project,module/}`, `parser/{declarations,type_syntax,runtime_syntax}`, and `bluets-bluejs/{expression,page_runtime,debug_attachment,tests/}` | `checker/tests.rs`, 1,361 |

## Enforcement for subsequent work

1. Re-run this inventory whenever a changed Rust file reaches 1,200 lines or gains an independently evolving responsibility.
2. Before a file would exceed 2,200 lines, extract the smallest cohesive child module and add focused public-boundary tests. Record the seam in the relevant phase plan or conformance document.
3. Preserve public APIs in parent modules; child modules own data tables, parsing/rendering flows, feature-specific behavior, or focused tests.
4. Do not split only to reduce a count. A localized correction in a coherent module is preferable to a gratuitous abstraction; an exception above 2,200 requires a written rationale and cannot be assumed.
5. Temporal code (`backend/bluejs/src/vm/temporal/` and its `tests/temporal_*`/`coverage_temporal_*` suites) has a stricter limit of **1,500 lines per Rust file**. A file that would cross it is split into a facade plus focused children as a separate pure-move commit (same public paths, unchanged tests) before further behavior is added. `vm/builtins/native_dispatch/dispatch.rs` and `native.rs` also list Temporal natives but are general engine files above 1,500 lines; they follow the general 2,200-line target and are not split as part of Temporal work.
