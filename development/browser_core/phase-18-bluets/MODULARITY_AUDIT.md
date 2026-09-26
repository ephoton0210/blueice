# BlueTS modularity audit

Snapshot: 2026-09-27, after C3.1.3.4.7.4. Rust line counts use `wc -l` on
tracked source and test files. The threshold is a review trigger, not a reason
to split a coherent type or copy code into arbitrary small files. A split must
give one concern an owner, preserve the public ABI and test names, and avoid
duplicating private validation logic. Run focused tests after each move and
the serial workspace gate before closing this checkpoint. Reuse `target/` and
prune only verified obsolete compiled test executables, not source tests.

## Direct BlueTS owners: split

| File | Lines | Boundary |
| --- | ---: | --- |
| `backend/bluets/src/compiler.rs` | 1,364 | Move its ~600 lines of inline compiler tests into `compiler/tests.rs`; retain compile/cache/project definitions in the owner module. |
| `backend/bluets/src/checker.rs` | 1,368 | Move cohesive call-inference/assignability helpers to a checker submodule; retain checked-project orchestration and public types here. |
| `backend/bluets/src/checker/tests.rs` | 1,469 | Group syntax/inference and project/generic contract tests into focused test modules, keeping fixtures and coverage. |
| `backend/bluets-bluejs/src/lib.rs` | 1,711 | Move direct AST lowering and provenance generation out of the bridge facade, then separate safe-point attachment/map construction; keep public types and ABI constants stable. |

## Mixed runtime, transport, and acceptance owners: staged splits

These files contain substantial BlueTS-owned paths but also serve BlueJS or
browser-generic behavior. Move a complete concern at a time, with its exact
private validation and regression tests; do not duplicate an entire file or
change wire variants to achieve a line-count target.

| File | Lines | First BlueTS boundary to evaluate |
| --- | ---: | --- |
| `backend/core/engine/src/script/javascript_child.rs` | 14,468 | Separate its ~7,450-line inline test module, then the BlueTS static metadata/linked debugger adapter from the general child transport/executor. |
| `backend/launcher/src/bluejs_host.rs` | 14,133 | Separate its ~6,425-line inline test module, then BlueTS prepare/execute and debugger metadata handlers from general host lifecycle. |
| `backend/core/engine/src/debugger.rs` | 11,744 | Group BlueTS static relation, metadata receipts, and linked scope dispatch behind one internal module boundary. |
| `backend/ipc/src/debugger.rs` | 7,103 | Group BlueTS metadata/static-scope wire shapes and validators; preserve protocol v41 and serde encoding. |
| `backend/ipc/src/page_host.rs` | 3,595 | Group BlueTS child debug/metadata wire shapes without changing the private protocol. |
| `backend/launcher/tests/out_of_process_debugger.rs` | 7,474 | Extract related BlueTS socket fixtures and scope/reload tests into test submodules sharing one process harness. |
| `backend/core/engine/tests/core_binary.rs` | 4,141 | Extract the BlueTS real-binary cases from generic core lifecycle cases without copying the socket helpers. |
| `backend/core/engine/src/compiler_ipc.rs` | 3,180 | Isolate compiler transport dispatch from project/receipt validation. |
| `backend/core/engine/src/compiler_service.rs` | 1,851 | Isolate sealed-project query handling from catalog retention/policy. |
| `backend/ipc/src/compiler.rs` | 1,361 | Separate compiler wire validation from data shapes if the split keeps public re-exports stable. |
| `backend/mcp-server/src/server.rs` | 1,842 | Continue moving compiler-specific handlers into existing `server/compiler_support.rs`. |
| `backend/mcp-server/src/lib.rs` | 1,748 | Separate compiler setup/routing from generic MCP startup. |
| `backend/mcp-server/tests/core_process.rs` | 1,646 | Group compiler public-process cases by session/authorization concern. |

## Reviewed adjacent files: defer a BlueTS-specific split

The following exceed 1,300 lines and mention BlueTS, but their large body is
primarily shared browser/BlueJS lifecycle or debugger machinery. Extracting
only scattered BlueTS references would increase coupling. Re-evaluate each
after the direct-owner and mixed-transport splits; any remaining general
monolith should get its own repository-wide maintenance task, rather than be
silently relabeled BlueTS work.

| File | Lines | Current owner |
| --- | ---: | --- |
| `backend/launcher/src/lib.rs` | 3,267 | Launcher process/startup supervision |
| `backend/bluejs/src/vm/debugger.rs` | 3,239 | BlueJS VM debugger machinery |
| `backend/core/engine/src/session/tests.rs` | 2,783 | Browser session integration tests |
| `backend/core/engine/src/bin/blueice-core.rs` | 2,560 | Core CLI and process wiring |
| `backend/core/engine/src/script/javascript.rs` | 2,188 | Inline BlueJS executor |
| `backend/core/engine/src/script/javascript/debugger_support.rs` | 2,073 | BlueJS debugger adapter |
| `backend/core/engine/src/script/http_resource_authorizer.rs` | 1,495 | HTTP source authorization |
| `backend/core/engine/src/session.rs` | 1,470 | Browser session orchestration |
| `backend/launcher/src/bin/blueice-launcher.rs` | 1,445 | Launcher CLI and process wiring |
| `backend/core/engine/src/script/direct_page.rs` | 1,405 | Generic direct-page execution |

