# macOS native text and page drag results

Accepted macOS software increment 2026-10-11T14:14:22.862221+00:00; parent `a886a46a1e6fd09d69aa5320d91def92878ba053`.
Complete macOS browser delivery remains in progress. Windows runtime acceptance
has not passed for this source snapshot.

SwiftUI/AppKit starts and receives actual OS page/text drags using Core-owned
payloads, operation decisions and document lifetimes. Protected page listeners,
read-only destinations, replaced documents, cancelled sources and withheld GUI
frame notifications retain their ownership checks. Accepted drops publish new
Core text/frame state and changed GUI image bytes. Cancelled sources revoke
future promised-data reads from the original owned pasteboard generation.

| Validation | Actual result |
| --- | --- |
| Focused AppKit/Core/OS native tests | 21 passed, 0 failed, 0 skipped |
| Complete XCTest/XCUITest | 296 passed, 0 failed, 1 existing physical Zhuyin skip |
| Fresh complete macOS Rust workspace | 7,455 passed, 0 failed, 69 ignored, 492 groups |
| Corrected fixtures within that full run | Downloads 65; native forms 18; debugger 33; launcher update/lifecycle 14 passed |
| Affected formatting, strict Clippy and all-target build | Passed |
| Linux x86_64 instrumented workspace / independent BlueJS | Passed original CI line floors of 90% / 88% on pb60g |
| Linux final counter fixture supplement | All 14 affected cases passed; production sources unchanged from complete coverage |
| Product verification | 11 strict signatures; three Swift products built for arm64 and x86_64; eight backend services arm64 |
| Ownership and artifacts | No unexpected owned processes; preexisting Cargo process preserved; screenshots and XCResult ignored |
| Windows Rust / WinUI 3 | Pending; C: space is below the unchanged 4 GiB guard; no completed runtime acceptance |

The public stale-hover regression fails before request/tab ownership is repaired
and passes afterward with the same test source. Background hover refusals retain
navigation-denial feedback, while unowned errors remain visible. The quit helper
parks the pointer on the owned BlueIce menu before Cmd-Q; its cursor invariant
fails before repair and passes afterward. The UUID-owned Finder fixture removes
only its own window/folder. Its isolated Settings relaunch checks did not reproduce
the earlier system-popup failure, which remains failed historical evidence.

The stationary drag keeps its original 1.2-second hold, 250 ms cancellation
condition, coordinates, Core values and no-fetch assertions. The accepted complete
suite records actual held-button stationary events and cancellation. The earlier
stationary timeout was not independently reproduced and remains a failed run.

Four existing test fixtures required repairs after the original native suite:
failed transfers close before delivering bytes and verify partial files; forms
half-close commands and drain replies through EOF; the closed-profile debugger
uses a 33-elision array literal with the same length/own-key budget expectations;
launcher wrappers atomically rename invocation counters. Production and native
UI sources remain identical. The fresh complete macOS workspace validates all
four corrections. Linux complete coverage uses its exact three-fixture snapshot;
the 14-case supplement covers the sole subsequent counter fixture delta.

Windows preparation verified the final transferred snapshot and pinned corpus.
Its previous attempts stopped at toolchain selection, Windows command length,
state-file sharing and the disk guard. Controller repairs and passed per-package
formatting are recorded separately and do not establish Rust or GUI acceptance.
The metadata probe responsible for memory/pagefile growth was stopped after its
PID/start/command identity was checked. No VM reboot or global pagefile change
was performed; reboot approval is still pending.

[Machine-readable receipt](artifacts/macos-native-drag-results.txt) records exact
case IDs, source hashes, commands, evidence hashes, historical failures and
cleanup. Raw result bundles and screenshots remain in ignored local build paths.

File promises/default images, operation modifiers, external/multiple-window
dragging, broader DOM APIs and the other [delivery milestones](MACOS_DELIVERY_PLAN.md)
remain open. Physical IME, VoiceOver, monitor/printer transitions and distribution
are not accepted by this increment. Intel products are built, not runtime tested.
