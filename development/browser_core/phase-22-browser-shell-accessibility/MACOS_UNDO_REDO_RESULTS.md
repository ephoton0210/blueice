# macOS native Undo/Redo results

Accepted on 2026-10-03 on Apple Silicon macOS 26.6.2 (25G83), Xcode 27.0
(27A266a), Swift 6.4 and Rust 1.96.0. Base commit:
`3932537a7a8061d7714edb2a9153f885af6d0a64`.

| Gate | Final result |
| --- | --- |
| Full native XCTest / XCUITest | `results-20261003-205824.xcresult`: **155 passed, 0 failed, 1 skipped**; 92 XCTest and 63 UI passes, terminal exit 0 |
| Full Rust workspace | **7,217 passed, 69 ignored**, 0 failures, terminal exit 0 |
| Workspace build, all targets | Passed, terminal exit 0 |
| Workspace Clippy, all targets, `-D warnings` | Passed, terminal exit 0 |
| Workspace formatting | Passed, terminal exit 0 |
| Strict bundle signatures | All 8 passed: parent, private panel app and 6 services |
| Owned native bundle processes after UI | **0**, Rust tests and other bundle prefixes excluded |
| Final native source/build freeze | **70 unchanged**, aggregate SHA-256 `5f1fc2fafd792a4c70d94dd94cfa601818100c66c98a86acc990ae7b4d787c6e` |

The physical Zhuyin test remains skipped because the UI runner lacks
Accessibility trust. No TCC settings were changed. The accepted native result
contains 50 internal runner QoS warnings and 0 SwiftUI view-update warnings.
Parent and private panel executables are universal x86_64/arm64; actual execution
acceptance is arm64. Focused passes overlap these gates and are not added to them.
All Rust/Cargo inputs remained unchanged throughout the workspace gates and the
native test corrections; only the session status view and UI observation
sequencing changed after the first freeze. Commands, hashes and audit receipts
appear in [macos-undo-redo-results.txt](artifacts/macos-undo-redo-results.txt).

The core owns private before/after text and UTF-16 selections for individual
controls. Histories share a per-page limit of 128 transactions and 4 MiB of
UTF-8 snapshots. Zeroizing snapshot storage has no serialization implementation;
only availability and the eviction flag leave the core. Password text remains
absent from native inspection, AI representation and accessibility values.

The native Edit menu, Command-Z/Shift-Command-Z and page context menu all forward
to the ordered text-input IPC boundary. The AppKit undo-manager proxy disables
undo registration. Cocoa address/find responders keep their native editing.
Selection replacement/paste/cut and committed composition are atomic; typing
and same-direction deletion coalesce within one second when selection and focus
remain contiguous. Movement, selection, focus and other actions close groups.
Marked composition disables replay, including pending AppKit callbacks;
cancellation preserves the earlier redo branch. New edits discard the current
control's redo branch.

The same live core page carries its histories during tab/window transfer, with
fresh focus ownership fencing queued source commands. Navigation/reload and
history document replacement clear histories. Form reset, successful external
value/text writes (including same-value writes), removed controls and controls
that become non-writable or change protected/multiline type invalidate affected
histories. Session restoration never persists them. Eviction drops the oldest
undo transactions or an old redo branch while keeping remaining chains valid;
the native context menu reports that earlier edits may be unavailable.

Focused public IPC tests cover Unicode/UTF-16 selection, identical restored
pixels, independent controls, stale contexts, typing/IME grouping, redo branches,
password redaction, same-value mutation, reset and the 128-transaction limit.
Core unit regressions cover the 4 MiB byte limit, global redo eviction,
composition cancellation/focus commit, textarea script writes, readonly changes
and replaced documents. Native AppKit tests cover composition selections,
pending callbacks, queued Undo, disabled registration, password privacy and
window transfer/reload. XCUITest operates actual native menus, shortcuts,
context menus, multiline clipboard, Cocoa address editing, password/readonly
controls, independent tabs and window transfer/reload. Older text-state payloads
default to unavailable Undo/Redo; Swift and Rust action encoding is checked.

Exploratory failures were test construction issues: a raw DOM ID instead of a
private script handle, optional UInt32 fixture-length comparisons, and a generic
paste helper waiting for plaintext in a correctly redacted secure field. The
accepted cases use the established boundaries without weakening behavior or
privacy assertions. The first full native run also exposed a Settings layout race: the first
session save inserted a status row while automation targeted the Reopen switch.
The failed domains persisted Remember without a Reopen value. The status row
now reserves its space; UI cases await the initial save, enabled switch and
confirmed checked state before relaunch. No automatic retries or preference
injection replace the native actions. A second full run exposed the live-plist
observation deadline in Forget: the app had cleared Remember and the archive,
but the UI reader timed out before the preferences daemon updated the plist.
The test now checks the immediate GUI state, normal termination, physical
archive removal and a clean relaunch before malformed-archive recovery.
The initial test-first IPC cases failed to compile because
Undo/Redo and availability were absent before implementation.

Physical Zhuyin, DOM keyboard/input/composition events, complete bidi shaping,
caret blink, physical VoiceOver, line coverage, distribution signing and
notarization remain outside this increment. No fresh Linux/Windows acceptance
is claimed. Screenshots, xcresult and build artifacts remain ignored.

The final unedited screenshot is `artifacts/macos-undo-redo.png`, SHA-256
`fffa22b6edfefb69e45dee49d7258b4acc269caae5c371ed3753ef530a3fe934`.
It was inspected and remains ignored. The existing Local Network system prompt
is visible; it was neither accepted, dismissed nor edited out. The first two
full native runs (153/2 and 154/1 passed/failed, each with one skip) are exploratory
and excluded from acceptance; the final complete suite passed after the fixes.
