# Phase 17 — Automation, DevTools, and Dynamic Web Networking

[← Back to plan](../BROWSER_CORE_PLAN.md)

**Status**: Design started — added at the user's request. BlueIce currently has multi-tab IPC/MCP control and a Puppeteer-driven *Chromium differential-test harness*, but it has no DevTools endpoint, page debugger, Selenium/WebDriver server, Playwright-compatible client surface, Postman-like API/SOAP client, `fetch`, `XMLHttpRequest`, or script-originated request path.

## Objective

Give BlueIce the capabilities users expect from Chrome DevTools, Playwright and Selenium, plus a Postman-like HTTP/SOAP workbench and a page JavaScript debugger, while preserving its central invariant: every human, AI and automation client observes and acts on the same `core`, `Page`, script realm and render pass. Add standards-based AJAX so pages can dynamically receive, decode and send data without full-document navigation.

This phase is feature-equivalence work, not a claim of immediate drop-in protocol compatibility with every version of those projects. The [research note](../research/automation-and-dynamic-networking.md) records the evidence and the compatibility boundary.

## Current-state correction

Puppeteer currently drives a real Chromium only inside Phase 15's differential-testing harness. It is not an API for driving BlueIce. Likewise, BlueIce's `navigate`, DOM snapshot, screenshot, click and multi-tab MCP tools are valuable foundations, but do not yet provide locator waiting, isolated browser contexts, automation event streams, breakpoints/stepping, network observation/interception, a manual HTTP/SOAP workspace or browser Web APIs for asynchronous data exchange.

## Decisions

### A single native automation service

Add a versioned `blueice_ipc::automation` protocol and a `core`-owned automation service. It is bidirectional and long-lived, like the existing script and extension protocols, but has a distinct vocabulary and version because it serves privileged browser-control clients rather than a page's JavaScript realm or an extension capability grant.

`blueice-automation` is a thin adapter process that connects through the Phase 8 launcher rendezvous socket; it does not own a second `core` or maintain a shadow DOM/network state. It exposes secure external transports and translates them to the internal protocol. A future native DevTools UI is another client of this service. This produces one source of truth:

```text
DevTools UI ─────┐
WebDriver/BiDi ──┼─> blueice-automation ─> Automation IPC ─> launcher ─> core
Playwright API ──┤                                        │             │
MCP adapter ─────┘                                        └── events ───┘
                                                               same Page / script realm / render pass
```

The service has the following semantic groups. Each command and event carries a protocol version, a caller request ID, `context_id`, `tab_id` where applicable, and a server-assigned sequence number for subscriptions.

- **Browser/context/page lifecycle**: create/close a browser context, open/close/navigate pages, viewport/visibility controls, current URL/title and screenshot.
- **Inspection**: full DOM, computed style, accessibility snapshot, layout bounds, screenshot and stable node identity. DevTools overlays/highlights must use the existing `NodeId` rather than a parallel selector-only identity.
- **Locators and waiting**: CSS, text and role/name locator resolution; `attached`, `visible`, `enabled`, `stable`, URL, network-idle and explicit event predicates. A resolved handle is scoped to its document generation and becomes *stale* after navigation or removal; it never silently retargets a similarly matching element.
- **Input and actions**: pointer, wheel, keyboard, focus, type, select and drag actions, with coordinate actions converted through the same layout/hit-test path as the reference frontend.
- **Script/runtime**: evaluate in a named page realm, structured serializable results/errors, console messages, exception events, breakpoints/pause/resume once BlueJS debugging hooks exist.
- **Network and tracing**: request lifecycle subscriptions, bounded metadata/body retention, request blocking/fulfilment/continuation, console/runtime events and trace spans.
- **API workspace**: explicitly user-authored HTTP/SOAP requests and saved collections, with a separate initiator/profile from a page and a fully auditable request lifecycle.

`TabManager` gains a `BrowserContextId -> TabId -> Page` ownership layer before external context APIs are exposed. The existing single default context preserves the current `tab_id` behavior. Cookie/cache/storage isolation is a required part of a usable context once those stores exist; until then, the implementation may expose only the default context rather than falsely claim incognito isolation.

### DevTools capability, not an imported Chrome frontend

Build a first-party, platform-neutral DevTools UI connected to the automation service. The initial panels are Elements (DOM/styles/layout/overlay), Console (BlueJS logs/errors), Network (request lifecycle and body preview), Accessibility and rendering diagnostics. Sources/debugger, performance timeline, storage and protocol-monitor panels follow once their backing data exists.

