## Research notes

Findings from reading the [`../reference/`](../reference/) Gecko and Chromium checkouts, written up as durable notes rather than left as one-off reading. This is where "read the source" turns into something the project can actually decide from and cite later.

This research runs in parallel with, and directly feeds, several phases — it isn't a phase of its own with a single done/not-done state:

- **Phase 1** (AI representation layer): needs notes on Gecko's accessibility-tree implementation and Blink's DOM/CSSOM/accessibility code before the §3 decision can be made on real evidence instead of general knowledge of the four candidates.
- **Phase 2** (MVP scope): needs notes on how Gecko/Blink actually structure HTML parsing, CSS cascade, and layout before a "minimal subset" can be scoped concretely rather than guessed at.
- **Phase 3** (engine skeleton): the notes plus the reference checkouts themselves are the direct porting basis once implementation starts.

### Organization

One file per subsystem studied, named for the subsystem rather than which phase requested it (a subsystem's notes may inform more than one phase):

- [x] `dom.md` — DOM tree structure and node identity in Gecko/Blink
- [x] `css-cascade.md` — CSS parsing and cascade algorithm
- [x] `layout.md` — layout/box-tree algorithm
- [x] `accessibility-tree.md` — Gecko's and Blink's accessibility tree implementations (directly relevant to the Phase 1 §3 decision)
- [x] `html-parsing.md` — HTML tokenizer/tree-builder
- [ ] `servo.md` — Servo's module decomposition (plan §4 names it as the closest existing precedent for a Rust engine; Servo isn't cloned under `../reference/` yet — only Gecko and Chromium are)

Each note should record what was actually found — file paths and function/struct names in the reference checkout, not just prose summary — so a later reader can go back to the source instead of trusting the note blindly.
