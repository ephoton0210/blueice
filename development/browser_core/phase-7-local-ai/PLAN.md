# Phase 7 — Local AI & Built-in AI Interface

[← Back to plan](../BROWSER_CORE_PLAN.md)

**Status**: In progress — the safety-gatekeeper's minimal slice (mechanism, protocol, concurrency, fail-closed behavior; see "Wiring design (resolved 2026-09-08)" below) is built and tested, per `CLAUDE.md`'s Phase 7 status paragraph. The gatekeeper's real rule-base/AI review content, the assistant agent, and per-component wiring beyond `core` navigation are still not started.

## Objective

Ship two built-in, on-device AI agents, distinct from Phase 1/5's AI-facing API (that API lets an *external* agent — Claude, another MCP client — drive BlueIce from outside over IPC; this phase is BlueIce's *own* embedded AI capability, usable with no external agent connected at all):

1. **Safety gatekeeper** — the primary, most important reason this phase exists. Reviews every incoming page and every risky action before it executes, regardless of whether a human, an extension, or an external AI agent (via Phase 5/12) initiated it. Backed by an independent, non-AI rule-base layer specifically because the gatekeeper itself — being an AI reading untrusted page content — is a plausible prompt-injection target.
2. **Assistant** — a general local helper: organizing data, summarization, live translation, and similar tasks. Secondary to the gatekeeper in priority, but shares the same underlying infrastructure decisions (runtime, process model).

## Design sketch

### Content processing pipeline

Both agents hook into the same place in the existing Phase 3 pipeline (parse → DOM → CSS cascade → layout → paint), at different points, for different reasons:

1. HTML response arrives (before parsing).
2. **Gatekeeper review** (AI): judges the content/context for risk.
3. **Rule-base review** (deterministic, independent of the AI layer — see rationale below).
4. **If either layer flags risk, the browser does not proceed** (fail-closed — settled, see Open questions). If the flagged risk is specifically about submitting sensitive information to a risky site, the gatekeeper intercepts the submission and surfaces a detailed explanation of the risk to the user, rather than a bare "blocked."
5. If clear: HTML parses into the DOM as normal (Phase 3).
6. **If live translation is enabled**: the assistant walks the DOM's text nodes and replaces their content with the target-language translation *before* layout runs — not a post-render overlay. Translated text can be substantially longer or shorter than the source (e.g. English→Chinese often shortens, English→German often lengthens), so layout has to compute against the *translated* text or every page reflows incorrectly; a visual overlay applied after layout would either misalign or require a second reflow pass, so the substitution has to land before step 7, not after.
7. CSS cascade → layout → paint proceed as normal, operating on the (possibly translated) DOM.

### Agent 1: Safety gatekeeper

**The core design constraint: it must be a real enforcement point, not an advisor that can be silently skipped.** If the gatekeeper were just another IPC client producing an opinion nobody's required to check, a compromised extension or a malicious/broken external AI agent could simply not ask it and proceed anyway — that would make it decoration, not security. This means the components that can *take* a risky action (`core` for navigation/permissions, `backend/downloads` for file transfers, the `extension` host for capability use, the Phase 12 MCP adapter for external-agent-initiated actions) need to *call through* the gatekeeper as a mandatory step before executing the risky class of action, not just have the option to consult it.

**Enforcement mechanism — resolved to a concrete pattern by [`research/safe-browsing-enforcement.md`](../research/safe-browsing-enforcement.md).** The research studied Chromium's Safe Browsing and Firefox's url-classifier as the closest real prior art for "a mandatory pre-action safety checkpoint" — and found both have real gaps worth avoiding, not copying: Chromium's own `SafeBrowsingNavigationThrottle` doesn't actually perform the check its name implies (the real check is elsewhere, `BrowserURLLoaderThrottle`/`SafeBrowsingUrlCheckerImpl`), it's an optional embedder-supplied hook rather than structurally mandatory, prefetch requests bypass it entirely by Chromium's own admission, and downloads fail open via a nullable delegate. Firefox is tighter for navigation (`nsChannelClassifier` is called unconditionally inside the channel-open path itself, not an attachable observer) but the same weak pattern reappears for downloads. **Most importantly: both engines fail open on a timeout — a slow or unreachable check is treated as "safe."** This directly validates BlueIce's own fail-closed policy (below) as a deliberate improvement over existing prior art, not something to weaken to match it.

