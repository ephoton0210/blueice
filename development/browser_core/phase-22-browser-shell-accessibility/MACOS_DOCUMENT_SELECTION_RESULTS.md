# macOS document text selection — 2026-10-06

Implementation, complete native acceptance and Rust workspace acceptance are
complete. See [the contract](MACOS_DOCUMENT_SELECTION_CONTRACT.md) and
[the validation receipt](artifacts/macos-document-selection-results.txt).

Core indexes public rendered text in document order, retains per-page UTF-16
selection, paints it and supplies grapheme, visual-line and range geometry.
Inline words, Unicode and paragraph boundaries share the same text for pointer
selection, keyboard movement, Select All, Copy and read-only AX callbacks.
Native controls, protected/private content and hidden/inert/AX-hidden subtrees
are excluded. Copy does not read the clipboard. Document selection does not
authorize Cut, Paste, composition, Undo/Redo, control activation or form submission.

AppKit exposes document selection and read-only text ranges, ordered clipboard
commands and document context-menu Copy/Select All. Link drag selects text;
completed clicks use the existing reviewed navigation path, with source,
document and focus ownership checked by core. The operation queue waits for
current input and display geometry, and rejects a changed CSS viewport. Frame
gaps suspend geometry reads without cancelling an otherwise current gesture.
AX selection explicitly refreshes the native input state before later commands.

Unchanged text/source identities retain selection across layout and scroll.
Privacy changes, replaced text sources and navigation discard it. Tabs retain
independent state and old AX/window ownership fails closed. Collection is limited
to 2,097,152 UTF-16 units/visited fragments, 4 MiB encoded text, depth 256 and
65,536 paint rectangles; clipping is reported. Existing control limits remain.

## Development evidence

The initial core boundary run compiled and failed all 12 new cases. Implementing
the core passed those 12. Extended regression runs exposed source replacement
with identical text retaining selection; the index now retains corresponding
DOM text identities. Two test API/import compile errors were corrected. A later
form fixture incorrectly included a submit button while expecting the no-submit
implicit-form path; it now uses an ordinary button. The NativeClick fixture now
obtains current focus after a reviewed activation before its next valid click.
All earlier failures remain in their distinct logs.

The final focused candidate passed 775 engine unit tests, 202 IPC unit tests and
seven actual-session context-menu/selection/click IPC tests. These overlap the
eventual workspace suite and must not be added to its total.

The initial native baseline compiled and failed the three new tests. The first
bridge run passed five cases and failed link activation after reload. The next
run passed that UI flow but failed the AppKit isolation case when the local text
state/tree had not caught up after AX selection. Explicit input refresh and
correlated test refresh fixed that case. The subsequent three new native tests
passed with no failure or skip; its source snapshot precedes the final geometry
and select-control guards and is not full-candidate acceptance.

The first strict Clippy gate found that adding an inline document menu state made
the reference frontend's event enum too large. Boxing the optional state keeps
the wire format unchanged. The second gate passed formatting, strict Clippy,
the all-targets build and the 984 focused Rust cases without source changes.

The first complete native candidate ran 182 methods: 180 passed, one failed,
and the existing physical Zhuyin case skipped. The new document UI flow passed.
The unchanged protected/readonly UI case failed because the readonly element
had no keyboard focus after a click. Its video shows the password field retaining
focus and receiving Select All. This result is not acceptance. A real-service
regression reproduced a mouse click dropped during the frame/viewport reply gap.
The viewport now retains the original down/drag/up events for at most two seconds
and dispatches them once current geometry and semantics return. Owner, tab,
document epoch/generation, view size and first responder must still match.
Changing tabs, navigating or resizing cancels the pending events; successive
drag events are coalesced and the event buffer is bounded to 64 entries.
The unchanged failing UI case, document-selection UI flow and new real-service
gap test then passed together. A second focused run passed gap, resize/tab
cancellation and ordered clipboard/privacy cases. No UI case was weakened or
skipped, and no host trust setting was changed.

The next unfiltered native invocation exposed a stale-frame AX read in the new
AppKit document case. It was interrupted after that known failure, preserving
its bundle and terminal exit 73; it is incomplete and not acceptance. The test
had accepted an earlier locally current frame while attaching its NSView had
queued a backing-scale resize. Its refresh condition now also requires core
viewport dimensions/backing scale to match the actual native view. Exact-frame
core rejection and production deadlines remain unchanged. The complete native
editing and Accessibility groups then passed all 58 cases together.

