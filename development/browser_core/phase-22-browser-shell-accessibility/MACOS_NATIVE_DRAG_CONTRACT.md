# macOS native page drag contract

Status: the bounded macOS software increment passed Core, AppKit and actual OS
drag acceptance 2026-10-11. Windows runtime acceptance remains pending. This increment follows the native input and privacy
requirements in [PLAN.md](PLAN.md). It does not complete the macOS delivery plan.

AppKit starts a page drag from an actual pointer gesture on Core-provided
selection geometry or a rendered draggable source. Core owns the source bytes,
supported operations, document identity and drag lifetime. The shell converts
view/screen coordinates and presents a native drag image. Password selections
cannot become ordinary drag payloads.

Every transaction retains context/window/tab, frame source, document generation,
displayed frame and drag identity. Coordinates, modifiers and physical buttons
travel together. Stale identities, replaced documents, unsupported operations,
cancelled sources and replayed drops cannot edit a successor document. Source
movement may leave the viewport within its bounded coordinate budget.

Destination entry and movement send format metadata only. Core dispatches page
drag events before applying defaults; listeners see a protected DataTransfer
until the explicit OS drop. Entering a new target precedes leaving the old one;
relatedTarget is limited to the same owned document. The final drop rechecks the
target and negotiated operation before revealing bytes or applying an edit.
Read-only, disabled and inert destinations refuse the ordinary text default.
Accepted edits participate in native Undo/Redo. A move consumes the owned source
selection once, after an accepted destination operation.

Start, source movement and drop acknowledgements use a bounded synchronous
private broker whose reply does not depend on the GUI MainActor reader. A failed
exchange is not retried as a mutation. Start and drop retain the displayed frame;
a newer Core frame cannot silently replace it while GUI updates wait. Refusal
releases the destination's metadata-only session. Source movement refreshes the
frame for its already-owned source without retargeting it. Destination entry and
updates also use the bounded broker: AppKit receives the actual Core decision
before its callback returns. Unchanged exact pointer/frame decisions are reused;
no queued movement can restore an earlier decision after the OS drag ends.
Negotiation carries metadata only. JSON parsing preserves each finite coordinate's
exact binary value, so an echoed fractional mouse snapshot passes strict identity
checks without relaxing the frame, owner or pointer comparison.

Core compares retained paint commands and scroll state around each native drag
transaction. A default edit or listener paint change publishes a new raster
frame before its drag acknowledgement, which identifies that new generation.
Unchanged metadata-only entry and stationary motion do not publish extra frames.
The private broker consumes frame, viewport and display-preference notifications
before the drag acknowledgement; the GUI displays the same Core-owned pixels.

Text source bytes are AppKit pasteboard promises. A shared publication lock
serializes promised-data callbacks with the Core source decision. Cancellation,
service failure or invalidated ownership revokes future publication and clears
only the original owned pasteboard generation. Successful drops preserve the
normal AppKit provider lifetime. Receivers may retain bytes already published
before cancellation; revocation cannot recall them. No private OS abort API or
injected global input is used.

Source updates run on movement and a 350 ms timer in common/event-tracking
run-loop modes. Completion or refusal stops the timer. The periodic update
checks the held physical button and current owner before dispatch. Foundation
timer checks establish the helper's behavior; real AppKit tracking-mode and
stationary OS drag behavior passed the acceptance tests below.

Plain text, HTML and URI text use their standard AppKit pasteboard types. Other
web format names have stable private UTI identifiers; bounded metadata preserves
their web names for BlueIce receivers. File destinations admit selected regular
file bytes only on explicit drop and do not expose filesystem URL text to pages.
External custom-format interoperability and mixed text/file formats remain open.

The VM supplies branded Event/UIEvent/MouseEvent/DragEvent interfaces, ordered
capture/target/bubble listeners, once/passive/object callbacks, protected drag
stores and bounded getAsString tasks. The separate immutable BlueTS drag profile
describes only installed APIs; it preserves the previous file profile.

Collector-owned transfer and event state use separate allocations so these
optional browser interfaces do not enlarge every ordinary JavaScript object.
The managed heap charges the state records as well as their dynamic payloads;
replacement preserves tracing and byte accounting. The existing arguments
elision/allocation regression retains its original collection thresholds.

Required native acceptance includes actual-Core protected/drop event behavior,
explicit-drop-only mutation, unavailable controls, same-URL document replacement,
an actual Core frame update withheld from the GUI during a drop, post-drop frame
publication and changed GUI image bytes with matching Core text/frame state,
synchronous metadata-only entry decisions, promised-data revocation, source-decision publication ordering, tracking-mode
timer shutdown and valid custom pasteboard types. Actual XCUITest gestures must
copy selected document text, refuse a read-only drop, cancel a source on its page
listener, and cancel after at least 250 ms at an unchanged held pointer position.
The latter test holds the OS drag for 1.2 seconds and inspects the same owned Core
document afterward; it cannot pass solely from helper callbacks.

Actual focused and complete macOS native suites and the corrected complete Rust
workspace passed. Sources, signatures and owned-process cleanup were checked.
See [dated results](MACOS_NATIVE_DRAG_RESULTS.md) for the exact scope and remaining
physical, Windows and broader browser gates.

Remaining work includes source files/promises, default source images,
setDragImage, operation-modifier policy, external/multi-window acceptance,
stationary update performance, Window/view branding and broader scroll/transform
geometry, AbortSignal, synthetic dispatchEvent and numeric indexed BlueTS types.
Physical accessibility/IME and the other browser milestones keep their existing
separate acceptance requirements.
