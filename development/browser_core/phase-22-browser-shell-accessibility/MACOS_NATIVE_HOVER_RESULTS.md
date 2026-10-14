# macOS native hover results

Accepted: 2026-10-10 (Asia/Taipei), Apple Silicon macOS 26.6.2.
Parent commit `c3494b74a0f9ea5b034d6c5cae5fa6732925b550`. Complete macOS browser delivery remains in progress.

AppKit tracking areas now forward actual pointer movement and explicit leave
to shared core hover state, independently of page keyboard focus and backing
scale. Movement requires the live document and displayed frame. Leave follows
document lifetime across focus and frame changes. Tab selection, chrome entry,
window focus loss, sheets, viewport changes and view removal clear hover.
Both cross-window tab handoff paths clear the old window's pointer state;
same-window organization and refused handoff preserve it.

## Regression and native evidence

A new native XCTest first failed both actual core link-hover observations.
Its unchanged declaration-bounded method then passed with the native bridge.
It uses AppKit events and real bundled services with a standalone simulated
key-window guard. The separate XCUITest uses actual OS pointer hover, chrome,
new-tab selection and a second browser window, observing the ordinary core
through an independent read-only connection. It verifies both hovered nodes,
explicit clearing, retained document identity and exact HTTP requests.

A public core test reproduced rejected leave after actual keyboard focus and
frame changes. Pointer context now follows document ownership independently of
keyboard focus; positioned movement remains frame-fenced. Four public tests
cover movement, leave, scrolled content, invalid coordinates and stale document,
frame, source, context and window ownership. A fifth reproduced retained hover
on cross-window handoff. Its complete test file stayed unchanged while exactly
one production file changed, clearing hover during editor transfer for both
MoveTab and cross-window PlaceTab. Old-window commands are rejected.

Earlier observer/setup failures remain in the receipt. The first UI setup used
the wrong main-window identifier. XCUITest Runner then rejected lsof PID lookup
with Operation not permitted; a descriptor probe returned no usable list,
without establishing its specific kernel cause. A direct private Unix-socket
observation also returned EPERM. The existing token-authenticated host fixture
now checks exact built GUI/launcher paths, their parent relationship and private
runtime ownership before read-only Hello/GetRepresentation requests scoped to
context, window and tab. No security, sandbox, entitlement or accessibility
setting changed. The actual OS hover method passed before expanded acceptance.

## Complete acceptance

Public focus passed all 74 keyboard, viewport and window tests. Expanded native
focus passed six XCTest and six XCUITest methods, without failures or skips.
Formatting, strict all-target Clippy, all-target build and the fresh Rust workspace
passed: 7,406 passes, zero failures and 69 ignored cases across 483 result groups.
Fresh unfiltered native acceptance executed exactly 280 methods: 157 XCTest
passes, 122 XCUITest passes, zero failures and the same physical Zhuyin skip.
All 278 parent methods remain present, and all 122 parent UI method spans are
unchanged. Raw native logs and structured xcresult trees agree on exact scope.

Every final gate used the same 1,837 unchanged program/build inputs:
`a45ee4373438b2e808e70ff451793360d42a4e4e4f8a7fe0fdddcddc04c9d6f0`. All five new public regressions passed in the full
workspace run. No earlier gate or source-manifest exception is substituted for
this increment's fresh acceptance.

## Product and cleanup evidence

All 11 strict signature checks passed. Product hashes include the actual BlueIce
debug GUI dylib; both Swift app executables and that payload contain arm64 and
x86_64 slices. Actual runtime and service acceptance are arm64. Owned SFTP/update
helpers recorded group, process and private-root cleanup. The full update case
exercised the actual staged-core lifecycle. The final audit found no owned
processes and preserved the unrelated preexisting launcher test's timestamp and
parent proof. xcresult bundles and verification pictures remain ignored and
untracked.

The [contract](MACOS_NATIVE_HOVER_CONTRACT.md) and
[machine-readable receipt](artifacts/macos-native-hover-results.txt) preserve
commands, dates, log hashes, exact scopes, failed attempts, unchanged-test proofs,
source inputs and product/process evidence. This increment establishes passive
core hover state. DOM pointer events, CSS pseudo-class rendering and remaining
software, distribution and physical requirements remain open in the
[delivery plan](MACOS_DELIVERY_PLAN.md). No fresh Linux or Windows acceptance is claimed.
