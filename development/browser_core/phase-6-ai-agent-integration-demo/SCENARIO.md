# Phase 6 local agent demo scenario

The Phase 6 demo uses only the first-party static pages in `demo-site/`,
served on a loopback-only HTTP listener. It never visits a third-party site,
does not send credentials, and its allowlist is the one ephemeral
`http://127.0.0.1:<port>/` origin chosen for that run.

The LLM agent's task is deliberately small and objectively checkable:

1. Navigate to `index.html` using only BlueIce's MCP/Phase 5 tools.
2. From the representation and a screenshot, report the heading, the bold and
   italic labels, the two list items, the bordered light-grey form box, and the
   labelled `Name` text box.
3. Set the `Name` text box to `BlueIce` by its stable representation node ID,
   then use the post-action representation to confirm that value. The human
   window must show `BlueIce` inside the same core-rendered input box.
4. Highlight the `Name` text box, so the human window receives the same
   core-produced highlight frame, then confirm its role and label through the
   post-action representation.
5. Click `Continue to confirmation` by its stable representation node ID and
   confirm the resulting heading is `Task complete`.

For the supported native text-input slice, `SetValue` mutates the core-owned
`value` attribute, relayouts, paints that value inside the form control, and
returns it in `AiSnapshot::NodeState`. The agent must use the post-action
representation rather than assume its write succeeded.

For the common-observer proof, run `blueice-launcher` normally, start the
reference frontend with `--launcher --url http://127.0.0.1:<port>/index.html`,
and let `blueice-mcp-server` attach to the same default rendezvous socket. The
frontend's shared mode never sends `Shutdown` or removes that socket on exit.
The final evidence must retain the agent's MCP transcript, the frontend view
while the highlight is active, and matching frame/snapshot generation numbers.