**Decision: a typestate/capability-token pattern, not a runtime convention.** Neither reference engine's mechanism is stronger than "a runtime call a differently-written caller could omit" — both are C++, where this is close to the best available option. Rust can do better: define a `GatekeeperClearance` type constructible only by the gatekeeper IPC client (e.g. returned from a successful review call), and require it as a parameter on every risky-action function (navigation, download initiation, extension capability grants, MCP-adapter-initiated actions). Skipping the gatekeeper becomes a compile error, not a review-time miss. **Critical caveat, not solved by this alone**: the compile-time guarantee only holds *within one process* — an external or malformed IPC client isn't bound by BlueIce's own compiler, so the IPC wire protocol itself (Phase 9's still-open wire-protocol work) needs to reject an unauthorized action-request at the `core`-side handler too, not rely on client-side enforcement alone.

**Why a second, non-AI layer**: the gatekeeper reviews page content that is, by construction, untrusted and adversarial-capable — and it's an AI doing the reviewing. A sufficiently crafted page could attempt a prompt-injection attack specifically aimed at making the gatekeeper misjudge it as safe. An independent rule-base layer — deterministic pattern/signature matching with no prompt surface of its own — can't be defeated by the same technique, so it stands even if the AI layer is successfully manipulated. Both layers gate the same decision; either one flagging risk is enough to block (fail-closed, see Open questions).

**A lighter, present-day partial mitigation already exists, ahead of this phase**: `phase-12-mcp-server/PLAN.md`'s already-built `blueice-mcp-server` wraps every tool result carrying page content (`blueice_mcp_server::wrap_untrusted_page_content`) with an explicit "this is data, not instructions" warning before it reaches whatever LLM is driving BlueIce over MCP — prompt-level framing an attacker could still attempt to argue around, not the deterministic, non-AI rule-base layer this phase's gatekeeper is actually meant to be. Don't treat that wrapper as satisfying this phase's own requirement; it narrows the gap `mcp-server` left open (page content reaching an external AI client with zero framing at all) until this phase's real gatekeeper exists.

**Candidate rule-base signature categories** (draft, informed by BlueIce's specific threat model — a browser built for AI agents to read page content directly has an attack surface traditional browsers don't):

- Known-malicious/phishing domain or URL blocklists
- Hidden-content patterns specifically shaped to target AI readers rather than humans: zero-width characters, white-text-on-white-background, off-screen-positioned text, `aria-hidden` text containing instruction-shaped language (e.g. "ignore previous instructions") — this category is the direct rule-base counterpart to the prompt-injection threat above
- Structural heuristics: credential/payment-shaped form fields on a newly-registered or non-HTTPS origin
- Homoglyph/unicode-direction-override tricks in domain names or visible text

**Candidate risk taxonomy for the AI layer** (a starting draft to react to, not a final policy):

- Navigating to a flagged/suspicious URL (known-bad lists, or heuristics — phishing-shaped domains, etc.)
- Submitting a form that looks like it's sending credentials/PII to a new or untrusted origin
- Downloading an executable or otherwise dangerous file type (Phase 10)
- Granting a site a dangerous permission (camera, microphone, location, clipboard)
- An extension (Phase 9) exercising `network:intercept` or `dom:write` in a pattern that looks like data exfiltration
- An external AI agent (via Phase 5's API or Phase 12's MCP server) attempting an action outside what its session was scoped to — e.g. file-system access beyond a downloads directory, or a burst of actions matching an automated-abuse pattern

This is the concrete technical mitigation for the risk already flagged in plan §5 ("Legal exposure from the AI agent's browsing activity itself... needs its own policy and inventory") — worth cross-referencing there once this taxonomy firms up.

**Latency/model implications**: the gatekeeper needs to run on every risky action *and every incoming page* with low latency and high precision on "is this actually risky" — that profile (fast, narrow, high-precision classification) may genuinely call for a different, smaller, possibly fine-tuned/classifier-style model than the assistant's, rather than assuming both agents share one model. Running full-page review on every navigation is also a real, accepted latency cost given the project's stated priority (safety over speed here) — worth revisiting caching/allowlisting for repeat-visited trusted domains as a *later* optimization, not a default weakening of "review everything."

**Script-level review**: for `<script>` content specifically, the gatekeeper doesn't need to parse JS itself — Phase 13 (BlueJS) is designing an AI-facing capability-summary output (derived from its own AST) exactly for this consumer, covering network-initiating calls, storage access, `eval`-like dynamic code, and DOM-mutating calls. The exact hook points and summary granularity are still open and need designing together with Phase 13, not assumed here.

### Wiring design (resolved 2026-09-08)

Resolved via a research pass plus explicit user direction on the two open tradeoffs: **both URL-level and content-level review land in the same first slice, and the round-trip must not block other clients/tabs sharing `core`'s one launcher connection** — the harder option on both axes, deliberately chosen over the cheaper URL-only/blocking-is-fine alternative.

