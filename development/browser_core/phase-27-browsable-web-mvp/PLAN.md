# Phase 27 — Browsable Web MVP: Event Loop, Page DOM, and AJAX

[← Back to plan](../BROWSER_CORE_PLAN.md)

**Status**: In progress — branch `feature/browsable-web-mvp` is cut from
`feature/phase-7-6-8-9-followup` with `feature/test262-remaining-failures`
(PR #25) merged in (see "Integration record"). No milestone below is
implemented yet; M1 is next.

## Objective

Let an ordinary script-driven page work end to end in the one `core`: HTML
arrives, its scripts run, they read and write the DOM, fetch data, and the
result is painted in the same render pass the human and the AI both observe.
The reference page is "fetch JSON, render a list with `innerHTML`, click to
update it" — small, but it exercises every layer a real site needs.

Phases 17, 20 and 21 already own the *complete* versions of these features.
This phase is the **vertical slice through them**, ordered so that something
a person can look at works as early as possible. It does not replace those
phases; each milestone lands as a checked item in the phase that owns the
full feature, and the owning plan is updated in the same commit.

## Current state (verified 2026-10-13, not inferred)

What exists:

- BlueJS is a complete engine (Test262 99.995%); a core-owned executor
  (`backend/core/engine/src/script/javascript.rs`) runs `<script>` declarations
  in one bounded realm per tab/document, behind the opt-in
  `blueice-core --inline-bluejs` / out-of-process page-host options. The
  default session does not construct it.
- Three fixed, owner-selected DOM profiles with generated typings
  (`core-script-dom-text-v1`, `-mutation-v1`, `-event-v1`): `getElementById`,
  `textContent`, `createElement`/`createTextNode`/`appendChild`, and a
  core-originated `click` listener. A page cannot select or widen its profile.
- Native form/default actions, File/Blob/FileReader, drag events,
  `EventTarget`/`CustomEvent`/`AbortSignal`, streams (these are Phase 20).
- A bounded browser-task queue (`backend/bluejs/src/vm/browser_tasks.rs`)
  used by abort timeouts, drag strings and file reads.

What does **not** exist (searched the Rust sources; not present):

| Gap | Consequence |
|---|---|
| `setTimeout`/`setInterval`/`requestAnimationFrame`/`queueMicrotask` globals and a page event loop | Nothing can wait, poll or animate; most frameworks fail at load |
| `querySelector(All)`, `innerHTML`/`outerHTML`, attributes, `classList`, `style` | Pages cannot find or build anything but by id |
| `input`/`change`/`submit`/`keydown` events, capture/bubble, `stopPropagation` | Only link-style click works |
| `fetch`, `XMLHttpRequest`, `Headers`/`Request`/`Response`, `FormData` | No dynamic data at all |
| Cookies, CORS/CSP enforcement for script-initiated requests | A script request has no policy to inherit |
| Default-on script execution | Even the narrow slice is not reachable in normal launcher use |

Rendering gaps (images, flex/grid, float) are Phase 21 and are deliberately
**after** this slice: a page with no script is merely ugly, a page whose script
does not run is empty.

## Decisions

1. **One event loop, owned by core's session tick.** Timers and network
   completions are browser tasks delivered by the existing session poll
   (the same mechanism `about:downloads` uses), with a microtask checkpoint
   between tasks. No script thread, no wall-clock sleep in the VM. Timer
   count, delay floor and queue bytes are bounded; a runaway page degrades
   itself, never the session.
2. **A DOM mutation is a render-pass event.** Every script-caused DOM change
   marks the document dirty and goes through the normal style → layout →
   paint → frame publication, with the same `generation` stamped on the
   `Representation`. The AI snapshot and the human frame can therefore never
   disagree about a script-built page. There is no second DOM.
3. **Capability-scoped profiles stay the model.** New bindings arrive as new
   *versioned, owner-selected* profiles with generated typings and an
   inventory test that pins the exact member set — the existing
   `DOM_EVENT_BINDINGS.md` discipline. The page never chooses its profile.
   "Default-on" means the **core** selects the supported profile for ordinary
   documents; it is a recorded, reviewed decision (M4), not a flag flip.
4. **`fetch`/XHR are not a socket.** They are a typed request to
   `blueice-net` carrying document origin, initiator and mode. They pass the
   same two-stage gatekeeper review as navigation, cannot reach loopback or
   private ranges until the (currently open) private-network URL policy
   exists, and their response bodies are page-derived data with the same
   untrusted framing as any other page content on the MCP surface.
5. **Fail closed on unsupported policy.** A script request whose CORS/CSP/
   cookie semantics are not yet implemented is refused with a visible
   console/diagnostic entry, never silently allowed.

## Milestones

Each milestone is test-first, ends with a passing real-interface test
(not only unit tests) and a dated entry in the owning phase's plan.

- **M0 — Integration.** Merge PR #25's BlueJS line; re-verify. (Done for the
  merge; verification is the open part — see below.)
