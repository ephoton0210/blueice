# BlueIce macOS frontend

The macOS frontend uses SwiftUI for browser chrome and AppKit for the
window lifecycle, core pixel viewport, pointer/scroll input, native text
editing and the page NSAccessibility adapter. By default it launches the bundled `blueice-launcher`, which
supervises the real core and gatekeeper. A private Unix socket carries the
browser protocol; the Rust core owns pages, history, tab identities and rendering.
HTTP(S) navigation follows the existing URL and content review policy.

## Build and run

Requires macOS 14 or newer, a full Xcode installation with its command-line
tools selected, and the repository's Rust 1.96 toolchain. No third-party
Swift packages or project generator are required. Builds target the host's
architecture; Intel builds have not been validated on this Apple Silicon host.

```sh
frontend/macos/build.sh
open frontend/macos/.build/Build/Products/Debug/BlueIce.app
```

`build.sh Release` builds optimized services and frontend. The Xcode project
copies and locally signs `blueice-core`, `blueice-launcher` and
`blueice-ai-gatekeeper` inside the app bundle. It
reuses `.build/core-target` for Rust output, keeping native frontend builds
independent of the workspace's larger cache; an explicit `CARGO_TARGET_DIR`
overrides that directory. Builds use an ad hoc identity for local development;
distribution signing and notarization are separate work. To build from Xcode, first build the services
with the script, then open `BlueIce.xcodeproj` and select the shared `BlueIce`
scheme. `BLUEICE_BACKEND_DIR` overrides the directory of all three binaries
when invoking Xcode directly. When running the app executable directly,
`--launcher-exe /absolute/path/to/blueice-launcher` overrides the launcher;
the core and gatekeeper must reside beside it. The explicit diagnostic option
`--core-exe /absolute/path/to/blueice-core` retains the private-pipe mode,
which denies external navigation when review is unavailable.

## Automated validation

```sh
frontend/macos/test.sh
# Run only the protocol/real-process tests:
frontend/macos/test.sh -only-testing:ProtocolTests
# Run only the actual native window tests:
frontend/macos/test.sh -only-testing:BrowserUITests
# On this Apple Silicon host, validate the launcher against the native bundle:
CARGO_TARGET_DIR="$PWD/frontend/macos/.build/core-target" \
    CARGO_TARGET_AARCH64_APPLE_DARWIN_RUNNER="$PWD/frontend/macos/TestSupport/run-signed-test.sh" \
    BLUEICE_TEST_LAUNCHER_EXE="$PWD/frontend/macos/.build/Build/Products/Debug/BlueIce.app/Contents/MacOS/blueice-launcher" \
    "$HOME/.cargo/bin/rustup" run 1.96.0 cargo test \
    -p blueice-launcher --bin blueice-launcher --test owner_lifetime --locked
```

XCUITest requires an unlocked graphical login session and macOS approval for
Xcode's test runner to automate the UI. The tests operate only the BlueIce
application. They use stable accessibility identifiers on native controls,
launch the bundled service stack, and inspect screenshots of the actual viewport.
They do not replace the core with a simulated renderer.

Protocol tests cover fragmented/truncated/oversized messages, Rust envelope
encoding, uncorrelated broadcasts, frame path/size/symlink boundaries,
startup failure, real core rendering, policy-reviewed HTTP pixels, malicious URL
and hidden prompt-injection denial, preservation of committed page/history,
unavailable review without fetching, cancellation during startup, and normal
and forced owned process-group cleanup. UI tests cover visible core pixels, settings
and back/forward history, reload of the committed URL, independent tabs and
closing the last tab, resize, visible navigation denial, window-close exit,
and startup failure with disabled controls. A loopback HTTP fixture supplies
safe and rejected HTML; the actual compiled gatekeeper rules review it.
The native-window tests also verify HTTP pixels, back/forward, reload of a
committed external URL, and recovery after content denial. Page accessibility
coverage includes typed representation decoding, bounded tree validation,
source/tab/frame/URL correlation, core-reported input capabilities, protected
value redaction, hierarchy/roles/state, scroll and Retina coordinate mapping,
native hit testing, stale element rejection, and object identity across frame
refreshes. A real-core XCTest invokes native accessibility focus/press and
checks ordinary input plus policy-reviewed link navigation. XCUITest reads the
OS-visible page elements and verifies typing, privacy and tab isolation.
The XCUITest runner's test-only network-server entitlement permits this
loopback fixture inside Xcode's runner sandbox.
Native editing tests exercise the real core's UTF-16 selection, CJK/RTL
composition callbacks, replacement/cancellation, multiline caret and candidate
geometry, password redaction, readonly rejection and stale-document fences.
Native-window tests add keyboard selection/deletion and copy/cut/paste with
multiline CJK/RTL and emoji grapheme deletion. They save and restore the test
clipboard and keyboard input source.

