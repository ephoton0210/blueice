# Automation, developer tooling, and dynamic networking

Research notes for [Phase 17](../phase-17-automation-devtools-and-ajax/PLAN.md). This is a scope and architecture note, not a claim that BlueIce currently implements any of the external protocols discussed here.

## Sources read

- [Chrome DevTools Protocol (CDP)](https://chromedevtools.github.io/devtools-protocol/) defines independent domains such as DOM, Network, Page, Runtime, Debugger and Accessibility. Each domain has commands and events encoded as fixed-shape JSON. Its tip-of-tree API changes frequently and explicitly offers no backwards-compatibility guarantee; a BlueIce endpoint must therefore advertise a pinned subset rather than claim blanket CDP compatibility.
- [Playwright's Browser API](https://playwright.dev/docs/api/class-browser) makes the ownership model clear: a browser owns isolated browser contexts, and each context owns pages. Its API surface also includes locators, input devices, request/response/route events, tracing and screenshots. These are useful user-level capabilities, not an open browser-wire specification that BlueIce should reverse-engineer or depend on.
- [Selenium's WebDriver documentation](https://www.selenium.dev/documentation/webdriver/) groups its modern automation surface around browsing context, input, logging, network and script capabilities. [WebDriver BiDi](https://www.w3.org/TR/webdriver-bidi/) standardizes a bidirectional JSON-over-WebSocket session model, including subscriptions and network interception. W3C WebDriver plus BiDi is consequently the portable interoperability target for Selenium clients.
- [The Fetch Standard](https://fetch.spec.whatwg.org/) models a response as a time-evolving value whose headers, status and body are not all available at once. It defines response types, redirects, CORS, credentials and bodies as one cohesive model; BlueIce must not treat an AJAX response as a fully decoded `String` returned by a synchronous GET helper.
- [MDN's Fetch API reference](https://developer.mozilla.org/en-US/docs/Web/API/Fetch_API) confirms that `fetch()` is Promise-based and is built around Request/Response objects, CORS and Origin semantics. [MDN's XMLHttpRequest reference](https://developer.mozilla.org/en-US/docs/Web/API/XMLHttpRequest) describes its still-important partial-page-update use case and its ability to return data other than XML.
- [Postman's SOAP request guide](https://learning.postman.com/docs/use/send-requests/protocols/soap/making-soap-requests) shows the essential workbench flow: choose an endpoint and `POST`, compose a raw XML envelope, choose the appropriate `Content-Type`/`SOAPAction`, then inspect the response. Its [API-definition documentation](https://learning.postman.com/v11/docs/design-apis/api-builder/develop-apis/defining-an-api) also lists WSDL 1.0 and 2.0 as importable formats. SOAP semantics themselves are defined by the [SOAP 1.2 messaging framework](https://www.w3.org/TR/soap12-part1/) and its [adjuncts](https://www.w3.org/TR/soap12-part2/), rather than by a browser AJAX API.
- Chrome's [JavaScript debugging guide](https://developer.chrome.com/docs/devtools/javascript/) centres its Sources workflow on breakpoints, stepping, call stack, scope, watches and console evaluation. CDP's [Debugger domain](https://chromedevtools.github.io/devtools-protocol/tot/Debugger/) makes the corresponding transport surface concrete: script source, breakpoint, pause/resume, step and call-frame evaluation commands, plus parsed/paused/resumed events.

## Findings applied to BlueIce

### One engine capability layer; several adapters

DevTools, Playwright and Selenium overlap heavily: navigation, tab/page lifecycle, DOM/Accessibility inspection, input, script evaluation, screenshots, logs and network observation. Implementing each at `Page` independently would create three subtly different views of a page, breaking the plan's central same-render-pass invariant.

The correct layer boundary is a versioned, bidirectional *internal automation service* over the existing `core`/launcher boundary. It owns semantic commands and event subscriptions. A DevTools UI, a WebDriver/BiDi server, a Playwright-shaped client library, MCP tools and a small CDP compatibility adapter are all clients of that one service. `core` remains the only owner of `Page`, DOM, rendering state, network policy and script realms.

The adapters must be honest about compatibility:

- A first-party DevTools UI can provide developer-facing inspection without pretending that Chrome's shipped DevTools frontend is universally compatible.
- A CDP adapter can expose a documented, versioned subset for tools that speak those domains. It must reject an unimplemented method explicitly; it must never advertise the moving CDP tip-of-tree as fully supported.
- Selenium compatibility should be standard WebDriver session commands first, then WebDriver BiDi for live events. This is preferable to a Selenium-specific private protocol.
- Playwright should get a first-party `@blueice/automation` TypeScript library with the familiar browser/context/page/locator model. Its behavioural contract can be close to Playwright's without claiming that BlueIce speaks Playwright's private browser-server transport or that the upstream `playwright` package can attach unchanged.

### AJAX is a browser platform feature, not an automation shortcut

An external automation client must not gain a privileged `POST arbitrary URL` escape hatch. Script-originated requests need the same-origin policy, CORS, credentials, redirect, cookie and referrer behavior that a normal page receives. The implementation must be shared with navigation where appropriate, but navigation is not a substitute for Fetch: it has a document destination and a visible state transition; Fetch/XHR has an initiator realm, a request handle, a stream of body bytes and cancellable asynchronous completion.

`blueice-net` therefore needs to evolve from its current synchronous, text-only, GET-only navigation helper into a request service that reports protocol metadata and decoded body chunks. BlueJS remains out of process: it asks `core` for a request over a dedicated IPC vocabulary, while `core` owns the actual request, policy enforcement and event publication. No received HTML/JSON or request headers should be trusted as instructions by an AI-facing adapter.

### An API/SOAP workbench is distinct from page networking

A Postman-like API client legitimately sends a user-authored request to an arbitrary service; that is not a CORS-governed page request and must not masquerade as one. It gets a distinct, operator-owned `ApiWorkspace` with its own explicit profile/cookie jar, selected credentials and request history. It shares the hardened HTTP/TLS, decoding, cancellation, proxy and capture machinery with `blueice-net`, but has a separate initiator and authorization policy. A page script can never acquire `ApiWorkspace` authority.

The useful first SOAP capability is not a bespoke transport: it is a raw XML request editor on top of HTTP `POST`, `Content-Type` and optional `SOAPAction`, with SOAP 1.1/1.2 template helpers, XML/SOAP Fault-aware response display and WSDL 1.1/2.0 operation-template import. XML parsing must disable external-entity/entity-expansion processing and use depth/size limits. WS-Security, MTOM/XOP attachments, generated strongly typed clients and arbitrary remote WSDL dependency resolution are separate work, not silent first-release omissions.

### Page debugging must be a BlueJS runtime capability

The Sources-style page debugger cannot be assembled from DOM snapshots after execution; it needs BlueJS to retain source metadata and expose safe instruction-boundary hooks. A dedicated bidirectional `blueice_ipc::debugger` channel connects `core` to the out-of-process BlueJS host; it is deliberately separate from the high-frequency script-to-DOM request vocabulary. A debugger service needs stable script IDs/source locations, executable breakpoint locations, pause reason, call frames, scope snapshots, exception and event-listener breakpoints, and resume/step-into/step-over/step-out controls. Its pause applies to the relevant tab's JavaScript realm only: unrelated tabs and the core's IPC/event delivery remain responsive, while new script tasks for the paused realm do not run until resume.

This belongs in the same controller-lease model as DevTools/automation. Evaluation in a paused call frame is inherently privileged and potentially side-effecting, so it is visible to observers, fuel/time-bounded and opt-in; a read-only preview may be offered only when the engine can actually prove it. The CDP `Debugger`/`Runtime` subset is an adapter over these native hooks, not the internal debugger design.

### Observable lifecycle is a shared prerequisite

Network panels, `page.waitForResponse`, BiDi subscriptions and tracing all need the same ordered record of page lifecycle, script, console, render and network events. Each event must identify its browser context, tab, request, initiator and monotonic sequence; optional retained bodies need explicit byte/time limits. A bounded event stream with loss/eviction markers is more truthful and safer than silently dropping history or allocating without limit.

### Security model

Developer/automation attachment is privileged. The default listener must be a per-user local Unix socket, with a newly generated capability token and restrictive permissions. TCP exposure, even loopback, is an explicit command-line opt-in; non-loopback requires TLS and an operator-supplied authentication mechanism. Read-only observers may coexist, but input, navigation, script evaluation, routing/interception and pause/resume require a controller lease per browser context. External commands and script-initiated network requests remain subject to Phase 7's gatekeeper policy hooks.
