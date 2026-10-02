# macOS browser delivery milestones

The macOS browser remains an active delivery target. This plan translates the
Phase 22 native-shell requirements into reviewable commits; an initial window,
a passing smoke test, or one bridge does not complete the browser design.
Every implementation milestone includes native UI tests, focused core/protocol
regressions, a dated result record and a scoped commit. Validated commits are
pushed to the current tracked branch as authorized by the owner.

| Milestone | Delivery and required evidence | State |
| --- | --- | --- |
| Native window and chrome | SwiftUI/AppKit address/tabs/history/settings, visible real-core pixels, startup/close/resize XCUITest | Committed `c762b53a9` |
| Owned service stack | Bundled launcher/core/gatekeeper, reviewed external navigation, fail-closed review, normal/forced cleanup | Committed `32fa07f6b` |
| Page accessibility | Core semantic tree mapped into NSAccessibility, native actions, privacy, stale-element and tab isolation tests | Committed and pushed `41952a803` |
| Native text editing | Core-owned UTF-16 selection, grapheme movement/deletion, IME composition/update/commit/cancel, caret/candidate geometry, text/password/textarea editing, clipboard policy and native UI tests | Foundation committed and pushed `8b80c3c53`; physical OS IME and remaining editing behavior pending, see [results](MACOS_NATIVE_EDITING_RESULTS.md) |
| Keyboard and page interaction | Keyboard-only form completion, checkbox/radio/select/range controls, find-in-page, native context menus, drag/drop and file-selection policy tests | Keyboard increment committed and pushed `97794bb57`; native form reset validated in [results](MACOS_FORM_RESET_RESULTS.md); form submission and remaining interactions pending |
| Windows and tab organization | Multiple native windows, tab groups, profile/context lifecycle, retained history and state handoff using core tab identities | Pending |
| Downloads and printing | Actual download manager/shelf, progress/cancel/open/reveal, print/PDF media output and native panel tests | Pending |
| Trusted browser panels | Assistant results and human permission decisions using the private owner boundary, policy-denial and no-AI-grant tests | Pending |
| macOS display and system integration | DPI/multi-monitor, zoom, theme/high contrast/reduced motion, fullscreen, localization and native menu/shortcut tests | Pending |
| Full accessibility and final audit | Text ranges/live regions and supported rotor operations; actual screen-reader action; final integration, design/UI acceptance and documented remaining core limitations | Pending |

## Native editing contract

The core owns focus, document identity, committed text, selection and temporary
IME composition. Platform ranges use UTF-16 offsets, while core mutations check
Unicode boundaries and delete/move by grapheme clusters. The existing append-only
compatibility messages remain distinct from the versioned native editing path.
Native commands carry the live frame-directory source and document generation;
a late command from a replaced page must not edit a new document.

AppKit implements `NSTextInputClient` using core-originated state and geometry.
The frontend does not create a DOM or relayout the page. Candidate positioning,
selection and marked ranges must follow the actual focused control and viewport.
Protected text is editable through ordinary input but remains redacted from
representation, input-state inspection and accessibility text values. Extension
DOM-write permissions remain independently constrained.

Clipboard and file-selection boundaries are explicit user operations. Page/AI
requests do not silently inspect OS content or authorize a native picker.
The eventual native editing milestone must include CJK and RTL input, replacement
and cancellation, focus/navigation/tab invalidation, selection/copy/cut/paste
policy, protected/disabled/readonly controls, visible caret/selection pixels,
and actual native-window automation. Deterministic composition-sequence tests
and OS input-method tests are reported separately.

The 2026-10-02 foundation includes those deterministic callbacks, range/geometry
boundaries and actual-window selection/clipboard tests. Its system Zhuyin test
is implemented but skipped on this host because `AXIsProcessTrusted()` is false
for the UI runner. Remaining editing work includes physical IME validation,
JavaScript keyboard/beforeinput/input/composition event dispatch, undo/redo,
complete bidirectional shaping, caret blink, preferred vertical caret position
and general keyboard input during asynchronous focus changes. The later
keyboard milestone buffers native events specifically across Tab handoffs;
it does not complete native editing acceptance.

## Keyboard control increment

The core derives Tab order and default actions from the current document.
Checkbox/radio state, single-select choices and stepped range values feed the
same semantic snapshot and shared layout/paint output. Native AppKit Tab
transitions retain bounded events until the core confirms the new focus, then
deliver them to the page or the SwiftUI address editor. Command-L uses native
address focus. The owned window also holds later keys until pending events have
replayed, preserving event order across responder changes. The keyboard activation boundary honors existing click
cancellation and document replacement before applying defaults.

This increment covers keyboard control completion and reviewed link activation,
not full form submission. Submission, validation, popup/typeahead and
multiple-select interaction, DOM key/input/focus/composition events, full chrome
Tab traversal, find, context menus, drag/drop and file-selection remain required.
Multiple-select direction keys deliberately preserve existing selections until
their own interaction model is implemented. Physical OS IME and screen-reader
acceptance remain separate gates.

## Native form reset increment

The core retains original control defaults separately from live native values.
Reset buttons restore their current form owner's text/password/textarea,
checkbox/radio, select and range state, including external associations and
disabled/readonly fields. Unrelated forms and tabs retain their values. A reset
retains control focus, invalidates old native input contexts, ends composition
and republishes shared pixels and semantic values without fetching.
Input reset/submit/button captions use the same core label for paint and
semantics. Existing pre-default click cancellation and document replacement
remain effective. Explicit page-script textarea textContent/appendChild changes
update the retained default; extension/native live-value edits do not.
The native shell clears a correlated editing error after a successful edit or
focus change, while preserving navigation and mandatory policy-denial notices.

This increment covers the native reset default. Full cancelable DOM reset-event
dispatch, form methods and encodings, constraint validation, dirty value/default
DOM property semantics and file-input state remain pending. The separate result
record identifies exactly which real-service and actual-window cases passed.

## Completion audit

Before final completion, check every milestone against its code, native app,
result bundle, result record and commit/push state. Verify the same core owns
all inspected/rendered/edited pages. Scope native UI assertions to the exact
feature they exercise; do not use a broad green test count as evidence for
untested IME, screen-reader, permission, print, profile or window behavior.
The Phase 22 cross-platform work remains separate from this macOS delivery;
its unfinished items must not be reported as achieved by the macOS frontend.
