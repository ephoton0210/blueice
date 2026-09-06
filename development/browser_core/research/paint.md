# Paint architecture in Gecko and Blink

Findings from a targeted read of `reference/gecko/layout/painting/` and
`reference/chromium/third_party/blink/renderer/platform/graphics/paint/`,
scoped specifically to the question Phase 3's paint checklist item
needs answered: what does "paint" actually produce, as a data shape,
between layout and an actual pixel buffer? This is a narrower pass than
`html-parsing.md`/`css-cascade.md`/`layout.md` (no exact line-by-line
inventory) — paint's MVP scope is correspondingly narrower too: real
rasterization (glyph shaping, GPU compositing) is out of scope until
Phase 4 has a real window to draw into, so the architectural question
that actually matters now is the *shape of the intermediate
representation*, not the rasterizer.

## Both engines: layout tree → display list → (separately) raster

Neither engine paints directly from the layout/fragment tree to pixels.
Both interpose a **display list**: an ordered collection of typed,
immutable "draw this" records, built once per paint pass and handed to
a separate rasterization/compositing stage.

- **Gecko** (`layout/painting/nsDisplayList.h`): `nsDisplayListBuilder`
  walks the frame tree and produces an `nsDisplayList` of
  `nsDisplayItem` subclasses. The concrete item classes are exactly a
  "what kind of thing is this" taxonomy: `nsDisplayBackgroundColor`,
  `nsDisplayBorder`, `nsDisplaySolidColor`, `nsDisplayBackgroundImage`,
  `nsDisplayBoxShadowOuter`/`Inner`, `nsDisplayOutline`, plus (declared
  in `layout/generic/nsTextFrame.h`, not the painting directory itself)
  `nsDisplayText` for text runs. Container/effect items
  (`nsDisplayOpacity`, `nsDisplayBlendMode`, `nsDisplayOwnLayer`, ...)
  wrap child lists rather than drawing anything themselves.
- **Blink** (`platform/graphics/paint/`): `PaintController` records a
  flat sequence of `DisplayItem`s per paint, most concretely
  `DrawingDisplayItem` — its own header comment states the contract
  directly: "contains recorded painting operations which can be
  replayed to produce a rastered output" (`drawing_display_item.h`).
  Actual replay happens later, via a `PaintRecord` (a Skia
  `PaintRecord`/display-list-of-Skia-ops) consumed by the compositor
  (`cc`) on a separate thread/process, not synchronously during paint
  itself.

**The shared, load-bearing idea**: paint's job is to turn "boxes with
computed styles" into an ordered, typed list of draw operations — not
to rasterize. Rasterization is a distinct, later, platform/GPU-specific
concern in both engines (Skia canvas playback for Blink; a similar
canvas/DrawTarget abstraction for Gecko), decoupled specifically so the
same display-list shape can be consumed differently by different
backends. That decoupling is exactly what BlueIce needs too, for a
different reason: `frontend` is per-platform (WinUI 3/SwiftUI/Qt, per
`BROWSER_CORE_PLAN.md` §1), so an intermediate, platform-agnostic
"paint command list" is the natural handoff point — one `blueice-paint`
crate that's the same regardless of which native frontend eventually
consumes its output, rather than baking a specific rendering API in
now.

## Recommendation for BlueIce's MVP

**`blueice-paint` produces a flat, ordered `Vec` of typed paint
commands from a `Fragment` tree — not pixels.** Concretely, per the
Phase 2 CSS property scope already implemented (background-color,
border-*, color/font-*), a command enum with exactly the cases real
pages actually need for MVP:

- A filled rectangle (background-color) — Gecko's
  `nsDisplayBackgroundColor`/`nsDisplaySolidColor`, Blink's solid-color
  `DrawingDisplayItem`s.
- A rectangle outline (border) — `nsDisplayBorder`. Only solid,
  single-color, single-width borders per side for MVP (matching the
  `border-*-width`/`border-*-style`/`border-*-color` properties already
  cascaded) — no border-radius, no per-side style variation beyond
  solid/none.
- A text run (position, content, color, font-size) —
  `nsDisplayText`/text `DrawingDisplayItem`s. No real glyph
  shaping/rasterization — that needs an actual font/text-shaping
  subsystem neither engine's *display-list* layer owns either (it's
  produced further down, at replay time); BlueIce's MVP command just
  carries what a future text-rendering backend would need (string,
  origin, color, font size) the same way a display item does before
  replay.

**Explicitly out of scope for MVP, named rather than silently missing**
(all of these are separate, later concerns in both reference engines
too, not gaps unique to cutting corners here): actual rasterization to
a pixel buffer (needs a real font/glyph and 2D-drawing backend, which
is a Phase 4 frontend concern, per-platform); clipping and
transforms/opacity (Gecko's `DisplayListClipState`,
`nsDisplayOpacity`/`nsDisplayOwnLayer`, Blink's
`ClipPaintPropertyNode`/`TransformPaintPropertyNode`); compositing into
layers (`cc`, Gecko's `WindowRenderer`); paint invalidation/caching
across frames (`RetainedDisplayListBuilder`, Blink's
`RasterInvalidator`) — BlueIce repaints the whole tree every call for
now, matching `research/layout.md`'s equivalent "no incremental
layout" MVP cut; box-shadow, outline, background-image, gradients.

**No new dependency, no vendored code**: this is a small, from-scratch
enum + builder, consistent with every other MVP-scoped stage this
phase has built. The main thing worth porting from either reference is
the *shape* (a flat ordered list of typed, self-contained commands),
not any actual code.
