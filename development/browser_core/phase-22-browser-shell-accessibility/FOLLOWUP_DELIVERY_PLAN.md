# Cross-platform, drag interoperability and Web API follow-up

Started 2026-10-12 from `67061e667`. The requested objective includes all four
tracks below. Completing one software increment does not complete this plan.
Evidence must identify its source snapshot, actual commands, runtime platform,
case scope, failures/skips and owned-process cleanup.

| Track | Required outcome | Current state |
| --- | --- | --- |
| Windows acceptance | Same source snapshot; complete Rust workspace, strict Clippy, all-target build, full formatter scope, WinUI protocol tests and actual logged-in desktop UI/pixels/cleanup | The owner's dedicated 320 GiB test disk on the Linux 2 TB `/data` volume is attached live/persistently as Windows B: `BlueIceTests320`. BlueIce evidence and tool caches are copied; 2,077 archived inputs and 1,181 toolchain files pass SHA256 checks. Subsequent workspaces, build products, tool caches and process TEMP/TMP use `B:\BlueIce`. C: now has 6.75 GiB free, passing the original 4 GiB guard after verified toolchain relocation with a compatible old-path junction. Six file-free temporary trees were removed. No VM reboot or pagefile-policy change occurred. Frozen `83436f846` full Windows acceptance now passes: 5,670 Rust cases, zero failures/64 ignored, strict Clippy, all-target build/formatter, WinUI build, five protocol cases and seven actual desktop cases. Task/bootstrap exit zero, frozen source hashes match and owned runtime cleanup passes; see [complete receipt](artifacts/windows-83436f846-results.json) and [storage delivery](WINDOWS_TEST_STORAGE.md). Integrated `ec4217440` complete Rust execution ended with 5,764 passed, two GC failures and 64 ignored; unchanged 2,186-source/log hashes and idle cleanup are verified. Corrected `504eeb924` now passes fresh complete Windows acceptance: 5,770 Rust cases, zero failures/64 ignored across 516 groups, full formatter/strict Clippy/all-target build, WinUI build, five actual-Core protocol cases and seven desktop cases. Task/bootstrap exit zero; 2,192 source files, six gate logs and 56 formatter/UI artifacts match, and the task/owned runtime are gone. See [current receipt](artifacts/windows-504eeb924-results.json). The B: 60 GiB target cap and B: 50 GiB/C: 4 GiB free-space guards remain enforced. See [failed full gate](artifacts/windows-ec4217440-failed-results.json). |
| Drag interoperability | Actual OS drags between live windows and an independent application; file export/promises, default/custom drag images, `setDragImage` and modifier-selected operations | [Related native acceptance](MACOS_DRAG_INTEROP_RESULTS.md) now passes 29 cases, zero failures/skips, including actual cross-window/external text, binary file-promise export, Copy/Move/Link, Core-backed custom images and stationary-source cancellation. Current `504eeb924` also passes all nine actual OS drag methods and its scoped retained recording shows two moving 100x48 Core images; see [current drag receipt](artifacts/macos-drag-504eeb924-results.json). Its complete native gate retains one unrelated assistant review-click failure and one actual IME skip. Corrected full native acceptance and broader promise acceptance remain required. |
| Physical system acceptance | Actual CJK/Zhuyin composition/commit/cancel, VoiceOver navigation/action and physical monitor/printer transitions | The completed native gate retains one real Zhuyin skip for missing Runner Accessibility permission. Direct CoreGraphics/NSScreen inspection identifies one online PA279CRV, 3840x2160 pixels at 2x/1920x1080 points, with 101 modes. The actual PA279CRV mode89→159→89 transition now passes: real owned Core backing scale2→1→2 and frame generation4→5→6 preserve the document/frame source, and normal app/runtime/preferences cleanup plus physical mode restoration pass. See [actual display result](artifacts/physical-display-transition-20261013-results.json) and [cleanup](artifacts/physical-display-cleanup-20261013-results.json). This proves one physical monitor mode change; a second monitor transfer remains unverified. Unsandboxed CUPS reports no configured/default printer. Actual system VoiceOver was enabled through its standard shortcut and startup UI, but cursor/last-phrase AppleEvents fail with -1728 then time out(-1712); navigation/action/speech remain unaccepted. VoiceOver was restored off and its process exited. See [actual attempt](artifacts/physical-voiceover-attempt-20261013-results.json). Actual Runner grant and printer connection information have been requested. See [capability evidence](artifacts/physical-capabilities-20261013-results.json). |
| Web platform APIs | Actual FileReader lifecycle and data reads; actual stream objects and readers; origin/document-scoped object URLs; broader DOM/event behavior and accurate host typings | FileReader, EventTarget, ProgressEvent and drag/event interfaces pass 58 VM cases plus five typing cases. Readable streams, default/BYOB readers and Blob byte/text streams pass 20 stream cases and six host-profile cases. Actual Core/BlueJS document, selected-byte and stream DOM integration passes, as does failed-task acknowledgement containment without script replay. The affected BlueTS crate passes all 354 cases with 60 optional oracle cases ignored. The [object URL increment](OBJECT_URL_RESULTS.md) adds actual retained bytes, document/context fencing, staged IPC, URL/BlueTS bindings and local Core navigation with preserved script origin. The [abort increment](ABORT_SIGNAL_RESULTS.md) adds real signal reasons, weak dependent graphs, timeout tasks and cancellation of native/generic listeners, with an 88-case final cohort and actual selected-byte integration. The [CustomEvent increment](CUSTOM_EVENT_RESULTS.md) adds branded arbitrary detail, dispatch-safe legacy initialization, GC retention and an additive 37-binding profile; 102 VM/type cases and actual Core/child DOM integration pass. The [node-event increment](NODE_EVENT_RESULTS.md) adds Core-backed arbitrary node dispatch, capture/target/bubble, trusted native base events and properly scoped nested named callbacks; 70 VM/type/compiler cases, nine Core/IPC cases and actual Core/child integration pass. The [tee increment](STREAM_TEE_RESULTS.md) adds two actual branches, independent byte/BYOB data, cancellation/error propagation and collectible Promise state; 1,093 BlueJS and 146 bridge cases pass. The [iteration increment](STREAM_ITERATION_RESULTS.md) adds serialized native values/next/return, real for-await cleanup, keyword-method typing/lowering and a consistent 40-binding classic/module profile; 1,110 BlueJS, 148 bridge and 357 compiler cases pass, with 60 optional compiler oracle cases ignored. Native/full/cross-platform gates, writable/transform/pipe streams and conformance, broader URL/subresource behavior and DOM/event work remain required. |