- **M1 — Event loop and timers.** *(Engine side moved: timers, Promise
  collectibility and the other engine gaps are tracked in
  [`phase-13-bluejs-engine/BROWSER_RUNTIME_GAPS.md`](../phase-13-bluejs-engine/BROWSER_RUNTIME_GAPS.md)
  and land on a dedicated BlueJS branch after PR #25 merges to `main`; a
  tested first slice is parked on `wip/bluejs-web-timers`. The core-side
  frame push below stays in this phase.)* `setTimeout`/`clearTimeout`/
  `setInterval`/`clearInterval`/`queueMicrotask`/`requestAnimationFrame`,
  delivered by the session tick. Acceptance: a counter page updating via
  `setInterval` produces N distinct frames with strictly increasing
  `generation`, and the matching `Representation` generation agrees. Owner:
  Phase 20 (task scheduling) + Phase 13.
- **M2 — DOM read/write surface.** `querySelector(All)`, `getAttribute`/
  `setAttribute`/`removeAttribute`, `classList`, `style` (inline), `innerHTML`
  /`textContent` write via the contextual fragment parser, `removeChild`/
  `insertBefore`/`replaceChild`, `input`/`change`/`submit` events with
  capture/bubble/`stopPropagation`/`preventDefault`. A bounded mutation count
  and node count per document. Owner: Phase 20 (DOM) + `DOM_EVENT_BINDINGS.md`.
- **M3 — `fetch` and `XMLHttpRequest`.** GET and urlencoded/JSON POST,
  `Headers`/`Response.json()/.text()`, `AbortSignal`, same-origin plus a
  fail-closed CORS subset, gatekeeper review, byte/time limits, and a
  request-lifecycle record readable over the same observation path as
  downloads. Owner: Phase 17 (dynamic networking) with Phase 20 (policy).
- **M4 — Cookies and default-on scripts.** Request-service cookie jar with
  `Secure`/`HttpOnly`/`SameSite`, form POST with a `Set-Cookie` round trip,
  and the reviewed decision to execute page scripts by default under the
  M1–M3 profiles. Owner: Phase 20.
- **M5 — Acceptance site.** A first-party demo (alongside
  `phase-6-ai-agent-integration-demo/demo-site`): fetch JSON → render list via
  `innerHTML` → click/typing updates it → form POST. Verified three ways on
  one run: human frame pixels, `get_page_representation`, and an actual MCP
  client; frame and representation `generation` must match at every step.
  Only after this passes are layout (flex/float) and images prioritised.

## Integration record (2026-10-13)

`feature/test262-remaining-failures` was merged into this branch
(11 conflicted files, all in BlueJS plus `README.md`). Resolution policy:

- Where both lines solved the same BlueJS problem differently (Promise/async
  GC rooting, `for await` iterator closing, the constructible-native list,
  `host_object` registration), the Test262 line's version was taken, because
  it is the one measured at 102,921 / 102,926 modes.
- Product-line features were preserved on top: the `web_*` natives (re-added
  to `heap/capabilities.rs`'s constructor set), `browser_tasks`, node/drag/
  file events and host-object families, and the product `README.md`.
- **Deliberately dropped from the product line:** the `AsyncIteratorClose`
  opcode with `AsyncIteratorCloseResult`, and the `owned_edges` Promise-edge
  scoped-roots argument (now always empty). Promises are therefore rooted
  conservatively as the Test262 line does: a Promise lives as long as its
  document's VM.
- **Known difference, tracked separately (2026-10-13):** the product line's
  `promise_gc_lifecycle` tests assert that an unreachable settled Promise is
  collectible; four of them fail on this model. A port of `owned_edges` onto
  the Test262 rooting was tried and reverted: it fixed those four but made
  ten lib allocation-failure tests and several integration targets fail
  (`invalid Promise receiver`, `Heap(InvalidObject)`), because the Test262
  async/generator paths assume every Promise is a root. The fix needs every
  such path to register its in-flight Promise, and a full Test262 inventory
  re-run, so it is its own branch (started from `main` after PR #25 merges),
  not part of this phase. Consequence for this phase: a document that
  creates Promises without bound grows until the managed-heap limit; the
  failure is confined to that document. A long-running-poll memory-growth
  test is the acceptance test for that branch and should be written before
  M5.

Verification gate before this branch is treated as integrated:
`cargo test -p blueice-bluejs` (including the Test262-dependent integration
tests, which need the pinned corpus at
`development/browser_core/reference/test262`), the stream/tee/async-iteration
and `AbortSignal` cases, the full `cargo test --workspace`, clippy with
`-D warnings`, and the Test262 inventory run recorded in
`phase-13-bluejs-engine/TEST262_ANALYSIS_REPORT.md`.

## Non-goals

- Raw HTTP/DOM handles for AI clients; the MCP surface stays typed and
  untrusted-framed.
- A second JavaScript realm or DOM for automation; DevTools/automation stay
  Phase 17 clients of the same core.
- Declaring site compatibility. Passing M5 means one fixture works, not that
  arbitrary sites do.
