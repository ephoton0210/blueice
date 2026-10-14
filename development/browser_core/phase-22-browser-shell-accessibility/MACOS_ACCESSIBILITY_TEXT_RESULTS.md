# macOS native Accessibility text controls — 2026-10-05

Accepted on Apple Silicon macOS 26.6.2 (25G83), Xcode 27.0 (27A266a),
Swift 6.4 and Rust 1.96.0. The complete native suite passed **166 cases with
0 failures and 1 physical Zhuyin skip**: 100 XCTest and 66 XCUITest passes.
The Rust workspace passed **7,233 cases, 0 failures and 69 ignored** across
471 suites. Workspace all-targets build, strict Clippy and formatting exited 0.
Eight strict bundle/service signature checks passed, and the owned parent,
private-panel and UI-runner executable prefixes contained no remaining processes.
Parent and private-panel executables are universal x86_64/arm64; actual execution
acceptance is arm64.

The native result is `frontend/macos/.build/results-20261005-132454.xcresult`.
Its 53 runtime warnings all concern internal QoS priority inversions; 0 concern
SwiftUI view updates. No exclusions, new skips, warning suppression or automatic
retries were introduced. Focused passes overlap the full gates and are not
added to their totals. Commands, scope definitions and file hashes are recorded in
[macos-accessibility-text-results.txt](artifacts/macos-accessibility-text-results.txt).
The final complete native run passed on its first execution after the final
source freeze. Earlier failed or interrupted attempts below remain separate.

Native text inputs and textarea expose UTF-16 character counts, selected/visible
ranges, selected text, insertion line, visual-line ranges, extended composed
character ranges, substring/attributed/RTF queries and clipped screen bounds.
Point queries use the same core geometry as pixels and caret rendering.
Hard newline line ranges include their separator; invalid indices return absent
ranges. Surrogate-splitting and overflow ranges are rejected rather than rounded.
The attributed string supplies the computed foreground color; rich font and
mixed document style attributes remain pending.

AXValue and selected-text writes use the ordinary native editor and record
atomic Undo transactions. Read-only controls permit selection but reject edits
before changing focus; disabled/unsupported controls remain unavailable.
Protected controls permit ordinary edits while returning no password plaintext
in values, selected text, substrings, attributed strings or RTF. Invalid version,
source, document, frame, node, context and window identities cannot mutate a
different control. Oversize replacement validation also completes before focus
or marked-composition changes. A marked composition is committed before an
accepted AX edit; successive Undo steps recover the composition and prior text.

AppKit's synchronous callbacks use a private broker connection with a two-second
whole-exchange deadline and independent request IDs. The normal browser reader
continues to own frame/viewport/preference notifications, including notifications
sent before an AX mutation reply. No mutation is automatically retried after
timeout. Read requests preserve focus, selection and rendered pixels. Requests
require the current input/representation frame; transient refresh gaps fail soft.
The core implements all text and layout work; AppKit only maps CSS document
coordinates to/from the native view and screen.

Inner horizontal scrolling and textarea vertical scrolling persist across
relayout and nonfocused queries. Range scrolling preserves focus and selection;
focused selection/editing reveals the caret. Reset and document replacement
clear transient offsets. Native geometry uses complete core text rather than
the bounded caret list used by ordinary input-state presentation.

Regression development first demonstrated the missing public text IPC. Public
socket tests cover emoji, combining marks, family graphemes, line/point/bounds
queries, privacy, readonly/disabled controls, stale and foreign ownership,
composition/Undo and visible pixel changes when revealing long ranges. MCP
navigation ignores unrelated AX broadcasts rather than accepting them as its
completion reply. Native XCTest invokes real NSAccessibility methods with
bundled core services and actual NSWindows. XCUITest reads OS-visible native
elements and exercises Unicode editing, long-control scrolling, Undo and reload.
The new native fixtures wait for the attached window's viewport dimensions and
matching frame/input state before parameterized queries.

The first complete native run passed the new AX cases but exposed a print
integration failure. AX broadcasts accumulated on the idle print socket while
the panel waited, causing the broker to prune that reader. Final page validation
failed, and AppKit displayed "No pages from the document were selected to be
printed," keeping the operation and Print command busy. A real-core regression
reproduced the closed connection after 1,000 AX queries and a panel wait.
Print exchanges now handshake on a connection limited to that exchange; AX
connections likewise close before returning from their synchronous callback.
Captured print documents remain bound to their original ownership and ticket.
The native shell also uses AppKit's asynchronous document-modal completion
callback and associates preview settings with the owning operation.
UI coverage opens a second print
job after saving and cancels it; the existing PDF page count, A4 landscape size,
80% scale, print-media pixels and retained-document assertions remain intact.
UI queries now identify the actual print sheet by its PDF menu button. The
preview regression also covers AppKit clearing the thread-local operation.

