# macOS announcement delivery — prior validation attempts, 2026-10-06

This is the historical progress record before milestone acceptance. It is
superseded by [MACOS_ANNOUNCEMENT_DELIVERY_RESULTS.md](MACOS_ANNOUNCEMENT_DELIVERY_RESULTS.md).
At the time of these attempts, no milestone commit or push had occurred. Earlier
incomplete results are preserved rather than relabeled as successful. The full
macOS browser goal remains incomplete.

The complete native command `frontend/macos/test.sh` exited 0, exported its
attachments and passed 176 cases with zero failures and one existing physical
Zhuyin skip: 109 XCTest and 67 XCUITest passes. Result:
`frontend/macos/.build/results-20261005-222359.xcresult`. There are 54 internal
QoS runtime warnings and zero SwiftUI view-update warning matches. The earlier
focused Accessibility XCTest run passed 23 cases, overlapping the full gate;
it is not added to totals. Physical VoiceOver speech is not established.

Actual bundled-core XCTest confirms retained announcement delivery once and
consumption readback. Actual-window XCUITest covers live-region editing,
privacy, tab state and reload. Eight strict signature checks passed; parent and
private-panel executables are universal x86_64/arm64. Execution was arm64.
The owned app/panel/UI-runner executable prefixes had no remaining processes.

The unedited, inspected screenshot remains ignored:
`artifacts/macos-announcement-delivery.png`, SHA-256
`912efb8c372f09046903cbb0eaf8b1de1051216dd04b378aa994ad38496a4f2a`.
It shows Chinese/emoji editing, a second tab and password bullets. Aria-hidden
fixture text remains painted while AX assertions exclude it; this does not
establish complete font fallback or pixel hiding.

After native acceptance, one Rust-only assertion file was corrected:
`backend/core/engine/tests/support/accessibility_notifications.rs`. Under
`aria-relevant=all`, replacing a text node contributes both new and removed
text; the no-replay assertion now rejects any message containing the old value.
That file is included only under `cfg(test)` and is absent from the production
binaries built by `frontend/macos/build.sh`. No production, frontend, native
test, build or fixture source changed. All eight native executable hashes
remain identical. The original broader native snapshot and the exact test-only
difference are preserved, rather than relabeled as unchanged.

Original native source snapshot: 1,793 files,
`702334a45d1e495a565e9584645a4a665ab499baacadae4e995cf059bbc22c85`.
Final candidate snapshot: 1,793 files,
`90c8c526353430b8faa62b23c517e75bf980479e05ee346ee0068331d05f3131`.
Final Rust snapshot: 1,735 files,
`dc1ffa7add255e4f647d3499a79ac32e1f502f36402d4fb092e94f6b19d293b3`.
Candidate/Rust file sets and bytes match those final snapshots. Only the
recorded cfg(test) file differs from the original native snapshot.

Formatting, strict Clippy and all-targets build passed again after that
assertion correction. Complete Rust workspace validation remains pending.
Early retention tests passed 12 and failed three; the later clipping regression
passed 18 and failed one before queued clipping metadata was retained. The
follow-up Cargo and unchanged-executable diagnostic runs were retired before
main, with no cases executed. A readiness `/usr/bin/env` launch and a Python
startup stub were likewise retired before main. Those results remain incomplete
and are not passes or new test skips. Samples showed `_dyld_start`; cause is
unresolved. No host trust, TCC, existing security prompt or test deadline changed.

The complete workspace invocation using the already-running Xcode Python
framework interpreter was retired at 2026-10-06 00:05:31 Asia/Taipei. Its first
readiness helper remained before Rust main for 2,070.521 s and was sent SIGTERM;
the workspace command then exited 1 after 2,071.77 s. Zero test suites or cases
executed. The helper's mapped inode 1767924401 matched the current cache file,
its SHA-256 remained
`8f91fe935deb685a4002741e2da728a01e4f8e9bd93099fd1c73727317895c67`,
and strict signature verification passed. The narrow startup-service log query
provided no relevant entries; the host-loader cause remains unresolved. This
is an incomplete validation attempt, not a test pass or a new ignored test.
Its original workspace log, readiness row and explicit retirement metadata
remain under `/private/tmp/blueice-ax-delivery-framework-*`.