## Physical input and VoiceOver preflight

The 2026-10-13 read-only input-source probe confirms that
`com.apple.inputmethod.TCIM.Zhuyin` is installed, enabled and selectable;
the current input source is ABC. This establishes availability, not physical
composition/commit/cancel acceptance. See the
[TIS result](artifacts/physical-input-source-availability-20261013.json) and
[retained probe](artifacts/physical-input-source-probe.swift).

Read-only inspection of the installed VoiceOver Utility finds its explicit
AppleScript enablement-marker check. The marker is currently absent. Subsequent
native AX inspection of the actual General page confirms the allow-AppleScript
checkbox is unchecked; its startup welcome checkbox is checked. This is
stronger evidence than interpreting the earlier
AppleEvent timeout alone, but does not establish successful VoiceOver use.
See the [installed-system result](artifacts/voiceover-script-control-readonly-20261013.json)
and [Apple's macOS 26 General settings](https://support.apple.com/guide/voiceover/general-category-cpvougen/10/mac/26).
After complete native UI cleanup, the next physical attempt can use documented
VoiceOver keyboard navigation/actions and its
[last spoken phrase command](https://support.apple.com/guide/voiceover/vo2725/mac),
with exact owned-application guards and preservation of the clipboard and
initial reader/input-source state. No system permission or scripting gate was
changed by this inspection.

The latest physical keyboard attempt reaches a stable actual reader with the
owned BlueIce app in the foreground. Its standard phrase-copy driver exits zero
but leaves the clipboard unchanged; no navigation, action or speech acceptance
is inferred. Reader-off/process absence, the original ABC input source, clipboard
and owned app/runtime/preferences/fixture cleanup are verified in the
[failed phrase-copy receipt](artifacts/physical-voiceover-phrase-copy-failed-results.json).
Read-only inspection of the actual system Utility Commands page then confirms
the `AXMenuButton` title `⌃⌥ 或 ⇪`: Control–Option or Caps Lock. The one newly
owned Utility is normally terminated and its post-cleanup application list is
empty. No preferences or authorization are changed. See the
[actual modifier receipt](artifacts/voiceover-modifier-readonly-20261013-results.json)
and [retained AX controls](artifacts/voiceover-commands-readonly-20261013.txt).
These results narrow the unresolved keyboard-observation failure; they do not
establish its cause or successful VoiceOver use.

## Drag acceptance

Destination keyboard focus must be independent from an explicit OS drag. A
visible inactive window can negotiate and receive a drop while context/window/
tab, source, document and displayed-frame identities remain enforced. Hidden,
foreign and modal destinations refuse the operation. Source invalidation still
revokes delayed payload publication.

Actual cross-window tests must assert the destination Core value, preserved
source document, Undo/Redo, no additional fetch and usability after closing the
source window. An independent AppKit fixture must exchange real OS drags in
both directions; it must not import BlueIce or emulate Core responses.

Subsequent file-promise work must cover exact binary bytes and Unicode names,
receiver errors, cancellation before fulfillment, source document/window
replacement, delayed provider lifetime and publication ownership. Drag images
must come from Core-owned rendered state and retain hotspot/scale geometry.
Modifier negotiation must preserve allowed operations, listener overrides and
the final operation used to consume a move source exactly once.

## Web API acceptance

Use the [File API](https://w3c.github.io/FileAPI/),
[Streams Standard](https://streams.spec.whatwg.org/) and
[DOM Standard](https://dom.spec.whatwg.org/) as the behavior references.
FileReader tests must cover binary/text/data-URL reads, encoding, progress and
terminal event ordering, abort/reentrant reads, invalid state/receivers and GC.
Streams must have actual reader locking, Promise-based reads, close/error/cancel
behavior and backpressure; Blob streams must preserve exact bytes and support
the byte-stream reader model. Object URLs must resolve real retained Blob data,
enforce origin/context/document ownership and release it on revocation/lifecycle
cleanup. Synthetic events must remain untrusted and cannot create an OS file or
clipboard grant. Exposed BlueTS declarations must match installed APIs.

## Evidence and remaining gates

The drag-image follow-up adds a branded `setDragImage` VM path, immutable BlueTS
image-profile extension, private protocol version 49, Core selection/custom
preview geometry and masked AppKit pixel extraction. The latest focused Rust/typing
cohort passes 43 cases. A real-service native regression failed because opaque
child DOM handles were mistaken for raw Core node IDs. The child boundary now
resolves those handles in the exact live document; actual Core geometry, updated
frame generation and exact green bitmap bytes pass. Pixel masking, vertical order,
clipping and zoom have separate native regressions.

OS preview validation uses a separately built native reference source, exact
Core/bitmap hotspot assertions and the receiver's measured one-point observation
accuracy. Actual recordings are checked for two moving 100x48 green images within
the custom drag's logged time range. An earlier unbounded-time verifier accepted
window positioning and is explicitly disqualified; the scoped verifier accepts
the actual drag and rejects the earlier 128x24 generic-label recording. AVFoundation
decoder attempts failed on this host; task-local PyAV 15.1.0 software decoding
preserves the actual 3840x2160 video and accounts for the 1920x1080 point desktop.
No system Python/Homebrew dependency or security preference was changed.

The final related run selects exactly 21 protocol and eight actual OS
methods and passes all 29 with zero failures/skips. The log and XCResult summary
agree on the exact case set; 11 strict signature checks pass. The
[image receipt](artifacts/macos-drag-image-results.json) preserves the 1,853-input
snapshot, subsequent FileReader deltas, video/log/image hashes and scoped results.
Earlier narrower runs and the missing
bundle fixture-metadata failure are historical evidence. The retained-recording
harness now builds inside the live SFTP/update fixture wrappers so their private
paths reach the actual test bundles. Complete native/Rust and cross-platform
acceptance remain required.

EventTarget covers branded construction/dispatch, cancellation, capture/once/
passive, live removal, object callbacks, reentrant refusal and GC. Empty constructed
event types are accepted as required by DOM; four original missing-interface
failures and the corrected 32-case cohort are retained. The affected six-crate
all-target strict Clippy run passed before that final small dispatch correction;
it is not current full-workspace or FileReader acceptance. Eight FileReader
regressions failed with `ReferenceError("FileReader")` before implementation.
The first compiled candidate passes 21 drag cases then fails all four EventTarget
cases with `value is not a constructor`: its installer calls the ECMAScript-only
lazy global factory for the host EventTarget name and replaces the constructor.
The same forced-collection regression is red before repair and green afterward.
A later run exposes ProgressEvent's incorrect constructor inheritance; its
regression passes after using the installed Event constructor. The final
[55-case VM receipt](artifacts/file-reader-vm-results.json) covers actual bytes,
binary/data-URL reads, Big5/UTF-16 BOM/windows-1252, trusted progress events and
microtask order, error/abort/load chaining, handler replacement and pending-reader
GC lifetime. A separate bounded-turn regression covers 40 simultaneous reads
without treating a normal scheduler yield as failed I/O.

The private scheduler also leaves paused debugger realms untouched. The actual
Core/BlueJS integration regression now passes: reads started in an earlier script
observe later listener registration, and selected binary bytes reach the live
DOM after input/change. Its first attempt failed at Darwin's socket path-length
limit; the short unique fixture then reproduced missing browser task scheduling.
Core now flushes after all document scripts and after the complete selection
event sequence. A hostile/malformed task acknowledgement closes only that realm,
keeps Core alive and cannot replay document scripts on the next poll.

Three additional red-to-green VM cases establish inert noncallable handler-object
retention, false-return cancellation and MIME charset parsing with quoted
semicolons, invalid essences and first-parameter precedence. The new immutable
BlueTS profile preserves prior profile bytes and adds an exact 27-binding
inventory. Interface overload inheritance, bounded intersection collection and
specific event tags with checked string fallbacks are repaired rather than
removing FileReader's EventTarget inheritance. All ten member-call cases and the
complete BlueTS crate pass (354 passed, 60 optional oracle cases ignored).
The [integration receipt](artifacts/file-reader-integration-results.json) records
58 VM/five typing cases, the complete compiler results, actual Core integration,
task-failure containment, source hashes and later lint-only deltas. Strict Clippy
passed after its MIME Pattern and test-config style corrections;
fresh full Rust/native acceptance remains required.

The five affected crates completed strict all-target Clippy after both style
corrections. This is the FileReader snapshot; later stream code requires its own
checks. Readable-stream work now has twelve original missing-interface failures.
The first executable candidate passes eleven cases and fails cancellation during
forced GC because its internal no-op Promise callback was not rooted before
allocating the derived Promise. That callback is now rooted. Four additional
cases cover split UTF-8, single-flight asynchronous pulls, zero-length BYOB close
responses and size-algorithm errors. Candidate stream/reader/controller state is
collector-owned, with actual Promise jobs, byte queues, transferred BYOB buffers
and incremental Blob sources. The related cohort now passes 20 stream and 58
FileReader/event/drag VM cases, plus six profile cases. Actual Core/BlueJS
integration also passes: Blob.stream().getReader().read() returns exact bytes
that update the same live DOM. The new immutable stream-v4 profile has exactly
33 bindings and preserves reader-v3 and earlier profile bytes.

Three additional regressions reproduce missing BYOB buffer transfer, incorrect
cancelled-read values and missing synchronous partial-element close errors.
Their corrections pass. A subsequent default-reader regression shows
autoAllocateChunkSize must not split an explicitly enqueued chunk; its original
failure and corrected 20-case run remain recorded. DataView results and
respondWithNewView preserve the original view type. The
[stream receipt](artifacts/web-streams-results.json) records exact cases, logs,
current source inputs and the smaller Core snapshot's later deltas. Four-crate
all-target strict Clippy passed after its initial style corrections.
Full Streams conformance, remaining stream APIs, complete Rust/native and Windows
acceptance are not established by these focused results. A fresh Windows probe
at 16:27 still finds only 2.44 GiB free on C: and no validation process.

The inactive-window regression failed on the original production sources and
passed after the correction. Initial actual-window runs retained two fixture
failures: an unscoped address query with two browser windows, then a 3-point
position correction below AppKit's drag threshold. The address query is now
window-scoped and positioning retains its original exact-target assertion.
These failures are not successful cross-window acceptance.

Final native/Rust acceptance, source/product/signature audits and the four
requested tracks remain open until their actual results are recorded. Current
results are tracked separately from the accepted parent in `.build` and the
owned Windows validation metadata; earlier evidence must remain available.

## Precommit verification on 2026-10-12

The first complete candidate native run executed 310 methods: 302 passed,
seven failed and the physical Zhuyin method skipped for missing Runner
Accessibility authorization. Its fixture servers and owned processes cleaned
up. The complete Rust run records 7,505 passes, two failures and 69 ignored
cases across 495 groups. Both Rust failures are unchanged debugger-shape tests
still asserting private page-host version 48 after the drag-image increment
introduced version 49. Updating those exact assertions retains their shape
checks; all 205 IPC library methods then pass with real socket access.

The seven-method native repeat passes assistant-result accessibility, SFTP
relaunch and the unchanged Finder fixture, while reproducing document selection
and three tab drags. The assistant's selectable Text now has content identity,
preventing an old accessibility wrapper value from outliving updated visible
text. The SFTP fixture reapplies its existing unobstructed-window positioning
after relaunch, preserving configuration/privacy/no-resume checks. The failed
Finder recording retains the system Local Network dialog that interrupted its
gesture; its later successful repeat does not erase that failed record.

Read-only observations from the exact owned Core verify the collapsed selection
and the subsequent selected bytes. Temporary pointer-event attachments show
that the original `press(...thenDragTo:)` step delivered no mouse events after
the collapse. The four methods now use macOS `click(...thenDragTo:)`, preserving
coordinates, hold durations, text/privacy/no-fetch and placement checks. All four
pass without failures or skips. Temporary trace code is removed from the final
candidate. The corrected complete Rust run for committed `83436f846` has
finished successfully: 7,507 passed, zero failed and 69 ignored across 495 result
groups. All 1,246 frozen Rust/Cargo inputs are unchanged. The complete native
run has finished with 309 passed, zero failed and one physical Zhuyin skip.
All 1,863 frozen inputs are unchanged; the SFTP and update wrappers confirm
owned-process/fixture cleanup. Physical input-method/hardware acceptance is
still open.
Windows uses the independently hashed B: archive of that same committed source.
After the storage-scanner race correction, format, strict all-target Clippy,
complete all-target build and complete Rust execution have passed. Windows
reports 5,670 passed, zero failed and 64 ignored across 495 result groups. Its
WinUI build first selected the GNU toolchain through `cargo +1.96.0` and failed
to find `gcc.exe`; the build script now pins the accepted MSVC triple. The
separate WinUI follow-up verifies and reuses the same-source complete Rust
receipts, then builds/tests unchanged Rust/C# inputs with an explicit MSVC
helper. WinUI build, all five protocol cases and all seven logged-in desktop cases
now pass. The final task/bootstrap exit zero, sources stay unchanged and an
independent audit finds no owned task/helpers/apps. Earlier GNU selection,
unsupported close pattern and JSON-publication failures remain recorded in
the [complete receipt](artifacts/windows-83436f846-results.json). Later API
increments still require current-source complete gates.

## Integrated source gates on 2026-10-13

The `88c1f1438` macOS complete Rust gate finishes with 7,537 passed, zero
failed and 69 ignored across 503 result groups. All 1,882 frozen program
inputs match immediately after terminal exit zero; see the
[receipt](artifacts/macos-workspace-88c1f1438-results.json). Only afterward
was the main branch fast-forwarded to integrated `ec4217440`, preserving
the committed Windows build/close fixes and CustomEvent, node events, tee,
Promise GC and async stream iteration increments.

Fresh complete macOS Rust and native gates are running for `ec4217440`.
The native gate has already passed all 178 protocol methods but has four
UI failures involving a missing permissions window; the failure record
is retained and diagnosis is in progress. Complete native acceptance is
not established. The integrated Windows source passes full formatter
scope, strict Clippy and complete all-target build; its full Rust and
WinUI gates remain running. The larger target cap and retained first stop
are documented in [test storage](WINDOWS_TEST_STORAGE.md). Physical
Zhuyin/VoiceOver/monitor/printer acceptance remains open.

The current native permissions-window failure is diagnosed from the actual
XCTest session: Panels PID 40642 is foreground at the current temporary
checkout, while the bundle-ID-only test proxy selects the older main
checkout's app with PID 0. The
[diagnosis](artifacts/macos-panel-path-diagnosis-ec4217440.json) preserves
both paths and exact daemon state. Both test queries now select the Panels
URL beside the current runner products, matching the drag-reference fixture
pattern. Swift parsing and diff checks pass; focused and complete native
verification of this test-only correction remain pending. The product's
launcher-group and exact-bundle ownership checks are unchanged.

The complete current Windows Rust run also exposes an Array.fromAsync
iterator-close GC regression at a 376,704-byte heap ceiling. It is
reproduced on macOS and corrected in an isolated follow-up; all 48
related cases, strict BlueJS all-target Clippy and format checks pass.
The [GC follow-up](ARRAY_FROM_ASYNC_GC_RESULTS.md) preserves the existing
red regression, additional semantic coverage and pending complete gates.
The active pre-fix complete runs still contain that failure.

Both complete pre-fix Rust runs also fail four original Atomics.waitAsync
fixture/mode cases because the OS waiter did not keep its pending
Promise alive. The [async host-wait follow-up](ASYNC_HOST_WAIT_GC_RESULTS.md)
roots the owned wait through VM settlement and cleans every dequeued
event root on success or error. All 138 related cases and the four
original fixture/mode executions pass, as do strict Clippy and formatting.
The next complete acceptance must use this final correction rather than
the earlier `33dfdb3b7` package. Physical requirements remain open.

## Completed failure gates and corrected-source acceptance

The complete `ec4217440` native gate ends with 295 passed, 14 failed and
one physical Zhuyin skip. All 1,902 program inputs and eight backend
binaries match; XCResult/log totals agree and both live fixture wrappers
confirm cleanup, independently followed by process absence. Every failure
is in the exact permissions/assistant/recovery cohort whose bundle-ID-only
proxy selected an older checkout's app. The
[failed receipt](artifacts/macos-native-ec4217440-failed-results.json) is retained.

The `504eeb924` correction passes all 14 of those actual UI methods, with
zero failures/skips and unchanged source/backend hashes. Both wrappers
and owned process cleanup pass; see the
[focused receipt](artifacts/macos-panel-focused-504eeb924-results.json).
The complete `504eeb924` native gate now ends with 308 passed, one failed
and one physical Zhuyin skip across all 310 cases. All 1,902 inputs and eight
runtime binaries match, 11 strict signatures pass, and the wrappers/owned
processes are gone; see [terminal receipt](artifacts/macos-native-504eeb924-failed-results.json).
All nine actual OS drag methods pass, and the current recording passes the
scoped verifier with two moving 100x48 Core previews; see
[current drag evidence](artifacts/macos-drag-504eeb924-results.json).
The remaining assistant case passes when repeated alone with unchanged source;
its failed recording shows the exact valid URL/model and an offscreen Review
button below the scroll viewport. `12bc50772` adds bounded explicit scrolling,
full button visibility and post-input value checks while preserving every
consent/persistence assertion. Its 14-case related cohort now passes with
zero failures/skips, unchanged 1,902 inputs/eight runtime binaries, 11 strict
signatures and wrapper/process cleanup; see [focused result](artifacts/macos-assistant-review-focused-12bc50772-results.json).
The corrected complete 310-case native gate is now running after physical
fixture cleanup, with production runtime unchanged.

The complete Windows `ec4217440` Rust run ends with 5,764 passed, two
failed and 64 ignored across 516 result groups, stopping before WinUI.
All 2,186 original source files and four step-log hashes remain unchanged,
and no related runtime remains. The
[failed receipt](artifacts/windows-ec4217440-failed-results.json) records
Array.fromAsync iterator-close and Atomics.waitAsync host-root failures.
Both are corrected with red/green public-boundary evidence. Fresh
`504eeb924` complete Windows acceptance now passes: 5,770 Rust cases,
zero failures and 64 ignored across 516 groups; full format/strict Clippy/
all-target build, WinUI build, five protocol cases and seven actual desktop
cases all pass. Task/bootstrap exit zero. The final read-only audit verifies
2,192 source files, the B: corpus junction, six gate logs and 56 formatter/UI
artifacts; toolchains/persistent policies are unchanged and the owned task/
runtime are gone. See [current receipt](artifacts/windows-504eeb924-results.json).
Later commits only change the Unix launcher test, macOS UI test interactions
and documentation; production source remains identical. The B: 60 GiB target
cap and original free-space guards remain enforced.

The original `ec4217440` macOS full Rust gate ends with 7,603 passed,
three failed and 69 ignored across 516 result groups; all 1,902 frozen
program inputs match. In addition to the two corrected GC regressions,
the Unix launcher click fixture retained an obsolete dispatchEvent-absence
assertion. Its corrected callable-API guard passes all seven real
Core/child cases against the same immutable `504eeb924` runtime. See
the [fixture follow-up](LAUNCHER_NODE_EVENT_FIXTURE_RESULTS.md). Main
integration and canonical corrected-source complete macOS Rust checks
now follow this terminal boundary. Focused tests, ongoing gates, direct
hardware inventory and compiled helpers do not establish full completion.

The two original GC failures and three new host-wait regressions, including
four original Test262 fixture/mode executions, now pass in both actual
current complete Rust runs; see [cross-platform evidence](artifacts/cross-platform-gc-504eeb924-results.json).
The corrected macOS workspace run now completes all 516 result groups:
7,610 passed, zero failed and 69 ignored, including the seven Unix launcher
Core cases; all 1,902 completion input hashes match. The original tool session
handle was lost, so its direct OS exit code is explicitly unrecorded rather
than inferred. See [complete case results](artifacts/macos-workspace-1e198f07e-results.json).
Main is integrated through `1e4286068`; formatting and complete strict
workspace/all-target Clippy both exit zero on the unchanged Rust source.
Their direct exit codes and log hashes are retained in the
[complete check receipt](artifacts/macos-final-1e4286068-rust-checks-results.json).
The current native Zhuyin case actually skips at its Accessibility guard;
no physical keys were injected. Actual VoiceOver navigation/action/speech,
multi-monitor handoff and printer transitions remain open. The single physical
monitor mode transition and restoration are now accepted separately. Capability discovery and cancellation
checks for the prepared display helper are preparation rather than physical
acceptance.

## Current native and actual input follow-up on 2026-10-13

The full `12bc50772` native gate is now terminal with exit 65: 307 passed,
two failed and one actual Zhuyin Accessibility skip across all 310 methods.
XCResult and case logs agree; 1,902 source files, eight frozen backend binaries,
11 strict signatures, both fixture wrappers and owned-process absence are
verified. See the [full failed receipt](artifacts/macos-native-12bc50772-failed-results.json).
The remaining UI failures occur before the custom-image drag, when an exact
four-point window correction does not move, and during a language-picker click
whose center overlaps a system VoiceOver AppleScript-control dialog. Retained
failed-case movies and AX trees distinguish that dialog from earlier Local
Network notices. No permission change is claimed or required as a fixture fix.

The four-point positioning helper now includes the exact four-point boundary.
The existing OS `setDragImage` method first exercises that correction from the
observed frame and restores the original geometry. The candidate builds without
changing the fixed Rust binaries and passes its actual selected native method:
one passed, zero failures/skips, exit zero, with all original independent native
reference/custom-preview geometry and payload assertions. Source, signatures
and owned cleanup also pass; see the
[focused receipt](artifacts/macos-window-four-point-focused-results.json).
The related positioning cohort and a fresh complete native gate remain required.

Separate actual system Zhuyin testing now reaches the live Core editor with OS
keycodes after choosing the real native input-context source. Six observed
stages pass: ㄋ and ㄋㄧ retain marked ranges, the third tone produces 你,
Return commits it without a marked range, and the next syllable marks only the
suffix. Escape fails to restore the committed prefix and leaves 你ㄋㄧ unmarked;
full IME acceptance therefore remains false. The failed attempt restores ABC
and cleans up its app/runtime/preferences/fixture normally. See the
[actual input-context result](artifacts/physical-zhuyin-native-input-context-results.json).
The earlier GUI inspection timeout and global-TIS-only ASCII result are retained
as failed preparation/execution rather than IME acceptance.

The actual VoiceOver keyboard attempt observes a transient reader followed by a
late welcome dialog and stops before navigation, spoken-phrase copying or action.
Its original cleanup initially leaves that dialog active, which is explicitly
retained in the [attempt](artifacts/physical-voiceover-keyboard-navigation-results.json).
Normal use of the exact newly started system dialog's Turn Off VoiceOver button
then restores the initial disabled state with no reader/startup process remaining;
see [follow-up cleanup](artifacts/physical-voiceover-keyboard-navigation-followup-cleanup.json).
Stable startup, actual reader navigation/action/speech, successful IME cancel,
language-popup placement and physical printer/multi-monitor acceptance remain
open; helper compilation and scoped passes do not complete those requirements.

## Actual Zhuyin cancellation repair on 2026-10-13

The macOS viewport now cancels an active marked composition before forwarding
unmodified Escape to the system input context. Discarding that conversion session
does not adopt its phonetic suffix through AppKit callbacks. A scoped Escape
unmark callback also cancels rather than committing, while ordinary unmarkText
retains its existing finish/commit semantics. This is a Mac Swift change;
production Rust and WinUI code are unchanged.

The separate owned native app now passes the actual system Zhuyin sequence with
OS keycodes: ㄋ, ㄋㄧ, third tone→你, Return commit, a second ㄋㄧ composition,
and Escape restoring only the committed 你. All seven observed Core states
preserve the document, frame source and editor identity. The original ABC input
source is restored; the app exits normally and its runtime, private preferences,
HTTP server and fixture are removed. See the
[actual OS result](artifacts/physical-zhuyin-escape-fixed-results.json).
This establishes physical IME composition/commit/cancel for the explicit
candidate frontend source hashes and unchanged Rust runtime; the older Runner
Accessibility skip is retained as historical evidence rather than relabeled.

Two public AppKit/Core cancellation and ordinary-unmark cases pass, followed by
all 38 NativeEditingTests with zero failures/skips and exit zero. All 1,902
program inputs, 11 strict signatures and owned-process absence are verified;
see [focused cases](artifacts/macos-ime-cancel-focused-results.json) and
[complete editing cohort](artifacts/macos-native-editing-cancel-results.json).
The generic key boundary already passed before the fix; the actual OS cancel
failure supplies the red evidence. An earlier proposed synthetic NSEvent/system
IME fixture failed to establish composition and is explicitly disqualified as
cancellation evidence. VoiceOver, the language-popup fixture, current drag and
fresh complete native acceptance retain their separate remaining gates.

## Current language fixture and OS drag acceptance on 2026-10-13

The runtime-language fixture now positions only its owned settings window before
choosing a language. Its left titlebar segment avoids the previously recorded
system dialog, and all three language choices wait for the actual menu item.
The original editor value, independent windows, private permission/assistant
panels, absent unconfirmed settings, persistence and normal restart assertions
remain. The complete language method and actual custom-preview method pass
two cases, zero failures/skips and exit zero; source, 11 strict signatures and
owned-process cleanup are verified in the
[language result](artifacts/macos-language-window-fixed-results.json).

All nine current-source actual OS drag methods now pass with zero failures/skips
and exit zero. They cover inactive-window and external AppKit text exchange,
exact binary file promises, Copy/Move/Link and readonly refusal, source/page
cancellation, hover lifecycle and actual Core custom-preview geometry/payload.
XCResult and logs agree on the exact nine methods; all 1,902 program inputs,
eight frozen backend binaries, 11 strict signatures, both owned fixture wrappers
and actual process absence are verified. See the
[current nine-method result](artifacts/macos-current-os-drag-nine-results.json).
The initial attempt omitted the required update-fixture wrapper and stopped five
methods at missing build-time metadata. Its actual four-pass/five-failure result
and cleanup remain in the [failed preparation](artifacts/macos-current-os-drag-fixture-failed-results.json);
the corrected run builds inside the live fixture wrappers on unchanged source.

The separately scoped current-source custom-drag recording also passes the
time-bounded pixel verifier: after the logged custom drag begins at 31.43s,
two moving green Core previews appear at 32.77/32.97s with measured 100x48 point
geometry (one sampled height is 49 within the existing three-point image bound).
The original recording remains 3840x2160 pixels on a 1920x1080 point desktop.
Video, log and [unaltered decoded frame](artifacts/macos-current-drag-preview.png)
hashes are retained in the nine-method receipt. Fresh complete native acceptance
now contains 311 methods after the public Escape-key regression; VoiceOver and
remaining physical hardware acceptance are still separate unfinished gates.

The complete native gate on committed `2cf93cf56` ends with 308 passed, two
failures and one actual Zhuyin Runner-permission skip, exit65. All nine actual OS
drag methods and the full runtime-language method pass within this integration
run. Both failures occur in `tearDownWithError`: an owned Settings window remains
over the browser close control, so normal close never reaches the required
app/service-stop assertion. The session-recovery and Traditional Chinese method
body assertions pass. XCResult agrees on all 311 outcomes; all 1,902 source
inputs, eight frozen Rust binaries, 11 strict signatures, both fixture wrappers
and actual owned-process absence are verified. See the
[complete failed receipt](artifacts/macos-native-2cf93cf56-failed-results.json)
and [source manifest](artifacts/macos-native-2cf93cf56-source-inputs.json).

UI teardown now normally closes the exact owned Settings window and verifies
its disappearance before closing browser windows; the original normal
app/service-stop assertion remains. The first four-method follow-up passes
both previous failures and runtime language, but the normal-relaunch fixture
cannot align its owned Finder zoom control at the Settings close point
(66 versus54, with the original three-point bound). Its real three-pass/one-fail
result and cleanup remain in the
[first focused receipt](artifacts/macos-owned-settings-close-first-focused-failed-results.json).
That fixture now positions only its owned Settings window inside the screen
before capturing the target and aligning only its newly opened Finder window.
The original three-point alignment, immediate pointer handoff, normal Cmd-Q,
relaunch and Finder/window/resource cleanup assertions remain.

All four methods then pass with zero failures/skips and exit0: session recovery,
Traditional Chinese multi-window, runtime language/editor/private-panel/state
preservation and normal quit/relaunch. All 1,902 inputs, eight frozen Rust
binaries, the ten signatures used by these four methods, both wrapper cleanups
and actual owned-process absence pass. The independent drag fixture is neither
built nor used by this cohort; complete native acceptance verifies its signature
separately. See the
[positive focused receipt](artifacts/macos-owned-settings-close-focused-results.json)
and [current source manifest](artifacts/macos-owned-settings-close-source-inputs.json).
A fresh complete311-method native gate remains required after these UI fixture
corrections. Production Rust/WinUI and the Mac Swift IME repair are unchanged.

The independent VoiceOver native-event attempt uses already authorized session
keyboard events, exact foreground-app/system-reader guards and preallocated
modifier-release events. The actual phrase-copy driver exits0 and observes
restored modifier state, but the clipboard remains unchanged and no navigation,
action or phrase is accepted. Reader-off/process absence, original input source,
clipboard, normal app exit0 and owned runtime/preferences/fixture removal pass.
See the [actual failed result](artifacts/physical-voiceover-native-event-channel-failed-results.json)
and [retained native driver](artifacts/voiceover-owned-native-event-key.swift).
The unresolved observation requires actual command-assignment inspection or the
system caption panel described by
[Apple](https://support.apple.com/en-lamr/guide/voiceover/unac078/mac); it does not
justify assuming a successful reader or changing scripting authorization.

The actual system Utility Command Assignments browser is then inspected with
native AX reads under exact new PID/system-path/foreground guards. Its selected
set is `使用者（0自訂）`, and the unique `將最後一段詞句拷貝到剪貼板`
command displays `⌃⌥  ⇧C`, matching the delivered shortcut. Normal category,
browser, fixed-search and row-selection UI actions do not edit assignments.
The command browser Done and newly owned Utility termination both exit0;
the post-termination application list is empty. See the
[actual assignment result](artifacts/voiceover-command-assignment-readonly-20261013-results.json)
and [retained inspector](artifacts/voiceover-utility-owned-native-inspection.swift).
Correct configured keys do not establish successful VoiceOver input recognition
or explain the missing phrase. Actual navigation/action/speech remain open.

The corrected complete311-method native gate on committed `26c97b5f8` ends
with 307 passed, three failed and one actual Zhuyin Runner skip, exit65.
All four previous Settings/relaunch/language methods pass in this full run.
The three failures are preparation errors: the fresh checkout lacks
`DragInteropFixture.app`, so file promises, custom preview and external text
methods stop before the independent app can launch or a drag can begin.
The direct recording runner omitted the separate fixture builder that the
standard `test.sh` entry point runs. All 1,902 source inputs, eight Rust binaries,
ten available signatures and owned wrapper/process cleanup pass; the missing
required drag-fixture signature is explicitly incomplete, and this full result
cannot be accepted. See the
[failed preparation receipt](artifacts/macos-native-26c97b5f8-fixture-failed-results.json).

The recording runner now builds and signs the independent drag fixture before
starting any test. The standard entry point retains its fixture build for the
ordinary mode and lets the recording runner perform it in recording mode,
so each mode builds the fixture once. A new checkout starts with that bundle
absent, then passes all nine actual OS drag methods and the four related
Settings/relaunch/language methods:13 passed, zero failed/skipped, exit0.
The actual new bundle identifier/executable digest, all 1,902 source inputs,
eight unchanged Rust binaries, all 11 signatures, both fixture wrappers and
actual owned-process absence are verified. See the
[cold-checkout result](artifacts/macos-cold-drag-fixture-thirteen-results.json)
and [source manifest](artifacts/macos-cold-drag-fixture-source-inputs.json).
All existing test assertions and production source remain unchanged by this
entry-point correction. Fresh complete311-method acceptance remains required.

Read-only inspection of the actual Utility Visuals/Panels and Menus page confirms
`顯示字幕面板` is already checked (`_NS:10`, value1). No preference toggle is
needed or performed; the newly owned Utility is normally terminated and its
post-cleanup application list is empty. See the
[caption preference result](artifacts/voiceover-caption-preference-readonly-results.json).
The next physical attempt reads only the exact newly started system Reader's
caption window while the owned fixture app is foreground. It permits only known
fixture phrases in output, injects no speech and writes no clipboard data.
Its actual `AXWindows` query returns `-25204` (CannotComplete) through the bounded
observation interval, yielding no windows or accepted phrase/navigation/action.
This error does not establish a permission denial. Original reader-off/process
absence, ABC input source, preserved clipboard, normal owned app exit0 and
runtime/preferences/fixture removal are verified in the
[actual failed observation](artifacts/physical-voiceover-caption-failed-results.json)
and [retained inspector](artifacts/voiceover-owned-caption-inspector.swift).

The fresh complete311-method gate on committed `a1fcf96d8` now finishes with
310 passed, zero failed and one actual Zhuyin Runner-permission skip, exit0.
All nine OS drag methods, four related Settings/language/relaunch methods and
the remaining protocol/UI methods pass in this complete run. The independent
seven-stage physical Zhuyin result already verifies current unchanged production
Swift; the separate Runner skip remains recorded as skipped. XCResult and logs
agree on all 311 outcomes; all 1,902 program inputs, eight frozen Rust binaries,
11 strict signatures, both live fixture wrapper cleanups and actual
owned-process absence pass. See the
[complete current receipt](artifacts/macos-native-a1fcf96d8-results.json)
and [frozen source manifest](artifacts/macos-native-a1fcf96d8-source-inputs.json).
The corrected fixture entry and UI cleanup fixes are accepted by this full gate.
VoiceOver navigation/action/speech, second-monitor transfer and physical printer
acceptance remain separate open requirements.
