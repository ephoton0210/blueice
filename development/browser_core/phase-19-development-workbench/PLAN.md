# Phase 19 — Development Workbench: Human/AI-Synchronous Frontend Development

[← Back to plan](../BROWSER_CORE_PLAN.md)

**Status**: Proposed — design only. The reference `frontend` can display and forward input, but it has no development-inspection protocol, candidate-preview lifecycle, workspace service, human review state, or MCP development interface.

## Objective

Provide a first-party, local **Development Workbench** for building and debugging BlueIce's own native `frontend` implementations and their HTML, XHTML, XML, XSD, CSS, JavaScript/TypeScript, JSON and YAML source/configuration documents, including complete JSON Schema validation and YAML schema-profile support, while a human developer and an AI agent observe the same candidate, source revision, browser state and review decision. The human can inspect a live candidate UI and approve or reject a proposed change; the AI can inspect, diagnose, propose, build, test and prepare a preview through MCP, but cannot silently publish a change or take over the human's review authority.

In this phase, **frontend** means the BlueIce human-facing presenter process defined by Phase 4 (`backend/frontend-reference` today and future native platform frontends), not a website loaded in a tab. Page HTML/CSS/BlueJS/BlueTS development remains owned by Phase 17, Phase 18 and the MCP debugger contract. The workbench consumes those page-debug targets when a frontend defect needs a live page reproduction, but does not duplicate their DOM/script debugger semantics.

## Current-state correction

Phase 4 proves that one frontend can present `core`'s frames over the control-plane and shared-memory frame-plane boundary. It does not expose a frontend control tree, source provenance, input trace, paint latency, candidate build/reload action, visual comparison or review workflow. The existing MCP server can drive core/page operations and is planned to adapt BlueJS/BlueTS debugging, but it has no authority model for a registered development workspace or a synchronized human code-review decision.

BlueIce already owns HTML parsing and CSS cascade/layout inputs, but exposes neither as a common development-document service. It has no project-wide XML/XSD/XHTML document contract; JSON currently exists only where an implementation chooses to parse it; YAML has no project-wide source/configuration contract. A developer-facing tool must reuse the authoritative HTML/CSS parser/cascade and add bounded XML/XSD/XHTML/JSON/YAML services, not let an AI invent a second parser or accept data as executable instructions.

The workbench extends the project's central invariant instead of creating a second browser: baseline and candidate frontends attach through the same launcher to the same `core`, `Page`, script realm and frame generations. A candidate is a second *presenter*, never a shadow page/DOM or an independently navigated browser instance.

## Decisions

### One development service, two equal clients

Add a proposed on-demand `blueice-development` service and a versioned `blueice_ipc::development` vocabulary. The service owns development-session records, registered workspaces, candidate lifecycle, bounded artifacts, comparison state and review requests. The first-party Development Workbench UI and `blueice-mcp-server` are clients of that one service; neither directly controls a frontend process or edits an arbitrary filesystem path.

```text
Human Development Workbench UI ─┐
                                ├─> Development IPC ─> blueice-development
AI MCP client ─> mcp-server ────┘                           │
                                                           ├─> launcher / same core / automation + debugger IPC
                                                           ├─> baseline and candidate frontend-dev channels
                                                           └─> isolated registered-workspace builder
```

The launcher remains the owner of process supervision and rendezvous. `core` remains the owner of page state and frames. The development service asks the launcher to start/stop a candidate presenter under a declared policy; it never privately spawns a second core. Candidate, baseline, workbench and MCP see the same sequence-numbered core/automation/debug events.

### Frontend development instrumentation is explicit and portable

Every frontend that elects to participate implements a small `FrontendDevHello`/`FrontendDevRequest`/`FrontendDevEvent` protocol. It is separate from Phase 4's ordinary input/frame-plane transport so production presentation does not accidentally expose internal widget state. Its portable minimum is:

