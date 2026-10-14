# macOS automatic response downloads

Status: implemented and accepted on 2026-10-08 (Asia/Taipei), after folder
selection commit `77da8abec358d368976baf5a99b20a1277e25e89`. The full browser
delivery remains in progress. See [dated results](MACOS_RESPONSE_DOWNLOADS_RESULTS.md).

## Required behavior

A normal browser navigation that receives a download response supplies the
actual response stream to the owned download service. It preserves the active
page, document identity and history cursor. Native progress/cancel/status and
the selected download directory apply, with normal quarantine before publication.

Attachment disposition takes precedence over rendering HTML. Disposition tokens
are case-insensitive; valid unhandled disposition types receive attachment
handling. Server filenames are advisory and reuse the existing shared sanitized
filename policy, including extended Unicode names. Inline HTML remains a page.
These requirements follow [RFC 6266](https://www.rfc-editor.org/rfc/rfc6266.html).
Navigation preserves the active document for download responses as described by
[the HTML navigation algorithm](https://html.spec.whatwg.org/multipage/browsing-the-web.html#attempt-to-populate-the-history-entry's-document).

Download classification happens before reading/decoding a binary body or waiting
for the complete response. The response remains owned, streamed and cancellable;
its size is not an unbounded memory allocation. Exact GET/POST response bytes
are consumed once. A POST is not replayed as GET, and a one-shot resource is
not probed/refetched to manufacture another response. Redirects keep existing
per-hop URL/extension review and method-rewrite behavior before connecting to
the target. File/transfer policy still applies before publication.

Completion and native actions retain tab/request/navigation/service ownership.
Closing a tab, superseding a navigation, stale windows, duplicate events or
service failure cannot publish an abandoned response or duplicate a download.
Human credential/folder settings remain at their existing owner boundary.
Page scripts, extensions and AI tools do not gain a new path or permission grant.

## Prepared regression boundary

`backend/net/tests/response_downloads.rs` began with eight tests against the
public navigation API and now has ten focused cases. They exercise attachment HTML, invalid-UTF-8
binary data, header-first response handoff with a held body, mixed-case/no-name
attachment, valid unknown disposition, inline HTML preservation, redirect policy
visibility and the original POST request bytes. Its owned loopback fixture uses
bounded I/O/deadlines and release/reaping. On the accepted parent, the eight tests produced six failures and two passes
(cargo exit 101). After typed header-first classification, the expanded ten
response tests and three existing navigation tests passed (cargo exit 0).
Original binary bytes and Unicode metadata are now asserted. This is focused
network evidence, not full manager/core/native acceptance.

The implemented classification assertions require the concrete download variant
and consume/assert its exact original binary stream. Public core and manager
regressions cover one-shot POST, no refetch, cancellation, stale ownership,
retained document/history, file policy, declared/streamed limits and interrupted
catalog restart. Bounded stream framing rejects disconnect/truncation and retains
partial-header progress across read timeouts. Native UI acceptance exercises
normal Return/link/form paths, selected-folder output, native progress, cancel,
relaunch, completed bytes/quarantine, actual Finder reveal and English/Traditional
Chinese surfaces. The final complete gates remain required below.

## Focused implementation evidence (2026-10-08, Asia/Taipei)

The original network boundary produced six failures/two passes on the accepted
parent, then ten response and three existing navigation passes. Three new
public manager protocol tests first failed against the unavailable endpoint,
then passed for exact bytes/no HTTP request, unknown-length disconnect refusal
and cancellation of a stalled incoming body. Three public core-to-real-manager
tests passed for retained document/history, stale/declined offers and duplicate
handoff refusal.

Five new native/XCUITest methods now pass with no failures or skips. They cover
protocol metadata, selected-folder output and unchanged document identity,
originating-window pane ownership, Return/link/one-shot POST, retained page and
history behavior, completed bytes/quarantine, relaunch without replay, cancel
and Traditional Chinese UI. Actual English and Traditional Chinese attachments
were visually inspected and remain ignored in `.build`. This is a focused run,
not the normal unfiltered native gate.

Historical focused failures are retained: two test assumptions (directory URL
trailing slash and an English Edit menu under Traditional Chinese), plus an
actual cross-window pane bug reproduced by a new regression and fixed by tab
ownership filtering. Logs and receipts are under
`/private/tmp/blueice-response-downloads-*`; the current passing bundle is
`frontend/macos/.build/results-20261008-011933.xcresult`.

A pending response remains fenced by tab, document and navigation sequence.
Superseding, closing, declining or expiring an offer drops its original body;
a duplicate decision cannot reuse it. After native acceptance, the download
service owns the transfer independently of later tab navigation. Original
responses are marked in durable transfer metadata, cannot pause/resume/replay,
and use a dedicated bounded binary stream with explicit completion. The same
secure partial-file publication and macOS quarantine apply.

A public core/manager/origin regression reproduced a stalled-source cancellation
problem (zero passes/one failure). The fix adds cancellable transport reads and
an owned, joined response-connection watcher. Four core cases then passed,
including source EOF within two seconds. Actual loopback TLS tests now pass for
body delays spanning several polling intervals and cancellation of a stalled
TLS body, with a per-agent test root and no host trust changes.

Further public manager regressions cover file-policy refusal, declared-length
mismatch, size limits and interrupted-record restart without replay. A restart
regression first reproduced Paused instead of Failed; restored original-response
records now fail and refuse resume. Queued cancel/shutdown regressions first
reproduced retained input and an incorrect resumable state. Pending input is now
retired on cancellation and shutdown, and shutdown records the interrupted
original response as failed. Fresh public core/manager/net focused verification passed 84 tests, with no
failures. TLS adds two actual encrypted transport passes.

TLS fixture setup failures and their correction are retained: macOS accepted
sockets inherited the listener's nonblocking flag, and a failed TLS handshake
kept its socket inside the thread's error result. The fixture now explicitly
uses blocking bounded I/O, drops failed handshake sockets and bounds the client
handshake. Only the exact owned test process was stopped after identity checks;
no unrelated process or global setting was changed.

## Final acceptance

The fresh complete Rust gates and the normal unfiltered macOS native/XCUITest
suite passed on the final source manifest. Actual screenshots and source/product
/process/fixture checks passed. The [dated results](MACOS_RESPONSE_DOWNLOADS_RESULTS.md)
and [receipt](artifacts/macos-response-downloads-results.txt) record exact counts,
method scope, retained historical failures and practical limits. This accepts
this increment, while the full browser delivery remains in progress.
