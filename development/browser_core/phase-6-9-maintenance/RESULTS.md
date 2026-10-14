# Phase 6–9 maintenance verification

Date: 2026-10-01. Baseline: `35dff6bfe` (`Gate Unix-only backends behind cfg(unix) and keep history generations monotonic`).

## Scope and existing fixes

This pass reviews and restructures existing Phase 6–9 behavior. It adds no browser tools, extension capabilities, wire variants, model backends, or platform transports. The relevant shared MCP, core, launcher and browser IPC boundaries are included. Separately owned compiler/script implementations and the Phase 10 downloads protocol are outside this pass.

The baseline includes the previously reviewed corrections: Unix-only service boundaries now compile and fail closed on Windows; launcher fixtures no longer copy the retired `bluejs` executable; history snapshots continue frame/document generations; engine HTTP requests preserve transfer headers; ephemeral extension tickets use operating-system entropy; and concurrent test fixtures use separate socket/directory identities and complete HTTP request framing.

## Module boundaries

- Scenario agent: configuration, stdio/model transport, bounded scenario validation, runner, and tests. Source files use `blueice-scenario-agent` and `scenario_agent_binary` names; an explicit Cargo target preserves the existing `blueice-phase6-agent` CLI without adding a second executable. Browser IPC tests use `browser_protocol_tests.rs`. The backend source filename audit finds no remaining numbered-phase names.
- MCP server: browser, transfer, compiler and locale tool routers, merged into the same tool catalog with the existing conditional output-tool filtering; core process ownership stays separate from message sequencing.
- Phase 7/8 launcher: assistant supervisor tests, cutover pipeline, trusted window requests, process lifetime management and permission worker. Public launcher functions and types retain their original imports.
- Phase 9 extension host: registry, storage, authentication/delegates, shared enforcement helpers and connection lifecycle. DOM writes, network rules and storage requests have separate handlers. Grant generations are still captured by the connection loop before dispatch; an early denial returns to that loop so later requests remain valid.
- Shared core: extension requests, session loop, assistant tasks, navigation, DOM writes/helpers, extension startup and protocol listeners. `Page`, `TabManager` and process ownership remain in the same owning layers.
- Large test files: domain modules reuse their original parent fixtures. Exact subprocess test entry names stay at the integration-test root. All 491 moved/directly affected tests retain their normalized bodies and literal contents. A separate function-body audit matches 1,288 of the 1,289 existing functions; only the connection dispatcher changes structurally, with its existing DOM/network/storage arms extracted into three handlers and a combined MCP router added. The 23 multiline fixtures affected by module indentation were restored byte for byte.

All 18 oversized files in this scope are below 1,300 lines after extraction. The largest affected Rust file, including new modules, is 1,283 lines. All 93 new Rust files have the existing MPL-2.0 header.

| Original file (current path) | Before | Entry file after |
|---|---:|---:|
| [backend/core/engine/src/bin/blueice-core.rs](../../../backend/core/engine/src/bin/blueice-core.rs) | 2,410 | 1,039 |
| [backend/core/engine/src/page.rs](../../../backend/core/engine/src/page.rs) | 3,142 | 1,010 |
| [backend/core/engine/src/session.rs](../../../backend/core/engine/src/session.rs) | 3,304 | 1,066 |
| [backend/core/engine/src/session/feature_tests.rs](../../../backend/core/engine/src/session/feature_tests.rs) | 5,393 | 225 |
| [backend/core/engine/src/tabs.rs](../../../backend/core/engine/src/tabs.rs) | 2,220 | 1,248 |
| [backend/core/engine/tests/core_binary.rs](../../../backend/core/engine/tests/core_binary.rs) | 3,828 | 851 |
| [backend/extension/src/lib.rs](../../../backend/extension/src/lib.rs) | 8,097 | 169 |
| [backend/extension/src/runtime.rs](../../../backend/extension/src/runtime.rs) | 2,391 | 1,018 |
| [backend/extension/tests/extension_host_binary.rs](../../../backend/extension/tests/extension_host_binary.rs) | 1,499 | 542 |
| [backend/ipc/src/lib.rs](../../../backend/ipc/src/lib.rs) | 1,444 | 888 |
| [backend/launcher/src/assistant.rs](../../../backend/launcher/src/assistant.rs) | 1,344 | 527 |
| [backend/launcher/src/unix.rs](../../../backend/launcher/src/unix.rs) | 2,437 | 988 |
| [backend/launcher/src/unix/tests.rs](../../../backend/launcher/src/unix/tests.rs) | 2,561 | 169 |
| [backend/launcher/tests/broker_end_to_end.rs](../../../backend/launcher/tests/broker_end_to_end.rs) | 1,540 | 340 |
| [backend/mcp-server/src/bin/blueice-scenario-agent.rs](../../../backend/mcp-server/src/bin/blueice-scenario-agent.rs) | 1,569 | 92 |
| [backend/mcp-server/src/server.rs](../../../backend/mcp-server/src/server.rs) | 1,966 | 677 |
| [backend/mcp-server/src/unix.rs](../../../backend/mcp-server/src/unix.rs) | 1,359 | 1,188 |
| [backend/mcp-server/src/unix/tests.rs](../../../backend/mcp-server/src/unix/tests.rs) | 1,703 | 96 |

## Native validation

All final checks passed on Linux (`ssh pb60g.bravotekcorp.cc`) and Windows MSVC in the running `omnisolve-winui-automation` KVM on that host. The final local/Linux/Windows filename/content SHA-256 digest matches across 110 changed/new Rust files and the Cargo target manifest:

`8c193158bf4ed0c5582f1c76a7f7b676393f4c43f863a124b4ed631caa27db3c`

| Gate | Linux | Windows MSVC |
|---|---|---|
| `cargo build --workspace --all-targets --locked` | Passed | Passed |
| Remaining workspace tests, excluding the four unchanged foundation crates | 2,608 passed, 0 failed, 5 ignored | 975 passed, 0 failed, 0 ignored |
| Foundation doctests / full workspace doctests respectively | 2 passed, 0 failed | 2 passed, 0 failed |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | Passed | Passed |
| `cargo fmt --all -- --check` | Passed | Passed |
| Final source SHA-256 | Matched | Matched |

After the last Unix-only storage-helper import and comment adjustments, Linux also passed the extension host's 137 tests and reran all-target workspace build/Clippy/format checks. The final filename pass passed 384 MCP/browser IPC tests and reran all-target workspace build/Clippy/format checks on Linux. Windows reran its complete gate after the filename changes. Cargo metadata confirms the same two binary targets, with the existing CLI name pointing at the renamed scenario source. Previously completed unchanged-foundation results contain 4,452 Linux and 4,447 Windows passes; including these results gives 7,062 Linux and 5,424 Windows passes without double-counting focused reruns.

The four unchanged foundation crates (`blueice-bluejs`, `blueice-bluets`, `blueice-bluets-bluejs`, `blueice-ecma402`) retain the successful native baseline results. The remaining workspace tests, workspace/all-target build, doctests, Clippy with `-D warnings`, and formatting were rerun for this refactoring. This is partitioned workspace validation, not a claim that a new unfiltered `cargo test --workspace` completed.

Windows checks cover portable behavior and explicit unavailable-platform responses. Existing Unix IPC/extension runtime restrictions remain; this pass does not implement or demonstrate a Windows browser GUI. A live configured LLM/human-window Phase 6 evidence run and a fresh coverage measurement are not part of this maintenance gate.
