# macOS native download management

Base: `ee450a3dfd3e7404345fef8551e6d80441f171ea`.
This increment connects the existing manager to the native macOS shell.
Printing and the remaining [delivery milestones](MACOS_DELIVERY_PLAN.md) stay open.

## Delivered behavior

File > Downloads (Command-Option-L) and the toolbar open a SwiftUI panel with
explicit URL and optional file-name entry. Live records show state, byte progress,
speed, connections, ETA, bounded events and segment details. Pause, Resume,
Cancel, Refresh and Remove use the real downloads protocol. Removing a completed
record keeps its file. Open uses the current macOS file handler; Show in Finder
selects the actual file. Both require a current completed record and a checked
regular file inside the configured root; endpoint symlinks and external paths
are refused.

The AppKit link menu adds Download Linked File. Core revalidates the live
tab/frame/document hit target before returning its resolved URL. Downloading
preserves the source tab and clipboard. The service independently applies
ordinary URL, metadata, redirect and resume review. The compiled executable
download denial remains effective. No human permission grant or AI bypass is added.

All native windows/profiles share one lazily started, bundled download manager.
Opening its panel, downloading a linked file or visiting about:downloads starts
the service; ordinary startup does not. Native controls and about:downloads
use the same socket and records. The launcher can select its own managed
Gatekeeper endpoint so downloads share browsing's actual checkpoint. An occupied
endpoint is preserved and refused; the external override remains distinct.
The native core disables its legacy detached download spawner because the GUI
owns the manager and its configuration.

The default file root is ~/Downloads/BlueIce, with a bounded catalog in
~/Library/Application Support/BlueIce/Downloads. Diagnostic options
--downloads-directory and --downloads-data-directory select explicit roots.
The inherited owner pipe requests a checkpoint on GUI death; normal shutdown
stops downloads before core/Gatekeeper. Relaunch restores interrupted records
paused and never resumes automatically. Files/catalog survive runtime cleanup.

UI automation exposed a backend restart defect: the global generation reset
while persisted records retained higher revisions, causing clients to reject
new mutations as stale. The counter now continues from the greatest loaded
revision, including records subsequently pruned from history. The public
restart regression failed before this fix and passes afterward. Native clients
reject unowned replies, stale events and removed-record resurrection, and fence
requests across reconnects. Uncertain mutations are not automatically retried;
Refresh reads current state.

## Verification

Full native acceptance finished at `2026-10-03T08:11:09.370000+08:00`.
The authoritative bundle is
`frontend/macos/.build/results-20261003-074939.xcresult`; the machine-readable
record is [artifacts/macos-download-results.txt](artifacts/macos-download-results.txt).

| Check | Result |
| --- | --- |
| XCTest, including actual bundled services | 62 passed, 0 failed |
| Actual native-window XCUITest | 37 passed, 0 failed, 1 physical Zhuyin case skipped |
| Native bundle total | 100 tests: 99 passed, 1 skipped, 0 failed |
| Six affected Rust packages: engine, IPC, MCP, launcher, reference frontend, downloads | 1,864 passed, 0 failed, 4 existing manual/environment cases ignored across 55 suite groups |
| Post-format full downloads suite | 93 passed, 0 failed; included once in the affected-package total |
| Workspace/all-target build and Clippy with warnings denied | Passed, Rust 1.96.0, locked/offline dependencies and existing cache |
| Rustfmt, whitespace and Xcode project plist | Passed |
| App and four bundled service signatures | Verified local ad hoc signatures |
| Owned app/launcher/core/Gatekeeper/downloads processes after teardown | 0 |
| Swift app / services and native runtime | arm64 + x86_64 / arm64 |

The bundle reports 29 XCTest internal QoS warnings and zero
SwiftUI state-during-view-update warnings. Runtime diagnostics are distinct from
compiler/Clippy results. The four ignored Rust cases are listed in the record.
The focused bundle results-20261003-074637.xcresult passed four XCTest cases and
both new XCUITest cases; failed diagnostic bundles are excluded from acceptance.