The DevTools UI uses the BlueIce automation protocol. A separately versioned, opt-in CDP compatibility endpoint can subsequently expose a supported subset of `Target`, `Page`, `DOM`, `Runtime`, `Debugger`, `Network`, `Input`, `Accessibility` and `Log`. Its discovery document lists exactly those domains/methods and the BlueIce protocol version. Unsupported methods return a structured `MethodNotFound`/`Unsupported` error; BlueIce must not advertise Chrome's moving tip-of-tree protocol or bundle Chrome's frontend as if it were our own.

### Page JavaScript debugger

Build a Sources-style debugger for page scripts, backed by BlueJS rather than simulated from console logs. BlueJS records `script_id`, URL/inline-script provenance, source text, line/column-to-bytecode locations and executable safe points as it compiles a page script. A dedicated full-duplex `blueice_ipc::debugger` channel connects `core` and the out-of-process BlueJS host; it is separate from `blueice_ipc::script`'s high-frequency DOM requests and from the public automation protocol. The internal debugger vocabulary then provides:

- line/column and URL breakpoints; resolved locations; enable/disable/remove; conditional and hit-count breakpoints after ordinary breakpoints are correct;
- pause-on-caught/uncaught exception, event-listener and Fetch/XHR URL breakpoints; logpoints follow as a non-pausing convenience feature;
- `paused`/`resumed`/`scriptParsed`/`scriptFailedToParse` events containing pause reason, call stack, lexical/global scope snapshots and async causal parent where available;
- resume, pause-next-statement, step into/over/out, and evaluate on a paused call frame; every frame/object handle is generation-bound and released when execution resumes;
- Sources UI: source tree/editor, breakpoint list, call stack, scopes, watches and console. Source maps, local-workspace file overrides, live-edit and heap profiling are later work.

Pausing freezes only the target tab's JavaScript realm. Its already-received network bodies may be buffered within quotas, but its callbacks and further script tasks do not execute until resume; other tabs, frame presentation and automation observers remain live. A non-debug automation mutation for the paused tab is rejected or explicitly queued, never interleaved invisibly with a paused stack. Debugger control and paused-frame evaluation require the controller lease, bounded fuel/time and an observer-visible audit event.

This supplies the native backing for the advertised CDP `Debugger`/`Runtime` subset; the adapter comes after breakpoint, step and scope semantics are proven through the native interface.

### Playwright-shaped workflow

Publish `@blueice/automation`, a TypeScript client library over the native endpoint. Its primary model intentionally matches the useful Playwright workflow:

```ts
const browser = await blueice.launch();
const context = await browser.newContext();
const page = await context.newPage();
await page.goto(url);
await page.getByRole("button", { name: "Save" }).click();
await page.waitForResponse("**/api/save");
```

The first release includes browser/context/page lifecycle, locators, automatic actionability waiting, navigation/waiting, input, screenshots, DOM/AX inspection, script evaluation, console/error events and request/response observation. Route interception, download artifacts, tracing/HAR and video follow in that order.

This is a source-compatible *shape* where practical, documented per method; it is not a promise that upstream Playwright's private browser-server protocol, all browser engines, test-runner fixtures or every selector extension works unchanged. A later Playwright Test adapter can run a supported test subset against `@blueice/automation` and report explicit unsupported capabilities rather than quietly routing tests to Chromium.

### Selenium interoperability: W3C WebDriver then BiDi

`blueice-automation` exposes a W3C WebDriver HTTP endpoint so standard Selenium language bindings can create a BlueIce session. The first conformance slice covers session lifecycle, navigation/history, window/viewport, element finding, element state/attributes/text, click/send-keys/clear, actions, screenshots, script execution and explicit errors for stale/no-such-element conditions.

The same session can advertise a WebDriver BiDi WebSocket URL. Implement `session`, `browser`, `browsingContext`, `input`, `script`, `log` and `network` incrementally, starting with subscriptions and request events before interception. BiDi is the eventful Selenium path; classic WebDriver remains for broad client compatibility. Neither endpoint may expose `core`'s private Unix IPC directly.

### API and SOAP workbench

