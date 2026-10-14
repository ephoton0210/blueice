# Debugging BlueIce with Claude Code

`scripts/dev-stack.sh` starts a complete stack with every debug aid on, and the
checked-in `.mcp.json` points Claude Code's MCP client at it. Everything here is
**observation-only**: nothing lets an agent press a key in the native window,
approve a settings proposal, or edit settings.

## Start

```
scripts/dev-stack.sh start            # builds, then starts launcher + core + window
scripts/dev-stack.sh start --no-window
scripts/dev-stack.sh status | logs | stop
```

Open Claude Code in the repo root; it offers the `blueice` server from
`.mcp.json` (approve it once). Files live in `/tmp/blueice-dev`
(`BLUEICE_DEV_DIR` overrides it).

## What shows what

| Question | Where |
|---|---|
| What is running right now? | MCP `blueice_status`, or `scripts/dev-control.py status`: launcher/core pids, core generation (bumped by each hot-swap), assistant backend/pid/start count, waiting proposal |
| What did the launcher just do? | `/tmp/blueice-dev/trace.log` (`BLUEICE_TRACE`): assistant spawn/reconfigure/ceiling kill, proposal accepted/blocked/approved/denied by id, trusted-window request kinds, cutover start/done/failed. One `[+seconds] kind: detail` line each; no settings values, digests, tickets, or page data |
| Why did a process die? | `/tmp/blueice-dev/launcher.log` (launcher stderr, with `RUST_BACKTRACE=full`) |
| What is the native window drawing? | `/tmp/blueice-dev/snapshots/native-window.png` (`BLUEICE_FRONTEND_SNAPSHOT`), rewritten when the picture changes: tab strip, the F9 panel, the permission panel — things the page frame does not contain |
| What does the page look like to an agent? | MCP `get_page_representation`, `get_dom`, `screenshot` (the page content, not the window chrome) |
| Tabs, downloads, translation, settings | the other MCP tools (`list_tabs`, `get_assistant_settings`, `summarize_page`, …) |

## Driving the settings flow without a person

`scripts/dev-control.py propose-nice 12` sends a proposal exactly as an agent
would (over the control socket, or the MCP `propose_assistant_settings` tool).
It then waits for a person to press **F9** in the window and approve it; the
trace shows `proposal.accepted` → `trusted.request: approve_assistant_proposal`
→ `proposal.approved` → `assistant.reconfigure`. A proposal expires after ten
minutes.

## Known environment issue: WSLg

WSLg's Wayland connection drops intermittently, which ends the window with
`Io error: Broken pipe` (exit code 3, with a hint printed). On WSL the script
therefore defaults to the X11 backend
(`WAYLAND_DISPLAY= WINIT_UNIX_BACKEND=x11`); `BLUEICE_DEV_WAYLAND=1` opts back in.

## Security boundary

The MCP server reaches the launcher only through the operator-control socket,
whose protocol has read (`Status`, inspect settings, proposal status), propose,
and `Cutover` requests — and no approve/deny/edit. Those live on a private pipe
between the launcher and its own window, which neither the MCP server nor the
trace exposes. The trace and status carry no secrets by construction (see
`backend/launcher/src/trace.rs` and its tests).
