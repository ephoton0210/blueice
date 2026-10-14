# macOS native tab placement — 2026-10-07

Implementation, focused/full native validation, complete Rust workspace and
final source/product acceptance are complete. Commands, all attempts, log hashes,
result summaries, signatures, process ownership and the accepted input manifest
are retained in [the validation receipt](artifacts/macos-tab-placement-results.txt).

The implementation adds native before/after ordering, collapsed/expanded group
drop targets, an ungrouped end target, same-context cross-window dragging,
cross-profile refusal and keyboard/menu alternatives. Canonical window order is
saved through the existing opt-in session archive. General page/OS drag-and-drop
remains separate delivery work. See [the contract](MACOS_TAB_PLACEMENT_CONTRACT.md).

## Complete native acceptance

The complete `bash frontend/macos/test.sh` invocation passed **199 methods,
zero failures and one existing physical Zhuyin skip**: 124 XCTest and 75
XCUITest methods passed. All nine new native methods passed, including delayed
owned-provider source/reference validation and four actual-window drag/menu/
restart cases. The signed Rust runner, service build, universal native build,
full Xcode test run and automatic attachment export reached terminal completion
with exit 0 in 3098.430 seconds. Bundle:
`frontend/macos/.build/results-20261007-020809.xcresult`.

Execution used an arm64 Mac mini, macOS 26.6.2 (25G83), Xcode 27.0 (27A266a),
Swift 6.4 and Rust 1.96.0. Both the parent app and private panel contain arm64
and x86_64; execution acceptance is arm64. The UI runner lacks Accessibility
trust for physical Zhuyin hardware events. No method was excluded and no host
trust/TCC/security approval was changed.

All 1,803 inputs remained unchanged, matching focused native, static and
complete workspace acceptance. Aggregate SHA-256:
`07aebf0a0393d06e218bffb93e446ed2e577a8568a2dd583600cedde3fde91d8`.
The result bundle records 61 identical internal QoS priority-inversion warnings,
zero failed tests and no expected failures. The run log contains no SwiftUI
view-update publishing warning. This acceptance does not claim a warning-free
runtime.

The completed-drag screenshot was visually checked and copied to ignored
`artifacts/macos-tab-placement.png`, SHA-256
`fa583da935c00f3b70c2fe1a4140b875dd5751b92beefafeecba1255965d5a80`.
It shows the reordered tabs, selected editor page, unsent address, find query,
Chinese/emoji editor content and 150% zoom without a residual insertion stripe.
Native screenshots, result bundles and build products remain untracked.

## Static and complete workspace acceptance

The final candidate passed 1,004 focused engine/IPC methods across eight result
suites (zero failures/ignored), `cargo fmt --all -- --check`, strict workspace
Clippy with `--all-targets -- -D warnings`, and the all-targets workspace build.
All four gates reached terminal exit 0 with unchanged inputs.

`rustup run 1.96.0 cargo test --workspace --locked --offline` passed **7,307
cases, zero failures and 69 existing ignored tests across 474 result suites**.
It reached terminal exit 0 in 4143.830 seconds. The run used the single existing
`frontend/macos/.build/core-target` and normal
`frontend/macos/TestSupport/run-signed-test.sh`; no readiness helper, retry,
deadline change or exclusion was introduced.

Eight strict signatures passed before and after the workspace run. Both apps
retain their universal architectures, and all eight native executable hashes
match the post-native snapshot. Source membership and bytes remain identical
across the final gates. No process owned by this increment remained.

The cleanup audit records an older Rust test PID 34421 and parent Cargo PID
33538, both started on 2026-10-03 local time before this increment. Their exact
start times and parent command distinguish them from this increment's processes;
both were left untouched. Physical VoiceOver/IME, general page/OS drag-and-drop,
storage partitioning, other full-browser requirements and Linux/Windows
acceptance remain outside this increment.

## Development evidence

