# BlueIce Browser Core — Development Plan

## 1. Purpose / Overview

Rewrite a browser core (rendering engine) from scratch in Rust, so that **a human user and an AI agent share the same engine and the same render pass** for the representation each of them works from — instead of the common dual-track setup where a human uses Chrome and an AI drives a separate Chromium instance through Puppeteer/CDP automation.

Problems the dual-track architecture (AI driven via CDP automation) cannot solve at the architectural level:

- **Anti-bot systems discriminate**: Cloudflare, Akamai, PerimeterX, and similar services fingerprint headless/CDP-controlled Chromium. Once detected, they can serve different content, add CAPTCHAs, or block outright — the content the AI sees may **already differ** from what a human sees, not merely how it's rendered.
- **Two independent instances inherently drift**: driving the AI through external automation means spinning up a separate headless instance to read the DOM/screenshot, with a different viewport, timing, and JS execution state than the human's browser — any correspondence between the two is an after-the-fact approximation.
- **Extensibility is limited**: adding a first-class API for the AI (e.g. keeping human-facing highlights and AI-facing semantic tags in sync) can only ever be a bolted-on hack in an externally-driven architecture. Owning the render pipeline is what makes it possible as a first-class feature.

The goal of building our own engine is for what a human sees and what the AI receives as a representation to both be derived from **the same render pass and the same JS execution state**, guaranteeing they stay in sync.

This goal implies two concrete engine requirements, not just a design philosophy:

- **AI-controlled visibility, decoupled from process lifecycle**: the AI must be able to show/hide the browser's human-facing window at runtime without restarting the engine. Hiding the window for AI-only operation and showing it again later has to be the same instance, same render pass, same JS state — not a fresh process. Whether a human window is currently visible is a property of the windowing layer (Phase 4), not of whether the engine instance exists; requiring a restart to toggle visibility would reintroduce the instance-drift problem this project exists to avoid.
- **Every DOM node has an explicit, stable ID**: nodes are addressed by an explicit identifier assigned at creation, not an implicit array index or an ephemeral pointer/address. This lets the AI-facing representation (§3) reference a specific element reliably across mutations, and lets human-facing highlight state and AI-facing semantic tags stay in sync by ID rather than by structural position.

### Process architecture: `core` / `extension` / `frontend`

The browser splits into a **backend** and a **frontend**, and the backend splits again into **core** and **extension**:

- **`core`** is the engine itself — parse, DOM, CSS cascade, layout, paint, and the shared render pass §1 is built around. It runs as a single process, kept deliberately low-memory and stable: it is the one thing in the whole system that must not crash.
- **`extension`** instances each run in their own OS process, talking to `core` over IPC — **process isolation, not in-process sandboxing** (settled decision: an extension process crashing, hanging, or corrupting its own memory must be physically incapable of reaching `core`'s address space, the same guarantee Chromium's multi-process architecture provides). The extension *ecosystem* itself (an actual extension API, a store, etc.) stays out of MVP scope per §4 below — but the process-isolation boundary and IPC seam are architected into `core` from Phase 3 onward, for the same reason DOM node IDs are: retrofitting a trust boundary after the fact is far more expensive than designing it in from day one.
- **`frontend`** is the human-facing UI — necessarily a separate process from `core`, since each platform gets its own native toolkit rather than one cross-platform Rust UI (WinUI 3 on Windows, SwiftUI on Apple platforms, PySide6/Qt where a Python/Qt frontend fits, etc.). `core` owns rendering; `frontend` only presents it and forwards input, per-platform, in whatever idiom that platform's users expect.

This converges with §3's settled representation layer in a useful way: since `core` already needs a robust IPC surface for `extension` isolation, the human `frontend` and the AI-facing API (§3) become two more kinds of IPC client of that same `core` process, on equal footing — neither gets a privileged internal-only channel `core` doesn't also expose to the other. That's a stronger, more literal version of "human and AI share the same render pass" than same-process-different-API would have been.

One open engineering question this raises, not yet resolved: `frontend` needs full-frame pixel output every frame, a very different traffic shape from `extension`/AI's occasional semantic calls — likely wants a separate high-bandwidth channel (shared memory / platform-native texture handles) alongside the same control-plane IPC protocol, rather than serializing whole frames through it. Tracked as a Phase 4 design item, not decided here.

