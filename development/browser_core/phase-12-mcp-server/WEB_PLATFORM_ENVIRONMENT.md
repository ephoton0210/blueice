# Web Platform, Graphics, Runtime, Shell, and PDF MCP Environment

[← Phase 12 MCP plan](PLAN.md)

**Status**: Proposed interface contract. It specifies the MCP adapter required for Phases 20–24; none of the described browser, graphics, worker, WebAssembly or PDF targets exists today.

## Objective

Let an AI MCP client inspect, diagnose, test and, where the same native policy allows a human/automation client to act, control general browser-platform capabilities. This includes browsing contexts and security policy; DOM/forms/events; layout/resources/Canvas/WebGL/WebGPU/media; browser shell and accessibility; storage/workers/Service Workers/WebAssembly; and PDF Viewer documents. Every operation is an adapter over the one native owner and reports `Unsupported` until implemented; MCP must not become a second browser, graphics driver, permission manager, database, Wasm runtime or PDF parser.

## Common target and lifecycle contract

All tools use opaque generation-bound identities, never a guessed current tab or global process handle.

| Target | Native owner | Examples |
| --- | --- | --- |
| `PlatformTargetId` | Phase 20 `core` | Browser context, top-level/child browsing context, document/realm/lifecycle/security policy. |
| `RenderTargetId` | Phase 21 core/compositor | Style/layout/layer tree, resource, canvas, graphics context or media element. |
| `ShellTargetId` | Phase 22 frontend/launcher | Registered window, viewport, visible permission prompt, native accessibility bridge. |
| `StorageTargetId` | Phase 23 core | One origin/profile/partitioned storage account. |
| `WorkerTargetId` / `WasmTargetId` | Phase 23 worker/Wasm host | One worker realm/message stream or one compiled/instantiated Wasm module. |
| `PdfDocumentTargetId` | Phase 24 PDF sandbox/core | One PDF tab/document and its PDF-native document model, page/tile/text/outline/structure/form/annotation state. |

Navigation, reload, bfcache restore, context loss, worker restart, storage clear, renderer crash, document replacement or PDF close invalidates dependent handles. Every result carries target generation, policy/capability version, request ID, cursors/byte limits and an explicit redaction/truncation result. Source text, DOM values, headers, shader logs, media metadata, stored values, Wasm names, PDF text/annotations and accessibility labels are untrusted content, not instructions to the AI.

`web_capabilities(target_id)` returns the implementation state, protocol version, supported standards/profile/version/extensions, permitted MCP scopes, privacy policy and resource limits. The same native capability report is visible in DevTools and human browser UI; AI clients cannot infer support merely because a similarly named Chrome API exists.

### PDF-native document model

`PdfDocumentTargetId` exposes a generation-bound, opaque-handle document model so an AI can understand a PDF as more than rendered pixels or a flat text-search result. Its root has page, outline, tagged-structure, text-range, link, form-widget and annotation children; pages and nodes supply bounded geometry and relations. Tagged PDFs retain declared roles, order, language and alternative text. An untagged document instead exposes a clearly labelled geometric/text fallback whose blocks, lines and spans carry extraction/layout provenance; OCR-derived content is marked separately. This model is not the Web DOM: it has no `window`, page realm, CSS selector API or JavaScript evaluation, and it does not expose raw objects, streams, xref IDs or parser handles.

## Tool families

| Family | Read/diagnostic surface | Controlled actions and hard boundary |
| --- | --- | --- |
| `platform_*` | Context/frame/document/history/lifecycle, origin/CSP/CORS/sandbox/permission/referrer/SRI state, DOM/event/default-action/form/module traces and capability reports. | Navigation, permitted input, form submission, popup/message actions require controller lease and Phase 20 policy. No cross-origin DOM read, raw cookie access, permission grant or file-picker bypass. |
| `render_*` | Computed style, layout bounds, layer/paint provenance, resource/font/image/SVG/MathML status, animation/scroll timeline and bounded screenshot. | Overlay/inspection controls only. It cannot rewrite CSS/DOM or read protected pixels except through `debug_*` in an authorized page realm. |
| `graphics_*` | Canvas/WebGL/WebGPU context state, feature/extension/limit report, shader compilation/link diagnostics, context-loss events, bounded framebuffer/timing preview. | Context pause/capture is controller- and budget-gated. No raw GPU device/command queue/memory access, unbounded readback or cross-origin-tainted pixels. |
| `media_*` | Media/caption/audio lifecycle, selected tracks, decoder errors, bounded frame/waveform preview and playback metrics. | Play/pause/seek/volume actions use normal page/media policy. Camera/mic/screen capture/device selection remain visible human permission flows. |
| `shell_*` | Registered shell/window/tab/profile/viewport/zoom/theme/download/print/prompt state and bounded input/selection trace. | Window/tab actions require controller lease; print/download/file/permission actions use normal human-visible shell review. No OS-window, clipboard, file or password bypass. |
| `accessibility_*` | Stable accessible tree, role/name/state/value/bounds/relations/text ranges, platform-bridge state and WCAG capability diagnostics. | Standard accessibility actions travel through normal input/default-action/permission policy; protected values stay redacted. |
| `storage_*` | Partition/quota/usage/transaction/cache manifest/service-worker registration metadata and redacted diagnostics. | Clear/evict requires explicit storage scope and controller policy. No raw DB/filesystem path, third-party partition bypass or protected-value export. |
| `worker_*` | Worker lifecycle, module/origin/capability metadata, message/structured-clone trace, service-worker route and crash/termination state. | Start/message/terminate/debug needs owner policy and controller lease. No arbitrary worker URL, host import or other-origin attachment. |
| `crypto_*` | Web Crypto algorithm/key-handle/usages/extractability metadata, operation lifecycle and bounded/redacted error diagnostics. | No key export, entropy/TLS-key access or AI signing/decryption oracle; page operations remain ordinary origin/policy-bound script work. |
| `wasm_*` | Validation/compile/import/export/disassembly/source-map, safe stack/trap, feature matrix, bounded memory preview and canonical debugger correlation. | Breakpoint/pause/step/evaluate/memory write delegates to the native debugger, requires lease/fuel/privacy budgets, and cannot inject imports or acquire native authority. |
| `pdf_*` | PDF metadata/security/signature/permission; document/page roots; cursor-based structure-tree traversal; node role/relations/provenance/geometry; bounded text ranges; outline/link/accessibility/form/annotation/search state; tiles and viewer/model events. | Link follows normal navigation policy. Form fill/annotation/save/print/download require write scope plus human-visible confirmation/review; no HTML-DOM selector/eval, raw parser/object/stream, password, attachment or embedded-JavaScript execution endpoint. |

