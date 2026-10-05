# macOS live regions and native rotors — 2026-10-05

Accepted on Apple Silicon macOS 26.6.2 (25G83), Xcode 27.0 (27A266a),
Swift 6.4 and Rust 1.96.0. The complete native gate passed **173 cases,
0 failures and 1 physical Zhuyin skip**: 106 XCTest and 67 XCUITest passes.
The host-prepared Rust workspace passed **7,249 cases, 0 failures
and 69 ignored** across 473 suites. All-targets build,
strict Clippy and formatting exited 0. Eight strict bundle/service signature
checks passed; owned parent, private-panel and UI-runner executable prefixes
contained no remaining processes. Parent and private-panel executables are
universal x86_64/arm64; actual execution acceptance is arm64.

The native result is `frontend/macos/.build/results-20261005-171104.xcresult`.
The first complete native execution after the final source freeze exited 0.
Its 54 runtime warnings concern internal QoS priority inversions; none concern
SwiftUI view updates. Native acceptance preceded final Rust acceptance; both
use unchanged source inputs. Focused checks overlap full gates and are not
added to totals. No exclusions, new skips, warning suppression, timeout changes
or automatic test retries were introduced. Exact commands, temporary readiness
runner source, failure history and sorted source hashes are recorded in
[macos-accessibility-live-rotors-results.txt](artifacts/macos-accessibility-live-rotors-results.txt).

Core computes explicit polite/assertive/off live-region updates and implicit
status/alert/log behavior. Atomic/relevant inheritance, busy coalescing, text
and element additions, removals and nested live ownership have regression
coverage. Hidden, inert, private and protected content is excluded from native
announcements and content-derived names. A public value becoming hidden or
protected cannot reappear as a removal announcement. Passwords retain their
accessible labels and secure control roles; their plaintext remains absent
from native inspection. The existing AI inspection tree retains its source
observability contract through a separate, optional native overlay.

AppKit advertises heading/level, link, image, list and button rotors. Searches
follow document order in either direction, support case/diacritic-insensitive
filters, include offscreen targets and do not wrap or mutate the page. AX focus
on a reading target invokes the core's source/document/frame/node-fenced reveal
command through a fresh two-second private exchange. Core minimally scrolls
without navigation, activation, DOM focus changes, selection changes or trusted
input authority. Reading focus remains separate from editor focus. A temporary
frame gap suspends readable focus while preserving same-document identity;
reload, replaced documents and foreign items invalidate old rotor actions.

The announcement bridge baselines newly selected documents, deduplicates
sequence/revision pairs and drops inactive updates without replaying them later.
Polite/assertive map to native low/high priorities. XCTest exercises real AppKit
callbacks against an actual bundled launcher/core, verifies announcement bridge
delivery once, reveals an offscreen heading, preserves editor focus and checks
that only navigation/reload issue HTTP requests. XCUITest checks Chinese/emoji
editing, private text exclusion, tab retention and document reload. These checks
do not establish physical VoiceOver speech or interactive rotor acceptance.

Regression development demonstrated the missing IPC boundary before adding
three wire cases, twelve core notification/reveal cases and one actual core
session case. Six XCTest cases and one XCUITest case extend native acceptance.
The text-relevance regression also failed before the fix distinguished new text
from newly added elements. A signing-runner regression demonstrated that
unconditionally signing a valid executable rewrote its bytes. The runner now
preserves valid signatures and only signs executables failing strict verification.
Tests cover byte preservation, argument/exit forwarding and unsigned signature
preparation; approval to execute an unsigned fixture is outside that test scope.

## Rust startup condition

Default-runner complete Rust attempts failed in existing subprocess tests:
first three BlueJS startup cases, later three assistant cases, and six downloads
cases. The last default-runner repeat again failed three assistant cases. These
runs remain unsuccessful; no failing case is counted as a pass. Samples found
owned child processes at `_dyld_start` with a 96 KiB footprint before Rust main.
The startup-delay cause is not established. Direct diagnostics subsequently
passed the complete core library, assistant suite and original downloads suite.
Downloads' seven original cases passed without rebuilding; test and production
binary hashes were identical before and after that diagnostic.

A separate `cargo test --workspace --no-run` probe established that Cargo
materializes new assistant/downloads file identities despite unchanged bytes
and no compilation. Consequently, readiness before invoking Cargo does not
prepare the binaries subsequently used by that invocation. Final host-prepared
acceptance places a temporary readiness wrapper at the first Cargo runner
callback, after materialization and before the first test executable. Eight
current development binaries must reject an unknown argument while retaining
identical inode and SHA-256. The wrapper then delegates every original test to
the repository signing runner and rejects later binary identity changes.
`cargo test --workspace` retains its complete scope and original subprocess
deadlines. This is an explicit host preparation condition; it does not fix or
claim clean cold-start acceptance for the default runner on this host. No TCC,
security prompt, host trust or service permission is changed.

Rust acceptance freezes **1,735 files** with aggregate SHA-256
`9f394ba8d1aecb963f391417106ba70a910457c2891ad85cac731a021727e0df`.
Native acceptance independently freezes **1,793 source/build/fixture inputs**,
with aggregate SHA-256
`353b9df748012a3a2027c99f1eeda3788042ff3ce9ed4aa4f19c74f89280522c`.
The temporary readiness wrapper is recorded separately in the receipt and does
not mutate these inputs or signed service/test binaries.

## Remaining scope

The current stream carries only the latest layout batch. A subsequent layout,
including a no-op resize, replaces it before a delayed frontend read; retained
announcement delivery and acknowledgement remain pending. Other remaining work
includes initial alerts before the first observed snapshot, descendant-scoped
atomic/relevant overrides, full ARIA/name computation, landmark/table/visited-link/
text-field rotors, document paragraph selection, font fallback and physical
VoiceOver/OS IME. The full contract is in
[MACOS_LIVE_REGION_ROTOR_CONTRACT.md](MACOS_LIVE_REGION_ROTOR_CONTRACT.md);
other browser milestones remain in [MACOS_DELIVERY_PLAN.md](MACOS_DELIVERY_PLAN.md).
This increment does not complete browser delivery, establish new coverage,
distribution signing/notarization or fresh Linux/Windows acceptance.

The unedited window attachment `artifacts/macos-accessibility-live-rotors.png`
was inspected and remains ignored. SHA-256:
`9adbb92590014ca84344d61218e8983129907f5228234752625d8c0ddf98de48`.
It shows the Chinese/emoji editor, retained second tab and password bullets.
The fixture's aria-hidden text is painted while excluded from AX assertions;
ARIA hiding does not imply pixel hiding. Existing Local Network prompts remain
untouched, and physical Zhuyin stays skipped because the runner lacks Accessibility
trust. Screenshots and xcresult bundles remain ignored.
