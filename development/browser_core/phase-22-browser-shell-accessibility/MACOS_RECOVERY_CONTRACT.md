# macOS browser service recovery

This milestone adds an explicit native recovery action after the supervised
browser stack fails. Implementation and acceptance are complete on 2026-10-07;
see [dated results](MACOS_RECOVERY_RESULTS.md). The complete browser delivery
plan remains active.

The stopped browser retains a visible failure surface with a keyboard-operable
Restart action. It explains whether a confirmed session can be recovered and
that unsaved page changes cannot survive a lost core. Starting or recovering
does not permit duplicate restart requests. A failed restart remains retryable.
Normal page controls remain disabled until a new owned connection is ready.

Recovery creates a new workspace, native models and supervised BrowserSession,
with a new private runtime directory and owner identity. It first awaits cleanup
of the old workspace, download connection and owned service process group.
Old window callbacks, queued keys, page/assistant replies, frame paths and
request IDs cannot mutate or close the replacement workspace. The GUI process
stays alive; recovery does not require quitting or relaunching the application.

A bounded confirmed canonical navigation snapshot is retained in memory while
the application is running, regardless of the opt-in disk-save preference.
Snapshot capture remains read-only and requires a consistent revision across
profiles, windows, groups, tabs, selection, history, zoom and owned window frames.
Partial or superseded captures keep the previous confirmed snapshot. Recovery
creates fresh canonical identities, restores structure and reviews reopened
GET pages through the ordinary policy path. POST history retains its marker
without replaying a request. Page data, password text, form-control contents,
selected files, POST bodies, transient AI results and one-shot confirmation
decisions are not part of the recovery snapshot.
Recovery never silently enables remembering or overwrites a malformed saved
archive. Failed/blocked page restorations retain their reviewed visible denial.

Acceptance requires an actual owned-service fault and keyboard restart in the
same application process; multiple windows/tabs, profile/group ownership,
history/selection/zoom restoration; no disk archive while remembering is off;
POST and private-data refusal; consecutive faults, failed startup/retry and
stale-owner/reply/input rejection. Native regressions, focused core/protocol
gates and final source/product/signature/process audits precede a scoped commit
and the owner's authorized normal push. Physical IME/VoiceOver and remaining
browser requirements remain separate delivery work. The accepted full native
suite passed 217 methods with one existing physical Zhuyin skip; the fresh
focused engine/IPC gates passed 1,004 cases. The previous complete Rust workspace
baseline is carried only for the 1,697 unchanged backend inputs.
