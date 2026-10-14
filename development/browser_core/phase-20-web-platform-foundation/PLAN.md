# Phase 20 — Web Platform Foundation: Contexts, Documents, Interaction, and Security

[← Back to plan](../BROWSER_CORE_PLAN.md)

**Status**: In progress — Core-owned browser contexts, document-generation fences, native form/default actions, selected File/Blob APIs and bounded page file/drag events are implemented. FileReader/EventTarget/ProgressEvent and drag interfaces pass 58 VM/five typing cases; actual Core task scheduling, selected-byte DOM updates and failed-task containment pass focused integration. Readable streams, default/BYOB readers and Blob byte/text streams pass 20 stream cases and six host-profile cases, with focused Core integration. The affected BlueTS crate passes 354 cases with 60 optional oracle cases ignored. Core-retained object URLs, document/context fencing, staged IPC upload, JavaScript URL and additive BlueTS bindings now pass focused software and actual Core/child navigation tests; see [results](../phase-22-browser-shell-accessibility/OBJECT_URL_RESULTS.md). AbortController/AbortSignal, signal-based native/generic listener removal, weak dependency GC and browser timeout tasks now pass the focused 88-case cohort and actual Core selected-byte flow; see [results](../phase-22-browser-shell-accessibility/ABORT_SIGNAL_RESULTS.md). The [CustomEvent increment](../phase-22-browser-shell-accessibility/CUSTOM_EVENT_RESULTS.md) passes 102 VM/type cases plus actual Core/child payload-to-DOM integration. Core-backed node dispatch now supports arbitrary names, capture/target/bubble, live listener changes and trusted native base events; 70 VM/type/compiler and nine Core/IPC cases plus actual Core/child integration pass. See [results](../phase-22-browser-shell-accessibility/NODE_EVENT_RESULTS.md). Readable-stream tee, independent byte/BYOB branches and owner-traced Promise collection now pass 1,093 BlueJS and 146 bridge cases; see [results](../phase-22-browser-shell-accessibility/STREAM_TEE_RESULTS.md). Readable-stream asynchronous iteration and awaited for-await cleanup now pass 1,110 BlueJS, 148 bridge and 357 compiler cases, with 60 optional compiler oracle cases ignored. See [results](../phase-22-browser-shell-accessibility/STREAM_ITERATION_RESULTS.md). General child browsing contexts, complete DOM/lifecycle/events, writable/transform/pipe stream APIs and conformance, broader URL/subresource behavior, standard module loading and full web-platform security policy remain open. Full browser/API acceptance is tracked in the [current follow-up plan](../phase-22-browser-shell-accessibility/FOLLOWUP_DELIVERY_PLAN.md).

> **Sequencing note (2026-10-13):** the first vertical slice of `fetch`/XHR, the page event loop, the DOM read/write surface and cookies is scheduled by [Phase 27](../phase-27-browsable-web-mvp/PLAN.md). Items it lands are checked off here, in the owning phase.

## Objective

