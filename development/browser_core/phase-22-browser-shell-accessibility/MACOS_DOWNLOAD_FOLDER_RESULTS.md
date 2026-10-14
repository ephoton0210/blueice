# macOS download folder selection results

Date: 2026-10-08 (Asia/Taipei). Accepted scoped increment; the complete macOS browser goal remains
in progress. Base commit: `2ad46935cac13195d1550e66a77f0007dc60d708`.
The [behavior contract](MACOS_DOWNLOAD_FOLDER_CONTRACT.md),
[machine-readable record](artifacts/macos-download-folder-results.txt) and
[delivery plan](MACOS_DELIVERY_PLAN.md) retain the scope and remaining work.

## Implemented behavior

Downloads > Folder supplies a SwiftUI draft and a real AppKit directory picker.
Explicit Apply selects a readable/writable local directory or restores the
default. Cancel and dismissal preserve the saved configuration. English and
Traditional Chinese controls are covered by actual XCUITest interactions.

The replacement download service preserves transfer IDs, original folders,
completed files and partial bytes, with unfinished work paused until explicit
Resume. New transfers use the selected folder. The browser core stays running;
old credential reviews become stale. Native Open/Finder and quarantine checks
continue to validate completed files against explicitly retained roots.
Existing selected-folder permissions remain unchanged.

Stored roots supply provenance; explicit startup arguments supply authorization.
An omitted or unavailable original root refuses startup before catalog changes.
Malformed/future preferences also refuse startup. Explicitly choosing the
original folder recovers history before another destination is applied. No files
are moved, and unavailable-volume recovery is outside this increment.

## Actual acceptance

The normal, unfiltered `frontend/macos/test.sh` ran from
`2026-10-07T15:06:34.773997+00:00` to `2026-10-07T16:22:38.056972+00:00` in 4563.260 seconds.
The actual result bundle is `frontend/macos/.build/results-20261007-230654.xcresult`.

| Gate | Actual result |
| --- | --- |
| Complete native suite | 254 passed, zero failed, 1 existing physical Zhuyin skip |
| Method scope | Exact 255 unique methods: committed SFTP baseline 246 plus nine new methods; no missing, extra or duplicate cases |
| Final focused suite | Nine passed, zero failed/skipped, 242.289 seconds |
| Fresh Rust workspace | 7,321 passed, zero failed, 69 ignored across 475 groups |
| Rust prerequisites | fmt exit 0, clippy exit 0, build exit 0, test exit 0 |
| Source consistency | 1,817 inputs; focused, fresh Rust and full native aggregate `76e37e53225f565d59c56622bc3ccb1a0b894b04bea97189fe178e29a6b00680` |
| Signatures and architectures | Eight strict verifications passed; two Swift apps include arm64 and x86_64 |
| Product/process audit | Eight executable hashes; zero owned native, pipeline or fixture processes |
| Fixture cleanup | Helper exit zero, both owned process groups gone and private root removed |
| Screenshots | Two actual English/Traditional Chinese images visually reviewed and ignored |

This increment changes Rust; acceptance uses a fresh complete Rust workspace
execution, not carried evidence. Actual warning counts, skip reasons, method
lists, frozen inputs, signatures and process ownership are retained in the record.
The full native log contains 81 warning lines.
Accessibility/TCC, Keychain ACLs, runner sandbox and system/user SSH settings
were unchanged.

## Test development and preserved attempts

A regression first demonstrated that selecting an existing mode-0750 directory
changed its permissions; creation now applies mode 0700 only to new directories.
The first native attempt passed 33 cases and failed two assertions that compared
canonical URLs with different trailing-slash forms. Path comparisons and
canonical-path deduplication fixed that issue; the second focused attempt passed
eight cases. Those attempts remain in the receipt.

An owned catalog probe then reproduced loss of completed-history metadata when
repair omitted its original root. The public protocol regression failed before
the fix. Manager startup now rejects that configuration before persisting any
catalog change. The complete 56-case protocol suite passed, and the final native
nine-case scope includes actual unsupported-preference repair, refusal with
unchanged catalog/payload, explicit original-folder recovery and later folder
selection. The incomplete earlier Rust run was intentionally interrupted and
reaped after this finding; it supplies no full-workspace acceptance.

Actual UI coverage includes old completed files, paused ranged transfers and
exact resumed bytes in their original folder; exact new bytes in the selected
folder; Finder access; unchanged directory permissions; relaunch without
automatic transfer; explicit default restoration; quarantine; picker cancellation
and dismissed drafts. Final acceptance uses only the corrected source freeze.

The first process audit found one temporary core child left by the fresh Rust
run after its temporary root was removed. Exact PID/start/group verification
preceded SIGTERM, the child exited, and the two preexisting Cargo processes
remained unchanged. The failed audit and cleanup evidence remain in the receipt;
the final repeated process audit passed with zero owned processes.

## Remaining browser delivery

Automatic response downloads, remote SFTP/FTPS interoperability, vector PDF,
physical printers/IME/VoiceOver, full storage partitioning, additional
accessibility and the other delivery milestones remain open. Historical folders
must remain available. The earlier family-emoji missing-glyph rendering remains
unresolved. No new Linux/Windows or Intel runtime acceptance is claimed; local
ad hoc signing does not establish distribution signing.

Reviewed ignored screenshot `development/browser_core/phase-22-browser-shell-accessibility/artifacts/macos-native-download-folder-final1.png`: SHA-256 `2e5d5a30d2769fb2beb80fed54cc4177db4a8e54d8989579c317cb5c7deb4de8`.

Reviewed ignored screenshot `development/browser_core/phase-22-browser-shell-accessibility/artifacts/macos-native-download-folder-zh-final1.png`: SHA-256 `462531ffbcb38c3d62abe1ecb895f640112249868d3a328710f7aa0d11873cd7`.
