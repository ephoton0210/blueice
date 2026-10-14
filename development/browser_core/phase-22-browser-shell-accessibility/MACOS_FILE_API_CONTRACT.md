# macOS page file API and selection events

Status: selected-file API and selection events implemented and accepted on
2026-10-08 (Asia/Taipei). Parent: `e32a422f8d194b902e3b57ef8e05cfc658aa484c`.
Remaining File API interfaces and full browser delivery stay in progress.
See [dated results](MACOS_FILE_API_RESULTS.md).
This extends the accepted native file picker. It does not replace the full
[macOS delivery plan](MACOS_DELIVERY_PLAN.md).

## Required behavior

Normal macOS browser startup must run admitted page JavaScript in the owned,
isolated BlueJS host. The ordinary browser must provide the same DOM/file
behavior exercised by the native tests; an operator-only proof profile is not
acceptance of normal browsing. Page scripts retain mandatory source review,
document/origin ownership, fixed execution budgets and owned-process cleanup.

The page must receive `FileList` through a file input's `files` property, with
ordered indexed access, `length`, `item()` and iteration. File objects provide
their basename, media type, size and last-modified metadata. File inherits Blob;
both use immutable bytes and proper branded prototypes. Blob/File construction
accepts string, binary-view and Blob parts. Slice and asynchronous binary/text
reads must preserve exact bytes, including NUL and invalid UTF-8. Remaining stream
methods require actual stream objects rather than fabricated synchronous results.
These interfaces follow the [File API](https://w3c.github.io/FileAPI/).

A successful changed selection updates the selected files before delivering
`input`, followed by `change`. Dismissing the native picker preserves the prior
selection and delivers `cancel`. Events bubble through the live document; page
listeners must observe the current files. Explicit empty value assignment and
form reset clear the selection without impersonating a user selection. These
rules follow the [HTML file-upload behavior](https://html.spec.whatwg.org/multipage/input.html#file-upload-state-(type=file)).

Navigation, tab/window replacement, reset, changed revisions and removed or
disabled targets reject stale callbacks before reading or modifying successor
state. A canceled obsolete native panel must not emit an event into another
document. Script-created Blob/File data does not grant OS access. No page-side
path, native-picker request, credential or arbitrary socket crosses the host
boundary. Existing native content/count limits remain enforced, and retained
script data must be included in actual heap accounting.

## Acceptance evidence

The initial native regression uses the ordinary browser launch. Its page creates
a Blob, installs real selection listeners, reads FileList and exact binary bytes,
then observes cancel with its prior selection and editor intact. Additional public
Rust regressions must cover construction, metadata, byte snapshots, branded
receivers, promises, garbage collection/accounting and stale document isolation.
Native acceptance must also cover reselection, reset, navigation and multiple
windows, with an actual reviewed screenshot. Compile-only and private-profile
tests do not establish ordinary GUI behavior.

Focused regressions, the complete Rust workspace, strict Clippy, formatting and
all-target build precede final unfiltered native acceptance. Exact method scope,
source/product checksums, signatures, fixture/process cleanup, earlier failures
and runtime limitations must be recorded before the milestone commit and push.

## Accepted scope and remaining interfaces

The unchanged Rust source and build inputs passed the full Rust gates. After
updating both Swift recovery fixture service lists, the final source manifest passed
the exact unfiltered native scope. The [dated results](MACOS_FILE_API_RESULTS.md) and
[receipt](artifacts/macos-file-api-results.txt) contain method counts, retained
failures and source/product/process evidence. This accepts ordinary selected
files, constructors/snapshots/metadata, slicing/asynchronous reads and selection
events; it does not claim complete File API or DOM conformance.

Blob.stream/textStream must still return actual stream objects. FileReader,
object URLs and general page/OS drag-and-drop remain pending. BlueTS FileList
numeric index signatures and host constructor new-expression inference are also
pending; typed page code can use item() and declared Blob/File interfaces.
