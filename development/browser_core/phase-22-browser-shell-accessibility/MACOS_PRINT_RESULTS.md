# macOS native printing and PDF

Base: `b62b38aa7f614a97ed33942e6d0c985c8f58c264`.
This increment adds frozen core print-media reflow and native print/PDF panels.
The [macOS delivery plan](MACOS_DELIVERY_PLAN.md) remains active.

## Delivered behavior

File > Print and Command-P capture the current core DOM. Native paper size,
orientation and scale determine the logical printable area in PostScript points;
core converts it to CSS pixels, cascades print media, lays out and paints pages,
and rasterizes at 192 DPI. Lines and native controls move intact to the next
page. Oversized controls, more than 32 pages or 64 million pixels are rejected,
with no silent truncation. Frontend code draws only core RGBA images.

The snapshot retains committed form values and masks protected controls.
Printing excludes editor selection, caret, focus and find overlays. It performs
no navigation, fetch or script execution and leaves screen zoom/preferences,
viewport, DOM, history and editor/focus state unchanged. Source document/window
replacement, closure or transfer invalidates its job. At most two jobs live for
ten minutes; normal release, invalidation and teardown remove their private
pixel files without changing screen frame generations.

A separate broker connection serves synchronous AppKit preview callbacks while
the main browser reader stays asynchronous. Random high request IDs avoid the
ordinary broker's other-client broadcasts; replies must match request, tab,
ticket, profile and monotonic revision. Tickets identify read-only snapshots
and grant no owner/permission authority. Pixel dimensions, budget, exact file
names and job directory are checked before mapping. Connection close and
read deadlines are fenced against descriptor reuse.

AppKit supplies the actual print panel, paper/orientation/scale/page-range
controls and PDF Save dialog. Default names remove unsafe filename characters.
UI automation found two transport/lifecycle problems and one print-state defect:
other-client replies were incorrectly treated as errors; screen generations
changed during a print shortcut; and transient unsupported scaling while typing
80 left a stale failure, creating a blank one-page PDF. Replies are now properly
correlated, Begin fences the document rather than focus repaint, navigation
disables Print immediately, and valid settings clear the preview failure.
Unsupported settings still cancel; no automatic mutation retry is added.

## Verification

Acceptance ran on Apple Silicon, macOS 26.6.2 (25G83), Xcode 27.0
(27A266a), Swift 6.4 and Rust 1.96.0, on 2026-10-03.

| Check | Result |
| --- | --- |
| Rust workspace tests | 7,190 passed, 0 failed, 69 ignored, across 466 suite groups |
| Rust workspace/all-targets build and Clippy | Passed; Clippy uses `-D warnings` |
| Rust formatting, Xcode project plist and diff whitespace | Passed |
| Full native run | 106 cases: 104 passed, 1 failed, 1 skipped |
| Corrected appearance case, focused rerun | 1 passed, 0 failed |
| Native acceptance across those two runs | 66 XCTest and 39 XCUITest cases passed; 1 physical IME case skipped; no unresolved failures |
| Local app and four bundled service signatures | Verified, ad hoc |
| Owned browser/service processes after teardown | 0 |

The full native bundle is
`frontend/macos/.build/results-20261003-092656.xcresult`. Its sole failure was
the existing appearance test's whole-page near-color search: one unrelated
pixel matched the previous background's tolerance. The test now checks the
empty CSS color block beneath the editor, retaining the positive light/dark
checks and the zero-old-background assertion. It passed in
`frontend/macos/.build/results-20261003-095148.xcresult`, including preference
retention across relaunch. Only that UI test source changed between runs;
product code, fixtures and other tests retained their frozen file hashes.
The first full bundle remains recorded as failed, rather than relabeled green.

The full run includes all four new printing XCTest cases and both actual
print-panel XCUITest cases. The saved PDF has three A4 landscape pages
(842 × 595 points), 80% scaling, red print-media ink and no blue screen-only
ink. Tests also cover source invalidation, bounded profiles, protected-control
masking, pixel cleanup, preview recovery and live page/history/focus retention.

The 51-input full-run fingerprint is
`588930365da3ca538ba34abf55d61ee6a7992b711e9c19dd549033321d43a397`;
the final fingerprint after the scoped test correction is
`0e05e623f489919d4ff4e8878a2b22da54b45cffc4a3544fc932f7b52165d517`.
The unchanged 37 product/build inputs have fingerprint
`0b20b98132c88a0c855156158e4e4ef09230622b49f0633af7435a410f41f295`.
Paths and per-file hashes are recorded in
[the machine-readable result](artifacts/macos-print-results.txt).

The native app executable contains arm64 and x86_64 slices; the four Rust
services and runtime acceptance use arm64. This is not Intel runtime acceptance
or distribution signing/notarization. The full bundle reports 31 existing
QoS priority-inversion warnings, and the focused rerun reports one; neither
reports a SwiftUI view-update warning. Physical Zhuyin was skipped because the
runner lacks Accessibility trust. No system permission was changed.

Unedited print-panel images and the PDF remain in ignored `.build` attachments.
An ignored copy of the settings image is
`artifacts/macos-native-print.png`. The screenshot also shows the system's
local-network discovery permission prompt over the panel; PDF completion is
verified separately by actual file/page/ink assertions. This increment does
not claim acceptance of that permission prompt or a physical printer.

## Scope and limits

The actual GUI tests cover cancel/reopen, edited content retention, native
paper/orientation/scaling, OS folder/name selection and real PDF bytes. The PDF
is a set of raster pages, so it does not supply vector/searchable text. Full
CSS @page/break rules and block/table fragmentation, custom margin/background
controls and physical printer output are pending. The engine's existing HTML,
CSS, resource-loading and shaping limits also apply to printed documents.

File input selection and private owner permission/assistant panels remain open,
as do session/storage restoration, physical IME, VoiceOver, localization and
other [delivery gates](MACOS_DELIVERY_PLAN.md). Physical Zhuyin requires the
runner's Accessibility permission; tests do not modify TCC. This milestone
makes no new Linux/Windows acceptance or workspace coverage claim.

API behavior was checked against the installed AppKit headers and
[Apple's NSPrintOperation documentation](https://developer.apple.com/documentation/appkit/nsprintoperation/).
