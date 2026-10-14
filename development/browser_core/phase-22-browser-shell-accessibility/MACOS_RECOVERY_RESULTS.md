# macOS browser service recovery — 2026-10-07

Implementation, full native validation and fresh focused/static Rust acceptance
are complete. The [contract](MACOS_RECOVERY_CONTRACT.md) defines this increment;
the [validation receipt](artifacts/macos-recovery-results.txt) retains all 12
native attempts, commands, log hashes, final source manifest, result summary,
signatures, product checks and process audit. The complete browser delivery plan
remains active.

A failed supervised stack now leaves a visible, keyboard-operable Restart
action. It reports the confirmed window/tab counts and warns that unsaved page
changes will be lost. Recovery awaits the old owned stack's cleanup, replaces
the workspace, models and native windows, and starts a fresh private session
inside the same GUI process. Repeated failures and failed startup remain
retryable; duplicate activations cannot launch another replacement.

Confirmed bounded navigation metadata is captured in memory even when disk
remembering is off. Recovery restores profiles, groups, windows, selection,
history, zoom and window frames through the existing canonical restore path.
GET pages are reviewed again. POST markers never replay a request or recover a
body. Form-control contents, selected files and transient assistant state are
outside the snapshot. A closed stopped window is removed from recovery, and a
failed/incomplete capture preserves the previous confirmed session. Recovery
does not enable disk remembering or overwrite a malformed archive.

Startup/restoration suspends ordinary shell, page, native editing, AX and menu
input while internal restoration commands proceed. Ownership checks fence old
window callbacks and close tasks. Window/session menus and native Settings
observe the replacement workspace. English and Traditional Chinese resources
include the failure, progress, restart and memory-recovery explanation.

## Acceptance

The full `bash frontend/macos/test.sh` invocation passed **217 methods, zero
failures and one existing physical Zhuyin skip**: 130 XCTest and 87 XCUITest
methods passed. No method was excluded. Build, signing, testing and automatic
attachment export reached terminal exit 0 in 3570.708 seconds. Result bundle:
`frontend/macos/.build/results-20261007-081428.xcresult`.

The focused recovery gate passed 17 methods: 12 XCTest and five XCUITest.
It exercises two consecutive owned permission-child faults, keyboard restart,
the unchanged GUI PID and new launcher groups, multiple windows/tabs, active
profile/group ownership, history/selection/150% zoom, unsaved-editor refusal,
stopped-window closure, no archive while remembering is off, POST refusal,
pending assistant result/instruction isolation, failed startup/retry, duplicate
rejection, old-owner rejection and incomplete capture retention. Existing native
editing, keyboard focus, settings, printing, downloads, profile/session,
localization and permission regressions also passed in the complete suite.

Test-first evidence includes a genuine missing Restart failure and a separate
restoration race that created an extra tab and fetched `/unexpected`, plus a
stopped native window that could not close. The receipt also records unsuccessful
AX-identifier, stale-menu, fixture-packaging, build and asynchronous-expectation
iterations. All owned jobs reached terminal results before source edits or the
next native/Rust job; one cache was reused throughout.

Execution used an arm64 Mac mini, macOS 26.6.2 (25G83), Xcode 27.0 (27A266a),
Swift 6.4 and Rust 1.96.0. Both Swift app products contain arm64 and x86_64;
execution acceptance is arm64. The Xcode summary records 218 total tests, zero
expected failures and 74 identical internal QoS priority-inversion warnings.
The final log contains no SwiftUI view-update/background-thread publishing
warning. Runtime acceptance is not warning-free.

The frozen 1,804 inputs have aggregate SHA-256
`c828d5e426e04f6b9096b530dea342fe3faee3c00b28c051ad0da6b897f2e18c`.
Membership and bytes match the full native invocation and all four fresh gates:

- Engine/IPC: 1,004 cases passed, zero failures/ignored across eight suites.
- `cargo fmt --all -- --check`: passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo build --workspace --all-targets`: passed.

All 1,697 backend inputs match the prior accepted freeze at `6329d65df` and
the carried `c4070298c` Rust baseline: 7,307 passes, 69 ignored across 474 suites.
That complete Rust workspace result is carried evidence, not a fresh full
workspace test for this Swift-only increment. Eight strict signatures passed
before and after the fresh gates, all eight native executable hashes were
unchanged, and no owned native process remained. The separately identified
pre-existing Cargo child and parent were left untouched.

Both final screenshots were visually checked and copied to ignored artifacts:

- `artifacts/macos-browser-service-stopped.png`: visible failure/counts/Restart,
  disabled page controls and the last rendered page. SHA-256:
  `ae617d10e57d1854d138caceb9da0d3b29e2da684eec6c5b7ff9ba75eadc7fb7`.
- `artifacts/macos-browser-service-recovered.png`: restored group and selected
  history entry, enabled native controls and 150% zoom. SHA-256:
  `883830597c92d6e775e6d510fe2f62a97aa66fdeef5fa243bda9cb6c50a9bb6e`.

Physical Zhuyin remains skipped because the runner lacks Accessibility trust.
No host trust, TCC, security or Local Network approval was changed. Physical
VoiceOver and remaining browser requirements stay open; this increment makes no
new Linux/Windows acceptance claim.
