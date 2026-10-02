# macOS native form reset validation

Validated on 2026-10-02 with the SwiftUI/AppKit frontend, bundled launcher,
real core and compiled gatekeeper. This extends the keyboard milestone
`97794bb57` in the [macOS delivery plan](MACOS_DELIVERY_PLAN.md).

## Measured results

| Check | Result |
| --- | --- |
| Build, complete native suite and attachment export | Exit 0 |
| Swift XCTest: 14 accessibility, 8 editing/keyboard, 13 protocol/process | 35 passed |
| Actual-window XCUITest | 18 passed, 0 failed, 1 physical IME case skipped |
| Native result summary | 54 total, 53 passed, 0 failed, 1 skipped |
| SwiftUI view-update state warnings | 0 |
| Internal XCTest QoS runtime warnings | 11 |
| Public keyboard/reset Unix-socket regressions | 18 passed |
| Affected CSS/engine/IPC/layout/paint/raster libraries | 1,077 passed |
| Additional shared rendering, clipping, history/stdio boundaries | 10 passed |
| MCP library and integration targets | 190 passed |
| Unique Rust cases, excluding repeated libraries | 1,295 passed, 0 failed, 0 ignored |
| Workspace check, all targets, locked/offline | Passed |
| Clippy: 8 affected packages, all targets, warnings denied | Passed |
| Rust format, Git whitespace and Xcode project parsing | Passed |
| App and three bundled services: strict signature verification | Passed |
| Validation image and result-bundle Git ignore checks | Passed |
| Owned native bundle app/service processes after complete suite | 0 |

## Delivered behavior and evidence

Previously reset activation left edited values unchanged. The core now keeps
original input/textarea/option defaults separate from their live native values.
Pointer, Enter/Space and ordinary node activation reset the current form owner's
text/password/textarea, checkbox/radio, select and range state. Externally
associated controls are included; explicit missing/different owners, unrelated
forms and other tabs retain their values. Disabled/readonly associated fields
reset too, while disabled or unowned reset buttons have no reset default.

Reset retains control focus and document identity, clears selection/composition
and advances the native focus generation. A pre-reset context is rejected;
CancelComposition cannot restore an old edited value after reset. Shared core
paint and accessibility values update together, without a network request.
Password originals and edited contents remain absent from semantic values and
native input-state observation. Reload/document replacement establishes new
defaults, independently of another tab's retained values.

Input reset/submit/button captions share one core label source for paint and
semantics, including mixed-case types, explicit values and the default reset
caption. Explicit page-script textarea textContent/appendChild changes update
its retained default; native and extension live-value setters do not. The
existing pre-default click cancellation and document-replacement boundary still
suppresses reset. Those cancellation cases use a configured executor test
double and do not claim full DOM reset-event support.

Real-service AppKit XCTest exercises marked text, native context invalidation,
readonly error recovery and an actual compiled gatekeeper content denial. The
shell clears only a correlated native editing notice after accepted input or
focus change; a mandatory navigation-denial notice stays visible. Actual-window
XCUITest covers keyboard reset across all supported controls, the externally
associated input reset button, pointer reset, disabled controls, unrelated forms,
tab isolation and defaults after same-URL reload.

Test-first regressions recorded unchanged live values and unchanged focus
identity before implementation, missing painted reset captions, outdated script
textarea defaults, and the old readonly status notice after successful reset.
The final measured run includes the corresponding successful assertions.

## Remaining acceptance

This is the native reset default, not full form/browser acceptance. GET/POST
submission, submitter overrides and encodings, constraint validation, cancelable
DOM reset-event dispatch, complete dirty value/default DOM-property behavior,
file-input state and native file selection remain pending. The existing
multiple-select interaction, input/focus/key/composition event stream, undo/redo,
bidi shaping and broader browser delivery milestones also remain open.

The physical system-Zhuyin case explicitly skips because the UI Runner lacks
Accessibility permission. Deterministic AppKit composition passed; no TCC,
privacy setting or system authorization dialog was modified. Actual VoiceOver,
Linux/Windows runtime UI, Intel/older macOS, Release distribution signing,
notarization, full Rust workspace execution and coverage were not rerun here.

## Reproduction and final evidence

