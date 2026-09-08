# Phase 12 — MCP Server (Standard AI Agent Integration)

[← Back to plan](../BROWSER_CORE_PLAN.md)

**Status**: In progress (a real foundation exists — `backend/mcp-server` — built ahead of this phase's original place in the sequence; see "Foundation built early" below for why and exactly what it covers)

## Objective

Expose BlueIce's capabilities — browsing/rendering (Phase 1/5's AI-facing API), downloads (Phase 10), file transfer (Phase 11), and whatever else applies — through a standard MCP server, so Claude Code or any other MCP-compatible AI agent can drive BlueIce using the standard protocol instead of a bespoke one.

## Foundation built early

Raised while Phase 5 was fresh, not while working through the numbered sequence: differential testing BlueIce's rendering against a real Chromium (via Puppeteer) needs a stable, external way to drive BlueIce *now*, not once Phases 6-11 are also done — waiting for this phase's original place in the sequence would have blocked that testing work for no good reason, since Phase 5's API was already real. `backend/mcp-server` (`blueice-mcp-server`) is that foundation: a real, working MCP server (stdio transport, via the official `rmcp` Rust SDK — reused rather than hand-rolling JSON-RPC framing, same "solved infrastructure" reasoning as `ureq`/`winit`/`fluent-bundle` elsewhere) exposing exactly Phase 5's API as tools, and nothing beyond it:

- `navigate(url)`, `get_page_representation()` — wrap `ClientMessage::Navigate`/`GetRepresentation` directly.
- `click(node_id)`, `type_text(node_id, text)`, `focus(node_id)`, `scroll_into_view(node_id)` — wrap `ActOn` with each `NodeAction` variant.
- `highlight(node_id)` — wraps `Highlight`.
- `screenshot()` — the one tool with no direct `ClientMessage` equivalent: reads the most recently cached `FrameReady`'s shared-memory frame and PNG-encodes it, since there's no "resend the current frame" message (every frame is the side effect of some state change).
- `get_dom()` — wraps `ClientMessage::GetDom`, added specifically for [Phase 15](../phase-15-chromium-differential-testing/PLAN.md)'s differential-testing harness (needs the *unfiltered* DOM, unlike `get_page_representation`'s AI-facing subset).

**A real design problem solved along the way, not glossed over**: several `ClientMessage`s produce a *variable* number of replies (an `ActOn`/coordinate `Click` that doesn't land on a link produces none at all — `blueice_engine::session`'s own documented behavior). An MCP tool call needs a deterministic reply, and a read-timeout heuristic would be flaky. The fix: every state-changing tool immediately pipelines a `GetRepresentation` after its own message and reads in a loop until the `Representation` reply appears — which is always exactly one, always last, and nothing else produces one — collecting any `Error` seen along the way rather than returning on it immediately (a `Navigate` failure means `Error` arrives *before* the pipelined `Representation`, and returning early would leave that trailing `Representation` unread for the next call to misinterpret). See `backend/mcp-server/src/lib.rs`'s module docs and `CoreConnection::send_and_drain`.

