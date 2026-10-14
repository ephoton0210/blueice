# Host asynchronous operations

M1k supplies the Promise/task mechanism for owner-installed asynchronous host
operations. It installs no page global by itself. The owner still chooses the
bindings, request service, authorization policy and generated typing profile.
Fetch, XHR and their request-service IPC remain separate platform work.

## Call and completion boundary

`BlueJsHostBindingRegistrar::install_async_global_function` installs one
non-constructable Promise-returning function. `install_async_host_object_method`
does the same for a private wrapper family, with the VM checking its receiver
before passing the private `HostObjectKey` to the host. Existing properties,
invalid names and standard globals are refused, including lazy intrinsics that
have no own global property yet.

Both callbacks receive primitive `HostValue` arguments and an opaque
`HostPromiseToken`. The VM creates and roots the Promise. The callback starts
external work and returns; it does not return a JavaScript value or obtain a
VM, heap or object ID. Tokens and primitive completion data can travel through
a bounded service channel to another thread. JavaScript execution remains on
the owning runtime thread.

Ordinary argument, receiver and start failures produce rejected Promises.
This follows the [Web IDL Promise-operation rule](https://webidl.spec.whatwg.org/#dfn-create-operation-function).
Engine resource failures retain their existing containment behavior. Calling
the function as a constructor fails before external work starts.

The host returns a token through
`BlueJsPageRuntime::queue_host_promise_completion`. Accepted outcomes are:

| Outcome | Settlement in the later task |
|---|---|
| `Resolve(HostValue)` | Resolve with a primitive |
| `Reject(HostValue)` | Reject with the supplied primitive reason |
| `RejectError(HostFunctionError)` | Reject with a VM-created TypeError |
| `ResolveObject { family, key }` | Mint/reuse a VM-owned branded wrapper, then run ordinary Promise resolution |

Wrapper completions support response/body facades whose private identity stays
with the embedding owner. Their Promise-returning methods can start further
operations. The mechanism therefore does not require every host result to be
flattened into a string. It does not define the Fetch Response, body, stream,
DOMException or binary-data semantics; those belong to their owning bindings.

The VM rechecks the exact realm capability, wrapper family, string limits and
scheduling admission before retaining a completion. Duplicate, already queued,
completed or foreign/stale tokens return `false` without settling another wait.
An invalid payload or wrapper family leaves the wait available for a valid
retry, unless the document's scheduling resource policy has failed.

## Task source and checkpoints

Registration selects `HostTaskSource::Networking` or `UserInteraction`.
Accepted completions receive a document enqueue serial and join M1j's source
rotation. Arrival order determines FIFO within that source; request start
order does not reorder independent service completions.

Enqueuing runs no JavaScript. A microtask checkpoint alone cannot settle the
wait. A selected document task resolves/rejects the Promise, releases the wait
root, then completes its normal microtask checkpoint. Ordinary Promise
resolution still assimilates thenables; a wrapper's throwing `then` getter
rejects rather than losing the wait. This follows HTML's
[external work and global-task model](https://html.spec.whatwg.org/multipage/webappapis.html#event-loops).

The production one-task Core boundary and separate render entry are preserved.
Queued completions advertise an immediate task wake; paused realms retain
their work and report no runnable wake until resumed. Native UI dispatch keeps
M1h's callback checkpoints and finishes before a subsequently selected
networking task.

Core currently owns native input ingress separately. An owner that introduces
queued UI service notifications alongside that ingress must preserve their
single user-interaction source order. The shipped profiles install no such
async service binding; real Core network/UI ingress integration remains in the
TODO rather than being inferred from the engine's source enum.

## Lifetime and accounting

The live wait owns a Promise root, including its pending reactions and awaited
frames. Settlement removes the wait and releases its root before the task's
checkpoint. Repeated completions must therefore remain collectable under a
bounded managed heap.

Navigation/reload, close and runtime destruction discard pending and queued
work. Their VM identity changes invalidate old tokens. Unfinished waits notify
`HostAsyncFunction::cancel` or `HostAsyncObjectMethod::cancel`; an already
completed and delivered wait has no cancellation notification. Cancellation
must tolerate a service result already in flight.

Waits share the document's 1,024 pending-reaction/request limit. Their owned
table capacity, queued completion payloads and active primitive argument
copies enter the existing 8 MiB scheduling ledger. Completion tasks share the
64 pending browser/cleanup task limit and the common turn's delivery budget.
Admission failure stops external starts or further queued payload retention
through the existing document resource-failure path. Host callbacks still own
the bytes they retain outside these VM tables; broader service-buffer and
process accounting remains open.

## Acceptance

Public page-runtime tests cover real pending Promises, delayed settlement,
source fairness and completion FIFO, native UI cleanup, receiver/family checks,
normal rejection and thenable behavior, duplicate/stale ownership, cancellation,
debugger pauses, cross-thread primitive/token transport, request/task/payload
limits, major-GC frame liveness and 1,000 completions under a 1 MiB heap.
Page-module top-level await also resumes from the later completion task and
finishes its following microtasks through the public runtime.
The milestone record contains the final acceptance gates.
