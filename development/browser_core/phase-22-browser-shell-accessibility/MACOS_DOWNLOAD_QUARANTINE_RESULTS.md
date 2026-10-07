# macOS download quarantine — 2026-10-07

Status: implemented and accepted; complete Rust/native validation and final
source/product/signature/process audit passed. The complete macOS browser
delivery goal remains active. The [contract](MACOS_DOWNLOAD_QUARANTINE_CONTRACT.md)
defines this increment.

The [validation receipt](artifacts/macos-download-quarantine-results.txt)
records completed attempts, diagnostic evidence, log hashes and the current
source manifest, accepted native result bundle, screenshot review and final audit.

The transfer engine installs system-generated quarantine and sanitized Finder
source metadata on its retained file descriptor before publishing the finished
name. Empty responses use the same atomic publication path. Metadata failure
refuses completion and preserves a pre-existing destination and partial bytes.
The native list reports “Origin recorded for macOS” only for a current completed
record and an actual checked regular file within its configured download root.
Ordinary Open, Finder and history removal retain the metadata.

Apple's public quarantine API generates the attribute on a private owner-only
temporary file; public descriptor syscalls copy and verify its exact bytes.
Finder receives a binary property-list source array derived from the final
response URL, without user-info, query or fragment. No quarantine flag format
is synthesized, and normal macOS file opening does not remove quarantine.

## Focused evidence

The public real-transfer regression first failed because a completed empty file
had no quarantine. Final focused Rust validation passed **306 cases, zero
failures and zero ignored across 12 suite groups**. It includes empty and
nonempty redirected responses, actual metadata at completion, privacy redaction,
unsupported attributes, refusal before publication and preservation of existing
and partial data. The existing interrupted-transfer protocol fixture completes
its small history transfer before starting the transfer to interrupt, retaining
its paused-state, byte and generation assertions.

Focused native validation passed **11 methods, zero failures or skips**:
eight XCTest and three XCUITest methods. Real bundled services, the current-file
status boundary, actual quarantine properties and Finder origin, token exclusion,
normal relaunch, history removal and the existing real Open/Finder and transfer
control tests passed. The corrected scheme targets are `ProtocolTests` and
`BrowserUITests`; an earlier invocation used nonexistent target selectors and
ran no methods. Earlier Rust fixture/compiler iterations and that invocation
must remain in the final validation receipt.

The focused native invocation reached terminal exit 0 in 216.943 seconds.
Result bundle: `frontend/macos/.build/results-20261007-093556.xcresult`.
Its 1,805 frozen source inputs have aggregate SHA-256
`edb674d3f5ec391b9ccc7cce2dbeb2b447ccf17e8d2cf7980babc5fe52970e6e`.
The current complete pipeline retains the same 1,805 source members with
aggregate SHA-256
`7c520f4d51b64c7bf17d6e4d0c84c82e1ce6e430bfc2bed72cc0c44e5bd7aaf1`.
Only the two Rust regression fixtures described below differ from the focused
native freeze; production and native test sources are unchanged. Complete
acceptance below passed for this current freeze, using the same Cargo cache.

The focused screenshot was visually reviewed and is ignored by Git:
`artifacts/macos-native-download-quarantine-focused.png`, SHA-256
`4d558679ac6d163c6400a3d7177bf7a79f89c0997972d1646a9781ccea9894f2`.
It shows a completed download, the native origin caption, Open and Finder actions.
Screenshot files and `.build` products must not enter the milestone commit.

## Complete acceptance

Formatting, strict workspace/all-targets Clippy and the workspace/all-targets
build passed. The first complete Rust workspace invocation reached terminal
exit 101 after 7,258 cases passed, one failed and 69 were ignored across 444
completed suite groups. `every_stalled_connection_is_replaced_without_waiting_for_its_read_to_return`
did not complete within its four-second deadline. Its unchanged isolated rerun
passed, as did the entire 51-case transfer target with workspace features.
A 32-thread rerun reproduced two timing failures. The captured public snapshot
showed all 4 MiB fetched, four complete segments, four retries and no active
connections while final completion was pending. The tests now retain their
four-/six-second bounds on fetched bytes and separately require successful
final publication and exact file contents; they do not extend those network
deadlines or skip metadata. The same 32-thread transfer target then passed all
51 cases, and fresh net/downloads validation passed 306 cases across 12 suites.
The complete accepted pipeline below includes this correction.
The second pipeline encountered four real Gatekeeper child startup failures
and was stopped after 25 completed suite groups (1,208 passed, four failed,
one ignored). Its metadata and separate interruption/process-group receipt
are retained; this interrupted invocation is not a complete workspace result.
Only its verified owned process group was terminated, including four orphaned
startup children, and its four owned stale sockets were removed.

Signature verification alone accepted the Cargo linker-signed executable. A
private copy retaining that signature remained alive without readiness for eight
seconds; explicitly signing a private copy reached readiness in 0.408 seconds.
The Gatekeeper fixture now follows the existing launcher/native-bundle workflow:
copy into a private directory and explicitly ad-hoc sign that copy on macOS,
without rewriting a shared Cargo executable. It constructs the child owner
before checking readiness, so a failed startup also runs cleanup. The original
five-second readiness deadline and all mandatory-review assertions remain.
All four real Gatekeeper binary regressions then passed. Fresh combined
Gatekeeper/net/downloads validation passed 347 cases, zero failures and one
existing live-model quality-matrix ignore across 17 suite groups. The third
complete pipeline below passed with this fix.

The third complete Rust workspace invocation reached terminal exit 0 in
3,638.512 seconds: **7,311 passed, zero failed and 69 existing ignores across
474 suite groups**. Formatting, strict Clippy and the all-target build also
passed for the current source freeze. Its complete log hash, ignored test
names and phase result are recorded in the accepted validation receipt.

The complete native suite reached terminal exit 0 in **3,647.647 seconds**:
**219 methods passed, zero failed and one existing physical Zhuyin skip**.
It ran from `2026-10-07T04:17:12.849066+00:00` to
`2026-10-07T05:18:00.497718+00:00` on arm64 macOS 26.6.2, using the same
1,805 source inputs and aggregate above. Result bundle:
`frontend/macos/.build/results-20261007-121735.xcresult`.
The skipped hardware IME method requires Accessibility permission for the test
runner; synthetic string keys cannot establish physical Zhuyin mapping. No
permission was changed. The bundle records 74 internal QoS priority-inversion
warnings; these remain in the receipt and did not produce test failures.

Final audit verified all eight code signatures, universal x86_64/arm64 Swift
applications, all eight native executable hashes and unchanged source inputs.
No pipeline-owned or native-test-owned process remained. The independently
identified pre-existing Cargo parent PID 33538 and launcher child PID 34421
were preserved.

The complete-suite screenshot was visually reviewed and is ignored by Git:
`artifacts/macos-native-download-quarantine-full.png`, SHA-256
`092669c5223675ffe46f911cfa21b16c1190af449f76dc13d1efaf7193ab1c23`.
It shows completed `notes.txt`, the origin caption and usable Open/Finder
controls without clipping. Only the text evidence is committed.

Existing files completed by older versions are not silently modified. The
physical Zhuyin/VoiceOver, printer, Intel runtime and distribution-signing gates
remain separate. No host trust, TCC, security or Local Network settings were
changed, and this increment makes no new Linux/Windows acceptance claim.
