# Page Promise rejection tracking

[← Browser integration TODO](BROWSER_INTEGRATION_TODO.md)

M1p completed on 2026-10-15. All 37 related Rust targets (3,234 passed; one
existing ignored), fresh official MCP acceptance (one passed), formatting and
workspace/all-target Clippy/build pass. All 102,926 pinned Test262 modes match
M1o's accepted source/result inventory. The verified red baseline and final
evidence are recorded in the [milestone record](BROWSER_INTEGRATION_MILESTONES.md#milestone-record).

## Engine state and ownership

Promise records retain their handled state. First rejection without a handler
enters the owner-enabled tracker; installing the first reaction marks the input
handled, including module/function/generator awaits. A derived Promise has its
own status. Adopting or consuming an input does not suppress an ignored rejected
result. Streams' internal reader `closed` promises receive their separate
handled marking at the specification's error/release boundaries.

The [ECMAScript hook](https://tc39.es/ecma262/multipage/control-abstraction-objects.html#sec-host-promise-rejection-tracker)
defaults to no notification in host-neutral VMs. The page owner explicitly
enables the browser policy. Its pending list retains Promise roots through
notification; the ordinary Promise-owned edges retain each reason. Once
reported, an outstanding notification is a non-rooting bit in the existing
collector-swept Promise record. Event/task roots are released on completion or
failure. Navigation/close discards the entire realm and its queued work.

## Checkpoint and task boundary

The [HTML notification algorithm](https://html.spec.whatwg.org/multipage/webappapis.html#unhandled-promise-rejections)
queues delivery after a complete microtask checkpoint. A handler attached
before delivery suppresses notification. An unhandled event is cancelable;
handling after notification queues a non-cancelable `rejectionhandled` event.
Attaching a handler during the first event does not create a late notification.

Both use the existing document DOM-manipulation task source and enqueue serials,
shared with drag string callbacks. A checkpoint queues one ordered batch of
candidates. Delivery shares one native-event checkpoint budget and task fuel
across the batch. New work waits for the ordinary task-turn scheduler. No
notification is dispatched from Promise settlement or reaction registration.

Pending, queued and active candidates count against the document's existing
1,024 waiting-work budget. Their vector storage, reasons and browser-task
slots participate in the native scheduling ledger and shared 8 MiB payload /
64-task limits. A failed admission poisons the existing document account;
JavaScript catch handlers cannot turn it into successful execution.

## Installed surface and diagnostics

File DOM v16 extends the v15 runtime identities with `PromiseRejectionEvent`,
`onunhandledrejection` and `onrejectionhandled`. Its global listener overloads
describe both event names. Old artifacts retain their bytes and keep these
names absent. The event's `promise` is an opaque object, matching Web IDL;
construction requires an object but does not manufacture or wrap a Promise.
`reason` retains the original value. Script-created events remain untrusted.

The two global attributes use ordinary EventHandler semantics: a false return
cancels a cancelable event. Notification events retain original Promise/reason
values inside their realm. Uncanceled first reports use getter-free console
previews with `Uncaught (in promise) `, method/level `error`, and existing bounds.
They do not dispatch an additional global `error` event. A throwing listener
uses the existing uncaught-exception path; peers retain ordinary dispatch.

Core/MCP drain copied, untrusted-framed diagnostics through private v61, whose
M1q console extension adds optional bounded source positions. No live
Promise/reason handle crosses IPC. Promise rejection/await origins retain their
separate open item and unavailable locations stay omitted; see
[the source contract](SOURCE_DIAGNOSTICS.md). This increment installs no network API.