Order: split compiler/checker tests and helpers, then the direct bridge,
then mixed runtime/IPC and public acceptance files in file-scoped leaves.
Recount after each leaf. Complete the listed BlueTS-owned boundaries before
closing C3.1.3.4.7.5; record any justified deferral explicitly in PLAN.md.

Progress: C3.1.3.4.7.5.2.1 moved the compiler's inline tests into a
600-line child module; `compiler.rs` is now 766 lines. Snapshot counts above
remain the original audit baseline for comparison.

C3.1.3.4.7.5.2.2 split the checker tests into 778-line expression/inference
and 697-line project-contract modules. The original test functions remain;
only their Rust module paths for the moved cases changed.

C3.1.3.4.7.5.2.3 moved type-relation helpers into a 301-line internal
module; `checker.rs` is now 1,080 lines. The direct compiler/checker phase is
complete without changing public compiler diagnostics or output.

C3.1.3.4.7.5.3.1 moved direct AST lowering and provenance to a 342-line
module; the bridge facade is now 1,378 lines pending attachment/map extraction.

C3.1.3.4.7.5.3.2 moved attachment and safe-point-map construction to a
524-line internal module; the bridge facade is now 866 lines. Direct BlueTS
compiler and bridge files from this audit are all below 1,300 lines.

C3.1.3.4.7.5.4.1.1 externalized the child executor's inline tests with their
original paths. `javascript_child.rs` is 7,016 lines and its new test module
is 7,344 lines; both still need the planned concern-level splits.

C3.1.3.4.7.5.4.1.2 separated the private static/linked adapter, linked pause,
and scope/value cases into 486-, 1,226-, and 992-line test children. The
shared fixture and remaining integration-test owner is 4,658 lines; it still
requires the planned transport/authorization and real-child metadata splits.

C3.1.3.4.7.5.4.1.3 moved transport/session, resource-accounting, and closed
authorization cases to 432-, 294-, and 506-line children. One 3,444-line
fixture/real-child test owner remains for the final test-family split.

C3.1.3.4.7.5.4.1.4 completed the child-executor test-family split: the
shared-fixture owner is 407 lines and every child file is below 1,300 lines
(maximum 1,226). Metadata, span envelopes, nested pause, stack/scope, and
lifecycle cases are independent modules; the 7,016-line production executor
still awaits its separate adapter extraction.

C3.1.3.4.7.5.4.2.1 externalized the Launcher host's inline tests without
rewriting them. The production `bluejs_host.rs` is 7,709 lines and the new
test owner is 6,394 lines; both require their planned concern-level splits.

C3.1.3.4.7.5.4.2.2 moved DOM/transport, nested scheduler, exception-site,
and static-scope/value cases to 704-, 659-, 261-, and 636-line children.
The shared-fixture/remaining host test owner is 4,158 lines pending the
metadata and admission/step splits.

C3.1.3.4.7.5.4.2.3 moved resource accounting, debugger control, metadata
roots, metadata inventory/contracts, and source-span cases into 184-, 516-,
520-, 690-, and 289-line modules. The fixture/admission/step parent is now
1,989 lines; one final test-family split remains.

C3.1.3.4.7.5.4.2.4 completed the Launcher host test-family split. Admission,
module stepping, and linked-module cases now own 533-, 940-, and 417-line
children. The fixture parent is 117 lines and the largest file across the
entire host test family is 940 lines. The 7,709-line production host remains
for its separate prepare/debugger modularization leaf.

C3.1.3.4.7.5.4.3.1 moved the public child-client transport contract into a
765-line internal module and kept the original public re-export. The
production child executor facade is 6,259 lines; socket transport, private
adapters, core debugger routing, and authorization/reply helpers are distinct
follow-up seams.

C3.1.3.4.7.5.4.3.2 moved the authenticated socket transport and its full
child-client implementation to a 901-line internal module. Existing private
transport tests retain parent-only access; the public connection type is
re-exported unchanged. The production executor facade is 5,365 lines and
still needs private adapter and core debugger concern splits.

C3.1.3.4.7.5.4.3.3 moved strict private linked/static-scope child reply
adapters to a 277-line internal module. The production facade is 5,097 lines;
the large core debugger trait implementation and authorization/report helpers
remain under audit.

C3.1.3.4.7.5.4.3.4.1 moved the intact core debugger trait implementation
to its own 2,865-line internal owner, separating it from the 2,238-line
executor facade. Both remain above threshold; metadata, linked/nested
frame/scope, and root execution methods require bounded delegated owners.
