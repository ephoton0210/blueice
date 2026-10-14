# macOS keyboard control and focus validation

Validated on 2026-10-02 using the SwiftUI/AppKit macOS frontend and its bundled
launcher, real core and compiled gatekeeper. This extends the committed native
editing foundation (`8b80c3c53`); it is an increment in the
[macOS delivery plan](MACOS_DELIVERY_PLAN.md), not final browser acceptance.

## Measured results

| Check | Result |
| --- | --- |
| Build, complete native suite and attachment export | Exit 0 |
| Swift XCTest: 14 accessibility, 7 editing/keyboard, 13 protocol/process | 34 passed |
| Actual-window XCUITest | 16 passed, 0 failed, 1 physical IME case skipped |
| Native result summary | 51 total, 50 passed, 0 failed, 1 skipped |
| SwiftUI view-update state warnings | 0 |
| Internal XCTest QoS runtime warnings | 9 |
| Public keyboard Unix-socket regressions | 11 passed |
| Affected Rust CSS/engine/IPC/layout/paint/raster libraries | 1,075 passed |
| Shared rendering, raster clipping, history/stdio boundaries | 10 passed |
| MCP library and integration targets | 190 passed |
| Unique Rust cases, excluding repeated libraries | 1,286 passed, 0 failed, 0 ignored |
| Workspace check, all targets, locked/offline | Passed |
| Clippy: 8 affected packages, all targets, warnings denied | Passed |
| Rust format, Git whitespace and Xcode project parsing | Passed |
| App and three bundled services: strict signature verification | Passed |
| Git ignore checks for validation image and result bundle | Passed |
| Owned native app/service processes after full suite | 0 |

## Delivered behavior

The core derives sequential Tab/Shift-Tab order from the live document:
positive tabindex precedes ordinary document order, negative entries stay
available for explicit focus, and disabled/hidden/inert controls are skipped.
Tab from a negative programmatic focus resumes at its document position.
Disabled fieldsets retain their first-legend exception and do not disable
ordinary links. Focused controls scroll into view and have a core-painted ring.

Space toggles checkboxes; radio arrows move within enabled members sharing a
name and form owner, including externally associated controls. Single-select
arrows/Home/End skip disabled options/optgroups. Decimal range keys honor the
implemented min/max/positive-step model, including a non-aligned maximum.
Disabled selects retain their displayed value. Multiple-select direction keys
preserve existing selections while their interaction model remains pending.

Shared layout/paint produces checkbox/radio/select/range pixels for both static
rendering and interactive pages. Semantic values, native focus and radio state
come from the same core document. Native checkbox/radio press uses the ordinary
click pipeline; direct AXValue writes remain unavailable.

The additive version-one input fields identify general focused nodes and page
focus exit; the browser envelope remains version two. Closed default-key intents
use the latest core focus rather than a frontend guess from cached text state.
Stale source/document/focus contexts are rejected before click listener dispatch.
Keyboard activation honors the existing click cancellation boundary, and a
listener replacing the document suppresses the previous document's default.
This boundary test uses a configured executor; it does not establish full DOM
keyboard/beforeinput/input/focus/composition event support.

The owned AppKit window retains at most 512 native key events across an outstanding Tab, pinned
to the current tab/document. Core acknowledgement determines whether they replay
to the new page control or the native SwiftUI address editor. Document/tab
changes and rejected/unavailable transitions discard the pending events.
Command-L focuses the native address editor. Native focus transfer and replay
run after SwiftUI view updates, avoiding publication during a view transaction.
The owned native field's begin-editing notification wakes pending address input
once its actual field editor becomes the responder.

## Regression evidence

The initial three public Unix-socket tests reproduced incorrect checkbox and
select values and skipped first-legend focus. The final public-boundary suite
covers order/page exits, pointer label parity, visible state changes, radio form
ownership, decimal range endpoints, textarea Space/Enter defaults, negative
focus starting position, disabled-select value, retained multiple selections,
stale commands before listener side effects, cancelled/document-replaced
activation and equal shared static/interactive native paint.

A real-service XCTest issues consecutive Tab/Space/arrow callbacks without
waiting between focus changes. Two actual-window XCUITest cases fill and inspect
text/readonly fields, checkbox/radio/single-select/range state, activate a
reviewed link with Enter, reverse focus, type immediately upon returning to the
address editor across three consecutive document loads, and navigate through
Command-L. The owned window buffers later keys during handoff so they cannot
overtake an earlier page key awaiting replay. They keep the original immediate
input assertions; no sleeps or synthetic address shortcut substitute for Tab
handoff. Earlier native runs reproduced address-focus reclamation and lost
post-Tab input. Xcode's result summary then exposed view-update publication
warnings despite passing assertions; replay was deferred outside that update.

## Limits and remaining acceptance

Form submission/reset and constraint validation, select popup/typeahead/multiple
interaction, the full physical/logical DOM keyboard event stream, general
asynchronous pointer-focus editing, complete chrome Tab traversal, find/context
menus, drag/drop and file-selection still require delivery. The current range
increment is not a complete HTML numeric-sanitization implementation.
Undo/redo, complete bidi shaping, caret blink and preferred vertical caret
position remain pending. This does not validate full ARIA/AX text/VoiceOver,
profiles/windows, downloads/print/permission panels or final macOS display and
localization behavior.