The separate system Zhuyin test sends physical key codes to the unique active
BlueIce test process. This requires Accessibility permission for
`.build/Build/Products/Debug/BrowserUITests-Runner.app` under System Settings →
Privacy & Security → Accessibility, plus an enabled Traditional Zhuyin input
source. It explicitly skips when either prerequisite is missing. Deterministic
`NSTextInputClient` composition callbacks do not establish that an actual OS
input method passed. The scripts do not change macOS privacy permissions.
The backend command uses the signed native service bundle and a Cargo runner
that locally signs test executables. Without the binary override, the Rust
tests use isolated copies of the Cargo-built sibling services.

Result bundles are under `.build/`. XCUITest retains a window screenshot as
an attachment; `test.sh` exports the attachments beside the result bundle.
Build products, result bundles, local Xcode state and screenshots are ignored
by Git. See [the service integration validation record](../../development/browser_core/phase-22-browser-shell-accessibility/MACOS_SERVICE_RESULTS.md)
for those measured results, and [the page accessibility validation record](../../development/browser_core/phase-22-browser-shell-accessibility/MACOS_ACCESSIBILITY_RESULTS.md)
for the later native bridge checks. The
[native editing validation record](../../development/browser_core/phase-22-browser-shell-accessibility/MACOS_NATIVE_EDITING_RESULTS.md)
distinguishes passed native checks from the pending physical input-method test.

Apple documents the native adapters and test entry points in
[NSViewRepresentable](https://developer.apple.com/documentation/swiftui/nsviewrepresentable)
and [XCUIApplication](https://developer.apple.com/documentation/xcuiautomation/xcuiapplication).

## Current scope

The shell supports built-in pages (`about:credits`, `about:settings`), reviewed
HTTP(S) navigation, native
address editing, tab creation/selection/closing, back/forward/reload,
viewport resize and core-owned page text editing. IPC uses the existing version-two
length-prefixed browser envelopes with an 8 MiB limit. Frame files are
mapped read-only within a fresh private runtime directory and validated before
image creation. Newer per-tab generations supersede older frames. Shutdown
first requests normal service exit, then terminates the owned process group if
needed, and removes its private sockets and frames. The runtime root has mode
0700 and short paths for Darwin's socket limit. Child-only `TMPDIR` and
`XDG_RUNTIME_DIR` select it; persistent gatekeeper settings retain their normal
owner-selected location. Missing services and unavailable review fail closed.
An inherited lifetime pipe and the launcher's opt-in `--exit-on-stdin-eof`
also request normal service shutdown if the GUI is killed. An abrupt GUI
exit can leave an empty runtime parent; normal frontend shutdown removes it.

The default app owns its launcher; attaching to an existing shared launcher,
assistant and trusted permission panels remain separate work. The
[macOS delivery plan](../../development/browser_core/phase-22-browser-shell-accessibility/MACOS_DELIVERY_PLAN.md)
tracks the remaining milestones. The page exposes the core's semantic representation through virtual
NSAccessibility elements: names, roles, hierarchy, values/state and screen
bounds. Labels and text use the existing core name algorithm; this is not a
complete HTML/ARIA implementation. On macOS 26 and newer, headings use the
native heading role; older systems fall back to static text with a heading-level
role description. Link/button activation and supported text-input focus use
the ordinary core click pipeline. Checkbox/slider/select values are currently
read-only through this adapter. Protected input values are redacted.

The AppKit viewport implements `NSTextInputClient` over a versioned core editing
state. Text/password inputs and textareas support UTF-16 selection, grapheme
movement/deletion, marked-text update/commit/cancel, replacement ranges,
document-derived caret/selection geometry and clipped control scrolling. The
native Edit → Input Source menu selects the current responder's input context,
including the address field. Copy/cut omit password contents; paste is an
explicit user operation with a bounded payload. Readonly controls permit
selection but reject writes, and disabled controls cannot become editors.
Commands are fenced by frame source, document and focus generations and
serialized through core acknowledgements. The shell retains only pending IME
range metadata while waiting for the core; it has no parallel DOM or layout.

Frame refresh temporarily suspends semantic actions until the representation
matches the current tab, source, generation and URL. Native element identities
survive a refresh within one document; reload, navigation and tab changes
invalidate old elements. The accessibility bridge does not expose writable
AXValue, accessibility text ranges, live-region announcements or rotor search.
Native editing geometry is currently available through `NSTextInputClient`,
independently of those AX text APIs. Actual OS IME verification is pending
runner Accessibility permission on the recorded host. JavaScript
keyboard/beforeinput/input/composition event dispatch, undo/redo, complete
bidirectional shaping and caret blink remain open, as do keyboard-only forms,
find/context menus, groups, multiple windows, downloads/printing/permission
panels and localization. Automated checks cover the recorded features; an
interactive VoiceOver session remains unvalidated.