**Process & connection shape**: `ai-gatekeeper` is a new always-resident process (`backend/ai-gatekeeper`), speaking a small new request/reply protocol over `blueice_ipc`'s existing length-prefixed-JSON framing primitives. Unlike `core`'s own launcher connection (held for the life of the session), a gatekeeper check opens a **short-lived, per-check connection** (connect → request → reply → disconnect) rather than multiplexing over one shared connection — this sidesteps needing any request-correlation/multiplexing protocol on the gatekeeper side, and lets concurrent checks from different tabs run as fully independent connections with zero shared mutable state between them. Registers `AlwaysResident` in `blueice-launcher`'s `ProcessRegistry`, per `research/multi-process-memory.md`.

**Two-stage review, both in this slice**:
1. **URL stage** — before any network fetch, `core` sends `GatekeeperRequest::CheckUrl { url }`. Catches known-bad domains cheaply, before spending a fetch on them.
2. **Content stage** — after fetch, before parse/cascade/layout, `core` sends `GatekeeperRequest::CheckContent { url, html }`. This is the stage that actually addresses the phase's named primary threat (hidden/adversarial content aimed at an AI reader) — URL blocklisting alone cannot catch it.

Either stage returning `Rejected` ends the navigation; both must clear for it to proceed. A connection/IO failure talking to the gatekeeper is treated the same as `Rejected` (fail-closed, per the already-settled failure-mode decision above).

**Concurrency**: `run_session`'s dispatch stops assuming one read+dispatch+write step per loop iteration. Every navigate-capable action (`Navigate`, a link-`Click`/`ActOn`'s resulting href, `OpenTab{url}`) becomes a two-phase operation:
- **Phase 1 (synchronous, in the main loop)**: resolve the target href/URL (identical to today), record a `PendingNav { tab_id, seq, request_id }`, bump that tab's `pending_nav_seq`, and hand the URL to a background `std::thread::spawn` that does: connect+`CheckUrl` → (if cleared) fetch via `blueice-net` → connect+`CheckContent` → report an outcome back over an `mpsc::Sender` the main loop owns. The main loop does **not** block on this thread; it returns to the top of the loop immediately.
- **Phase 2 (asynchronous, polled by the main loop)**: `run_session`'s stream gains a read timeout (a new small `ReadTimeout` trait, implemented for `UnixStream` — every real caller, production and test, already uses `UnixStream` via `UnixStream::pair()`, so this isn't a breaking bound in practice) so the loop can periodically drain the completion channel between reads. On a completion whose `seq` still matches the tab's current `pending_nav_seq` (not superseded by a newer navigation issued to the same tab in the meantime — stale completions are silently discarded, matching ordinary browser "a new navigation cancels the in-flight one" behavior), the main thread applies the result against the real `Page`/`TabManager` (never touched by the background thread itself) and writes the reply tagged with the *original* `request_id`/`tab_id` — `Navigated`+`FrameReady` on a cleared outcome, a new `ServerMessage::GatekeeperBlocked { reason, category, url }` on a blocked one.

This deliberately reuses the same "poll loop + pending-work channel" shape `phase-13-bluejs-engine/PLAN.md`'s own event-loop integration will need for timers/macrotasks — not a coincidence: both problems are "let something finish in the background without blocking the one shared connection," and building the mechanism once now means Phase 13 extends it rather than inventing a second one.

**`Page::navigate` split**: the fetch step (currently inline inside `navigate`) separates from the parse+layout step, since fetch now happens on the background thread and parse+layout must happen back on the main thread against real `Page` state. `Page` gains `Page::apply_fetched(clearance: GatekeeperClearance, url: &str, html: &str)` (parse/cascade/layout only, no network); `navigate` stays as a synchronous `fetch`-then-apply convenience for callers that don't need gating (tests, and BlueIce's own trusted `about:` pages, which are never fetched and skip the gatekeeper entirely).

**Same-tab collision policy**: a second `Navigate` (or link click) arriving for a tab that already has a pending gated navigation bumps that tab's `pending_nav_seq`, superseding the old one — the old background thread runs to completion but its result is discarded on arrival (not actively cancelled; a known minimal-slice limitation — rapid repeated navigation could transiently accumulate a few harmless background threads/connections, worth revisiting if it proves to matter in practice). Other messages to the *same* tab that don't navigate (`Resize`/`Scroll`/`Hover`) apply immediately against the tab's current (pre-navigation) `Page` state, the same way a real browser reflows/scrolls the still-displayed old page while a new one loads. Messages to *other* tabs are entirely unaffected, as today.

