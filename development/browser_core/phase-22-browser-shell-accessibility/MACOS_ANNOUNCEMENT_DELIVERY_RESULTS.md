# macOS retained announcement delivery — 2026-10-06

Accepted with **176 native passes, zero failures and
1 existing physical Zhuyin skip**, and **7,260
Rust workspace passes, zero failures and 69 ignored** across
473 suites. Formatting, strict Clippy and all-targets build exited 0.
Eight strict signature checks and owned native process-prefix cleanup passed;
parent/private-panel executables are universal x86_64/arm64, with arm64 execution.

Core retains at most 64 ready announcements until document-scoped native
consumption, with a 4096 Unicode-scalar limit per event. Reads, resize and later
mutations preserve pending messages. Prefix acknowledgements cannot clear newer
revisions, emit frames or mutate editor focus, selection, scroll, URL or network
review state. Duplicate/older prefixes are idempotent; unsupported versions,
foreign sources/contexts, stale windows/documents and future revisions reject.
Overflow and queued text clipping remain reported until the relevant prefix is
acknowledged. Hidden/protected, removed/disabled regions and nested live-off
contributors are purged; disabling content does not announce it as a removal.
An explicit inner live region continues to override its off ancestor.

The additive `delivery_version` capability defaults to zero for older payloads,
so an older core receives no new acknowledgement command. AppKit keeps its local
watermark, delivers retained batches once, and acknowledges deliberately dropped
baseline/inactive updates. A private background connection coalesces per-tab
metadata and retries only this idempotent operation with 250 ms–8 s backoff
while the originating document remains current. Stop cancels pending work.
Acknowledgement means native consumption, not physical VoiceOver completion.

The complete native result is `/Users/ephoton/git/blueice-downloads/frontend/macos/.build/results-20261005-222359.xcresult`.
Terminal completion and attachment export preceded result inspection. Its
54 runtime warnings concern internal QoS; none concern SwiftUI view updates. The
preliminary 23-case XCTest run overlaps this gate and is not added to totals.
No new skip, scope exclusion, warning suppression, deadline change or automatic
test retry was introduced. The complete gate retains the physical Zhuyin skip.

## Host startup condition and failed evidence

After an eight-hour interval, the exact original IPC and engine executables
started normally and passed 4 and 21 focused cases with unchanged inode/SHA-256.
Those checks overlap the complete workspace and are not added to its totals.
The complete accepted workspace command then used the normal repository signing
runner, without the auxiliary readiness helper. This verifies the current
workspace in the recorded host state; it does not establish a loader fix or
pristine cold-start reliability. No host trust, TCC or existing security prompt
was changed. Earlier incomplete attempts remain preserved independently.

The initial retention red run passed 12 cases and failed three. The ACK interface
first failed to compile. The later clipping regression passed 18 and failed one.
A follow-up Cargo run compiled but was retired before main after more than
16 minutes; an independent unchanged-binary diagnostic was also retired before
cases executed. Samples showed `_dyld_start`, its mapped inode matched the file,
and strict signature verification passed. Those runs remain incomplete and are
not counted as passes, failures of executed cases or extra skips. Their logs and
metadata are preserved separately. The preliminary native compile likewise
failed before the bridge initializer existed; subsequent native acceptance is
reported independently.

The earlier complete-workspace readiness launch and direct engine/IPC launches
were also retired before main with zero cases executed. The readiness workspace
exited 1; the direct Cargo commands exited 101 after explicit SIGTERM. Their
original logs, samples and retirement metadata are retained in the historical
[pending validation record](MACOS_ANNOUNCEMENT_DELIVERY_VALIDATION_PENDING.md).
They are not relabeled as successful. The delayed IPC retirement worker ended
with ProcessLookupError after the child had already been stopped; no second
signal was applied. The host-loader cause remains unestablished.

Native acceptance records 1793 inputs, aggregate
`702334a45d1e495a565e9584645a4a665ab499baacadae4e995cf059bbc22c85`. Rust freezes 1735
inputs, aggregate `dc1ffa7add255e4f647d3499a79ac32e1f502f36402d4fb092e94f6b19d293b3`. After native acceptance, only
the Rust-only `accessibility_notifications.rs` assertion was corrected to
include both new and removed text under `aria-relevant=all` and to reject any
replayed text containing the old value. That cfg(test)-only file is absent from
the production/native build. No production, frontend, native test, build or
fixture inputs changed; all eight tested native executable hashes stayed
identical. The original broader native snapshot and this exact source
difference remain recorded. The final complete candidate source hash is
`90c8c526353430b8faa62b23c517e75bf980479e05ee346ee0068331d05f3131`. Exact commands, original failed
results, final audit, sorted hashes and the repository runner source appear in
[macos-announcement-delivery-results.txt](artifacts/macos-announcement-delivery-results.txt).

The inspected unedited window image `artifacts/macos-announcement-delivery.png`
remains ignored, SHA-256
`912efb8c372f09046903cbb0eaf8b1de1051216dd04b378aa994ad38496a4f2a`.
It shows Chinese/emoji editing, a retained second tab and password bullets.
The aria-hidden fixture text is painted; AX exclusion does not imply pixel
hiding or complete font fallback.

Physical VoiceOver, initial alerts, complete ARIA/name computation, additional
rotors, document paragraph selection and the other browser requirements remain
in [MACOS_DELIVERY_PLAN.md](MACOS_DELIVERY_PLAN.md). This milestone does not
complete browser delivery, measure new coverage, establish distribution signing
or provide fresh Linux/Windows acceptance.