The physical system-Zhuyin case remains separate from deterministic AppKit
composition. It explicitly skips when the UI Runner lacks Accessibility
permission; no TCC database, privacy setting or system authorization dialog was
modified. Linux/Windows runtime UI, Intel/older macOS, Release distribution
signing/notarization and full Rust workspace execution/coverage were not rerun
for this increment.

## Reproduction and final evidence

```sh
frontend/macos/test.sh
export CARGO_TARGET_DIR="$PWD/frontend/macos/.build/core-target"
export CARGO_TARGET_AARCH64_APPLE_DARWIN_RUNNER="$PWD/frontend/macos/TestSupport/run-signed-test.sh"
"$HOME/.cargo/bin/rustup" run 1.96.0 cargo test -p blueice-engine --test native_keyboard --locked --offline
"$HOME/.cargo/bin/rustup" run 1.96.0 cargo test -p blueice-css -p blueice-engine -p blueice-ipc -p blueice-layout -p blueice-paint -p blueice-raster --lib --locked --offline
"$HOME/.cargo/bin/rustup" run 1.96.0 cargo test -p blueice-layout -p blueice-paint -p blueice-raster --tests --locked --offline
"$HOME/.cargo/bin/rustup" run 1.96.0 cargo test -p blueice-engine --test fixtures --test history_generations --test stdio_session --locked --offline
# MCP integrations need the sibling signed BlueJS host and downloads service.
"$HOME/.cargo/bin/rustup" run 1.96.0 cargo build -p blueice-bluejs --bin blueice-bluejs-host -p blueice-downloads --locked --offline
codesign --force --sign - "$CARGO_TARGET_DIR/debug/blueice-bluejs-host"
codesign --force --sign - "$CARGO_TARGET_DIR/debug/blueice-downloads"
"$HOME/.cargo/bin/rustup" run 1.96.0 cargo test -p blueice-mcp-server --tests --locked --offline
"$HOME/.cargo/bin/rustup" run 1.96.0 cargo check --workspace --all-targets --locked --offline
"$HOME/.cargo/bin/rustup" run 1.96.0 cargo clippy -p blueice-engine -p blueice-ipc -p blueice-css -p blueice-layout -p blueice-paint -p blueice-raster -p blueice-frontend-reference -p blueice-mcp-server --all-targets --locked --offline -- -D warnings
"$HOME/.cargo/bin/rustup" run 1.96.0 cargo fmt --all -- --check
```

Host: Apple Silicon macOS 26.6.2 (25G83), Xcode 27.0 (27A266a),
Swift 6.4 and Rust 1.96.0. UI tests require an unlocked graphical session
and existing Xcode automation authorization; socket tests require local socket
access. Newly rebuilt Apple Silicon test binaries can pause while dyld warms;
the original live test handles were retained until they completed.

Final result: `frontend/macos/.build/results-20261002-193203.xcresult`, with its adjacent attachments.
The 28 affected Rust/Swift source and test paths have combined SHA-256
`624100f2cbc37490358145eb61b5288d22895c87354a68027833fa781a6f618a`. Hashing uses sorted repository-relative paths, a NUL,
raw bytes and a NUL. The source fingerprint remained unchanged throughout the
final full run. Documentation/output are excluded. Base:
`8b80c3c533a6ef1f0c116b21ef5b3e7e182c5a66`.

The unchanged validation attachment is
[artifacts/macos-keyboard-controls.png](artifacts/macos-keyboard-controls.png),
502,693 bytes, SHA-256
`6691b961cd9281a370d276f872de0253191f721d2af6f3c98737132297c4d86a`. Screenshots, build output and result bundles
remain ignored. The tracked compact record is
[artifacts/macos-keyboard-results.txt](artifacts/macos-keyboard-results.txt).

The inspected attachment shows an unobstructed native window with the core's
`Alice` value, readonly/disabled/inert fields, checked checkbox, selected radio,
`Beta` select label and focused range track. This is visual evidence for the
recorded controls; complete browser design and DPI/display acceptance remain
in the delivery plan.

| Signed bundle file | SHA-256 |
| --- | --- |
| `BlueIce` | `9942f070c52cebdc8143d608c5537484f89fe6912210ff541b342d528fce4aa5` |
| `BlueIce.debug.dylib` | `6e45b20ce609fe83f2411ebf264bfc590d0342cdc4402635c35ea6446eb82a3d` |
| `blueice-core` | `0b188ac7cee4f93a64739d4bcff1d7933e41ea1d3941e8a11ab91920b2e18210` |
| `blueice-launcher` | `af7b04ff6753ee88e7cb9500231daaaba7ab4459c5953b832edf4dab0f4649df` |
| `blueice-ai-gatekeeper` | `022471088f7fa271ff6e389fef97e4e42986caf5ea5852be1e571caf38ba3d93` |