**`GatekeeperClearance`**: even though this slice's checks are a trivial stub, the enforcement-mechanism's Rust type discipline still applies in full — a cleared outcome carries a `GatekeeperClearance` value (non-`Clone`, no public constructor outside the gatekeeper-client call site, fields: `tab_id`, `url`) that `Page::apply_fetched` requires as a parameter, so skipping the gate is a compile error even in this minimal slice, not just a runtime convention.

**Minimal first slice's actual scope, given the above**: `ai-gatekeeper` itself is a trivial stub that always clears both stages (no model, no rule-base yet) — the mechanism (process, protocol, concurrency, two-stage hook points, fail-closed-on-down, the `GatekeeperClearance` typestate) is what's real and tested in this slice, not the risk taxonomy or rule-base content.

### Agent 2: Assistant

Organizing data, summarization, and **live translation** per the given scope. Can tolerate higher latency and more model variety than the gatekeeper — a good candidate for a larger local model, or even a fallback that only activates when no external AI agent is connected.

**Live translation**, specifically: when enabled, output is the target-language text directly substituted into the DOM before layout (see the pipeline above), not a translated overlay/popup shown alongside the original. Raises a few concrete follow-on questions of its own — see Open questions.

### Shared infrastructure

**Runtime candidates** (final pick may differ per agent, given the gatekeeper/assistant latency profiles above — this table isn't assumed to mean "one model for both"):

| Runtime | Pros | Cons |
|---|---|---|
| [`candle`](https://github.com/huggingface/candle) (Hugging Face's Rust ML framework) | Pure Rust — no C++ toolchain dependency, fits this codebase's all-Rust posture and avoids cross-compilation pain across the eventual multi-platform frontend matrix (plan §1); supports GGUF-quantized small models (Llama/Mistral/Phi-class) via `candle-transformers` | Smaller/less battle-tested model zoo and kernel optimization than llama.cpp |
| `llama.cpp` (via Rust bindings, e.g. `llama-cpp-2`) | Most mature quantization/perf story, largest GGUF model ecosystem | C++ dependency — build complexity (cmake) and another toolchain to keep working across every target platform |
| ONNX Runtime (via `ort`) | Strong tooling, good fit for non-generative models (embeddings, classifiers) — plausibly a good fit for the gatekeeper's AI layer specifically if it ends up being a classifier rather than a generative model | Less natural fit for autoregressive text generation than llama.cpp/candle's purpose-built inference loops |

**Process design**: given the gatekeeper's security-critical role, it likely deserves its *own* process — `backend/ai-gatekeeper/` — separate from `backend/ai-assistant/`, so the assistant's failure modes (a stuck summarization/translation request, a heavier/slower model) can't degrade the gatekeeper's availability, and so the gatekeeper can stay as deliberately minimal and robust as Phase 8's launcher is designed to be. The rule-base layer, being deterministic and lightweight, doesn't need the same isolation an inference workload does, but should stay code-independent from the AI layer (not sharing logic/state) so a defeat of one genuinely doesn't imply a defeat of the other. Both AI agents are isolated OS processes on the same reasoning as `extension` (plan §1) — inference workloads are resource-heavy and their stability under arbitrary input is unproven — and both speak the same IPC family `core` already exposes rather than a bespoke channel.

**Resource governance**: since both agents run as separate OS processes from `core`, plan §1's "core must not crash" guarantee already holds structurally — but a runaway inference process could still starve the host machine's shared resources (RAM/CPU) and degrade `core` indirectly. Worth enforcing an OS-level resource ceiling on both processes (cgroups on Linux, Job Objects on Windows, similar on macOS) once this phase is built, not just relying on process isolation alone. **Refined by [`research/multi-process-memory.md`](../research/multi-process-memory.md), which studied Chromium's and Firefox's fleet-wide memory management (neither has a true whole-fleet budget, only per-process limits) and made per-BlueIce-process recommendations**: `ai-gatekeeper` should stay always-resident (it's a fail-closed dependency — treating it as a teardown candidate would conflict with the fail-closed policy above), while `ai-assistant` is a reasonable idle-teardown candidate (with a carve-out for live translation, which needs to stay responsive while active) — the Phase 8 supervisor is flagged as the natural place to own that idle-teardown authority once it exists.

## Open questions (blocking real design)

- ~~Enforcement architecture~~ — **resolved for `core` navigation** by "Wiring design (resolved 2026-09-08)" above (per-component detail for `backend/downloads`, the `extension` host, and the Phase 12 MCP adapter still follows once those exist). Still open: the IPC-wire-protocol-level enforcement the design above flags as a separate, still-necessary layer (ties into Phase 9's wire-protocol work, which now has a concrete `GatekeeperClearance` shape to build against).
- ~~Failure mode~~ — **resolved, and now independently corroborated**: fail-closed, including when the gatekeeper process itself is down/unresponsive (a timeout is not "safe"). `research/safe-browsing-enforcement.md` found both Chromium's Safe Browsing and Firefox's url-classifier fail *open* on exactly this case — confirms BlueIce's policy is a deliberate improvement, not something to reconsider toward matching prior art.
- ~~Synchronous blocking vs. async review~~ — **resolved**: every page load and every risky action goes through both review stages (URL and content), and the round-trip is *non-blocking* with respect to other clients/tabs sharing `core`'s one connection — see "Wiring design" above for the concurrency mechanism. The requesting client's own request still waits for its own result (that's inherent to "review everything before it happens"), but other traffic on the shared connection is never stalled by someone else's pending review.
- **Rule-base content/format**: the candidate signature categories above need to become an actual maintained ruleset — sourced from a threat-intel feed (e.g. Google Safe Browsing-style lists) plus BlueIce-specific heuristics (the hidden-AI-targeted-content patterns), versioned and updatable independently of the AI model.
- **Risk taxonomy completeness**: the AI-layer candidate list above is a starting draft — needs review for gaps and for false-positive risk (a gatekeeper that blocks too aggressively makes the browser unusable, the same practical failure mode as fail-closed-when-down).
- **Live translation and the Phase 1/5 AI-facing representation**: when translation is active, should an *external* AI agent (via Phase 5/12) see the original text or the translated text? Plan §1's whole premise is human and AI perceiving the same rendered state — since translated text is what's actually on screen once substituted pre-layout, the representation reflecting translated text seems like the consistent answer, but this is a real consequence worth confirming rather than assuming.
- **Live translation reversibility**: does BlueIce retain the original-language DOM/text alongside the translated version (so a user can toggle back, and so the gatekeeper/rule-base review — which should probably run on the *original* content, not a translation of it — has something authoritative to check), or is the substitution destructive?
- **Assistant capability scope**: organizing data, summarization, live translation are the given starting scope — worth confirming whether that's the complete initial capability list.
- **Model/runtime choice per agent**, once the above firm up.

## Checklist

- [x] Scope what the local AI is for — two agents: safety gatekeeper (primary, two-layer AI+rule-base pipeline) and assistant (secondary)
- [x] Decide fail-open vs. fail-closed — fail-closed
- [x] Decide the enforcement mechanism — `GatekeeperClearance` capability-token pattern, see `research/safe-browsing-enforcement.md`
- [x] Wire the capability-token requirement into `core` navigation (minimal slice, per "Wiring design" above) — `backend/downloads`, the `extension` host, and the Phase 12 MCP adapter remain, since none of those exist/are gated yet
- [x] Build the minimal-slice `ai-gatekeeper` process (always-clears stub), the `CheckUrl`/`CheckContent` protocol, and `core`'s non-blocking two-phase dispatch integration — see "Wiring design (resolved 2026-09-08)"
- [ ] Add IPC-wire-protocol-level enforcement (server-side, not just the client-side compile-time guarantee), with Phase 9
- [ ] Confirm synchronous-blocking applies to every page load, not just the action-level risk taxonomy
- [ ] Build the initial rule-base ruleset (signatures above) and decide its update/versioning mechanism
- [ ] Review and firm up the AI-layer risk taxonomy draft above
- [ ] Decide where the live-translation DOM-substitution hook sits relative to Phase 3's HTML→DOM pipeline, and whether original text is retained alongside the translation
- [ ] Decide whether the Phase 1/5 AI-facing representation reflects translated or original text when live translation is active
- [ ] Confirm the assistant's initial capability scope
- [ ] Choose the model/runtime per agent (may differ between gatekeeper and assistant)
- [ ] Finalize `backend/ai-gatekeeper/` vs. `backend/ai-assistant/` module boundaries, and where the rule-base layer's code lives relative to both
- [ ] Decide whether each agent is a client of the Phase 1/5 IPC surface or needs something that surface doesn't expose
- [ ] Define the resource budget (memory/CPU ceiling) for each process
- [ ] Design `ai-assistant`'s idle-teardown behavior (with a live-translation carve-out), coordinated with the Phase 8 supervisor once it exists
- [ ] Cross-reference the finished risk taxonomy back into plan §5's "AI agent browsing" risk entry
