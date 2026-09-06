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

## MVP CSS scope (decided)

Per `css-cascade.md` §4's recommendations, cross-checked against `layout.md`'s independent flex/grid-layout call. These are two different layers deciding independently, not a conflict: `css-cascade.md` scopes what the **parser/cascade** accepts and computes (cheap to include, since neither engine special-cases flex/grid at the cascade level — they're ordinary longhands); `layout.md` scopes what the **layout algorithm** (a separate, not-yet-implemented crate) actually lays out. BlueIce's cascade stores `flex-*` computed values from day one; `blueice-layout` simply won't interpret them yet.

**Selectors**: type (`div`), class (`.foo`), ID (`#foo`), universal (`*`), descendant combinator (` `), child combinator (`>`), attribute-presence (`[attr]`) and attribute-equality (`[attr=val]`, `[attr="val"]`). Specificity computed as the standard (id-count, class-like-count, type-count) triple, packed into one comparable value — same encoding both Stylo and Blink independently converged on (`css-cascade.md` §1/§2).

**Deferred selectors**: `:has()`, `:is()`/`:where()`/`:not()`, `:nth-child()`/`:nth-of-type()` and other functional pseudo-classes, sibling combinators (`+`, `~`), shadow-DOM selectors (`:host`, `::slotted`, `::part`), `@scope`.

**Cascade**: two origins only — `UA` (a built-in default stylesheet, itself expressed as ordinary CSS text parsed through the same parser rather than hardcoded Rust, covering sensible defaults for the Phase 2 HTML element list: block/inline `display`, list/table `display` values, heading font sizes, bold/italic text-level elements) and `Author` (the page's own `<style>`/`style=""`), in that precedence order — both engines agree `UA < User < PresHints < Author < Animations < Transitions`; BlueIce's two-tier `UA < Author` is a safe, spec-faithful subset of that ordering, not a guess. `!important` is supported (reverses UA/Author precedence, per spec). Inheritance is supported for the properties real pages depend on it for (`color`, `font-family`, `font-size`, `line-height`, `text-align`). Computed-value resolution covers at least `em`/percentage-of-parent-font-size and `currentColor`.

**Deferred cascade features**: custom properties/`var()`, `@layer`, container queries, `@starting-style`, `revert`/`revert-layer`, animations/transitions as cascade origins, a `User` origin (no browser chrome/user-stylesheet concept exists yet to originate one from).

**Properties**: box model (`display: none|block|inline|inline-block|flex`, `width`, `height`, `margin`/`padding` — including the 1-to-4-value shorthand syntax, since it's extremely common in real authored CSS — `border-width`/`border-style`/`border-color`, `box-sizing`), typography (`font-family`, `font-size`, `font-weight`, `font-style: normal|italic`, `color`, `line-height`, `text-align`), `background-color`, positioning (`position: static|relative|absolute`, `top`/`right`/`bottom`/`left`), and basic single-axis flexbox (`flex-direction`, `justify-content`, `align-items`, `flex-grow`/`flex-shrink`/`flex-basis`) per `css-cascade.md` §4's flexbox-in/grid-deferred call. `font-style` is a small addition beyond `css-cascade.md`'s own list, added during implementation because it's the one property the UA stylesheet needs to give `<i>`/`<em>` their defining visual difference from plain text — arguably the single most common real-world use of the cascade on "simple" pages, so omitting it wasn't a defensible cut. Values supported: keywords, lengths (`px`, `em`, unitless `0`), percentages, named colors (a common subset) and `#rgb`/`#rrggbb` hex colors, and the `currentColor` keyword. Grid is entirely deferred, at both the cascade and layout level.

Table-related `display` keywords (`table`, `table-row`, `table-cell`, ...) are explicitly **not** in the supported `display` value list above — table layout itself is deferred with Grid, so the UA stylesheet treats `<table>`/`<tr>`/`<td>`/`<th>` as plain `display: block` for now (stacked block boxes, not a real table layout) rather than introducing display keywords nothing downstream can act on yet.

**Parsing strategy**: a from-scratch CSS Syntax-level tokenizer + recursive-descent selector/declaration parser, consistent with the project's "written from scratch in Rust" identity (`CLAUDE.md`) and the same choice already made for the HTML tokenizer — not vendoring Stylo's `style`/`selectors` crates (their `TElement` trait integration surface alone is 82 methods against a DOM they don't know about, per `css-cascade.md` §1) nor pulling in the standalone `cssparser` crate as an external dependency, since the MVP's ~30-40 property, two-origin cascade is tractable to tokenize from scratch at comparable effort to the HTML tokenizer. Unknown at-rules (`@media`, `@font-face`, `@import`, ...) are recognized and skipped (consumed to their matching block or `;`) rather than corrupting the rest of the stylesheet — the same "ignore, don't crash" error-recovery philosophy as the HTML parser, not full support.

## Checklist

- [x] Read Gecko/Blink's CSS cascade and layout code in `../reference/`; write up findings in `../research/css-cascade.md` and `../research/layout.md`
- [x] Read Gecko/Blink's HTML tokenizer/tree-builder code in `../reference/`; write up findings in `../research/html-parsing.md`
- [x] List supported HTML elements/attributes for MVP — see "MVP HTML scope (decided)" above
- [x] List supported CSS selectors/properties for MVP; decide flexbox/grid in-or-out explicitly — see "MVP CSS scope (decided)" above (flexbox in at the cascade level, Grid deferred entirely)
- [x] Decide the JS strategy for MVP — custom engine (BlueJS), tracked in [Phase 13](../phase-13-bluejs-engine/PLAN.md)
- [ ] Scope the MVP JS language-feature and DOM-binding subset (separate from Phase 13's engine-architecture questions)
- [ ] Re-confirm and extend the MVP non-goals list from plan §4
- [ ] Define one or more concrete demo pages the MVP must render correctly as its acceptance bar
- [ ] Cross-check the scope against the Phase 1 representation-layer decision (the chosen representation must be extractable from whatever this scope actually renders)
