# Phase 27 — Browsable Web MVP: Event Loop, Page DOM, and AJAX

[← Back to plan](../BROWSER_CORE_PLAN.md)

**Status**: In progress — branch `feature/browsable-web-mvp` is cut from
`feature/phase-7-6-8-9-followup` with `feature/test262-remaining-failures`
(PR #25) merged in (see "Integration record"). The integrated
`feature/bluejs-browser-integration-gaps` branch completes M1a checkpoints,
M0a compatibility, M1b session-driven timers and M1c's separate animation-frame
render entry with real frame/Representation acceptance. M1d bounds document
scheduling work and owned payloads, including ShadowRealms. M1e charges that
payload to Core's live per-tab ledger. M1f restores collectible Promise state
and awaited for-await cleanup with bounded timer-poll regressions. M1g adds
document-owned finalization tasks, automatic checkpoint discovery and shared
browser-task admission across ShadowRealms. M1 remains
in progress for task-source ordering and broader host-buffer/aggregate
accounting. M1h adds native DOM callback cleanup with a shared 256-job budget,
preserved dispatch fuel and rooted frames during module jobs; script-driven
dispatch retains its enclosing JavaScript ordering. The
[milestone record](../phase-13-bluejs-engine/BROWSER_INTEGRATION_MILESTONES.md#milestone-record)
tracks its acceptance without closing M1 as a whole.

M1i shares timer/browser work in one bounded document task entry. Core can
publish a render pass after each selected task; ready browser work and future
abort deadlines participate in wake scheduling. The existing render entry
stays separate. General logical source associations, network/UI ordering and
broader host-buffer policy keep M1 in progress.

M1j replaces storage-group scheduling with logical document task sources and
one page timeout clock. Same-source order spans same-document realms; source
selection rotates before returning to Core. Real network/UI ingress, broader
host buffers keep M1 in progress; M1o/M1p below supply reporting. See
[the policy](../phase-13-bluejs-engine/HOST_TASK_SOURCES.md).

M1k adds owner-installed Promise-returning service operations and branded async
methods. Opaque completions join document task sources and existing admission;
realm teardown cancels unfinished waits. This establishes the engine host-wait
mechanism while real network/UI ingress, external service buffers and reporting
keep M1 in progress. See
[the contract](../phase-13-bluejs-engine/HOST_ASYNC_OPERATIONS.md).

M1l installs top-level `window`, `self`, `top` and `parent` in the active
File DOM v13 profile. Generated host-member signatures and an independent
real Core/child global-inventory test cover JavaScript and checked BlueTS.
The typed profile remains a bounded installed surface; nested browsing
contexts and WindowProxy remain open; M1o/M1p below supply global
error/rejection delivery. See [the global contract](../phase-13-bluejs-engine/WINDOW_GLOBALS.md).

M1m completes console's MCP route. File DOM v14 adds typed,
bounded page logging; private v60 drains one document's copied records through
Core to MCP `get_console_messages`. Real mixed-language MCP acceptance passes,
including task/native-click output and navigation isolation. Rust/build/Clippy
gates pass and the complete Test262 inventory matches M1l; see the
[milestone record](../phase-13-bluejs-engine/BROWSER_INTEGRATION_MILESTONES.md#milestone-record).
The M1o/M1p follow-ups below supply exception and rejection reporting; the
remaining M1 policy work stays open.

M1n's checked host-boundary correction is accepted. Missing member
chains now fail before a checked page's earlier DOM writes can run; ambient
callable queries retain their declarations and local shadows. The existing
installed surface is unchanged. Complete compiler/bridge and related host/MCP
acceptance, workspace build/Clippy/formatting and pinned Test262 gates pass. See the
[live acceptance record](../phase-13-bluejs-engine/BROWSER_INTEGRATION_MILESTONES.md#milestone-record).

M1o's exception-reporting follow-up is accepted. File DOM v15
dispatches cancelable error events and sends uncanceled bounded diagnostics
through the existing Core/MCP console channel. Real pages and the official
MCP client verify continuation after task/native-listener throws. Related
Rust, format, workspace Clippy/build and complete pinned Test262 gates pass.
Final evidence is recorded in the
[milestone record](../phase-13-bluejs-engine/BROWSER_INTEGRATION_MILESTONES.md#milestone-record).
M1p now implements general page rejection tracking; broader host-buffer policy
keeps M1 open.

M1p's rejection follow-up is accepted. File DOM v16 installs both
global rejection events with immutable-profile compatibility. Pending roots,
weak outstanding notifications, cancellation, early/late handlers and
navigation are covered by public VM/profile and real Core/MCP acceptance.
Uncanceled copied diagnostics use the existing private v60 console route.
All 37 related Rust targets, formatting and workspace/all-target Clippy/build
pass. The complete pinned Test262 inventory matches M1o; final evidence is in the
[milestone record](../phase-13-bluejs-engine/BROWSER_INTEGRATION_MILESTONES.md#milestone-record);
see [the rejection contract](../phase-13-bluejs-engine/HOST_PROMISE_REJECTIONS.md).

M1q is in progress. A verified source-provenance foundation supplies original
classic JavaScript statement positions through the real Core/child error
handler and charges immutable maps with installed code. The cleanup follow-up
preserves these origins through finalizers, ignored iterator-close failures
and cross-script generator resumption. Bounded optional source records now
pass through private v61, exact-document Core validation and MCP's untrusted
frame, with message/source byte accounting. Typed, expression,
module and asynchronous positions remain required; see
[the live record](../phase-13-bluejs-engine/BROWSER_INTEGRATION_MILESTONES.md#milestone-record).

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

**M1 progress (2026-10-14):** The integrated BlueJS branch now provides bounded
host-owned microtask checkpoints between script declarations, including throws
and completed debugger turns. Browser-task and native-event checkpoints
continue after job exceptions; page top-level-await drains obey the limit.
M1b adds owner-selected File DOM timers and `queueMicrotask`, generated v11
typings, exact-document relative wake-up propagation, and session tick delivery.
Real JavaScript and BlueTS counter pages each produce three distinct unsolicited
frames with increasing generation and matching Representation generation/text;
clear and navigation stop old timer work. See the
[milestone results](../phase-13-bluejs-engine/BROWSER_INTEGRATION_MILESTONES.md#milestone-record).
M1c adds `requestAnimationFrame`/`cancelAnimationFrame`, the exact v12 artifact,
and private v58 render requests. A Core-owned software cadence runs the callback
snapshot and its microtask cleanup before dirty frame publication. JavaScript
and BlueTS counter fixtures also verify direct rAF self-rearming without further
timers/frontend work. M1d adds shared document admission for queued jobs,
waiting reactions/requests and owned scheduling payloads.
M1e propagates that payload through private v59 acknowledgements to the exact
live Core tab, including post-turn refresh and lifecycle/revocation release.
M1f adds live-owner Promise tracing, explicit async/host roots and awaited
for-await cleanup, with managed-heap/native-payload timer-poll acceptance.
M1g adds a separate cleanup task source and shared document browser-task cap,
including task delivery and FIFO across ShadowRealms.
Task-source ordering and complete host-buffer/aggregate accounting remain
open, so M1 remains in progress.

- **M0 — Integration.** Merge PR #25's BlueJS line; re-verify. (Done for the
  merge; verification is the open part — see below.)
- **M1 — Event loop and timers.** *(Engine side moved: timers, Promise
  collectibility and the other engine gaps are tracked in
  [`phase-13-bluejs-engine/BROWSER_RUNTIME_GAPS.md`](../phase-13-bluejs-engine/BROWSER_RUNTIME_GAPS.md)
  and land on a dedicated BlueJS branch after PR #25 merges to `main`; a
  timer mechanism and parked argument/nesting limits are reconciled by M1b,
  and M1c adds the separate render entry for animation-frame callbacks.
  The core-side frame push below stays in this phase.)* `setTimeout`/`clearTimeout`/
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

**2026-10-15 follow-up:** M1f on `feature/bluejs-browser-integration-gaps`
addresses the Promise lifetime and iterator cleanup differences described
above. The merge record remains historical; current implementation and full
Test262 acceptance are recorded in the
[M1f milestone](../phase-13-bluejs-engine/BROWSER_INTEGRATION_MILESTONES.md#milestone-record).

Verification gate before this branch is treated as integrated:
`cargo test -p blueice-bluejs` (including the Test262-dependent integration
tests, which need the pinned corpus at
`development/browser_core/reference/test262`), the stream/tee/async-iteration
and `AbortSignal` cases, the full `cargo test --workspace`, clippy with
`-D warnings`, and the Test262 inventory run recorded in
`phase-13-bluejs-engine/TEST262_ANALYSIS_REPORT.md`.

## Real-site layout gap (measured 2026-10-28)

Rendering `https://www.google.com` and `https://www.cloudflare.com` (HTML saved
with a desktop user agent, laid out at 1280 px by `render`, pixels by
`blueice_raster::rasterize`) shows the engine is still the Phase 3 block/inline
skeleton, which makes real-page BlueJS acceptance unreliable: scripts mutate
pages whose layout is wrong. Measured, not inferred:

| Page | Observed | Cause |
| --- | --- | --- |
| Google | CSS source printed as text; one stacked column; black rectangles for SVG; no search box layout | `<style>` inside a `<span>` leaked into the line (fixed 2026-10-28, `collect_words` skips `display:none` descendants); no flex, positioning or `var()`/`calc()`; no SVG |
| Cloudflare | Entirely unstyled, 12,272 px tall, empty boxes for images | all site CSS is in 4 external `<link rel=stylesheet>` files, which core never fetches; no images/SVG |

What the two pages' inline CSS uses (counts of declarations): Google `display:flex`
92, `none` 56, `block` 32, `inline-block` 27, `-webkit-box` 11, `inline-flex` 4,
`table-cell` 2; `position` absolute 58 / relative 52 / fixed 12 / sticky 1;
`var(` 324, `calc(` 26, `@media` 39, `@keyframes` 24, `transform` 56, `z-index` 37,
`overflow` 72, `float` 8, `:not(` 37. Cloudflare's inline CSS is small (10 KB); its
64 `<img>`/140 inline `<svg>` and external sheets carry the rest.

Order of work (each step test-first, with a fixture rendered to pixels):

1. **External stylesheets**: fetch `<link rel=stylesheet>` (and `@import`) through
   `blueice-net`, with the same URL review as other subresources, a size/time
   bound and a failure that leaves the document styled by what loaded.
2. **Computed values**: custom properties (`var()` with fallback and inheritance),
   `calc()`/`min()`/`max()`/`clamp()`, viewport units, `inherit`/`initial`.
3. **Box generation and layout**: `inline-block`, `overflow`, `position`
   (relative, absolute, fixed, sticky) with containing blocks, `z-index`
   stacking, `float`, flexbox (`flex`/`inline-flex`, `-webkit-box`), then
   `table`/`table-cell` and grid.
4. **Replaced content**: `<img>` fetch/decode/paint and inline `<svg>` raster
   (Phase 21 owns the general pipeline; this phase needs a minimum).

Progress (2026-10-28):

- Step 1 is done. After both gatekeeper stages clear, core fetches each
  `<link rel=stylesheet>` with `blueice_net::fetch_subresource_hop` (a single GET
  hop that never classifies `text/css` as a download, with a streamed byte limit),
  at most 16 sheets of 2 MiB each and 6 MiB together, 3 redirects, `text/css`
  only, `CheckUrl` plus extension block rules before every hop; a failed sheet is
  left out. The clearance carries them to the page, which merges each at its
  link's position among the `<style>` tags (and `media` is honoured). `@import`
  and sheets referenced from CSS are not fetched yet. Stylesheet text is not
  given to the content stage of the gatekeeper (that stage reviews HTML).
- Cloudflare's four sheets (655 KB, Tailwind v4: `@layer`, 883 `@supports`, 128
  `@property`, `@container`) used to vanish in the parser because every at-rule
  except `@media` was skipped. `@layer` blocks now join the sheet in source order
  (no layer precedence: an unlayered rule should beat a layered one) and
  `@supports` is true only for properties and values the engine implements, so a
  page gets its fallback. Verified in the native window: headings and spacing now
  follow the site's reset instead of UA defaults.
- Known parser limits found on the way, all in the next steps: a rule body ends at
  its first `}` (no CSS nesting), there is no function token (`var()`, `calc()`,
  `oklch()`, `color-mix()` are unparsed), only `px`/`em`/`%` lengths exist, colour
  is hex/rgb/a short name list, and the cascade tests every rule against every
  element (a 650 KB sheet needs a rule index before it is fast).
- Layout still has only block and inline boxes, so Cloudflare is styled but not
  laid out; step 2 (computed values) and step 3 (flex, positioning) are next.

Progress, second slice (2026-10-28), all test-first with the workspace green:

- **Step 2 (computed values) is done.** The CSS crate now parses 148 named
  colours, `rgb()/hsl()/hwb()/oklab()/oklch()/color()/color-mix()/light-dark()`,
  `calc()/min()/max()/clamp()`, `rem`/`vw`/`vh`/`vmin`/`vmax` and absolute units;
  custom properties inherit with cycle and depth guards and `var()` substitutes
  (shorthands expand afterwards); `inherit`/`initial`/`unset` work everywhere; more
  text properties inherit; tokenizer escapes (`.md\:flex`) and exponents work; the
  selector language gained `+`/`~`, every attribute operator, `:not/:is/:where`,
  `:nth-*` and the structural/form/link states; rules are found through
  id/class/type buckets; a nested rule no longer corrupts a sheet; `flex`, `gap`,
  `inset`, `overflow`, `background`, per-side `border`, `text-decoration` and
  logical shorthands expand. The HTML tokenizer decodes all 2,231 named references.
- **Step 3 (layout) is mostly done.** Block layout (auto margins, min/max, margin
  collapsing, relative/absolute/fixed positioning with containing blocks,
  shrink-to-fit), inline layout (line boxes on a shared baseline, atomic inlines,
  `white-space`, `text-align`, `vertical-align`, CJK breaking), flexbox (the
  spec's algorithm), fragment records for z-index/clipping/visibility, layout
  results memoised (a 650 KB Tailwind page: 82 s to 0.55 s in release, 5 s in
  debug). Paint handles inherited opacity, `visibility`, `overflow` clipping,
  z-order, text decoration and letter-spacing. The UA sheet follows the HTML
  rendering section.
- Cloudflare now shows its orange hero, centred white heading, button and the
  following section; Google's home page shows its links, sign-in button, inputs and
  centred footer. Chromium side-by-side is still to do.

Progress, third slice (2026-10-28), test-first:

- **Table layout** (`layout/src/table.rs`): row groups with header/footer order,
  `colspan`/`rowspan`, automatic column widths between min- and max-content,
  percentage columns, captions, `border-spacing`, `vertical-align`, anonymous rows.
  `table-layout: fixed` is treated as automatic and `border-collapse: collapse` as
  zero spacing.
- **Grid layout** (`layout/src/grid.rs`): `px`/`%`/`fr`/`auto`/`minmax()`/`fit-content()`/
  `repeat()` with `auto-fill`/`auto-fit`, named areas, lines and spans, row/column
  auto-placement (dense too), `gap`, item alignment and auto margins. No subgrid,
  masonry or named lines.
- **Floats** (`layout/src/float.rs`): left/right placement beside earlier floats,
  line boxes narrowed (or lowered) around them, `clear`, containment in
  formatting-context roots, floats escaping a plain parent to later siblings.
  Block formatting-context boxes do not yet move beside floats.
- **Inline boxes** now paint their own background and border, one fragment per line.
- **`border-radius`, `box-shadow` (outer), linear/radial gradients** as new display-list
  commands drawn with tiny-skia; inset shadows and `transform` are not painted.
  `overflow` clipping on a box with `border-radius` is a `PushRoundClip` (closed by the same `PopClip`):
  the rasterizer copies the four corner squares when it opens and puts back what is outside the arcs
  (antialiased) when it closes, so no drawing operation needs to know about it.
- **`::before`/`::after`**: pseudo-element selectors and cascade, `content` strings,
  `attr()`, quotes and hex escapes; generated boxes lay out like children. Counters
  and `url()` content generate nothing. List markers (`disc`/`circle`/`square`,
  decimal, alpha, roman, `start`/`reversed`/`value`).
- **Images**: `<img>` (PNG, JPEG, GIF first frame, WebP, SVG) and CSS `url()` backgrounds
  (size, position, repeat, `object-fit`) fetched behind the same per-hop URL review as
  stylesheets, with byte and count limits; `data:` URLs decoded locally; inline `<svg>`
  rendered (the HTML parser now treats SVG as foreign content: self-closing children,
  camel-case names) with the page's colours reaching it; `<use href="file.svg#id">` of an icon
  sprite fetches the file with the images and carries the used `<symbol>`s inside the drawing;
  a block-level `img`/`svg` takes its picture's size (attributes, CSS, `viewBox` ratio), not the container's width.
  AVIF stills decode through `blueice-avif` (`avif-parse`, MPL-2.0, for the container; `rav1d` 1.1.0,
  BSD-2-Clause, with its assembly feature off so there is no C or assembler build step, for AV1) behind a
  safe `decode()`; all `unsafe` is in that one crate's `av1` module. `zenavif`/`rav1d-safe` were rejected
  (AGPL-3.0). Colour only through matrix coefficients 601/709/2020 and identity, 8/10/12-bit, alpha items
  (straight or premultiplied); no HDR tone mapping, grids or animation. `rav1d` logs to stderr on a bad file
  (its logger callback is private). Decoding is the new
  `blueice-image` crate (SVG through `resvg`, with no text support).
- Known limits: images are fetched before the page is shown, so a page with many large
  images appears later; `srcset` picks its first candidate and `<picture>` is ignored;
  animated GIF/APNG show their first frame; SVG `<text>` and external references do not
  render; CSS stylesheets that `@import` others are still not followed.
- `cargo test -p blueice-engine --release --lib live_page_to_png -- --ignored --nocapture`
  with `BI_URL=...` fetches a real page (no gatekeeper) and writes a PNG, for
  eyeballing against a browser. Cloudflare and Google both show their logos, icons,
  rounded buttons and gradients; their remaining differences from a browser are
  listed rather than hidden: the Cloudflare page keeps large empty bands where
  script-driven content is missing, and Google's header links stack instead of
  forming a row.

Still open, in priority order: `position: sticky`/`fixed` scrolling in the frontends,
`text-overflow`, `@import` and `@container`, `:has()`, `@layer` precedence, `rotate`/`scale`/`skew` transforms,
inset shadows, counters, asynchronous image loading after first paint, and the
Chromium side-by-side.

Acceptance for this gap: the two pages above render with their text readable in
the intended regions, the search field and navigation visible and clickable, and
a Chromium side-by-side recorded (Phase 15 differential harness) with the known
remaining differences listed, not hidden. It does not claim site compatibility.

## Page scripts: the web-compat host (2026-10-28)

Ordinary web JavaScript now runs against the page's own DOM, **in-process and
opt-in** (`blueice-core --web-scripts`, off by default). This is a deliberate
departure from decision 3 above (typed, owner-selected profiles executed by the
out-of-process host) and needs the owner's review before it is on by default:
those profiles stay, unchanged, for BlueTS; this path exists because real sites
need hundreds of web members no per-profile typing exercise will produce soon.

- **Shape.** `engine/src/web/`: one native function, `__bi(op, ...)`, with
  primitives over the page's `Document` (`dom_bridge.rs`, pure Rust, tested without
  a VM), and the web platform written in JavaScript on top (`bootstrap.js`: Node,
  Element and about 70 HTML element classes, events with capture/bubble, `classList`,
  `dataset`, `style`, `querySelector`, `innerHTML`/`insertAdjacentHTML`, `DOMParser`,
  templates, custom elements with upgrade, `MutationObserver`, a real
  `IntersectionObserver` driven by layout rects and scroll, `ResizeObserver`,
  `location`, `history` with `pushState`/`popstate`, `URL`/`URLSearchParams`,
  storage, `matchMedia`, `getComputedStyle`, timers/`requestAnimationFrame`/
  `requestIdleCallback`, `fetch`/`Headers`/`Request`/`Response`/`FormData`,
  `XMLHttpRequest`, `TextEncoder`/`TextDecoder`, `atob`/`btoa`, `structuredClone`...).
  The page can shadow or break all of it without affecting the host, which validates
  every primitive.
- **One DOM.** The realm borrows the page's `Document`, styles and layout boxes for
  each turn and gives them back; changes go through the ordinary restyle, layout,
  paint and frame publication (at most one script relayout per 250 ms).
- **Loading.** After both gatekeeper stages clear, the navigation thread fetches the
  document's external scripts, its module graph (module scripts, `modulepreload`,
  `*-url` island attributes, static imports, literal `import()`), stylesheets and
  images, each hop behind the same URL review. Classic scripts run in order, then
  `defer`, then modules; `DOMContentLoaded`/`load` fire; timers run from the session
  tick; a script that fails ends only itself; startup is bounded to 6 s.
- **Network.** `fetch`/XHR are typed jobs on worker threads (`blueice-net`'s
  `fetch_script_request_hop`): every connection (request, preflight, redirect hop) is
  reviewed first, cross-origin responses need `Access-Control-Allow-Origin` (non-simple
  requests get a preflight), forbidden request headers are dropped, no cookies are sent
  and `Set-Cookie` is never exposed, bodies are bounded (16 MiB). Without a review a
  page cannot fetch at all. PJAX (fetch, `DOMParser`, swap, `pushState`) is covered by a
  real-interface test.
- **Measured (Cloudflare, 2026-10-28).** The page's Astro islands now hydrate (React
  components from the fetched module graph, GSAP, the nav scripts) and 124 images paint
  (92 before). The big empty bands that remain are **not** content hidden by `opacity:0`
  (an earlier note here said scrolling would reveal them; a scrolled-through render
  showed it does not). Probed in the page's own realm: the 500 px band under "Region:
  Earth" is a `<canvas>` (the globe, 1278x500; WebGL content), the hero `<video>` has a
  `poster` in AVIF (not decodable here) and zero height, and the other empty boxes are
  scripted animation hosts. So the remaining blanks are missing features, in order of
  size: `<canvas>` (2D and WebGL), `<video>`/poster painting, AVIF, and `position:
  sticky`/`transform`-driven scenes. (`<canvas>` is now done; see "Canvas and software
  WebGL" below.)
- **Known limits, in the order they bite.** (1) Speed: restyle + layout of the 650 KB
  Tailwind page takes ~0.9 s and every script change pays it (there is no incremental
  restyle), module compilation of the 110-module graph takes ~1.5 s, and starting the
  page's scripts takes ~15 s in total on this page, which blocks the session thread
  (bounded by the 6 s budget, which this page nearly spends). (2) No cookie jar: scripts
  and requests carry none; `document.cookie` is per page and not persistent;
  `localStorage` does not persist. (3) `import.meta.url`, import maps, top-level
  `await` and module workers are unsupported; `<script type=module>` entries are
  linked one entry at a time. (4) No shadow DOM rendering (`attachShadow` returns a
  detached root), no `Range`/`Selection`, no canvas paths/text/gradients, no WebSocket/EventSource, no
  service workers, no synchronous XHR. (5) Scripts' own `fetch`es are not shown in the
  request-lifecycle record that downloads have. (6) The core's AI snapshot does not
  list generated or script-hidden content differently from other content.

## Canvas and software WebGL (2026-10-28/29)

Cloudflare's last big blank band was a three.js globe (react-three-fiber, WebGL2), so the
branch grew a software WebGL and the browser pieces three.js needs. It now renders:
the globe's graticule and the land-point dots (North and South America in the measured
render) paint into the page through the same image map as `<img>`.

- **`backend/core/gl` (`blueice-gl`).** Pure computation, no I/O. A GLSL ES 1.00/3.00
  front end (preprocessor, lexer, parser) and a tree-walking interpreter with
  slot-resolved names; a `Context` holding buffers, VAOs, programs and uniforms
  (arrays, matrices, samplers), textures (wrap, filters, mipmaps, cube maps, half-float
  and packed uploads), framebuffers and renderbuffers. A draw call shades vertices on
  several threads, assembles and clips primitives against the near/far planes
  (x/y are handled by the viewport bounding box), then rasterizes perspective-correct
  with depth, blending, culling, scissor, instancing, points and lines. Fragments are
  shaded in thin row bands taken from a shared queue, with primitives pre-binned to
  the bands they touch. A matrix attribute (`mat4` instance transforms) occupies one
  location per column. A fragment varying the vertex shader lacks links only if the
  fragment shader never reads it (three.js declares unused ones).
- **Budget.** One draw call may shade fragments for 1.5 s (`Context::draw_budget`),
  checked per fragment; a context whose draws keep running out stops drawing after
  three. A page can therefore never hold the session for more than that per draw.
- **Not built.** Screen-space derivatives (`dFdx`/`dFdy`/`fwidth` return 0), per-pixel
  mip level selection (level 0 unless the shader gives a LOD), multisampling,
  `blitFramebuffer`, 3D/array textures, uniform blocks as buffers, transform
  feedback, stencil, queries and sync objects (accepted and inert).
- **Page side.** `web/gl_bridge.rs` is one `gl` primitive (`context id`, `operation`,
  arguments; binary data as base64, number lists as JSON); `bootstrap.js` provides
  `WebGLRenderingContext`/`WebGL2RenderingContext` and the object classes over it.
  `getParameter`/`getExtension` report a small, honest capability set; the renderer
  string is `BlueIce WebGL` (Cloudflare turns 3D off for a renderer whose name
  contains "software") and `WEBGL_debug_renderer_info` is not offered.
- **Canvas 2D (`web/canvas2d.rs`).** A straight-alpha bitmap with `fillRect`,
  `clearRect`, `drawImage` (scaling, cropping, affine transforms, bilinear),
  `getImageData`/`putImageData`, `toDataURL`. Paths, text and gradients are accepted
  and draw nothing. Both canvas kinds paint through the image map, without a restyle
  after the first picture.
- **Images from script.** `new Image().src = url` (and `data:` URLs) is fetched as a
  no-cors request (`NetJob::no_cors`; every hop is still reviewed), decoded, sized,
  and reports `load`/`error`; `decode()` works, and the picture is available to
  `drawImage`/`texImage2D` and to `<img src=url>` painting.
- **Things this needed that are not WebGL.** A 48 MiB single-string limit for page
  realms (the default 1 MiB made any response body over about 768 KB fail, because
  bodies cross as base64 text); `ResizeObserver` reporting size changes after each
  layout and delivering its first report a frame late (react-use-measure ignores a
  size reported before it has mounted); and **finished CSS animations**: a rule
  whose `animation` is finite and keeps its last frame (`forwards`/`both`) gets that
  keyframe applied as its final state, because Cloudflare hides every canvas with
  `opacity:0; animation: ... both canvas-fade-in`. Only `@keyframes` in the same
  sheet are found, and nothing is animated.
- **Measured.** Per-run interpreter cost for a dumped three.js `MeshBasicMaterial`
  shader: about 0.7 us per vertex, about 6 us per fragment (`bench_vertex`). The
  globe's instanced dot draw (2.4 M vertex runs) takes about 0.4 s of vertex shading and
  0.2 s of assembly, with the fragment stage in the tens of milliseconds. The page
  itself is the slow part: reaching the first render takes minutes in the developer tool
  (every GSAP-driven DOM change repays a ~1 s restyle and layout, which also starves
  timers), so an interactive session shows the globe much later than a browser would.
- **Developer tools.** `live_page_to_png` takes `BI_SCROLL_TO` (scroll there and keep
  ticking for `BI_SCROLL_SECS`), `BI_DUMP_GLSL=<dir>` (keeps every shader the page
  compiles), `BI_GL_PROFILE` (per-draw stage timings) and `BI_DIAG` (draw durations).
  `blueice-gl`'s `bench_vertex` measures a dumped shader's cost
  (`BI_VERT`/`BI_FRAG`).
- **Next.** Derivatives and mip LOD for
  shaders that use them, `<video>` poster painting and AVIF, a persistent cookie jar,
  unhandled-rejection reporting in the page realm (calling
  `install_host_rejection_tracking` makes the realm's own bootstrap fail with "page
  scheduling budget exhausted", so it needs a budget decision first), and stack traces on BlueJS errors (a thrown error names no function today,
  which made the diagnosis above much slower).

## Making script-driven pages fast enough (2026-10-29)

The Cloudflare globe needed minutes of wall time to appear in the developer tool, so the
branch measured where that time went (a sampling profiler on the live run, not guesses)
and fixed the three things that dominated, in the order they did:

- **Restyle was a full cascade per script change (~730 ms of ~800 ms).** Layout was only
  ~55 ms. Three changes: an element with no custom properties of its own now shares its
  parent's resolved table (`Arc`) instead of deep-copying hundreds of token vectors; the
  structural pseudo-classes (`:first-child`, `:last-child`, `:nth-*`, `:*-of-type`) no
  longer collect every sibling into a `Vec` per check (a full cascade 730 -> 307 ms);
  and the cascade is now **incremental**.
- **Incremental restyle.** `blueice_dom::Document` journals what changes (attribute and
  text touches with the data from before the first touch, attachments, and detachments
  with the neighbours each had). `blueice_css::cascade_incremental` restyles only what a
  change can reach: a `style`-attribute-only change stops at the first element whose
  computed style does not change; a `class`/`id`/other attribute change or an insertion
  redoes its subtree; the siblings that `:first-child`, `:nth-child`, `+`, `~` or
  `:empty` can see are redone only if the rules use such selectors (adjacent ones when
  that is enough). It declines (the caller does a full cascade, and the styles are left
  untouched) when it cannot be sure: a journal overflow, or a change to the root
  element's font size. `Page` also does a full cascade when the media environment
  changed or a stylesheet source (`<style>`, `<base>`, `<link rel=stylesheet>`) did. A
  **differential test** builds random trees, applies random edits (class, id, style,
  insert, remove, move) and requires the incremental result to equal a full cascade
  after every step; two deliberate mutants of the implementation fail it. An incremental
  restyle + relayout of Cloudflare is ~80 ms.
- **The JavaScript engine gathered every GC root for every heap operation.** After the
  restyle work, ~89% of busy time was `Vm::prepare_gc_trace` walking the operand stack
  (3,600 values on average, 2 million calls in a minute) before each allocation or
  property write, though almost none of them collect. The heap can now be told not to
  collect (`Heap::set_collection_deferred`): an operation that would then have to
  collect fails with `HeapError::CollectionDeferred` *before it changes anything*, and
  `Vm::with_roots_lazy` runs the operation without roots and repeats it with them only
  then. It is opt-in per call site, for single-primitive operations that are safe to
  repeat: iterator results, `Array.prototype.push` and array construction, typed-array
  element writes, loop-scope cells, global stores and `[[DefineOwnProperty]]`.
  `backend/bluejs/tests/lazy_roots.rs` runs those paths with a one-object nursery, so
  the retry happens on nearly every allocation, and a mutant that retries without roots
  fails it. The full Test262 inventory is unchanged (102,921 pass, 0 fail,
  1 `stale_corpus`, 4 `excluded`). An earlier attempt, caching the operand stack's object
  ids, did nothing (replaying 3,600 ids is itself the cost) and was removed.
- **GC sizing.** A realm for page scripts gets a 16,384-object nursery and a 16 MiB first
  major threshold, not the test-sized defaults (256 objects, 256 KiB): script start-up on
  Cloudflare 9.6 s -> 7.7 s.
- **JavaScript strings share their buffer.** With the root gathering fixed, sampling showed
  ~99% of busy time in `memcpy` of strings: `JsString` was a `Vec<u16>`, so reading a
  variable, a property, or a string method's receiver deep-copied the whole string (a
  script holding a megabyte of text and calling `charCodeAt` was quadratic). It is now a
  handle on an `Arc<Vec<u16>>`: a clone bumps a count, and a change copies only if another
  handle still holds the buffer. A handle carries the capacity it is *charged* for in the
  memory accounting (an original: the allocation it was built with; a clone: its length, as
  a `Vec` clone was), so no accounting assertion changed (4,145 library tests, 7,603
  integration tests across 368 binaries, and the Test262 inventory are all as before).
  Microbenchmark, `charCodeAt` over a 200 KB string 20,000 times: 814 ms -> 45 ms. The
  type is smaller by 8 bytes, which moves every size-derived byte budget in the heap and so
  the moment a collection is triggered; `arguments_object_elision` counted collections
  under the default byte threshold as a proxy for allocation and was rewritten to count
  them with a threshold that is never reached (a build with the optimization disabled still
  fails it). The script shim had its own quadratic path: base64 one character at a time,
  which took over ten minutes for 1 MiB of buffer data; it uses the native
  `Uint8Array#toBase64`/`fromBase64`.
- **Measured, whole page.** Script start-up 11.5-12 s -> 7.7 s; per-change restyle +
  relayout ~800 ms -> ~80 ms; WebGL draws in the same window 60 -> 3,021; the globe's land
  dots, graticule, connection arc and caption card are all drawn within a 220 s window. After
  these fixes a sample of the live run is dominated by the software GL itself
  (`glsl::interp::Machine::eval` ~17%, `draw::fetch_attrib` ~11%, value coercion,
  allocation), not by the JavaScript engine.
- **Software GL speed.** After the JavaScript fixes, a sample of the live run was dominated by
  the GL itself, and the 96 draws of the globe's instanced dots (2.4 M vertex-shader runs,
  ~400 k triangles each) were 95% of GL time: ~547 ms per draw, of which vertex 332,
  assembly 202 (single-threaded), fragment 13. Changes, each checked against the previous
  implementation by a pixel fingerprint of a synthetic reproduction
  (`blueice-gl/tests/bench_draw.rs`): vertex output in flat arrays instead of a `Vec<u32>` per
  vertex; each attribute's bytes, stride and type resolved once per draw instead of a hash
  lookup and an allocation per attribute per vertex; a per-thread memory of recent
  `(index, instance)` vertices, because an indexed mesh names most vertices several times
  (about 3x fewer shader runs on a disc); triangles keep vertex ids into the store instead of
  cloning their varyings (only clipped polygons own copies), triangles wholly inside the
  clip volume skip clipping and ones wholly beyond the near or far plane are dropped, and
  assembly runs on all cores (215 ms -> 13 ms); the time budget reads the clock every 16
  fragments rather than every fragment. On the reproduction a draw is ~680 ms -> ~120 ms with
  an identical picture; on the live page ~547 ms -> ~103 ms (vertex 84, assembly 8, fragment
  11), and the whole run's GL time fell from 55 s for 3,021 draws to 19 s for 4,596, about
  8% of the run. What is left of the vertex stage is the GLSL interpreter itself (`eval`,
  `binary`, `coerce`, a few allocations): a tree walk that builds a ~70-byte value per node.
  Tests written against the old code first cover attribute decoding (normalized and
  unnormalized bytes and shorts, stride and offset, integer attributes, constants, divisors,
  half floats, matrix columns, repeated indices) and near-plane clipping.
- **GLSL interpreter, and its own crate.** The front end and interpreter moved to
  `backend/core/glsl` (`blueice-glsl`, no dependencies, meant to become its own repository;
  `blueice-gl` re-exports it as `blueice_gl::glsl`). `tests/corpus.rs` holds eleven shaders
  run on 64 pseudo-random input sets each, their outputs fixed as 64-bit fingerprints taken
  before any optimization, so an optimization may not change a result by a bit;
  `tests/bench.rs` prints nanoseconds per run. Swizzles without a search or an allocation,
  pooled argument lists for built-in calls and constructors, a fixed lane buffer in
  constructors, a float fast path in `binary()` and built-ins resolved to an enum when a
  shader is lowered took the corpus from 31,010 to 22,122 ns (-29%), but the heavy draw of
  the live page only from ~103 to ~95 ms: it is dominated by real matrix work, so GL is
  about 6% of a run and no longer worth more effort until a page draws far more.
- **BlueJS calls were the next thing in the profile** (a sample of the live run, GL aside):
  each call of a closure was O(everything visible where the closure was made).
  `tests/closure_call_bench.rs` reproduces it (a small function called 20,000 times by a
  caller that holds N objects), ns per call, before -> after the three changes below:
  10 held 2,777 -> 2,194; 400 held 6,418 -> 3,736; 4,000 held 47,085 -> 20,991.
  (1) The slot-to-cell tables use a multiplicative hash instead of SipHash
  (`slot_map::CellMap`; keys are compiler-assigned integers). (2) A frame shares its
  function's binding table (`Bytecode.bindings` is an `Arc<Vec<Binding>>`) instead of
  cloning every binding and its name per call: 200 captures 8,262 -> 4,557 ns; a frame is
  still charged what a copy of exact length would cost, so the budget does not depend on
  spare capacity. (3) A closure captures the names its source mentions
  (`ast::function_mentions`: every identifier in any position, nested functions and classes
  entered, `eval` anywhere meaning everything), not every name in scope. The analysis is a
  hint: `Compiler::resolve` notices a left-out name being looked up and the function is
  compiled again with all of them, and a function with `with_depth > 0` (inside a `with`, or
  in a sloppy function containing a direct eval) is never pruned, because Test262's
  `staging/sm/Function/function-name-assignment.js` showed that such code looks names up at
  run time through the captures. Test262 stayed at 102,921 pass / 0 fail.
  Still per call, regardless of what the callee uses: the caller's bindings and cells are
  pushed on the VM stack as GC roots (`call_closure_inner`), and `prepare_gc_trace` walks
  the whole stack on every eager `with_roots`; saving the caller frame in a structure the
  collector walks only when it collects would make a call O(1) in the caller's size, but it
  touches the execution-control accounting and every restore path.
- **CSS `transform: translate`.** `transform` and `translate` are kept as tokens by the
  cascade; layout records the pending translation on the fragment when it marks the box and
  applies every one in a single pass after the whole tree is placed
  (`layout/src/transform.rs`; a parent assigns its children's positions after they are laid
  out, so it cannot be applied earlier). Fragment `x`/`y` are relative to the parent, so
  adding to them moves the subtree and paint, hit-testing and the AI bounds follow with no
  further change. Percentages are of the box's own border box (resolved against bases 0 and
  100, which also covers `calc(100% - 4px)`); `translate()`, `translate3d()`,
  `translateX/Y()` and the individual `translate` property work for in-flow, absolutely
  positioned and flex boxes. `rotate`, `scale`, `skew`, `matrix` and `perspective` are the
  identity, and a list that mixes them still applies its translations. On the Cloudflare
  page the two globe caption cards now sit at their pins instead of the page's top-left.
- **A BlueJS bug found along the way, not fixed:** a named function expression written in
  code that a *sloppy function's direct eval* runs cannot read its own name
  (`function t(){ return eval("(function f(){ return f.name; })()"); } t()` throws
  `ReferenceError: f`), and assigning to it stores instead of being ignored. It behaves
  the same before and after the capture pruning and Test262 does not cover it; the cause is
  that code's run-time by-name lookup (`with_get`/`active_binding_slot`), which does not
  find the frame's self binding.
- **Incremental layout.** A block-level layout is a pure function of its node's subtree and the
  constraint it is laid out against, and a pass already memoised on that. `LayoutCache`
  (`layout/src/cache.rs`) keeps the memo between passes (`layout_with_cache`) and
  `LayoutCache::invalidate` drops what a change can have affected: every node whose style
  came to differ or whose data changed, all of their ancestors, and every child of a parent
  anything changed under (a list item numbers itself from its siblings and its parent's
  `start`/`reversed`, the only two reads layout makes outside a node's own subtree besides
  generated `::before`/`::after` boxes). The viewport and root font size are the cache's
  epoch. `cascade_incremental_report` is `cascade_incremental` that also reports the nodes
  whose style actually came to differ; a randomized test checks that report equals the real
  difference. In `Page`, only an incremental restyle keeps the cache (and invalidates exactly
  its changes); a full cascade, and every `relayout()` that did not come through one, clear
  it, because native editing, selection, images and dragging change the document and styles
  in ways the cache is not told about. Verification: a randomized differential test in the
  layout crate (60 seeds x 40 steps of attribute, text, insert, remove and move edits over
  floats, flex, inline-block, tables, positioning, transforms and lists) and a list-numbering
  one require the cached layout to equal a fresh one after every edit; an engine test does
  the same through scripts and `restyle_and_relayout`. Dropping the ancestors, the children
  of a touched node, or a detached parent's children each fail them, and dropping the
  engine's `invalidate` call fails the engine test on its second step. On the live page:
  223 relayouts at a mean 59.7 ms (13.3 s in a 120 s window) became 248 at a mean 3.4 ms
  (median 2.4 ms; 0.85 s), the one expensive pass being the first, cold one. A cache hit
  still clones the cached fragment subtree (`Placed` owns its `Fragment`), so a pass is
  O(page) in copying but not in layout; sharing subtrees (`Arc`) is the next step if that
  shows up.
- **Where the time is now** (a run of the live page, 138 s): start-up ("styled, scripted
  and laid out") went from 7.4-7.9 s to 3.4 s with the BlueJS call-path work; relayout is
  0.85 s, find/selection 0.9 s, accessibility 0.4 s; the rest is the page's own script and
  the software GL.
- **Hashing.** A second profile showed SipHash (the keyed default) as the largest single cost:
  every property read looks the object up in the heap's `ObjectId` table and then hashes the
  key in the object's property table. Both now use a non-keyed multiply-rotate hash
  (`prop_map.rs`: `PropMap`, `ObjectMap`). Changing only the property tables moved nothing
  (3.67 s vs 3.72 s); the object table is what paid: start-up 3.70 s -> 3.18 s on the live
  page, Test262 unchanged at 102,921 pass / 0 fail, BlueJS suite 7,622 pass. The keys are
  per-object and bounded by the heap budget, so flood resistance buys nothing here.
  The collector's mark/remembered sets and the global-name tables (`NameMap`) use the same
  hash: start-up 3.18 s -> 3.03 s more. Total this round 3.70 s -> 3.03 s (-18%); the
  largest remaining entry in a profile is `prepare_gc_trace` gathering roots.
- **Caller roots parked, not copied.** A closure call used to copy the caller's object ids
  onto the operand stack so the collector could see them (O(caller) per call). The caller's
  bindings and cells now move into `Vm::saved_caller_frames` (restored on return, one
  restore point) and `visit_execution_roots` reads them in place. Micro-benchmark: a caller
  holding 4,000 objects 21.3 -> 11.1 us per call, 400 held 4.6 -> 3.5 us; the live Cloudflare
  page is unchanged (3.03 s) because its callers are small. Covered by
  `tests/caller_frames_survive_collection.rs` (fails if the collector stops reading them).
  What remains O(caller) per call is the payload *accounting*
  (`caller_execution_payload_bytes`), not rooting.
- **Allocation and accounting, measured.** A sampled page showed ~30% of the allocation samples
  were `JsString::from(&str)` re-encoding the same short names (`"value"` on every captured-variable
  read, property names): short names (<= 32 bytes, 2,048 entries, per thread) now share one buffer,
  with the same units and the same charged size as a fresh conversion. Allocation samples -40%,
  active samples -11%, a closure call 2.76 -> 1.91 us. The summed name length of a large binding
  table (charged on every call) is remembered by table identity: a caller holding 4,000 objects
  10.5 -> 8.3 us per call. What is left there is `caller_execution_payload_bytes` scanning every
  binding (4,000 x 32 bytes per call, memory-bandwidth bound); removing it needs per-frame payload
  tracking through every binding write, which touches the exact-accounting tests and buys nothing
  on the live page (small callers), so it is left.
- **Real-site correctness and start-up, measured (tw.yahoo.com, Google, Cloudflare).** Tools:
  `BI_NET_LOG=1` (every script request and how it ended, each script's result and console
  output, text an `eval`/`Function` could not parse), `BI_SAVE=<dir>` (the page, its sheets and
  scripts, parse failures), `BI_PRELUDE=<js>` (run before the page's scripts, e.g. to wrap timers
  and print what they throw), `BI_EVAL`, and `RUST_MIN_STACK` (a `cargo test` thread has 2 MiB,
  so a deep React render reports `maximum call depth exceeded` there; `blueice-core` runs on an
  8 MiB main thread and does not). What a profile of Yahoo found, in order:
  - *Correctness first.* `Date.parse` read only ISO and RFC 1123, so dayjs's time-zone code
    (`new Date("11/2/2026, 5:06:22 PM")`) gave NaN and the horoscope module threw; `innerHTML` on a
    `<script>`/`<style>`/`<textarea>`/`<title>` parsed its text as markup and ate the middle of
    a bundle; `removeAttributeNode` and a live `attributes` list were missing (React clears an
    element with `for (l = el.attributes; l.length;) el.removeAttributeNode(l[0])`); `NodeList`
    had no `item()`; more than 64 requests in a turn threw (now a queue, 16 at a time); a `Blob`
    request body was sent as the text `[object Blob]`; `fetch(..., {mode: 'no-cors'})` and
    `sendBeacon` were CORS-checked; a `position: fixed` box inside an absolutely positioned
    box was dropped by layout. With these the app hydrates and the feed and right rail load.
  - *Legacy HTML.* Google's basic page is HTML 3.2: presentational attributes
    (`blueice-css`'s `presentational.rs`: `align`, `valign`, `bgcolor`, `cellpadding`,
    `cellspacing`, `nowrap`, a cell's `width`, `<font>`), `<center>` (`-webkit-center` centers
    tables too), a button input as wide as its caption, percentage columns widening an automatic
    table, and a preferred width no longer counting the room a centered line was pushed across.
  - *Start-up.* Yahoo 15.3 s -> 8.0 s, Cloudflare 3.7 s -> 2.1 s, from: the compiler walking every
    visible name per function (now only the names a function mentions; the "pruned name was
    looked up" guard asks the creator after the build), and `with_roots` gathering ~1,100
    roots on 500,000 calls (now `with_roots_lazy` at the hottest sites: captured-variable cells,
    closures, object/array literals, `for-in`, legacy function properties; 100,000 calls left,
    spread flat over many sites). SipHash on the heap's object and property tables, the
    collector's sets and the global-name tables was replaced by a non-keyed hash, short
    `&str` -> `JsString` conversions share a buffer, and a caller's frame is parked in
    `saved_caller_frames` instead of being copied onto the stack per call.
  - *Not a defect.* The blank band under Yahoo's header is the billboard ad slot (`#fullbleed`,
    258 px reserved by the page's own CSS); with no ad filled in it is empty in any browser.
  - *Still open for these sites.* `NodeFilter`/`TreeWalker` are missing (an ad script); one ad script (305 KB,
    obfuscated) reaches the 384 MB managed-heap limit; `window` `error` events are not delivered
    to listeners added with `addEventListener` for errors thrown from timers; the uncaught-error
    report for a `DOMException` prints `[Object]` without its message.
- **Still slow, in the order it bites.** The caller-payload scan above; most of the ~536
  `with_roots` call sites are not converted (convert one only after checking it fails before
  changing anything and is safe to repeat); a `class`/`id` change on a high element still
  restyles its whole subtree; the GLSL interpreter (a bytecode or register form with unboxed
  vectors would be the next step); `find`/selection rebuild after every relayout.
- **Developer tools added.** `BI_PROFILE_LAYOUT` (stage timings of a restyle), `BI_CASCADE_LOOP=N`
  (only the cascade, repeated, for a sampling profiler), `BI_SCROLL_TO`/`BI_SCROLL_SECS`.
  Profile with macOS `sample <pid> 10 -file out.txt` on the test binary (not `cargo`'s pid).

### A changed stylesheet restyles only what its changed rules match (2026-11-08)

A script that inserts, removes or edits a `<style>` used to cost a full cascade (about 0.35 s on
Cloudflare's page; its customer-logo carousel did it eleven times). `Page::restyle_for_appended_rules`
now compares the old and new author rule lists, drops what they share at the start and the end, and
restyles only the elements the remaining rules match (with their subtrees, against the complete new list
in its proper order, so the result is exact wherever the rules sit); more than 64 changed rules, or
matches over half the page's elements, still take a full cascade. A `<style>` inside an inline `<svg>`
feeds the page's cascade no rules (the SVG renderer reads it), so it changes nothing there. Measured on
that page after scrolling to the logos: restyle passes 104 -> 59, their total 11.4 s -> 4.5 s, passes over
200 ms 13 -> 4. `BI_PROFILE_LAYOUT` says which style element forced a full cascade.

### Page scripts can scroll the window (2026-11-08)

`window.scrollTo`/`scroll`/`scrollBy` (numbers or `{ top, behavior }`; smooth is instant, the final position
is what matters), `element.scrollIntoView` (`true`/`false`, `block: start|center|end|nearest`, computed from
the element's box and the window) and `documentElement`/`body`/`scrollingElement` `scrollTop` now ask the
page to scroll. `scrollY` reads the new position at once; the request travels back as `scroll_to` in the
turn's result (`Returned` for a thread, `ChildDone` over the wire for the process), and `Page` clamps it to
the scrollable range, publishes a frame (`script_change_unpublished`) and, as for a wheel scroll, tells the
scripts with a scroll event and a fresh look at what is visible on the next tick. Inner scrolling
containers keep a plain `scrollTop` that moves nothing (not built). Tested through a thread page, through the
real script process, and through a session where the representation and the frame it follows share the
`generation` and the `scroll_y`.

### Web fonts (2026-11-08)

`@font-face` rules (`blueice_css::font_faces`, read from the text because `unicode-range: U+0131` is hex;
rules inside `@layer` and a supported `@supports` are found, `@media` ones are not) in a page's `<style>`
elements and fetched sheets name the fonts to load. `engine/src/fonts.rs` fetches them with the page's other
subresources (same per-hop review, 24 faces and 8 MiB each at most, `data:` URLs decoded locally), tries
each face's sources in order (skipping `embedded-opentype`/`svg` hints), and registers what decodes in
`blueice_font::web` -- WOFF2 and WOFF through `wuff` (MIT, pure Rust), plain TrueType and CFF/OpenType
directly -- as a small `WebFontId`, deduplicated by content, bounded to 256 faces / 96 MiB for the life of
the process (past that a file is simply not registered). A `font-family` list is kept whole in the computed
style; after each cascade `fonts::assign_faces` gives every element the first family the page has a face for
(unless a system or generic family such as `sans-serif`/`system-ui` comes first), matched on style, then
nearest weight (the heavier for a bold request), then, among faces split by `unicode-range`, the one that
covers Latin, and records it as `-blueice-web-font` on the style. Layout measures and aligns lines with that
face (`measure_text_width_in`, `ascent_descent_in`), `PaintCommand::Text` carries it as `face`, and the
rasterizer draws with it, taking the baseline from the face's own ascent and a character the face lacks
from the bundled fonts. Fonts load before the first paint (`font-display: swap` has nothing to swap to).
`unicode-range` is honoured per character: what an element's `-blueice-web-font` names is a *set* of
faces (`blueice_font::web::register_set`; one face is the set of itself), each with the ranges it covers --
for each family in the list that the page has faces for, the faces of its best style and weight, the one
declared last first, then the next web family, up to a system or generic family -- and every character is
measured and drawn by the first member whose range covers it and that has a glyph for it, else the bundled
fonts; line metrics come from the member covering Latin. Not built: variable-font axes (a variable
face draws at its default instance), synthetic bold/italic for a missing weight, `local()` sources,
`@font-face` inside `@media`, canvas `ctx.font` (since built, below). Native form controls' text (input value, select label and options) is drawn in the element's page face (`face_of(style)`).

`FontFace` and `document.fonts` really load (2026-11-09): `new FontFace(family, "url(..) [format(..)], url(..)" |
ArrayBuffer | view, descriptors)` has `status`/`loaded`/`load()`; a URL is fetched as a CORS font request
(`netSend` kind `font`), the bytes are checked by the page's decoder (`fontCheck`; failure rejects with
`SyntaxError` or `NetworkError`), and a loaded face that is in `document.fonts` is handed to the page
(`fontAdd` -> `State.loaded_fonts` -> `ChildDone.loaded_fonts` over the script-process wire -> `Page`). The page
decodes it into its own registry (a `WebFontId` is per process, so bytes cross, not ids), adds it to
`linked_fonts` (at most 64 per page) and takes a full recascade, so every element chooses its face again.
`document.fonts.add` of an unloaded face loads it; `load(font)`/`check(font)` work on the faces added to the
set (family taken from the `font` shorthand). Faces the stylesheets declare are listed in `document.fonts` (status `loaded`, family as the page keeps it, lowercased; read through the `fontList` bridge op the first time the set is touched), `status` is `loading` while a face of the set loads, `ready` is a new pending promise per batch, and `loading`/`loadingdone`/`loadingerror` fire (with `fontfaces`) on the set and its `on*` handlers. Not built: `local()` sources.

Variable fonts (2026-11-09): a face with an `fvar` `wght` axis is drawn at the element's weight (clamped to the `@font-face` weight range, then to the axis): `blueice_font::web::register_set_weighted` gives each set member a weight, `Glyphs` is a static font or a `VarInstance` (outlines moved by `ttf-parser`, drawn by `ab_glyph_rasterizer`; one instance per face and weight, made on first use). Test font: Inter (OFL) in `backend/core/font/tests/data`.

Other variation axes (2026-11-10): `register_set_varied` takes a `blueice_font::Variation` per member (`wght`, `wdth`, `slnt`, `opsz`, `ital`; widths/angles/sizes in tenths), resolved against the file's `fvar` (an axis the face lacks is dropped, values clamp to the axis, an `ital` axis replaces a slant) and drawn by one `VarInstance` per (face, resolved variation), at most 512. The engine's `FaceRequest` is built from the computed style: `font-weight`, `font-stretch` (keywords or %; clamped to the `@font-face` `font-stretch` range, which defaults to 100% so an undeclared face never condenses), `font-style: oblique <angle>` (new in the cascade: kept as `-blueice-oblique-angle`, `oblique` now counts as italic) → `slnt`, `italic` → `ital` and a 14° slant, `font-size` → `opsz` unless `font-optical-sizing: none`, and `font-variation-settings` (not held to the declared ranges, up to 16 entries). A face that slants by an axis is not also sheared by the rasterizer. Canvas text follows the optical size of its font size; a canvas's `font` shorthand has no stretch, and script-made `FontFace`s have no stretch descriptor. Script-made `FontFace`s take a `stretch` descriptor (carried to the scripts' process with the rest of `ScriptFont`), a canvas `font` shorthand takes a stretch keyword (held to the declared range), and `document.fonts` lists a stylesheet face with its `stretch`. `font-synthesis` (`none`, `weight`, `style`, `font-synthesis-weight`, `font-synthesis-style`, inherited) masks the synthetic bold and italic. The `@font-face` `font-style: oblique <angle> <angle>` range is read (`FontFaceRule.slant`, tenths of a degree; a bare `oblique` is 14deg; none for `normal`/`italic`), travels with script faces (wire format has a flag and two angles) and shows in `document.fonts` as the style text; a text's oblique angle (and the italic's 14deg) is held to it before the axis clamp. No slant-axis fixture exists, so the clamp is checked on the request-to-axes step, not on drawn glyphs. Not built: `font-variation-settings` inside `@font-face`, `font-synthesis: small-caps` as anything but off. Custom axes (`GRAD`, `XTRA`, ...): `Variation.custom` holds up to four other tags in thousandths (sorted, a later setting of a tag replaces the earlier), `font-variation-settings` fills it for any four-letter tag, `resolve` drops axes the face lacks and clamps the rest, and `axes()` lists every axis of the file, not only the five registered ones. Test fonts: Inter with `opsz`, Open Sans with `wdth` and Roboto Flex with `wght`+`GRAD` (all OFL; the licence texts sit beside them), same directory.

`local()` sources (2026-11-10): `@font-face { src: local(name) }` is parsed (`FontSource.local`) and tried in order with the `url()`s; the only installed fonts BlueIce has are the ones it carries, so `blueice_font::bundled_font_bytes` answers DejaVu Sans (regular, bold, oblique, bold oblique by full or PostScript name) and Noto Sans TC, and any other name falls through to the next source. A script-made `new FontFace(family, "local(..), url(..)")` takes the same sources in order (bridge op `fontLocal`); a source string with neither is a `SyntaxError`, one whose sources all fail is a `NetworkError`.

Synthetic bold/italic (2026-11-09): when the primary face of a list has no weight >= 600 for a bold request, or no italic for an italic one, `assign_faces` records `-blueice-web-synth` (1 bold, 2 italic); a page-face run is then emitted `bold`/`italic` only for what the face lacks, and the rasterizer smears the glyph sideways (size/20 px, at least 1) and shears it 0.2 about the baseline. Advances are not widened. Characters that fall back to the bundled fonts inside such a run are not synthesized.

Canvas 2D text (2026-11-09): `fillText` (with `textAlign`, `textBaseline`, `maxWidth`, the transform and `globalAlpha`) and `measureText` use the real glyphs and advances. `font` is parsed (`style weight size[/lh] family-list`); its families are looked up in the faces scripts added through `document.fonts` (a script-side catalog in the script process, since face ids are per process), then the bundled fonts. The page's own faces (stylesheet `@font-face` too) reach the scripts: a `Handoff.fonts` (thread) or a `ToChild::Fonts` frame (process) carries every face's file and descriptors whenever the page's catalog changed, and the script side decodes its own copy (face ids are per process), so a canvas draws in the page's fonts. `strokeText` draws the outline `lineWidth` wide (the glyph coverage grown and shrunk by half a line, at most 16 px, joins and caps are those of the grown shape), and `createLinearGradient`/`createRadialGradient` are real (`CanvasGradient`, two-circle radial, stops interpolated in straight RGBA, clamped at the ends) for `fillStyle`/`strokeStyle` of `fillRect`, `fillText` and `strokeText`; the canvas takes a gradient as `grad:` + JSON in the same string slot as a colour. `strokeRect` is four bars; `measureText` reports the real ink extents (`actualBoundingBox*` from the placed glyphs for the current `textAlign`/`textBaseline`, font ascent/descent from the face). Paths are real (2026-11-10, `web/canvas_path.rs`): `beginPath`/`moveTo`/`lineTo`/`closePath`/`rect`/`roundRect`/`arc`/`ellipse`/`arcTo`/`quadraticCurveTo`/`bezierCurveTo` keep user-space points (curves flattened as added), `fill(nonzero|evenodd)` and `stroke()` draw them with the transform in force then (4 samples per pixel row, exact horizontal overlap) in colours or gradients; `stroke` honours `lineWidth`, `lineCap`, `lineJoin`, `miterLimit` (strokes are polygons per segment plus join and cap shapes, wound alike so their union is non-zero); `isPointInPath` tests the user-space path. Dashes, clips and `Path2D` (2026-11-10): `setLineDash`/`getLineDash`/`lineDashOffset` cut each stroked path into dashes before it is stroked (`canvas_path::dash_paths`: odd lists repeat, a bad or zero-length list leaves the line solid, at most 100,000 dashes a stroke; `strokeRect` and `strokeText` are not dashed). `clip()` keeps a full-bitmap coverage mask in the canvas (anti-aliased, intersected with the earlier clip, an `Arc` so `save` is cheap) that `fillRect`, `clearRect`, paths, text and `drawImage` multiply into their coverage; `save`/`restore` are forwarded to it, a resize drops it, `putImageData` ignores it as the spec says. `Path2D` shares the context's path methods (`PathBuilder`), copies another `Path2D`, takes SVG path data (M L H V C S Q T A Z, relative forms, arc flags run together) and `addPath(path, matrix)`; `fill`, `stroke`, `clip` and `isPointInPath` accept one. `roundRect` takes one to four radii, each a number or `{x, y}`, shrunk together when two on a side overlap (a negative radius is a `RangeError`). `createConicGradient` (colours swept clockwise from the start angle) and `createPattern` (an image, canvas or ImageData tiled in user space by `repeat`/`repeat-x`/`repeat-y`/`no-repeat`, nearest-pixel, `setTransform` accepted and ignored) are real, as `fillStyle`/`strokeStyle` for every draw; `direction` (`ltr`/`rtl`/`inherit`, saved with the state) turns `textAlign` `start`/`end` into the left or right side (`inherit` counts as `ltr`), and a line of text is shaped and put in visual order like page text (below).

Right-to-left text and bidirectional layout (2026-11-10): page text follows the Unicode Bidirectional Algorithm (UAX #9, the `unicode-bidi` crate for levels) and right-to-left layout.
- **Inline layout** (`layout/src/bidi.rs`, `inline.rs`): the pieces of an inline run become one paragraph string (a word its text, a space ` `, a box edge or atomic box U+FFFC, a `<br>` U+2028, an inline box with `unicode-bidi: embed/isolate/bidi-override/isolate-override/plaintext` its formatting characters). The paragraph's base direction is the container's `direction` (or the first strong letter for `dir=auto`/`plaintext`); a word whose characters fall on different levels is cut into one word per level; each line's items are put in visual order (rule L2, trailing spaces at the paragraph level, L1) and each text fragment carries `rtl` for an odd level. `text-align` defaults to `start`, and `start`/`end` follow the paragraph's direction while `left`/`right` do not; `text-indent` is taken from the start side; the last line of a `justify` paragraph sits at the start side; an inline box's start edge (margin, border, padding) is on the side of its own direction and its painted edges swap sides in right-to-left order.
- **UA sheet**: `dir=ltr|rtl` set `direction` and isolate, `dir=auto`, `<bdi>` and `<output>` isolate, `<bdo>` overrides, `textarea`/`pre` `dir=auto` are plaintext.
- **Painting**: `PaintCommand::Text.rtl` marks a run whose text is in reading order; the rasterizer draws it from its last character to its first with brackets mirrored (`blueice_font::visual_order`, `mirrored`). A letter-spaced run is emitted a character at a time in drawn order.
- **Arabic joining** (`blueice-font/src/shape.rs`): letters take their isolated, final, initial or medial form from the Arabic Presentation Forms (Arabic, Persian and Urdu letters; lam-alef ligatures), a form the face lacks leaves the plain letter, and measuring and drawing both go through `shape_in` so widths and glyphs agree. There is no OpenType layout engine: other ligatures, mark positioning and contextual forms of other scripts are not formed.
- **Block level**: an over-constrained block (fixed width, no auto margins) in a right-to-left container sits at the right (its left margin grows); a flex row starts at the right of a right-to-left container (`row-reverse` reads from the left there); list markers hang off the right edge and `1.` reads `.1`; `margin-inline*`, `padding-inline*` and `inset-inline*` resolve against the element's own direction in the cascade (UA `ul`/`ol`/`dd` indents use them).
- **Canvas**: `fillText`/`strokeText`/`measureText` shape the string, then put it in visual order for the context's `direction` (`blueice_layout::visual_text`); the formatting characters draw nothing.
- **Tables and selection**: a right-to-left table lays its columns out from the right. A line's fragments are in screen order and carry `Fragment.logical` (their reading-order piece), so the document-selection index sorts a line by it: copied text reads in logical order, a right-to-left run's clusters run leftward, and caret, hit-testing and highlight rectangles use the right edge as the start of such a run. Checked by eye on a Hebrew/Arabic page rendered with `examples/render_html` (right-aligned lines, joined letters, mirrored brackets, bullets and flex row on the right, table columns reversed).
- **`float` and `text-align-last`**: `float` and `clear` take `inline-start`/`inline-end` relative to the containing block's direction; `text-align-last` (inherited) places the last line of a run and every line a `<br>` ends, with `justify` stretching them too.
- **Vertical writing** (`layout/src/vertical.rs`): `writing-mode: vertical-rl | vertical-lr` (and `sideways-rl|lr` as those) and `text-orientation: mixed | upright | sideways`, both inherited. A vertical box is laid out in a turned frame and rotated back: inside it `Ctx::writing` makes every style read name the sides the turned frame sees (its `width` is the page's `height`, its `left`/`right` the page's top/bottom, its `top` the page's right in `rl` and left in `lr`), so block, inline, line-breaking and list code run unchanged with an inline size equal to the box's height (its own, or the viewport's when `height` is auto, then shrunk to the longest line). The box's own margin, border, padding and background stay in the page's frame. Blocks stack from the right (`rl`) or the left (`lr`); inside a line the letters' tops face right in both. Text runs are cut where orientation changes: ideographs, kana, hangul, fullwidth forms and emoji are `Upright`, the rest `Sideways` (a quarter turn clockwise); `upright`/`sideways` force one. `Fragment.orient` and `PaintCommand::Text.{orient, breadth}` carry it; the rasterizer draws a sideways run as the horizontal text turned clockwise about a baseline `ascent` inside the box's right edge, and an upright run one character below the other, centred, in the vertical presentation form (`blueice_font::vertical_form`, checked against Unicode's compatibility decompositions) where the face has one. The document-selection index, caret, hit-testing and highlight rectangles follow a vertical run down its column. Checked by eye on a page with Chinese `vertical-rl`, Japanese `vertical-lr` with Latin and digits, and `text-orientation: upright`.
- **More vertical writing**: `sideways-lr` writes text turned counter-clockwise, running up the page with lines stacking from the left (`Writing::SidewaysLr`, `TextOrient::SidewaysCcw`; `sideways-rl` is `vertical-rl` with every run sideways); the turned frame's mapping is a `Turn { block_reverse, line_reverse, inline_reverse }` used by `rotate` and its inverse `unrotate`. A horizontal box inside a vertical one (a `writing-mode: horizontal-tb` block or inline-block, and always `input`/`button`/`select`/`textarea`, set so by the UA sheet) is an orthogonal flow: `layout_orthogonal` lays it out whole in the page's frame at its own width or the room across the page (`Ctx::orth_room`), then turns it into the vertical frame (its text carries `Fragment.turned_back`), so turning that frame back leaves it as written; on a vertical line it takes room along the line by its page height and across the column by its page width. Pictures and other replaced elements keep their page size (`replaced_size` swaps the sides the turned frame sees). Absolutely positioned boxes in a vertical box are placed afterwards in the page's frame (their static positions are turned), and floats (`left`/`right` are the line-left/right sides, which are the top/bottom in a vertical box), tables (columns run down the page), flex rows and grids already ran correctly in the turned frame. `text-combine-upright: all | digits 2..4` sets the chosen characters side by side in one em (`TextOrient::Combined`; layout measures such a run as one em, the rasterizer shrinks it to fit and centres it). Line breaking gained the kinsoku rule that an opening bracket or quote is never left at the end of a line, and `hanging-punctuation: allow-end | force-end` lets a stop or comma at the end of a chunk hang past the line's end (both horizontal and vertical text). The selection index follows text running up too. Checked by eye on a page with each of these.
- **Hanging punctuation, whole-element combining, mixed block directions, flex items** (2026-11-11): `hanging-punctuation` takes `first`, `last`, `allow-end` and `force-end`: a stop or comma at the end of a line hangs past it only when it does not fit (`allow-end`) or always (`force-end`, so a right-aligned or justified line is laid out without it); a closing bracket or quote at the end of the last line hangs when it does not fit (`last`); an opening bracket or quote at the start of the first line hangs before it (`first`, left-to-right paragraphs). `text-combine-upright: all` takes the whole text of the element, across words and inline boxes, as one cell (the layout makes one piece of it), and the rasterizer squeezes a combined run sideways only, its glyphs keeping their height. A vertical box inside a vertical box of another mode (`vertical-rl` in `vertical-lr`, or `sideways-lr`) is an orthogonal flow too: it is laid out in the page's frame, its own mode turning its content, and turned into the outer frame. A vertical `display: flex | grid | table` container now runs its own algorithm in the turned frame (`layout_vertical_flow` dispatches on the display type; before, only block flow did), and a horizontal flex item in a vertical container is sized by its page height (`intrinsic::preferred_widths` and `content_widths` measure an orthogonal box that way).
- **OpenType layout** (`blueice-font/src/opentype.rs`, 2026-11-11): Arabic and Hebrew text is shaped with the font's own `GSUB`/`GPOS` tables through `rustybuzz` (a port of HarfBuzz): joining forms from the font's `init`/`medi`/`fina` lookups, ligatures (lam-alef and the font's others), kerning, and marks placed on their letters (a mark has no advance and an offset). `shape_for_drawing` returns glyphs in screen order for the direction asked (a script is shaped in its own direction and turned around if the other is asked); measuring (`measure_text_width_in`), the rasterizer, canvas text (`visual_runs` cuts a line into bidirectional runs, each shaped) and the selection index (`char_advances`: a ligature's width is its first character's) all use it, so a run is as wide as it is drawn. Only text in those scripts is shaped this way, so Latin text keeps its per-character advances and does not change by a pixel; a face a page brings as a variable font keeps the joining-form table of `shape.rs`. Upright characters of a vertical line are drawn with the font's own `vert`/`vrt2` alternate (`vertical_glyph`) where it has one, before the presentation-form table. Registered faces keep their file for this (`web::bytes_of`).
- **Shaping for every script, features, vertical metrics** (2026-11-11): all text drawn with a static or variable face (a variable one at its axis values, `VarInstance::shaper`) is shaped with the font's own tables, not only Arabic and Hebrew: kerning and the common ligatures are on, as in any browser, and contextual forms of the Indic, Southeast Asian and other scripts come out as the font makes them (Latin text got narrower by the kerning, 0.2px on the demo page, so `demo.dat` changed). A box's `font-kerning`, `font-variant-ligatures | caps | numeric | east-asian | position` and `font-feature-settings` (the last wins) become an OpenType feature list (`blueice_css::font_features`), registered as a small number (`blueice_font::FeaturesId`) that `PaintCommand::Text.features` carries to the rasterizer and that layout's measuring and the selection index use, so what is measured is what is drawn; letters spaced out with `letter-spacing` are not ligated. Widths are cached (32,768 entries). In a vertical line an upright character is set from the font's own vertical layout (`blueice_font::vertical_cell`: `vmtx` advance, `VORG`/default origin, `vert`/`vrt2` alternates): layout measures it by that advance (`vertical::upright_length`) and the rasterizer hangs the glyph from the pen where the font says. `hanging-punctuation` now works in right-to-left paragraphs too: a hung character is outside the line edge it is at on screen (the logical end is the left edge there).
- **Feature properties, synthesized small caps, canvas text, bidi override on blocks** (2026-11-12): `text-rendering: optimizeSpeed` turns kerning and the ligatures off (an explicit `font-kerning: normal` or `font-feature-settings` still wins); the `font-variant` shorthand takes the keywords of all its longhands and `font-variant-alternates` takes `historical-forms` and the functions that pick a feature by number (`stylistic(2)`, `styleset(1, 3)`, `character-variant(5)`, `swash`, `ornaments`, `annotation`). Small capitals that the face has no `smcp` for are synthesized as capitals at 70% of the size on the same baseline (`all-small-caps` shrinks the capitals too; `font-synthesis: none`, a list without `small-caps`, or `font-synthesis-small-caps: none` refuses): layout cuts a word into runs with a size share, `Fragment.size_scale` carries it, paint draws it smaller and moves it by the difference of the two baselines (`blueice_font::baseline_offset`, the figure the rasterizer draws by), and selection measures at the drawn size. Canvas text takes `fontKerning`, `fontVariantCaps`, `textRendering`, `letterSpacing` and `wordSpacing` (saved and restored with the state, validated like the rest, sent as one array with each draw): features reach the shaper, letter spacing is added after every letter without ligatures and word spacing after each space. A face with no `arab` layout table is given the presentation-form joins before shaping (`shape_run`, with each glyph's source character kept so selection widths stay right). `unicode-bidi: bidi-override | isolate-override | embed` on a block reads all of its text in the block's direction.
- **Script faces, vertical proportional metrics, canvas small caps and stretch** (2026-11-12): the engine now carries Noto Sans (Regular, SIL Open Font License, `assets/NotoSansScripts-LICENSE.txt`, credited on `about:credits`) for Devanagari, Bengali, Gurmukhi, Gujarati, Tamil, Telugu, Kannada, Malayalam, Sinhala, Thai, Lao, Myanmar and Khmer, about 1.8 MB in all. `font_for_char` tries DejaVu, then the script's own face (found by code point range, parsed on first use, so Latin text still parses only DejaVu), then the CJK face; the faces answer to their names in `local()`. Their text is shaped by the font's tables like any other (conjuncts, vowel signs, Thai tone marks). Upright text in a vertical line is set as a whole run from the font's vertical layout (`blueice_font::vertical_glyphs`: `vert`/`vrt2`, `vkrn` pair kerning, `vmtx`/`VORG`, per-glyph offsets), layout measures it the same way, and `font-variant-east-asian: proportional-width` or `font-feature-settings: "vpal"` gives the proportional alternates that tuck brackets and stops in (`pwid` is `vpal` in a vertical line). Canvas text synthesizes small capitals where the face has no `smcp` (each piece carries its own size; `fontVariantCaps` set by a `font` shorthand naming `small-caps`, and reset by any other `font`), and takes `fontStretch` (reset by the `font` setter, winning over a stretch keyword in the shorthand it follows) for faces with a width axis.
- **Done in this batch**: `@font-feature-values` is parsed (merged per family, last definition wins, names case-sensitive) and `font-variant-alternates` names resolve per CSS Fonts 4 (`stylistic`→`salt`, `styleset`→`ssNN`, `character-variant`→`cvNN`, `swash`→`swsh`+`cswh`, `ornaments`→`ornm`, `annotation`→`nalt`, `historical-forms`→`hist`; the earlier numeric `stylistic(2)`→`ss02` form was non-standard and is gone). `petite-caps` falls back to `smcp` (or synthesized small caps), `all-petite-caps` adds `c2sc`, `unicase` synthesizes small capitals for the capitals when the face has no `unic`, `titling-caps` has no fallback, and `font-feature-settings: "smcp" 0` / `font-synthesis` refuse synthesis. `text-spacing-trim` (space-all, normal, space-first, trim-start, trim-both, trim-all) draws trimmed fullwidth marks with `halt` (`vhal` vertically), only where the face makes the mark narrower; trim-start/trim-both hang the blank half of an edge mark. Regular-only fallback faces get synthesized bold (smear) and italic (shear) in raster and canvas. Twelve more Noto faces are bundled (Ethiopic, Tibetan as Noto Serif, Cherokee, Mongolian, Thaana, Syriac, Javanese, Balinese, Yi, Runic, Coptic, Glagolitic); Georgian and Armenian were already covered by DejaVu.
- **Not done**: `text-spacing-trim`'s `space-first` behaves as `normal` (line-start opening marks are never trimmed by either; only `trim-start`/`trim-both` do), which is believed but not verified against the spec's exact wording; marks in adjacent inline elements are trimmed (covered by a test); Noto faces are Regular only (bold/italic are synthesized).

## Page scripts off the session thread (2026-11-03)

**Problem, measured.** In the macOS app on tw.yahoo.com, a wheel scroll's frame took a
median 221 ms (p95 831 ms, max 7.7 s) because the core ran page scripts on the thread that
answers input: one React render is a single timer callback of about 7 s here, and a steady
stream of 200–300 ms callbacks followed it.

**What was found on the way (all fixed, with tests).**
- A scroll rasterized the whole page and cropped one screenful (10–15 s per scroll on a
  1,500-item page in a debug build). Paint and raster now cover only the visible band; the
  render-pipeline crates are optimised in dev/test profiles.
- `TextEncoder`, `TextDecoder`, `btoa` and `atob` were per-character script loops (a 6 MB
  string exhausted the 384 MB heap; `btoa` cost about 7 µs a character). They are native
  bridge operations.
- `JSON.parse`, iterators, `for-in`, `RegExp` results and `String.split` gathered every GC
  root at each allocation, which a deep call stack makes expensive. They use
  `with_roots_lazy`. The longest Yahoo turn went from 35 s to about 7 s.
- A timer callback now has a wall-clock limit (20 s, a guard against a loop that never ends,
  not a responsiveness target; Yahoo's longest real callback is about 7 s), and a pass over
  the due timers stops starting callbacks after 30 ms. A stopped callback is reported with
  the functions it was in. `BI_DIAG` reports every callback over 80 ms; `BI_SCRIPT_LIMIT_MS`
  overrides the limit while measuring.

**The change.** With `blueice-core --web-scripts --async-scripts` a document's `WebHost` lives
on a thread of its own (`web/worker.rs`). A turn is a hand-off: the page lends a copy of its
document, styles and layout boxes, says what to run, and keeps painting and answering input
from what it has; the result (the document back, navigations, requests, canvases) is taken at
the next tick. One turn is in flight at a time. The launcher passes both flags to every core it
starts (`--no-page-scripts` turns them off). Page startup and the blocking callers still wait
for their turn.

**Result** (same app, same page, 95 wheel notches, frame latency):

| | scripts on the session thread | scripts in the background |
|---|---|---|
| median | 221 ms | 13.8 ms |
| p95 | 831 ms | 17.1 ms |
| max | 7.7 s | 146 ms |

Page position and the document position of every tracked node were identical in both runs.

**Limits of the thread version, stated plainly.**
- A thread is not a process: a script that crashes the realm or exhausts memory still takes the
  core with it (the process version below removes this).
- A change the core makes to the document while a turn runs (typing into a field, say) is
  overwritten when the turn's document comes back.
- A callback that runs past the wall-clock limit is stopped and cancelled, so a legitimately
  longer one would lose its work.

**Also found: the launcher drops a client that stops reading.** Its per-client writer gives up
after 5 s on a full socket and the client's end stays open and silent, so an MCP session idle
between tool calls on a page that keeps repainting never got another reply. The MCP connection
now reads on a thread of its own and keeps only the latest unsolicited frame notice per tab
(`backend/mcp-server/src/unix/drained.rs`). The launcher's behaviour is unchanged.

### A process of their own (2026-11-03)

`blueice-core --script-process` (with `--web-scripts --async-scripts`) runs each page's
scripts in a child process: the core starts itself as `blueice-core --page-scripts-child
<socket>` (no new binary to package). The launcher passes it by default (`--no-script-process`
keeps the thread). A script that crashes, hangs or eats memory costs the page its scripts, not
the core; tested with `kill -9`, after which the page keeps what it last had and a new document
gets scripts again.

*What crosses the boundary.* A turn is plain data (`web::turn::Turn`: startup, a loaded script,
after-layout, scroll, network calls, tick) in a hand-written length-prefixed binary format
(`web::wire`; every message is round-trip tested and refused when truncated or corrupt).
- **The document** lives in the child. The core sends it once (`Document::encode`/`decode`,
  which keep node IDs, tree links, gaps and the next ID) and fetches it back, with the DOM's
  change journal (`Changes::encode`/`decode`, `absorb_changes`), when a turn changed it, so
  incremental restyle and layout reuse still work across the swap. A change the core makes to
  its own copy is detected by a revision counter and sent before the next turn.
- **Layout boxes** go to the child once per layout, with the next turn, so
  `getBoundingClientRect` never leaves the process. An earlier version asked for each box over
  the socket; an `IntersectionObserver` pass asks for thousands, and the core was held for about
  110 ms per layout.
- **Computed styles and pictures** are asked of the core mid-turn, one question at a time, and
  cached in the child until the next layout.
- **Everything the scripts want to do outside** (network requests, navigations, canvas pictures,
  scripts they insert) comes back as data and is reviewed by the core exactly as before. The
  child has no network or file code path.
- Reports on layout (`AfterLayout`) run as background turns instead of blocking the session.

*Measured* (macOS app, tw.yahoo.com, 95 wheel notches, frame latency, same page and steps):

| | scripts on the session thread | background thread | own process |
|---|---|---|---|
| median | 221 ms | 15.3 ms | **13.4 ms** |
| p95 | 831 ms | 25.7 ms | **20.2 ms** |
| max | 7.7 s | 115 ms | **39.5 ms** |

The loaded page is the same: both modes reach 507/508 nodes with the same roles at the same
rate, and the page position and every tracked node's position were identical in all runs.

*Not done, stated plainly.*
- **No operating-system sandbox.** The child is a separate process, not a restricted one: no
  `sandbox_init`, no resource limits. It has no network or file code path of its own, but nothing
  stops a compromised realm from using the OS. Memory is bounded by the realm's own 384 MB heap
  limit, not by the OS.
- A turn that runs past 90 s is ended by killing the process (the realm's own 20 s per-callback
  limit comes first).
- Page startup and a script the page inserts later are fetched and run with the session waiting
  (the fetch is network I/O on the core's thread).
- A change the core makes to the document while a *background* turn is running is lost when that
  turn's document comes back.

## Non-goals

- Raw HTTP/DOM handles for AI clients; the MCP surface stays typed and
  untrusted-framed.
- A second JavaScript realm or DOM for automation; DevTools/automation stay
  Phase 17 clients of the same core.
- Declaring site compatibility. Passing M5 means one fixture works, not that
  arbitrary sites do.

## Render gaps found against Safari (2026-11-12)

Safari could not be driven in this session (screen recording, `safaridriver` remote automation and
"JavaScript from Apple Events" are all off), so these were found by reading each page's source and
comparing BlueIce's render with what the standards and Safari do, not by a side-by-side. Pages:
example.com, Hacker News, and small probe pages.

Fixed, each with a test:
- `insertBefore(node, node)` (what `prepend` does to a first child) moved the node to the end
  instead of leaving it; example.com's language picker came out reversed.
- The root (or, failing that, body) background painted only the content height; it now covers the
  whole canvas (`blueice_paint::canvas_background`, drawn first in `Page::render_band`).
- A block-level picture with `margin: auto` (`svg`/`img { display: block; margin: 0 auto }`) was not
  centred: the forced width skipped the auto margins.
- Underline/line-through stopped at every space; the gap between two words of one element is now
  covered (`Fragment.deco_after`).
- Quirks mode (no doctype) was parsed but never reached CSS. `Document::quirks_mode` now carries it
  and the cascade adds the "tables inherit" quirk (a table does not inherit `text-align`, font
  weight/style/variant, `line-height`, `white-space`), which is why every cell of Hacker News was
  centred under its `<center>`. Fixing it exposed that `initial` on a keyword property such as
  `text-align` fell back to the parent's value; `initial` now resets it. A snapshot decoded for the
  script process drops the flag (the script side does not style).

Open, not fixed:
- Hacker News's logo and vote arrows were not shown in the offline `render_html` check (it fetches no
  images); not yet checked in a live page, so it is unknown whether images are really missing.
- An inline link's AI-representation bounds cover only its first word.
- `system-ui`, Verdana and similar fall back to DejaVu Sans, so line breaks and widths differ from
  Safari's system font.
- `get_console_messages` answered "Page console is unavailable" on a local test page.
- Quirks mode is only "no doctype"; the legacy-doctype table, the quirks line-height and body-height
  rules, and image-baseline quirks are not implemented.
