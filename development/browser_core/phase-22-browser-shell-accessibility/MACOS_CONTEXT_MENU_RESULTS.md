# macOS native context menu results

Validation finished: 2026-10-03T00:00:35+08:00. Base: `614489c595f733cdc29a3235ea3c94e1ae5c9a26`.
This records the link/editor/page menu increment in the active
[macOS delivery plan](MACOS_DELIVERY_PLAN.md), not full-browser completion.

## Delivered behavior

- Actual AppKit NSMenu popup from right-click, Control-click and Shift-F10 on
  a focused editor, with native keyboard navigation, Escape and accessibility
  menu items. SwiftUI continues to display core-owned shared pixels.
- Core hit testing in current viewport coordinates. Tab, frame source and
  frame generation are checked before inspecting/focusing the target; menu
  actions additionally check the document identity. The core rederives a link
  at execution rather than trusting a frontend-supplied URL or node copy.
- Link Open, Open in New Tab and Copy Link Address. Open retains ordinary
  URL/content review. Copy returns only the public resolved URL without a
  fetch; a correlated explicit native action writes the clipboard. The core,
  page and MCP do not gain access to the OS clipboard.
- Right-click focus on a supported native text control without click default
  activation or selection replacement. Only the hit editor's state is
  returned. Password plaintext is redacted, disabled/hidden/inert/opacity-zero
  targets do not provide target-specific actions, and unsupported URL schemes are not
  offered. URLs are bounded to 8 KiB; replies use the existing IPC frame cap.
- Native Cut/Copy/Paste/Select All use the acknowledged input queue. Password
  Copy/Cut and readonly Cut/Paste are disabled; readonly selection/Copy work.
  Enabling Paste does not inspect the clipboard. Page Back/Forward/Reload and
  Find use their existing core/navigation/resubmission paths.
- OpenTab's committed URL now survives the Swift decoder and native model;
  a successfully loaded link tab is not navigated again to credits. A denied
  new-tab destination is reported on its source page and its unpublished empty
  tab is closed. Pending new-tab completions have their own bounded observation
  slots rather than sharing a popup's lifetime.
- Native menus cancel after lifecycle changes and callbacks recheck the live
  frame/tab. Canceled async menu requests stop promptly; waits and early-reply
  buffers are bounded. No new DOM copy, native network loader or privacy grant
  was introduced.

## Final verification

| Check | Authoritative final result |
| --- | --- |
| XCTest | 42 passed: existing accessibility, protocol/process, real-core editing/keyboard/form/find, plus native menu state and malformed wire regressions |
| Actual XCUITest | 24 passed; one explicit physical Zhuyin skip because the runner lacks Accessibility trust |
| Rust engine/IPC | 957 passed: engine 711, context menus 5, find 9, forms 15, keyboard 18, IPC 199; zero failed/ignored |
| MCP | 191 passed; zero failed/ignored, including interleaved menu/copy broadcasts before history completion |
| Workspace compile | `cargo check --workspace --all-targets --locked --offline` passed |
| Affected Clippy | engine, IPC, MCP and reference frontend, all targets, `-D warnings`, passed |
| Formatting / project / signatures | Rustfmt, Git whitespace, Xcode project plist and strict signatures passed |
| Owned application/services | Normal UI-test teardown completed; 0 exact bundled app/service processes remained |

The complete native result bundle is
`frontend/macos/.build/results-20261002-235111.xcresult`; attachment export is
`frontend/macos/.build/results-20261002-235111-attachments`. It reports 67 total tests,
66 passed, zero failed and one skipped. 17 issues are XCTest's
internal priority-inversion diagnostics; 0 SwiftUI view-update state
warnings were observed. Actual VoiceOver and physical OS IME are separate,
unproven acceptance gates; deterministic composition callbacks remain covered.

The two added actual-window cases verify clipboard copy without a fetch,
current/new-tab link navigation without a credits redirect, source-tab retained
state, Back, both current/new-tab policy denial and no phantom native tab.
They also verify right-click Select All/Cut/Paste, Shift-F10, Control-click,
readonly Copy with disabled Cut/Paste, protected menu policy/no plaintext,
disabled controls, Escape and Find from the page menu. All prior native tests
remain present and ran in the final complete suite.

