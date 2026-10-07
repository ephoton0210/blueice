# macOS address-bar search — 2026-10-07

Status: implemented and accepted; complete native regression passed 230 methods,
zero failed and 1 skipped. The full macOS browser delivery goal remains active.

Enter/Go resolves ordinary search text using one persisted shared provider and
submits the generated URL through the normal core/Gatekeeper navigation path.
Typing sends no request. Explicit URLs retain navigation, including opaque
schemes; a leading question mark forces a query. Exact UTF-8 percent encoding
preserves literal plus, CJK and emoji, while bounded invalid input or provider
settings refuse search. Native Settings offers DuckDuckGo, Google, Bing and a
custom endpoint/query field with the same Save action for pointer and Return.

The isolated source checks and six-method headless XCTest history include
unsuccessful Boolean preference fixtures and four RED opaque-scheme assertions.
The corrected headless run passed six methods; its source precedes the final
Save keyboard shortcut, so it is historical evidence. Warnings-as-errors
app/XCTest and UI-source compiler checks passed on the final candidate. The
pure resolver CLI passed 25 cases. Real-window acceptance is recorded separately.

The search-only integration preserved all 1,701 backend/Cargo/toolchain inputs
from quarantine acceptance. The complete 7,311-pass, 69-ignore Rust baseline may
be carried only while these inputs and their source membership remain unchanged.
Complete native regression, strict Clippy, formatting, all-target build,
screenshot review and source/product/signature/process audit passed.
Screenshots and `.build` products are ignored by Git.

Physical Zhuyin/VoiceOver, Intel runtime, distribution signing and other open
browser delivery requirements remain separate. No host Accessibility, TCC,
security or Local Network settings were changed. No new Linux/Windows acceptance
is claimed for this macOS increment.

The first integrated native focus reached terminal exit 65 after 192.423 seconds:
eight methods passed and three failed. The failures were a missing localized
Picker accessibility name, querying the status by a nonexistent text label,
and assuming a denied navigation leaves the Back control disabled regardless
of its prior history. The corrected regression checks actual preserved-page/history
navigation and unknown-provider refusal after an ordinary URL navigation.
These failures and their result bundle are retained; no focused acceptance is
claimed for this attempt.

The extra static invocation initially selected Homebrew Rust 1.98 from PATH.
Its formatting and Clippy phases passed, but its all-target build was stopped
before completion. Only its verified owned process group was terminated; the
interruption receipt records zero remaining members. This attempt is historical
and does not establish the project's Rust 1.96 acceptance. The replacement
invocation explicitly uses `rustup run 1.96.0`; native build.sh uses the same
version and Cargo directory. The two Swift repairs were applied before the
second native focus.

The second native focus reached terminal exit 65 after 455.654 seconds:
nine passed, two failed. Actual Gatekeeper/history preservation passed and the
localized Picker name passed. The remaining failures used the sandboxed runner's
UserDefaults suite instead of the application's suite: the invalid-draft read
returned nil, and the unknown setting was not seeded into BlueIce. The existing
storedPreferences helper explicitly documents this process boundary. The corrected
fixture supplies the unknown value through the application's argument preference
domain and verifies the visible provider before submission. A normal exit and
fresh application settings window verify that a rejected draft is not durable.
The six-method unit suite covers malformed stored schema and value types.

The third native focus reached terminal exit 0 in **197.761 seconds**:
**11 methods passed, zero failed or skipped** (six XCTest, five XCUITest).
It uses 1,808 source inputs, aggregate SHA-256
`ee5ff202674475f0ced5cc6126492a19faefce56d7e768750460c269f48a1c08`.
Result bundle: `frontend/macos/.build/results-20261007-135149.xcresult`.
The focused result and Traditional Chinese settings screenshots were visually
reviewed and are ignored. Query HTTP bytes and AX text preserve the exact family
emoji, but the current core screenshot paints missing-glyph boxes. This increment
accepts query resolution and native controls; emoji glyph rendering remains open.

Rust 1.96 formatting, strict workspace/all-target Clippy and all-target build
reached terminal exit 0. Only the native UI fixture differs from those completed
static inputs; all production and Rust inputs remain identical. The complete native suite
below passed with the same accepted focused source freeze; final product/source/signature/process checks also passed.

## Complete acceptance

The complete native invocation reached terminal exit 0 in 4028.811
seconds: **230 passed, zero failed and 1 skipped**. It ran from
`2026-10-07T05:55:17.450224+00:00` to
`2026-10-07T07:02:26.161579+00:00` on arm64 macOS 26.6.2.
Result bundle: `frontend/macos/.build/results-20261007-135520.xcresult`.
All 1,808 source inputs match the focused freeze, aggregate
`ee5ff202674475f0ced5cc6126492a19faefce56d7e768750460c269f48a1c08`.
The only change from completed Rust 1.96 static inputs
is the corrected native UI fixture; production/Rust inputs are unchanged.

The existing physical Zhuyin method requires Accessibility permission for the
test runner and cannot be established with synthetic string keys. No permission
was changed. The bundle records 79 internal QoS priority-inversion warnings; their exact details
and skipped-case reason remain in the [validation receipt](artifacts/macos-address-search-results.txt).
The earlier 7,311-pass/69-ignore complete Rust workspace result is carried only
for the 1,701 unchanged backend/Cargo/toolchain inputs.

Final audit passed eight strict code signatures, both universal x86_64/arm64
Swift applications, all eight native executable hashes and unchanged source
membership/hashes. No owned native or validation-group process remained.
The pre-existing Cargo parent PID 33538 and launcher child PID 34421 were
preserved. Method scope also matches exactly: 220 baseline methods plus all
11 new search methods, with no missing, unexpected or repeated methods.

Reviewed and ignored screenshot: `artifacts/macos-native-address-search-full.png`, SHA-256
`bd510de4134fea4a054a6fe652ed09f2f55e98d45f2b3fa7c080ffbc008a86d6`.

Reviewed and ignored screenshot: `artifacts/macos-native-search-settings-zh-full.png`, SHA-256
`7ccb23b7d24290cbae6c5a56a40fef29448024bc3ec13b5d43027fb337f117c9`.

Screenshots and build products are excluded from the milestone commit. The
observed family-emoji glyph limitation and other browser delivery requirements
remain open. This acceptance establishes native search behavior and integration.
