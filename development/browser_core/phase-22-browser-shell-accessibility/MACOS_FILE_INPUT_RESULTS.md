# macOS native file inputs

Snapshot base: `0d2e3ea583f163c322f6d3a9f4f331a97bddb089`.
Milestone parent: `ee40b05e736d0e93dea40a01e7712c2429b76094` (the independently committed transport correction).
The [macOS delivery plan](MACOS_DELIVERY_PLAN.md) remains active.

## Delivered behavior

Core paints file inputs as buttons and publishes the same selected basenames
through its semantic tree. The macOS page view opens an actual AppKit
`NSOpenPanel` after a local pointer, accessibility press, Enter or Space action.
The panel honors single/multiple selection and resolves supported extension/MIME
`accept` hints using Uniform Type Identifiers. A normal browser/AI activation
does not cause the frontend to open an OS picker.

Preparation uses the existing pre-default page-click dispatch. A cancelled
click, unavailable listener or replaced/disabled/hidden target fails before
the panel opens. Panel replies bind to the original tab, window wrapper,
frame-directory source, document generation, node and selection revision.
Navigation, tab ownership changes or closure cancel obsolete callbacks; core
rechecks the binding when selected content arrives. Reset and explicit empty
value assignment clear files and invalidate an outstanding selection revision.
Cancelling the native panel retains the previous selection.

Only the native panel's selected URLs reach the native reader. It checks regular
file descriptors, rejects final symlinks/directories, reads off the main actor,
and rejects growth or size/timestamp changes during reading. No selected OS path
is sent to core as reader instructions or metadata. Public `FileInput` IPC
accepts caller-provided names, media types and bytes; it neither reads a path
nor attests human consent or grants private owner
authority. Its replies contain hints and basenames, never file bytes. Debug
output for a file payload omits both its name and content.

Core retains content outside DOM attributes. A markup `value` path cannot
select or restore an OS file; nonempty ordinary value assignments do not create
a selection. Browser-owned display names remain available to the shared print
DOM without carrying file contents. Removed/non-file nodes release retained
content, and document replacement clears it. Failed selection input does not
partially replace the existing list.

Limits are 16 files and 1 MiB of content per input, with 64 files and 4 MiB per
document. Names must be basenames of at most 255 UTF-8 bytes, and MIME metadata
cannot inject headers. Multipart preserves raw binary bytes and entry order;
filenames escape quotes and field names retain existing CRLF escaping. GET,
URL-encoded and plain-text submission use filenames rather than file contents.
Required controls use the actual selection, including a chosen zero-byte file.
The existing 1 MiB navigation-body limit also includes multipart headers.

Actual submissions retain mandatory form/URL/content review at each applicable
network hop. File bytes do not enter review metadata, representation or DOM
inspection. This adds no path-reading MCP operation, extension grant or automatic
native-picker request.

## Verification

The 2026-10-03 full native run completed on macOS 26.6.2 (25G83), Apple
Silicon, Xcode 27.0 (27A266a): 110 cases, 109 passed, zero failed and one
physical Zhuyin-input skip because the runner lacks Accessibility trust.
The bundle is `frontend/macos/.build/results-20261003-102457.xcresult`.
It reports 33 internal QoS priority-inversion warnings and no SwiftUI
view-update warning. This is a development run with local ad-hoc signing.

The full native run used 54 frozen inputs with SHA-256
`075e1ee2305e46b94adad20f126f3e013d3a1f108dbc3f6bced7c6d64f9e989e`.
The final 56-input set has SHA-256
`d6d99628282dc5d06dd8ba720c1d527219dda72168965b4ba11e84e93b7b9a03`.
Changes after the full native run are the `Result::inspect` style correction,
Unicode format-character filename handling with emoji reader/UI regressions,
and the separately scoped [page-host reply/EOF correction](MACOS_TRANSPORT_RESULTS.md).
The final input set adds that correction's connection and transport-test files.
The emoji regression first failed on the original native validator; the fix
uses Unicode control-category checks matching core and accepts zero-width
joiners used in legitimate emoji names.
The final-source rerun passed both file-input XCTest cases and both actual-window
UI cases (four passed, zero failed/skipped) in
`frontend/macos/.build/results-20261003-121022.xcresult`. It reports two internal
QoS warnings and no SwiftUI view-update warning. The two native runs therefore
cover 109 unique passed cases and one physical-input skip.

The final app and bundled launcher/core/gatekeeper/download services passed
`codesign --verify --strict`; signing is local ad hoc. The app contains arm64 and
x86_64 slices, while services and this host runtime are arm64. The native app,
UI runner and bundled service process count after teardown is zero; this audit
is scoped to this build and does not claim unrelated background jobs ended.

The final frozen-source `cargo test --workspace` completed successfully:
7,200 passed, zero failed and 69 ignored across 468 result groups, including
four engine file-input cases, three new native-form cases, two IPC cases and
the final-reply/EOF transport regression.
`cargo build --workspace --all-targets`, Clippy with `-D warnings`, rustfmt,
project plist validation and whitespace checks passed. All Rust commands reused
`frontend/macos/.build/core-target`; no second cache was created.

An exploratory workspace run had compiled dependencies before the final
file-count addition. Its late rustdoc stage saw current engine source with older
IPC metadata and exited with an unresolved constant. It is excluded from
acceptance; the complete frozen-source run above resolves that mismatch.
Per-input hashes, exact counts, skip reasons and local evidence paths are in
[the text result record](artifacts/macos-file-input-results.txt).

Focused actual-window tests have selected multiple files through the system
folder/path dialog, checked CJK/emoji basenames and retained editor text, and
submitted real multipart bytes to an HTTP fixture through the bundled launcher/core and
compiled gatekeeper. They also cancel a second picker, reopen with Space and
reset without fetching. Unit/protocol boundaries cover unsafe names, MIME
headers, size/count/document budgets, stale revisions/documents/tabs, required
validation and file-reader failures. Native model tests distinguish explicit
picker actions from receiving ordinary browser replies.

The inspected, unedited multi-selection screenshot is kept locally at
`artifacts/macos-native-file-picker.png` (SHA-256
`d326cf5e8089bc49b72d4a54642a008611f2c214405ef6a7b9e9791fa17066e1`).
Its source is `9BB16DFC-15DE-46D0-A6EE-97D7B44B1489.png` in the final
result attachments. It shows the actual two-file, 18-byte selection behind the
pre-existing local-network permission prompt. Screenshots and result bundles
remain ignored; the text acceptance record is tracked.

## Remaining scope

Directory/capture selection, drag/drop, HTML label click forwarding, JavaScript
File/Blob/FileList and full input/change/cancel event dispatch remain open.
Private owner/assistant panels, session/storage restoration, physical IME and
VoiceOver remain separate delivery gates. No new Linux/Windows acceptance,
Intel runtime, distribution-signing or coverage result is claimed. Native UI
screenshots may include the pre-existing system local-network permission prompt;
tests do not change TCC or claim acceptance of that prompt.

The implementation follows the file-control and entry rules in the
[HTML input standard](https://html.spec.whatwg.org/multipage/input.html)
and [form submission standard](https://html.spec.whatwg.org/multipage/form-control-infrastructure.html),
with the bounded omissions above. Native multi-selection uses
[AppKit NSOpenPanel](https://developer.apple.com/documentation/appkit/nsopenpanel/allowsmultipleselection).
