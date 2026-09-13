# Phase 24 — Sandboxed PDF Viewer and Document Interaction

[← Back to plan](../BROWSER_CORE_PLAN.md)

**Status**: Proposed — PDF responses currently have no first-party document viewer, searchable text model, accessible page tree, form/annotation workflow or PDF-specific MCP target.

## Objective

Provide a secure, accessible, human- and AI-observable PDF Viewer for `application/pdf` documents in a normal BlueIce tab, without treating PDF as HTML, granting PDF JavaScript browser authority, or allowing a hostile parser to compromise `core`.

## Decisions

### Viewer architecture and content dispatch

When Phase 20's loader identifies a permitted PDF response or download-open request, the existing tab creates a `PdfDocumentTarget` rather than a hidden second page. A sandboxed `blueice-pdf` process owns parsing, font/image decode, page display-list generation, text/structure extraction, search index, form/annotation model and bounded raster tiles. `core` owns tab/navigation/origin/download/policy lifecycle; the frontend presents the viewer's tiles using the existing compositor/frame contract.

The viewer declares PDF version/features, encryption state, page count, signatures, document permissions, attachments, forms, annotations, tagged structure and accessibility status. Password entry is a visible human-only shell operation; passwords are never returned through DevTools/MCP. Malformed, encrypted, oversized, recursive, decompression-heavy or unsupported PDF content yields an error target without leaving partial privileged state in the tab.

### Rendering, text and accessibility

Render pages incrementally into generation-bound tiles with zoom/rotation/color-management/print profiles, text selection/search/copy geometry, outline/destination navigation, links, tagged-PDF semantic tree, reading order, alternative text and annotations. A document-level resource budget covers page count, objects, stream/decompression bytes, nesting, font/image glyph/cache memory, tile cache, search index and per-page CPU. Fonts/images follow the same decoder isolation principles as Phase 21.

The accessibility bridge maps tagged PDF structure, text ranges, headings, tables, links, forms and annotations into Phase 22's platform accessibility abstraction. Untagged PDF gets a clearly labelled geometric/text fallback, never fabricated semantics. OCR is opt-in, resource-bounded and labels generated text/provenance distinctly from embedded PDF text.

### Forms, annotations, links and safety

AcroForm/XFA support is a published capability matrix. Standard AcroForm value entry, validation, appearance generation, signature-status display, annotations and save/export workflows are staged; XFA, JavaScript actions, Launch actions, embedded executable files, external URI resolution and arbitrary attachments are disabled by default. A link uses Phase 20 navigation/gatekeeper policy; opening/downloading an attachment or saving a modified PDF requires normal shell/download authority and visible user confirmation.

PDF JavaScript is not executed as BlueJS and never gains page DOM/network/storage/MCP authority. If a later interoperability profile supports a narrowly selected document action, it must be sandboxed, capability-negotiated and human-visible rather than silently interpreting active PDF content.

### AI MCP integration

Phase 12 exposes `pdf_*` targets/resources/tools: list/attach documents; inspect redacted metadata, signatures/permissions, page/outline/tag/annotation/form structure; render bounded page/region tiles; search/select bounded text ranges; inspect accessibility mapping; follow a link through normal navigation policy; and observe viewer/render/search/form/annotation events. `pdf_fill_form`, `pdf_add_annotation`, `pdf_save` and print/download actions require an appropriate controller/write scope plus the same visible human confirmation or workspace review policy as the human viewer. AI cannot obtain a password, arbitrary attachment bytes, unredacted protected text, a raw PDF parser handle or PDF-JavaScript execution.

## Delivery and acceptance

1. Build the sandboxed parser/display-list/tile protocol and simple non-encrypted PDF rendering/search/text-selection fixture suite.
2. Add fonts/images/links/outlines/tagged accessibility/print, then AcroForm/annotations/save with human confirmation and audit.
3. Add encrypted-document prompt flow, signatures/permissions display, bounded OCR and the complete `pdf_*` MCP adapter.
4. Run adversarial corpus/fuzzing, memory/time/decompression limits, process-crash recovery and human/AI same-target tests before enabling general PDF viewing.

Acceptance requires a tagged accessible PDF, untagged fallback, complex fonts/images, outline/link navigation, search/selection, AcroForm, annotation, encrypted/password-denial, malformed/object-stream bomb, blocked JavaScript/attachment action, print/download review and an MCP client that observes the identical page/tile/text/form generation as the human viewer.

## Explicit non-goals

- Treating a PDF as a trusted HTML page or executing embedded JavaScript/Launch actions by default.
- A raw PDF object/file/attachment interface for AI clients.
- Unbounded rasterization, OCR, decompression, font loading or document retention.
