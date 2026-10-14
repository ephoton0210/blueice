# macOS native find validation

Validated on 2026-10-02 after form submission milestone `1bf78a3d9`. This
increment delivers page find in the existing SwiftUI/AppKit browser and fixes
an ordered-clipboard regression exposed by the complete native suite. The
[delivery plan](MACOS_DELIVERY_PLAN.md) retains every remaining browser milestone.

## Measured results

| Check | Result |
| --- | --- |
| Build, complete native suite and attachment export | Exit 0 |
| XCTest: 14 accessibility, 11 editing/keyboard, 15 protocol/process | 40 passed |
| Actual-window XCUITest | 22 passed, 0 failed, 1 physical IME case skipped |
| Complete native structured result summary | 63 total, 62 passed, 0 failed, 1 skipped |
| Expanded Return/Shift-Return/repeated Command-F XCUITest after the full suite | 1 passed; same unchanged app code |
| SwiftUI view-update state warnings | 0 |
| Internal XCTest QoS runtime warnings | 15 in full suite; 1 in expanded keyboard check |
| Engine library | 711 passed |
| Public session find boundary | 9 passed |
| Public form / keyboard boundaries | 15 / 18 passed |
| IPC library, including native find wire round trip | 198 passed |
| MCP library and all integration targets | 191 passed |
| Unique Rust cases in final successful runs | 1142 passed, 0 failed, 0 ignored |
| Workspace check, all targets, locked/offline | Passed |
| Clippy: 4 affected packages, all targets, warnings denied | Passed |
| Rust formatting, Git whitespace, Xcode project parsing | Passed |
| App and three bundled services: strict signature verification | Passed |
| Screenshot/result-bundle Git ignore checks | Passed |
| Owned native app/service processes after the complete suite | 0 |

## Delivered behavior and evidence

The core searches its current layout runs and retains grapheme geometry in
document space. Literal queries normalize whitespace and canonical Unicode;
case-insensitive matching uses Unicode simple case folding, with an explicit
Match case option. Public tests cover canonical accents, Greek sigma variants,
regex punctuation treated literally, soft wrapping and inline font changes.
Block boundaries cannot join separate paragraphs into a phrase. Native public
input/textarea values and painted button/select captions participate; hidden,
non-content, opacity-zero and password/payment/credential input subtrees are
excluded. Native control geometry respects its content clipping.

Search requests check the addressed tab, frame directory source and document
generation before mutation. Replies contain the user-provided query, count,
active ordinal, lifecycle metadata and current match rectangles, never a page
text snapshot or protected value. The literal query is bounded to 1,024 UTF-8
bytes, the index to two MiB / 200,000 grapheme segments, matches to 10,000 and
active geometry to 1,024 rectangles. Partial indexes/counts are disclosed in
state and UI. Overlong requests leave existing search state intact.

The core paints all matches, outlines the current match in orange and scrolls
it into view. Next/Previous wrap and retain their direction-independent wrapped
notice. Live edits and resize rebuild counts/geometry. Each tab retains its own
query and position; navigation clears search, including explicit retained
history snapshots. Closed tabs and mismatched contexts are rejected. An MCP
history test injects a find broadcast before Navigated and verifies it cannot
satisfy the history completion barrier; reference/MCP clients accept the new
additive server variant without changing ownership or granting permissions.

The native search row embeds NSSearchField in SwiftUI, with Edit > Find,
Command-F, Command-G/Shift-Command-G, Return/Shift-Return, Escape and labelled
Previous/Next/Close controls. Command-F focuses and selects the query. A real
window test enters text directly after Command-F, wraps to the last match,
checks its viewport bounds and orange raster pixels, then exercises case,
buttons and dismissal. The final expanded keyboard check also verifies Return,
Shift-Return and replacing the selected query after a repeated Command-F.
Another checks Unicode, literal punctuation, exclusion,
independent tabs and navigation invalidation. The bundled real-service test
covers live native edits and confirms searching never fetches another page.

## Regression corrections

The initial public find test received the unimplemented error; the same test
now passes through the actual session protocol and shared frame file. A history
snapshot regression first restored an old query with two matches; committed
navigation now clears the core search before publishing the restored frame.
A resize fixture accounts for the session's frames for all live tabs while
preserving per-request identity assertions for terminal replies.

Actual Command-F UI automation exposed SwiftUI focus being applied before the
new field was installed. NSSearchField now receives native focus outside the
view-update transaction. UI assertions still require typing immediately after
the shortcut; no extra click was substituted for that requirement.

