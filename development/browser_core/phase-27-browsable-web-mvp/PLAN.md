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
- **`::before`/`::after`**: pseudo-element selectors and cascade, `content` strings,
  `attr()`, quotes and hex escapes; generated boxes lay out like children. Counters
  and `url()` content generate nothing. List markers (`disc`/`circle`/`square`,
  decimal, alpha, roman, `start`/`reversed`/`value`).
- **Images**: `<img>` (PNG, JPEG, GIF first frame, WebP, SVG) and CSS `url()` backgrounds
  (size, position, repeat, `object-fit`) fetched behind the same per-hop URL review as
  stylesheets, with byte and count limits; `data:` URLs decoded locally; inline `<svg>`
  rendered (the HTML parser now treats SVG as foreign content: self-closing children,
  camel-case names) with the page's colours reaching it. Decoding is the new
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
`text-overflow`, `@import` and `@container`, `:has()`, `@layer` precedence, `transform`,
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
- **Still slow, in the order it bites.** The software GL's per-fragment and per-vertex
  interpreter cost (`fetch_attrib` re-reads a buffer and converts per vertex per attribute,
  `Machine::eval` allocates values); layout is a full relayout (~55 ms); most of the ~536
  `with_roots` call sites are not converted (convert one only after checking it fails
  before changing anything and is safe to repeat); a `class`/`id` change on a high element
  still restyles its whole subtree; CSS `transform` is not applied, so the globe's pinned
  caption card is drawn in the page's top-left corner instead of beside its pin.
- **Developer tools added.** `BI_PROFILE_LAYOUT` (stage timings of a restyle), `BI_CASCADE_LOOP=N`
  (only the cascade, repeated, for a sampling profiler), `BI_SCROLL_TO`/`BI_SCROLL_SECS`.
  Profile with macOS `sample <pid> 10 -file out.txt` on the test binary (not `cargo`'s pid).

## Non-goals

- Raw HTTP/DOM handles for AI clients; the MCP surface stays typed and
  untrusted-framed.
- A second JavaScript realm or DOM for automation; DevTools/automation stay
  Phase 17 clients of the same core.
- Declaring site compatibility. Passing M5 means one fixture works, not that
  arbitrary sites do.
