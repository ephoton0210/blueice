# macOS inert and painted pointer contract

Status: accepted for ordinary DOM inert pointer exclusion and ordinary painted
sibling priority. See [dated results](MACOS_INERT_POINTER_RESULTS.md). This
increment does not complete the macOS delivery plan.

## Pointer behavior

Ordinary DOM elements carrying the boolean `inert` attribute, and their text or
element descendants, are excluded while traversing layout fragments for a pointer
target. Attribute presence applies even to `inert="false"`. An eligible containing
element or overlapping lower layer can receive the pointer instead. This follows
the pointer transparency requirement in the
[HTML inert subtree specification](https://html.spec.whatwg.org/multipage/interaction.html#inert-subtrees).

The shared traversal serves public link hit testing, element click dispatch and
hover. It checks the live document ancestry and skips unavailable fragment
subtrees. Ordinary sibling fragments are visited in reverse of their existing
paint emission order, so the last painted eligible fragment receives a hit.

A label excluded by its own or inherited inertness cannot forward a pointer
activation to an external file input. An inert descendant inside an active label
is transparent, leaving the active label eligible to activate its associated
control. Existing native gesture correlation, control availability and document
ownership checks continue to govern a picker request.

## Reproduction and public boundary

Six regressions were added to `backend/core/engine/tests/native_keyboard.rs`
before changing production. The original implementation produced five failures
and three passes in the eight-method inert selection. After the correction, the
same test bytes passed all eight methods. Exactly two production files changed
between those runs: `page.rs` and `page/dom_helpers.rs`.

The cases cover inert label sources, inert descendants of active labels, inert
links, transparent overlapping layers, an inert document root and painted sibling
priority. Active label/link controls establish the pointer geometry. Overlap
tests also require the public rendered pixel at the clicked position to be blue,
demonstrating that the later layer is actually painted there. The original
overlap transparency case already passed because its incorrectly ordered hit
testing happened to choose the desired lower layer; the active overlap control
reproduced that ordering defect.

The completed keyboard, file-input and script-file focus has 61 passes across
three public test groups. A preceding command used a nonexistent test target;
its exit and raw log are retained separately from test execution results.

## Native dispatch correction

Actual-window testing exposed a second defect after the core correction. Painted
inert controls remain in the shared representation while the native accessibility
tree excludes them. The original native pointer classifier still treated an inert
button as a control, chose the ordinary focus/click path and never sent the
correlated native activation that can return a file hint. An unchanged isolated
run and a button-first run reproduced the failure; moving the owned browser
window with the existing sheet fixture helper did not resolve it. Temporary
pointer tracing confirmed this dispatch branch. Those traces were removed before
final acceptance inputs were frozen.

The native control classifier now excludes controls that are both absent from
the accessibility tree and unavailable for native focus. Enabled aria-hidden
controls retain native focus geometry and their normal pointer interaction.
Direct file/select pointer interception and select-list wheel routing additionally
require the existing native focus availability flag. The core remains the owner
of the actual hit target and default action; the classifier only selects the
native dispatch path.

A new XCTest method sends AppKit mouse events through the view and real core
services. Its span positive control passed before the inert button reproduction.
The final corrected test failed on the original native routing and passed after
exactly two Swift production files changed, with test bytes unchanged. It covers
inert span/button/ancestor/file/select profiles, validates the external hidden
file control's unique accept profile and live context, and verifies enabled
aria-hidden button focus and listener delivery. The standalone XCTest host uses
an explicitly simulated key-window guard; real window ownership remains an
XCUITest requirement.

Earlier unit fixture failures are retained separately: non-rendered file controls
intentionally have no representation node; the standalone host did not acquire a
key window; and button focus must use the general focused-node field rather than
the text-control field. A temporary diagnostic also had an unused-result compile
error before any native method ran. These attempts are not semantic acceptance.

## Actual-window verification

Four new XCUITest methods use the existing normal HTTP fixture, real core and
isolated page script host. Coordinate clicks reach painted inert sources even
though those sources are absent from the accessibility tree. Assertions check
picker presentation or cancellation, listener counts, retained input values,
document URLs and exact HTTP requests. An active source in each relevant profile
provides a positive control. The active-label case includes inert file and select
descendants alongside span/button/ancestor variants. Seven existing native
label/file-picker cases and the new dispatch XCTest method are included in
focused acceptance.

Completed focused native acceptance passed one XCTest method and eleven
actual-window XCUITest methods, with zero failures and zero skips, on the same
final program/build inputs as the 61-case public Rust focus.

That initial focus preceded the later correction to the UI positioning fixtures.

An earlier unfiltered native attempt executed all 278 methods and retained one
failure in an existing cross-window tab-drag positioning helper. Its third owned
window reached (48, 40) rather than (40, 40), while retaining the requested
760 by 600 size. The unchanged complete method then passed in isolation. The
underlying positioning cause is not established; no program or assertion was
relaxed between those attempts. Both attempts and the owned-window debug
attachments remain available for final acceptance review.

The second complete native attempt executed all 278 methods: 274 passed, three
failed in the positioning helpers, and the same physical Zhuyin method skipped.
Owned-window attachments show horizontal positions 47, 43 and 44 instead of 40,
with the expected vertical positions and sizes. The corrected helpers capture a
single frame and use bounded native drags to reach the existing target; they
retain the original less-than-three-point condition and all feature assertions.
All 122 UI test method bodies remain unchanged. The three formerly failing full
feature methods pass on the corrected fixture. Expanded focus passes 15 methods
(one XCTest and fourteen XCUITest methods). Fresh unfiltered native acceptance
passes 277 methods with zero failures and the existing physical Zhuyin skip.
Rust gates retain their original input manifest: the corrected native manifest
differs only in this private UI positioning helper file; all other 1,836 inputs
and all 122 UI test method bodies are unchanged. No Rust gate is represented as
a fresh execution on the corrected Swift fixture.

Complete acceptance additionally requires formatting, strict all-target Clippy,
the all-target build, the Rust workspace and the exact unfiltered native method
set, followed by unchanged-input, product and owned-process audits. Generated
xcresult bundles and verification images stay in the ignored macOS build tree.

## Remaining boundaries

This increment covers the current ordinary DOM ancestry and ordinary fragment
paint order. Full CSS stacking contexts, positioned layout, general CSS
`pointer-events`, flat-tree slotting, modal dialog/top-layer inertness and inert
mutation during already-dispatched defaults remain outside this acceptance.
The shared core hover traversal changes with the hit test, but this increment
does not add ordinary mouse-movement or mouse-leave forwarding from the macOS
viewport. That native bridge remains separate delivery work.
The existing physical Zhuyin skip does not prove physical IME behavior.
The [delivery plan](MACOS_DELIVERY_PLAN.md) retains the remaining software,
distribution and physical hardware requirements.