Final source fingerprints matched after validation:

- 24 changed Rust/Swift/project/build-script inputs:
  `c13f1c386dfe968702153e5b1ce665389851265a6b5f09fc94b1cd68b7d89fc8`.
- 15 native Swift/project/build-script inputs:
  `e07833adcd3391e66ac2a6d56718caf5aaeb1c452446ad62b71fce7fab160ffc`.

Fingerprints hash sorted relative path bytes, NUL, file bytes, NUL; the record
lists every input. Native inputs remained frozen during the full run; Rust test
formatting was finalized and the complete downloads suite rerun on that source.
Documentation/results are outside these fingerprints. The unedited actual-window
attachment `DE6D66F0-216E-4115-99EE-5D95BCCCFC9E.png` is retained locally at
`artifacts/macos-native-downloads.png`, SHA-256
`1dde1f0421517f9e22c6c7f5d24f000084f488d66130a67094edbb576b0500a9`.
It shows a preserved paused transfer and a completed file after relaunch.
Screenshots/result bundles remain ignored; result text is tracked.

The real-service XCTest verifies lazy startup, actual file bytes, about:downloads
sharing, the compiled .exe denial, progress, pause/resume, cancelled partial-file
removal, orderly exit/reopen, generation advancement and history removal while
preserving the file. Boundary cases cover malformed metadata, fragmented/invalid
framing, wrong request IDs, old revisions, tombstones, confinement and symlinks.

Actual-window tests use ordinary loopback HTTP fixtures with real bundled
launcher/core/Gatekeeper/downloads binaries. They exercise invalid entry, actual
bytes, Refresh, Finder selection and the default TextEdit handler's document
window, pause/resume with eight-MiB bytes, compiled denial, cancel/remove,
normal close/relaunch and paused history. The link-menu case preserves source
page and clipboard, kills only its uniquely identified test GUI, verifies the
durable paused checkpoint, then reopens and cancels without an automatic transfer.
Temporary roots, keyboard source and clipboard are isolated/restored. No
simulated download service or renderer supplies these UI results.

## Reproduction and remaining scope

```sh
export CARGO_TARGET_DIR="$PWD/frontend/macos/.build/core-target"
export CARGO_TARGET_AARCH64_APPLE_DARWIN_RUNNER="$PWD/frontend/macos/TestSupport/run-signed-test.sh"
"$HOME/.cargo/bin/rustup" run 1.96.0 cargo build --workspace --all-targets --locked --offline
# Locally sign compiled Mach-O service executables before process tests on macOS.
"$HOME/.cargo/bin/rustup" run 1.96.0 cargo test \
  -p blueice-engine -p blueice-ipc -p blueice-mcp-server -p blueice-launcher \
  -p blueice-frontend-reference -p blueice-downloads --locked --offline
"$HOME/.cargo/bin/rustup" run 1.96.0 cargo clippy --workspace --all-targets --locked --offline -- -D warnings
"$HOME/.cargo/bin/rustup" run 1.96.0 cargo fmt --all -- --check
frontend/macos/test.sh
```

Automatic navigation/Content-Disposition downloads, an OS destination chooser,
credential/settings UI, quarantine integration, print/PDF, per-profile download
partitioning and private browsing remain separate work. HTTP and the compiled
file-type review were exercised; native FTP/FTPS/SFTP authentication UI acceptance
is not claimed. Physical OS IME and VoiceOver remain separate gates. Distribution
signing/notarization and Intel runtime acceptance remain open. No fresh workspace
coverage measurement or whole-workspace test result is claimed.

No new Linux or Windows acceptance is claimed. Automatic approval review
previously rejected source-snapshot transfer to pb60g.bravotekcorp.cc because
test authorization did not explicitly authorize sending that snapshot. No
snapshot was transmitted; explicit transfer approval remains pending.