Build a first-party API workspace with the focused manual-testing flow users expect from Postman: collections/folders, environments with variable interpolation, request history, request/response examples and generated `curl` snippets. Its request editor supports URL/method/query/headers, raw text/JSON/XML/binary bodies, URL-encoded and multipart forms, Basic/Bearer/API-key authentication, timeout/redirect policy and a distinct selected cookie jar. Secrets reside in the OS-backed secret store or an encrypted local store, are masked in UI/logs/traces by default and never appear in an AI/MCP result without an explicit privileged export.

SOAP is a first-class editor mode, not merely syntax highlighting. It provides SOAP 1.1 and 1.2 envelope templates, raw XML editing/formatting/validation, `text/xml` or `application/soap+xml` content-type helpers, optional `SOAPAction`, response-header/timing display, namespace-aware XML tree/text views and clear SOAP Fault rendering. Importing a WSDL 1.1 or 2.0 document creates reviewable operation/request templates; it never automatically executes an imported endpoint or recursively fetches unapproved remote WSDL/XSD dependencies. XML parsing has external entities and entity expansion disabled, and enforces depth, node and byte limits.

`ApiWorkspace` is a consciously privileged, operator-created client context, separate from a tab's browser context. It can send an arbitrary user-authored HTTP/SOAP request and therefore is **not** subject to page CORS, but it is not a generic unauthenticated remote automation escape hatch: send requires a controller lease plus `api:send` authorization, records the initiator and policy decision, and follows the same URL/TLS/proxy/resource/gatekeeper policy as other network requests. It never silently shares a page's cookies, credentials or CORS authority. It reuses `blueice-net`'s transport, decoding, cancellation and event model with `initiator = api-workspace`; there is no separate unsafe HTTP stack.

### Dynamic networking / AJAX

The browser-facing APIs are standard `fetch()` and `XMLHttpRequest`, implemented over one request service. Do not add a BlueIce-specific script `ajax()` API: sites need the standard contracts, while automation gets observation and test-control through the separate network event/routing capabilities above.

`blueice-net` is refactored from `fetch(url) -> FetchedPage` into an asynchronous request service whose public internal model is intentionally byte-oriented:

```text
RequestStart { url, method, headers, body, initiator, mode, credentials, redirect }
  -> ResponseHeaders { final_url, status, headers, response_type }
  -> BodyChunk { bytes, encoded_bytes, decoded_bytes }
  -> Complete | Failed | Aborted
```

`core` owns actual network I/O, redirects, cookies/cache as they arrive, policy checks and event emission. The out-of-process BlueJS host asks for start/abort/body-read operations through a dedicated `blueice_ipc::network` module; it never gets direct sockets or a way around the gatekeeper. A request is bound to its initiating document, origin and browser context.

Required semantics, delivered in stages rather than as a text-only GET shortcut:

- Request construction: relative URL resolution, method, headers, body bytes, URL-encoded forms, `FormData`/multipart when form upload lands, and streamed upload where the host runtime supports it. Forbidden headers, referrer/credentials mode and redirect behavior are enforced at the browser boundary.
- Receiving and decoding: incremental response bytes; HTTP transfer/content decoding delegated to a maintained HTTP/TLS stack; MIME/charset parsing with BOM/declared charset fallback; `text()`, `json()`, `arrayBuffer()` and binary response handling. JSON is parsed only on explicit `json()`/`responseType = "json"`, so invalid JSON becomes a normal observable parse error rather than corrupting transport state.
- Security: same-origin policy, CORS response filtering and preflight, credentials/cookie policy, mixed-content policy once secure-context support exists, size/time/resource limits, cancellation and redirects. A same-origin implementation that silently reads cross-origin bodies is not an acceptable first release.
- Fetch: Promise-returning `fetch(input, init)`, `Request`, `Response`, `Headers`, `AbortController` and a one-consumer body model; resolve the promise at headers, then expose body completion asynchronously.
- XHR: `open`, `setRequestHeader`, `send`, `abort`, timeout, `readyState`, status/response headers and `loadstart`/`progress`/`load`/`error`/`abort`/`timeout`/`loadend`. Support `""`/`text`, `json` and `arraybuffer` response types first; `document` response and synchronous XHR are explicitly out of scope.
- Live transports: Server-Sent Events is a follow-up sharing the streaming path; WebSocket/WebTransport are separate protocol work and are not blocked behind nor substituted for AJAX.

