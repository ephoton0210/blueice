# Phase 8 — Live Core Hot-Swap / Seamless Update

[← Back to plan](../BROWSER_CORE_PLAN.md)

**Status**: Not started

## Objective

Let `core` be updated at runtime without restarting the browser: start a new `core` process (the updated version), verify it's healthy, hand off from the old instance to the new one, then terminate the old instance — invisible to the user, no dropped session.

This generalizes a principle plan §1 already established for window visibility ("AI-controlled visibility, decoupled from process lifecycle") to `core`'s own lifecycle: a process boundary existing is not supposed to mean session continuity breaks when that process cycles.

## Design sketch

A concrete shape for the supervisor role, assuming (pending confirmation, see open questions) the **reload-based** state-transfer approach — it's the far cheaper of the two options and the sketch below is written against it:

1. A small, deliberately minimal **launcher** binary — simpler than `core` itself on purpose, so it's the least likely thing in the whole system to itself need updating — spawns `core` v1 and hands it a listen socket/named pipe for the control-plane IPC.
2. `frontend`/`extension` connect to a stable rendezvous point the launcher owns (a well-known local socket path), not directly to a specific `core` process — so which `core` instance is actually behind that socket can change without either client needing to know.
3. **Update**: the launcher spawns `core` v2 alongside the still-running v1, pointed at the same on-disk profile/state directory (opened read-only by v2 until cutover, to avoid concurrent-write corruption).
4. **Health check**: `frontend` (or the launcher, via `frontend`'s current tab list) replays the session's open URLs into v2 and confirms it renders without crashing; an exact pixel-perfect match against v1 isn't a reasonable bar (font/anti-aliasing can legitimately differ between builds) — a first-pass health bar could be simply "renders without panicking and produces a DOM of comparable structure," tightened later once this phase is actually built.
5. **Cutover**: existing connections are drained/reconnected to v2 (either the launcher starts proxying new rendezvous connections to v2, or it pushes a "reconnect now" signal to already-connected clients — a real design choice for whoever implements this, not resolved by this sketch).
6. **Teardown**: v1 is terminated after a grace period once v2 is confirmed healthy under real traffic, not immediately at cutover.

If reload-based state transfer turns out to be insufficient (see open questions), most of this sketch still holds — only step 4/6 and what "session state" means at handoff would need to change, not the overall supervisor/rendezvous shape.

## Open questions (blocking real design)

- **This needs a new process role.** Neither `core`, `extension`, nor `frontend` (plan §1) is the right place to own spawning/health-checking/handoff of `core` instances — that has to sit *above* `core`, as a supervisor `frontend`/`extension` connect through rather than connecting to a specific `core` instance directly, so a swap doesn't require either of them to reconnect or notice.
- **What state actually needs to transfer from old `core` to new `core`?** Two very different answers with very different engineering cost:
  - *Reload-based*: the new instance reconstructs current tabs from URL + navigation history — much simpler, likely sufficient for "the engine binary changed."
  - *True state migration*: in-flight DOM/JS-heap/form-input state moves live from old to new — needed only if mid-session continuity (e.g. unsaved form input) must survive a swap, and is a much larger undertaking.
  
  This has to be decided before any implementation starts; don't assume the harder option is required without confirming it's actually needed.
- **Health verification before cutover**: what concretely counts as "tested no errors" — a smoke-render of the current page(s) in the new instance, diffed against the old instance's current output, before traffic cuts over?
- **Failure handling**: if the new instance fails its health check, does the old instance just keep running (safe default) — this needs to be the explicit designed behavior, not an accident of whatever code happens to run first.

## Checklist

- [ ] Decide reload-based vs. true state migration (blocks everything else)
- [ ] Design the supervisor process role and how `frontend`/`extension` connect through it rather than to a specific `core` instance
- [ ] Define the health-check contract a new `core` instance must pass before cutover
- [ ] Define failure behavior when a new instance fails its health check
- [ ] Prototype against the Phase 3 skeleton once it exists in enough detail to actually spawn two instances and hand off between them