Make BlueIce capable of running ordinary interactive websites on one coherent web-platform model before adding specialised graphics, background execution or browser chrome. The compatibility baseline is the relevant [HTML Standard](https://html.spec.whatwg.org/multipage/), DOM, URL, Encoding, Fetch and related W3C/WHATWG specifications, delivered through explicit feature profiles and conformance tests rather than a claim that every API happens to work.

The invariant is unchanged: one launcher/core owns all browsing-context state, origin decisions, document generations, event ordering and rendered frames. Neither a child frame, a popup, an MCP client nor a module loader creates a shadow browser or receives an unmediated network/filesystem capability.

## Decisions

### Browsing context, navigation, loader and lifecycle

Replace the implicit one-document `Page` model with `BrowserContext -> TopLevelBrowsingContext -> Document/ChildBrowsingContext`. Every document has immutable IDs for its origin, active history entry, navigation/document generation, policy container and active realm. `iframe`, `frame`, `object`/`embed` document loading, named targets, popup creation, `window.open`, opener isolation, `postMessage`, `MessageChannel`, `BroadcastChannel`, `Location`, `History`, hash navigation, `popstate`, `hashchange`, scroll restoration and the back/forward cache are all specified against these identities. Same-origin and cross-origin frames have distinct realm/DOM access; an MCP handle cannot cross that boundary by naming an internal node ID.

`blueice-net` becomes the sole resource loader for document, module, stylesheet, image, media, font, frame and PDF destinations. It owns MIME/content-type and charset handling, URL/base-URL resolution, redirects/referrer, content disposition, integrity, priority, cancellation, caching and policy checks. HTML, XHTML, XML, JSON, PDF and download dispatch is determined before a document commits; no parser may quietly reinterpret a hostile response as a more privileged content type. Navigation commit, abort/error page, load events, visibility/freeze/resume, `DOMContentLoaded`, `load`, `beforeunload`/`pagehide` and bfcache restoration are sequence-numbered core events.

### Origin, isolation and security policy

Define one `SecurityContext` per document/frame: origin/site, secure-context state, ancestor origins, sandbox flags, permissions policy, CSP, referrer policy, mixed-content state, cross-origin isolation and storage partition key. The request service enforces same-origin/CORS, CORP/COEP/COOP, CSP source/nonces/hashes, SRI, cookie `Secure`/`HttpOnly`/`SameSite` policy, HSTS and certificate/TLS policy before a resource or script runs. Permission requests (camera, microphone, location, clipboard, notifications, fullscreen, file picker, popup) are explicit browser-chrome decisions; an AI MCP client can inspect/request a prompt but cannot grant it.

All policy decisions retain a redacted explanation with source/header/document provenance. Inherited frame policy can only narrow authority. An unsupported enforced policy fails closed with a visible document/security error; it is never ignored because the browser has not yet implemented a convenient UI.

### DOM, events, interaction and forms

Implement standard DOM tree, node/document/element collections, namespaces, ranges/selections, template/fragment parsing and serialisation, `innerHTML`/`outerHTML` with the correct contextual fragment parser, custom elements, Shadow DOM, slots and MutationObserver. Event dispatch implements capture/target/bubble, composed paths/retargeting, cancellation/default actions, trusted-versus-synthetic events and task/microtask ordering. Pointer, mouse, wheel, keyboard, composition/IME, focus, input/change, selection, clipboard, drag/drop, resize/scroll and lifecycle events use that one dispatch path.

HTML forms include association, control state, constraint validation, form-data construction, submit/reset/default actions, GET/POST/urlencoded/multipart/text bodies, file controls and navigation/error semantics. Credentials or file content never flow to an AI/MCP result without a separate privacy scope and a human-approved user gesture. Form events and default actions remain observable in the same causal trace as Fetch/XHR/PJAX.

### Modules and practical browser APIs

Add classic/defer/async/module script scheduling, import maps, static/dynamic `import()`, module graph/error propagation, top-level `await`, `URL`/`URLSearchParams`, `TextEncoder`/`TextDecoder`, `Blob`/`File`/`FileReader`, structured clone/transfer, `AbortController`, `DOMParser`/`XMLSerializer`, `MutationObserver`, `ResizeObserver`, `IntersectionObserver` and geometry/query APIs. BlueJS implements ECMAScript; this phase owns their browser-host bindings, origin checks and event-loop integration. Unsupported APIs appear in a per-document capability matrix and return a standard feature error rather than an accidental host panic.

### Live transport APIs

`EventSource`, `WebSocket` and `WebTransport` are explicit browser transport APIs, not aliases for Fetch or an MCP socket. Their connection lifecycle, framing/streaming, backpressure, close/error ordering, proxy/TLS/secure-context requirements, CSP `connect-src`, mixed-content/CORS/origin rules, cookies/credentials, quotas and hibernation behavior are owned by `blueice-net` and linked to the initiating document/worker. No live transport gives BlueJS a raw socket or lets a Service Worker, extension or AI bypass the gatekeeper. The Phase 12 `network_*` surface observes and tests these native connection IDs with bounded/redacted message previews and native-equivalent controller routing rules.

### AI MCP integration

The [Web Platform MCP environment](../phase-12-mcp-server/WEB_PLATFORM_ENVIRONMENT.md) projects this phase as `platform_*` tools/resources/events: target discovery, document/frame/history/lifecycle inspection, policy and capability inspection, bounded DOM/event/default-action traces, form diagnostics, controlled navigation/input and feature conformance reports. Any mutating action requires the appropriate controller lease and native policy/gatekeeper result. MCP has no direct cross-origin DOM access, raw cookie jar, permission grant, popup bypass, file-picker bypass or request socket.

## Delivery and acceptance

1. Define browser-context/document/security identities and navigation lifecycle before exposing iframes or history APIs.
2. Implement URL/loader/MIME/charset policy, CSP/SRI/referrer/sandbox/permission enforcement and local deterministic navigation security fixtures.
3. Implement DOM fragments/events/default actions, forms and standard input/selection path; then modules/import maps and observers.
4. Add child contexts, popups/opener policy, messages and bfcache only after same-origin/cross-origin isolation tests pass.
5. Publish the negotiated `platform_*` MCP adapter and cross-check the same context/document/policy IDs in DevTools, the human UI and an actual MCP client.

Acceptance requires a real site fixture with nested same-origin and cross-origin frames, module graph, form POST/multipart, keyboard/IME-style event sequence, History/PJAX transition, EventSource/WebSocket/WebTransport lifecycle, CSP/SRI block, sandboxed frame, permission prompt and bfcache restore. Tests must prove no policy bypass through a frame, module, form, popup, live transport, MCP call or stale document handle.

## Explicit non-goals

- Treating browser compatibility as merely completing ECMAScript or Fetch/XHR.
- A raw HTTP/DOM/permission/file API for AI clients.
- Silently downgrading unsupported security headers or running an isolated child frame in the parent realm.
- Implementing platform-specific window chrome, GPU/media stack, persistent workers, WebAssembly or PDF rendering here; those have dedicated later phases.