The new public core boundary tests first failed to compile because `place_tab`
did not exist. The next run passed the three atomicity/ownership methods but
exposed a wire fixture that clicked before explicitly configuring its native
viewport. Adding that required fixture step retained the original response and
deadline checks. The next invocation passed all 20 focused engine/IPC methods,
including same-window marked composition and exact document/focus/frame
preservation, stale-source rejection and context ownership.

The first native UI attempt expected 150% after only two increments (actually
125%). The corrected fixture reached a real drag, which failed to update order
before native dragging was implemented. The initial sandboxed validation stopped
in the signed runner harness; the authorized native run used the normal Xcode,
codesign and UI automation environment.

The first native implementation passed source/provider and target boundary tests
but exposed an unsent address draft being replaced by the current URL. Updating
tab membership now refreshes selected-page presentation only when selection or
its URL changes. The tab title now uses a plain button style, preserving button
accessibility while allowing native dragging, and the exported custom data type
is declared in the app Info.plist. These changes enabled the actual drag. The next run
passed all four placement XCTest methods and the actual drag/editor/find/zoom UI
case. The later UI case also checks the unsent address draft directly.

Group dragging into a collapsed header and back to the ungrouped end, same-page
cross-window handoff, editing after closing the source, and cross-profile refusal
passed. The first ordering/session assertion read a nonexistent top-level URL;
the fixture now reads the current entry in the actual saved navigation history.
The next run retained an incorrectly auto-typed `/firrst` URL, showing that the
archive faithfully preserved the page that was actually opened. The focused
fixture now prepares an exact URL with the existing native Paste helper and
asserts it before navigation. Ordering shortcuts and menu actions still use real
keyboard and native menu events; no order or preservation assertion is removed.

The following run passed the saved order and selection assertions, then looked
for the old startup window after restoration had replaced it. The fixture now
checks the freshly created core window, waits for its actual selected page and
asserts that the startup window has closed. Runtime identities are intentionally
not restored from the archive.

An earlier focused run passed all four placement XCTest methods and the native
keyboard/menu/normal-restart UI method: five passes, zero failures and zero skips
in 81.070 seconds. Its bundle is
`frontend/macos/.build/results-20261006-231139.xcresult`; attachment export and
the exact owned invocation reached terminal completion with exit 0.

That candidate's static gates passed 1,004 focused engine/IPC methods across eight
suites (zero failures/ignored), `cargo fmt --all -- --check`, strict workspace
Clippy and the all-targets workspace build. They froze 1,803 inputs with no
changes, matching the latest focused native run:
`808f53607447fae739f5f1e1710680fd1bcdef5f430e1e6045f8792ed82d5439`.

The first complete native invocation passed 196 methods, failed two existing UI
methods and retained one existing physical Zhuyin skip. All new placement methods
passed. The print case failed before opening the panel: Select All/Paste appended
to the initial editor text. The language-switch case displayed the restored
Chinese control but its final owner-preference file assertion read English.
Neither original method nor its assertions changed; three repetitions of each
passed (six passes, no failures) on the same frozen sources. The initial full
invocation is retained as a failed attempt, not accepted evidence.

Inspecting its successful drag screenshot exposed a persistent blue insertion
line after the drop. A native screenshot regression passed its pre-drag check,
then failed with 22 accent-color samples after completed placement. The drag
registry now publishes cancellation/consumption so every target clears its
insertion indicator when the owned drag ends. The regression retains the actual
native gesture and the original page/editor/find/zoom/address checks.

One indicator-fix invocation used the nonexistent `BrowserTests` target and
stopped with exit 70 before any tests. The corrected invocation selects the
existing `ProtocolTests` target. This command error is retained in the receipt.

The corrected indicator-fix invocation passed all eight new native methods (four
XCTest and four actual-window UI methods), with zero failures/skips in 255.943
seconds. It completed attachment export and the owned wrapper with exit 0. The
drag screenshot was inspected and its insertion line is absent. Its bundle is
`frontend/macos/.build/results-20261007-002124.xcresult`. This updated candidate
freezes 1,803 inputs at
`3b7a40364883c19c84ee31be7d04894416121f71d1e43179b8c0b62a4503bcd8`.

