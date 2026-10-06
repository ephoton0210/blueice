# macOS descendant live-region semantics — 2026-10-06

Accepted with **178 native passes, zero failures and one existing physical
Zhuyin skip**, and **7,279 Rust workspace passes, zero failures and 69 ignored**
across 473 suites. Formatting, strict Clippy and all-targets build passed.
Eight strict signature checks and owned native process cleanup passed; the
parent/private-panel executables are universal x86_64/arm64, with arm64 execution.

Each public contributor now carries its effective relevant set and nearest atomic
boundary. Child relevant settings replace inheritance; suppressed edits refresh
the baseline without later replay. Removals retain their original relevant scope.
True boundaries expand their public group once, including its author label;
explicit false stops outer expansion. Multiple activated groups and ordinary
updates retain document order, and an activated parent covers simultaneous inner
updates without duplicate content. Nested live owners remain independent.

The native name and atomic announcement paths share bounded public
`aria-labelledby`/`aria-label` resolution with reference priority and contributor
provenance. External reference changes refresh the next name without producing a
live notification alone. Hidden/private transitions purge retained label text.
Live-off content inside the region cannot enter its label; an external public off
reference remains a readable name. This is a public-content subset, excluding
hidden references, not the complete AccName or ARIA implementation.

Busy regions coalesce current public contents/labels after readiness. Unicode
clipping stays attached to its retained event until prefix acknowledgement.
Collection allows 256 live regions and 256 atomic groups, 50,000 visited nodes,
depth 256, 4,096 Unicode scalars per region/event and 64 retained events. Excess
atomic groups are omitted and reported rather than emitted as partial non-atomic
updates. See [the contract](MACOS_DESCENDANT_LIVE_CONTRACT.md).

## Validation

The test-first baseline passed the 21 previous engine cases and failed all 12
new descendant regressions. A later extension found two ordering/duplication
failures (35 passed, two failed), following one corrected test API compile error.
The final focused suite passed 38 cases. The three actual-core IPC cases passed
with the repository signing runner and local loopback fixture. The first sandboxed
IPC invocation failed at `TcpListener::bind(127.0.0.1:0)` with EPERM, before any
core started; the successful unsandboxed invocation used unchanged source and
deadlines. Both results are retained in the text receipt.

The complete native gate passed 110 XCTest and 68 XCUITest cases, with one existing
physical Zhuyin skip. New actual-service AppKit AX edits verified grouped/narrow/
suppressed announcements, privacy, deduplication, ACK readback, focused editor,
tab isolation and reload. The actual-window flow verified Unicode values, password
redaction, retained tabs and reset on reload. Its result bundle is `/Users/ephoton/git/blueice-downloads/frontend/macos/.build/results-20261006-095319.xcresult`;
terminal completion and attachment export preceded result inspection.
There were 55 internal QoS runtime warnings and
0 concerning view updates; their original messages are retained.

Two incomplete workspace attempts are retained as failures. The first ended with
6,984 passes, one MCP socket-fixture bind failure and 69 ignored across 430 suites.
The fixture used PID plus wall-clock time without a uniqueness guarantee. A new
fixed-clock regression reproduced the collision, then passed with a process-local
sequence and owned socket cleanup; the complete MCP unit suite passed 158 cases.
This test-only correction was separately committed and pushed as
`6ba632e2b3e96cae8c028f7eddd1b285b44747af`. The original failing clock value was
not captured, so the fixed-clock result demonstrates the naming hazard without
establishing that exact timestamp as the cause of the original bind failure.

The second workspace attempt ended with 53 passes and three Assistant listening
timeouts across three suites. The unchanged service then created its socket in
0.11 seconds, and all eight original Assistant binary
tests passed with unchanged executable inodes, hashes, signatures and deadlines.
The final workspace invocation followed this readiness observation. The startup
failure's cause remains unproved; successful testing does not establish pristine
cold-start reliability.

The complete workspace used Rust 1.96.0 and the normal repository signing runner,
with the single `frontend/macos/.build/core-target` cache and no auxiliary readiness
helper after Cargo started or changed deadlines. It took 4523.467
seconds. Focused counts overlap the workspace and are not added to its total. No
new ignore, skip or test exclusion was introduced. This validates the recorded
host state after the startup observations described above.

The native run froze 1793 inputs with SHA-256 aggregate
`f92f083005fee8b098e48e8a9a05356a165f034ee6f85551ad888408f61a7b30`. The final workspace froze 1793 inputs
with aggregate `c2e783dc850a4771e4708d8959d6f7167f43d9bdbdac7d68bb9d9d7859611d89`; its 1735 Rust input subset has
aggregate `3f08d0fad5d285af5846c50b6af7798683c25d0a5097707ff7ecb73ecc78fdc1`. Exactly one broad-scope input changed after native
testing: `backend/mcp-server/src/assistant_settings.rs`, inside its `cfg(test)`
fixture module only. Its production prefix remained byte-identical; this crate
is not a bundled native service. Application, core, native-test and HTTP-fixture
inputs remained identical, and all eight native executable hashes remained
unchanged. This scoped exception is recorded rather than claiming one identical
global source snapshot. Full command,
source-hash, failure, summary and audit receipts are in
[macos-descendant-live-results.txt](artifacts/macos-descendant-live-results.txt).

The ignored validation image is `artifacts/macos-descendant-live-regions.png`,
SHA-256 `05290ba80c20abf57beb116954bf103d0fa81de5bbcffb4d01f3fc076df0d1bc`. It is an XCUITest window attachment, not font/pixel-quality
or physical screen-reader acceptance. The `aria-hidden` fixture text remains
visually painted and is excluded from AX; password contents remain redacted.
No host trust/TCC or security prompt was
changed, and this milestone makes no new Linux/Windows acceptance claim.

Initial alerts, remaining ARIA/name semantics, additional rotors, document text
selection, physical VoiceOver and other browser requirements remain in the
[macOS delivery plan](MACOS_DELIVERY_PLAN.md). Consumption ACK means native
consumption, not physical speech completion.
