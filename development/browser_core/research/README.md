## Research notes

Findings from reading the [`../reference/`](../reference/) Gecko and Chromium checkouts, written up as durable notes rather than left as one-off reading. This is where "read the source" turns into something the project can actually decide from and cite later.

This research runs in parallel with, and directly feeds, several phases — it isn't a phase of its own with a single done/not-done state:

- **Phase 1** (AI representation layer): §3 decision settled from `accessibility-tree.md`/`dom.md`.
- **Phase 2** (MVP scope): `html-parsing.md`/`css-cascade.md`/`layout.md` ground the HTML/CSS subset decisions.
- **Phase 3** (engine skeleton): the notes plus the reference checkouts themselves are the direct porting basis once implementation starts.
- **Phase 4** (frontend): `frontend-ipc.md` grounds the core↔frontend frame-delivery channel decision.
- **Phase 7** (local AI): `safe-browsing-enforcement.md` grounds the gatekeeper enforcement mechanism; `multi-process-memory.md` grounds resident-vs-idle-teardown decisions for `ai-assistant`.
- **Phase 8** (hot-swap): `multi-process-memory.md` extends the launcher's role to fleet-wide idle-teardown authority.
- **Phase 9** (extension protocol): `extension-architecture.md` grounds the capability list, manifest model, and WASM-over-native decision.
- **Phase 10/12** (downloads, MCP server): `multi-process-memory.md` grounds their idle-teardown/on-demand-spawn placement.
- **Phase 13** (BlueJS): `js-engine-gc.md` and `js-bytecode-eventloop.md` ground the GC algorithm, bytecode format, and event-loop shape.

### Organization

One file per subsystem studied, named for the subsystem rather than which phase requested it (a subsystem's notes may inform more than one phase):

- [x] `dom.md` — DOM tree structure and node identity in Gecko/Blink
- [x] `css-cascade.md` — CSS parsing and cascade algorithm
- [x] `layout.md` — layout/box-tree algorithm
- [x] `accessibility-tree.md` — Gecko's and Blink's accessibility tree implementations (directly relevant to the Phase 1 §3 decision)
- [x] `html-parsing.md` — HTML tokenizer/tree-builder
- [x] `js-engine-gc.md` — SpiderMonkey GC design (V8's own source wasn't available in this checkout — see the note in the file)
- [x] `multi-process-memory.md` — Chromium/Firefox multi-process fleet memory management
- [x] `extension-architecture.md` — Chromium Manifest V3 and Firefox WebExtensions capability/permission/format design
- [x] `safe-browsing-enforcement.md` — Chromium Safe Browsing and Firefox url-classifier enforcement architecture
- [x] `frontend-ipc.md` — Chromium Viz and Firefox WebRender cross-process frame delivery
- [x] `js-bytecode-eventloop.md` — SpiderMonkey bytecode format and Blink/Gecko event-loop/render-pass interleaving (V8/Ignition's own source wasn't available in this checkout — see the note in the file)
- [ ] `servo.md` — Servo's module decomposition (plan §4 names it as the closest existing precedent for a Rust engine; Servo isn't cloned under `../reference/` yet — only Gecko and Chromium are)

Each note should record what was actually found — file paths and function/struct names in the reference checkout, not just prose summary — so a later reader can go back to the source instead of trusting the note blindly.