- a stable `FrontendTargetId`, process/build revision, toolkit adapter version and capability matrix;
- a `FrontendNodeId` tree with role, parent/child relationship, bounds, visible/enabled/focused state, accessible label and redacted properties;
- optional source provenance (`source_id`, line/column, component/template identity) where the toolkit can provide it;
- layout/paint/input/IPC frame events, including the `core` frame generation actually presented, latency and dropped-frame reason;
- a bounded screenshot or visual-snapshot capability, plus a workbench overlay for node selection, bounds, invalidation and input paths;
- explicit lifecycle actions: inspect, enable/disable overlay, capture snapshot, request passive/review mode and stop.

The contract does not require every native toolkit to expose a DOM-like tree or live source editing. Missing source provenance, screenshot support or an advanced layout property appears in the frontend capability matrix as `Unsupported`; BlueIce must not manufacture a false cross-toolkit abstraction. `backend/frontend-reference` is the first adapter and validates the contract before WinUI, SwiftUI or Qt implementations are required.

### Multi-language document services share one revision graph

Every development session maintains immutable, content-addressed `DocumentId`s and a dependency/provenance graph. A document records language, source root-relative identity, revision hash, parser/schema version, diagnostics and references to the frontend component, page, style rule, script/module, configuration entry or candidate build that consumes it. Human UI and MCP tools read the same document revision; an AI patch proposal creates a new `ChangeSetId`, never mutates the inspected revision in place.

| Language | Authoritative service | Workbench/MCP capabilities | Safety boundary |
| --- | --- | --- | --- |
| HTML | `blueice-html` tokenizer/tree builder and page source provenance | Parse tree, source spans, parse diagnostics, live DOM/source correspondence, resource/reference graph and candidate visual mapping. | Parsing a document does not execute scripts, fetch subresources or apply a mutation. |
| XHTML | `blueice-xml` strict XML parser plus XHTML namespace-to-DOM adapter and `blueice-css` | Namespace tree, source spans, well-formedness/conformance diagnostics, live XML DOM/source correspondence, CSS provenance and page/content-type mode. | `application/xhtml+xml` never falls back to forgiving HTML parsing; failed parse creates no executable/mutable page document. |
| XML / XSD | `blueice-xml` plus `blueice-xsd` | Namespace-aware XML tree, declarations/CDATA/comments/processing-instruction spans, XML/XSD diagnostics, type/identity/assertion results, schema dependency graph and configuration/API diff preview. | Parsing and validation have no entity/schema I/O, stylesheet execution or object construction; approved dependency bundles are immutable and resource-bounded. |
| CSS | `blueice-css` parser/cascade plus layout/style inspection | Rule/selector tree, diagnostics, declaration source spans, matched/overridden rule explanation, computed-style provenance and visual/layout diff. | Explanations use the same cascade as the rendered page; a selector/property is never interpreted as an AI command. |
| JavaScript / TypeScript | Phase 13 BlueJS and Phase 18 BlueTS/BlueTSC | Canonical Phase 12 `debug_*`, diagnostics, source/lowering/type/contract artifacts and bounded evaluation. | Execution remains controller-lease/gatekeeper/fuel bounded; document inspection does not run code. |
| JSON | Strict JSON parser plus `blueice-json-schema` | Value tree, pointers, diagnostics, schema/dialect validation, reference graph, vocabulary/capability report, annotation/result output and configuration-diff preview. | No comments, executable extensions, prototype construction or dynamic schema fetch. |
| YAML | `blueice-yaml` YAML 1.2.2 parser, selected failsafe/JSON/core resolution schema, and registered JSON Schema validation | Node tree, diagnostics, comments/presentation spans, anchors/aliases/reference graph, selected YAML schema profile, JSON-model conversion result, validation output and configuration-diff preview. | No custom tags, object construction, arbitrary deserialization, include/import hooks or implicit network/filesystem loading; aliases/depth/bytes have hard limits. |

