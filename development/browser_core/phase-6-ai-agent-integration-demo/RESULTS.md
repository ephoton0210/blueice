# Phase 6 live-model results — 2026-09-24

Status: the **real local-model browser task passed**; the separate
**human-window same-frame capture remains open**. This result is not a claim
that Ollama or Hugging Face TGI themselves were run, nor that the whole phase
is complete.

## Runtime and scope

- First-party `demo-site/` was served only at `127.0.0.1:4312`. The shared
  `blueice-launcher` supervised its private gatekeeper and core; the agent
  accessed that core solely through the standard stdio MCP server.
- The model was the locally cached `Qwen_Qwen3.5-4B-Q4_K_M.gguf` from
  [bartowski's Qwen3.5-4B GGUF release](https://huggingface.co/bartowski/Qwen_Qwen3.5-4B-GGUF/tree/main),
  with its matching `mmproj-Qwen_Qwen3.5-4B-f16.gguf` vision projector
  (SHA-256 `659b59dd44b73b1cd34af6cc424669484b06dc80f4340adf8ea84ad776eef813`).
- A temporary [llama.cpp](https://github.com/ggml-org/llama.cpp) build at
  commit `bd4f514db14d87fded667787a7a963bfbaa98e89` served the model
  through `http://127.0.0.1:18080/v1/` on Apple Metal, with a 16,384-token
  context, one slot, and alias `qwen3.5-4b-local`. It did not receive an API
  key or expose a non-loopback listener. Its Chat Completions response to a
  small probe contained a genuine `tool_calls` array; a multimodal projector
  loaded before the demo.

The initial two successful runs used the existing generic compatible client
transport under `--provider huggingface`; their `huggingface-local` transcript
label did **not** identify the actual runtime. That surprise led to the
dedicated `--provider llamacpp` option. The third successful run below used
that option and records `llamacpp-local` accurately.

## End-to-end outcome

The third run used `--provider llamacpp --llamacpp-base
http://127.0.0.1:18080/v1/ --model qwen3.5-4b-local`, the one shared
launcher socket, and `--highlight-hold-seconds 0`. The runner exited `0`
after these model-selected scenario actions, in order:

1. `navigate_demo` — the loopback demo server received the page GET.
2. `inspect_page` and `take_screenshot` — the first MCP PNG represented
   `tab_id=1`, `generation=1`.
3. `set_name_to_blueice` — the runner resolved the live labelled text box and
   confirmed `BlueIce` in the post-action representation.
4. `highlight_name` and `take_screenshot` — the highlighted core snapshot and
   second MCP PNG both reported `tab_id=1`, `generation=3`.
5. `continue_to_confirmation` — the final representation reported the
   `Task complete` heading before the model's final report was accepted.

Visual inspection of the retained MCP PNGs confirmed the heading, bold and
italic labels, two list items, grey bordered form container, `Name` control,
`BlueIce` value, and orange highlight. With the reference frontend attached,
the screenshots were `1600 × 1166` pixels; the first run without it produced
`800 × 600` pixels, reflecting the shared core's viewport change.

The local, uncommitted evidence is under
`/private/tmp/blueice-phase6-euHGbB/` on the test machine. For the third
run, its JSONL transcript is `real-qwen-run-3.jsonl` (SHA-256
`d295213b6ca09aa555c4ef91c91741c0bbaf7859c233d9002ebf55020d86ad20`);
the first and highlighted PNGs are the two files in `real-qwen-evidence-3/`
(SHA-256 `27db491a4a11ed56a504e643a6da42c0efb4d4ede718f3b000ce23af242d8ecd`
and `ecf2ec5b281a70d2f097f6df026f416b5ae1be88d796d59673b92c25c1f67c09`,
respectively). These temporary binary artifacts are not in Git; the transcript
hashes and the reproducible runbook are recorded here without claiming that a
future checkout contains those files.

The second real-model run also passed with the reference frontend started
using the same launcher socket and `--show-generation`. It recorded a
`highlight_hold` of 90 seconds, then a second MCP screenshot matching
`TAB:1 / GEN:3`. The OS window list identified a visible BlueIce frontend
window, but macOS refused a capture restricted to that window
(`screencapture`: `could not create image from window`). We deliberately did
not capture the entire desktop as a workaround. No independently supplied
human-window screenshot or observation is therefore available. The existing
compiled-stack test's independent launcher client did receive the matching
`FrameReady`, but it is not a human-window substitute.

## Follow-up

- Obtain a human screenshot of the actual BlueIce frontend during a fresh
  highlighted-frame hold, including its `TAB`/`GEN` badge. Compare its badge
  to that run's `highlight_frame` and second `evidence_saved` JSONL entries.
- Use [`run-human-evidence.sh`](run-human-evidence.sh) for the next joint run.
  It requires a running credential-free loopback Ollama/TGI/llama.cpp model,
  pauses until the person sees the independent BlueIce window, holds the
  highlighted frame for 90 seconds, and prints the exact `SRC`/`TAB`/`GEN`
  identity to compare with a window-only PNG. Its 2026-09-25 no-model
  startup/cleanup preflight passed; that is not human observation evidence.
- The documented Ollama and Hugging Face TGI transports retain scripted
  compiled-stack coverage; a genuine service run for either is separate from
  this llama.cpp result.
- No CDP, Puppeteer, remote origin, cloud endpoint, or model-supplied arbitrary
  browser URL was used in these runs.