**Deliberately not done in this early pass** (real Phase 12 work, not skipped by oversight): Phase 10/11 tools (don't exist yet), `bluejs_run`/`bluejs_analyze` (Phase 13 doesn't exist yet), on-demand process *spawn-timing* (`blueice-mcp-server` still spawns unconditionally on startup, though it now shares an already-running `core` via Phase 8's launcher when one exists — see below), and validation against a real MCP client such as Claude Code (only tested against a fake `core` responder and the real `blueice-core` subprocess directly — not yet through an actual MCP client speaking the wire protocol end to end). The `protocol_version` handshake Phase 1/5 originally deferred is no longer on this list — see the checklist below.

## Design sketch

**Process design**: `backend/mcp-server/` as a thin adapter process — speaks MCP (JSON-RPC based, per the MCP spec) on one side, and BlueIce's own internal IPC protocol on the other. It translates incoming MCP `tools/call` requests into internal IPC requests against `core`/`downloads`/etc., and (where the MCP transport in use supports it) surfaces internal state changes back out as MCP notifications — it does not implement any browsing/download/transfer logic itself, only the translation. Being a stateless adapter, [`research/multi-process-memory.md`](../research/multi-process-memory.md) flags it as the clearest on-demand-spawn candidate in the whole process fleet — no reason for it to run at all when no MCP client is connected, unlike `core`/`ai-gatekeeper` which need to stay resident.

**Tool sketch** (signatures will firm up once the wrapped APIs are real, but the shape — one MCP tool per capability, thin pass-through to internal IPC — is reasonably stable regardless of exactly how Phase 5/10/11 end up looking in detail):

- `navigate(url)`
- `get_page_representation()` — wraps the Phase 1/5 AI-facing representation
- `click(node_id)` / `type(node_id, text)` — act on the stable DOM node IDs plan §1 already requires
- `download_file(url, dest)` / `list_transfers()` — wraps Phase 10
- `ftp_connect(...)` / `sftp_connect(...)` — wraps Phase 11
- `bluejs_run(code)` — wraps the Phase 13 `bluejs` shell's batch mode, so an MCP client can execute/test a JS snippet directly
- `bluejs_analyze(code)` — wraps Phase 13's AI-facing parse/analysis output (AST plus the capability summary the Phase 7 gatekeeper also consumes), so an MCP client can ask "what does this script do" without executing it

## Open questions

- **MCP should be an adapter, not a fourth protocol.** Plan §1 already establishes `core` exposing one IPC surface shared by `extension`, `frontend`, and the AI-facing API (Phase 5) — introducing MCP as a separately-designed channel would fragment that "one source of truth" principle. The default assumption going in should be: an MCP server process translates MCP tool calls into calls against the existing internal IPC protocol, rather than `core` growing a second, parallel API surface. Confirm this holds once the IPC protocol (Phase 9's wire-protocol work) actually exists — don't assume it without checking.
- **Which capabilities become MCP tools, and their shape** — genuinely can't be fully specified until the subsystems being wrapped (Phase 5 at minimum; Phase 10/11 for file-transfer tools) have real APIs. Speccing MCP tool signatures against not-yet-existing APIs would just need redoing.
- **Scope of "all of it"**: the user's ask was that all 6 new components be callable via MCP — confirm whether that includes Phase 7 (local AI) and Phase 9 (extensions) as MCP-controllable too, or just the browsing/download/transfer capabilities.

## Checklist

- [x] Confirm MCP is implemented as an adapter over the existing internal IPC protocol, not a parallel channel — `backend/mcp-server` wraps `blueice-ipc` exclusively, no browsing logic of its own
- [x] Design and build MCP tool definitions for the capabilities that are real today (Phase 5's API) — see "Foundation built early" above
- [ ] Confirm which of Phase 7/9/10/11's capabilities (once they exist) are in scope for MCP exposure, beyond the Phase 5 API already covered
- [ ] Extend the tool set as Phase 10/11/13 land (`download_file`/`list_transfers`, `ftp_connect`/`sftp_connect`, `bluejs_run`/`bluejs_analyze`)
- [x] Connect to Phase 8's rendezvous socket first, falling back to today's unconditional private `CoreProcess::spawn` only if nothing is listening there — the *shared-instance* half of this item (architecture-wide risk survey finding: today, `mcp-server` and `frontend` can never observe the same `core`/`Page` at all, the same dual-track split plan §1's core goal rejects). Built per `phase-8-live-core-hotswap/PLAN.md`'s "Minimal first slice": `CoreProcess::connect` tries the rendezvous socket first; `BlueIceMcpServer::spawn` now calls it instead of the unconditional-spawn `CoreProcess::spawn`. Existing tests (including the real-subprocess `tests/core_process.rs`) keep exercising the private-spawn fallback path unchanged, since none of them run a launcher alongside.
- [ ] Switch `mcp-server` itself to on-demand process spawning (spawn on first inbound MCP connection, idle-teardown with zero connected clients) — the *spawn-timing* half of the original item, unrelated to the shared-instance half above (`mcp-server` remaining a separate on-demand-spawned adapter process either way; only whether it privately spawns its own `core` or attaches to a shared one changes), per `research/multi-process-memory.md`
- [x] Implement the `protocol_version` handshake Phase 1/5 deferred, once a genuinely independent client makes protocol drift a real risk — done, per `phase-8-live-core-hotswap/PLAN.md`'s minimal-slice checklist: `CoreConnection::handshake` (`blueice_ipc::client_handshake`), called from both `CoreProcess::spawn` and `connect_to`
- [ ] Validate against an actual MCP client (e.g. Claude Code) driving a real BlueIce instance end to end
- [x] Mark every tool result that carries page-derived content as untrusted data, not instructions, before it reaches the driving LLM — `blueice_mcp_server::wrap_untrusted_page_content` (a plain, tested string-formatting function in `lib.rs`, applied in `server.rs`'s `outcome_to_result`/`get_page_representation`/`get_dom`/`screenshot`), plus a matching warning in `get_info()`'s server `instructions`. This is a lighter-weight, present-day mitigation for the exact threat `phase-7-local-ai/PLAN.md`'s safety-gatekeeper design already names (hidden, instruction-shaped content aimed at an AI reader) — prompt-level framing, not a hard guarantee, and explicitly not a substitute for Phase 7's independent non-AI rule-base layer once that's built (see that phase's own doc). Raised by the user directly asking whether page content reaching an AI is safely treated as data, not something executable.