The following complete native candidate passed 182 methods, failed one existing
Undo/Redo window-transfer method and retained the one physical Zhuyin skip.
All document, readonly and AX cases passed. The failed Redo used a native key
while the preceding pointer focus had cleared local input state but its core
acknowledgement had not arrived; the video shows the focus outline arriving
after the key. A new actual-service regression reproduced the lost key.
The native viewport now buffers page-input keys through pointer-focus
acknowledgement, retaining the existing owner/tab/document/size fences. Global
browser shortcuts retain normal window dispatch. Original mouse events and
following keys share the bounded deferred buffer during the geometry gap;
another red regression verified that Select All previously vanished there.
Cancelled gestures discard their queued keys, and expired transitions do not
trap later keyboard input. The first focused correction passed all six selected
actual-service/UI cases; the subsequent complete editing/AX groups and selected
UI rerun passed all 62 cases (59 XCTest and three XCUITest), with no failure or
skip. Original complete-run failures remain preserved; this focused result is
not a substitute for full acceptance.

Another unfiltered attempt exposed an existing form-reset test reading a nil
representation immediately after a blocked navigation notice. It was interrupted
and ended with exit 73; Xcode also reported an unfinished action log while saving
that interrupted bundle. The complete attempt's console log is retained. The
test now separately waits for current semantics/viewport availability before its
native Reset action. Policy denial and the original reset assertions remain.
The complete XCTest target then passed all 115 methods without failure or skip.

The final complete native invocation passed **184 methods, zero failures and one
existing physical Zhuyin skip** (115 XCTest and 69 XCUITest passes). It took
3230.134 seconds and its owned process and attachment export reached terminal
completion before result inspection. Bundle:
`frontend/macos/.build/results-20261006-164025.xcresult`. Its summary contains
56 internal QoS warnings and no view-update warning. Eight strict signatures,
universal x86_64/arm64 parent/private-panel executables and zero owned native
processes passed. Execution was arm64.

The native gate and final static gates froze 1795 inputs with SHA-256 aggregate
`fc85c17984e30dac306e4fe94ff323665fc3729e2b4ce831fb68270f649e110a`;
no input changed during either run. The screenshot was inspected and remains
ignored at `artifacts/macos-document-selection.png`, SHA-256
`ead6cb806208ed7b2220e8ea9aaced171e8a7297cca2bec6e56e19b41dd6bf48`.
It shows actual core selection paint across paragraphs, an ordinary control
excluded from document Copy, a masked password and visually painted aria-hidden
fixture text excluded from document Copy/AX. It is not font-quality or physical
screen-reader acceptance.

The complete Rust workspace rerun passed **7302 cases, zero failures and 69
ignored across 473 suites**, including all five original owner-lifetime cases.
`cargo test --workspace --no-fail-fast` used Rust 1.96.0, the ordinary macOS
signing runner and the existing shared target cache. It ran from
2026-10-06 10:45:29 UTC to 11:42:54 UTC (3444.913 seconds), without filtering,
additional helpers or changed source inputs. Final formatting, strict Clippy
and the workspace all-targets build passed. The final audit passed eight strict
signatures, universal x86_64/arm64 parent/private-panel executables, zero owned
native processes, unchanged source inputs and unchanged hashes for all eight
accepted native executables.

The first workspace invocation ended after 421 suites with 6791 passes, four
launcher owner-lifetime startup/exit timeouts and 66 ignored. Its source snapshot
remained unchanged. The original five owner-lifetime cases then passed together
with the normal signing runner, original parallelism and unchanged deadlines.
Cargo rebuilt dependencies during that focused invocation, so this does not
establish unchanged executable bytes or the cause of the earlier timeouts.
No auxiliary readiness helper, host trust change or deadline extension was used.
All eight accepted native executable hashes remained unchanged afterward.
The subsequent complete workspace invocation passed as recorded above; these
startup failures remain part of the acceptance record rather than a claim of
pristine cold-start reliability. The receipt preserves the original failed run,
focused diagnostic, complete rerun, native attempts, static gates, product hashes
and the full accepted input hash manifest.

Physical VoiceOver, physical Zhuyin, complete bidirectional shaping and remaining
browser requirements stay in the [delivery plan](MACOS_DELIVERY_PLAN.md). No host
trust/TCC/security prompt was changed, and this work makes no new Linux/Windows
acceptance claim.