JSON/YAML are development/configuration data, not page script languages. They may configure an operator-registered workspace or a frontend only through a versioned schema owned by that subsystem. Unknown keys, unsupported YAML features, an unregistered schema reference or a configuration that would widen workspace/build/network authority fail closed. The workbench never parses a YAML value as a shell command, MCP instruction, JavaScript fragment or dynamically loaded plugin.

### Complete JSON Schema service

Add a pure `backend/core/json-schema` (`blueice-json-schema`) engine as the sole schema implementation for development documents, API-workspace JSON request/response validation and any BlueTS JSON-like runtime contract that elects to use JSON Schema. The Development Workbench and MCP adapter own registration, authorization and serialization around that engine; the engine itself has no filesystem, network, page, shell or MCP authority. Its baseline is the complete [JSON Schema Draft 2020-12](https://json-schema.org/draft/2020-12/json-schema-core) dialect: Core, Applicator, Unevaluated, Validation, Meta-Data, Format-Annotation, Format-Assertion and Content vocabularies; boolean schemas; `$id`, `$anchor`, `$dynamicAnchor`, `$ref` and `$dynamicRef`; URI/JSON-Pointer resolution; recursive and compound/bundled resources; exact decimal-number semantics; annotation collection; and `flag`, `basic`, `detailed` and `verbose` standard result formats. Dialect selection also supports Draft 2019-09, Draft-07, Draft-06 and Draft-04 with each draft's original keyword and reference semantics; it never silently treats an older schema as 2020-12.

Every schema first validates against its selected meta-schema and produces an immutable `SchemaResourceId { canonical_uri, content_hash, dialect, vocabularies, registry_generation }`. `$id` and references use their specified URI rules, but URI identity does not confer I/O authority: resolution is against a versioned registry of built-in meta-schemas, workspace-approved schemas and operator-imported content-addressed bundles. Relative, recursive and dynamic references are evaluated correctly within that registry; a missing resource, unsupported required vocabulary, reference cycle that cannot be safely evaluated, duplicate identifier or resource-budget breach returns a deterministic diagnostic. An operator may import a remote schema bundle through the normal reviewed network path and pin its URI/hash; validation never fetches `http`, `https`, `file` or package resources implicitly.

All official vocabularies are implemented with their declared assertion versus annotation behavior, including the separate `format` annotation/assertion vocabularies and `contentEncoding`/`contentMediaType`/`contentSchema` annotations. Custom vocabularies are permitted only after an operator registers their URI, meta-schema, deterministic implementation and version; a schema declaring an unknown **required** vocabulary is rejected, while unknown optional vocabulary data is preserved as annotations exactly as the dialect requires. Neither an official nor custom vocabulary may run code, read a file, fetch a URL, instantiate a host object or alter MCP authority.

The service compiles schemas to a bounded immutable validation plan keyed by schema resource, dialect and registry generation. It retains instance JSON Pointers, schema keyword/absolute locations, source spans, annotation output and nested evaluation results so the Workbench, human and MCP client receive the same explanation. Validation has explicit recursion, regex-work, instance-byte/node and result-size budgets; a budget result is never reported as `valid`. YAML is eligible only after its bounded core-profile tree converts losslessly to the JSON data model (string mapping keys, JSON-compatible scalar/value tree and no unresolved aliases). The API workspace may validate a selected JSON request or response against a registered schema, but page `Response.json()` never gains an implicit schema check and a validation failure never causes a hidden retry or request.

### YAML schemas and schema-backed YAML validation

Add a pure `backend/core/yaml` (`blueice-yaml`) parser/presentation model with YAML 1.2.2 syntax and all three [recommended YAML schemas](https://yaml.org/spec/1.2.2/#chapter-10-recommended-schemas): failsafe, JSON and core. Here “YAML schema” is used in the specification's precise sense—a tag-resolution policy for scalars and collection nodes—not as a substitute for a validation language. Every YAML document has an explicit, versioned `YamlSchemaProfile`; the configuration default is the YAML 1.2 core schema, while failsafe and JSON schemas remain selectable for compatibility. The workbench shows both a scalar's lexical spelling and its resolved tag/value so a human and AI can diagnose a YAML 1.1-versus-1.2 type surprise instead of receiving an unlabelled host-language object.

Only the standard tags permitted by the selected profile are resolved. The parser preserves directives, comments, anchors, aliases, scalar style and source spans in its immutable document graph, but application/local/global custom tags, `!!python/*`-style object construction, merge-as-execution, include/import hooks and arbitrary deserializers are unsupported and fail with a source diagnostic. Anchors/aliases remain data references under hard alias, expansion, depth and byte quotas; they never invoke constructors or make a schema fetch. A multi-document stream gets one profile and validation result per document, all linked to its parent `DocumentId` revision.

For configuration validation, `YamlSchemaBinding` names both a `YamlSchemaProfile` and an operator-registered `SchemaResourceId`. `blueice-yaml` converts the resolved representation graph to the JSON data model before calling the one `blueice-json-schema` engine. The conversion is exact and carries a YAML-node-to-JSON-Pointer/source-span map: mappings must have unique string keys; scalar values must be JSON-compatible finite string/boolean/null/number values; and aliases must be resolved within quota without changing observable JSON-tree meaning. A complex key, unsupported standard tag, duplicate key, non-finite number, cyclic/unresolved alias or other non-JSON value yields `YamlNotJsonRepresentable`, never a guessed coercion. JSON Schema failures map back to the original YAML line/column, YAML path and JSON Pointer. This gives YAML the full selected JSON Schema dialect/vocabulary/output support without a second, weaker YAML validator.

### XML, XSD and XHTML share a strict data path

Add pure `backend/core/xml` (`blueice-xml`) and `backend/core/xsd` (`blueice-xsd`) engines. `blueice-xml` implements XML 1.0 Fifth Edition and XML 1.1 syntax, their XML Namespace rules, encoding detection/decoding, declarations, comments, processing instructions, CDATA, character references, internal DTD declarations and validated source spans. XML parsing has no ambient resolver: external parsed/parameter entities, external DTD subsets, `xml-stylesheet` execution and any URI dereference are disabled. An internal entity is allowed only where the chosen XML mode permits it and within explicit entity-count, expansion-size, depth, node, attribute and total-byte limits; a limit breach is a diagnostic, not a partially trusted tree.

`blueice-xsd` implements the complete XSD 1.0 and 1.1 Structures and Datatypes recommendation set, including namespaces/import/include/redefine/override, simple and complex types, derivation/substitution, wildcards, attributes, identity constraints, assertions, type alternatives and the required XPath expression semantics. A schema graph is registered as an immutable `XsdResourceId { root_uri, content_hashes, xsd_version, dependency_generation }`; every import location is resolved only from that pre-approved, content-addressed bundle. The engine returns the validated element/type/attribute provenance and bounded assertion/type diagnostics. It does not fetch `schemaLocation`, load external entities or instantiate a host-language class. DTD validity remains a separately selectable XML validation mode; XSD validation never depends on a DTD side effect.

XHTML is an XML page mode, not a tolerant HTML spelling. For `application/xhtml+xml`, BlueIce builds the DOM through `blueice-xml`, applies the XHTML/HTML namespace and DOM rules, then feeds that same DOM to CSS/layout/paint. XML well-formedness, namespace errors, duplicate attributes and invalid entity references are fatal for that document generation: there is no `text/html` recovery fallback and scripts/subresources do not start from a failed tree. The XHTML target covers XML-serialized HTML/XHTML semantics plus declared XHTML 1.x module/DTD metadata when an operator-pinned bundle is selected; no external DTD/schema is fetched merely because a document names one. DevTools, the Workbench and MCP report the parser/content-type/conformance mode so a human and AI never mistake a strict XHTML failure for HTML recovery.

`dev_get_document_tree`, `dev_get_document_diagnostics`, `dev_validate_document`, `dev_get_document_schema`, `dev_get_document_references` and `dev_explain_style` expose these services through one paged artifact schema. Schema operations name the document revision, `SchemaResourceId`/dialect, `YamlSchemaProfile`, `XsdResourceId`/XSD version or XHTML conformance mode where applicable, registry generation and requested standard result format; results include instance and keyword locations plus bounded nested causes. `dev_explain_style` takes a page/frontend target plus node/property and returns the source-ordered winning/overridden CSS declarations from the real cascade; it does not reproduce CSS matching in the MCP server. Selecting a frontend node, page DOM node, style rule, script handler or XML/XSD/JSON/YAML configuration entry follows graph links to related documents and, where available, a canonical Phase 12 `DebugTargetId`.

### Baseline/candidate synchronization and safe input

A `DevelopmentSession` names one registered workspace, one baseline frontend target and zero or more candidate targets. A candidate is launched from an immutable `ChangeSetId`/build fingerprint and connects to the launcher's existing core rendezvous socket. It renders the same `FrameReady` generation as the baseline. The development service records:

```text
WorkspaceRevision -> ChangeSet -> Build -> Candidate -> CoreFrameGeneration
                                           │                 │
                                           └── VisualSnapshot ┴── Comparison / Review
```

Baseline and candidate snapshots are comparable only after both report presentation of the same `CoreFrameGeneration`, viewport and relevant frontend revision. The workbench labels an unsynchronized preview instead of comparing stale frames. Human navigation/input against the baseline changes the one shared page; every passive candidate observes the resulting frame/event sequence. Candidate input is disabled by default, so an experimental UI cannot accidentally submit a form, navigate, spend a capability or race the human.

An operator may grant a candidate the same controller lease/input mode used by automation, scoped to a named tab/context and shown in both the workbench and MCP event stream. It still drives the one core through normal input/automation IPC, preserves page policy/gatekeeper checks and emits causal events. There is no direct candidate-to-page shortcut.

### Human review is a state machine, not an AI claim

Changes move through a durable, visible state machine:

```text
Draft -> Proposed -> Built -> PreviewReady -> ReviewRequested
      -> HumanApproved -> Applied
      -> HumanRejected | BuildFailed | Discarded
```

The AI may create a bounded patch proposal, attach analysis/test/build/visual artifacts and request review. The Development Workbench UI is the only component allowed to produce `HumanApproved` or `HumanRejected`; MCP has no approval tool or authority. Approval binds the exact `ChangeSetId`, workspace fingerprint, build outputs, compared frame generation and policy version. If any changes before apply, review becomes stale and must be repeated.

The human review UI presents the code diff, build/diagnostic summary, baseline/candidate visual snapshots, frontend-tree/layout diff, accessibility change summary, test results, active input/controller state and all AI-provided rationale as untrusted proposal text. A human can inspect the candidate live, select synchronized frontend nodes, accept, reject, request a revision or discard the candidate. Applying an approved change is an atomic workspace transaction or a documented handoff to the user's version-control workflow; this phase never auto-commits, pushes, uploads or deploys source.

### Workspace and build isolation

A workspace is registered locally by an operator with canonical source root, allowed config files, declared build/test task IDs, output roots, source-control policy, secret/redaction policy and resource budgets. An AI MCP request may name only this opaque `WorkspaceId`, never an arbitrary local path, command line, environment variable, compiler plugin, output directory or network destination.

`blueice-development` runs approved build/test tasks in an isolated candidate runner with explicit CPU, memory, wall-time, process, output-byte and network/secrets policy. AI-authored source is not executed merely because it was proposed. Build scripts and candidate presenters are untrusted until they run inside that constrained environment. Candidate artifacts are content-addressed, bounded and linked to the workspace/config/toolchain fingerprint; a successful build under one workspace or policy cannot be reused for another.

Hot UI replacement is an optimization, not a correctness requirement. The first implementation may rebuild and restart a candidate frontend process. A frontend can advertise a safe `reload_candidate` capability later only if it proves build revision, instrumentation generation and core attachment remain coherent. The baseline presenter remains alive throughout candidate failure/restart.

### Workbench interface and MCP adapter

The native Development Workbench is a local authenticated UI. Its MCP surface extends, rather than overlaps, Phase 12's `debug_*` tools: `debug_*` owns page/BlueJS/BlueTS VM and compiler inspection; `dev_*` owns workspace, frontend presenter, candidate and review workflow. A development session can return references to `DebugTargetId`s so the AI or human follows the same Phase 12 debugger handles into a page/script problem.

| MCP tool family | Contract |
| --- | --- |
| `dev_list_workspaces`, `dev_open_session`, `dev_session_status`, `dev_close_session` | Discover operator-registered workspaces and create generation-bound development sessions. |
| `dev_list_frontends`, `dev_get_frontend_tree`, `dev_get_frontend_events`, `dev_capture_snapshot`, `dev_set_overlay` | Inspect baseline/candidate frontend instrumentation, synchronized frame status and bounded visual/layout/input artifacts. |
| `dev_list_documents`, `dev_get_document_tree`, `dev_get_document_diagnostics`, `dev_validate_document`, `dev_get_document_schema`, `dev_register_schema_bundle`, `dev_get_document_references`, `dev_explain_style` | Inspect and validate HTML/XHTML/XML/XSD/CSS/JS/TS/JSON/YAML revisions through their authoritative parser/cascade/checker/schema service, with graph links to candidates and debug targets. XHTML identifies strict parser/conformance mode; XML/XSD identifies namespace/version/bundle/type provenance; YAML identifies YAML 1.2 resolution schema and exact JSON-model conversion; registered JSON Schema results retain dialect/vocabulary/hash provenance. Schema registration/import is operator-authorized; AI calls cannot supply an arbitrary fetch target. |
| `dev_propose_changeset`, `dev_get_changeset`, `dev_validate_changeset` | Submit and inspect a bounded patch against a registered workspace; validate scope/diff/policy before it can build. |
| `dev_build_candidate`, `dev_start_candidate`, `dev_stop_candidate`, `dev_compare_candidate` | Build and manage isolated presenters, wait for generation synchronization and return visual/tree/accessibility/performance comparisons. |
| `dev_request_review`, `dev_get_review`, `dev_next_events` | Send an immutable review request to the human UI and observe review/candidate/session events. No MCP approval operation exists. |
| `dev_apply_approved_changeset` | Applies only the exact, still-valid human-approved proposal using the registered workspace transaction policy; requires a separate write scope and records an audit event. |
| `dev_attach_debug_target` | Returns the canonical Phase 12 `DebugTargetId` for the shared page or relevant BlueJS/BlueTS target; it does not proxy debugger operations. |

MCP resources provide paged immutable diffs, snapshots, frontend-tree/layout diffs, build logs, test reports and review artifacts. All source, log, screenshot OCR/text, UI labels, page content and AI rationale remain untrusted data. Long operations emit `DevelopmentEvent`s through MCP notifications plus `dev_next_events` polling fallback, with session sequence number, `ChangeSetId`, candidate/build ID, core frame generation and explicit `EventsDropped` backpressure markers.

### Authorization, privacy and resource policy

The minimum development scopes are `development:inspect`, `development:propose`, `development:build`, `development:control` and `development:apply`. `development:apply` can consume a current human approval but cannot create one. Candidate controller/input actions additionally require the normal automation controller lease; script evaluation still requires Phase 12's `debug:evaluate`; build logs/source artifacts follow workspace redaction policy. Schema registration/editing is an operator action, not an AI-requested document field. The normal local MCP token authenticates a client but grants none of these scopes by itself.

The workbench attributes builder, candidate, snapshot, comparison, log and retained diff allocations to a distinct development-workspace account under Phase 8's fleet policy; it never charges them invisibly to an arbitrary tab. Visual snapshots and event retention are bounded and evict with visible markers. Under memory pressure, stale candidate artifacts/snapshots are evicted before a live baseline or an active human review; a review whose required artifact was evicted becomes `ReviewIncomplete`, never silently approved.

## Delivery order and acceptance

### Slice 1 — introspection and passive synchronized preview

1. Define `blueice_ipc::development` session, workspace, frontend-target, document, node, event, snapshot and error schemas.
2. Expose HTML/CSS through the existing authoritative parser/cascade and add bounded XML/XSD/XHTML/JSON/YAML services: XML 1.0/1.1 and XSD 1.0/1.1 with pinned dependency bundles; strict XHTML page/document mode; complete JSON Schema Draft 2020-12 and legacy-dialect validation; YAML 1.2.2 failsafe/JSON/core schema profiles; and lossless YAML-to-JSON Schema bindings, all with no executable tags or dynamic loading.
3. Instrument `frontend-reference` with the portable minimum frontend-dev protocol and build fingerprint.
4. Build the local Development Workbench UI with baseline tree/overlay/frame timeline/snapshot and document inspection.
5. Start a read-only candidate against the same launcher/core and prove both presenters report the same core frame generation.

Acceptance: a human navigates the baseline while the candidate visibly follows the exact later frame generation; the workbench identifies a selected frontend node in both visual overlay and tree; an HTML node and its CSS winner/overridden declarations resolve to the same source spans the renderer used; a well-formed XHTML document selects strict XML mode while malformed XHTML never silently becomes HTML; XML/XSD fixture bundles return exact namespace/type/assertion locations without entity/schema I/O; malformed JSON/YAML is diagnosed without invoking a command or loader; failsafe/JSON/core YAML schema profiles resolve the required YAML 1.2.2 cases with visible tags; YAML-to-JSON conversion either preserves pointer/span provenance or rejects an unrepresentable value; the complete JSON Schema test corpus cases selected for every supported dialect/vocabulary return their expected standard output location and format; a candidate crash/restart leaves baseline browsing uninterrupted.

### Slice 2 — AI proposal, build and human review

1. Register a local workspace and implement bounded multi-document change-set validation plus isolated approved build/test task execution.
2. Add candidate build/start/compare artifacts, review state machine, review UI and stale-review invalidation.
3. Expose the read-only/proposal/build/compare/review MCP tool families through Phase 12's adapter.
4. Require a human UI decision before workspace application; audit every request, build, candidate lifecycle and apply attempt.

Acceptance: an AI MCP client proposes a frontend patch, builds a candidate, attaches diagnostics/tests/visual diff and requests review; a human sees the live synchronized candidate and rejects it; the source tree and baseline remain unchanged. A separately approved unchanged proposal can be applied once, atomically, with an auditable revision record.

### Slice 3 — controlled input, cross-debug and advanced adapters

1. Add controller-lease-gated candidate input and causal core/automation event correlation.
2. Link frontend nodes/snapshots to Phase 17 page debugger and Phase 12 BlueJS/BlueTS targets without duplicating their protocol.
3. Add toolkit-specific adapters only after each meets the portable instrumentation/privacy/resource contract.
4. Add safe candidate reload only where a frontend can prove the required generation invariants.

Acceptance: a human and AI observe one controlled candidate interaction through the same core frame/event sequence, then use its returned `DebugTargetId` to inspect the corresponding BlueJS/BlueTS pause/diagnostic state; an unauthorized candidate cannot generate page input or bypass the gatekeeper.

## Testing strategy

- Test the development-session state machine, stale handles/reviews, approval binding, atomic apply/discard, capability checks and event ordering with deterministic fake frontend/build adapters.
- Run real-process tests with launcher, core, baseline `frontend-reference`, a candidate presenter, Development Workbench service and MCP adapter. Assert same core frame generation, no second page/core, candidate-crash isolation and passive-input denial.
- Test frontend-dev tree/node/source-provenance serialization, overlay coordinates, viewport mismatch, dropped frames, screenshot limits, redaction and unsupported toolkit capabilities.
- Test HTML parse/live-DOM provenance, strict XHTML page/parser behavior, XML 1.0/1.1 namespace/encoding/DTD-mode/parser cases, XSD 1.0/1.1 type/identity/assertion/dependency-bundle cases, CSS cascade explanations, JSON pointer/schema failures, every required JSON Schema dialect/vocabulary/output format/reference mode, YAML 1.2.2 failsafe/JSON/core schema resolution, comments/directives/anchors/aliases/multi-document provenance, JSON-model conversion/rejection and custom-tag/alias/depth rejection, cross-document graph invalidation and source-span/visual-diff correlation against the authoritative services. Run pinned upstream XML/XSD, YAML and JSON Schema suites plus local hostile-schema/resource-limit cases; a syntax/schema failure, timeout, external resolution attempt, unknown required vocabulary or unrepresentable YAML node must never become success.
- Test hostile patches/build logs/source strings, build timeout/resource exhaustion, arbitrary workspace/path/task/options, secret leakage and untrusted-content framing.
- Test one real AI MCP client plus human UI review flow end to end: proposed, built, previewed, reviewed, rejected, approved, applied, stale and memory-evicted variants.

## Explicit non-goals for the first release

- Replacing normal source editors, version control, code review systems, package managers, CI or deployment pipelines.
- Giving an AI arbitrary filesystem, shell, network, secret, plugin or source-control push authority.
- A second browser/page/DOM/script realm, a duplicate BlueJS/BlueTS debugger, or a candidate that automatically controls a shared page.
- Treating XML/XSD/JSON/YAML as executable configuration, resolving external XML entities/schemas or accepting custom YAML tags/object construction/dynamic includes, implicitly fetching a JSON Schema URI, coercing non-JSON YAML values during schema validation, or inventing a second HTML/XHTML/XML/CSS parser/cascade/schema validator for AI tooling.
- Automatic human approval, auto-commit, auto-push, auto-deploy, unreviewed live edit or invisible candidate replacement.
- Claiming a uniform DOM/source-tree/hot-reload abstraction for all native frontend toolkits before their adapters implement it.
- Retaining unbounded screenshots, traces, diffs, logs, candidate processes or source metadata.

## Checklist

- [x] Decide one development service shared by the human workbench and MCP adapter
- [x] Decide that baseline/candidate frontends attach to the same launcher/core and synchronize by presented core frame generation
- [x] Decide explicit frontend-dev instrumentation, passive candidate input and controller-lease-gated interaction
- [x] Decide AI proposal/build/review workflow with a human-only approval transition
- [x] Decide registered-workspace/build isolation, bounded artifacts, redaction and no arbitrary shell/path authority
- [x] Divide `dev_*` workspace/frontend workflow from Phase 12 `debug_*` VM/compiler debugging
- [x] Decide authoritative HTML/CSS integration plus strict XML/XSD/XHTML, bounded JSON/YAML document services, YAML 1.2 schema profiles and complete versioned JSON Schema support for the shared human/AI revision graph
- [ ] Define and implement `blueice_ipc::development`, frontend-dev protocol and reference frontend adapter
- [ ] Implement document identity/provenance graph, HTML/CSS adapters, safe `blueice-xml`/`blueice-xsd` engines and XHTML adapter, safe `blueice-yaml` parsing/schema profiles, lossless YAML schema bindings and the complete multi-dialect `blueice-json-schema` engine/registry
- [ ] Build the local Development Workbench UI and passive synchronized candidate lifecycle
- [ ] Build workspace/change-set/builder/review state machine and human approval UI
- [ ] Expose capability-scoped `dev_*` MCP resources/tools/events through Phase 12
- [ ] Add candidate input control, cross-debug linking and toolkit-specific adapters
- [ ] Complete real-process human/AI review, safety, resource and synchronization coverage
