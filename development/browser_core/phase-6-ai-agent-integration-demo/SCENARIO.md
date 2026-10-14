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
while the highlight is active, and matching frame-source/tab/generation identities.
Start the frontend with `--show-generation` so its native lower-right badge
shows `SRC:<16-hex-digits> TAB:<id> GEN:<generation>` for the selected core frame. The MCP
`screenshot` tool reports the exact cached frame source/tab/generation used for its PNG;
the agent transcript records that metadata alongside each saved PNG path as
`evidence_saved`. The runner also records `highlight_frame` and automatically
rejects a second PNG whose frame source, tab, or generation differs from the highlight
snapshot, before saving it as evidence. Compare the human screenshot's badge
with the matching second PNG and `highlight_frame` transcript entries. A
mismatch is evidence of different frames, not a successful common-observer
proof.

## Live-model runbook

This is intentionally an operator-run proof, not a replacement by a scripted
client. Build the three binaries, serve the project-owned fixture only on
loopback, and choose a fresh scratch directory for the run artifacts:

```sh
cargo build -p blueice-launcher -p blueice-frontend-reference -p blueice-mcp-server
RUN_DIR="$(mktemp -d)"
SOCKET="$RUN_DIR/launcher.sock"
python3 -m http.server 4312 --bind 127.0.0.1 --directory development/browser_core/phase-6-ai-agent-integration-demo/demo-site
```

In a second terminal, start the shared core. In a third, start the human
frontend against that exact socket; it must remain visible through the
post-highlight pause.

```sh
target/debug/blueice-launcher --socket "$SOCKET"
target/debug/blueice-frontend-reference --socket "$SOCKET" --show-generation --url http://127.0.0.1:4312/index.html
```

Finally, in a fourth terminal, select one local model backend that supports
vision and tool calling. The default is a locally installed Ollama model at
`http://127.0.0.1:11434/v1/`. Its endpoint is loopback-only and requires no API
key; never use a remote model endpoint for this local evidence run.

```sh
target/debug/blueice-phase6-agent \
  --model "<local-ollama-model>" \
  --demo-url http://127.0.0.1:4312/index.html \
  --launcher-socket "$SOCKET" \
  --transcript "$RUN_DIR/agent.jsonl" \
  --evidence-dir "$RUN_DIR/evidence"
```

Advanced operators can instead run their chosen Hugging Face TGI model locally
(including their selected model, quantization, adapter, and accelerator
settings) and point the runner at its loopback-compatible endpoint. TGI's
`/v1/chat/completions` endpoint supports the required Messages API; no Hugging
Face cloud account or token is used by this runner.

```sh
target/debug/blueice-phase6-agent \
  --provider huggingface \
  --huggingface-base http://127.0.0.1:8080/v1/ \
  --model "<local-tgi-model>" \
  --demo-url http://127.0.0.1:4312/index.html \
  --launcher-socket "$SOCKET" \
  --transcript "$RUN_DIR/agent.jsonl" \
  --evidence-dir "$RUN_DIR/evidence"
```

A locally operated [llama.cpp server](https://github.com/ggml-org/llama.cpp/blob/master/tools/server/README.md)
is a third supported choice, useful for a GGUF vision-and-tool-capable model.
Supply the matching multimodal projector for screenshot input and bind only to
loopback; `--model` must match the server's `--alias`. Start the server and
agent commands below in separate terminals:

```sh
llama-server --host 127.0.0.1 --port 8080 --no-webui \
  --model "<local-model.gguf>" --mmproj "<matching-mmproj.gguf>" \
  --alias "<local-model-name>" --ctx-size 16384 --parallel 1
target/debug/blueice-phase6-agent \
  --provider llamacpp \
  --llamacpp-base http://127.0.0.1:8080/v1/ \
  --model "<local-model-name>" \
  --demo-url http://127.0.0.1:4312/index.html \
  --launcher-socket "$SOCKET" \
  --transcript "$RUN_DIR/agent.jsonl" \
  --evidence-dir "$RUN_DIR/evidence"
```

This is a separate provider label in the transcript, not a claim that a
llama.cpp run also exercised Ollama or Hugging Face TGI. All three client
choices retain the same credential-free loopback and first-party URL bounds.

The runner refuses a non-loopback start URL, a missing launcher, model-supplied
function arguments, a missing highlight-time screenshot, an incomplete
scenario, or a missing final report. It holds the shared highlight for ten
seconds by default (`--highlight-hold-seconds 0` disables that pause only when
no human capture is required). Preserve the JSONL transcript, retained PNGs,
and a human-window screenshot taken during that hold. The transcript's
`evidence_saved` entries identify each MCP PNG by path, frame source, tab ID, and exact core
frame generation. Record the human badge, `highlight_frame` snapshot identity,
and second PNG's metadata in the final result note; they must match for the
same-frame claim.
