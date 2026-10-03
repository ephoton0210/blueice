# macOS native select controls acceptance — 2026-10-03

Accepted on Apple Silicon macOS with Xcode 27.0 and Rust 1.96.0. The complete
native suite passed **161 cases with 0 failures and 1 physical Zhuyin skip**:
96 native XCTest passes and 65 XCUITest passes. The Rust workspace passed
**7,226 cases, 69 ignored and 0 failures**. Workspace all-targets build,
strict Clippy and formatting checks exited 0. Eight strict bundle/service
signature checks passed, and the owned parent/private-panel/UI-runner executable
prefixes contained no remaining processes after teardown. Parent and private
panel executables are universal x86_64/arm64; execution acceptance is arm64.

The native result is `frontend/macos/.build/results-20261003-222511.xcresult`.
Its 52 runtime warnings all concern internal QoS priority inversions;
0 concern SwiftUI view updates. No exclusions, new skips, warning suppression
or automatic retries were introduced. Focused passes overlap the full gates and
are not added to their totals. All **1,782 frozen source/build/fixture inputs**
remained unchanged across complete acceptance; the aggregate SHA-256 is
`dda375645253af91efd1680f9bd0e4cd8034ebd6a8edf01b2b09ad48c3b86c89`. Commands and file hashes are recorded in
[macos-select-controls-results.txt](artifacts/macos-select-controls-results.txt).

Single-choice controls present a real AppKit NSMenu with option identities,
option label attributes, optgroup headings, disabled choices and selected state.
Mouse and Enter/Space opening wait for acknowledged focus; Escape cancels.
Native choices use the ordered input queue and must belong to the live focused
select. Popup choices additionally fence the exact frame that supplied labels
and enabled state. New frames, navigation, ownership/focus changes and native
chrome editing invalidate tracking. The core performs mutations; the menu
contains no copied DOM or independent selectedness model.

Multiple and size-based selects render selected/disabled/active option rows
through the shared layout and paint pass. Option pointer/accessibility bounds
are clipped to the same content box. Command click and Space toggle choices,
Command arrows move the active row, Shift extends ranges while skipping disabled
options, Command-A selects enabled choices, and Home/End/PageUp/PageDown navigate.
Wheel scrolling over a list preserves page focus, selectedness and outer scroll;
keyboard movement reveals the active row. Typed Unicode prefixes and repeated
letters search the complete core option list. Focus changes end search/range
sessions; form reset and document replacement clear transient interaction state.
The multiple-select public value is its first selected option. Existing GET/POST
entry construction preserves selected enabled values in DOM order, including
repeated names. An unselected size-based list has no implicit first choice.

Native accessibility lists expose selected children and supported option
selection writes. AX focus preserves current selection. Disabled, clipped or
invalidated elements cannot perform those writes. Select state decodes alongside
older text-state payloads, with malformed ownership, duplicate choices,
incompatible presentation and unknown active choices rejected. The optional
Rust state uses indirection to preserve compact outer messages/events.

Native metadata is bounded to 1,024 choices and 256 Unicode scalars per
label/group. The menu reports omitted content; core keyboard navigation/search
retains access beyond the presentation bound. The core list itself is not
truncated for form submission or selection. The scope follows the native select
interaction described by the
[WHATWG HTML select specification](https://html.spec.whatwg.org/multipage/form-elements.html#the-select-element),
without claiming complete HTML select conformance.

The first regression failed because multiple-select ArrowDown was deliberately
a no-op. Public socket cases now cover Shift ranges, Command movement/Space,
Select All, identity rather than value matching, disabled/foreign/stale choices,
Unicode search, clipping, empty selection, reset, scrolling without focus,
Page keys and keyboard access beyond 1,024 choices. Layout cases verify popup
intrinsic height, labels/groups and partial-row clipping. AppKit cases operate
real services, menu actions, AX writes, state decoding and POST submission.
XCUITest uses physical native menu/mouse/key paths for popup choices, cancellation,
Command/Shift selection, reset and GET/POST data. New cases passed again in the
complete frozen suite.

Exploratory fixes corrected the XCUITest list query, explicit CSS longhands/UA
border expectations in a layout fixture, helper ordering and outer enum size.
They did not weaken behavioral assertions. The final complete native run passed
on its first execution after source freeze.

Complete DOM keyboard/beforeinput/input/change/focus events, HTML dirty
selectedness/defaultSelected IDL, customizable selects, full option CSS/layout,
complete constraint validation, physical VoiceOver and physical OS IME acceptance
remain pending. Line coverage, distribution signing/notarization and fresh
Linux/Windows acceptance are not claimed by this increment. The macOS browser
remains an active delivery target in [MACOS_DELIVERY_PLAN.md](MACOS_DELIVERY_PLAN.md).

The unedited screenshot `artifacts/macos-select-controls.png` was inspected and
remains ignored. SHA-256: `579aac752af00d558c1335f4eed7d4917bfbdaf18590bc6815851c3def70cc7c`.
It shows Beta, selected Topic Alpha/Gamma and a disabled Topic Locked row. The
existing Local Network system prompt remains visible; it was neither accepted,
dismissed nor edited out. The physical Zhuyin runner still lacks Accessibility
trust; no TCC changes were made. Screenshots, xcresult and build artifacts remain
outside Git tracking.