The existing navigation path uses this same request service with `destination = document`, but retains its own page-commit semantics. A Fetch/XHR response never replaces the document unless page JavaScript explicitly mutates it.

### Event ordering, resource budgets, and access control

One `AutomationEvent` stream carries navigation, DOM-document generation, render, console, exception and network events. It is ordered per browser context by a monotonic sequence number; event payloads contain stable IDs and causal parent IDs (`request_id`, initiator script/event) when known. Subscriptions declare metadata-only vs. body-preview access. Body capture is opt-in, redacted for sensitive headers by default, bounded by per-request and per-context byte limits, and emits an explicit `BodyEvicted`/`EventsDropped` marker on eviction or backpressure.

**Memory-mode interaction (Phase 8):** Performance mode adds no artificial automation/DevTools cap. Recommended minimum-memory mode creates screenshots, trace buffers, DevTools/API response previews, SOAP XML trees, and `waitFor*` retention with low-retention/streaming settings, but does not evict an inactive tab’s retained state before its longer timeout; timeout hibernation releases it. Extreme memory mode reserves these artifacts to the initiating tab (or the separate API-workspace account) before capture, gives active-tab work priority, and can reject optional capture or the allocating operation once the tab/fleet hard budget is exhausted; it considers an inactive tab’s artifacts only when room is needed. Event streams must state an eviction/resource result rather than silently losing observable state. The shared [Phase 8 policy](../phase-8-live-core-hotswap/PLAN.md#user-selectable-memory-modes-design-resolved-not-implemented) governs the limits and reclaim order.

Automation and DevTools can list a Hibernated/Dormant tab’s bounded session metadata, group, residency state and restore reason, but there is no DOM, screenshot, script realm, request history, or debugger target to inspect. A read that needs live page state returns `TabNotResident`; activation/reload is an explicit controller-authorized operation and emits the new runtime `TabId` mapping for a post-restart session key. This prevents a background catalog from quietly allocating memory just because an observer lists or inspects it.

The default external endpoint is a per-user Unix socket protected by filesystem permissions plus a generated capability token. TCP is disabled by default; loopback requires explicit opt-in and non-loopback requires TLS plus operator authentication. Read-only inspector sessions can coexist. Mutating commands, script evaluation, request routing/interception, debugger pause and `ApiWorkspace` sends require an exclusive controller lease plus the narrow capability appropriate to that action, surfaced to all observers as an event. Phase 7 determines the gatekeeper policy for navigation, script-originated network requests, interception and API-workspace sends; losing its required clearance fails closed before I/O.

## Delivery order and acceptance

### Slice 1 — foundations and safe inspection

1. Define `AutomationCommand`/`AutomationReply`/`AutomationEvent`, capabilities negotiation, error taxonomy, context/tab identities, event sequencing and controller leases in `blueice_ipc`.
2. Refactor `TabManager` to own the default browser context without changing existing `tab_id` clients.
3. Add an internal automation service in `core`, then the local-token `blueice-automation` adapter process. Prove two observers see the same frame/DOM generation and a controller action is reflected to both.
4. Ship DOM/AX/style/layout inspection, screenshot, navigation, locator resolution, basic waiting and input. Add the initial Elements/Accessibility DevTools panels and the TypeScript browser/context/page/locator client surface.

### Slice 2 — API/SOAP workbench and page debugger

1. Build `ApiWorkspace` on the common request service: collections/environments/secrets, HTTP request editor, response/timing/history display and a public local-socket UI/API test.
2. Add safe SOAP 1.1/1.2 XML envelope/fault handling and WSDL 1.1/2.0 operation-template import. Test a local SOAP service for `SOAPAction`, content type, namespaces, normal and Fault responses, malformed XML, blocked external entities and secret redaction.
3. Add BlueJS source metadata and instruction-boundary debugger hooks. Test script parsing, line breakpoints, scopes, exception breakpoints, pause/resume, each stepping mode, call-frame evaluation limits, task blocking while paused and other-tab liveness.
4. Deliver the DevTools Sources panel and native `Debugger`/`Runtime` commands before exposing their explicitly supported CDP counterparts.

### Slice 3 — standards-based browser automation

1. Implement the WebDriver HTTP baseline and run a scoped W3C WebDriver conformance corpus plus real Selenium client smoke tests.
2. Add BiDi session/subscription, browsing-context, log, input and script modules; prove reconnect, unsubscribe, stale-element and multi-context isolation behavior.
3. Implement the documented CDP subset only after each underlying capability exists, with discovery/capabilities tests. Do not make CDP a prerequisite for the native service.

### Slice 4 — Fetch and XHR transport

1. Build `blueice-net`'s asynchronous request state machine with local HTTP fixtures for redirects, status errors, chunked bodies, compressed payloads, malformed charset/JSON, cancellation, timeout and upload body integrity.
2. Add the BlueJS/core network IPC and event-loop integration. First implement `fetch` with same-origin GET/POST, streamed response, text/JSON/bytes decoding and abort; then CORS/preflight/credentials and cache/cookie integration.
3. Implement XHR as a compatibility facade over the same handles, testing state/event ordering and progress against deterministic local servers.
4. Feed the same request lifecycle into DevTools, Playwright-shaped `waitForResponse`/routing and BiDi network subscriptions. There is no second network implementation in any adapter.

### Slice 5 — advanced tooling and regression testing

1. Add Network/Console panels, response-body preview/redaction, route interception, trace export and supported Playwright-style artifacts.
2. Add BlueJS dynamic-page fixtures to `differential-testing/`: drive the same local server from BlueIce and Chromium, compare visible DOM/frame output and assert request order, method/body, redirects, CORS result, cancellation and decoded payload behavior.
3. Add DevTools, debugger, API-workbench and automation real-process tests through public WebDriver/BiDi/TypeScript/UI interfaces; retain unit/integration coverage in the underlying Rust crates. Add only stable, scoped conformance tests to CI; live external-server tests remain opt-in.

## Explicit non-goals for the first release

- Universal Chrome DevTools frontend/CDP compatibility, or copying Google's branded frontend.
- Speaking Playwright's private browser-server protocol or claiming all upstream Playwright Test features work unchanged.
- A generic external HTTP endpoint that bypasses page policy or Phase 7 review. The controller-authorized `ApiWorkspace` is a distinct human/API-client context, not a page-CORS bypass.
- Synchronous XHR, response `document` parsing, Service Workers, WebSocket/WebTransport, HTTP/3-specific APIs, HAR/video fidelity or a complete performance profiler.
- WS-Security, MTOM/XOP attachments, arbitrary remote WSDL/XSD dependency fetching, generated SOAP client code, or automatic import/execution of a third-party API collection.
- Persisting full response bodies, credentials or sensitive headers without explicit bounded capture policy.

## Checklist

- [x] Read and record the external API/specification boundaries — [`research/automation-and-dynamic-networking.md`](../research/automation-and-dynamic-networking.md)
- [x] Decide that DevTools, Playwright-shaped workflows, Selenium/WebDriver and CDP compatibility share one native automation service
- [x] Decide that Fetch/XHR share one `core`-owned request service and that BlueJS has no direct socket access
- [x] Define the compatibility, security, event-ordering and staged-delivery contracts above
- [ ] Define and implement the versioned `blueice_ipc::automation`, `blueice_ipc::network` and `blueice_ipc::debugger` modules and public error/capability schemas
- [ ] Refactor `TabManager` around browser contexts while retaining the current default-tab wire compatibility
- [ ] Build the local, authenticated automation adapter and inspection/locator/input slice
- [ ] Build the first-party DevTools Elements, Accessibility, Console, Network and Sources panels
- [ ] Build the controller-authorized API workspace, request collections/environments/secret handling and common request-event integration
- [ ] Implement the safe SOAP 1.1/1.2 editor/fault display and WSDL 1.1/2.0 operation-template import
- [ ] Add BlueJS source metadata and executable source locations at compile time, then native page-debugger breakpoints, pause/step/stack/scope semantics
- [ ] Publish and test the `@blueice/automation` browser/context/page/locator client library
- [ ] Implement the W3C WebDriver baseline and real Selenium integration tests
- [ ] Implement WebDriver BiDi subscriptions, log/script/input/network modules
- [ ] Implement the explicitly advertised CDP subset and discovery tests
- [ ] Replace `blueice-net`'s synchronous text GET with the asynchronous request state machine
- [ ] Wire Fetch, Promise/event-loop integration, origin/CORS/credentials policy and cancellation through BlueJS/core IPC
- [ ] Implement XHR over the same request handles and validate state/event ordering
- [ ] Extend Chromium differential testing with dynamic-page/network fixtures and add scoped conformance coverage to CI
