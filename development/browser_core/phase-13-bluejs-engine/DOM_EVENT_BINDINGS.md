# First page DOM and event binding boundary

[← Phase 13 plan](PLAN.md) · [Phase 18 worklist](../phase-18-bluets/TODO.md)

This is the design decision for the first page-visible slice of the broader
[Phase 2 MVP DOM surface](../phase-2-mvp-scope/PLAN.md).
It does not claim that the bindings have been implemented. The existing
`blueice_ipc::script` requests and core dispatcher prove DOM operations against
live tabs, while the current BlueJS page hosts install only copied document
text and origin functions. A page VM cannot yet call the DOM dispatcher or
receive an event. The implementation and real-page acceptance remain open in
Phases 13 and 18.

## First exposed surface

Install the following only for a core-selected page profile whose runtime
installer and generated `lib.blueice.d.ts` artifact name the same bindings.
The empty profile and the existing copied-snapshot profiles retain their
current meaning. No page can select or extend its own profile.

| JavaScript surface | Core operation | Capability | Initial behavior |
| --- | --- | --- | --- |
| `document.getElementById(id)` | `GetElementById` | `dom-read` | Return a realm-owned element wrapper or `null`. |
| `document.createElement(tag)` | `CreateElement` | `dom-write` | Return a detached element wrapper. |
| `document.createTextNode(text)` | `CreateTextNode` | `dom-write` | Return a detached text wrapper. |
| `node.textContent` get/set | `GetTextContent` / `SetTextContent` | `dom-read` / `dom-write` | Read or replace text in the current document. |
| `parent.appendChild(child)` | `AppendChild` | `dom-write` | Move only an allowed detached node into the same live document. |
| `element.addEventListener("click", callback)` | Child-owned listener table | `dom-event` | Register a callable in the current realm for a core-originated click. |
| `element.removeEventListener("click", callback)` | Child-owned listener table | `dom-event` | Remove that exact callable registration. |

The first event type is `click`. The initial callback receives an event with
`type`, `target`, `currentTarget`, and `preventDefault()`. The latter prevents
the click's default link navigation. The event cannot request a network
operation. Other Phase 2 DOM properties, selectors, capture/bubble phases,
`stopPropagation()`, `input`/`change`/`submit` events, timers, and general
`lib.dom.d.ts` remain later implementation slices. A missing operation stays
absent at both the TypeScript and JavaScript boundaries.

## Ownership and identity

Core alone owns `Page`, `TabManager`, document generation, origin and DOM
mutation. The BlueJS child owns JS wrappers and listener callables. A wrapper
stores a child-private handle mapped to an exact `(tab, document generation,
node)` tuple; a numeric `NodeId` is never page-visible or accepted as a JS
argument. Core validates the tuple against the live page on every operation,
including after navigation and removal. Closing or replacing a realm clears
its wrappers, listeners and queued events before another document can use the
same tab. A node removed by `textContent` replacement becomes invalid.

The script socket must accept only the launcher-owned child for the matching
core generation. Extend its handshake with a per-spawn capability and bind
each DOM request to the exact live document generation before exposing these
bindings. An arbitrary process that can reach a socket or guess a tab/node
number must not gain DOM authority. Public frontend, debugger and MCP requests
cannot invoke this private operation set.

The VM's current `HostValue` callback ABI accepts primitives only. DOM wrapper
objects and event listeners therefore need a VM-owned host-object/callable
mechanism with collector-visible roots; do not pass or retain JS object IDs
through `HostValue`, or put a Rust reference to `Page` in the child. Generated
host typings may advertise a member only once that exact runtime installer
and its capability policy exist.

## Scheduling and failure behavior

A DOM call returns its core reply before the next JS statement observes its
result or mutation. The current core session waits synchronously for a child
document execution reply, so its ordinary between-turn script-request drain
cannot serve reentrant DOM calls. The page-host wait must service bounded,
authenticated DOM requests from that same child and tab while preserving the
session thread's exclusive `TabManager` ownership. It must stop servicing when
the child response, deadline, realm, or core generation ends. It must not
dispatch unrelated frontend or debugger requests in that nested wait.

Core originates a click only after resolving a live target in the current
document and before applying any link navigation. The child queues it for the
owning realm's event loop; listener invocation is a task, with a microtask
checkpoint before core applies the default action or renders the next frame.
The bounded child reply reports only whether `preventDefault()` ran. Core
applies navigation only when the original document generation is still live
and the reply did not prevent it. Event delivery rechecks the exact realm and
target tuple. There is no direct callback from a core worker into a running VM
frame. Navigation discards pending old-generation events rather than
retargeting them. A listener exception follows the page-script error policy
and cannot abort core's session loop.

The binding adapter gives script only fixed categories for stale handles,
denied capability, invalid arguments, and over-limit data; it does not echo
private socket tokens, source bytes, internal paths, or DOM data from another
tab. Core validates bounded strings and request frames before allocation or
mutation. The first implementation must keep the existing per-tab VM, source,
program and bytecode budgets, and account DOM-call/event-queue work to the
initiating tab. A failed validation must leave the DOM and listener table
unchanged.

## Implementation and acceptance order

1. Add authenticated, document-generation-bound script IPC and a bounded
   session-owned child wait that can answer a DOM call during execution. The
   core dispatcher now rejects an old document generation on every existing
   DOM operation; child authentication and the nested wait are still pending.
   Verify that an unauthenticated, stale, cross-tab or old-core request cannot
   mutate a page; prove a child can complete a synchronous lookup without a
   session deadlock.
2. Add VM-owned node wrappers and the listed DOM methods. Verify a real HTTP
   page's `<script>` reads, creates and changes visible text through the
   launcher/core/child route in document order. Reload must invalidate prior
   wrappers. Unsupported APIs must fail both generated BlueTS checking and
   JavaScript runtime access.
3. Add click listener ownership and event-loop delivery. Verify a real click
   changes visible text through the listener, a removed listener does not run,
   `preventDefault()` suppresses link navigation, and navigation discards
   queued callbacks. Repeat the route with a supported direct BlueTS page
   using the same host profile, without a second DOM bridge.

The legacy proof-of-mechanism script channel now has v2 negotiation, an exact
tab/document-generation target on every DOM request, capped frames and
name/text fields, and a real-core regression rejecting a stale socket request
after navigation. This does not satisfy step 1: the listener still needs a
launcher-owned child authentication grant and a bounded reentrant session wait
before a page VM may call it.

These are acceptance gates for the existing Phase 13/18 runtime items; this
document closes only the Phase 13 **design** checklist item.
