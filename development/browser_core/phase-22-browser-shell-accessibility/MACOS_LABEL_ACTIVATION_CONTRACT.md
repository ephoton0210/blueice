# macOS label activation

Status: accepted on 2026-10-09; see the [dated results](MACOS_LABEL_ACTIVATION_RESULTS.md).
Parent: `d42c23a33422cf8fd64b8c801cc2ed4ac7e98861`.
This increment extends the native form controls and selected-file picker in the
[macOS delivery plan](MACOS_DELIVERY_PLAN.md). Full browser delivery remains active.

## Behavior

Core resolves HTML labels against the current document. An explicit `for`
resolves the first matching ID in tree order only if that element is labelable.
Without `for`, the first labelable descendant is selected. Hidden inputs are not
labelable; buttons, nonhidden inputs, meter, output, progress, select and textarea
are. Clicking an interactive descendant uses that descendant's own action and
does not forward activation to the ancestor label's associated control. These
rules follow the [HTML label element](https://html.spec.whatwg.org/multipage/forms.html#the-label-element).

The macOS pointer path dispatches the original click before the associated
control's click, then applies the control's existing native default. Either
listener may cancel. Core resolves association after the first listener and
checks the same association, document and available control after the forwarded
listener. Disabled controls, including disabled fieldsets, cannot receive the
forwarded activation. An unavailable listener suppresses the default. Existing
document selection gestures continue to distinguish a click from a drag.

Forwarding currently requires an enabled control with a layout fragment. The
control and its ancestors must not have `hidden` or `inert` attributes. Labels
targeting file controls without a layout fragment remain pending.

`NativeActivate` validates the source, document, focus context and finite point.
Every successful gesture receives its own `NativeActivationCompleted` reply,
including a null file hint for empty points, canceled actions and navigation.
File hints contain the actual control's existing tab/source/document/node/revision
context. Legacy Click, NativeClick and node actions do not request an OS picker.

Only the pending native gesture consumes the matching request and tab reply.
Because launcher broadcasts replies from its connected clients, each gesture
also carries an independent nonzero random identifier that core echoes. A hint
with another identifier is ignored even when its request and tab IDs overlap.
Its document, input serial and view/window focus must still be current when its
reply is consumed. Incoming hints alone never open a panel. The native picker validates
the prepared control again before opening, avoiding a second click event, and
retains the existing cancellation, revision, navigation and file-content checks.
Only files chosen in the native panel are read. Selected-file bytes and event
semantics remain those of the accepted [file API contract](MACOS_FILE_API_CONTRACT.md).

## Acceptance

Public session regressions cover association, exact original/forwarded event
targets, cancellation at either event, document replacement at either event,
disabled controls, invalid ownership and points, file hints and empty-point
acknowledgment. Native model regressions verify that unsolicited replies have no
presentation effect. Real XCUITest mouse coordinates click the rendered label
text because a label has no separate actionable AX node.

Native acceptance must cover text focus/editing, checkbox and radio defaults,
an implicit button after a hidden input, real file selection and cancel with
ordered clicks and exact selected bytes, canceled label/control events, disabled
targets, changed association and interactive descendants. Relevant existing file
picker tests, the full Rust gates and the exact unfiltered native suite must pass.
Screenshots remain ignored; dated results must retain failures, source/product
hashes, signing and fixture/process cleanup before commit and normal push.

## Remaining scope

This increment does not establish full DOM/event or HTML conformance. Form-associated
custom elements are not implemented. Existing native control defaults remain the
baseline; label forwarding does not add new keyboard or select popup behavior.
Labels targeting controls without layout fragments remain pending.
Directory/capture uploads, general drag-and-drop, remaining File API stream and
FileReader/object URL interfaces, storage partitioning, private owner panels and
physical IME/VoiceOver/printer/display gates remain open.