The new public Unix-session cases exercise actual nested-link text hit testing,
Copy without navigation, unavailable-review failure, editor focus and selection
preservation, password redaction, readonly/disabled state, stale frame/document/
source/cross-tab/closed-tab commands, viewport rejection and current/new-tab
built-in navigation. The protocol tests round-trip typed context messages and
copy metadata. The MCP boundary injects both menu and copy replies before a
real history completion barrier; its original assertions remain intact.

## Corrections found during verification

Test-first Rust compilation failed on the missing typed protocol before it was
implemented. Readonly controls required a selection-capable predicate separate
from writable native-input support. The initial semantic fixture's nested inline
anchor had no standalone representation box; a block anchor with nested bold
text supplies a public layout boundary, and the final test explicitly hits the
text near the left edge rather than the empty center of its anchor box.

The native wire initially discarded OpenTab's URL and always navigated newly
opened tabs to credits. New-tab denial also arrived addressed to an unpublished
new tab, so it needed correlated source-page feedback and empty-tab cleanup.
The actual-window case retains exact fixture request order/count assertions for
both successful and denied navigation.

The first UI run found an ambiguous Select All query because the main Edit
menu also contains that item. AX failure attachments showed the popup under the
page group, so queries now select that exact native subtree. A policy-denial
assertion initially inspected only AXLabel; the actual failure snapshot proved
`status` carries `Navigation blocked…` in AXValue. The corrected assertion checks
that same required text in either platform representation. Product assertions
were retained. A test compile correction uses the installed SDK's
`XCUIElement.perform(withKeyModifiers:)` API for Control-click.

## Reproduction and artifact identity

Use the existing single build cache and local signed-test runner:

```sh
export CARGO_TARGET_DIR="$PWD/frontend/macos/.build/core-target"
export CARGO_TARGET_AARCH64_APPLE_DARWIN_RUNNER="$PWD/frontend/macos/TestSupport/run-signed-test.sh"
"$HOME/.cargo/bin/rustup" run 1.96.0 cargo test -p blueice-engine -p blueice-ipc --lib \
  --test native_context_menu --test native_find --test native_forms --test native_keyboard --locked --offline
"$HOME/.cargo/bin/rustup" run 1.96.0 cargo test -p blueice-mcp-server --locked --offline
frontend/macos/test.sh
```

Local network/process fixtures and Xcode test/report services require the host's
normal local socket and GUI test permissions; sandbox denials are not product
regressions. Verified host: Apple Silicon macOS 26.6.2 (25G83), Xcode 27.0
(27A266a), Swift 6.4 and Rust 1.96.0. The Debug app build includes arm64/x86_64
Swift compilation; all runtime acceptance here ran on Apple Silicon.

Seventeen changed Rust/Swift production/test inputs were frozen before the final
complete native run and remained unchanged through verification. Sorted relative
path + NUL + raw bytes + NUL combined SHA-256:
`02962a84c49af7eca2ede0c61c2ebfaa21a38246f8ebbee37f20e915e887a193`. Documentation and generated artifacts are excluded.

The actual passing-run popup screenshot is
`artifacts/macos-context-menu.png`, SHA-256 `74d5df80b3c039f141e0883a2c6aa3cf67c458a7d2fcbbc4715b8442dd67b5cf`.
It was inspected directly without image editing. The PNG, result bundles and
build outputs are ignored, not tracked. Compact non-sensitive evidence metadata
is retained in `artifacts/macos-context-menu-results.txt`; logs and raw AX failure
attachments stay local.

## Remaining acceptance

This delivers the core's current link/editor/page context menu UI. Image/media
open/save commands, general page-text selection/copy, JavaScript contextmenu
cancellation/default events, full `<base>` anchor semantics, drag/drop and file
selection remain open. Existing layout/overflow/text-shaping limits remain.

The full delivery plan still requires the remaining editing/form events and
validity, multi-window/tab-group/profile handoff, downloads/print, trusted human
permission/assistant panels, display/theme/zoom/localization/system integration
and full accessibility/final design acceptance. No Linux/Windows native UI,
Intel/older macOS runtime, Release/notarization, full Rust workspace execution/
coverage, physical OS IME or actual VoiceOver completion is claimed by this run.
