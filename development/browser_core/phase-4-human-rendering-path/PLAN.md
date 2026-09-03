# Phase 4 — Human-Visible Rendering Path

[← Back to plan](../BROWSER_CORE_PLAN.md)

**Status**: In progress (frame-channel shape decided; boundary implementation, reference frontend, and everything else still open)

## Objective

Get the Phase 3 paint output onto an actual screen with basic interaction, so there's a human-usable window onto the engine — the first half of the "human and AI share the same render pass" goal from plan §1.

Per plan §1's process architecture, `frontend` is a separate process from `core` by design — every platform gets its own native UI toolkit (WinUI 3, SwiftUI, PySide6/Qt, etc.) rather than one cross-platform Rust UI, since `core` only owns rendering, not presentation. This phase is therefore really "define the `core`↔`frontend` boundary and prove it with one reference frontend" — not "build the browser window," since the real deliverable (the IPC/embedding API) has to work for frontends BlueIce doesn't control the language/toolkit of.

## Plan

This phase is deliberately scoped to *displaying and interacting with* what Phase 3 already produces, not extending the pipeline itself:

- Define the `core`↔`frontend` boundary: the control-plane IPC (shared with `extension`/AI per plan §1) for input/navigation/lifecycle, plus the separate high-bandwidth frame-pixel channel — **shape now grounded by [`research/frontend-ipc.md`](../research/frontend-ipc.md)**, which studied Chromium's Viz (transfers real per-platform GPU buffer handles: dma-buf on Linux, `IOSurface` on macOS, DXGI shared handles on Windows, shared memory as the universal CPU fallback) and Firefox's WebRender (ships a serialized *display list* instead, rasterizing inside the compositor process itself) as the two real production approaches. Neither reference engine has to solve BlueIce's actual hard problem, though: both hand their compositor process the native window handle directly and present locally, with no "ship finished pixels to the window's owner" cross-process leg at all — because in both browsers the compositor *is* the process that owns the window. BlueIce can't assume that, since `frontend` (which owns the window) is a different, non-Rust process per platform. Recommendation: a common frame-plane interface (handle-kind union + metadata + sync token, shaped like Chromium's `TransferableResource`) with a genuinely separate adapter per `frontend` platform behind it, shared memory as the universal fallback — not one portable zero-copy mechanism, since even Chromium and Firefox each needed a per-OS union and BlueIce adds a second divergence axis (UI toolkit, not just OS) on top.
- Build exactly one reference frontend against that boundary first — pick based on what's fastest to validate the API with, not the eventual primary platform target — before any platform-native (WinUI 3/SwiftUI/PySide6) frontend is attempted, so the boundary gets proven against a real out-of-process client before multiple frontends have to be kept in sync with it.
- Confirm the boundary supports showing/hiding the frontend independently of the `core` process — the window's visibility must not be tied to the engine instance's lifecycle (plan §1), so the AI can hide it for AI-only operation and show it again later without a restart.
- Wire Phase 3's paint stage to the reference frontend's surface over the boundary above.
- Handle the minimum input needed to browse: scroll, click, window resize (each of which needs to feed back into layout/paint, not just be swallowed at the window layer).
- Handle basic navigation: load a URL, follow a link.
- Manually verify against the Phase 2 demo page(s) as a smoke test — this is the first point where the pipeline can be checked by eye instead of only by automated fixture assertions.
- Add a Help/About/Credits screen crediting Chromium and Gecko as technical references — this satisfies BSD-3-Clause's binary-distribution notice clause (reproducing Chromium's copyright notice "in the documentation and/or other materials provided with the distribution"), which the per-file source headers from Phase 0 don't cover. See [Phase 0](../phase-0-license-legal-foundation/PLAN.md) and plan §2.

## Checklist

- [ ] Design the `core`↔`frontend` control-plane IPC (input/navigation/lifecycle), sharing the protocol design with `extension`/AI per plan §1
- [x] Decide the high-bandwidth frame-pixel channel shape — a common frame-plane interface (handle-kind union + metadata + sync token) with a per-platform-`frontend` adapter behind it, shared memory as universal fallback; see `research/frontend-ipc.md`
- [ ] Implement the frame-plane interface and the reference frontend's adapter
- [ ] Build one reference frontend against that boundary, chosen for fastest API validation, not as a platform-native deliverable
- [ ] Confirm the boundary supports runtime show/hide independent of `core` process lifecycle (plan §1)
- [ ] Wire Phase 3 paint output to the reference frontend's on-screen surface
- [ ] Handle scroll input, feeding back into layout/paint as needed
- [ ] Handle click input and hit-testing against the layout tree
- [ ] Handle window resize, feeding back into layout
- [ ] Handle basic navigation (load URL, follow links)
- [ ] Manually verify rendering against the Phase 2 demo page(s)
- [ ] Add a Help/About/Credits screen reproducing the Chromium BSD-3-Clause notice and crediting Gecko (BSD-3-Clause binary-distribution requirement — see Phase 0)
