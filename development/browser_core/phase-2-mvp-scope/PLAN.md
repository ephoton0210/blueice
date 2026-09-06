# Phase 2 — MVP Scope

[← Back to plan](../BROWSER_CORE_PLAN.md)

**Status**: In progress

## Objective

Turn the general MVP boundary already agreed in plan §4 ("a minimal usable HTML parse → DOM → CSS cascade → layout → paint pipeline, nothing more") into a concrete, checkable list of what HTML/CSS/JS is actually supported, so Phase 3 has a fixed target instead of an open-ended one.

## Plan

Scope each layer of the pipeline independently, favoring "smallest subset that can render real, simple pages" over completeness:

- **HTML**: which elements and attributes are parsed and represented in the DOM (structural elements, text, forms, media placeholders vs. actual media support).
- **CSS**: which selectors and properties participate in the cascade and layout (box model, basic flow layout at minimum; explicitly decide whether flexbox/grid are in or out of MVP rather than leaving it implicit).
- **JS**: **decided — a custom, from-scratch engine ("BlueJS"), not an embedded existing engine.** See [Phase 13](../phase-13-bluejs-engine/PLAN.md) for the engine itself; this phase still needs to scope which JS *language features* and DOM bindings are in the MVP subset, same as HTML/CSS below.
- Re-confirm the explicit non-goals already listed in plan §4 (extension ecosystem, multi-tab state sync, full JS engine optimization, DevTools) still hold, and add any newly discovered ones.
- Ground each of the above against the Gecko/Chromium reference checkouts under [`../reference/`](../reference/) rather than scoping from memory — write up what's actually found as [`../research/`](../research/) notes (`dom.md`, `css-cascade.md`, `layout.md`) so the scoping decisions here can cite specifics.

**Research done for all three layers.** [`../research/css-cascade.md`](../research/css-cascade.md) found flexbox/grid are cleanly separable from cascade (ordinary longhands, no special cascade path) but not from layout (each is its own large subsystem in both engines) — recommends including basic single-axis flexbox in MVP and deferring Grid. [`../research/layout.md`](../research/layout.md) recommends an immutable-fragment-tree design (Blink LayoutNG-style) over Gecko's mutable frame graph as a better fit for Rust, with flex/grid as later dispatch arms rather than a rewrite. [`../research/html-parsing.md`](../research/html-parsing.md) found the tokenizer/tree-builder split is a real interface boundary in both engines (not just file organization), that adoption-agency/foster-parenting error recovery is load-bearing even for "simple" real pages, and that foreign content (SVG/MathML) is a cleanly-gated subsystem safe to cut wholesale for MVP — notably, Blink ships its own restricted fast-path HTML parser (`HTMLDocumentParserFastpath`, a 19-tag whitelist with no auto-closing/misnesting handling, falling back to the full parser on anything unsupported) as direct production precedent for this kind of MVP cut.

## MVP HTML scope (decided)

Concrete element/attribute list, scoped per `html-parsing.md` §4's recommendation (keep the full tokenizer state machine and adoption-agency/foster-parenting error recovery; cut foreign content, `document.write` reentrancy, speculative parsing, and the full named-character-reference table) and cross-checked against Blink's own `HTMLDocumentParserFastpath` whitelist (`a, b, body, br, button, div, footer, i, input, li, label, option, ol, p, select, span, strong, ul`) as production precedent for exactly this kind of MVP cut — extended beyond that 19-tag fast-path list since BlueIce's parser is a full tree builder, not a bail-out-on-anything-complex fast path.

**Elements**:

- Document structure: `html`, `head`, `body`, `title`, `meta`, `link`, `style`, `script` — `script` is recognized and tokenized as RAWTEXT so its content never corrupts tree construction, but it is *not executed*; execution is Phase 13/BlueJS's concern, wholly outside the parser's job.
- Grouping/sectioning: `div`, `span`, `p`, `br`, `hr`, `section`, `article`, `header`, `footer`, `nav`, `main`, `aside`, `ul`, `ol`, `li`, `pre`, `blockquote`, `figure`, `figcaption`
- Headings: `h1`–`h6`
- Text-level semantics (also: the formatting-element set below): `a`, `b`, `i`, `em`, `strong`, `u`, `small`, `code`, `sub`, `sup`
- Forms: `form`, `input`, `button`, `label`, `select`, `option`, `optgroup`, `textarea`, `fieldset`, `legend`
- Tables: `table`, `caption`, `colgroup`, `col`, `thead`, `tbody`, `tfoot`, `tr`, `td`, `th`
- Media placeholder: `img` — DOM node + attributes only, no decode/fetch (plan §4's "media placeholders vs. actual media support" distinction)

**Void elements** (no children, no end tag expected — tree builder never pushes these onto the open-elements stack): `br`, `hr`, `img`, `input`, `link`, `meta`, `col`.

**RAWTEXT elements** (tokenizer switches content model; content is opaque text up to the matching end tag, never tree-constructed): `script`, `style`.
**RCDATA elements** (tokenizer switches content model; content is text but character references still apply): `textarea`, `title`.

**Formatting elements** (participate in the active-formatting-elements list / adoption agency algorithm — `html-parsing.md` §3 found this load-bearing for ordinary misnested markup, not just adversarial input): `a`, `b`, `i`, `em`, `strong`, `u`, `small`, `code`.

**Elements that close an open `<p>`** (spec's paragraph-closing rule, restricted to what's actually in scope above): `div`, `p`, `ul`, `ol`, `li`, `h1`–`h6`, `blockquote`, `pre`, `section`, `article`, `header`, `footer`, `nav`, `main`, `aside`, `figure`, `figcaption`, `form`, `table`, `fieldset`.

**Attributes**:
- Global (any element): `id`, `class`, `style`, `title`, `hidden`, `tabindex`, `lang`, `dir`, `data-*`
- `a`: `href` · `img`: `src`, `alt`, `width`, `height` · `link`: `rel`, `href` · `meta`: `name`, `content`, `charset` · `script`: `src`, `type` (not executed)
- `form`: `action`, `method` · `input`: `type`, `name`, `value`, `placeholder`, `checked`, `disabled`, `required` · `button`: `type`, `disabled` · `label`: `for`
- `select`: `name`, `disabled`, `multiple` · `option`: `value`, `selected`, `disabled` · `textarea`: `name`, `rows`, `cols`, `placeholder`, `disabled`
- `td`/`th`: `colspan`, `rowspan` · `col`: `span`

The tokenizer/DOM don't reject attributes outside this list — real pages carry `role`/`aria-*`/etc. this list doesn't enumerate, and later phases (CSS, Phase 5's accessibility output) will need some of those. This list is what CSS/layout/accessibility can rely on having defined *meaning* for, not a parser-enforced allowlist.

**Explicitly deferred / cut** — parsed as a generic unknown element (no special insertion-mode behavior, no rejection): `svg`, `math` and all foreign-content descendants (the "honest cut" `html-parsing.md` §4 calls for: no HTML-breakout list, no namespace adjustment, no integration-point checks); `template`, `slot`, `dialog`, `details`, `summary`, `iframe`, `canvas`, `audio`, `video`, `noscript`, `frameset`/`frame`/`noframes` (legacy), `datalist`, `output`, `progress`, `meter`, `map`, `area`.

**Named character references**: only `&amp;`, `&lt;`, `&gt;`, `&quot;`, `&apos;`, `&nbsp;`, plus numeric (`&#…;`) and hex (`&#x…;`) references — matching Blink's own fast-path hand-covered set (`html-parsing.md` §3). The full 2231-entry table is future work, added once a real page is observed needing an entry outside this set.

## Checklist

- [x] Read Gecko/Blink's CSS cascade and layout code in `../reference/`; write up findings in `../research/css-cascade.md` and `../research/layout.md`
- [x] Read Gecko/Blink's HTML tokenizer/tree-builder code in `../reference/`; write up findings in `../research/html-parsing.md`
- [x] List supported HTML elements/attributes for MVP — see "MVP HTML scope (decided)" above
- [ ] List supported CSS selectors/properties for MVP; decide flexbox/grid in-or-out explicitly (see `css-cascade.md`/`layout.md` — leaning toward basic flexbox in, Grid deferred)
- [x] Decide the JS strategy for MVP — custom engine (BlueJS), tracked in [Phase 13](../phase-13-bluejs-engine/PLAN.md)
- [ ] Scope the MVP JS language-feature and DOM-binding subset (separate from Phase 13's engine-architecture questions)
- [ ] Re-confirm and extend the MVP non-goals list from plan §4
- [ ] Define one or more concrete demo pages the MVP must render correctly as its acceptance bar
- [ ] Cross-check the scope against the Phase 1 representation-layer decision (the chosen representation must be extractable from whatever this scope actually renders)