A direct focused invocation then attempted the final engine unit tests with
the repository signing runner. Compilation completed in 2m16s, but the test
executable also remained at `_dyld_start` before Rust main. After an explicit
SIGTERM retirement, Cargo exited 101 at 00:14:10 Asia/Taipei; total duration was
468.69 s with zero cases executed. Strict signature verification passed. The
queued IPC and actual-core commands did not run. State, original log, sample
and retirement metadata are `/private/tmp/blueice-ax-delivery-direct-focused.json`,
`/private/tmp/blueice-ax-delivery-direct-engine.log`,
`/private/tmp/blueice-ax-delivery-direct-engine-startup.sample.txt` and
`/private/tmp/blueice-ax-delivery-direct-engine-retirement.json`.

Independent IPC notification-protocol validation started at 00:14:49
Asia/Taipei. Compilation completed in 12.89 s, but this executable likewise
remained at `_dyld_start` before Rust main with zero cases executed. It was
explicitly retired with SIGTERM at 00:20:53; Cargo exited 101 after 364.38 s.
The original signing runner verified its signature before exec, and an
independent strict signature check subsequently passed. A delayed retirement
worker then exited 1 with `ProcessLookupError` because the explicit framework
interpreter had already stopped the IPC child; that late worker did not apply
a second signal. Its state and original log are
`/private/tmp/blueice-ax-delivery-direct-ipc.json` and
`/private/tmp/blueice-ax-delivery-direct-ipc-boundary.log`; the sample, actual
retirement and late-worker records are also preserved under the same prefix.
Both direct commands
use the same single Rust cache and frozen Rust inputs, omit the auxiliary
readiness helper and do not change any test deadline. Narrower verification
cannot replace the required complete workspace result.

The final audit after all validation commands terminated again passed eight
strict signature checks, universal parent/private-panel architecture checks,
native executable hash checks and owned native process cleanup. The final
candidate/Rust file sets and hashes remain unchanged. The original broader
native source snapshot still differs only by the recorded cfg(test)-only
assertion correction. No host trust settings, existing security prompt or
unrelated processes were changed. No accepted-results document was generated.

At that point, further runtime acceptance required a macOS test environment
that could start these executables normally. The host-loader cause is not
established; this record does not prescribe a security-service or trust-setting
change. The user's milestone-commit and normal automatic-push authorization
remains valid once the required checks complete.

## Resumed validation, 2026-10-06 Asia/Taipei

After an eight-hour interval, both original executables started normally with
unchanged inode/SHA-256. The independent IPC boundary passed all four cases in
5.255 s. The engine accessibility boundary passed all 21 cases in 2.168 s
(0.59 s inside the test harness), including the final clipping, nested-off and
explicit-inner-override regressions. These focused checks overlap workspace
cases and must not be added to its eventual total. The earlier incomplete runs
remain separately recorded. No host trust settings or security prompt changed,
and the source hash remained identical; this does not establish the loader cause
or a pristine cold-start reliability fix.

The complete original `cargo test --workspace` started at 08:37:55 Asia/Taipei
using the normal repository signing runner and the single existing Rust cache.
It has entered actual test execution, without an auxiliary readiness helper,
filters, scope exclusions, retries or deadline changes. Its live state and log
are `/private/tmp/blueice-ax-delivery-acceptance-resumed.json` and
`/private/tmp/blueice-ax-delivery-resumed-workspace.log`; final acceptance still
requires terminal completion, count inspection and a fresh source/product audit.

Current logs and metadata remain under `/private/tmp/blueice-ax-delivery-*`.
The definitive accepted result and source receipt must only be written after
Rust terminal completion, count inspection and final signature/source audit.

## Completed resumed validation

The complete workspace subsequently exited 0: 7,260 passed, zero failed and
69 ignored across 473 suites. The actual-core retained-prefix ACK case passed.
The fresh post-workspace source/product/signature audit passed. The final result
and exact command/source receipts supersede the running-state entries above.
Native acceptance remains 176 passed, zero failed and one existing physical
Zhuyin skip. No broader browser completion or loader-cause claim is made.