The first complete suite exposed Select All followed by Copy being dropped
while the core selection acknowledgement was pending. A deterministic real-core
regression reproduced the dropped command. Copy/Cut/Paste now share the bounded
input queue, with tab/document/focus fences; Paste reads the clipboard at its
turn and subsequent edits retain ordering. Password Copy/Cut and readonly Cut
remain inert; readonly Copy works; a queued Copy cannot change the clipboard
after a tab switch. Tests preserve and restore existing pasteboard types.
The existing keyboard clipboard test waits for the confirmed OS clipboard
result, retaining its original exact-content assertion. The final complete
suite includes this regression and all original editing/form/window tests.

The restricted shell initially rejected local Unix/TCP fixture sockets with
Operation not permitted. The unchanged Rust assertions were rerun with local
socket access and passed. No assertion, original timeout or policy rule was
weakened to accept that environment failure.

## Scope and remaining acceptance

This searches text the current core paints. Regex search, locale-tailored/full
multi-character case folding, accent-insensitive matching, complete shaping/bidi,
iframe content and unsupported overflow scrolling are not delivered here.
The core's existing whitespace/layout/font limits also bound what can be found.
Context menus, drag/drop, file selection, complete DOM input/form events and
validity, multiple windows/groups, downloads/printing, trusted permission panels,
system integration and final screen-reader acceptance remain in the plan.

Physical system Zhuyin still explicitly skips because the UI Runner lacks
Accessibility permission. Deterministic AppKit composition passes; no TCC/privacy
setting was changed. Actual VoiceOver, Linux/Windows UI, Intel/older macOS,
Release signing/notarization, full Rust workspace execution and coverage were
not rerun. The manual gatekeeper model quality probe was outside this affected
suite. Native tests contain internal XCTest QoS warnings recorded above.

## Reproduction and final evidence

```sh
frontend/macos/test.sh
export CARGO_TARGET_DIR="$PWD/frontend/macos/.build/core-target"
export CARGO_TARGET_AARCH64_APPLE_DARWIN_RUNNER="$PWD/frontend/macos/TestSupport/run-signed-test.sh"
"$HOME/.cargo/bin/rustup" run 1.96.0 cargo test -p blueice-engine -p blueice-ipc --lib --test native_find --test native_forms --test native_keyboard --locked --offline
codesign --force --sign - "$CARGO_TARGET_DIR/debug/blueice-core"
"$HOME/.cargo/bin/rustup" run 1.96.0 cargo test -p blueice-mcp-server --tests --locked --offline
"$HOME/.cargo/bin/rustup" run 1.96.0 cargo check --workspace --all-targets --locked --offline
"$HOME/.cargo/bin/rustup" run 1.96.0 cargo clippy -p blueice-engine -p blueice-ipc -p blueice-mcp-server -p blueice-frontend-reference --all-targets --locked --offline -- -D warnings
"$HOME/.cargo/bin/rustup" run 1.96.0 cargo fmt --all -- --check
```

MCP integration expects the existing sibling blueice-bluejs-host and downloads
binaries in the same cache. When absent, build them with
`cargo build -p blueice-launcher -p blueice-downloads --bins --locked --offline`
and sign the local test binaries as described by the preceding form result.
Socket tests require local socket access. Native tests use the existing unlocked
session and Xcode automation authorization on Apple Silicon macOS 26.6.2
(25G83), Xcode 27.0 (27A266a), Swift 6.4 and Rust 1.96.0.

Complete native bundle: `frontend/macos/.build/results-20261002-230030.xcresult`.
Expanded keyboard bundle: `frontend/macos/.build/results-20261002-231353.xcresult`.
After the complete suite, only the search keyboard test added Return/Shift-Return
and repeated-Command-F assertions; its expanded case passed without any app-code
change. The 20 other scoped runtime/build/test inputs remained unchanged,
combined SHA-256 `61ed698b41b0a5133c24e5ce9dc39b6466b77a32b144533968da792ececaaea7`.
The full-suite 21-path source digest was `916070dffa3ec3ab286dedd05e078f8f8708f08cc3c7cb3eee7339e8ca64f0ab`;
the final digest including expanded UI assertions is `f4a4d7d5735997d8ea907a7c2d5fc13e39b993badaa11057e45ed7bf514485cb`.
The supplementary MCP test file SHA-256 is
`0927fec129ad50d7f61b0901e41cbe5b4ec5079c2b240e50b2f24271b513ac71`, covered by its final full Rust suite.
Hashing uses sorted relative paths, NUL, raw bytes, NUL; docs/generated output
are excluded. Base: `1bf78a3d9b56d1a36b4bfe713d83d5b283833dba`.

The inspected actual-window screenshot is ignored at
`development/browser_core/phase-22-browser-shell-accessibility/artifacts/macos-find-in-page.png` (480,789 bytes), SHA-256
`342548d8c759568e7774ad717d1b8e95562c3cdfca5b66be7d6d5a193dbdb11d`. It shows the native search row,
last match/wrap count and the core-painted orange outline after scrolling.
Binary screenshots and result bundles remain untracked; compact
[validation metadata](artifacts/macos-find-results.txt) is tracked.
