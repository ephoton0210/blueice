# BlueJS / BlueTS MCP Debug Environment

[← Phase 12 MCP plan](PLAN.md)

**Status**: Proposed interface contract. No current MCP tool exposes BlueJS execution, TypeScript compilation, a page-script target, a breakpoint, a paused frame, a type, a runtime contract, or a debugger event stream.

## Objective

Give an AI MCP client a complete, target-aware environment for inspecting, diagnosing, controlling and validating BlueJS and BlueTS programs. “Complete” means that every debugger capability BlueIce advertises—source, compilation, execution, pause/step, scopes, values, type diagnostics, contracts, events and resource data—has a versioned MCP representation or a deliberate, discoverable `Unsupported` result. It does **not** mean that MCP grows a second interpreter, compiler, debugger, page state, or policy bypass.

The environment covers three target kinds:

| Target kind | Owner | Typical use |
| --- | --- | --- |
| `page-realm` | `core` plus the live BlueJS host | Debug a named page's classic/module realm in its actual browser context. |
| `scratch-realm` | BlueJS host | Safely parse, analyze and execute an isolated JS experiment with no DOM/network authority. |
| `bluets-project` | BlueTS/BlueTSC front end | Check, emit, inspect diagnostics, types, contracts and lowered output for an operator-authorized TypeScript project. |

MCP is an adapter over the canonical native debugger and compiler interfaces defined by Phases 13, 17 and 18. It never opens a private BlueJS socket, directly reads a VM heap, spawns a second page or shell-executes `tsc`/`bluetsc` on an arbitrary client-supplied path.

## Architecture and identity

```text
AI MCP client
   │ tools / resources / notifications
   ▼
blueice-mcp-server
   │ versioned automation + debugger + compiler IPC
   ▼
launcher ── core ── blueice_ipc::debugger ── BlueJS host ── BlueTS front end
                                      │                         │
                                      └──── same Page / realm ──┴── BlueTSC
```

`core` remains the owner of page, tab, browser-context, policy and lifecycle state. BlueJS remains the owner of bytecode, frames, heap handles and instruction-safe points. BlueTS remains the owner of TypeScript syntax, binding, types, diagnostics, lowering provenance and contract plans. The MCP server only translates the requests and serializes bounded results.

Every operation names a `DebugTargetId` and returns a target generation. A page target carries its `browser_context_id`, `tab_id`, realm kind and realm generation; a hibernation, navigation, reload or hot-swap makes old target/frame/value handles stale. A project target carries an operator-registered project ID, compiler/configuration fingerprint and build generation. MCP never infers a “current tab” or “current frame”.

`debug_attach` returns a short-lived `DebugSessionId`, selected target generation and the negotiated capability document. A session is scoped to one authenticated MCP connection and is invalidated on detach, realm loss, controller-lease loss or policy change. All object, frame, scope, type and cursor handles include that session/generation and fail closed as `StaleHandle` rather than referring to a similarly named successor.

## Capability negotiation and common result rules

`debug_capabilities(target_id)` returns a pinned `debug_protocol_version`, target metadata, implementation state and resource limits. Each named capability reports `available`, `planned`, or `unsupported` with an error code; an AI client must check it instead of assuming a tool exists because another browser implements something similar. This permits a complete interface contract while BlueJS, BlueTS and Phase 17 arrive incrementally.

Every result includes the target/session generation, request ID and, where relevant, an ordered event sequence. Large source, AST, bytecode, diagnostic, property, trace and heap results use a `cursor`, a declared byte/item limit and `truncated`/`next_cursor` markers. When the MCP client supports resources, immutable artifacts also have resource URIs; tools return the resource URI and a compact summary rather than duplicating an unbounded payload. Tool fallback remains required for MCP clients that do not consume resources.

Page source, AST strings, error messages, console output, object property names/values, URLs, TypeScript diagnostics from remote code and contract-failure samples are all untrusted page/project data. The MCP adapter applies the same untrusted-content envelope as Phase 12's existing DOM tools and never represents such data as AI instructions.

## MCP capability surface

### Discovery, attachment and artifacts

| Tool or resource | Contract |
| --- | --- |
| `debug_list_targets` | Lists visible page, scratch and authorized project targets with language, generation, residency, policy and capability summary. Hibernated/Dormant pages report no live realm. |
| `debug_capabilities` | Returns the versioned feature/error/limit matrix for one target. |
| `debug_attach` / `debug_detach` | Creates or releases a generation-bound debug session. Attach requests the minimum capability set needed; elevation is separate. |
| `debug_list_scripts` | Lists script/module IDs, language (`js`, `ts`, lowered JS), URL/inline provenance, content hash, source-map state, compilation status and diagnostic summary. |
| `debug_get_source` | Returns a bounded source segment or immutable source resource. It supports original TS, original JS, emitted BlueTSC JS and lowered BlueJS views where each exists. |
| `debug_get_artifact` | Returns a bounded AST, capability summary, BlueJS bytecode/disassembly, TS-to-lowered provenance map, source map, contract plan summary or BlueTSC output manifest. It never executes the artifact. |
| `debug_get_diagnostics` | Returns categorized parse/bind/type/resolve/lower/contract/build diagnostics with source spans, related locations and configuration fingerprint. |

