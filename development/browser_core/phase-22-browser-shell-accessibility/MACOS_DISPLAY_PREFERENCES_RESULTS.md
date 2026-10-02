# macOS display preference results

Validation finished: 2026-10-03T02:25:13+08:00.
Base: `91f2dd78c1db312745078772e68c0e7bd66c525f`.
This records the appearance/contrast/motion increment in the active
[macOS delivery plan](MACOS_DELIVERY_PLAN.md). Other browser milestones remain open.

## Delivered behavior

- BlueIce > Settings (Command-comma) provides System/Light/Dark appearance,
  System/Standard/Increased contrast and System/No reduction/Reduce motion.
  View > Appearance also changes the appearance. Choices persist in the app's
  UserDefaults domain and Restore System Settings returns all three choices
  to System. Overrides apply to BlueIce windows without changing global macOS
  settings or the application-wide system appearance used by observation.
- The adapter observes NSApplication.effectiveAppearance and NSWorkspace's
  accessibility display options notification. The resolved values feed native
  chrome and the same Rust page environment. High contrast adds explicit
  selected-tab and toolbar borders and primary status text. Reduced motion
  disables the shell's SwiftUI animation transactions. Page behavior follows
  authored conditional CSS; this does not automatically recolor every page.
- Core-owned inline styles now evaluate nested `@media` and `<style media>`
  for screen/all, light/dark, contrast and motion preferences, CSS viewport
  width/height and resolution. Supported conditions include grouped logical
  operators, query lists, minimum/maximum and range comparisons, px/em/rem
  lengths and dppx/x/dpi/dpcm resolution. Unknown feature truth is preserved
  through negation, invalid query grammar stays invalid through `or`, commas
  inside parentheses do not split a query list and nesting is bounded.
- Preferences reach live tabs, new tabs, navigation and retained history.
  Updating preferences restyles, lays out and repaints the existing document;
  it preserves the native editor's focus, text and document identity and makes
  no new HTTP request. Typed additive protocol-two state is correlated with
  the live tab, frame directory source and frame generation. Legacy clients
  receive no extra preference broadcasts until they opt in; MCP/reference
  consumers tolerate the new state without releasing completion barriers.
- CSS media resolution uses actual native backing density times page zoom.
  The optional `backing_scale` field separates that density from a raster
  density reduced by the 4096-pixel output cap. Existing callers omit the field
  and retain their wire shape and device-scale fallback. Invalid backing scales
  leave the live viewport unchanged. Core RGBA images carry an explicit sRGB
  profile; screenshot assertions retain and convert the monitor ICC profile.
- Native NSSearchField synchronization preserves temporary AppKit marked text
  and suppresses programmatic change feedback during SwiftUI view updates.
  The existing Unicode search, privacy and tab/history assertions remain enabled.
- Rapid absolute zoom changes use one ordered, coalescing writer, and inbound
  frame/metadata delivery preserves the transport reader's FIFO order on the
  main queue. An older zoom step cannot overwrite the user's final step.

