# AJAX, PJAX, SOAP, and Structured-Data MCP Environment

[← Phase 12 MCP plan](PLAN.md)

**Status**: Proposed interface contract. BlueIce currently exposes no MCP target for a Fetch/XHR request, PJAX transition, API workspace, SOAP operation, XML/XSD document, XHTML document, JSON/YAML document, or schema-validation result.

## Objective

Give an AI MCP client a complete, capability-negotiated way to inspect, diagnose, validate, test and—only with the relevant operator authority—control the AJAX, PJAX, SOAP and structured-document facilities planned in Phases 17 and 19. “Complete” means every native capability BlueIce advertises has a versioned MCP representation or an explicit `Unsupported` result. It does not mean MCP receives a second HTTP client, an XML/YAML/JSON parser, a schema engine, an unrestricted URL fetcher, or authority to turn data into instructions.

The native ownership boundary is fixed; Phase 20 extends the page network owner with EventSource/WebSocket/WebTransport while retaining this same boundary:

```text
AI MCP client
  │ network_* / api_* / dev_* tools, resources and events
  ▼
blueice-mcp-server
  │ automation + network + development IPC
  ▼
core ── one page Fetch/XHR/PJAX service ── blueice-net
  │
  ├── operator-created ApiWorkspace ── SOAP/XML/XSD layer
  └── Development Workbench documents ── HTML/XHTML/XML/XSD/JSON/YAML/schema engines
```

`core` remains owner of the browser context, page request policy, history, DOM and frame sequence. `ApiWorkspace` remains the Phase 17 operator-authorized client context. Phase 19 remains owner of registered workspaces, document revisions and schema registries. MCP is solely their bounded adapter.

## Targets, artifacts and capability negotiation

Every operation names a generation-bound target; no tool guesses a current tab, request, document or schema.

| Target / artifact | Owner | Meaning |
| --- | --- | --- |
| `NetworkTargetId` / `RequestId` / `LiveTransportId` | `core` | One page/browser-context network stream and one Fetch/XHR/navigation request or EventSource/WebSocket/WebTransport connection, including initiator and document generation. |
| `PjaxNavigationId` | `core` | One causally linked script request, history transition, DOM replacement and frame sequence. |
| `ApiWorkspaceId` / `ApiRequestId` | Phase 17 API workspace | An operator-registered HTTP/SOAP profile and one explicit user/AI-authorized invocation. |
| `DocumentId` | Phase 19 | Immutable registered HTML, XHTML, XML, XSD, JSON or YAML revision. |
| `SchemaResourceId` / `XsdResourceId` | Phase 19 registries | Pinned JSON Schema or XSD resource/bundle with dialect/version/hash provenance. |

`network_capabilities`, `api_capabilities` and `dev_capabilities` return protocol version, target generation, visible languages/profiles, implementation state (`available`, `planned`, `unsupported`), scopes, retention/parse/validation limits and redaction policy. Navigation, reload, hibernation, registry replacement or workspace revision invalidates old handles with `StaleHandle`; an MCP client must re-discover rather than following a similarly named resource.

Large request bodies, SOAP attachments, DOM/XML trees, schemas, diagnostics and validation results are cursor-paged immutable resources. Every response includes byte/item limits, `truncated`/`next_cursor`, target generation and a redaction marker. Body text, XML comments, headers, URLs, schema annotations, WSDL documentation and validation messages are untrusted content and are framed as data before reaching the AI.

## Page AJAX and PJAX tools

| Tool / resource | Contract |
| --- | --- |
| `network_list_targets`, `network_attach`, `network_detach` | Discover and attach to page network streams with a generation-bound session. |
| `network_list_requests`, `network_get_request`, `network_get_response` | Inspect Fetch/XHR/navigation lifecycle, timing, initiator, CORS/cache/redirect outcome, headers and declared body state. Secrets and configured sensitive fields are redacted. |
| `network_get_body` | Returns a bounded, policy-authorized response/request-body segment or resource; no implicit decompression, parsing or secret disclosure beyond the target policy. |
| `network_wait_for` | Waits for an explicit lifecycle predicate (request URL/method/status, Fetch/XHR completion, network idle, abort or resource failure) with deadline and cursor; never guesses an idle state. |
| `network_subscribe`, `network_next_events` | Delivers sequence-numbered request, route, body-retention and PJAX events through notifications with a required polling fallback. |
| `network_list_live_transports`, `network_get_live_transport`, `network_get_live_messages` | Inspects a bounded/redacted EventSource/WebSocket/WebTransport connection, negotiated protocol/stream state, lifecycle and message preview; never exposes a raw socket/stream. |
| `network_set_route`, `network_clear_route` | Controller-lease and `network:control`-protected native interception/block/fulfil/continue rules. Rules are ordered, origin/policy constrained, audited and time/byte limited. |
| `network_list_pjax`, `network_get_pjax_trace`, `network_wait_for_pjax` | Read or await the native causal `PjaxNavigation` trace: request, history entry, old/new DOM generation, scroll/focus outcome, cancellation/race result and rendered frame generation. |

These tools expose standard Fetch/XHR and History/DOM behavior through the Phase 17 service; they never offer an MCP-only `ajax()` or `pjax()` command. A request body is parsed as JSON/XML only when the caller asks for a selected registered parser/schema operation, and a response observation never alters a page history entry or DOM.

## HTTP, SOAP, XML and XSD API-workspace tools

