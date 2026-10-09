# macOS owned update and shutdown lifecycle

Status: accepted on 2026-10-09; see the [dated results](MACOS_UPDATE_SHUTDOWN_RESULTS.md).
Parent milestone: `69dd6926797878fccfb57102ab4d147d07e2d25c`.

## Reproduced failure

A real launcher serving a real core can exit while an automatic update still
owns a replacement process in startup. A public IPC regression holds that
replacement at a private wrapper barrier, sends Shutdown and checks both owned
PIDs before fixture containment. Before the correction, the serving core was
reaped but the pending process survived. The unchanged-core control passed.
The same two methods passed after the correction without changing their tests.

This reproduces a process ownership defect in the update/shutdown path. The
previous non-rendered label acceptance observed a retained real-core descendant
in an automatic-update fixture, but did not prove the exact stage where that
particular process escaped. That observation and its explicit cleanup remain
separate from this controlled reproduction.

## Lifetime boundary

Closing the broker closes core-switch admission before cleaning up its active
generation. Automatic and manual cutovers share that admission gate. A pending
attempt stops waiting for startup sockets; a registered staged connection is
interrupted during handshake, replay or health checking. Initial core handshakes
also have bounded read/write deadlines. Synthetic capture channels and the
serving connection are closed before waiting for the cutover guard to release.

Core publication and closing admission serialize under the same lifetime lock.
A closing broker cannot publish a staged replacement. Pending attempts check
closure before each retry and core spawn; an attempt racing with closure remains
owned until shutdown drains it. The cutover guard releases only after staged resources have been
dropped and their children reaped. The update watcher wakes when shutdown
begins, including with a long configured polling interval, and is joined before
the broker returns. Normal completion and trusted-window startup failure use
the same shutdown boundary.

Existing retries, generation handoff, client connections, navigation review,
permission boundaries and fresh per-core capabilities remain subject to their
existing rules while the broker is live. Supervised page-host startup retains
its own bounded startup/cleanup path. The frontend retains its existing owned
process-group fallback when graceful service shutdown cannot finish in time.

## Verification and delivery

Seven public process regressions cover unchanged and long-interval shutdown;
automatic and manual replacement startup; a staged peer withholding Hello;
successful replacement; and broken update attempts. Every lifetime assertion
runs before the isolated fixture group's fallback cleanup. Existing automatic
update, cutover retry and launcher library tests also exercise the correction.
The final focused launcher gate passed 286 cases, zero failures and one existing
ignored case across four result groups. The focused launcher and native gates
share 1,837 unchanged source/build inputs with aggregate SHA-256
`2fdec4252d68a8cf24d0581d14cba04648db6e3bf86efff5daf3c0cc57c0e28e`.
Complete workspace acceptance passed 7,395 cases with 69 ignored.

A new XCUITest copies the real local service bundle into a private fixture,
enables automatic updates there and holds a replacement at startup. It verifies
the launcher is a child of that test's BlueIce application and both cores belong
to its owned group. It then closes the real browser window and checks the
launcher, serving core, pending core and group are gone. The original bundle is
kept intact. An external test helper owns the private bundle files; the sandboxed
Runner receives a read-only lease and triggers a replacement over authenticated
loopback HTTP. The helper does not manage the browser's service processes. It
retains the real launcher's packaged filename so trusted sibling resolution
selects the actual BlueIcePanels child. PID/group probes require ESRCH after close.
Native service close, startup, recovery and navigation-review tests
form the focused native gate. Three protocol/supervision XCTest methods and five
actual-window XCUITest methods passed without failures or skips; complete unfiltered native acceptance executed all 273 methods: 272 passed and the existing physical Zhuyin method skipped.

Formatting, strict all-target Clippy, the all-target build, complete workspace
tests, exact native method scope, source/product/signature consistency and owned
process cleanup passed on the same final inputs; see the dated results and receipt.
Other macOS browser requirements remain in the [delivery plan](MACOS_DELIVERY_PLAN.md).