Each family supplies `list_targets`, `attach`/`detach`, `get_*`, `subscribe` and cursor-based `next_events` forms appropriate to the native object. All mutating tool replies include the resulting target/document/frame generation and causal native event ID. `debug_*` remains the canonical BlueJS/BlueTS/Wasm pause/stack/scope interface; these families link to a `DebugTargetId` instead of copying VM semantics.

## Authorization and privacy

Minimum scopes are `platform:inspect`, `platform:control`, `render:inspect`, `graphics:inspect`, `graphics:capture`, `media:inspect`, `shell:inspect`, `shell:control`, `accessibility:inspect`, `storage:inspect`, `storage:control`, `worker:inspect`, `worker:control`, `crypto:inspect`, `wasm:inspect`, `wasm:control`, `pdf:inspect` and `pdf:write`. More powerful actions additionally need a native controller lease, target policy, Phase 7 gatekeeper clearance and, where the human browser normally requires it, visible human confirmation. A local MCP token authenticates; it grants none of these scopes by itself.

Permission grants, credentials, passwords, clipboard/file values, camera/microphone/screen content, cross-origin protected pixels, cookies, protected storage, private PDF text/attachments, raw GPU memory and direct process/socket handles are never ordinary MCP data. Request, resource, parser, tile, decoder, compilation, GPU, worker, storage and PDF work is charged to its native account with time/byte/object/memory limits. Resource exhaustion, stale handles, hibernated tabs, context loss, crashes, denied policy and dropped events are explicit results.

## Events and cross-layer debugging

`web_subscribe` creates a capability-filtered stream with MCP notifications and required polling fallback. Events include navigation/document/lifecycle/policy/permission; DOM/form/event/default action; style/layout/layer/resource/canvas/graphics/media; shell/input/accessibility/download/print; storage/worker/service-worker/Wasm; and PDF model/render/search/form/annotation states. They are sequence-numbered in their browser-context/owner stream and carry causal context/document/request/worker/module/PDF/frame IDs wherever possible.

An AI can therefore trace, for example, a click through trusted input, DOM default action, form body, Fetch request, service-worker route, Wasm callback, WebGL render, accessibility update and presented frame; or trace a PDF link/form action through the same navigation/permission/download policy. It cannot fabricate the trace by calling an MCP-only shortcut.

## Delivery and acceptance

1. Each native phase first defines target IDs, native events, capabilities, scopes, redaction and resource errors.
2. Add read-only discovery/inspection/resources/subscriptions once its native owner is real; validate a real MCP client against deterministic fixtures.
3. Add only native-equivalent controlled actions after controller lease, policy/gatekeeper and human-review paths exist.
4. Cross-check DevTools, human shell and MCP on the same IDs/generations for contexts/frames/resources/workers/Wasm/PDF.

Acceptance requires one real MCP client to inspect and safely fail/control each available phase capability: sandbox/CSP frame, DOM/form/live-transport lifecycle, CSS/layer and Canvas/WebGL context, media error, IME/accessible action/permission prompt, partitioned storage/worker/service-worker/Web Crypto, Wasm trap/breakpoint, and PDF model traversal/search/form/blocked-action. PDF fixtures must cover a tagged tree correlated with page geometry and accessibility, plus an untagged/OCR fallback whose provenance is observable. Tests must prove that every unauthorized, stale, over-budget, cross-origin, privacy-protected or unsupported operation returns an observable native error without creating a second owner or bypassing a human decision.
