# macOS browser contexts and persistent profile identities

Base: `b318e0de816a3ede4c94536ef1aa58fac4b5cfdf`.
This increment implements named live contexts and persistent profile identity in
the [macOS delivery plan](MACOS_DELIVERY_PLAN.md). Full session restoration and
storage partitioning remain open.

## Delivered behavior

The toolbar and native Profiles menu create, rename, open and remove profiles
through the core registry. New windows use the active context. Windows, tabs and
groups have canonical context ownership; removing a nondefault context retires
only its members. Default context 1 cannot be removed. Same-context transfer
retains the existing Page and its document/history/edited state. Cross-context
transfer or group assignment fails before mutation. All native windows continue
to use one owned launcher/core/gatekeeper session and mandatory URL/content review.
Runtime identities confer no controller lease, privacy grant or human permission.

The additive BrowserContext action/state protocol preserves existing TabSummary
and TabOpened shapes. Names are trimmed, NFC-normalized, bounded to 256 UTF-8 bytes
and unique under lowercase comparison; embedded controls are rejected. At most
16 contexts and 64 total windows are live. Runtime IDs are monotonic and stale
context/window commands are rejected. Nested scope wrappers and context-scoped
shutdown are rejected. Debugger realms report their actual owning context.
Canonical context/group metadata precedes window snapshots. A context-scoped tab
list updates known values without removing another context's canonical members.
Malformed registry metadata retains windows and offers Retry; recovery refreshes
history, pixels, semantics and input without an HTTP request or document replacement.

MCP `list_browser_contexts` reads the same core context, window and group registry.
It waits for the exact request, preserves unrelated tab/frame state and does not
let unsolicited snapshots release its completion barrier. No context lifecycle
or private human-grant MCP tool is added in this increment.

Bounded application preferences persist only profile names and logical UUID keys.
Relaunch recreates empty named contexts with fresh runtime IDs and preserves the
logical keys. Additional saved contexts allocate no Page/window until explicitly
opened; the compatibility default window still starts on about:credits.
Runtime tab/context/window IDs, URLs, POST bodies and edited text are absent from
this catalog. Malformed or oversized catalogs remain intact and disable profile
management. This is profile identity restoration, not restoration of open tabs.

## Verification

The full native acceptance run finished at `2026-10-03T06:59:06+08:00`.
Its authoritative bundle is
`frontend/macos/.build/results-20261003-063953.xcresult`; the machine-readable
record is [artifacts/macos-context-results.txt](artifacts/macos-context-results.txt).

| Check | Result |
| --- | --- |
| XCTest, including real bundled services and preferences/wire cases | 59 passed, 0 failed |
| Actual native-window XCUITest | 35 passed, 0 failed, 1 physical Zhuyin case skipped |
| Native bundle total | 95 tests: 94 passed, 1 skipped, 0 failed |
| Affected Rust packages: engine, IPC, MCP, launcher, reference frontend | 1,768 passed, 0 failed, 4 existing manual/environment cases ignored across 50 suite groups |
| New public context regressions | 5 engine + 1 IPC + 1 MCP passed, included in the Rust total |
| Workspace/all-target build and Clippy with warnings denied | Passed with Rust 1.96.0, locked/offline dependencies and the existing cache |
| Rustfmt, whitespace and Xcode project plist | Passed |
| App and all three bundled service signatures | Verified local ad hoc signatures |
| Owned app/launcher/core/gatekeeper processes after teardown | 0 |
| Universal app / native runtime | arm64 + x86_64 / arm64 |

The bundle reports 27 XCTest internal QoS priority-inversion warnings and zero
SwiftUI state-during-view-update warnings. These runtime diagnostics are separate
from compiler/Clippy results. A whole workspace test rerun and fresh coverage
measurement are not claimed. The four ignored Rust cases and their requirements
are listed in the machine-readable record. Failed diagnostic bundles are excluded
from final acceptance.

Frozen acceptance inputs matched after the full run:

- 32 Rust/Swift/project files:
  `8d728830df84ea1002b78037182099a8730ca4669ec5b75f21a777389d60c3ac`.
- 13 native Swift/project files:
  `2ab5e21d6e78c08491d7e5f8e6e51bf0c863c8dd59eb3c7c7c9652b0e4e90ed9`.

Each fingerprint hashes sorted relative path bytes, NUL, file bytes, NUL. The
record lists every input; documentation/results are outside these fingerprints.
Tested implementation files remained unchanged. The unedited actual-window
attachment `41945A3D-D41A-47AF-8E5E-334FD4662CD5.png` is retained locally at
`artifacts/macos-profile-contexts.png`, SHA-256
`2b84f97d97e1ddec3be2e8e08200101ac85d65b23b3668773ae7beac28347ef2`.
It shows the renamed 工作 profile in actual Window 3 after Window 2 closed.
Screenshots and result bundles remain ignored; result text is tracked.

New public core tests exercise context/window/group ownership, same-page transfer,
cross-context rejection, scoped close, monotonic IDs, bounded names, debugger
realm ownership and actual Unix session dispatch. IPC tests round-trip the added
actions and unchanged legacy tab shapes. A public MCP regression verifies exact
correlation, unsolicited metadata, unrelated live frame state and core errors.

The real bundled-service XCTest edits `Root 中文`, creates independently owned
groups, rejects cross-context moves/assignments, renames and closes contexts,
recovers malformed metadata, verifies stable process/document identity and no
extra HTTP requests, then restarts and checks a persistent UUID against a fresh
runtime context ID. Protocol/preferences cases cover duplicate namespace IDs,
invalid events and retained malformed catalogs. The actual-window XCUITest covers
native profile validation/create/rename, Command-N in the active context, closing
one member window, preserved source text, normal exit/relaunch, opening a saved
empty profile, removal Cancel/confirmation and survival of the default window.

Diagnostic runs exposed a partial context Tabs reply erasing unrelated native
tab values; the frontend now revokes membership only through WindowState and
retains existing values when a scoped list omits them. The profile sheet assigns
accessibility identifiers to its individual controls. New AppKit windows cascade,
and UI actions activate the intended window and qualify the profile submenu.
Profiles commands and toolbar observe workspace readiness/busy state directly,
so an in-flight window operation cannot leave them permanently disabled.

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
Rust 1.96.0, Apple Silicon. The universal app includes arm64 and x86_64; runtime
acceptance is arm64. Physical Zhuyin remains explicitly skipped when the UI runner
lacks Accessibility trust. Physical IME and VoiceOver acceptance remain open.
Current networking has no cookies, cache or authentication. Private browsing,
per-profile storage/assistant/extension/download/display settings, durable tab
restoration, drag/drop, printing and the other delivery gates are separate work.
No new Windows or Linux acceptance is claimed. Automatic approval review rejected
transferring the source snapshot to the owner-designated pb60g.bravotekcorp.cc,
stating that Linux test authorization did not explicitly authorize source transfer.
No snapshot was transmitted; explicit transfer approval remains pending.