The existing `bluejs_analyze(code)` remains the compact scratch-target shortcut: it creates no page target, parses/analyzes only, and returns the same bounded AST/capability/diagnostic schema as `debug_get_artifact`. It cannot obtain DOM/network authority by choosing a page-like source string.

### Breakpoints, pause and execution control

| Tool | Contract |
| --- | --- |
| `debug_set_breakpoint` / `debug_remove_breakpoint` / `debug_list_breakpoints` | Uses original source URL/line/column, script ID, or executable generated location. The reply resolves every request to concrete safe points or a structured unresolved reason. Conditions/hit counts/logpoints are explicit opt-in capabilities. |
| `debug_pause`, `debug_resume`, `debug_step_into`, `debug_step_over`, `debug_step_out` | Controls only the attached target realm. A page pause never freezes another tab, the UI/render transport or unrelated automation clients. |
| `debug_wait_for_pause` | Waits with a bounded deadline for a breakpoint, exception, event-listener, contract-failure or explicit pause reason; returns the paused snapshot or timeout, never a guessed state. |
| `debug_get_pause_state` | Returns frames, async causal parent, lexical/global scope handles, exception/contract-failure metadata and the target generation. |
| `debug_set_exception_policy` | Selects caught/uncaught exception pause behavior from the native debugger's documented subset. |

All pause/control actions call the same Phase 17 native debugger operations used by the first-party DevTools, CDP and WebDriver/BiDi adapters. MCP cannot manufacture a pause state from a DOM snapshot or alter a bytecode offset that is not a proven instruction safe point.

### Values, evaluation and BlueJS scratch execution

| Tool | Contract |
| --- | --- |
| `debug_get_scope` | Pages a frame/scope's bindings with declared name, availability/TDZ state, static type where present, and a safe serialized runtime preview. |
| `debug_get_properties` | Pages own properties of a remote object handle without invoking getters, proxies or arbitrary user code. Prototype traversal and accessor evaluation are separate explicit options. |
| `debug_evaluate` | Evaluates code in a named paused frame or scratch realm with mode `preview`, `expression`, or `statement`. It reports completion, exception, fuel/time/heap use, console events and remote handles. `preview` succeeds only when the engine can prove it will not execute user code or mutate state; otherwise it returns `PreviewUnavailable`. |
| `bluejs_run(code)` | Convenience wrapper for an isolated `scratch-realm` statement evaluation. It has minimal documented host bindings, no live page DOM, no direct socket and no implicit authority escalation. |
| `debug_release_handles` | Eagerly releases remote value/frame/scope handles; all handles also expire on resume/detach/generation change. |

`expression`/`statement` evaluation is potentially side-effecting. It is controller-lease protected, operator-audited, fuel/time/heap bounded and visible to every observer. A result serializer preserves primitive values, bounded structural previews and opaque remote handles; it never walks arbitrary graphs indefinitely or calls user getters merely to format output.

### BlueTS types, contracts and BlueTSC builds

| Tool | Contract |
| --- | --- |
| `debug_get_type` | Looks up a source span, symbol, expression or runtime binding and returns its declared/inferred `TypeId`, display form, generic substitutions, declaration locations and reifiability. A static type is always labelled static, never presented as a runtime proof. |
| `debug_get_symbol` | Returns declaration/reference/rename/scope information from `BlueTsDebugInfo`, subject to source privacy policy. |
| `debug_get_contract` | Returns a contract ID's boundary site, supported structural shape, policy, validation limits and a redacted last-failure summary. It never exposes an unrestricted validator implementation as executable code. |
| `debug_validate_contract` | Validates a bounded JSON-like value against a selected contract in an isolated validator context, returning success or a bounded failure path. It cannot invoke page getters or acquire page capabilities. |
| `bluetsc_check` | Checks an operator-authorized project using the shared BlueTS front end and returns diagnostics, dependency/cache state and output eligibility without writing files. |
| `bluetsc_build` | Runs the same checked/lowered BlueTSC build contract: explicit registered project/output root, `noEmitOnError`, atomic publication, emitted-artifact manifest and source-map/contract-policy fingerprint. It is not a shell command endpoint. |

For direct TypeScript pages, `debug_evaluate` parses and type-checks the expression in the paused source scope before BlueJS compilation. The result distinguishes a compile/type diagnostic from a runtime exception. BlueTSC and direct BlueTS must report the same supported diagnostics, lowering provenance and contract semantics for the same configuration.