The updated candidate also passed 1,004 focused engine/IPC methods across eight
suites (zero failures/ignored), formatting, strict workspace Clippy and the
all-targets workspace build. All four gates reached terminal exit 0 on this same
source aggregate, with no input changes.

The second complete native invocation reached terminal exit 65 after 3,203.633
seconds: 197 passes, one failure and one existing physical Zhuyin skip, with no
source changes. It again failed the final owner-preference
file assertion in the language-switch method (`en` rather than `zh-Hant`), while
the restored Chinese control passed. Its original assertions remain intact.
The earlier print cancellation method passed this time without changing it.
All four new actual-window placement methods passed, including disappearance of
the insertion indicator. This complete invocation remains failed evidence;
language persistence requires further investigation before acceptance.

Six unchanged language repetitions with an external read-only observer passed
in 374.686 seconds. The observations confirm that relaunch removes the forced
English argument and that the owner plist can update after the visible controls;
its writes occur on delayed batches. An isolated Foundation/Core Foundation
probe then seeded English, set Chinese and inspected the actual owner file.
Both `UserDefaults.synchronize()` and `CFPreferencesAppSynchronize()` returned
success with Chinese in the defaults database while the plist still contained
English. The later plist update is independent of the browser process.

The UI fixture had treated that backing-file read as immediately synchronous.
[Foundation documents asynchronous disk writes](https://developer.apple.com/documentation/foundation/userdefaults).
The fixture now checks the same owner file with a predicate expectation under
the existing 15-second limit, followed by the unchanged exact `zh-Hant`
assertion. The fresh-process Chinese control and no-page-refetch checks are
unchanged. A missing or incorrect saved value still fails. No product preference
API, timing, ownership or permission behavior was changed for this correction.

Three language/file repetitions passed in 166.504 seconds on 1,803 inputs at
`3888905878386544c621ffc134e1cd1a19f94be360d1793d214d650973a8a4ca`.
The same candidate passed all four static gates. Its complete native invocation
was then intentionally superseded after 819.363 seconds (144 methods passed;
exit 75 after SIGINT to the verified owned xcodebuild child, no source changes).
Code review identified a delayed-provider reference-order boundary requiring a
regression and fix before final acceptance. The interruption and exact parent/
child identities are retained; unrelated processes were not signalled.

The new real-core regression uses actual owned token bytes and keeps all tabs
live, then moves the reference before completing the provider. The old drop
changed `[2,3,1]` into `[3,2,1]` using its obsolete after-reference neighbor.
The existing delayed-source method also exposed its authenticated rejected drop
leaving insertion state active. That invocation failed two methods and passed
three. The registry now authenticates the current token before re-resolving and
comparing the entire placement; authenticated stale placement cancels the drag,
while forgery or an old token cannot cancel a newer valid drag. The next focused
invocation passed five XCTest and four actual-window UI methods (nine passes,
zero failures/skips in 235.302 seconds).

A further public validity assertion failed when the reference and neighbor
remained live in the same group but reversed order. Shared placement validity
now checks the current after-reference neighbor, covering queued menu placement
as well as dragging. Before-reference placement still names the reference itself
and remains valid when that reference moves. The boundary was then included in
the final focused and complete acceptance runs above.

The final focused invocation passed all five placement XCTest and four actual
window UI methods (nine passes, no failures/skips in 239.209 seconds), completed
automatic attachment export and exited 0. Its bundle is
`frontend/macos/.build/results-20261007-020234.xcresult`. The candidate freezes
1,803 inputs at
`07aebf0a0393d06e218bffb93e446ed2e577a8568a2dd583600cedde3fde91d8`.
All 1,004 focused engine/IPC methods (eight suites, no failures/ignored),
formatting, strict workspace Clippy and all-targets build passed on that same
aggregate without source changes. Complete native/workspace and final audits
subsequently passed on that exact aggregate, as recorded above. Failed and
superseded attempts remain in the text receipt; result bundles and PNGs remain
ignored.
