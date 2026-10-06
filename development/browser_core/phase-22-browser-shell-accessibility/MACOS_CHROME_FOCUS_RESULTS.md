# macOS browser chrome keyboard focus — 2026-10-07

Implementation, complete native validation and fresh static/focused Rust
acceptance are complete. The 27 native attempts, commands, log hashes, result
summary, signatures, source/product checks and process audit are retained in
[the validation receipt](artifacts/macos-chrome-focus-results.txt).
The complete browser delivery plan remains active. See
[the focus contract](MACOS_CHROME_FOCUS_CONTRACT.md).

Tab and Shift-Tab now traverse enabled rendered notices, tabs/groups, toolbar,
native find, core page, assistant and zoom controls. The loop wraps, shows an
accent focus indicator, scrolls focused tabs into view and recovers at a
surviving control when a focused pane or tab closes. Space/Return invoke the
normal control action. Native menus, sheets and the owned permission child
retain their ordinary keyboard behavior.

Core still owns DOM focus order and reports the direction of a page exit.
Native address/find/assistant editing retains its text, selection and input
method behavior. Focus changes retain following keys in a bounded queue;
model, tab, document, readiness and native-window ownership fence their replay.
Cmd-L/Cmd-F supersede pending moves. Closing/resigning a window, opening a modal
or changing service readiness discards old shell/page keys. Enabled notice
controls remain keyboard operable after the service connection ends.

## Final acceptance

The complete `bash frontend/macos/test.sh` invocation passed **210 methods,
zero failures and one existing physical Zhuyin skip**: 126 XCTest and 84
XCUITest methods passed. All nine new chrome UI methods and both new native
editing methods passed, together with the strengthened offline, translation and
reverse-page-boundary regressions. No method was excluded. Build, signing,
testing and automatic attachment export reached terminal exit 0 in 3377.617
seconds. Bundle: `frontend/macos/.build/results-20261007-062256.xcresult`.

Execution used an arm64 Mac mini, macOS 26.6.2 (25G83), Xcode 27.0 (27A266a),
Swift 6.4 and Rust 1.96.0. The parent app and private panel contain arm64 and
x86_64; execution acceptance is arm64. Physical Zhuyin hardware events remain
skipped because the UI runner lacks Accessibility trust. No host trust, TCC,
security or Local Network approval was changed. AppKit marked-text preservation
and commit passed independently; that does not establish physical IME acceptance.

The frozen candidate contains 1,804 inputs with aggregate SHA-256:
`aecc5572a814c26fbb30a9aa70544c6d89fc2ca2e4d7d892b1363c24d6745e27`.
Input membership and bytes match complete native and all four static gates.
The Xcode summary records 211 total tests, no expected failures and 71 identical
internal QoS priority-inversion warnings. There is no SwiftUI view-update
publishing warning in the final log. This is not warning-free runtime acceptance.

Both final screenshots were visually checked and copied to ignored artifacts:

- `artifacts/macos-chrome-keyboard-focus.png` shows the assistant close focus
  outline, preserved `Alice` page input and `Keyboard note` instruction at 100%
  zoom. SHA-256: `4a96b3cb38e3dbf19d42ca6cecc76c78251be53fc152523a464ced8a575992a1`.
- `artifacts/macos-chrome-scrolled-tab-focus.png` shows the selected seventh
  tab revealed at the strip's right edge with its close focus outline and
  unchanged editor page. SHA-256:
  `d7d1b2d7294ac879b1cdfd5aee894db4e44b9d9a13e5e57004e95b1877b6c770`.

## Static, source and product acceptance

Fresh invocations passed **1,004 engine/IPC cases across eight suites** (zero
failures/ignored), `cargo fmt --all -- --check`, strict workspace Clippy with
`--all-targets -- -D warnings`, and `cargo build --workspace --all-targets`.
All four reached terminal exit 0 with unchanged inputs. Only one owned
native/Rust job and the existing `frontend/macos/.build/core-target` were used,
with the normal repository signed runner.

All 1,697 backend inputs match accepted commit `c4070298c`. That increment's
complete Rust workspace passed 7,307 cases with 69 ignored across 474 suites.
This is an unchanged Rust baseline, not a newly executed full workspace claim;
the current increment changes twelve macOS Swift/project/README inputs.

Eight strict signatures passed before and after the fresh gates. Both app
architectures remained universal, and all eight native product executable
hashes matched the post-native snapshot. No process owned by this increment
remained. The audit records pre-existing processes separately and leaves them
untouched.

## Development evidence

Actual address Tab/Space first failed to activate Go. Default focus proxies and
an accessibility-press experiment did not resolve it. Focused button/toggle
activation now invokes the original action after the SwiftUI transaction, with
later keys ordered behind it. Native profile menus need no accessibility-press
fallback. The temporary trace and unsuccessful attempts are retained separately
from accepted evidence.

Removing an assistant split pane exposed cached preference entries. Rendered
order is now filtered through actual control lifecycle and enabled state.
Registration belongs to each mounted instance, so an older drag-wrapper view's
disappearance cannot unregister a replacement tab control. Actual-window tests
cover removed-pane recovery and a scrolled selected-tab closure.

Translation updates temporarily disable their control group. The keyboard test
waits for re-enabled controls and then follows the actual recovery position;
both translated/original page checks and persistence assertions remain. Window
activation and the permission child's initial section are asserted using the
actual owning window and rendered controls. The page reverse-boundary test now
traverses the complete chrome before checking native address editing.

The first complete native candidate passed 210 methods with one existing
physical Zhuyin skip. A subsequent regression then exposed an offline gap:
after the uniquely owned permission child exits and the supervisor ends the
connection, a surviving notice could not be dismissed with Tab/Space.
Process ownership is checked with native `proc_pidinfo`; an initial attempt to
spawn `ps` was denied by the XCTest sandbox before reaching the regression.
The native ownership check allowed the next attempt to reproduce the actual
keyboard failure. The readiness fix passed the offline UI case and rendered
availability unit test. A misspelled additional rapid-test selector selected no
method in that focused run; the full final invocation includes the correctly
named rapid-key test.

The unrestricted final invocation re-ran every native method after the readiness
fix. Its source freeze, static freeze and final product audit agree.
Native screenshots, result bundles and build products remain ignored. Physical
VoiceOver/OS IME, remaining browser requirements and Linux/Windows acceptance
remain outside this increment.