The next full attempt exposed two existing fixture synchronization gaps: the
file-input test prepared a second selection before its new representation
arrived, and the multiple-select test read a temporarily absent representation
after input acknowledgement. That unsuccessful attempt was stopped. Both tests
now wait for matching frame/input/representation generations and the expected
file basename or POST button before continuing, preserving all functional
assertions. Their focused run passed before final input freeze.
Another full attempt passed all 100 XCTest cases but failed the assistant
Loopback editor's model-field lookup. Its UI flow now asserts the native backend
selection value and exposes the editor scroll view by a stable identifier;
field editing scrolls to a hittable target before typing. The complete existing
settings review/cancel/reopen/apply assertions passed in the focused UI run.
That unsuccessful full attempt was stopped and is not counted as acceptance.
The subsequent complete run passed all 100 XCTest and 65 UI cases but failed
the readonly Undo-menu assertion immediately after a queued password Redo.
The UI case now waits for the native Redo entry to be consumed and confirms
the readonly element's OS-visible keyboard focus before checking Undo/Redo.
Its password privacy, clipboard, readonly and address-responder assertions
passed in the focused run. The earlier failed complete run is retained
separately, with no failure counted as successful acceptance.
The next complete run passed 165 cases but failed while saving the new profile
before session restoration began. The existing SecurityAgent Local Network
window covered the profile sheet's Save button. The UI case now drags only the
owned second BlueIce window to an unobstructed position before opening that
sheet. The focused session case passed with all window/profile/group/history/
zoom and unsaved-form privacy assertions intact; the system prompt remains
untouched. That earlier failed complete run is retained separately.
The following attempt was stopped after the download case's Show in Finder
button was likewise covered by the same SecurityAgent prompt. The download
case now positions its owned browser window before opening the download sheet,
including after relaunch. It shares the explicit native title-bar drag helper
with the session case; neither case acts on the system prompt or changes TCC.
The first focused positioning run reached Finder/open and transfer controls but
showed that the wider window's Done button still overlapped the prompt. The
helper also resizes a wider owned window to 900 points using its native corner,
and the download case waits for its sheet to close before closing the window.
All download bytes, pause/resume/cancel, Finder/open, policy and persistence
assertions remain in place. This interrupted attempt is not acceptance.
The next complete run passed 165 cases, including the repaired download and
session cases, but failed the pointer-reset case. The exported synthesized
event shows that a normalized coordinate on the disabled reset element resolved
to the window center (960, 489), inside the existing SecurityAgent prompt. The
following enabled reset click left the values and input focus unchanged. The
case now asserts the disabled state and nonempty control frame and clicks that
frame's center using
the enabled BlueIce window as its coordinate anchor. It retains all reset,
other-form, tab-isolation, reload/default and network-request assertions.
The failed complete run is retained separately and is not acceptance.
The corrected focused pointer-reset case passed before the final source freeze.

Rust acceptance uses **1,731 frozen files** covering non-macOS inputs and the
signed Rust test runner, all unchanged after the later Swift/UI repairs.
Its aggregate SHA-256 is
`bcf2eeae8589510c9d4965e4789f489baf81a62d9cbf33e271ef19d49a2fc5fb`.
Final native acceptance separately freezes **1,788 source/build/fixture inputs**,
all unchanged across that gate, with aggregate SHA-256
`f2ce638f3a2566416228bfde65f5f7a597311389e7920b2e20e16cfb1821756a`.
The distinct scopes and their freeze times are recorded in the receipt; the
earlier failed native runs are not counted as successful gates.

Document paragraph selection/copy, live regions, rotors, rich font attributes,
physical VoiceOver, physical OS IME and the other browser delivery requirements
remain pending in [MACOS_DELIVERY_PLAN.md](MACOS_DELIVERY_PLAN.md). This increment
does not claim complete browser delivery, new line coverage, distribution
signing/notarization or fresh Linux/Windows acceptance. Screenshots and xcresult
remain ignored. Existing Local Network prompts are retained, and TCC permissions
are not changed by this workflow.

The unedited window attachment `artifacts/macos-accessibility-text.png` was
inspected and remains ignored. SHA-256:
`e5a428673411d3b392f54597b58286f855c6dffca59f5d6fb13ffea1cde7c2d2`.
It shows the Unicode field, the revealed end of a long input, the focused
textarea scrolled to its end, the readonly value and password bullets. The
family-emoji sequence uses the current core's fallback glyph placeholders;
complete glyph fallback and shaping are not claimed by these text API tests.
The attachment captures BlueIce's window. The host's existing Local Network
prompt is retained, and no TCC permissions were changed.