| Tool / resource | Contract |
| --- | --- |
| `api_list_workspaces`, `api_get_workspace`, `api_list_requests`, `api_get_request_template` | Discover only operator-registered collections, environments, endpoint templates and SOAP bindings; secret values remain masked. |
| `api_prepare_request` | Fills a bounded registered request/template with declared non-secret inputs and returns a reviewable redacted preview, policy decision and required capabilities. It does not send. |
| `api_send` | Sends one prepared request only after `api:send`, controller lease and gatekeeper approval. The result names `ApiRequestId`; arbitrary client URL, proxy, credential, shell or filesystem options are rejected. |
| `api_subscribe`, `api_next_events` | Delivers the registered API workspace's request/policy/SOAP fault/retention events through notifications with a required polling fallback. |
| `api_import_service_bundle` | Requests an operator-reviewed WSDL/XSD dependency import under the API-workspace network policy, pins every approved resource URI/hash and returns the immutable bundle. It never recursively fetches a referenced resource without approval or invokes an operation after import. |
| `api_list_soap_operations`, `api_get_soap_binding`, `api_validate_xml`, `api_validate_xsd` | Inspect a pinned SOAP operation/binding/policy, or validate a bounded XML instance/schema using the canonical XML/XSD engine. Results contain source spans, namespace and schema locations, bounded assertion/type errors and redacted SOAP fault information. |

SOAP capability results identify the precise SOAP, WSDL, XSD, MTOM/XOP, WS-Addressing, WS-Policy and WS-Security version/profile that is available. A required but unsupported assertion returns `UnsupportedPolicyAssertion` before send. XML, XSD and WSDL parsing never enables external entity resolution, external schema loading, stylesheet execution or object construction.

## Registered document and schema tools

`dev_*` runs only against a registered Phase 19 workspace/document revision. Its document capability surface covers all named formats:

| Tool / resource | Contract |
| --- | --- |
| `dev_list_documents`, `dev_get_document_tree`, `dev_get_document_diagnostics`, `dev_get_document_references` | Return paged source/provenance trees for HTML, XHTML, XML, XSD, CSS, JS/TS, JSON and YAML. XHTML exposes strict XML namespace nodes separately from forgiving HTML tree nodes. |
| `dev_get_document_schema` | Returns the active JSON Schema dialect/vocabularies, YAML 1.2 failsafe/JSON/core schema profile, XSD version/bundle or XHTML conformance mode, as applicable. |
| `dev_validate_document` | Validates a selected immutable revision: HTML/XHTML/XML well-formedness, XSD, JSON Schema or YAML-to-JSON Schema binding. The reply never executes a script, request, tag, entity, schema fetch or build. |
| `dev_register_schema_bundle` | Operator-authorized registration of a JSON Schema or XSD resource bundle, with URI/hash/version/dependency provenance. An AI may select an existing opaque registration but cannot nominate an arbitrary remote URL. |
| `dev_explain_style` | Uses the live BlueIce CSS cascade for HTML/XHTML DOM nodes and maps the result to source spans; no CSS matching occurs in MCP. |

`dev_validate_document` returns the canonical standard result shape for the selected format: JSON Schema `flag`/`basic`/`detailed`/`verbose`; YAML profile plus conversion/pointer/span information; XML/XSD namespace/type/assertion locations; or XHTML strict parse/conformance locations. A missing schema, required vocabulary, XSD import, custom YAML tag, XML external entity or resource-budget breach is a visible failure, never a permissive fallback.

## Authorization, safety and events

Minimum projected scopes are `network:inspect`, `network:control`, `api:inspect`, `api:send`, `data:inspect`, `data:validate` and `data:register-schema`. `network:control` and `api:send` require the relevant controller lease and Phase 7 decision; `data:register-schema` is normally human/operator-only. An ordinary local MCP token provides authentication, not elevation. Read-only network/document observers may coexist where source/body privacy policy permits.

`network_subscribe`, `api_subscribe` and `dev_next_events` provide notifications plus a polling cursor fallback. Events include request lifecycle, body eviction, route decision, PJAX start/commit/cancel, API-workspace policy decision, SOAP fault, schema-bundle registration, document/schema diagnostic changes, validation completion and `EventsDropped`. Each is sequence-numbered in its owner context and carries the causal request/document/schema/frame IDs where available.

No tool can directly access sockets, an unregistered workspace, arbitrary URLs, filesystem paths, process commands, unredacted secrets, a raw XML entity resolver, a YAML tag constructor, a schema plugin or an infinite parser/validator budget. A lost lease, stale target, unavailable hibernated tab, failed gatekeeper check, unsupported capability, redaction policy or resource limit returns a structured error/event rather than silently proceeding.

## Delivery and acceptance

1. **Native owners first** — complete Phase 17 network/PJAX/API/SOAP operations and Phase 19 document/schema engines with stable target IDs, events, audit data and bounded artifacts.
2. **Read-only MCP** — add discovery, target attachment, request/PJAX/API/document/schema inspection, diagnostics, source resources and validation. Validate one real MCP client against local deterministic servers and registered fixture workspaces.
3. **Controlled mutation** — add route control, API request preparation/send and reviewed WSDL/XSD bundle import only after controller lease, gatekeeper, redaction and audit paths exist.
4. **Cross-layer diagnosis** — prove an AI can follow a PJAX trace from page request to DOM/frame generation, validate an attached JSON/YAML/XML/XSD/XHTML document, inspect a SOAP fault, and link the same event/source identifiers to Phase 12 BlueJS/BlueTS debug targets.

Acceptance requires explicit safe failure for an unregistered URL/path, stale request/document/schema handle, secret-bearing body read, oversized XML/schema/result, XML external entity, custom YAML tag, missing required JSON Schema vocabulary/XSD dependency, lost controller lease and unavailable hibernated target. It also requires tests proving MCP returns the same parser/cascade/schema/network result visible to the first-party DevTools or Development Workbench, with no second HTTP client or validator.
