# Phase 7 — Local AI & Built-in AI Interface

[← Back to plan](../BROWSER_CORE_PLAN.md)

**Status**: Not started

## Objective

Ship a lightweight, on-device AI runtime built into BlueIce, plus an interface for it. This is distinct from Phase 1/5's AI-facing API: that API lets an *external* agent (Claude, another MCP client) drive BlueIce from outside over IPC. This phase is about BlueIce having its *own* embedded AI capability, usable with no external agent connected at all.

## Design sketch

**Runtime candidates, in more depth** (final pick still blocked on scope, task 1 below, but the tradeoffs are concrete now):

| Runtime | Pros | Cons |
|---|---|---|
| [`candle`](https://github.com/huggingface/candle) (Hugging Face's Rust ML framework) | Pure Rust — no C++ toolchain dependency, fits this codebase's all-Rust posture and avoids cross-compilation pain across the eventual multi-platform frontend matrix (plan §1); supports GGUF-quantized small models (Llama/Mistral/Phi-class) via `candle-transformers` | Smaller/less battle-tested model zoo and kernel optimization than llama.cpp |
| `llama.cpp` (via Rust bindings, e.g. `llama-cpp-2`) | Most mature quantization/perf story, largest GGUF model ecosystem | C++ dependency — build complexity (cmake) and another toolchain to keep working across every target platform |
| ONNX Runtime (via `ort`) | Strong tooling, good fit for non-generative models (embeddings, classifiers) | Less natural fit for autoregressive text generation than llama.cpp/candle's purpose-built inference loops |

Leaning `candle` as the default candidate given the project's existing all-Rust posture (CONTRIBUTING.md, plan §1's process architecture) — not a final decision, just the sketch's working assumption.

**Process design**: `backend/ai-local/` as its own isolated OS process (same isolation rationale as `extension` — inference workloads are resource-heavy and their stability under arbitrary input is unproven), speaking the same IPC family `core` already exposes rather than a bespoke channel. Once scope (task 1) is fixed, this becomes concrete request/response messages, e.g. `Summarize { text }` / `Complete { prompt }`.

**Resource governance**: since `ai-local` is a separate OS process from `core`, plan §1's "core must not crash" guarantee already holds structurally — but a runaway local-inference process could still starve the host machine's shared resources (RAM/CPU) and degrade `core` indirectly. Worth enforcing an OS-level resource ceiling on the `ai-local` process (cgroups on Linux, Job Objects on Windows, similar on macOS) once this phase is built, not just relying on process isolation alone.

## Open questions (blocking real design)

- **What is the local AI actually for?** Candidates: page summarization, autocomplete/form-fill assistance, a local fallback agent when no external AI is configured, content extraction. Nothing downstream (model choice, resource budget, interface shape) can be decided until this is scoped — this is the one thing this phase can't start without.
- **Model/runtime choice**, once scope is known: a pure-Rust ML runtime (e.g. `candle`, fits an all-Rust codebase and avoids a C++ toolchain dependency) vs. `llama.cpp`/GGUF bindings (more mature, larger model zoo, but a C++ dependency) vs. ONNX Runtime. "Lightweight" as stated rules out anything requiring a large local model download or GPU dependency by default.
- **Process placement**: model inference is resource-heavy and its stability under arbitrary input is a real question — plausibly should be isolated the same way `extension` is (plan §1), rather than running inside `core`, for the same "don't destabilize the one process that must not crash" reasoning.
- **Relationship to the Phase 1/5 AI-facing API**: does the local AI consume the *same* IPC surface an external agent would (dogfooding it, and keeping BlueIce's own AI on equal footing rather than a privileged in-process shortcut), or does it need something the external-facing API doesn't expose?

## Checklist

- [ ] Scope what the local AI is actually for (blocks everything else in this phase)
- [ ] Choose the model/runtime given that scope
- [ ] Decide process placement (isolated like `extension`, or in-process)
- [ ] Decide whether the local AI is a client of the Phase 1/5 IPC surface or needs a separate interface, and why
- [ ] Define the resource budget (memory/CPU ceiling) given plan §1's low-memory requirement applies to the whole browser, not just `core`
