# macOS SwiftUI/AppKit first-slice validation

Later page NSAccessibility implementation and validation are recorded in
[`MACOS_ACCESSIBILITY_RESULTS.md`](MACOS_ACCESSIBILITY_RESULTS.md). The results
below describe this earlier slice.

This records the original private-pipe slice. The default app's later launcher
and gatekeeper integration is recorded in
[`MACOS_SERVICE_RESULTS.md`](MACOS_SERVICE_RESULTS.md).

Validated on 2026-10-02 using a real graphical login session on macOS 26.6.2
(Apple Silicon), Xcode 27.0 (27A266a), Swift compiler 6.4 and Rust 1.96.0.
The application uses SwiftUI chrome, an AppKit window/pixel/input viewport
and the bundled, locally signed Rust core. No simulated renderer is used.

## Results

| Check | Result |
| --- | --- |
| `frontend/macos/test.sh`: core/app build, tests and screenshot export | Passed, exit 0 |
| Swift XCTest protocol/frame/real-process tests | 8 passed, 0 failed |
| Actual native-window XCUITest | 8 passed, 0 failed |
| Rust `blueice-engine --test stdio_session`, Rust 1.96.0 | 4 passed, 0 failed |
| App and bundled core ad hoc signature verification | Passed |
| Xcode project/shared scheme parsing and shell syntax | Passed |
| Git whitespace validation | Passed |
| Remaining private macOS frame directories after tests | 0 |
| Screenshot, build products and result bundles excluded by Git | Confirmed |

The Swift tests exercise fragmented, truncated, empty and oversized frames;
Rust envelope encoding and request/tab identities; broadcasts without request
IDs; frame size/path/symlink boundaries; missing-core failure; real core pixels;
external navigation denial; repeated shutdown and owned-child/frame cleanup.

The eight XCUITest cases exercise:

1. Actual visible core-rendered pixels: the viewport screenshot must contain
   more than 100 sampled dark pixels, and the real window screenshot is retained.
2. Settings navigation and back/forward history.
3. Reload of the committed URL while the address contains an unsubmitted draft.
4. Independent tab state, selection, closing an inactive tab and the last tab,
   then reopening a tab.
5. Native window resize, with a new core frame at the changed viewport width.
6. Visible, fail-closed external navigation and recovery by reloading the
   last committed built-in page.
7. Window-close termination of the application.
8. Startup failure with a visible message and disabled navigation controls.

The Rust public-boundary tests separately verify fragmented polling,
oversized-frame rejection, actual stdio rendering, absence of an HTTP request
when navigation is denied, normal frame cleanup, and refusal to reuse/delete
an existing frame directory. No Rust source was changed for this macOS slice;
the full workspace suite and workspace coverage were not rerun here.

## Reproduction and artifacts

```sh
frontend/macos/test.sh
CARGO_TARGET_DIR="$PWD/frontend/macos/.build/core-target" \
    "$HOME/.cargo/bin/rustup" run 1.96.0 cargo test \
    -p blueice-engine --test stdio_session --locked
```

The verified workflow produced
`frontend/macos/.build/results-20261002-130815.xcresult` and exported its
retained attachment into the adjacent `results-20261002-130815-attachments`
directory. Both paths are local, ignored build output. The screenshot was
copied without modification to
[`artifacts/macos-swiftui.png`](artifacts/macos-swiftui.png), which is also
ignored. The compact test record is
[`artifacts/macos-ui-results.txt`](artifacts/macos-ui-results.txt).

The 13 Swift, shell, Xcode-project and shared-scheme source files have combined
SHA-256 `78977c22b981e8a29fefed2444d5cbee2e51ee0a6a63434ba6c163473a057d6f`.
This hashes sorted repository-relative paths, a NUL separator, raw file bytes
and another NUL separator; documentation and build output are excluded.
The bundled signed core has SHA-256
`4e85abe061e379836d442ee1fda8f1ea53a98dfd8f803450aac882315580c532`.

## Scope

This delivers the first macOS shell slice, matching the initial Windows
shell's built-in navigation and tab/rendering scope. The private-pipe mode
still lacks launcher/gatekeeper/service integration, so external website
navigation is denied. Complete IME/document editing, clipboard, groups,
multiple windows, downloads/printing/permission panels, localization and
DOM-derived NSAccessibility/VoiceOver support remain Phase 22 work. Native
shell accessibility identifiers and XCUITest success do not establish full
page accessibility. Intel hardware, older supported macOS versions, Release
distribution signing and notarization were not validated on this host.
