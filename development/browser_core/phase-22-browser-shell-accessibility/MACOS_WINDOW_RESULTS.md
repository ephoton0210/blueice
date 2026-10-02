# macOS shared-core window results

Base: `6e141967f9218633337a6891115f1c04f55d632a`.
This records the native multi-window increment in the active
[macOS delivery plan](MACOS_DELIVERY_PLAN.md). Profile/context lifecycle,
session restoration, drag reordering and the other browser milestones remain open.

## Delivered behavior

Command-N / File > New Window creates an actual AppKit window. Window menu
entries activate it, and tab context menus move an existing tab to another
window or to a new empty window. All windows share one owned launcher, core and
gatekeeper session. The active window supplies native editing, find, zoom and
group commands. Closing one window closes only its tabs; the last native window
requests application exit and owned service teardown. Close requests wait behind
an in-flight window operation instead of silently disappearing.

The core owns monotonic window IDs, ordered membership and each window's logical
viewport/backing scale. Native resize, legacy Resize and SetViewport reflow only
members of the addressed window, including background pages and retained history.
Moving a tab keeps the same page/document, committed text, selection, history,
group and zoom; it does not fetch again. The destination receives fresh geometry,
while the source loses its old semantic and input state. Transfer bumps the focus
generation and ends temporary composition. Native queued page commands also carry
their originating window ID; stale source-window commands and nested window
commands are rejected before dispatch. IDs confer no human trust or permission.

The additive Window / WindowState protocol leaves existing TabSummary and
TabOpened shapes intact. Legacy clients receive no registry snapshots until
window actions opt the session in. Legacy OpenTab retains default window 1 and
reports its closure instead of silently targeting another window. New membership
precedes navigation review,
so denied/failed tabs retain canonical ownership and can be closed. A denied
context-menu Open Link in New Tab retains the source URL/page and removes the
failed destination, preserving the existing visible-denial contract. Generic
new-page failures remain visible in their owning window. MCP and the reference
frontend ignore registry notifications without releasing completion barriers.

A single reader preserves FIFO envelope/frame order and dispatches canonical
membership to per-window models. Malformed metadata disables management and
provides Retry while preserving native windows. Successful Retry fetches fresh
history, pixels, semantics and editing state even when membership is unchanged;
it preserves document identity and performs no HTTP request. Window errors have
their own notice. Identical membership snapshots preserve address drafts and policy
messages in other windows. Legacy global tab lists update values in canonical
window-local order; a late list cannot reorder moved members. Shared display
preferences are written once per workspace; each window retains its own viewport
and each tab its own zoom.
Find queries/options and pending resubmission prompts follow the moved tab;
find geometry is refreshed against the new core input identity.

## Final verification

The machine-readable record is [artifacts/macos-window-results.txt](artifacts/macos-window-results.txt).
The full native acceptance run finished at `2026-10-03T05:42:56+08:00`.
Its authoritative bundle is
`frontend/macos/.build/results-20261003-052507.xcresult`.

| Check | Result |
| --- | --- |
| XCTest, including bundled shared-session and wire tests | 56 passed, 0 failed |
| Actual AppKit-window XCUITest | 34 passed, 0 failed, 1 physical Zhuyin case skipped |
| Native bundle total | 91 tests: 90 passed, 1 skipped, 0 failed |
| Affected Rust packages: engine, IPC, MCP, launcher, reference frontend | 1,761 passed, 0 failed, 4 existing manual/environment cases ignored across 47 suite groups |
| New public core window regressions | 7 passed, included in the Rust total |
| Workspace/all-target build and Clippy with warnings denied | Passed with Rust 1.96.0, locked/offline dependencies and the existing cache |
| Rustfmt, whitespace and Xcode project plist | Passed |
| App and all three bundled service signatures | Verified local ad hoc signatures |
| Owned app/launcher/core/gatekeeper processes after teardown | 0 |
| Native app architectures / runtime acceptance | arm64 + x86_64 / arm64 |

The final bundle reports 26 XCTest `[Internal]` QoS priority-inversion warnings
and zero SwiftUI state-during-view-update warnings. These runtime diagnostics
are retained separately from compiler/Clippy results. The four ignored Rust
case names and requirements are listed in the machine-readable record. A whole
workspace test rerun and fresh coverage measurement are not claimed; all changed
Rust packages were tested, and all workspace targets were built and linted.

The final source fingerprints match the frozen acceptance inputs:

- 27 Rust/Swift/project files:
  `0edda2148c3a12937cddfac216949a3c8a3c163e3d8df34ef45ca81b9ae99c40`.
- 12 native Swift/project files:
  `58b4acefbefbfac9df1513c3104397d0b3720f05d3e49ef68e8a8f6b6582a7c9`.

