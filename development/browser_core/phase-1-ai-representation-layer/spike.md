# Phase 1 spike — accessibility-tree vs. custom-hybrid, worked example

[← Back to Phase 1](PLAN.md)

This is the toy-DOM spike called for in the Phase 1 checklist, run against both remaining candidates from plan §3 at once, since [`../research/accessibility-tree.md`](../research/accessibility-tree.md) §4 already argues the custom hybrid is just an accessibility-tree-shaped schema plus a small delta — the cheapest way to test that claim is to build both from the same toy page and diff them, rather than spiking each in isolation.

## Toy DOM and runtime state

```html
<body>
  <h1>Sign in</h1>
  <form id="login">
    <label for="email">Email</label>
    <input id="email" type="email" required>
    <button id="submit" type="submit" style="background:#0060df; transition: background-color .2s">
      Sign in
    </button>
    <p id="error" class="error-msg" style="display:none">Invalid credentials</p>
  </form>
  <div id="banner-overlay" style="position:absolute; inset:0 0 0 40%; opacity:.5; background:#000"></div>
  <a id="forgot" href="/forgot">Forgot password?</a>
</body>
```

Runtime state at the moment this snapshot is taken: the cursor is over `#submit`, so it's mid CSS transition from `#0060df` toward its hover color; `#error` is `display:none` (login hasn't failed yet); `#banner-overlay` is a decorative, non-interactive div with no ARIA role that visually and geometrically covers `#forgot` (a promo banner overlapping the footer, say) at 50% opacity.

## Candidate A — pure accessibility-tree-shaped representation

Modeled directly on Blink's `AXNodeData` (per `accessibility-tree.md` §2): role, state bitset, typed attributes, provenance-tagged name, offset-relative bounds. One record per semantically-relevant node; purely decorative/non-semantic nodes are excluded, matching how both reference engines actually build their trees.

```jsonc
[
  { "id": "h1",     "role": "heading",     "name": "Sign in", "level": 1 },
  { "id": "email",  "role": "textbox",     "name": "Email", "required": true, "bounds": [...] },
  { "id": "submit", "role": "button",      "name": "Sign in",
    "state": ["hovered"], "color": "#ffffff", "backgroundColor": "#0060df",
    "bounds": [...] },
  // #error: display:none -> no frame -> absent from the tree entirely, per
  // both engines' VisibilityState()/ComputeIsHiddenViaStyle() behavior
  { "id": "forgot", "role": "link", "name": "Forgot password?", "bounds": [...] }
  // #banner-overlay: no ARIA role, not focusable, purely decorative -> absent
  // from the tree entirely in both reference engines, same as #error
]
```

## Candidate B — custom hybrid (Candidate A + the four deltas from `accessibility-tree.md` §4)

```jsonc
[
  { "id": "h1",     "role": "heading", "name": "Sign in", "level": 1,
    "opacity": 1, "animating": false, "occluded": false },
  { "id": "email",  "role": "textbox", "name": "Email", "required": true, "bounds": [...],
    "opacity": 1, "animating": false, "occluded": false, "focused": false },
  { "id": "submit", "role": "button", "name": "Sign in",
    "color": "#ffffff", "backgroundColor": "#1a70e8",   // <- interpolated, not the CSS source value
    "hovered": true, "focused": false,
    "opacity": 1, "animating": true, "occluded": false,
    "bounds": [...] },
  // #error: still absent -- display:none exclusion is unchanged, see below
  { "id": "forgot", "role": "link", "name": "Forgot password?", "bounds": [...],
    "opacity": 1, "animating": false,
    "occluded": true, "occludedBy": "#banner-overlay", "occludedFraction": 0.6 }
  // #banner-overlay itself: still absent as its own record -- see finding 2 below
]
```

## What the worked example actually surfaces

1. **The transition/animation field needs to carry an interpolated value, not just a boolean.** `backgroundColor` at this instant is `#1a70e8` — partway between the CSS source value and the hover target — not the static `#0060df` a DOM+CSSOM reading would give, and not usefully expressed as `animating: true` alone. This confirms `research/dom.md`'s and `research/accessibility-tree.md`'s framing was slightly under-specified: the delta field isn't just "is this animating," it's "read computed style at the current instant," which BlueIce's shared render pass can do for free (it already computes this to paint the frame) but neither reference engine's AX schema attempts at all.

2. **Occlusion cannot be modeled by exposing the occluding node — it has to be a flag on the occluded node.** `#banner-overlay` has no semantic role and would be correctly excluded from *both* candidates as its own record, by the same logic that excludes `#error` — but that means Candidate B can't answer "why is `#forgot` unclickable" by adding the overlay as a new tree entry; the occlusion test has to run during the paint/compositing pass BlueIce already does, and stamp its result (`occluded`, `occludedBy`, `occludedFraction`) directly onto the covered node's own record. This is a real, previously-unstated design requirement the abstract research alone didn't surface — the spike found it by forcing a concrete example through the schema.

3. **`display:none` exclusion is confirmed correct, not a gap to fix.** `#error` is absent from both candidates. This matches plan §1's actual goal — the AI should perceive what the human currently perceives, and a human looking at this page right now cannot see the error message either. Making `#error` visible to the AI while invisible to the human would violate the project's own premise, not improve on it. (If BlueIce later wants an AI capability like "list all messages this page *could* show," that's a distinct, deliberately-different query — e.g. a raw DOM dump — not a gap in this representation.)

4. **The delta really is small in practice, as `accessibility-tree.md` predicted.** Diffing the two candidates above: every Candidate-A record survives unchanged into Candidate B, plus a handful of new fields (`opacity`, `animating`, `occluded`/`occludedBy`/`occludedFraction`, explicit `hovered`/`focused` booleans) that are either always `false`/`1`/absent (cheap) or, when present, are read from data the render pass already has. No new nodes, no restructuring, no new identity scheme — this is consistent with the "small, enumerable delta over `AXNodeData`" recommendation, not a from-scratch design.

## Recommendation

Adopt Candidate B: an accessibility-tree-shaped schema (role, state, provenance-tagged name, offset-relative bounds — following `AXNodeData`'s field categories) extended with `opacity`, `animating` (carrying the live interpolated value(s), not just a bool), `occluded`/`occludedBy`/`occludedFraction`, and always-populated per-node `hovered`/`focused` booleans. Keyed by BlueIce's stable per-node `NodeId` (plan §1, `research/dom.md`) rather than a separate AX-ID scheme. `display:none` and purely-decorative non-semantic nodes stay excluded, matching human perception per plan §1 — this is deliberate parity, not an omission to fix later.

This resolves the plan §3 open question. **Still needs your sign-off before I update `BROWSER_CORE_PLAN.md` §3 and the plan's top-level candidate table** — this is the single most consequential, hardest-to-reverse call in the whole design so far, and everything from here (Phase 1's remaining checklist items, all of Phase 5) builds on it.
