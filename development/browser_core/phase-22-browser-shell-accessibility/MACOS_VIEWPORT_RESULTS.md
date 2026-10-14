# macOS native viewport and page zoom results

Validation finished: 2026-10-03T00:50:10+08:00. Base: `531b6b2df48107873f65d86948ed1c24e237f9a9`.
This records the display/zoom/fullscreen increment in the active
[macOS delivery plan](MACOS_DELIVERY_PLAN.md). Other browser milestones remain open.

## Delivered behavior

- Native logical content dimensions, backing density and per-tab page zoom are
  separate core inputs. At 100%, a CSS pixel occupies a native point on this
  Retina host; high-density text is rerasterized at the output scale. The old
  shell incorrectly supplied backing pixels as CSS dimensions, halving visible
  font/control sizes. The adapter follows Apple's distinction between points
  and [backing scale](https://developer.apple.com/documentation/appkit/nswindow/backingscalefactor)
  and the CSS viewport/page-zoom model in [CSSOM View](https://www.w3.org/TR/cssom-view/).
- Core layout uses logical size divided by page zoom. Physical bitmap edges
  come from logical size times backing density, independently of zoom. Native
  frames rasterize transformed paint commands/glyphs directly into a bounded
  viewport, including scroll and nested clips. No full-page bitmap or enlarged
  low-resolution image is needed on this path. Pixel edges are capped at 4096;
  the frontend reduces raster density when needed while preserving logical
  layout within its 4096-point limit.
- Frame/source/tab-correlated viewport metadata supplies CSS geometry for
  pointer input, wheel deltas, accessibility clipping/actions, find and IME
  candidate positioning. Invalid wire geometry fails softly. Invalid scale,
  zoom, dimensions and closed-tab requests preserve live state.
- Core-owned 25–500% zoom is per tab and survives navigation, reload and history.
  New tabs start at 100%. View-menu zoom presets, Command-plus/equal,
  Command-minus and Command-zero work in the native window. The status-bar
  percentage resets actual size; pending zoom commands retain their intended
  next step through rapid acknowledgements.
- Control-Command-F and View > Toggle Full Screen use NSWindow fullscreen.
  Entry/exit updates the same logical/backing viewport contract and restores
  the original window size.
- Additive protocol-two display messages leave legacy frame streams unchanged
  until a native display/zoom command opts in. MCP ignores viewport metadata
  while waiting for actual navigation completion; the reference frontend also
  tolerates the new broadcast.

## Final verification

| Check | Authoritative final result |
| --- | --- |
| XCTest | 44 passed: accessibility, protocol/process and real-core editing/keyboard/form/find/menu/viewport tests |
| Actual XCUITest | 26 passed; one physical Zhuyin skip because the runner lacks Accessibility trust |
| Final-core native recheck | 2 XCTest and 2 XCUITest passed after the legacy-viewport boundary correction |
| Rust affected crates | 1,272 passed across 27 all-target test executions: engine 867, IPC 200, raster 14 and MCP 191; zero failed/ignored |
| Workspace compile | `cargo check --workspace --all-targets --locked --offline` passed |
| Affected Clippy | engine, IPC, raster, MCP and reference frontend, all targets, `-D warnings`, passed |
| Formatting / project / signatures | Rustfmt, Git whitespace, Xcode project plist and strict app/service signatures passed |
| Owned application/services | Normal native teardown completed; 0 exact bundled app/service processes remained |

Complete native bundle: `frontend/macos/.build/results-20261003-003623.xcresult`.
Attachments: `frontend/macos/.build/results-20261003-003623-attachments`. The structured report
contains 71 tests, 70 passed, zero failed and one skipped.
18 issues are XCTest internal priority-inversion diagnostics; 0
SwiftUI view-update state warnings were observed.

The full suite finished at 2026-10-03T00:46:31+08:00. Its UI and viewport implementation
were unchanged in the final recheck; the later core-only correction rejects
oversized legacy dimensions before native zoom/raster opt-in. That rejection
and recovery ran in the final all-target Rust suite. The final-core native
recheck is `frontend/macos/.build/results-20261003-004919.xcresult` (four passed, zero
failed/skipped), exercising the normal Retina/zoom/tab/fullscreen paths again.

The new actual-window cases inspect a 280-CSS-pixel editor at 280 native points
on a high-density display, then verify widths at 110% and 150%, menu/shortcut
zoom, native 中文 paste and right-click Select All, independent new-tab zoom,
source-tab value retention, reload with retained zoom and exact HTTP request
counts. They also verify physical Control-Command-F fullscreen entry/exit and
window-size restoration. The passing-run screenshot is of the actual window.
All prior native cases remained enabled and ran in the complete suite.

Six public session regressions cover density changes without CSS reflow,
zoom-dependent text wrapping and pixel geometry, navigation/snapshot-history
retention and new-tab defaults, invalid/closed-tab commands, zoomed editing/menu/
find scroll with actual highlight pixels, and fractional dimensions through
zoom from 25% to 500%, including the 4096 edge. A legacy viewport above the
native raster cap is rejected before opt-in without changing zoom; a valid
viewport repair then permits ordinary zoom. Three raster regressions verify
transformed nested clips/scroll, high-density font antialiasing distinct from
nearest-neighbor enlargement, and pre-allocation limits/nonfinite rejection.
IPC verifies typed messages and request/tab correlation. The existing MCP
history barrier injects viewport metadata before its actual completion.

## Corrections found during verification

Test-first raster compilation failed before the new entry point existed.
A fractional native viewport regression then exposed frame `(300, 200)` while
metadata advertised `(301, 201)` near an integral edge. An epsilon-based rounding
workaround also depended on zoom division/multiplication. The final rasterizer
accepts physical edges explicitly from the same DisplayViewport calculation
used by metadata. The final fractional/limit test retains exact dimension
assertions at all tested zoom factors.

A final boundary regression reproduced a core panic when an oversized legacy
viewport first enabled page zoom. SetPageZoom now validates the effective
display descriptor before changing any tab/snapshot state. The real-session
test retains the error, unchanged-state and successful-recovery assertions.

XCUITest's synthetic `typeText` of 中文 left a system InputSource popup that
interrupted the next right-click. The case now issues the user's native
Command-V with the same exact Unicode value, saves/restores every clipboard
representation, and retains all editor/menu/tab/value/request assertions.
Physical input-method acceptance remains independently skipped.

The expanded all-target Rust run found an older extension integration assertion
expecting a textarea newline to become a space. The already-existing
`extension_textarea_write_only_changes_live_enabled_native_textareas` unit test
requires the exact multiline value, and current native editing preserves it.
The integration assertion now also requires `from extension v4\nwith detail`;
no production behavior changed for this correction. Two unnecessary clones of
the Copy input-context type were removed after warnings-denied Clippy flagged them.

## Reproduction and artifact identity

Use the existing shared build cache and signed test runner:

```sh
export CARGO_TARGET_DIR="$PWD/frontend/macos/.build/core-target"
export CARGO_TARGET_AARCH64_APPLE_DARWIN_RUNNER="$PWD/frontend/macos/TestSupport/run-signed-test.sh"
"$HOME/.cargo/bin/rustup" run 1.96.0 cargo test \
  -p blueice-engine -p blueice-ipc -p blueice-raster -p blueice-mcp-server \
  --all-targets --locked --offline
frontend/macos/test.sh
"$HOME/.cargo/bin/rustup" run 1.96.0 cargo check --workspace --all-targets --locked --offline
"$HOME/.cargo/bin/rustup" run 1.96.0 cargo clippy \
  -p blueice-engine -p blueice-ipc -p blueice-raster -p blueice-mcp-server \
  -p blueice-frontend-reference --all-targets --locked --offline -- -D warnings
"$HOME/.cargo/bin/rustup" run 1.96.0 cargo fmt --all -- --check
```

Local socket/process fixtures and Xcode's GUI test/report services require the
host's local permissions. Verified host: Apple Silicon macOS 26.6.2 (25G83),
Xcode 27.0 (27A266a), Swift 6.4 and Rust 1.96.0. Debug Swift app compilation
includes arm64/x86_64; runtime acceptance here ran on Apple Silicon.

Seventeen changed production/Swift test inputs remained unchanged during the
complete native run; combined SHA-256: `4c4afe97cee707b667c29edaed72fcdf0b2766e3fb7ffa86018ba1924f18c733`. After the
core-only legacy opt-in validation correction, the same native recheck uses
final SHA-256 `1b034b532ccaeaccf91a9673a1f1d6218bf7a88d7139145629e0f9d4147d25b9`. Five Rust-only regression inputs were checked
in the final Rust run. The final 22 changed
Rust/Swift production/test inputs use sorted relative path, a NUL, raw bytes
and a NUL for SHA-256: `539659aa2466d7955af5d1ee95b9aacf8ea223b4da6baa7b9ccef54c1d86915a`. Documentation and generated artifacts are excluded.

The directly inspected actual-window screenshot is
`artifacts/macos-retina-viewport.png`, SHA-256 `af5b230f1bbdb77ff7cc7ac8afd6bd6a7313f98e8e99568e629258e7a94ee133`. It was exported without
image editing. PNGs, result bundles and build outputs are ignored and untracked.
Compact non-sensitive metadata is retained in
`artifacts/macos-viewport-results.txt`; logs and failure attachments stay local.

## Remaining acceptance

Synthetic density changes establish the geometry contract; physical monitor
handoff, Intel/older macOS runtime and additional display modes remain unproven.
Theme/high contrast/reduced motion, localization, remaining editing/form events
and validity, multi-window/tab-group/profile handoff, downloads/print, trusted
human permission/assistant panels and full accessibility/final design acceptance
remain in the delivery plan. Existing overflow/text-shaping/core limitations
remain. This run does not claim Linux/Windows native UI, full Rust workspace
execution/coverage, physical OS IME, actual VoiceOver or distribution/notarization
acceptance. No system privacy permissions were modified.