### Events, tracing and resource investigation

`debug_subscribe` creates a scoped event stream; the server uses MCP notifications where available and `debug_next_events(subscription_id, cursor)` as the required polling fallback. Event types include `script_compiled`, `script_failed`, `diagnostic_changed`, `breakpoint_resolved`, `paused`, `resumed`, `exception`, `console`, `contract_validation_failed`, `cache_invalidated`, `build_finished`, `gc`, `resource_limit`, and lifecycle/realm-loss events. Events are sequence-numbered per target/context, carry causal request/script/frame IDs where known, and report `EventsDropped` rather than silently losing history under backpressure.

`debug_runtime_stats` exposes bounded interpreter/compile/cache/GC/contract counters attributed to the target/tab. `debug_capture_trace` and `debug_heap_snapshot` are negotiated advanced capabilities: they must obey Phase 8 memory accounting, use explicit byte/time limits, page their output and preserve object/source privacy. Until their native implementations exist they return `Unsupported`; the interface must not imply that a heap profiler is already available.

## Authorization, safety and lifecycle

The normal local MCP token authenticates the client but does not grant every debugger action. Native capability checks are projected into these minimum MCP scopes:

| Scope | Examples |
| --- | --- |
| `debug:inspect` | Targets, source, artifacts, diagnostics, static types, safe previews and subscriptions. |
| `debug:control` | Attach with pause control, breakpoints, pause/resume/step and exception policy. Requires the relevant controller lease. |
| `debug:evaluate` | Side-effecting frame/scratch evaluation and accessor/prototype inspection. Requires `debug:control`, controller lease and audit event. |
| `debug:build` | BlueTSC check/build on an operator-registered project/output root. Build/write elevation is explicit. |
| `debug:profile` | Trace/heap capture and resource-detail access. Requires an explicit retention budget. |

Read-only observers may attach concurrently only where the page/project policy permits source and value disclosure. Controller actions never interleave invisibly with a paused stack; a lease loss resumes/rejects according to the native debugger policy and invalidates its handles. Debugging a Hibernated/Dormant tab returns `TabNotResident` rather than reviving it or inventing stale scope data.

The debug path is not a policy bypass. Page-realm evaluation follows the realm's normal DOM/network/capability restrictions and Phase 7 review. Scratch realms have less authority, not more. BlueTSC project registration pins canonical project/config/output roots before a call; an AI-provided path, config inclusion, plugin, import-map or compiler option cannot expand those roots at call time. Debug values and diagnostics may contain secrets, so serializers redact configured credentials, request bodies and sensitive headers before an MCP result is created.

## Delivery and acceptance

1. **Native foundation** — Phase 17 implements target IDs, source/safe points, pause/step, frame/scope handles, debugger events and controller leases through `blueice_ipc::debugger`. Phase 13 exposes scratch parse/analyze/run through the same capability model.
2. **BlueJS MCP adapter** — Phase 12 adds target discovery, attachment, `bluejs_analyze`, `bluejs_run`, source/artifact inspection, breakpoints, pause/step, scope/property paging, bounded evaluation and event subscriptions. Validate from an actual MCP client against a live page and isolated scratch realm.
3. **BlueTS/BlueTSC adapter** — Phase 18 adds type/symbol/contract/provenance resources, typed paused evaluation, `bluetsc_check`/`bluetsc_build` and source-map fidelity. Validate direct TS and emitted JS against one fixture project.
4. **Advanced investigation** — Add trace/heap capabilities only after their native retention, privacy and memory contracts are implemented; update the negotiated capability matrix rather than changing old tool meanings.

Acceptance requires one real AI MCP client to: discover a page and scratch target; analyze and run an isolated JS snippet; set a source breakpoint; receive a pause event; inspect a scope without executing getters; evaluate a bounded expression under a controller lease; resume only that realm; inspect a TS diagnostic/static type/contract failure; run a registered BlueTSC check/build; and receive explicit `Unsupported` for an unavailable advanced feature. Tests must also prove that a stale handle, unauthorized controller action, arbitrary build path, oversized artifact, untrusted page string, Hibernated tab and lost event sequence fail safely and observably.

## Explicit non-goals

- A raw VM socket, arbitrary local shell, arbitrary project-directory, unrestricted filesystem or network endpoint for an AI client.
- A second debugger semantics different from Phase 17 native DevTools/CDP/WebDriver/BiDi behavior.
- Treating source/type diagnostics/contracts as trusted instructions, security policy, or proof that runtime data is safe.
- Universal CDP, Chrome DevTools frontend, `tsserver`, TypeScript language-service, heap profiler or live-edit compatibility claims before each capability is implemented and negotiated.
- Implicit object graph traversal, getter/proxy execution, permanent pause, unbounded event retention, secret disclosure or cross-tab/realm control.