Each fingerprint hashes sorted relative path bytes, NUL, file bytes, NUL.
The record lists every input. Documentation/result records are outside that
source fingerprint; tested implementation files remained unchanged.

The unedited actual-window attachment was exported as
`2503780C-29A8-4471-9F41-A707BC5EF512.png` and retained locally at
`artifacts/macos-shared-core-windows.png` (SHA-256
`843010a649a0e8a78553445f46ac6b6debc732e66d7646322dff4cabe15a7aad`).
It shows Window 3 retaining the Shared group and `After close 中文` after the
original window closed and the same page moved again. Screenshots and result
bundles remain ignored by Git; the result record is tracked.

The new actual-window cases exercise Command-N, Window menu activation, moving
a grouped edited Chinese tab to another existing window and then to a new window,
CSS sizing at 150% zoom, active-window shortcuts, closing the original window,
editing afterward, preserving independent destination tabs, last-window exit,
find query/result transfer and Find Next, and independently resizing a second
window while the first keeps its pixels and address draft. A real shared-session
XCTest verifies the same process ID throughout transfer/close, group/history,
document generation, independent density/zoom, preserved policy status, denied
new-tab ownership and malformed-registry recovery, unchanged HTTP request counts
and process-group/runtime cleanup.
The wire decoder rejects ambiguous ownership; public core tests cover stale
windows, invalid viewports, monotonic IDs, scoped close/resize and stale input.

Initial diagnostics identified a stale native menu binding after switching or
closing windows. An observed active-command layer now follows the key window.
Publishing pending membership also exposed an existing denied-link regression;
selection now waits for successful open, and the failed context destination is
closed through its correct window scope. A find test now waits for actual loaded
page semantics before invoking Find, so a navigation commit cannot legitimately
invalidate a query opened on the previous document. Original assertions remain.
A malformed-registry real-service regression first demonstrated missing input
and semantics after Retry; readiness recovery now requests fresh history and
frames. Repeated key-window notifications no longer republish the same active
model. The UI fixture explicitly refocuses the address after selecting the input
source before typing, avoiding an observed XCTest no-keyboard-focus dispatch
failure. The focused recovery/address cases passed before the final full run.
A further boundary regression reproduced a late global Tabs reply changing
window-local IDs `[2, 1, 3]` to `[1, 2, 3]`. The model now retains the canonical
ordered ownership list and resolves incoming values by ID, including after
opening/closing a further destination tab. Interrupted/failing diagnostic
bundles are excluded from final acceptance.

Broad compatibility testing identified an existing macOS large-echo test fixture
that wrote the complete 200 kB request before reading any response. A sampled
stack showed the client and both relay directions blocked in sendto. The test
fixture now drains replies while writing with bounded socket deadlines; the
unchanged full-size byte equality assertion remains. Production relay behavior
is unchanged. Launcher end-to-end tests require building the sibling assistant
binary as well as the three GUI services; the full workspace build supplies it.

## Reproduction and remaining scope

```sh
export CARGO_TARGET_DIR="$PWD/frontend/macos/.build/core-target"
export CARGO_TARGET_AARCH64_APPLE_DARWIN_RUNNER="$PWD/frontend/macos/TestSupport/run-signed-test.sh"
"$HOME/.cargo/bin/rustup" run 1.96.0 cargo build --workspace --all-targets --locked --offline
# Locally sign compiled Mach-O service executables before process tests on macOS.
"$HOME/.cargo/bin/rustup" run 1.96.0 cargo test \
  -p blueice-engine -p blueice-ipc -p blueice-mcp-server -p blueice-launcher \
  -p blueice-frontend-reference --locked --offline
"$HOME/.cargo/bin/rustup" run 1.96.0 cargo clippy --workspace --all-targets --locked --offline -- -D warnings
"$HOME/.cargo/bin/rustup" run 1.96.0 cargo fmt --all -- --check
frontend/macos/test.sh
```

Host: macOS 26.6.2 (25G83), Xcode 27.0 (27A266a), Swift 6.4,
Rust 1.96.0, Apple Silicon. Native app builds include arm64 and x86_64;
runtime acceptance is on arm64. The physical Zhuyin test remains an explicit
skip when the UI runner lacks Accessibility trust. Physical IME/monitor changes,
VoiceOver acceptance, full editing/DOM events, downloads/printing/trusted panels,
profile handoff, restoration, drag/drop and localization remain separate gates.
No new Windows GUI or Linux acceptance is claimed by this macOS record.
Automatic approval review rejected transmitting the source snapshot to the
owner-designated `pb60g.bravotekcorp.cc`, stating that the existing Linux-test
authorization did not explicitly cover transferring source code. No snapshot
was transmitted. Explicit transfer approval remains pending; this does not
prevent the separately authorized macOS milestone commit and push.
