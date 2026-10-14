# macOS interface localization — 2026-10-06

Implementation, focused/full native validation, complete Rust workspace and
final source/product acceptance are complete.
See [the contract](MACOS_LOCALIZATION_CONTRACT.md).
Commands, attempts, log hashes, result summaries, process scope, signatures and
the complete final input manifest are retained in
[the validation receipt](artifacts/macos-localization-results.txt).

English and Traditional Chinese tables cover native browser controls and private
permission/assistant panels. Follow macOS and explicit language choices persist
through the owner preference domain. Existing windows and the private panel
observe changes without recreating their core pages or authorizing a decision.
Native startup configures only the app's own language preference; standard macOS
menus/dialogs update on restart. URLs, page content, authored names, capability
tokens and reviewed setting values retain their original data.

## Development evidence

The first native UI regression failed because the Back button still displayed
English. The first implementation build failed because the distributed
notification publisher requires a class object; its domain now bridges to
NSString. The next run passed all four resource/preference/format tests but
exposed an empty AX label on the appearance picker and a fixture startup-language
mismatch. Explicit picker labels fix the native accessibility issue. The retained
failure hierarchy showed Chinese controls and standard menus while the fixture
searched for English Edit. The sandboxed UI runner's preferences are separate
from the app's store; startup language is now an explicit app option rather than
a runner-side preference write.

The following run passed Chinese chrome/settings/multi-window UI. Its runtime
switching flow preserved the editor and page, updated the private panel in both
languages and successfully reopened in Chinese, but its last preference assertion
read the runner's unrelated store. It now reads the app's actual isolated
preference file using the existing session fixture boundary. Cleanup removes
only the UUID-named owned test file after the app stops. Startup standard menus
and immediate browser controls have distinct, visible restart behavior.

Typed template formatting retains existing ungrouped numeric metadata. The
original Retina test still compares visible pixel dimensions to its screenshot;
its assertion is unchanged. Assistant settings show localized backend titles,
while the reviewed protocol values stay unchanged. Existing UI expectations
for the disabled backend now read Off rather than the raw token none.

The latest focused invocation passed **eight methods, zero failures and zero
skips**: four localization XCTest methods, Chinese chrome/settings/menu/multiple
window UI, live language switching/private-panel/editor/persistence UI and the
existing Retina and assistant settings review/cancel/apply/relaunch UI cases.
It took 188.976 seconds, froze 1799 inputs and reached terminal completion and
attachment export with no source changes. Aggregate SHA-256:
`da280e9b3b8c8910cadcea374154186b8f8de9fa7aab0adfd10787b36dc5a45e`.
Bundle: `frontend/macos/.build/results-20261006-201724.xcresult`.
The later README update is documentation only; full acceptance will freeze the
complete final candidate. Earlier failed attempts remain recorded separately.

## Complete native acceptance

The complete `bash frontend/macos/test.sh` invocation passed **190 methods,
zero failures and one existing physical Zhuyin skip**: 119 XCTest and 71
XCUITest methods passed. The UI runner lacks Accessibility trust for actual
Zhuyin hardware events; no test was excluded and no host trust was changed.
The signed Rust runner harness, service build, universal native build, full
Xcode test run and attachment export reached terminal completion with exit 0
in 2962.502 seconds. Bundle:
`frontend/macos/.build/results-20261006-203101.xcresult`.

All 1799 inputs stayed unchanged, matching the accepted static gates:
`8fc9ef6de4d86065b0b482fdf32d252b85bbc5811fa4ded6fa433f6f5f9b3e13`.
Eight strict signatures passed; both the parent and private panel contain arm64
and x86_64. No owned native app or UI-runner process remained. Eight executable
hashes were retained for comparison after the workspace run.

The result bundle records 57 identical internal QoS priority-inversion warnings.
It records no failed tests; the run log contains no SwiftUI view-update publishing
warning. This acceptance does not claim a warning-free runtime.

The fresh Chinese Settings/multiple-window screenshot was visually checked and
copied to ignored `artifacts/macos-localization.png`; its SHA-256 is
`36b69da59914699b2a6f6bc74b709850b32eb71e6f805667ed4b027070898432`.
It shows Chinese menus, settings and window chrome while the page retains its
original text. Xcode results and screenshots remain outside tracked changes.

## Static gates

The same final input aggregate passed 984 focused engine/IPC/context-menu tests,
`cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets -- -D warnings` and
`cargo build --workspace --all-targets`. These commands used Rust 1.96.0, the
normal signed test runner and the single existing native core target directory.
Both 338-entry source localization tables also passed `plutil -lint`.

## Complete workspace and final acceptance

`rustup run 1.96.0 cargo test --workspace --no-fail-fast` passed **7302 cases,
zero failures and 69 existing ignored tests across 473 result suites**. It
completed with exit 0 in 2927.142 seconds. The launcher owner-lifetime suite
passed all five cases during this invocation. The run used the existing
`frontend/macos/.build/core-target` and normal
`frontend/macos/TestSupport/run-signed-test.sh`; no readiness helper, retry,
deadline change or exclusion was introduced.

The workspace retained the same 1799 inputs and aggregate as complete native
and static acceptance. Final strict signature and architecture checks passed;
all eight native executable hashes match the retained post-native snapshot.
No source or native product changed during either complete suite.

An initial expanded cleanup scan counted an older Rust unit-test executable
solely because it shares the target path. Its PID 34421 and parent Cargo PID
33538 both started on 2026-10-03 local time, before this increment's first
invocation; the parent's command is a distinct locked/offline four-crate test.
The corrected audit retains that ancestry/start evidence and leaves both
processes untouched. No process from this increment remained. Both audits are
preserved in the receipt; the first scope assertion is not treated as accepted
cleanup evidence. The complete native, workspace and static invocations remain
unchanged and passed without a rerun.

Physical VoiceOver/IME, other browser delivery requirements and Linux/Windows
acceptance remain outside this increment. No host trust/TCC/security prompt or
global macOS language setting was changed.
