# BlueJS large-file audit

Audited 2026-09-23. This audit covers every Rust file in `backend/bluejs/`
that was at least 1,500 lines before this change. Line count alone is not a
reason to split an execution-critical unit: a split must have an ownership
boundary that does not expose VM internals or separate a single dispatch loop.

| Area | File(s) audited | Result |
| --- | --- | --- |
| VM façade | `src/vm.rs` (2,345 lines) | Refactored. It mixed lifecycle entry points, foundational intrinsic installation, completion records, and numeric helpers. These now live in `vm/lifecycle.rs`, `vm/intrinsics.rs`, `vm/completion.rs`, and `vm/operations.rs`; the façade is 1,403 lines. |
| VM module graph | `src/vm/modules.rs` (1,977 lines) | Retain its existing `deferred`, `namespace`, and `synthetic` modules. The remaining code coordinates one static graph and its continuations; splitting the coordinator would make rooted-state ownership less clear. |
| VM execution | `src/vm/execution.rs` (1,927 lines) | Retain as the execution-context owner. Its global, eval, scope, completion, and root methods share the same frame invariants. |
| VM interpreter | `src/vm/interpreter.rs` (1,745 lines) | Retain as one opcode dispatch loop. Extracting opcode arms would complicate stack/PC ownership without a stable sub-API. |
| Test262 membrane | `src/vm/test262/foreign.rs` (1,977 lines) | Retain as the single cross-realm membrane boundary; foreign value, buffer mirror, and call forwarding must preserve common rooting and identity rules. |
| Object built-ins | `src/vm/builtins/object.rs` (1,560 lines) | Retain ordinary-object and Proxy internal methods together; proxy traps delegate directly to the corresponding ordinary operations. |
| Array built-ins | `src/vm/builtins/arrays.rs` (1,561 lines) | Retain shared array iteration and species helpers with their callers. |
| Binary-data built-ins | `src/vm/builtins/binary_data.rs` (1,706 lines) | Retain ArrayBuffer, DataView, Atomics, and TypedArray operations because they share validation and resize/detach invariants. |
| Generator built-ins | `src/vm/builtins/generators.rs` (1,860 lines) | Retain synchronous and asynchronous generator transition code as one state-machine boundary. |
| Native dispatch | `src/vm/builtins/native_dispatch/dispatch.rs` (1,887 lines) | Retain one exhaustive `NativeFunction` dispatch match; its exhaustiveness is a correctness property. |
| Iterator runtime | `src/vm/builtins/execution/runtime.rs` (1,680 lines) | Retain iterator closing and callback helpers with the stateful iterator runtime. |
| Compiler façade | `src/compiler.rs` (1,967 lines) | Already delegates expression, function, private-name, and statement compilation to dedicated modules; retained as compiler state and bytecode assembly owner. |
| Compiler expression/statement passes | `src/compiler/expressions.rs` (1,968), `src/compiler/statements.rs` (1,832) | Existing pass boundaries are appropriate. Both files are single `Compiler` passes whose helpers share temporary slots, scopes, and bytecode offsets. |
| Heap | `src/heap.rs` (1,648 lines) | Already delegates storage, exotic objects, lifecycle, binary data, and collection iteration. The root owns internal data representations used across those modules. |
| AST | `src/ast.rs` (1,771 lines) | Retain one public AST schema. Splitting types would add re-export churn without reducing a behavioral implementation; its executable scanners are covered by focused tests. |
| Native function enum | `src/native.rs` (1,822 lines) | Retain one canonical `NativeFunction` inventory so dispatch remains exhaustive and object references remain centrally auditable. |
| Tokenizer | `src/token.rs` (1,911 lines) | Production tokenizer ends before line 1,275; the remaining lines are inline tests. No production-size refactor required. |
| Test-only sources | `tests/intl.rs` (2,130), `src/parser/tests.rs` (1,820) | Retain grouped conformance fixtures/tests; production modules are not enlarged by them. |

The refactor deliberately preserves public APIs and keeps cross-cutting VM
records private to `vm`. The full BlueJS test suite and Clippy are the required
post-audit verification gates.
