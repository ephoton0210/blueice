# macOS labels for non-rendered controls

Status: accepted on 2026-10-09; see the [dated results](MACOS_NONRENDERED_LABEL_RESULTS.md).
Parent milestone: `1dfcd7930c96ba12d952e13c863182d68efda2cc`.

## Activation and focus

A visible label can activate its associated enabled control even when the control
has no layout fragment or carries `hidden`. The added cases cover `display:none`
and the HTML `hidden` attribute. Explicit association still uses the
first matching ID only when that element is labelable; implicit association uses
the first labelable descendant. An input whose type is `hidden` remains outside
that set. Disabled controls, disabled fieldsets and inert control subtrees remain
unavailable. Interactive descendants retain their own action.

Activation availability and focus geometry are separate core decisions. Hidden
and non-rendered controls stay outside native Tab order and cannot acquire native
focus. A visible label with an explicit `tabindex` can retain its own focus when
its associated control cannot take focus. Hidden radio activation changes its
group without clearing that label's focus. Existing visible-control focus and
text editing retain their previous rules.

The [HTML label definition](https://html.spec.whatwg.org/multipage/forms.html#the-label-element)
permits platform label behavior, and the
[picker algorithm](https://html.spec.whatwg.org/multipage/input.html#show-the-picker,-if-applicable)
separates mutable file-input activation from the select-specific rendered check.
The native behavior above is an implementation choice consistent with those
boundaries; it does not establish complete DOM or HTML event conformance.

## File selection and defaults

Label and associated-control click dispatch precede the default. Cancellation at
either listener, a replaced document or changed association suppresses the stale
action. The existing correlated gesture returns the actual file control's context
and retains its tab, frame-source, document and revision ownership. The AppKit
picker validates that context without requiring an accessibility node or another
click. A reply alone does not read any OS file; actual native selection retains
the accepted file-count, byte, metadata and regular-file limits.

Prepare, Validate, Set and Cancel use the same control availability rules.
Selection and cancellation retain the accepted File/FileList snapshots and event
ordering. Form reset clears current selections and invalidates old picker
contexts while previously retained immutable File values remain readable.

Associated hidden checkbox, radio and submit controls execute their existing
defaults. Radio group membership and successful form controls retain their
existing semantics. Hidden controls acquire no accessibility node or native
editing capability through label activation.

## Verification and delivery

Five new public Rust regressions first produced four real method failures while
the disabled/inert restrictions passed. All five pass after the correction;
the complete keyboard, file-selection and script-file interface suites pass
55 cases with zero failures or ignored cases.

Three added XCUITest methods exercise the real file panel through explicit and
implicit labels, exact binary bytes and Unicode names, cancellation, reset and
retained File reads; canceled/disabled/inert/interactive-child rejection; and
hidden checkbox/radio/submit defaults through an actual GET submission. These
and the existing label/file/model/AX regressions pass all 13 focused methods with
zero failures or skips. Three new UI images were reviewed and remain ignored by
Git. Complete unfiltered native acceptance executed all 272 methods: 271 passed and the existing physical Zhuyin method skipped.
Full workspace formatting, strict Clippy, build, tests and the unfiltered native
suite must pass before this increment is accepted and committed.

Directory/capture selection, streams, FileReader/object URLs, general drag/drop
and the remaining macOS delivery requirements stay in the
[delivery plan](MACOS_DELIVERY_PLAN.md).