Phases 7-12 add more roles to this picture, each still mostly open design at this point (see each phase's own doc for the specific blocking questions): two local AI agents, isolated the same way `extension` is for the same crash-containment reason — a **safety gatekeeper** that must sit as a mandatory, non-bypassable checkpoint in front of risky actions (navigation, downloads, extension capability use, external-AI-agent actions), and a lower-stakes **assistant** for tasks like summarization (Phase 7); a supervisor role above `core` that owns spawning/health-checking/handoff between `core` versions so `frontend`/`extension` never talk to a specific instance directly (Phase 8); the `extension` protocol actually specified into something third parties can build against, rather than just the process/IPC boundary that already exists (Phase 9); an isolated download-manager subsystem (Phase 10) with pluggable FTP/SFTP backends (Phase 11); and an MCP server that should be an adapter over this same internal IPC protocol rather than a competing channel (Phase 12).

## 2. Licensing and Legal Framework (settled parts)

- **Fully open source, project-wide MPL 2.0** — this is not a clean-room implementation; Firefox (Gecko) and Chromium (Blink/V8) source is read directly and adapted as technical reference and a porting basis, so the project is already bound by derivative-work rules. Adopting MPL uniformly across the project is the simplest, lowest-risk choice.
  - Files derived from Gecko: must remain MPL regardless (file-level copyleft — an inherent MPL obligation, not a choice).
  - Files derived from Chromium/Blink: BSD-3-Clause permits relicensing under a different license. When relicensed to MPL, **the original BSD copyright and disclaimer notice must be preserved** (either in a header note at the top of the file, or in a centralized `NOTICE`/`THIRD_PARTY_LICENSES` file).
  - Wholly original code (the AI representation layer, the project's own architecture/glue code): has no derivative relationship, so choosing MPL is purely for project-wide consistency, not a legal obligation.
- **BSD-3-Clause also requires binary-distribution attribution, separate from the source-file requirement**: its second clause is "Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the following disclaimer in the documentation and/or other materials provided with the distribution." Per-file source headers satisfy the source-form clause but not this one. Any distributed BlueIce binary needs the Chromium notice reproduced somewhere a user actually encounters — a Help/About/Credits screen is the natural place. Gecko/MPL carries no equivalent obligation, but crediting Gecko there too keeps the disclosure consistent with the nominative-fair-use framing already used for both projects. This is a real screen to build, not just a doc convention — tracked under Phase 4 once a UI shell exists.
- **No commercial activity**: the project itself / its maintainer will not commercialize it, but this is a positioning choice, not something written into the license terms — MPL places no restriction on downstream commercial use (a field-of-use restriction would violate the OSI Open Source Definition; doing so would disqualify it as open source). Downstream commercial users are responsible for their own patent due diligence.
- **Patent scope must be understood clearly**: the patent grant in MPL §2.1 only covers patents held by *this project's contributors, over their own contributions* — it does not cover third-party patents unrelated to the project (e.g. codec patent pools, GPU rendering patents, JS engine JIT-related patents). This risk does not disappear while non-commercial; it's only less likely to be pursued in practice. Before actually moving toward commercial use, a proper patent due-diligence pass is needed.
- **Trademarks**: the names/logos "Firefox", "Mozilla", "Chrome", "Google Chrome", and "Chromium" must not be used as this project's branding. Descriptive references such as "this project references/derives from X" are fine as nominative fair use. The project name `blueice` is already unrelated to these trademarks and can stay as-is.

## 3. AI Representation Layer (settled)

**Which layer the AI representation is shared at.** Originally left open across four candidates (raw pixels + OCR, DOM+CSSOM, Accessibility Tree, custom hybrid). Resolved by directly reading how Gecko and Blink already do this in production — not by an independent validation exercise, since accessibility trees driving both screen readers and (in Blink's case) emerging AI-agent APIs are already proven at scale in exactly the browsers this project studies. See `research/accessibility-tree.md` for the full evidence and `phase-1-ai-representation-layer/spike.md` for the worked example this decision is based on.

**Decision**: an accessibility-tree-shaped representation — role, state bitset, provenance-tagged name, offset-relative bounds, following `AXNodeData`'s field categories (Blink's actual schema) — extended with four fields neither reference engine exposes generally: `opacity`, `animating` (carrying the live interpolated value, not just a bool), `occluded`/`occludedBy`/`occludedFraction`, and always-populated per-node `hovered`/`focused` booleans. Keyed by BlueIce's own stable per-node `NodeId` (§1) rather than a separate AX-ID scheme — mirroring Blink's `AXID`-reuses-`DOMNodeId` design, which `research/dom.md` found is already the closest existing prior art for this project's stable-ID requirement. `display:none` and purely-decorative non-semantic nodes stay excluded, matching human perception (§1's goal) rather than being treated as a gap to fix.

| Option | Description | Verdict |
|---|---|---|
| Raw pixels + OCR/vision model | AI consumes the rendered image directly | Rejected — high cost, low structure, doesn't reuse proven prior art |
| DOM + CSSOM | AI reads the structured tree directly | Rejected — diverges from what a human perceives (§1's goal), no proven precedent for AI-agent use at this layer |
| Accessibility Tree (ARIA semantic tree) | Reuse the existing assistive-technology standard layer | **Adopted as the base** — proven in Gecko and Blink; Blink's schema turned out to already carry more of the "visual" data the original framing assumed it lacked (color, live hover state) than expected |
| Custom hybrid representation | Derive both pixels and structured semantics from the same render pass | **Adopted, but as a small delta over the Accessibility Tree base, not a from-scratch design** — see the four added fields above |

## 4. Technical Reference Basis and Scope

- **References**: Gecko (Firefox's rendering engine), Blink + V8 (Chromium's rendering engine / JS engine) — source is read directly as technical reference and a porting basis. Mozilla's own Servo (an experimental engine written in Rust) is the closest existing precedent, and its module decomposition is worth studying.
- **Reference checkouts and research notes**: shallow, read-only clones of Gecko and Chromium live under [`reference/`](reference/) (gitignored — third-party source, not committed). Findings from reading them are written up per-subsystem under [`research/`](research/). This research ran in parallel with, and directly fed, Phase 1 (the §3 representation-layer decision was settled from this evidence rather than general knowledge — see §3) and continues to feed Phase 2 (the MVP subset needs to be scoped against how Gecko/Blink actually structure parsing/cascade/layout) — it starts now rather than waiting for those phases to formally begin.
- **Reality check**: rewriting a browser engine from scratch is Servo-scale (dozens of engineers, multiple years) or Ladybird-scale (a dedicated team, multiple years) engineering effort. This project's goal is "an AI browsing websites," not "a general-purpose consumer browser at feature parity with Chrome from day one." The MVP scope should converge on:
  1. A minimal usable rendering pipeline (HTML parse → DOM → CSS cascade → layout → paint)
  2. The shared human/AI representation layer (implemented per the §3 decision)
  3. Explicitly not yet: an extension ecosystem, multi-tab state sync, full JS engine optimization, DevTools
- **Testing**: [`testing/TEST_PLAN.md`](testing/TEST_PLAN.md) is the test pyramid (unit/integration/UI), coverage policy, and CI setup — settled infrastructure, not an open decision, so it's referenced here rather than repeated per phase.

## 5. Risk Register (carried over from prior discussion, tracked long-term)

- **Patent risk**: codec, GPU rendering, and JIT-related technology has dense patent coverage. Risk is lower but not zero at the non-commercial stage; patent due diligence is needed before moving toward commercial use.
- **Trademark risk**: branding/naming must continue to stay clear of Mozilla/Google trademark scope.
- **Legal exposure from the AI agent's browsing activity itself**: independent of the engine's IP status — most site ToS prohibit automated access, and there's robots.txt and anti-bot mechanisms to account for. Before the AI actually browses real third-party sites in production, it needs its own policy and inventory; "this is our own engine" alone doesn't sidestep this. Phase 7's safety-gatekeeper agent is the concrete technical mitigation being designed for this — but a policy/inventory is still needed independent of it, since the gatekeeper only intercepts the risk categories its taxonomy actually covers.
- **Engineering scale risk**: a full-featured engine rewrite is too large in scope; needs to be kept in check by the MVP convergence described in §4.

## 6. Progress Tracking

| Phase | Content | Status |
|---|---|---|
| [Phase 0](phase-0-license-legal-foundation/PLAN.md) | License/legal foundation (`LICENSE` file, per-file header conventions in `CONTRIBUTING.md`) | Done |
| [Phase 1](phase-1-ai-representation-layer/PLAN.md) | §3 representation layer decided; define the AI-side API shape, schema, and versioning | In progress |
| [Phase 2](phase-2-mvp-scope/PLAN.md) | Finalize MVP scope (supported HTML/CSS/JS subset) | In progress |
| [Phase 3](phase-3-engine-skeleton/PLAN.md) | Rust core engine skeleton (parse → DOM → layout → paint) | In progress |
| [Phase 4](phase-4-human-rendering-path/PLAN.md) | Human-visible rendering path | Not started |
| [Phase 5](phase-5-ai-representation-output/PLAN.md) | AI representation output path (implemented per the Phase 1 decision) | Not started |
| [Phase 6](phase-6-ai-agent-integration-demo/PLAN.md) | AI agent integration demo (running on our own engine, not CDP) | Not started |
| [Phase 7](phase-7-local-ai/PLAN.md) | Two local AI agents: safety gatekeeper (primary) + assistant | Not started |
| [Phase 8](phase-8-live-core-hotswap/PLAN.md) | Live `core` hot-swap / seamless update, no browser restart | Not started |
| [Phase 9](phase-9-extension-protocol/PLAN.md) | BlueIce extension protocol (third-party plugins on the `extension` architecture) | Not started |
| [Phase 10](phase-10-download-manager/PLAN.md) | Built-in download manager (chunked/resumable/multi-threaded transfers) | Not started |
| [Phase 11](phase-11-transfer-protocol-clients/PLAN.md) | FTP/SFTP and other transfer-protocol clients | Not started |
| [Phase 12](phase-12-mcp-server/PLAN.md) | MCP server exposing BlueIce to Claude Code and other AI agents | Not started |
| [Phase 13](phase-13-bluejs-engine/PLAN.md) | BlueJS: custom JavaScript engine (decided over embedding an existing one) | Not started |

Phases 7-12 extend the browser beyond the rendering-engine MVP (Phases 0-6) into a full application platform — added once that broader scope was set, not part of the original MVP path. Several (7, 8 especially) have open design questions blocking real work; see each phase's own doc. Phase 13 (BlueJS) is different from 7-12 in that respect — it resolves Phase 2's JS-strategy decision and is genuinely MVP-relevant (Phase 2's HTML/CSS scope already assumes `<script>` exists), numbered last only because inserting it earlier would have meant renumbering everything else.

This document is the first version of the plan, meant to be refined in later iterations.
