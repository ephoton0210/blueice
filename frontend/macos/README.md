# BlueIce macOS frontend

This first macOS slice uses SwiftUI for browser chrome and AppKit for the
window lifecycle, core pixel viewport, pointer/scroll input and committed
text input. The frontend launches a private `blueice-core --stdio` child;
the Rust core owns pages, history, tab identities and rendering.

## Build and run

Requires macOS 14 or newer, a full Xcode installation with its command-line
tools selected, and the repository's Rust 1.96 toolchain. No third-party
Swift packages or project generator are required. Builds target the host's
architecture; Intel builds have not been validated on this Apple Silicon host.

```sh
frontend/macos/build.sh
open frontend/macos/.build/Build/Products/Debug/BlueIce.app
```

`build.sh Release` builds an optimized core and frontend. The Xcode project
copies and locally signs the freshly built core inside the app bundle. It
reuses `.build/core-target` for Rust output, keeping native frontend builds
independent of the workspace's larger cache; an explicit `CARGO_TARGET_DIR`
overrides that directory. Builds use an ad hoc identity for local development;
distribution signing and notarization are separate work. To build from Xcode, first build the core
with the script, then open `BlueIce.xcodeproj` and select the shared `BlueIce`
scheme. `--core-exe /absolute/path/to/blueice-core` can override the bundled
executable when running the app executable directly.

## Automated validation

```sh
frontend/macos/test.sh
# Run only the protocol/real-process tests:
frontend/macos/test.sh -only-testing:ProtocolTests
# Run only the actual native window tests:
frontend/macos/test.sh -only-testing:BrowserUITests
```

XCUITest requires an unlocked graphical login session and macOS approval for
Xcode's test runner to automate the UI. The tests operate only the BlueIce
application. They use stable accessibility identifiers on native controls,
launch the bundled core, and inspect screenshots of the actual viewport.
They do not replace the core with a simulated renderer.

Protocol tests cover fragmented/truncated/oversized messages, Rust envelope
encoding, uncorrelated broadcasts, frame path/size/symlink boundaries,
startup failure, real core rendering, blocked external navigation, and
owned process/frame cleanup. UI tests cover visible core pixels, settings
and back/forward history, reload of the committed URL, independent tabs and
closing the last tab, resize, visible navigation denial, window-close exit,
and startup failure with disabled controls.

Result bundles are under `.build/`. XCUITest retains a window screenshot as
an attachment; `test.sh` exports the attachments beside the result bundle.
Build products, result bundles, local Xcode state and screenshots are ignored
by Git. See [the macOS validation record](../../development/browser_core/phase-22-browser-shell-accessibility/MACOS_RESULTS.md)
for the measured results.

Apple documents the native adapters and test entry points in
[NSViewRepresentable](https://developer.apple.com/documentation/swiftui/nsviewrepresentable)
and [XCUIApplication](https://developer.apple.com/documentation/xcuiautomation/xcuiapplication).

## Current scope

The shell supports built-in pages (`about:credits`, `about:settings`), native
address editing, tab creation/selection/closing, back/forward/reload,
viewport resize and basic page input. IPC uses the existing version-two
length-prefixed browser envelopes with an 8 MiB limit. Frame files are
mapped read-only within a fresh per-child directory and validated before
image creation. Newer per-tab generations supersede older frames. Shutdown
first requests normal core exit, then terminates only its owned child if
needed, and removes its private frames.

The private-pipe mode still lacks launcher/gatekeeper/service integration:
external navigation is visibly denied. Complete page IME composition,
selection/caret geometry, clipboard policy, groups, multiple windows,
downloads/printing/permission panels, localization, and the DOM-derived
NSAccessibility bridge remain Phase 22 delivery items. AppKit accepts text
input commits, but the initial viewport does not implement a complete
document text editor or expose page semantics to VoiceOver. The shell's
accessible controls and XCUITest coverage do not establish those capabilities.
