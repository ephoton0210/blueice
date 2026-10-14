# macOS native text editing foundation validation

Validated on 2026-10-02, Apple Silicon macOS 26.6.2 (25G83), Xcode 27.0
(27A266a), Swift 6.4 and Rust 1.96.0. This extends the
[page accessibility bridge](MACOS_ACCESSIBILITY_RESULTS.md) with core-owned
native editing. The actual app uses its bundled launcher, core and compiled
gatekeeper; a loopback HTTP fixture supplies the reviewed test document.
This is a foundation milestone in the [macOS delivery plan](MACOS_DELIVERY_PLAN.md).

## Measured results

| Check | Result |
| --- | --- |
| `frontend/macos/test.sh`: build, native tests and attachment export | Exit 0 |
| Swift XCTest: 14 accessibility, 6 native editing, 13 protocol/process cases | 33 passed, 0 failed |
| Actual native-window XCUITest | 14 passed, 0 failed, 1 skipped |
| Physical system Zhuyin composition test | Pending; Runner `AXIsProcessTrusted()` is false |
| Affected Rust libraries: CSS / engine / IPC / layout / paint / raster | 106 / 709 / 197 / 35 / 19 / 9 passed; 1,075 total |
| Layout/paint shared fixtures, raster clipping, engine render/history/stdio boundaries | 10 passed |
| MCP server library and all integration targets | 190 passed |
| Unique Rust cases across those commands | 1,275 passed, 0 failed, 0 ignored |
| `cargo check --workspace --all-targets --locked --offline` | Passed |
| Clippy for 8 affected packages, all targets, `-D warnings` | Passed |
| Rust formatting, Git whitespace, Xcode project parsing | Passed |
| App and bundled-service strict signature verification | Passed |
| Validation screenshot and result-bundle Git ignore rules | Passed; no tracked validation PNGs |

The result bundle's summary independently reports 48 total native cases,
47 passed, zero failed and one skipped. It reports seven internal QoS
priority-inversion runtime warnings. The full native suite retains existing
window, navigation, policy denial, semantics, privacy, tab and process-lifecycle
checks.

## Delivered behavior and boundaries

The additive version-one `blueice_ipc::input` state uses UTF-16 ranges and
frame-source/document/focus identities. The core owns selection, temporary
composition and committed control values. It validates scalar boundaries,
bounds control contents to 65,536 UTF-16 units and bounds reported caret
positions to 1,024. Commands from an older document or focus are rejected.
Legacy committed-text commands remain for existing platform clients.

Text/password inputs and textareas support replacement, repeated marked-text
updates, commit/unmark/cancel, selection, word/line/grapheme movement and
grapheme deletion. Cancellation restores the original value and selection.
Textarea whitespace is preserved in both rendering and semantic values.
Password layout contains masks; input-state replies and semantic/accessibility
values omit plaintext. Readonly fields permit selection and reject writes;
disabled fields cannot become native editors. Extension DOM-write permissions
retain their independent constraints.

Layout produces source-indexed native text runs and resolved control content
boxes. Candidate/caret/selection geometry derives from those runs. Paint and
raster commands clip nested content and editor overlays; control scrolling
keeps the caret within its content box. The shared paint fixture now includes
the intentional content clip and empty text run for an empty native input.
The layout fixture and engine end-to-end render check also pass.

The AppKit viewport implements `NSTextInputClient`, native key bindings,
candidate screen coordinates and explicit clipboard commands. A bounded queue
serializes edits through core acknowledgements, including replies that arrive
before the writer returns their request ID. Document/focus changes invalidate
queued edits and cached state. The frontend keeps pending composition range
metadata for synchronous AppKit callbacks; it does not own a second DOM or
layout. Copy/cut do not export password contents, and paste is a bounded user
operation. The native SwiftUI Edit → Input Source menu changes the current
responder's input context, including native address editing.

## Tests and the actual input-method gate

The six new XCTest cases verify typed wire fields; malformed/out-of-range and
surrogate-splitting replies; protected-plaintext rejection; real-service CJK
and RTL composition/update/replacement/cancellation; synchronous marked-range
and unmark behavior; candidate geometry while a longer composition is pending;
multiline screen geometry/substrings; emoji grapheme deletion; password
redaction; readonly rejection; document fences; and native input-source menu
state. The pending-candidate regression first reproduced a zero-height
candidate rectangle, then passed after falling back to the core's current
caret while awaiting the new layout.

Two new actual-window tests exercise selection/replacement/deletion through
native keyboard commands, copy/cut/paste, multiline CJK/RTL clipboard text,
ZWJ emoji deletion, secure fields, readonly rejection and disabled state.
The tests restore clipboard contents and the user's keyboard input source.
These passed against the real owned services and OS-visible page elements.

The third new XCUITest selects the actual native Zhuyin input context and sends
physical key codes for `ㄋ → ㄋㄧ → 你`, commit and cancellation to the unique
active BlueIce test process. XCUITest string keys are insufficient here: its
`3` key was observed as numeric-keypad key code 85, while the required
third-tone key uses main-keyboard code 20. The physical-event test checks
`AXIsProcessTrusted()` and explicitly skips when the Runner lacks Accessibility
permission. This host returned false; the final full run therefore did not
exercise actual system IME composition. Direct AppKit callbacks passed, but
they do not establish physical OS IME acceptance.

