# Phase 8 — Live Core Hot-Swap / Seamless Update

[← Back to plan](../BROWSER_CORE_PLAN.md)

**Status**: Not started for this phase's own objective (hot-swap); a minimal first slice solving a related, more urgent problem (a human's `frontend` and an AI's `mcp-server` sharing one `core` instance at all) is designed below and ready to build — see "Minimal first slice"

## Objective

Let `core` be updated at runtime without restarting the browser: start a new `core` process (the updated version), verify it's healthy, hand off from the old instance to the new one, then terminate the old instance — invisible to the user, no dropped session.

This generalizes a principle plan §1 already established for window visibility ("AI-controlled visibility, decoupled from process lifecycle") to `core`'s own lifecycle: a process boundary existing is not supposed to mean session continuity breaks when that process cycles.

## Minimal first slice: the rendezvous broker (multi-client sharing, not yet hot-swap)

Raised the same way "Foundation built early" was for Phase 5/`mcp-server` (`phase-12-mcp-server/PLAN.md`): an architecture-wide risk survey found that **today, a human's `frontend` and an AI's `mcp-server` can never observe the same `core` instance at all** — `blueice-mcp-server` spawns its own private `core` subprocess with its own fresh `Page` on every startup (`CoreProcess::spawn`), and `blueice-core`'s own docs state it "accepts exactly one client connection... no multi-frontend support." That's the exact dual-track architecture plan §1's core goal rejects (a human on one instance, an AI driving a separate one), just as fully present internally as the external "human on a real browser, AI on headless Chromium" case the whole project exists to avoid. Waiting for this phase's full hot-swap design (blocked on the reload-vs-migration decision below) to also fix this would block it for no good reason — hot-swapping to a *new core version* and fanning out *one already-running core* to *multiple simultaneous clients* are different problems that happen to want the same rendezvous shape, and the first open question below ("clients connect through a stable point, not to a specific `core` instance") is exactly the piece this slice needs and nothing more.

**Design**: a new, deliberately minimal `blueice-launcher` binary — the same "simpler than `core` itself on purpose" launcher role sketched below, built now for its multi-client-broker job rather than its hot-swap job:

1. **Rendezvous point**: the launcher listens on a well-known Unix domain socket (default `$XDG_RUNTIME_DIR/blueice/core.sock`, falling back to `/tmp/blueice-<uid>/core.sock` if unset — per-user, not system-global; overridable via `--socket`/an env var for tests and for running more than one BlueIce session side by side).
2. **One real `core`, spawned once, never idle-torn-down**: on its own startup, the launcher spawns exactly one `blueice-core` process on a private, internal socket it generates itself (clients never see or connect to this one directly). Per `research/multi-process-memory.md`'s own finding ("`core` — always-resident, no change... nothing suggests treating the always-must-be-up process as an idle-teardown candidate"), this spawned `core` stays up for the launcher's whole lifetime regardless of how many external clients are connected — including zero. Only `mcp-server` (a genuinely stateless adapter, per that same research doc) is an on-demand-spawn candidate; `core` itself never is.
3. **Fan-in / fan-out, not a protocol change to `core`**: `blueice-core`/`session.rs` needs **no changes at all** for this slice. The launcher holds the single internal connection `session.rs` already expects, and does the multi-client work itself: every external client's incoming `ClientMessage`s are forwarded (interleaved, from however many external connections are open) into that one internal stream; a single reader task on the internal stream broadcasts every `ServerMessage` `core` sends back out to *every currently-connected external client*, not just whichever one's action triggered it. This is what actually delivers "same render pass": an AI's `ActOn` produces a `FrameReady` the human's `frontend` receives too, and vice versa, because both are reading the same broadcast off the same one `core` connection.
4. **`mcp-server` changes** (resolves `phase-12-mcp-server/PLAN.md`'s blocked "switch from unconditional spawn to on-demand process spawning" checklist item): `CoreConnection`'s construction tries connecting to the well-known rendezvous socket *first*; only if that fails (nothing listening — no launcher running, e.g. a standalone dev/test workflow) does it fall back to today's behavior, spawning a private `core` via `CoreProcess::spawn`. This is a pure addition — every existing test that relies on today's unconditional-private-spawn behavior (including the real-subprocess integration test) keeps working unchanged, since "nothing listening at the rendezvous path" is exactly the environment those tests already run in.
5. **Known limitation, flagged rather than silently accepted**: `ClientMessage`/`ServerMessage` has no per-request correlation ID today, so with the broadcast in step 3, a `ServerMessage::Error` genuinely caused by *another* client's concurrent action could be misattributed by `mcp-server`'s `send_and_drain` (which loops until it sees *a* `Representation`, treating any `Error` seen along the way as its own) to the tool call currently in flight. Concurrent multi-client usage is expected to be occasional (a human clicking around while an AI separately drives the same page), so this is a rare, cosmetic misattribution, not a safety issue -- but it's a real gap, not nothing. Fix: add a client-generated request ID `core` echoes back on every reply, bundled with the already-deferred `protocol_version` handshake (`phase-1-ai-representation-layer/PLAN.md` §3) since both are the same kind of wire-format extension; tracked as its own checklist item below rather than blocking this slice on it.

**What this slice deliberately does not do** (still gated on the open questions below, unchanged): hot-swap `core` to a new *version* (spawning v2 alongside v1, health-checking it, cutting traffic over, tearing down v1), and idle-teardown authority over `mcp-server`/`downloads`/`ai-assistant`. Both remain real Phase 8 work; this slice only builds the rendezvous/fan-out piece both eventually need anyway.

## Design sketch (the fuller hot-swap shape this phase is ultimately for)

A concrete shape for the supervisor role, assuming (pending confirmation, see open questions) the **reload-based** state-transfer approach — it's the far cheaper of the two options and the sketch below is written against it:

1. A small, deliberately minimal **launcher** binary — simpler than `core` itself on purpose, so it's the least likely thing in the whole system to itself need updating — spawns `core` v1 and hands it a listen socket/named pipe for the control-plane IPC. **This role gets a second job**, per [`research/multi-process-memory.md`](../research/multi-process-memory.md): neither Chromium nor Firefox has a true whole-fleet memory budget (only per-process limits), and BlueIce's own Phase 7 plan flags several of its processes (`ai-assistant`, `mcp-server`, `downloads`) as idle-teardown candidates with nothing currently owning that authority. The launcher, already supervising `core`'s own lifecycle, is the natural place to own idle-teardown of those other processes too, rather than inventing a separate coordinator role.
2. `frontend`/`extension` connect to a stable rendezvous point the launcher owns (a well-known local socket path), not directly to a specific `core` process — so which `core` instance is actually behind that socket can change without either client needing to know.
3. **Update**: the launcher spawns `core` v2 alongside the still-running v1, pointed at the same on-disk profile/state directory (opened read-only by v2 until cutover, to avoid concurrent-write corruption).
4. **Health check**: `frontend` (or the launcher, via `frontend`'s current tab list) replays the session's open URLs into v2 and confirms it renders without crashing; an exact pixel-perfect match against v1 isn't a reasonable bar (font/anti-aliasing can legitimately differ between builds) — a first-pass health bar could be simply "renders without panicking and produces a DOM of comparable structure," tightened later once this phase is actually built.
5. **Cutover**: existing connections are drained/reconnected to v2 (either the launcher starts proxying new rendezvous connections to v2, or it pushes a "reconnect now" signal to already-connected clients — a real design choice for whoever implements this, not resolved by this sketch).
6. **Teardown**: v1 is terminated after a grace period once v2 is confirmed healthy under real traffic, not immediately at cutover.

If reload-based state transfer turns out to be insufficient (see open questions), most of this sketch still holds — only step 4/6 and what "session state" means at handoff would need to change, not the overall supervisor/rendezvous shape.

## Open questions (blocking the fuller hot-swap design; the process-role question is resolved for the minimal slice above)

- ~~**This needs a new process role.**~~ **Resolved for the minimal slice**: `blueice-launcher`, designed above — `frontend`/`mcp-server` connect to its rendezvous socket rather than to a specific `core` instance directly. What's still genuinely open for the *fuller* hot-swap objective is whether that same launcher process (vs. a second, separate role) is also the right owner of spawning/health-checking/handoff between `core` v1 and v2 — the minimal slice gives it exactly one `core` instance to manage, forever; hot-swap needs it to manage a transition between two.
- **What state actually needs to transfer from old `core` to new `core`?** Two very different answers with very different engineering cost:
  - *Reload-based*: the new instance reconstructs current tabs from URL + navigation history — much simpler, likely sufficient for "the engine binary changed."
  - *True state migration*: in-flight DOM/JS-heap/form-input state moves live from old to new — needed only if mid-session continuity (e.g. unsaved form input) must survive a swap, and is a much larger undertaking.
  
  This has to be decided before any implementation starts; don't assume the harder option is required without confirming it's actually needed.
- **Health verification before cutover**: what concretely counts as "tested no errors" — a smoke-render of the current page(s) in the new instance, diffed against the old instance's current output, before traffic cuts over?
- **Failure handling**: if the new instance fails its health check, does the old instance just keep running (safe default) — this needs to be the explicit designed behavior, not an accident of whatever code happens to run first.

## Checklist

**Minimal first slice (rendezvous broker — designed above, ready to build; resolves the multi-client/"same instance" gap without waiting on the hot-swap decisions below):**

- [ ] Build `blueice-launcher`: spawns one `core` on a private internal socket at launcher startup, never idle-torn-down
- [ ] Implement the external rendezvous listener (well-known socket path, overridable) accepting multiple simultaneous client connections
- [ ] Implement fan-in (multiple external `ClientMessage` streams -> one internal `core` connection) and fan-out (every internal `ServerMessage` broadcast to every connected external client) — no changes needed to `blueice-core`/`session.rs` itself
- [ ] Update `mcp-server`'s `CoreConnection` construction to try the rendezvous socket first, falling back to today's private `CoreProcess::spawn` only if nothing is listening there (resolves `phase-12-mcp-server/PLAN.md`'s blocked "on-demand spawning" checklist item for the *shared-instance* half of that item — see that phase's own checklist for the on-demand-*spawn-timing* half, which is a separate, `mcp-server`-side concern)
- [ ] Add an end-to-end test: two client connections (standing in for `frontend` and `mcp-server`) through the same launcher observe the same `generation` sequence and the same `FrameReady`/`Representation` state after either one's action — the concrete, checkable version of "same render pass, now across two real client connections" this slice exists to prove
- [ ] Add a client-generated request ID to `ClientMessage`, echoed back on the corresponding reply, bundled with the already-deferred `protocol_version` handshake (`phase-1-ai-representation-layer/PLAN.md` §3) — closes this slice's known broadcast-misattribution gap (see "Known limitation" above); not blocking for the slice's initial build

**Fuller hot-swap work (blocked on the decisions below, not started):**

- [ ] Decide reload-based vs. true state migration (blocks everything else)
- [ ] Define the health-check contract a new `core` instance must pass before cutover
- [ ] Define failure behavior when a new instance fails its health check
- [ ] Prototype against the Phase 3 skeleton (spawning a v2 `core` alongside the launcher's existing v1 and handing off between them)
- [ ] Scope the launcher's idle-teardown authority over `ai-assistant`/`mcp-server`/`downloads` (with Phase 7/10/12), per `research/multi-process-memory.md`
