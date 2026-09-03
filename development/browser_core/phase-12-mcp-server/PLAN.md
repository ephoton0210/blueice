# Phase 12 — MCP Server (Standard AI Agent Integration)

[← Back to plan](../BROWSER_CORE_PLAN.md)

**Status**: Not started

## Objective

Expose BlueIce's capabilities — browsing/rendering (Phase 1/5's AI-facing API), downloads (Phase 10), file transfer (Phase 11), and whatever else applies — through a standard MCP server, so Claude Code or any other MCP-compatible AI agent can drive BlueIce using the standard protocol instead of a bespoke one.

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

- [ ] Confirm MCP is implemented as an adapter over the existing internal IPC protocol, not a parallel channel
- [ ] Confirm which of Phase 1/5/7/9/10/11's capabilities are in scope for MCP exposure
- [ ] Design MCP tool definitions once the underlying APIs they wrap are real (blocked on those phases, not startable in isolation)
- [ ] Build the MCP server against those tool definitions, as an on-demand-spawned process (with Phase 8's launcher), per `research/multi-process-memory.md`
- [ ] Validate against an actual MCP client (e.g. Claude Code) driving a real BlueIce instance end to end
