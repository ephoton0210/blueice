# macOS original response download results

Accepted: 2026-10-08 (Asia/Taipei), on Apple Silicon macOS. Parent commit
`77da8abec358d368976baf5a99b20a1277e25e89`. This increment completes automatic navigation
response downloads; the full browser delivery remains in progress.

Normal Return, link and one-shot POST navigation can download the exact original
response into the selected folder. The active document and history remain intact.
The native shelf shows progress, cancellation, quarantine/source status and
Finder actions. Original responses cannot pause, resume or be replayed; interrupted
records fail and require an explicit new navigation.

## Validation

The fresh complete Rust workspace passed 7,352 cases with zero
failures and 69 ignored tests across 478
result groups. Formatting, all-target strict Clippy and all-target build passed.
Thirty-one new Rust regressions cover header-first binary/attachment classification,
original POST bytes, core document/history ownership, cancellation of the original
HTTP connection, actual delayed/stalled TLS, file-policy/size/length refusals,
queued-source retirement, explicit bounded stream completion, nonreplayable restart
and MCP completion without body authorization.

The unfiltered macOS native/XCUITest run executed exactly 260 methods:
259 passed, zero failed, 1 skipped. The skip set is
checked against the existing physical Zhuyin requirement; it is not inferred from
an exit code. Five new native methods exercise protocol metadata, selected-folder
output, current-document and originating-window ownership, Return/link/POST,
completed binary bytes/quarantine, actual Finder reveal, cancellation, relaunch
without replay and English/Traditional Chinese UI. The final focused run also
passed all five methods without a skip.

Actual final English and Traditional Chinese images were reviewed and are ignored
by Git. Eight strict signing checks passed, both native Swift app executables
contain arm64 and x86_64 slices, eight executable checksums were captured, the owned
SFTP fixture was removed and reaped, and the final source/product/process audit
found no owned pipeline or native process left running. Runtime validation was
on arm64; universal binary slices do not prove Intel runtime behavior.

## Evidence and limits

The [contract](MACOS_RESPONSE_DOWNLOADS_CONTRACT.md) defines the ownership and
publication rules. The [machine-readable receipt](artifacts/macos-response-downloads-results.txt)
records the source checksum manifest, exact method sets, actual complete counts,
logs, signatures, processes, fixture lifecycle and screenshots. It retains the
initial network/endpoint failures, cross-window pane regression, HTTP upstream
cancel failure, interrupted catalog state, queued cancel/shutdown, MCP completion
barrier, compile repairs and TLS fixture failures with their corrections.

The first unfiltered native run recorded 258 passes, one failure and one existing
skip. The failing cross-window drag test stopped during title-bar positioning:
the window reached 760 × 600 at y=40, but x=46 instead of the asserted x=40.
The unchanged focused reproduction passed, followed by the unchanged complete
260-method run above. No source, assertion tolerance or timeout was changed;
the failed run, diagnostic attachment and reproduction remain in the receipt.

Linux/Windows, remote SFTP/FTPS, distribution signing and physical hardware tests
are not accepted by this increment. General page/OS drag-and-drop, File/Blob/FileList
semantics, storage partitioning, remaining permission/accessibility features and
physical IME/VoiceOver/printer/display behavior remain in the full delivery plan.