```sh
frontend/macos/test.sh
export CARGO_TARGET_DIR="$PWD/frontend/macos/.build/core-target"
export CARGO_TARGET_AARCH64_APPLE_DARWIN_RUNNER="$PWD/frontend/macos/TestSupport/run-signed-test.sh"
"$HOME/.cargo/bin/rustup" run 1.96.0 cargo test -p blueice-engine --test native_keyboard --locked --offline
"$HOME/.cargo/bin/rustup" run 1.96.0 cargo test -p blueice-css -p blueice-engine -p blueice-ipc -p blueice-layout -p blueice-paint -p blueice-raster --lib --locked --offline
"$HOME/.cargo/bin/rustup" run 1.96.0 cargo test -p blueice-layout -p blueice-paint -p blueice-raster --tests --locked --offline
"$HOME/.cargo/bin/rustup" run 1.96.0 cargo test -p blueice-engine --test fixtures --test history_generations --test stdio_session --locked --offline
# MCP integrations use the sibling signed BlueJS host and downloads service.
"$HOME/.cargo/bin/rustup" run 1.96.0 cargo build -p blueice-launcher -p blueice-downloads --bins --locked --offline
codesign --force --sign - "$CARGO_TARGET_DIR/debug/blueice-bluejs-host"
codesign --force --sign - "$CARGO_TARGET_DIR/debug/blueice-downloads"
"$HOME/.cargo/bin/rustup" run 1.96.0 cargo test -p blueice-mcp-server --tests --locked --offline
"$HOME/.cargo/bin/rustup" run 1.96.0 cargo check --workspace --all-targets --locked --offline
"$HOME/.cargo/bin/rustup" run 1.96.0 cargo clippy -p blueice-engine -p blueice-ipc -p blueice-css -p blueice-layout -p blueice-paint -p blueice-raster -p blueice-frontend-reference -p blueice-mcp-server --all-targets --locked --offline -- -D warnings
"$HOME/.cargo/bin/rustup" run 1.96.0 cargo fmt --all -- --check
```

Host: Apple Silicon macOS 26.6.2 (25G83), Xcode 27.0 (27A266a), Swift 6.4,
Rust 1.96.0. Native tests require an unlocked graphical session and existing
Xcode automation authorization. Socket tests require local socket access.

Final result: `frontend/macos/.build/results-20261002-202807.xcresult`, with adjacent exported attachments.
The 15 affected Rust/Swift source/test paths have combined SHA-256
`845cf13a3707d9a2a20aa289409ab7b14e68d3955667fe08840d7d9fdcf872b7`. Hashing uses sorted repository-relative paths, a NUL,
raw bytes and a NUL. The final complete native run retained that fingerprint;
documentation and generated output are excluded. Rust source stayed unchanged
while the final Swift error-status fix was validated. Base:
`97794bb5727894e456f73d35d18963a44a2f2e7b`.

The inspected, unobstructed native window shows restored text/notes, checked
checkbox/standard radio, Alpha selection and range position, the focused
external reset button, an unchanged other-form value and Ready status.
The unchanged validation attachment is
[artifacts/macos-native-form-reset.png](artifacts/macos-native-form-reset.png),
504,381 bytes, SHA-256
`0fea37622f63ff8b796262df174a5e61c296aab2e24eddedf1b8891899767003`. Images, build output and result bundles remain
ignored. The tracked compact record is
[artifacts/macos-form-reset-results.txt](artifacts/macos-form-reset-results.txt).

| Signed bundle file | SHA-256 |
| --- | --- |
| `BlueIce` | `dd3efceae265ff44be45aba04ea92d0d4d4cd4ecc08de2343142892f977f6d80` |
| `BlueIce.debug.dylib` | `fade453241d9cc85e54421268f86091fdd21dc1149492e1932a972275551dd9c` |
| `blueice-core` | `8ebfdb003eea203c1f99f32dcd2472fe108cc9826c4c640226fe654619b181f6` |
| `blueice-launcher` | `af7b04ff6753ee88e7cb9500231daaaba7ab4459c5953b832edf4dab0f4649df` |
| `blueice-ai-gatekeeper` | `022471088f7fa271ff6e389fef97e4e42986caf5ea5852be1e571caf38ba3d93` |
