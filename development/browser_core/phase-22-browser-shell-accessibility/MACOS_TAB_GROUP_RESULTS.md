# macOS native tab group results

Validation finished: 2026-10-03T03:22:03+08:00.
Base: `0609c8a813f6da5f0da1faace410bff9027e8651`.
This records the native group increment in the active
[macOS delivery plan](MACOS_DELIVERY_PLAN.md); multiple-window/profile delivery
and the other browser milestones remain open.

## Delivered behavior

The SwiftUI tab strip consumes Phase 16's core group IDs, names, colors,
collapsed state and nullable tab membership. The toolbar, tab/group context
menus and View > Tab Groups create, edit, collapse/expand, move/ungroup and
remove groups. Command-Option-G opens the native sheet. Palette buttons and a
hex field edit the color; names use the core's nonempty trimmed, 80-Unicode-scalar
limit. Core replies supply the canonical trimmed name and lowercase color.

Collapsing hides member tab buttons while their selected page remains live.
Removing a group leaves all member tabs open and ungrouped. Empty groups remain
editable after closing their last tab, and creating an empty group works when
no tab is selected. The sheet captures its original tab/group ID; removing that
group through the native menu preserves the draft, displays the closed-target
message and disables Save. Cancel creates no group. Group headers expose their
name, member count, expanded/collapsed state and selected membership to native
accessibility, and explicit contrast borders remain on group/toolbar controls.

The model changes group metadata only from core replies/broadcasts. It preserves
tab IDs, document identity, edited values, history and zoom. Metadata commands
make no HTTP request. Exact request IDs and global/tab-bound replies determine
completion. Reply ownership is registered before writing, using the same FIFO
main queue as inbound delivery, so an immediate group error cannot replace a
navigation or policy-denial message. Group errors have a separate sheet/notice
and dismissal. Only explicit null assignment removes membership; malformed
missing membership, invalid IDs, duplicate group IDs and invalid names/colors
fail soft. Missing membership on legacy TabSummary still defaults to ungrouped.

## Final verification

| Check | Authoritative final result |
| --- | --- |
| XCTest | 54 passed, including four new protocol/real-core group cases |
| Actual XCUITest | 31 passed; one physical Zhuyin skip for missing runner Accessibility trust |
| Repeated real-core group boundaries | Two cases, ten iterations each: 20 successful executions |
| Existing Rust contracts | 203 passed: one engine group case, 201 IPC tests and one MCP/real-core group case |
| Native build | Debug Swift app built for arm64 and x86_64; runtime acceptance on Apple Silicon |
| Formatting/project/signatures | Rustfmt, Git whitespace, Xcode plist and strict local app/service signatures passed |
| Owned app/services | Normal teardown completed; zero exact bundled app/service processes remained |

Complete native bundle: `frontend/macos/.build/results-20261003-030502.xcresult`.
The structured report contains 86 tests: 85 passed,
zero failed and one skipped, ending at 2026-10-03T03:19:09+08:00.
23 diagnostics are XCTest internal priority-inversion warnings;
zero SwiftUI view-update state warnings were observed. Every previous native
case remained enabled in the final full run. Repeated boundary bundle:
`frontend/macos/.build/results-20261003-030227.xcresult`; the structured summary
lists two unique tests, while the execution log records all 20 passing iterations.

The three new window tests exercise toolbar/context/View-menu creation,
Command-Option-G, trim/length/hex validation and Cancel, collapse with retained
selected content, moving a second tab into/out of a group, rename/recolor and
removal without closing either tab, last-tab/empty-group lifecycle and a stale
open editor. Screenshot pixels converted from the embedded monitor profile to
sRGB verify actual red and green group indicators. They retain the original
edited 中文 text and 150% zoom; the fixture's exact request count stays one
through all organization actions, then becomes two after a genuine history
navigation. Real-core cases separately assert unchanged document generation,
member IDs, editor values and history, plus core errors on both reply routes
that leave a prior navigation denial intact.

The initial public tests failed while the group wire/model APIs were absent.
An early UI diagnostic used an unscoped menu query and matched both the browser
context menu and the same title under View. Queries now select the browser-window
menu or the menu bar explicitly; the original interaction/content assertions
remain. Diagnostic failures and incorrectly filtered test invocations are
excluded from final acceptance.

## Reproduction and artifact identity

```sh
export CARGO_TARGET_DIR="$PWD/frontend/macos/.build/core-target"
export CARGO_TARGET_AARCH64_APPLE_DARWIN_RUNNER="$PWD/frontend/macos/TestSupport/run-signed-test.sh"
"$HOME/.cargo/bin/rustup" run 1.96.0 cargo test -p blueice-engine tab_group --locked --offline
"$HOME/.cargo/bin/rustup" run 1.96.0 cargo test -p blueice-ipc --lib --locked --offline
"$HOME/.cargo/bin/rustup" run 1.96.0 cargo test -p blueice-mcp-server --test core_process \
  tab_groups_round_trip_through_mcp_and_a_real_core --locked --offline
frontend/macos/test.sh
frontend/macos/test.sh \
  -only-testing:ProtocolTests/NativeEditingTests/testNativeTabGroupsPreserveCoreDocumentEditorHistoryAndZoom \
  -only-testing:ProtocolTests/NativeEditingTests/testTabGroupValidationAndCoreErrorsPreserveNavigationDenial \
  -test-iterations 10
"$HOME/.cargo/bin/rustup" run 1.96.0 cargo fmt --all -- --check
```

Host: macOS 26.6.2 (25G83), Xcode 27.0 (27A266a), Swift 6.4,
Rust 1.96.0, Apple Silicon. Local socket/process fixtures and Xcode GUI/report
services require host permissions. Tests isolate and remove only their own
preference domains, and restore the original clipboard and keyboard source.
All ten changed Swift/project inputs stayed unchanged through the repeated
core checks and final full native run. Sorted relative path, NUL, raw bytes,
NUL produce SHA-256
`fe310ed239e06833d12cef127f8d95a440752ba74d0fe1c6126022db0ba79d6d`; documentation and generated artifacts are excluded.

The directly inspected window attachment is `artifacts/macos-tab-groups.png`,
SHA-256 `e912303eae23e35c51678ee8b877bf1c2e450db5d1411333c028e177d16229e1`. It shows the native green Work group, two retained
core tabs, edited 中文 text and 150% zoom. The exported image is unedited.
Screenshots, result bundles, local logs and build products stay ignored and
untracked. Compact evidence is in `artifacts/macos-tab-group-results.txt`.

## Remaining acceptance

Group state currently lives in the running core session. Restart restoration,
drag reordering and cross-window/profile lifecycle remain open. Multiple native
windows need independent core viewport routing and tab transfer that retains
identity/history/document state; separate reloaded core copies would not fulfill
that contract. The global group metadata API remains unchanged; rename and
color updates are separate acknowledged core mutations.

Physical OS IME, actual VoiceOver, monitor/system setting transitions,
localization, remaining editing/form behavior, downloads/printing, trusted
human permission/assistant panels and final browser acceptance remain in the
plan. This run does not establish Linux/Windows native UI, Intel/older macOS
runtime, full-workspace Rust test execution/coverage, distribution signing or
notarization. No system privacy permissions were changed.
