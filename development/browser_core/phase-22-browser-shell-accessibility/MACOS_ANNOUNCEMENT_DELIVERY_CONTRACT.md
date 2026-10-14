# Retained macOS announcement delivery

Implementation and complete native/Rust acceptance are recorded in
[MACOS_ANNOUNCEMENT_DELIVERY_RESULTS.md](MACOS_ANNOUNCEMENT_DELIVERY_RESULTS.md).
The test-first history below preserves earlier incomplete and red results.

This increment replaces the latest-layout-only stream introduced in commit
`3edaeff81`. That accepted increment documents a known loss: an intervening
layout can clear an announcement before the native frontend reads it.

## Required behavior

- Preserve ready announcements across representation reads, resize and later
  public mutations until the native consumer acknowledges their sequence.
- Keep a FIFO of at most 64 announcements, each at most 4096 Unicode scalars.
  Advance sequence for every admitted update; evict oldest pending messages on
  overflow and report loss until the consumer acknowledges that revision.
- Do not retain or replay messages across document replacement. Purge queued
  content when its live region disappears, is disabled, hidden or private, or
  its contributing source/container becomes hidden or protected. Previously
  removed public text may remain an eligible removal announcement while its
  containing live region is valid; privacy checks must cover original sources.
  An original contributor subsequently placed under a nested `aria-live=off`
  boundary is removed from the queue and removal baseline. An explicit inner
  live region still overrides an off ancestor above its own region boundary.
- Add a versioned, source/document-fenced acknowledgement through the existing
  context/window/tab envelope. Acknowledge only a prefix through an observed
  revision. Reject future revisions, foreign ownership and old documents before
  changing the queue. Duplicate and older acknowledgements are idempotent.
- Do not fence acknowledgement to an exact frame: a legitimate consumption
  acknowledgement may arrive after a subsequent layout. It must never clear a
  newer, unobserved sequence or mutate focus, selection, scroll or navigation.
- Report an acknowledged revision in snapshots/replies so the native bridge
  can confirm resource release. Retrying this idempotent acknowledgement is
  distinct from retrying an uncertain editor/reveal mutation.
- Advertise `delivery_version: 1` in supporting snapshots. Missing capability
  and acknowledged revision default to zero for older payloads; the native
  bridge must not send the new command to a core that does not advertise it.
- Perform consumption acknowledgements off the AppKit main thread, using a
  private scoped connection with the existing exchange deadline. Coalesce
  requests per tab and retry only this idempotent operation with backoff from
  250 ms to 8 s while the originating tab/document remains current. Stop and
  document replacement invalidate pending work. No text is stored in this
  retry queue; it contains only delivery identities and revision metadata.
- Keep the native bridge's local delivery watermark. Baseline newly selected
  documents, acknowledge deliberately dropped baseline/inactive updates and
  never repeat a delivered message when a retained batch is read again. Do not
  acknowledge another tab or a replaced document through a delayed callback.
- Distinguish native consumption acknowledgement from physical VoiceOver speech
  completion, for which AppKit does not provide this acknowledgement contract.

## Test-first evidence

The first focused run on 2026-10-05 exited 101: 12 passed and 3 failed, with
716 unrelated tests filtered. It used the existing single macOS Rust cache,
Rust 1.96.0 and the repository signing runner. The three failures demonstrate
resize loss, consecutive-layout loss and missing bounded retention. A new
privacy-transition case passed on the old clearing implementation and must
continue to pass once retention exists. Full acceptance was not established at
that stage; the final result is linked above.

A later focused run added overflow-release and disabled/hidden/removed-region
coverage. It exited 101 with 18 passed and one failure: a clipped queued update
lost its truncation marker after a later short update. The run took 708.72 s,
including compilation and a pre-test startup delay; the tests themselves took
0.57 s. Its sample captured `_dyld_start` before Rust main. The implementation
now retains truncation metadata with each queued event; final verification is
linked above.
The original result is preserved in
`/private/tmp/blueice-ax-delivery-retention-truncation-red.log` and `.json`.

The follow-up run compiled successfully but was retired with SIGTERM after
more than 16 minutes before test main. It executed no cases; its 1337.91 s
includes 5m16s compilation. Sampling again showed `_dyld_start`, and the mapped
inode matched the current executable; strict signature verification exited 0.
This is an incomplete run, not a passing result or an additional test skip.
The queued IPC and actual-core focused commands did not run. Logs, retirement
metadata and samples remain under `/private/tmp/blueice-ax-delivery-focused-*`
and `/private/tmp/blueice-ax-delivery-core-green-startup.sample.txt`.

An independent launch of the exact original test executable was also retired
before any cases executed, after 682.73 s. Its inode and SHA-256 were identical
before and after. It does not replace the incomplete Cargo result. New nested
off/explicit-inner-override regressions were then added from code inspection;
their final runtime verification is included in the accepted result.

The complete native gate subsequently passed 176 cases with zero failures and
one existing physical Zhuyin skip. After completion and attachment export,
eight signatures, architecture and native process cleanup passed. One
cfg(test)-only Rust assertion was then corrected to account for both new and
removed text under `aria-relevant=all`; all production/native inputs and the
eight tested native executable hashes remain unchanged. Formatting, strict
Clippy and all-targets build passed again. The workspace attempt at that stage
remained pending at backend startup, before any test cases had run. The subsequent
complete workspace passed with the normal repository signing runner. The exact
source difference, original native snapshot and final candidate/Rust snapshots are in
[MACOS_ANNOUNCEMENT_DELIVERY_VALIDATION_PENDING.md](MACOS_ANNOUNCEMENT_DELIVERY_VALIDATION_PENDING.md).

The preliminary native focused run passed all 23 Accessibility XCTest cases,
with zero failures/skips/runtime warnings, including typed wire capability,
bridge deduplication and real bundled-core acknowledgement/readback. Its result
is `frontend/macos/.build/results-20261005-220647.xcresult`, exported after
terminal completion. It predates the subsequent nested-off core adjustment and
does not establish final frozen-input native or Rust workspace acceptance.

Logs: `/private/tmp/blueice-ax-delivery-red.log` and
`/private/tmp/blueice-ax-delivery-red.json`. Final acceptance additionally covers
ownership/revision acknowledgement, overflow release, privacy/off-region purging,
actual core IPC, native bridge deduplication and an actual-window flow.

The existing physical VoiceOver, initial-alert, complete ARIA/name, additional
rotor, document-selection and other browser requirements remain in
[MACOS_DELIVERY_PLAN.md](MACOS_DELIVERY_PLAN.md). This work does not claim that
those requirements are complete.