The native contracts follow Apple's
[effective appearance](https://developer.apple.com/documentation/appkit/nsappearancecustomization/effectiveappearance),
[display options notification](https://developer.apple.com/documentation/appkit/nsworkspace/accessibilitydisplayoptionsdidchangenotification)
and [marked text](https://developer.apple.com/documentation/appkit/nstextinputclient/hasmarkedtext())
APIs. The conditional CSS subset follows
[Media Queries Level 5](https://www.w3.org/TR/mediaqueries-5/), including the
distinction between valid unknown features and invalid condition grammar.

## Final verification

| Check | Authoritative final result |
| --- | --- |
| XCTest | 50 passed: accessibility, appearance, protocol/process and real-core native interactions |
| Actual XCUITest | 28 passed; one physical Zhuyin skip because the runner lacks Accessibility trust |
| Repeated real-core boundaries | Two zoom/tab and display preference cases, ten iterations each: 20 executions passed |
| Rust affected crates | 1,389 passed across 31 all-target executions: CSS 112, engine 871, IPC 201, raster 14 and MCP 191; zero failed/ignored |
| Workspace compile / Clippy | All workspace targets, locked/offline compile and warnings-denied Clippy passed |
| Formatting / project / signatures | Rustfmt, Git whitespace, Xcode project plist and strict app/service signatures passed |
| Owned app/services | Normal native teardown completed; 0 exact bundled app/service processes remained |

Complete native bundle: `frontend/macos/.build/results-20261003-021208.xcresult`.
Attachments: `frontend/macos/.build/results-20261003-021208-attachments`.
The structured report contains 79 tests, 78 passed, zero failed and one skipped.
The full suite finished at 2026-10-03T02:23:36+08:00. Twenty diagnostics are
XCTest internal priority-inversion warnings; zero SwiftUI view-update state
warnings were observed in the final run. The repeated real-core boundary bundle
is `frontend/macos/.build/results-20261003-021033.xcresult`.

The two new actual-window cases exercise native Settings controls and the View
menu, exact light/dark surface colors, a visible high-contrast selected-tab
outline on dark chrome, reduced-motion media branches, retained edited text,
normal exit/relaunch persistence, Restore System Settings, tab/history inheritance
and exact HTTP request counts. Every previous native case remained enabled in
the complete final run. The earlier failed/interrupted diagnostic runs are not
used as final acceptance evidence.

## Regressions and corrections found

Test-first CSS/protocol/native cases failed before their APIs existed. Public
tests then reproduced invalid `not`/`or` grammar and a nested comma being
mistaken for a separate matching medium. Invalid syntax now dominates its
query, while valid unsupported features retain three-valued media logic.

Native/core tests verify exact light/dark RGBA, hidden semantic branches,
same-document edited text and focus, rapid preference changes, new tabs,
navigation/snapshot history, invalid and closed-tab requests, and changes to
CSS size, backing density and zoom. The 2300-point native boundary test caps
the bitmap at 4096 pixels while a 2dppx query still matches the real density.

UI pixel checks initially failed because NSBitmapImageRep.colorAt produced
Generic RGB colors on this host, losing the screenshot's PA279CRV monitor
profile. Converting the complete CGImage into an sRGB CGContext reproduces the
CSS colors. All UI color checks now use that conversion. The reviewed-HTTP
test requires the fixture's actual `#207840` color rather than a device-space
range. Core pixel counts and an exact sRGB image round trip remain separate
assertions.

The first complete UI run exposed `café` becoming `caf´e` in the search field,
along with a SwiftUI view-update publication warning. The adapter now leaves
marked text with AppKit and prevents programmatic synchronization from feeding
back into the model. The Unicode test retains its native keyboard input and
exact search results; its expected value was not relaxed.

A later complete native run exposed the rapid-zoom tab-retention regression.
Repeating it ten times with stronger diagnostics reproduced actual 125% state
after a final 150% request. The independent outbound Tasks had sent an older
step last. A single coalescing zoom writer now preserves command order. The
reader also uses ordered main-queue delivery so metadata cannot overtake its
matching frame. Both zoom/tab retention and display preference editing run
repeatedly against the real core before the complete native rerun.

The named accessibility high-contrast Aqua variants resolve to the same Aqua
objects on this macOS runtime. Native contrast acceptance therefore checks the
actual explicit selected-tab outline on the same dark chrome, along with
the core's conditional CSS and persisted user choice.

## Reproduction and artifact identity

```sh
export CARGO_TARGET_DIR="$PWD/frontend/macos/.build/core-target"
export CARGO_TARGET_AARCH64_APPLE_DARWIN_RUNNER="$PWD/frontend/macos/TestSupport/run-signed-test.sh"
"$HOME/.cargo/bin/rustup" run 1.96.0 cargo test \
  -p blueice-css -p blueice-engine -p blueice-ipc -p blueice-raster \
  -p blueice-mcp-server --all-targets --locked --offline
frontend/macos/test.sh
"$HOME/.cargo/bin/rustup" run 1.96.0 cargo check --workspace --all-targets --locked --offline
"$HOME/.cargo/bin/rustup" run 1.96.0 cargo clippy --workspace --all-targets --locked --offline -- -D warnings
"$HOME/.cargo/bin/rustup" run 1.96.0 cargo fmt --all -- --check
```

Local socket/process fixtures and Xcode's GUI/report services require host
permissions. Verified host: Apple Silicon macOS 26.6.2 (25G83), Xcode 27.0
(27A266a), Swift 6.4 and Rust 1.96.0. The Debug Swift app compiles for both
arm64 and x86_64; runtime acceptance here uses Apple Silicon. UI tests isolate
their preference domains, delete only those domains and restore the original
input source and any clipboard representations they use.

All 29 changed Rust/Swift source, fixture, test and project inputs remained
unchanged throughout the final complete native run. Their sorted relative path,
a NUL, raw bytes and a NUL produce SHA-256
`94a908c7f8892ef22acc89bf2301ac7c85bfe621a47daa8e71f78e2fc436f481`.
Documentation and generated artifacts are excluded. The affected Rust tests
used final Rust source; the later native ordering corrections changed Swift only.

The directly inspected actual-window screenshot is
`artifacts/macos-display-preferences-dark.png`, SHA-256
`6ab3254490b18a2d1898365cc3c9b77b5e9c181b6e0230c2bb99df88b839e812`.
It was exported without image editing and shows the dark native chrome,
explicit contrast borders, retained editor text and core media-driven content.
PNG screenshots, result bundles and build output remain ignored and untracked;
logs and failure attachments stay local. Compact non-sensitive metadata is
retained in `artifacts/macos-display-preferences-results.txt`.

## Remaining acceptance

System accessibility option observation uses an injected reader/notification
in XCTest; actual NSApplication appearance KVO is exercised separately.
Physical global-setting transitions, monitor handoff, Intel/older macOS runtime
and wide-gamut/HDR output remain unproven. The core still lacks external
stylesheet/import fetching, general media features, print media output,
JavaScript matchMedia, automatic CSS color-scheme UA recoloring, forced colors,
reduced transparency and the complete CSS animation pipeline. The motion
override applies to shell SwiftUI transactions and authored media rules; it
does not claim control over every AppKit/OS animation.

Physical OS IME, actual VoiceOver, localization, remaining editing/form events
and validity, multi-window/tab-group/profile handoff, downloads/printing,
trusted human permission/assistant panels and final browser acceptance remain
in the delivery plan. This run does not claim Linux/Windows native UI,
full-workspace Rust test execution or coverage, distribution signing or
notarization. No system privacy settings were changed.
