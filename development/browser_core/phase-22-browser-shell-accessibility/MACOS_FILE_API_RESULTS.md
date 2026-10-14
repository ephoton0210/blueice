# macOS ordinary selected-file API results

Accepted: 2026-10-08 (Asia/Taipei), on Apple Silicon macOS. Parent commit
`e32a422f8d194b902e3b57ef8e05cfc658aa484c`. This milestone completes the selected-file API
and selection-event increment. Full browser delivery and remaining File API
interfaces stay in progress.

Normal macOS startup runs admitted page scripts in the owned BlueJS process.
Page file inputs expose immutable FileList/File snapshots, ordered access and
iteration, names/types/sizes/lastModified, Blob/File constructors, slicing and
promise-based text/bytes/arrayBuffer reads. Native selection emits input then
change with bubbling; cancellation retains the files and emits cancel. Script
clear and form reset preserve retained snapshots without user-selection events.
Navigation starts a new empty input, while another window retains its own files.

## Validation

The fresh full Rust workspace passed 7,372 cases with zero failures
and 69 ignored tests across 482 result
groups. Formatting, strict all-target Clippy and all-target build passed.
The twenty added Rust regressions cover branded objects, immutable bytes, binary
views, UTF-8/BOM decoding, conversion order, promises, GC/accounting, FileList
identity, event propagation, bounded IPC reads, revisions and document ownership,
ordinary isolated-host startup and finite BlueTS event overloads. They also
reproduce delayed Unix listener readiness and a request arriving
after a nonblocking HTTP fixture accepts its client.

The unfiltered macOS XCTest/XCUITest run executed exactly 264 methods:
263 passed, zero failed and 1 skipped. The sole
physical Zhuyin limitation is checked against the accepted parent scope. All
ten focused methods passed without a skip, including both recovery retry cases. Four added UI methods exercise
ordinary DOM/RegExp scripts, exact selected bytes and ordered events, cancel,
script clear/form reset, retained snapshots, multiple windows and navigation.
Existing native tests additionally verify metadata, real picker interaction,
binary form uploads, editor retention and stale native callbacks.

Actual successful UI images were reviewed and remain ignored by Git. Ten strict
signature checks and ten executable checksums include the isolated BlueJS host
and its RegExp worker. Both Swift app executables contain arm64/x86_64 slices;
runtime acceptance is arm64. Source/product/process checks and fixture removal
passed without leaving an owned native or pipeline process running.

## Evidence and remaining work

The [contract](MACOS_FILE_API_CONTRACT.md) and
[machine-readable receipt](artifacts/macos-file-api-results.txt) record exact
source manifests, native method sets, Rust counts, logs, historical failures,
signatures, cleanup and reviewed images. Retained failures include the initial
missing File API, invalid typed declarations, missing RegExp worker, BOM decoding
and the serial DOM listener blocking a second window. Ordinary File realms now
share one owned connection with each call fenced by its exact tab/document/node;
the earlier proof profiles retain their transport lifetime behavior.
Full validation also corrected a missing launcher Args fixture field and two
protocol-version assertions. A later workspace attempt hit four Gatekeeper
readiness timeouts; both the focused crate and the same workspace test executable
then passed all four cases without source changes. The earlier timeout remains
recorded; its root cause is not established. Another full workspace attempt
reached 6,752 passing cases but four assistant-settings end-to-end startup cases
timed out; the exact workspace executable then passed all four unchanged in
3.57 seconds. Both timeout histories are retained alongside the final complete
workspace run, which uses no-fail-fast to collect every target's result.
A subsequent complete workspace run reached 7,363 passes and seven failures:
one core frontend connected before its socket began listening, five extension
hosts created their sockets about ten seconds after cold launch, beyond their
five-second test deadline, and one HTTP fixture disconnected before receiving
its request. Two deterministic regressions failed before the fixes. Core frontend
readiness now retains a successful connection; subprocess fixtures own their
children before assertions and use bounded startup deadlines. The HTTP fixture
explicitly restores blocking mode on accepted sockets. A focused run reproduced
four cold Gatekeeper private-copy startup timeouts; that fixture now uses the
macOS-only bounded 60-second cold-start deadline, also used by the extension-host
fixture. A later focused run reproduced 27 startup failures, including core
processes whose sockets appeared about 28 seconds after launch. A private
Gatekeeper diagnostic remained at dyld entry before Rust main and connected
after 17.8 seconds, with overlapping operating-system executable scans. The
final related preflight passed all 84 cases across seven result groups. The Unix
readiness regression uses the existing core process-test guard to avoid competing
with unrelated subprocess startup. The unchanged parallel
HTTP executable reproduced premature WouldBlock failures; the production HTTP
transport was not changed. Earlier failure logs and startup timing evidence are
retained in the receipt. The fixed HTTP response suite subsequently
passed 100 repetitions with the original concurrent harness, totaling 1,100 cases.
The first full native run exposed an outdated recovery fixture: its private app
linked only the earlier services and omitted the now-required BlueJS host and
RegExp worker. The run was interrupted after 156 passes and that one failed
method; its failure log and successful owned-fixture cleanup remain recorded.
A separate XCUITest recovery fixture contained the same incomplete service list;
the next full run was interrupted after 183 passes and that one failed method.
Both Swift recovery fixtures now include the two required services. These two
single-line fixture changes are the only changes after the completed Rust run.
All 1,832 other source inputs, including every Rust source and build input, match
that full run. Ten focused native methods and the final complete 264-method
native suite validated both corrected fixtures and the unchanged product.

Blob.stream/textStream, FileReader, object URLs and general page/OS drag-and-drop
remain pending. BlueTS uses FileList.item() because numeric index signatures are
unsupported; host constructor records do not yet infer new expressions. This
milestone does not establish full Web IDL/File API or unrestricted DOM conformance.
Page source review and execution budgets remain required. Linux/Windows, Intel
runtime, remote SFTP/FTPS, distribution signing, storage partitioning and physical
IME/VoiceOver/printer/display acceptance remain in the full delivery plan.