Enable the exact Runner app under System Settings → Privacy & Security →
Accessibility before rerunning this test:
`frontend/macos/.build/Build/Products/Debug/BrowserUITests-Runner.app`.
An enabled Traditional Zhuyin source is also required. No privacy settings,
TCC database, permissions or system dialogs were modified by the scripts.

## Limits and next delivery

JavaScript keyboard/beforeinput/input/composition event dispatch, undo/redo,
complete bidirectional shaping, caret blink, preferred vertical movement
position and keyboard handling across asynchronous focus are still required.
This logical text editing path does not establish full RTL visual editing or
all `NSTextInputClient` input methods. Accessibility text ranges, writable
AXValue, VoiceOver speech/navigation and the remaining browser milestones
also remain open. Linux/Windows runtime UI, older macOS/Intel, Release
distribution signing/notarization and the full Rust workspace execution and
coverage gate were not rerun for this foundation milestone.

The native editing screenshot was inspected: fixture fields, masked password,
multiline text and the caret are visible on the left. A macOS Developer Tools
authorization dialog obscures the center of the native window; this attachment
is not an unobstructed-window visual acceptance result. The OS dialog remains
under the user's control. No owned native app/services remained after tests.
One runtime root with only an empty `blueice` directory was observed and those
empty directories were removed with `rmdir`; no live service directory was
deleted.

## Reproduction and evidence

```sh
frontend/macos/test.sh
frontend/macos/test.sh \
    -only-testing:BrowserUITests/BrowserUITests/testSystemZhuyinInputMethodCommitsAndCancelsComposition

export CARGO_TARGET_DIR="$PWD/frontend/macos/.build/core-target"
export CARGO_TARGET_AARCH64_APPLE_DARWIN_RUNNER="$PWD/frontend/macos/TestSupport/run-signed-test.sh"
"$HOME/.cargo/bin/rustup" run 1.96.0 cargo test \
    -p blueice-css -p blueice-engine -p blueice-ipc \
    -p blueice-layout -p blueice-paint -p blueice-raster --lib --locked --offline
"$HOME/.cargo/bin/rustup" run 1.96.0 cargo test \
    -p blueice-layout -p blueice-paint -p blueice-raster --tests --locked --offline
"$HOME/.cargo/bin/rustup" run 1.96.0 cargo test -p blueice-engine \
    --test fixtures --test history_generations --test stdio_session --locked --offline
# MCP integration tests also require the sibling downloads service binary.
"$HOME/.cargo/bin/rustup" run 1.96.0 cargo build \
    -p blueice-bluejs --bin blueice-bluejs-host -p blueice-downloads --locked --offline
codesign --force --sign - "$CARGO_TARGET_DIR/debug/blueice-bluejs-host"
codesign --force --sign - "$CARGO_TARGET_DIR/debug/blueice-downloads"
"$HOME/.cargo/bin/rustup" run 1.96.0 cargo test -p blueice-mcp-server --tests --locked --offline
"$HOME/.cargo/bin/rustup" run 1.96.0 cargo check --workspace --all-targets --locked --offline
"$HOME/.cargo/bin/rustup" run 1.96.0 cargo clippy \
    -p blueice-engine -p blueice-ipc -p blueice-css -p blueice-layout \
    -p blueice-paint -p blueice-raster -p blueice-frontend-reference \
    -p blueice-mcp-server --all-targets --locked --offline -- -D warnings
"$HOME/.cargo/bin/rustup" run 1.96.0 cargo fmt --all -- --check
```

Native tests require an unlocked graphical session and existing Xcode UI
automation authorization. Socket/HTTP tests require local socket access;
Apple Silicon test executables and child binaries require local signatures.
The first constrained IPC attempt failed to create sockets; the full rerun with
local socket access passed. A first MCP run lacked the sibling downloads
binary; the complete rerun passed after building and signing that binary.

The final native result is
`frontend/macos/.build/results-20261002-174053.xcresult`, with its adjacent
attachments directory. The unchanged inspected screenshot is
[`artifacts/macos-native-editing.png`](artifacts/macos-native-editing.png),
671,230 bytes, SHA-256
`e21c6e2626649d66df9aadb8c585e95b9c26f38d050d856a809fae90aeb5f4c7`.
Screenshots, native build output and result bundles remain ignored by Git.
The compact source/build/result record is
[`artifacts/macos-native-editing-results.txt`](artifacts/macos-native-editing-results.txt).

The 52 affected source/configuration/fixture and native frontend/test files
have combined SHA-256
`383ae922bf02e1ee83f2ec45e8a7192898c0c171bcc2e02b1567309f56465762`.
The record lists every path. Hashing uses sorted repository-relative paths,
a NUL, raw bytes and another NUL; documentation and output are excluded.
The native app code was unchanged after the full native run; the shared paint
expectation was then updated and its affected public-boundary tests rerun.
The worktree was based on `41952a80361468bba5cabaf6b483487b9ee692ec`.

| Signed bundle executable | SHA-256 |
| --- | --- |
| `BlueIce` | `e86f95960d1c7f5f7f8f7ce4e62916d0c75ceb2824dd0ad037f95cf983787290` |
| `blueice-core` | `7741820d9b43af807d164e782d40c83742485592bf3bd1e474a66337200befe7` |
| `blueice-launcher` | `9554b90e4f75aaf14d3ecee611d4f574d5657e5fad5dc148a1c34feafe4cd8d7` |
| `blueice-ai-gatekeeper` | `98464012e341f6c3520ab8f7628bbebbaeec509112ead444f8d3501cfcc935a4` |
