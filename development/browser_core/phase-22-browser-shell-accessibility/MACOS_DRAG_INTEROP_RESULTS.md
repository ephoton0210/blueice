# macOS native text drag interoperability

2026-10-12, starting from `67061e667`. Focused native acceptance passed **18
cases, zero failures and zero skips** on macOS arm64. Full native acceptance
and the complete four-track [follow-up plan](FOLLOWUP_DELIVERY_PLAN.md) remain
open.

The inactive-destination regression fails on the original production sources
and passes after the fix. Destination negotiation no longer depends on keyboard
focus. Context/window/tab, document, displayed-frame, source and pasteboard
identity checks remain enforced. Hidden and foreign windows still refuse drops.

Actual OS tests copy document text to an inactive second browser window, inspect
the destination Core value, verify Undo/Redo and continue after the source window
closes. A separately built AppKit application exchanges actual OS text drags in
both directions. It neither imports BlueIce nor emulates Core responses. Existing
protected-data, disabled/inert/readonly, replaced-document, unseen-frame,
publication/revocation and stationary-helper regressions pass in the same run.

Two earlier cross-window fixture failures remain recorded: the initial address
query was not window-scoped, and a later 3-point positioning correction stayed
below AppKit's native drag threshold. The corrected helper moves far enough to
begin dragging while retaining the original precise position assertion. The
source comparison and failure history are in the
[machine-readable receipt](artifacts/macos-drag-interop-results.json).

The successful run uses `.build/results-20261012-103251.xcresult`. Update/SFTP
fixtures report complete owned cleanup. Screenshots are exported beside that
bundle and copied into the receipt's artifact paths. The unchanged backend's
1,740 program/build inputs match the accepted parent; this increment does not
claim a fresh complete Rust workspace run. Xcode records existing internal QoS
warnings separately from test failures.

The separate system Zhuyin attempt skips because the Runner lacks Accessibility
permission. Windows remains below its unchanged C: disk guard. Physical IME,
VoiceOver, monitor/printer transitions, file export/promises, custom/default drag
images, modifier policy, FileReader, streams and object URLs remain required.

## File-promise and modifier follow-up

The subsequent focused file-export run passes three cases on
`.build/results-20261012-105259.xcresult`: an actual OS drag to an independent
NSFilePromiseReceiver writes `export-中文.bin` with the exact seven binary bytes;
public delegate checks verify delayed materialization, retained delegate lifetime,
revocation and preservation of existing files/symlinks. Publication writes through
a retained receiver-directory descriptor and uses exclusive atomic rename. No
source path is supplied by the page. Source validation/revocation shares the
snapshot-admission lock; disk I/O does not hold that lock. Already admitted
fulfillments may finish after revocation, while later admissions refuse.
The [file receipt](artifacts/macos-file-promise-results.json) retains the original
missing-export failure and the corrected incompatible Foundation write options
that crashed the first candidate. This is focused evidence, not complete native
or comprehensive promise acceptance.

Actual Option-copy, Command-move, Option-Command-link and readonly move refusal
pass on `.build/results-20261012-111921.xcresult`, with one public policy case.
The original OS test passed Copy but refused Command Move. BlueIce now computes
its source mask from Core-advertised operations and disables AppKit's additional
legacy modifier filter. Destination negotiation, prepare and commit retain the
same captured modifier decision. The actual dragend event reports each committed
operation, and Move clears the source while Copy/Link preserve it.

The combined 25-case related suite passes with zero failures or skips on
`.build/results-20261012-112742.xcresult`. All 11 frozen native inputs match,
11 app/service signatures pass, and update/SFTP fixtures report complete owned
cleanup. The unchanged 1,740 backend/build inputs still match the accepted parent.
The [combined receipt](artifacts/macos-drag-related-results.json) records exact
case IDs, hashes, product checks and screenshots. Actual stationary OS drag and
page-source cancellation pass in this same cohort.
Full native acceptance, custom/default source images, `setDragImage`,
incoming/mixed promise interoperability and broader file metadata/receiver tests
remain open, along with the other follow-up tracks.

## Core-backed drag-image follow-up

The subsequent related native cohort passes 29 cases (21 protocol/Core and eight
actual OS methods), zero failures/skips, on
`.build/results-20261012-135034.xcresult`. All 11 strict signature checks pass.
Core resolves the child's opaque DOM handle in its exact live document before
producing custom image regions and hotspot geometry. AppKit extracts masked
pixels from the correlated Core frame, respecting clipping and zoom. The actual
service test asserts the 100x48 custom preview, exact hotspot and updated green
bitmap; receiver measurements also compare against an independent native source.

The retained OS recording contains two moving green preview observations within
the custom drag's logged time range. Their bounds are 100x49 and 100x48 points,
with 117 points of horizontal movement, on the actual 3840x2160 pixel recording.
The verifier rejects an earlier 128x24 generic-label recording. An earlier
verifier accepted window positioning outside the custom event's time range;
that result is disqualified and retained separately. Software decoding uses
task-local PyAV 15.1.0 without changing system Python or security settings.

The [image receipt](artifacts/macos-drag-image-results.json) retains exact cases,
the 1,853-input native snapshot, signatures and log/video/image hashes. Later
FileReader source changes are explicitly outside that native acceptance. This
cohort does not replace full Rust/native or Windows/hardware acceptance.
Incoming/mixed/multiple file promises and broader metadata/receiver errors remain
open in the full follow-up objective.
