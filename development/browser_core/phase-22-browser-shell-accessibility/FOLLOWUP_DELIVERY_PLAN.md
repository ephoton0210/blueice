# Cross-platform, drag interoperability and Web API follow-up

Started 2026-10-12 from `67061e667`. The requested objective includes all four
tracks below. Completing one software increment does not complete this plan.
Evidence must identify its source snapshot, actual commands, runtime platform,
case scope, failures/skips and owned-process cleanup.

| Track | Required outcome | Current state |
| --- | --- | --- |
| Windows acceptance | Same source snapshot; complete Rust workspace, strict Clippy, all-target build, full formatter scope, WinUI protocol tests and actual logged-in desktop UI/pixels/cleanup | The owner's dedicated 320 GiB test disk on the Linux 2 TB `/data` volume is attached live/persistently as Windows B: `BlueIceTests320`. BlueIce evidence and tool caches are copied; 2,077 archived inputs and 1,181 toolchain files pass SHA256 checks. Subsequent workspaces, build products, tool caches and process TEMP/TMP use `B:\BlueIce`. C: now has 6.75 GiB free, passing the original 4 GiB guard after verified toolchain relocation with a compatible old-path junction. Six file-free temporary trees were removed. No VM reboot or pagefile-policy change occurred. Frozen `83436f846` full Windows acceptance now passes: 5,670 Rust cases, zero failures/64 ignored, strict Clippy, all-target build/formatter, WinUI build, five protocol cases and seven actual desktop cases. Task/bootstrap exit zero, frozen source hashes match and owned runtime cleanup passes; see [complete receipt](artifacts/windows-83436f846-results.json) and [storage delivery](WINDOWS_TEST_STORAGE.md). Later API increments still need current-source complete Windows gates. |
| Drag interoperability | Actual OS drags between live windows and an independent application; file export/promises, default/custom drag images, `setDragImage` and modifier-selected operations | [Related native acceptance](MACOS_DRAG_INTEROP_RESULTS.md) now passes 29 cases, zero failures/skips, including actual cross-window/external text, binary file-promise export, Copy/Move/Link, Core-backed custom images and stationary-source cancellation. This snapshot precedes FileReader changes. Full native acceptance and broader promise acceptance remain required. |
| Physical system acceptance | Actual CJK/Zhuyin composition/commit/cancel, VoiceOver navigation/action and physical monitor/printer transitions | System Zhuyin test still skips because the Runner lacks Accessibility permission. The hardware report omits display details and lists no configured printers; the actual recorded desktop is 1920x1080 points. Physical device availability and Runner authorization are pending. VoiceOver has not been accepted. |
| Web platform APIs | Actual FileReader lifecycle and data reads; actual stream objects and readers; origin/document-scoped object URLs; broader DOM/event behavior and accurate host typings | FileReader, EventTarget, ProgressEvent and drag/event interfaces pass 58 VM cases plus five typing cases. Readable streams, default/BYOB readers and Blob byte/text streams pass 20 stream cases and six host-profile cases. Actual Core/BlueJS document, selected-byte and stream DOM integration passes, as does failed-task acknowledgement containment without script replay. The affected BlueTS crate passes all 354 cases with 60 optional oracle cases ignored. The [object URL increment](OBJECT_URL_RESULTS.md) adds actual retained bytes, document/context fencing, staged IPC, URL/BlueTS bindings and local Core navigation with preserved script origin. The [abort increment](ABORT_SIGNAL_RESULTS.md) adds real signal reasons, weak dependent graphs, timeout tasks and cancellation of native/generic listeners, with an 88-case final cohort and actual selected-byte integration. The [CustomEvent increment](CUSTOM_EVENT_RESULTS.md) adds branded arbitrary detail, dispatch-safe legacy initialization, GC retention and an additive 37-binding profile; 102 VM/type cases and actual Core/child DOM integration pass. Native/full/cross-platform gates, remaining Streams APIs/conformance, broader URL/subresource behavior and DOM/event work remain required. |

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
