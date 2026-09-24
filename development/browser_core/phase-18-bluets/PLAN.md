# Phase 18 — BlueTS / BlueTSC: TypeScript Front End, Emitter, Type-Aware Debugging, and Runtime Contracts

[← Back to plan](../BROWSER_CORE_PLAN.md)

**Status**: In progress. `backend/bluets` provides a standalone, host-neutral BlueTS front end and `bluetsc` command for an explicitly bounded initial language matrix; `backend/bluets-bluejs` directly lowers the supported classic-script and resolver-preserving ESM graph subsets to public BlueJS AST/bytecode without reparsing emitted JavaScript. A startup owner seals a closed `CoreCompilerProjectCatalog` before serving opaque v5 `describe`/`check`/diagnostic/static-metadata queries through `blueice-core --compiler-socket`; diagnostic and metadata inventory cursors are one-shot and exact-generation-bound. MCP attaches only to this bounded service. Its paired-core constructor keeps the browser and compiler adapters on one already-running core lifetime, and a real `tools/call` acceptance test covers all four metadata inventory kinds, diagnostic queries, exact metadata lookups, and source-free cross-kind, replay, and stale-cursor rejection. `blueice-launcher --compiler-mcp-socket <absolute-path>` is the one opt-in distribution seam: the launcher owns that stable `0600` public listener and passes only the compiled-in `core-closed-fixture-v1` profile plus a fresh private compiler socket to each core generation, never a profile, source, project/config/output path, resolver, option, update, build, or write input. Stale public sockets are reclaimed safely and non-socket/live endpoints reject before child spawn. A cutover stages and health-checks v2's sealed private listener before switching future public accepts; every already-accepted compiler/MCP stream stays bound to its one v1 private peer until v1 ends, then fails closed rather than being retargeted across catalog generations. New public connections after commitment reach v2. MCP now exposes `bluetsc_session_capabilities`: when a compiler adapter is attached, the core listener first mints one source-free 32-byte opaque attestation for that accepted relay stream, then MCP returns that exact core-issued ID as its receipt alongside all nine fixed read-only operations. `bluetsc_describe_project` exposes only an already-known opaque project handle and its canonical entry-module identity; it cannot enumerate a catalog or reveal source/project/config/output roots. When the adapter is absent it truthfully reports unavailable. Every compiler tool repeats that receipt and static queries additionally require the exact generation first observed by `bluetsc_check` under the same receipt. `bluetsc_list_diagnostics` uses that same observed generation and a core-minted one-shot cursor; it returns only compiler code, severity, canonical module identity, byte range, and untrusted diagnostic prose, never source text. Malformed core evidence, a mismatch, an unobserved generation, or a replayed/stale diagnostic cursor fails source-free; receipt/session state is never retargeted on cutover. `blueice-launcher --out-of-process-bluejs` supervises one capability-authenticated BlueJS child per core generation and routes core-authorized inline JavaScript and BlueTS declarations in DOM order into one bounded realm. An immutable core-owned startup authorizer may additionally pass a complete external closed graph, but the child never fetches, resolves URLs or import maps, reads a filesystem, or falls back to another graph. Core and child enforce the same document contracts; a shared generated, inventory-verified `core-script-document-context-v1` artifact lets the fixed checked BlueTS profile directly call only immutable `blueiceDocumentText()` and `blueiceDocumentOrigin()` snapshots. The page cannot select or widen this profile. No page/frontend/log can configure or reflect the child capability; the default launcher does not enable it. The child exposes no DOM/event object, fetch/cache, URL/import-map resolution, or general host callback. The opt-in debugger socket offers generation-checked source-free discovery, lifecycle-bound breakpoints, and root-code-unit pause/resume for pending classic scripts; its observable execution state is `Pending` → `Paused` → `Resuming` → `Completed`, but modules, nested frames, stepping, stack, scope, and values remain unavailable. Remaining prerequisites include production source/cache/integrity policy, bytecode source-map aggregation, general launcher compiler-catalog distribution/authorization and update/output elevation, fuller MCP negotiation, and native debugger execution.

The core-owned compiler query listener now binds each static-metadata and diagnostic pagination cursor to the one accepted stream that actually received it. Its internal worker hand-off carries the core-minted Hello attestation, not a client-supplied session field; guessed or cross-stream cursors fail before reaching the shared compiler service. A fixed core-owner receipt cap bounds outstanding stream bookkeeping. Closing the stream releases any unused cursor slots, and a later check revokes that project's outstanding receipts across streams. This strengthens the existing read-only MCP/core lifecycle without adding project registration, build, artifact, source-read, or output-write authority.

`blueice-launcher --debugger-socket <absolute-path>` now provides a stable owner-only debugger transport. The launcher gives each core generation a fresh private debugger socket and uses the same generation handoff gate as browser and compiler routes. A stream accepted before cutover remains bound to v1 and closes with it; only a stream accepted after commitment reaches v2, so opaque realm, program, safe-point, and static-metadata handles cannot cross a generation boundary. The relay parses no debugger traffic and grants no operation beyond the bounded core debugger protocol. Static-metadata inventory is additionally default-denied: an owner must select `--debugger-static-metadata-inventory`, a client must request `OpaqueInventory` in `Hello`, and the exact realm must report it available. The generation-bound opaque handle is the sole target for fourteen independently default-denied derived capabilities: `OpaqueSummary` requires `--debugger-static-metadata-summary` and returns only the BlueTS language label, compiler-options fingerprint, and source/type/symbol/contract counts; `OpaqueSourceInventory` requires `--debugger-static-metadata-source-inventory` and returns only bounded compiler-minted `source_id` values parent-bound to that exact handle; `OpaqueSourceProvenance` additionally requires `--debugger-static-metadata-source-provenance` and that prior source-inventory grant, then returns one already-inventoried ID's non-filesystem canonical module identity and labeled SHA-256 digest; `OpaqueTypeInventory` requires `--debugger-static-metadata-type-inventory`, `OpaqueTypeDisplay` requires `--debugger-static-metadata-type-display` plus the exact type-ID receipt and returns a compiler-produced display capped at 4 KiB; `OpaqueSymbolInventory` requires `--debugger-static-metadata-symbol-inventory`, `OpaqueSymbolDisplay` requires `--debugger-static-metadata-symbol-display` plus the exact symbol-ID receipt and returns a compiler-produced display capped at 4 KiB; `OpaqueContractInventory` requires `--debugger-static-metadata-contract-inventory`, `OpaqueContractDisplay` requires `--debugger-static-metadata-contract-display` plus the exact contract-ID receipt and returns a compiler-produced name capped at 4 KiB; `OpaqueContractValidation` requires `--debugger-static-metadata-contract-validation`, that exact prior contract-ID receipt, a live tuple, and a child capability report before it accepts a fixed-bounded data-only value and returns only `valid: true|false`; `OpaqueLoweringSummary` requires `--debugger-static-metadata-lowering-summary`, the exact prior parent-handle receipt, a live tuple, and a child report before it returns only the fixed safe-point-map ABI, fixed program ABI, canonical aggregate source-set fingerprint, and bounded safe-point count; and `OpaqueSymbolLocation` requires `--debugger-static-metadata-symbol-location`, separate exact same-stream source-ID and symbol-ID receipts under that parent, a live tuple, and a child report before it returns only those IDs and a non-empty half-open UTF-8 byte range capped at 1 MiB. `OpaqueSymbolType` requires `--debugger-static-metadata-symbol-type`, separate exact same-stream symbol-ID and type-ID receipts under that parent, a live tuple, and a child report; it returns only the exact pair of opaque IDs when the child confirms that the symbol's retained static type matches. `OpaqueSymbolContract` requires `--debugger-static-metadata-symbol-contract`, separate exact same-stream symbol-ID and contract-ID receipts under that parent, a live tuple, and a child report; it returns only the exact opaque pair when the child confirms the symbol's retained reifiable contract. A false relation returns an invalid target, never a contract plan or validation result. The symbol-contract relation never exposes a contract name, source/span, static record, or runtime value. The symbol-type relation never exposes a type display, symbol name, source identity, span, static record, runtime value, or general metadata read. The symbol-location operation never exposes source text, a source/module/path identity, line/column mapping, name, type, contract, span record, map entry, AST node, code-unit identity, bytecode offset, VM object, value, source-map translation, or a general metadata read. Core checks value depth (64), collection entries (4,096), nodes (32,768), strings/object keys (256 KiB), and finite numbers before forwarding contract data; the child repeats those limits before its pure `ContractPlan` validation. An owner may select all fourteen, but none is implied by another or by a transport upgrade. Source, type, symbol, and contract IDs carry no module identity, source/content hash, text, span, name, type, contract plan, validation behavior, bytecode, runtime value, child ID, or dereference operation; displays can contain only their independently authorized project identifiers; validation contains neither submitted data, source/module identity, span, plan, failure path, expected/observed category, bytecode, VM object, value, nor a general static-record read. Page-host v21 and debugger v20 carry the newest operation, and the named `DebuggerMetadataCapabilitySelection` builds the canonical manifest without fragile positional capability flags.

The debugger's metadata policy also has a same-stream opaque-handle boundary: after capability negotiation, only a parent handle actually returned by `ListStaticMetadata` may feed `DescribeStaticMetadata`, `DescribeStaticMetadataLoweringSummary`, `ListStaticMetadataSources`, `ListStaticMetadataTypes`, `ListStaticMetadataSymbols`, `DescribeStaticMetadataSymbol`, `DescribeStaticMetadataSymbolLocation`, `DescribeStaticMetadataSymbolType`, `DescribeStaticMetadataSymbolContract`, `ListStaticMetadataContracts`, `DescribeStaticMetadataContract`, or `ValidateStaticMetadataContract`. A bounded local receipt ledger includes the full realm/program/handle generation tuple and fails closed for guessed, changed, or cross-stream handles before core reaches the child. Source, type, symbol, and contract inventories each have bounded ID receipt ledgers; source provenance needs its exact source receipt, while `DescribeStaticMetadataType`, `DescribeStaticMetadataSymbol`, `DescribeStaticMetadataSymbolLocation`, `DescribeStaticMetadataSymbolType`, `DescribeStaticMetadataSymbolContract`, `DescribeStaticMetadataContract`, and `ValidateStaticMetadataContract` require their exact respective ID receipt in addition to independent grants. Symbol location requires both its exact source and symbol receipts; symbol type requires both its exact symbol and type receipts; symbol contract requires both its exact symbol and contract receipts. Guessed IDs cannot become child-probing targets.

The launcher-supervised out-of-process debugger regression now drives the public debugger socket through `Pending` → `Paused` at a compiler-verified non-entry root safe point → `Resuming` → `Completed`, then reloads the real HTTP document and verifies that the old realm, program, and safe-point tuple fails as stale. The test has no access to the private child endpoint or capability and does not claim stepping, stack, scope, or runtime-value inspection.

After an out-of-process child accepts an exact document, core now asks for one source-free aggregate realm accounting record and retains it only under the matching tab/document generation. The record contains only program count plus bytecode and heap byte totals; it is core-owned state, not a page, frontend, debugger, or MCP surface. Navigation and tab removal clear it. During admission, a malformed/mismatched child reply, child error, or transport loss clears it and closes the newly acknowledged realm; a later failed liveness probe clears the cached record rather than associating a successor with predecessor accounting. Host-wide quotas and public accounting remain separate work.

A trusted launcher embedding may now fix a per-realm BlueJS envelope before each child binds its private socket: live realm count, retained programs, root bytecode, and VM managed heap. The same immutable envelope is applied to ordinary launch and cutover children; the public launcher CLI, core, page, frontend, and page-host IPC cannot select or widen it. This bounds BlueJS admission and managed heap per realm, not child RSS or host-wide usage: source/registry/Rust/allocator/operating-system overhead and aggregate reservations remain outside the envelope.

Compiler IPC v5 supersedes the legacy v3/v4 wording below. A successful exact-version `Hello` now carries both the per-stream opaque attestation and a separately versioned, core-authored fixed query-only manifest containing the complete canonical nine-operation vocabulary. `ListDiagnostics` exposes only pages of diagnostics retained for an exact check generation, each with a fixed-bounded, one-shot cursor; no cursor has offset, source, path, or metadata-ID semantics. MCP validates both values exactly and copies the manifest unchanged into its session receipt; a missing, reordered, subset, duplicate, unknown-version, or locally derived manifest never creates an MCP compiler session. The manifest grants no source, path, resolver, option, registration, update, build, artifact, or output-write authority.

Standalone `bluetsc build` now rejects `strict-runtime` before compilation or output staging because this branch has not yet emitted or imported the versioned runtime boundary helper that would make that policy executable. This prevents an artifact or manifest from naming strict runtime enforcement that it cannot provide; `check` remains a static operation, while direct-page contracts remain core-owned. Emitting the helper and proving equivalent direct-page/ESM malformed-boundary rejection remain open work.

The prioritized completion worklist is [TODO.md](TODO.md). Update it with this plan when an implementation or acceptance condition changes.

The supervised-child route now also has its first concrete external-resource
authority: `HttpOutOfProcessPageScriptSourceAuthorizer` is immutable core
startup configuration, never page/frontend/child/MCP input. It fixes either a
canonical same-document-origin rule or one canonical exact origin, a canonical
URL-to-lowercase-SHA-256 manifest, and fixed module/depth/per-module/graph-byte
bounds. It accepts only direct `200`, identity-encoded UTF-8 responses with an
allowed JavaScript or BlueTS MIME type and a matching bounded `Content-Length`;
redirects, missing integrity, cross-origin targets under the same-origin rule,
ambiguous URL forms, and over-limit graphs fail closed. Core parses only the
manifest-covered static JavaScript/BlueTS edges, uses deterministic
`(canonical URL, expected SHA-256, language MIME lane)` cache keys, and derives
the resolver fingerprint from the full policy. It transfers only that closed
graph to the child; the child still receives no HTTP/cache/manifest/resolver
capability. A real loopback HTTP plus supervised-child test covers classic,
JavaScript module, and BlueTS module graphs; cache reuse; integrity, MIME,
redirect, and cross-origin denials; and source-free reports.

`blueice-launcher` now also has an isolated `blueice-bluejs-host` child behind
a private versioned, per-spawn-capability-authenticated IPC protocol. The child
owns bounded BlueJS tab realms and accepts only complete caller-authorized
source/resolver graphs, returning source-free outcomes and aggregate accounting.
An explicitly configured core route now forwards one loaded HTTP(S) document's
inline JavaScript and explicit BlueTS declarations as core-minted closed
one-module graphs in original DOM order, and closes their child realms on
navigation or tab removal. The BlueTS child profile is fixed to checked direct
lowering with no ambient declarations or page-selected resolver/compiler
options. Its v3 document record has no profile/capability selector: the core
must validate and supply exactly one copied document-text snapshot and one
canonical HTTP(S)-origin snapshot, and the child repeats their fixed byte and
canonical-spelling checks before installing the two primitive callbacks.
Navigation and close destroy that VM and its copied strings. The shared fixed
typing artifact is generated from the same callback inventory and is the only
ambient declaration admitted to the child BlueTS compiler. The
explicit
`blueice-launcher --out-of-process-bluejs` mode creates that endpoint and token
per core generation, passes them only to its core child, and retains the
`SpawnedBlueJsHost` supervisor through normal shutdown or a cutover. Neither
the launcher CLI, frontend/page traffic, nor logs can configure or reflect this
capability; the default launcher still does not select the mode, so normal page
scripts do not run out of process by default.

The opt-in debugger socket now additionally supports source-free opaque
program-location enumeration and exact compiler-verified BlueJS safe-point
validation for a live JavaScript realm. Every request remains tab, document,
and program-generation bound. It also owns a bounded, idempotent exact
breakpoint-configuration table that is cleaned up on realm replacement. When
the same core process selects `--inline-bluejs` or its trusted out-of-process
child route and a debugger socket, the
post-navigation admission turn does not immediately execute the declaration;
each handshaken bounded discovery/configuration request retains one further
session turn so the peer can learn and arm its exact root code-unit safe point.
For the out-of-process route those deferrals have a fixed 64-turn budget per
document, so repeated discovery cannot become an execution lease.
`ArmEntryBreakpoint` remains the instruction-zero compatibility form. v5
`ArmRootSafePointBreakpoint` starts a pending classic declaration, then stops
immediately before its verified non-entry root instruction and retains the
actual root interpreter frame until one resume. The owner can observe only
source-free state and resume it once. This is not arbitrary interpreter
suspension: modules/top-level await, child code units, re-arming/loop hits,
stepping, stack/scope/exception/object inspection, source or bytecode access,
and runtime-value exposure remain absent.

## Objective

Let a BlueIce page opt in to TypeScript source without a build-time `.js` artifact, while also providing BlueTSC for projects that need to compile TypeScript to portable JavaScript. Both paths preserve the reasons to author code in TypeScript: deterministic static diagnostics, source-level debugging, and—where data crosses a trust boundary—runtime validation of an explicit, reifiable contract.

## Standalone implementation and first BlueJS integration status

The direct bridge lowers parenthesized `new Identifier(args)` expressions with normal or spread arguments. Member constructors and omitted parentheses remain excluded.

The direct bridge lowers `delete` only for ordinary dot or bracket property references (without optional chaining). Identifier and non-reference operands remain outside the v1 subset.

The direct bridge lowers relational `in` and `instanceof` expressions. BlueTSC gives both a `boolean` result in the bounded checker; narrowing and custom-instance analysis remain future work.

Direct calls may use identifier, dot-member, or bracket-member callees with normal or spread arguments, preserving the BlueJS receiver. Optional calls remain excluded.

The current direct bridge subset supersedes earlier phase summaries: literals/identifiers, bounded quoted-string and template escapes, template slots containing supported direct expressions, direct calls/constructors with normal or spread arguments, supported unary/binary/logical/conditional/assignment/sequence expressions including relational `in`/`instanceof`, arrays with holes or spread elements (but not both in one literal), identifier/string/numeric/computed object keys with identifier shorthand and spread properties, dot/bracket reads, property assignments/updates/deletion. Nested templates, optional chaining, object methods, and accessors remain outside the subset.

The direct bridge accepts identifier, quoted-string, numeric, and computed object keys plus spread properties. A computed key lowers its supported direct expression to BlueJS `PropertyKey::Computed`; shorthand remains identifier-only, while methods and accessors remain excluded. BlueTSC merges an identifier-bound known record spread source into an inferred literal; computed keys, unknown/non-record spread sources, and general spread expressions remain `unknown` under the bounded static rule.

The direct bridge preserves non-spread array holes as BlueJS AST holes, rather than materializing `undefined` properties. It also lowers normal and spread array elements, but deliberately rejects any one literal that combines a hole and a spread element because BlueJS's spread construction path would otherwise materialize that hole. BlueTSC infers a known `T[]` spread source as `T` for a surrounding array literal; unknown/non-array sources retain the bounded checker's `unknown` element type.

The direct bridge lowers normal and spread call/constructor arguments. For calls to a known local function, BlueTSC expands only a tuple-typed spread into fixed parameter types; ordinary arrays, unknown values, and general iterable analysis remain outside that static rule.

Named local functions may use optional identifier parameters and a final identifier rest parameter. An optional parameter without an initializer lowers to an ordinary JavaScript parameter; within the function body BlueTSC treats its value as `T | undefined`. The direct bridge uses BlueJS's rest-parameter AST form; with an explicit `T[]` annotation, BlueTSC checks every normal or tuple-spread tail argument against `T` and infers generic `T` from the same tail. General array/iterable spread analysis remains outside the direct rule.

Named local functions may use a default parameter whose initializer is in the direct expression subset. BlueTSC retains the original initializer tokens, verifies a typed initializer against the parameter type, and the bridge lowers it directly to BlueJS `Param::default`; default expressions outside the direct subset remain excluded.

Named local function bodies retain ordered initialized local declarations, semicolon-terminated direct-expression statements, and `return` statements. The bridge lowers each structured expression statement to BlueJS in source order, so assignments and direct calls may have their normal local side effects. BlueTSC applies its existing bounded direct-call, property-access, and arithmetic checks to those statements; control flow, exception handlers, and every other unstructured body statement remain opaque and are rejected by the direct bridge.

Named local function bodies may also use `throw expression;`. BlueTSC retains and checks the direct expression, rejects an omitted value or a line terminator immediately after `throw`, and the bridge lowers it to BlueJS `Stmt::Throw` so the VM preserves the thrown JavaScript value. `try`/`catch`/`finally` and all other exception control flow remain outside the v1 direct subset.

Named local function bodies may use a direct-expression `if` condition with braced consequent, braced `else if` branches, and an optional braced `else` body. BlueTSC retains each branch structurally and the bridge emits BlueJS `Stmt::If` nodes with explicit blocks for braced bodies, while lowering an `else if` as a direct alternate rather than inventing an artificial block scope. The checker applies its existing bounded direct-expression checks to every condition and recursively to the branch bodies; it does not claim control-flow narrowing. Unbraced branches, an `else if` with an unbraced branch, loops, and every other control-flow form remain opaque and fail closed at the direct bridge.

For this structured function subset, an explicit return annotation that does not admit `undefined` must terminate every known path with a value `return` or `throw`. A bare `return;` is checked as `undefined`; `void`, `any`, `unknown`, and an annotation admitting `undefined` may fall through. Opaque control flow never supplies a termination proof and remains independently fail-closed at the direct bridge.

The direct bridge lowers template substitutions containing supported direct expressions to BlueJS expression slots. It tokenizes the original substitution with BlueTS's lexer and constructs BlueJS AST directly; it never calls a BlueJS source parser on BlueTSC output. Nested templates and embedded expressions outside the direct subset remain excluded.

Non-substituted template literals use the same ordinary escape decoder as quoted strings.

The direct bridge decodes ordinary quoted-string escapes (`\\`, quote, `\n`, `\r`, `\t`, `\b`, `\f`, `\v`, and `\0`) to BlueJS strings. Unicode, hexadecimal, legacy octal, and line-continuation escape forms remain deliberately unsupported in this bounded subset.

Record inference and direct lowering accept identifier-keyed object shorthand `{ key }` for a local binding. Direct lowering also accepts computed keys; because their names need not be statically known, the bounded record inference result is `unknown`. Object methods and accessors remain excluded.

The direct bridge lowers simple and compound assignments plus prefix/postfix updates to ordinary dot or bracket property references. The existing template/object/member-call boundaries remain in force.

The initial implementation completes the work that has no BlueJS dependency before page-runtime coupling. The first direct bridge remains host-neutral and deliberately narrower than BlueTSC's emitted-JavaScript matrix:

- `blueice-bluets` accepts only caller-supplied `ModuleLoader` records. The library itself does not read files, URLs, DOM state or host capabilities. `bluetsc` is the separate, project-root-confined filesystem adapter.
- Its pinned `blue-ts-0.1` matrix parses/binds typed variable and function declarations, named `export default function`, local-value `export default name`, and local named value-export declarations, interfaces, aliases, type-only imports/exports, primitive and literal types, records, arrays, tuples, unions, intersections and the corresponding erasable annotations/assertions. It resolves a closed relative module graph, produces stable diagnostics for parse/unsupported syntax, resolution, duplicate names, unknown types and the implemented assignment / return checks, and never emits on an error.
- `bluetsc check` uses that shared pipeline without writes. `bluetsc build` stages ESM `.js`, optional column-provenance source maps and public `.d.ts` files, then replaces the selected output directory only after every artifact has been staged. Its fingerprint includes source content, the pinned language version, target, source-map/declaration modes, runtime-policy label and resolver identity.
- `bluetsc` accepts either one explicit entry or a `bluetsc.json` project file. The latter supports multiple `entries`, project-root-confined `outDir`, `sourceMap`, `declaration`, `target`, `runtimePolicy`, and exact/prefix `imports` mappings. Config flags cannot be mixed with client-side overrides; an import target, entry or output path that escapes the declared root is rejected before compilation, including a syntactically local `outDir` whose existing ancestor is a symlink outside that root. An atomic output directory also cannot contain an input source module, so it cannot replace `src/`. This is the standalone compiler's closed-world resolver, not a page/network loader or an arbitrary package-manager hook.
- A project-root-confined relative import or exact `imports` mapping may target a local `.d.ts` declaration module. Declaration modules are parsed, checked, hashed and retained in VM-independent debug metadata, but are type-only: they never emit JavaScript or a runtime import-map target. BlueTSC rejects a `.d.ts` entry, a value import resolving to one, or runtime content within one. When declaration output is selected, BlueTSC preserves the authorized `.d.ts` source root-relatively and retains consuming `import type` clauses. Separately, a direct page host may supply an exact, already-verified generated `lib.blueice.d.ts` through `CompilerOptions::ambient_declaration_modules`; it is subject to the same source/module limits and build fingerprint, cannot import or re-export, and exposes static ambient declarations only. Direct-page admission also enables the fingerprinted `require_declared_global_calls` policy, which makes a top-level direct call fail with `UnknownName` unless it resolves to a local or verified ambient function. The standalone CLI cannot configure either host-only facility, and BlueTS never acquires ambient, remote, package-manager, or arbitrary `lib.dom.d.ts` declarations on its own.
- Every successful build stages a root-relative `bluetsc.manifest.json` with language version, project fingerprint, target, runtime policy, requested output modes and emitted entries. A configured `imports` map also emits `bluetsc.importmap.json`, translating source `.ts`/`.tsx` mappings to their generated `.js` locations. Both files are published with the artifacts, so they contain no absolute host path and cannot drift from an atomic build.
- The filesystem adapter supplies root-relative source identities to the shared compiler. Therefore artifacts, source maps and the VM-independent `BlueTsDebugInfo` do not expose checkout paths, and relocating an unchanged project does not change its resolver or source-identity contribution to the build fingerprint.
- The standalone compiler emits the VM-independent portion of `BlueTsDebugInfo`: compiler-minted source-content-hash identities, static types, symbols and spans, plus exact reifiable plans for successful non-generic local declarations only. It also contains a bounded, pure contract IR/validator for data-only JSON-like values. Imported, generic, erased or otherwise unreifiable declarations have no static contract handle; neither artifact claims a runtime type tag or validates a live page boundary.
- BlueTSC source maps use Source Map v3 segments at copied, rewritten and erased-source boundaries rather than line-only placeholders. Generated and original columns use UTF-16 code units; CRLF is represented as one source line transition. This gives portable JavaScript builds column-level TypeScript provenance without claiming a BlueJS bytecode safe-point map.
- `IncrementalCompiler` is a reusable, host-neutral single-entry session for development hosts. It reloads the caller-authorized graph to detect changed source or resolution edges, reuses parsed modules with identical bytes, and rebinds/rechecks only changed modules plus their reverse dependencies. It keeps only a successful cache entry and refuses reuse when the entry or any compiler option differs; its work-selection result is observable without exposing a BlueJS VM or page state.
- Generic aliases, interfaces and direct function calls retain their declarations' type parameters, including through local type-only imports. An interface may extend one or more named interfaces, including a generic instantiation; inherited fields participate in bounded structural checking, property lookup, declaration emission and reifiable contract conjunction. Its exported type-only surface carries inherited local declaration fields so an authorized consumer need not import private parent declarations. Direct local calls may either infer or explicitly supply their type arguments. They instantiate bounded structural checks, enforce `extends` constraints, and resolve trailing default type arguments (including in declaration modules). A declaration's type parameter never leaks into surrounding module scope. The checker additionally infers array literal element types, boolean comparisons, boolean-only `&&`/`||` chains, nullish coalescing after removing left-side `null`/`undefined`, unary `typeof`/`void`/boolean/numeric expressions, conditional branch joins, numeric `+`/`-`/`*`/`/`/`%`/`**` expressions, `<<`/`>>`/`>>>`/`&`/`^`/`|` expressions and known-string concatenation. It rejects numeric, exponentiation or bitwise/shift operators whose two operands are known incompatible primitives; it reports an unparenthesized unary exponent base as an ECMAScript early error; and it rejects a strict equality operator whose known `number`, `string` or `boolean` operands are disjoint. Unknown, `any`, union and structural operand rules remain outside this narrow initial rule; method/callback overload resolution and all other general expression inference remain pending.
- The parser rejects a `.tsx` module at its source-identity boundary, even if it has not yet reached a JSX tag. This prevents a TSX project from being treated as ordinary erasable TypeScript; tagged JSX is rejected by the same stable `UnsupportedSyntax` path. It also reports the ECMAScript early errors for an unparenthesized `??` mixed with `&&` or `||`, and a unary expression used directly as an exponentiation base, including in runtime spans that the bounded front end otherwise preserves for emission.
- Legacy CommonJS-oriented `import =` and `export =` forms are likewise rejected as `UnsupportedSyntax`, rather than being emitted as invalid ESM.
- For a direct call to a locally declared function, the checker verifies the accepted argument count and each annotated parameter after bounded generic substitution, whether type arguments are inferred or explicitly supplied. The same rule applies to semicolon-terminated direct-expression statements in a function body. Optional and default-initialized parameters are omittable while preserving a known return type. In a function body, a bare optional parameter has type `T | undefined`, while a default-initialized parameter has type `T`. Signature-only local overload declarations are resolved in declaration order for direct calls and erased from JavaScript; a non-declaration signature must have a compatible local implementation. This intentionally does not claim method calls, callback analysis, constructors, or general expression inference.
- Direct property access on an inferred record or a local/interface type alias is resolved to the declared field type (including a generic instantiation). Optional fields produce `T | undefined`; chained/member-call analysis and arbitrary JavaScript property semantics remain outside this static subset.
- The standalone `ContractPlan` validator accepts per-boundary `ValidationLimits` for depth, collection entries, visited-node fuel and string bytes. Core now applies it only to its two installed immutable host-to-script snapshots (`blueiceDocumentText` and `blueiceDocumentOrigin`) before realm callback capture; their inventory names stable binding IDs, contract IDs, direction, capability and fixed budgets. JSON/Fetch/XHR, URL/query, storage, messaging, foreign-module, extension, DOM-object, provenance, strict-runtime coverage and allocation-attribution enforcement remain deliberately separate work.
- `backend/bluets/tests/typescript_oracle.rs` is the BlueTSC compatibility-oracle job run on every push and pull request. Its test-only `BLUEICE_BLUETSC_ORACLE` environment variable names the pinned TypeScript 5.9.3 `tsc` executable; the all-caps spelling is environment-variable convention, while BlueTSC is the BlueIce compiler name. The job verifies that pin before executing, then runs a fixture matrix covering generic properties, constraints/defaults, explicit direct-call type arguments and generic interface heritage (including local `.d.ts` parents and rejected conflicting inherited fields), optional record fields and optional/default/explicit-`undefined` parameters, local constrained generic function overloads, ordered direct-expression function-body statements and rejected calls within them, function throws and invalid bare throws, braced function `if`/`else if`/`else` bodies and rejected direct calls in their conditions, ordinary and hole-bearing array literals plus array/object literals with spread elements/properties and computed object keys, direct-expression default and array-typed rest parameters plus direct calls/constructors with spread arguments, simple object literal property reads, templates with supported expression substitutions, generic arithmetic, exponentiation, numeric bitwise/shift, compound assignments, identifier/property updates and comma sequences, boolean comparisons and relational membership (`in`/`instanceof`), boolean logical chains, `??`, `typeof`/`void` expressions and conditional branch joins, named default-function, local-value default, and local named-value ESM exports, rejected assignment/call arguments including incompatible known primitive arithmetic, exponentiation or bitwise operands, disjoint strict primitive comparisons, `typeof` annotation mismatch, `??` annotation mismatch, unparenthesized `??`/logical mixing and unparenthesized unary exponent bases, Source Map v3 shape and accepted Node output. It uses ES2022 modules and a temporary `type: module` package boundary, so both BlueTSC and external `tsc` ESM artifacts execute under Node; the local default and named-value fixtures additionally compare each compiler's exact public `.d.ts` output. Rejected fixtures assert the exact BlueTS diagnostic count, stable code and source line, then require the pinned compiler to report the same count and source lines. Node and the external `tsc` are test tools only; neither is linked, spawned, or discovered by BlueTSC or BlueTS.
- `CompilerLimits` makes source bytes/tokens/type nesting, module count/edge count/import depth, aggregate source bytes, generic-expansion work and source-map segments explicit compiler policy. Every limit participates in cache and artifact fingerprints; adversarial unit tests require a stable `ResourceLimit` failure with no output.
- `blueice-bluejs` owns the public `BlueJsProgramV1` wrapper and its `bluejs-program-v1` identity. `backend/bluets-bluejs` is the sole crate that depends on both compilers. It consumes a checked, already-tokenized BlueTS source module or closed module graph and directly constructs the BlueJS AST before BlueJS compiles bytecode; it never gives BlueTSC-emitted JavaScript to the BlueJS parser. Its executable v1 subset is the authoritative current direct-bridge subset summarized above, which supersedes the historical phase-level lists in this document. It retains ECMAScript grammar boundaries by rejecting unparenthesized `??` mixed with `&&` or `||`, and an unparenthesized unary exponent base. ESM mode maps local named/default exports and runtime imports to BlueJS module entries. Every runtime request uses the exact canonical target retained by BlueTS's caller-authorized resolver; only runtime-reachable modules become BlueJS nodes, so type-only `.d.ts` inputs remain static. The parser represents initialized local declarations, direct-expression statements, throws, returns, braced `if` bodies, and braced `else if` chains in functions; it records every other body token as opaque so the bridge rejects rather than silently drops a statement it cannot lower. Re-exports and other runtime shapes not listed in the current subset fail explicitly until their direct lowering is implemented.
- The public [BlueTS ↔ BlueJS integration contract](INTEGRATION_CONTRACT.md) records the shipped structured-program bridge and its still-pending source-provenance, safe-point-map and generated host-typings requirements without adding a BlueJS dependency to BlueTS itself.
- `blueice_engine::script::direct_page::DirectPageScriptHost` binds the in-process direct bridge to a live core `TabManager`: a typed `TabId` resolves the current `Page`, its HTTP(S) URL is canonicalized to a realm origin, and a private replacement-document generation forces realm recreation even for same-origin navigation. A core owner may inject a validated `DirectPageRealmOwner` at construction, keeping VM, realm, program-count, bytecode, and static-debug retention limits outside every script request; an over-budget direct attachment leaves no retained program or debug record. Its `realm_stats` exposes only the owning tab's retained program count, bytecode charge, and VM heap statistics; navigation releases the old charge before a successor document can admit code. The optional `run_session_with_script_requests_and_direct_page_host` lifecycle seam synchronizes only previously admitted realms after each session batch. `DirectPageInlineExecutor` is the separate, explicitly configured lifecycle owner for inline declarations: it generates its selected profile itself, may receive that same validated realm owner, runs a document's opted-in inline declarations once in document order, and retains only bounded source-free reports. An over-budget inline attachment leaves no program or debug record and becomes a source-free rejection report. `run_session_with_script_requests_and_inline_page_executor` invokes that owner after each session batch. `blueice-core --inline-bluets-profile <known-profile>` may construct this executor with a core-selected profile and fixed default compiler policy; omitted means no executor. The source-free `GetBlueTsScriptReports` frontend query drains only the addressed tab's retained `(document generation, ordinal, classic/module kind, fixed outcome category)` records. It neither enables execution nor returns source text, compiler diagnostics, bytecode, or runtime values; a binary subprocess regression exercises a classic script, module script, and rejected typed call after real HTTP navigation. BlueJS now provides a realm-local, GC-safe primitive host-function ABI plus a page-runtime registrar that cannot execute bytecode or inspect the VM; registrations disappear with the realm on navigation. `DirectPageScriptHost` installs the checked-in `core-script-document-text-v1` profile and the composed `core-script-document-context-v1` profile only: the latter adds `blueiceDocumentOrigin(): string`, a copied canonical tuple origin with no path, query, fragment, URL object, DOM node, event, or mutation capability. Both values are rebound after replacement. This is not a general `document` object, automatic production script owner, or BlueJS process launcher.
- The real-process inline regression suite now also opens a second tab in the
  same opt-in `blueice-core` instance, navigates both tabs to independent
  script-bearing HTTP documents, and drains `GetBlueTsScriptReports` through
  each tab's addressed envelope. This proves report isolation at the process
  boundary: draining tab one reveals and removes only tab one's record, leaving
  tab two's record available only to tab two's query. It is evidence for the
  narrow report control plane, not a substitute for the outstanding general
  multi-tab page-host, resource-accounting, or debugger acceptance work.
- A separate binary regression replaces one tab's document through two real
  HTTP navigations and drains the report after each one. The report's distinct
  core-private document generations prove the configured executor observes the
  replacement and executes the new inline declaration once; it does not by
  itself prove every stale-handle/debugger invalidation requirement.
- The equivalent standard-JavaScript binary regression navigates one tab through
  two HTTP documents. Each document asserts that `blueiceDocumentOrigin()` is
  its current core-canonical tuple origin before its bounded report is emitted;
  the two successful, distinct-generation reports prove the executor replaces
  the prior realm and snapshot rather than retaining the first document's
  callback. This remains process evidence for the narrow immutable binding,
  not general DOM, debugger, or stale-handle coverage.
- A two-tab standard-JavaScript session regression drives independent HTTP
  documents through the tab lifecycle and drains each addressed
  `GetBlueJsScriptReports` envelope. Draining the first result neither reveals
  nor consumes the second, proving the source-free JavaScript observation queue
  remains tab-isolated at the core session boundary.
- Another binary regression serves a document whose copied text exceeds the
  document-text binding's fixed one-mebibyte contract budget. The normal HTTP
  navigation completes, but inline admission reports only the fixed source-free
  contract-rejection category before BlueTS/BlueJS program admission. This is
  process evidence for the two installed immutable result boundaries, not
  general strict-runtime contract, allocation, provenance, or foreign-data
  enforcement.
- `blueice_engine::debugger` is the core-side dispatcher for the versioned
  debugger IPC. `blueice-core --debugger-socket <path>` accepts a separate
  peer, performs its independent `Hello` negotiation, and forwards later
  requests through a bounded worker-to-session channel. `ListPageRealms`
  returns at most 128 loaded tab/document-generation identities, with no URL,
  source, program, bytecode, or runtime value; an over-cap list fails closed.
  With a selected live `--inline-bluejs` executor, the session additionally
  serves `ListPrograms`, bounded `ListSafePoints`, `ValidateSafePoint`, and v5
  `SetBreakpoint`/`ListBreakpoints`/`ClearBreakpoint` from the exact tab-owned
  BlueJS registry. These return or retain opaque core-minted program IDs and
  instruction-boundary tuples only; they reject wrong tab, realm, program
  generation, and non-boundary inputs instead of remapping them.
  `ProgramLocations` and the deliberately narrower
  `BreakpointConfiguration` capability are available only at that live seam;
  the latter is an idempotent bounded table cleared at realm replacement, not
  a general VM interruption mechanism. With either `--inline-bluejs` or the
  trusted out-of-process child route plus the debugger socket, v5 also offers
  `ArmRootSafePointBreakpoint`,
  `GetExecutionState`, and `ResumeExecution` for a pending classic
  declaration's exact root-code-unit boundary; `ArmEntryBreakpoint` remains
  the zero-offset compatibility form. A non-entry arm runs BlueJS to that
  exact compiler-verified boundary and retains the root interpreter frame
  (operand stack, bindings, scopes, handlers, iterators, completion state,
  and GC roots) until the owner-session scheduler resumes it. A successful
  `ResumeExecution` exposes only the source-free `Resuming` state for one
  bounded session turn before an idle scheduler turn resumes that frame; it is
  observability, not an execution lease. It exposes no source, bytecode,
  stack, scope, object, or completion value. Modules,
  top-level await, child function code units, re-arming/loop hits, stepping,
  arbitrary nested-frame interruption, exception, and runtime-value features
  remain planned. The real-process regressions verify the v4 `ArmEntry`
  compatibility route and the v5 `Hello`/discovery/`Pending`/non-entry-root-
  arm/`Paused`/`Resuming`/same-frame-completion route. The latter rejects
  cross-program and invalid-state resumes without consuming a pending
  declaration, proves child-code-unit, module, repeat-arm, and stale-realm
  targets fail closed, and checks that its debugger replies never reflect
  fixture source/completion data, VM values, or BlueJS opcode data.
- A launcher real-process regression combines `--out-of-process-bluejs` with
  `--debugger-socket` and uses only the public browser and debugger listeners.
  It navigates two local HTTP documents containing one classic declaration,
  discovers only opaque realm/program/safe-point records through the
  launcher-selected core/child route, then proves the first document's realm,
  program, and safe point all fail with `StaleRealm` after the HTTP reload.
  It does not know, configure, or contact the launcher's private child socket
  or capability.
- A parsed `Page` now discovers the explicit non-portable `application/x-blueice-typescript` and `application/x-blueice-typescript-module` declarations in document order, retaining inline source or an external `src` as data. It never grants loading authority or evaluates them: a future page loader must apply origin, feature-profile, integrity, resolver, and resource policy before assembling the `AuthorizedModuleLoader` for direct admission.
- As a deliberately bounded normal-page fixture seam, `DirectPageScriptHost::execute_inline` may admit one inline declaration only. It derives a canonical module ID from core tab/document-generation/declaration identities and creates a one-module closed loader, so it neither embeds caller text in an identity nor reads an external source. `DirectPageInlineExecutor` can invoke that seam automatically only when a core owner explicitly selects it. By default it rejects and reports external `src` without reflecting the page-controlled URL; `PageScriptSourceAuthorizer` is the sole optional core-owned authority that can turn that declaration into a supplied closed graph plus resolver fingerprint. The executor never fetches, resolves, or falls back itself. `HttpOutOfProcessPageScriptSourceAuthorizer` also implements that direct BlueTS-only authorizer interface, reusing its immutable origin rule, URL/SHA-256 manifest, response checks, static-graph parser, limits, cache key, and resolver fingerprint rather than maintaining a second HTTP policy. The direct executor still receives only a completed graph and redacts every authorization failure to its existing source-free report category. A real core-session/loopback-HTTP regression covers document navigation followed by the manifest-authorized external module; a cross-origin declaration is rejected before fetch without URL reflection. This is a trusted in-process core-construction seam only: the reference binary has no HTTP-authorizer CLI/profile or page/frontend/MCP configuration path. General deployment policy and remaining host bindings remain open.
- The out-of-process portion now has a deliberately narrow shared-realm
  bridge: `OutOfProcessJavaScriptPageExecutor`, selected only when core
  receives both a trusted child endpoint and its per-spawn capability token,
  inventories supported JavaScript and explicit BlueTS declarations under one
  DOM-order ordinal sequence, and turns each inline record into a core-minted
  closed one-module graph. The private v5 child protocol carries a trusted
  language tag; JavaScript follows the normal BlueJS parser while BlueTS uses
  only an `AuthorizedModuleLoader` derived from that graph and child-fixed
  checked options before `blueice-bluets-bluejs` directly attaches it to the
  same `BlueJsPageRuntime`. It redacts child outcomes into separate existing
  JavaScript/BlueTS report lanes and sends exact realm closes on navigation or
  tab removal. Its default constructor rejects external `src` without
  reflecting it. The separately named startup-only constructor accepts one
  immutable core-owned `OutOfProcessPageScriptSourceAuthorizer`; it receives
  the exact tab/document-generation/DOM ordinal/parser language-and-kind/raw
  URL/`src` tuple and may return only an existing typed closed JavaScript or
  BlueTS graph. Core checks the returned language, copies its exact canonical
  IDs, static edges, source bytes, and resolver fingerprint into
  `PageHostModuleGraph`, then rejects a malformed graph or authorizer failure
  source-free. The child revalidates the graph and never fetches, resolves a
  URL/import map, reads a filesystem, or gains the authorizer's cache,
  integrity, or network authority. A missing static edge has no fallback.
  `SpawnedBlueJsHost::spawn_for_core` preserves launcher supervision while
  handing that configuration to one trusted core. `blueice-launcher
  --out-of-process-bluejs` creates the child itself and retains it as part of
  the core generation lifecycle; cutover creates a fresh child/capability pair
  for the replacement core. Before realm replacement, core serializes exactly
  two validated primitive snapshots for ordinary JavaScript:
  `blueiceDocumentText()` (1 MiB) and `blueiceDocumentOrigin()` (4 KiB,
  canonical HTTP(S) tuple). The child repeats those limits and canonical
  spelling checks, installs no other binding, and destroys both copies on
  navigation or close. It receives no DOM object, URL, resolver, fetch/cache,
  IPC, or page-selected capability. `blueice-bluets-bluejs` owns the one fixed
  generated `core-script-document-context-v1` artifact shared by the core
  catalog and child: its byte-checked declaration and exact callback inventory
  make only `blueiceDocumentText(): string` and
  `blueiceDocumentOrigin(): string` ambient to child BlueTS. A missing, extra,
  renamed, or page-selected binding fails closed before compiler admission;
  `document`, `fetch`, URL, resolver, and object APIs remain untyped and
  unavailable. A real launcher-to-core-to-child regression observes both
  classic and ESM BlueTS report lanes in that shared realm and verifies a
  wrong callback arity becomes only the fixed source-free compilation
  category. This still is not a general DOM surface.
  `HttpOutOfProcessPageScriptSourceAuthorizer` now creates the closed graph
  itself under a fixed startup policy: canonical same-document origin or one
  canonical owner-selected origin, a URL/SHA-256 manifest, and fixed resource
  bounds. It accepts only a direct `200`, identity-encoded UTF-8 response with
  a language-approved MIME type and matching bounded `Content-Length`; it
  rejects redirects, missing manifest records, bad integrity, ambiguous/bare
  static URL forms, and over-depth/count/byte graphs. It parses manifest-
  covered JavaScript and BlueTS static edges, privately caches only verified
  bytes under deterministic URL/integrity/language keys, and derives a
  policy-complete resolver fingerprint. Core copies only the finished graph
  into the child protocol, while the child still has no fetch, URL-resolution,
  import-map, filesystem, cache, manifest, or fallback authority. The real
  core lifecycle can select exactly one compiled
  `core-page-http-fixture-v1` profile through a private launcher-to-core
  startup selector: it fixes one same-origin classic JavaScript path and its
  SHA-256 expectation, then constructs the regular HTTP authorizer inside
  core before any page executes. Neither the public launcher CLI nor that
  selector accepts URLs, manifests, resolvers, source, paths, or fetch
  settings; the per-document origin only instantiates the fixed same-origin
  policy. A subprocess regression crosses core, a real HTTP origin, and the
  supervised child to prove the finished graph handoff. This is a bounded
  integration fixture, not a general application-resource profile. The v8
  channel additionally carries source-free debugger-location operations (list
  retained programs, list a fixed bounded set of compiler-recorded safe
  points, and validate one exact tuple) and a 256-record exact-breakpoint
  configuration table. The child mints private IDs only; core verifies an
  exact live child realm, remints every public program handle/generation in a
  disjoint core namespace, and rejects mismatched tab/document/program/safe-
  point replies. Set/clear must echo the complete child-private safe-point
  tuple; core rejects duplicate or unmapped list records and revalidates each
  one before reminting a public record. Replacement and close discard both
  private and core mappings. The route deliberately does not proxy
  generic interruption, stepping, VM frames, stacks, scopes, values, bytecode,
  or source. A core-owned debugger socket selects the otherwise default-off
  document lifecycle, which defers document-order declarations for one turn
  and can arm a pending
  classic program at one exact root-code-unit point. It reports only
  `Pending`/`Paused`/`Resuming`/`Completed`; core revalidates the paused private
  tuple before reminting it publicly, and only a later core-owned advance turn
  resumes the same root frame. `ArmEntryBreakpoint`, modules, child code units,
  re-arms/loop hits, and nested interruption remain unavailable.
  Out-of-process discovery/configuration can consume at most 64 such deferred
  turns for a document, after which core advances it even if the peer keeps
  querying. DOM/event callbacks, URL or import-map resolution, broader
  deployment HTTP policy, and page-selected compiler
  profiles remain open. The launcher owns an optional stable public debugger
  listener, but no debugger session or opaque handle survives a core cutover:
  each pre-cutover stream remains pinned to its old private core peer and
  closes fail-closed when that generation ends.
- [`bluets-test-interface`](TEST_INTERFACE.md) now exposes the same persistent JSON-lines ready/request/reply transport as BlueJS's test adapter. It is intentionally compile-only, accepts BlueJS's `sloppy` mode as a `raw` alias, and has stable BlueTS diagnostic codes/spans and caller-controlled compiler limits; Test262 runtime execution remains a future bridge concern rather than a hidden BlueJS dependency.
- The [BlueTS test report](TEST_REPORT.md) records the complete per-platform test-suite results, the TypeScript 5.9.3 oracle matrix and per-file line coverage for `blueice-bluets` and `blueice-bluets-bluejs` (2026-09-21).

This is deliberately not a claim of general `tsc` compatibility. Control-flow narrowing, overload resolution, decorators, enums, classes, TSX, namespace emission, parameter properties, and arbitrary JavaScript expression typing remain pending. A construct outside the implemented matrix must be added with a parser/checker/emitter test and a precise compatibility entry; it must not be advertised merely because its tokens happen to be erasable.

BlueTS is a Rust front end feeding the existing BlueJS bytecode compiler and VM. It is **not** a second JavaScript/TypeScript VM, an embedded `tsc` process, or a type-tagged replacement for BlueJS values. TypeScript's ordinary type system is compile-time only: the checker uses it to accept or reject a program, while BlueJS executes ordinary ECMAScript values. The [official TypeScript documentation](https://www.typescriptlang.org/docs/handbook/typescript-from-scratch) calls this "erased types" and states that types do not change JavaScript runtime behavior. BlueTS follows that compatibility rule for ordinary TypeScript, then adds an explicit BlueIce contract mode at foreign-data boundaries rather than silently making every local operation dynamically typed.

This phase depends on Phase 13 completing the BlueJS host, page-script pipeline and an end-to-end `<script>` fixture. It also depends on Phase 17's source-location debugger capability. It must not delay either prerequisite by pretending that a TypeScript parser can substitute for a correct ECMAScript engine or page-script host.

## Current-state correction

The current BlueJS parser accepts ECMAScript grammar, not TypeScript grammar. A colon annotation such as `const value: number = 1;` is a syntax error. The library example reads arbitrary source text and immediately calls BlueJS `parse`, so a `.ts` filename is not a TypeScript mode switch. The Phase 13 host/core wiring and real script-page test are also still open. BlueTS therefore begins after a working JavaScript path exists; it is not a way to claim current page-script support early.

References to TypeScript elsewhere in the plan refer to external automation clients or Node tooling, not page-language support. In particular, Phase 17's planned `@blueice/automation` client is TypeScript *client code* and must not be confused with executing TypeScript in a page.

## Decisions

### One front end, one runtime

Add a proposed `backend/bluets` Rust crate (`blueice-bluets`) with one shared TypeScript front end and lowering IR. Its in-process use in the long-lived BlueJS host permits a typed source program to lower directly to BlueJS's ECMAScript AST or bytecode input; it does not serialize generated JavaScript text and parse it a second time. The same crate also backs the standalone BlueTSC emitter. BlueTS has no network, filesystem, DOM, capability, or script-evaluation authority of its own. It receives source and host-resolved module records from the same page-script loader as BlueJS, consumes the same per-tab compile/memory budgets, and cannot bypass the script IPC, gatekeeper, origin, or module-resolution policies.

The execution path is deliberately narrow:

```text
TS/TS module source
  -> tokenize + parse -> bind -> type check -> shared lowered IR
  -> page path: BlueJS Program / bytecode -> BlueJS VM
  -> build path: BlueTSC ESM emitter -> .js + optional .js.map / declarations
  -> BlueTsDebugInfo + optional contract plans
```

The direct page path has no `TS -> emitted .js text -> parse .js again` round trip. BlueTSC is the deliberately separate build path that serializes the same lowered IR as JavaScript for use outside BlueIce. A test oracle may compare results with a pinned external TypeScript compiler, but Node.js and `tsc` remain test-only tools, never runtime dependencies.

### BlueTSC: standalone JavaScript emitter

`bluetsc` is the proposed BlueTSC command-line compiler. It invokes the same parser, binder, checker, resolver, lowering rules, contract planner and source-span machinery as BlueTS; it must not fork a second, subtly incompatible TypeScript implementation. It offers a build-time alternative for authors who want their project to run in ordinary JavaScript hosts, publish JavaScript packages, inspect emitted code, or move TypeScript checking out of page-navigation startup.

BlueTSC's default output is standard ECMAScript modules (`.js`) preserving ESM import/export semantics, accompanied on request by `.js.map` and `.d.ts` artifacts. Type-only imports/exports, interfaces and type aliases do not appear in emitted JavaScript. The emitter supports only an explicitly declared ECMAScript target matrix; it neither silently produces CommonJS nor claims that every TypeScript feature can target every JavaScript version. Resolution, import-map and declaration policies match BlueTS's declared project configuration, so a source graph cannot type-check one way in the browser and emit another way on the command line.

`bluetsc check` performs parse/bind/type/resolution validation without writing artifacts. `bluetsc build` type-checks first, lowers once, stages all output and publishes it atomically only if every selected entry and dependency succeeds. The default is equivalent to `noEmitOnError`: no stale or partial JavaScript output is described as a successful build. The initial implemented CLI accepts either `check|build <entry.ts>` or `check|build --config bluetsc.json`. A config has `entries`, optional project-root-confined `outDir`, `sourceMap`, `declaration`, `target`, `runtimePolicy`, and `imports`; a config invocation rejects extra CLI overrides so the recorded settings cannot drift. Its import map permits only exact file keys or trailing-slash prefixes to existing project-root-confined local source files/directories. A build always includes root-relative `bluetsc.manifest.json`; a configured map additionally produces the standard-shape `bluetsc.importmap.json` for the resulting ESM tree. This remains a deliberately small resolver, not Node/package-manager resolution.

BlueTSC's source maps map emitted JavaScript back to original TypeScript spans using the same provenance records that make BlueTS debugger locations correct. Declarations describe the checker-approved public surface, not a claim that an unimplemented BlueIce host API exists. JavaScript and declaration outputs carry a compiler/host-typing/configuration fingerprint; consuming an artifact built under a weaker contract policy must be observable and rejectable by a strict project policy.

For a `strict-runtime` project, BlueTSC lowers the same explicit boundary contracts as the direct BlueTS path. It either emits a deterministic, versioned contract helper/module with the bundle or rejects an output form unable to carry the required validators; it never strips the checks merely because the target is JavaScript. `checked` and `transpile-only` outputs are labelled in build metadata and DevTools/source-map extensions so that a deployment cannot be mistaken for strict-runtime output.

### Explicit BlueIce opt-in, not a claim of web compatibility

TypeScript is not a browser script language. BlueTS initially recognizes only these BlueIce-specific script kinds:

```html
<script type="application/x-blueice-typescript" src="app.ts"></script>
<script type="application/x-blueice-typescript-module" src="app.ts"></script>
```

The suffix alone never selects BlueTS, and `text/typescript`, TSX, or a JavaScript MIME type is not silently reinterpreted. The classic/module distinction follows the corresponding BlueJS realm and module rules; fetching, integrity, origin and policy checks remain the page loader's responsibility. These script kinds are deliberately non-portable: an author who needs ordinary-browser compatibility must precompile TypeScript to JavaScript.

The type-safety policy is core-owned configuration attached to the script request or a trusted application manifest. A fetched document cannot weaken it with an HTML data attribute. The initial product policy is `strict-runtime` for direct BlueTS pages; `checked` and `transpile-only` exist only as explicit operator/development choices and are visibly reported to DevTools and automation.

### Static semantics are required, not a cosmetic stripping pass

BlueTS's normal path implements a TypeScript-compatible parser, binder, module resolver and type checker for its declared compatibility version. It resolves source declarations, control-flow narrowing, generic instantiations, overloads, structural assignability and diagnostics before emitting executable bytecode. A parse, resolution, type, lowering, resource, or contract-planning error prevents that script/module from executing; BlueIce never falls back to treating rejected TypeScript as JavaScript or executes a partial output.

`transpile-only` is retained solely for controlled migration tests. It may erase only declared supported syntax, but it is not advertised as type-safe TypeScript support and cannot be the default direct-page mode.

The compatibility target is pinned by a `BlueTsLanguageVersion` and an explicit feature matrix. The first implementation need not promise bit-for-bit `tsc` compatibility. Each accepted syntax, checker rule, emit rule and diagnostic class must name its supported version and tests; unsupported syntax produces `UnsupportedSyntax`, not an accidental BlueJS parser error. BlueTS's runtime target is additionally bounded by the BlueJS ECMAScript feature matrix. A TypeScript construct may type-check yet be rejected as `UnsupportedRuntimeTarget` until BlueJS can faithfully execute its lowered ECMAScript semantics.

BlueTS owns a small `lib.blueice.d.ts` generated from the actual BlueIce DOM, event and network bindings. It must not claim the full web platform merely by importing a broad `lib.dom.d.ts`: declarations for APIs that BlueIce does not expose would make a successful type check dishonest. Browser URL modules and explicitly configured import maps are the initial resolution model. Node resolution, CommonJS, `node_modules`, arbitrary package-manager hooks and arbitrary remote `.d.ts` acquisition are out of scope.

### Runtime contracts protect data boundaries, not every instruction

Static checking proves properties only under the program's declared assumptions. Network JSON, URL/query data, storage, `postMessage`, extension/native IPC, foreign JavaScript module exports and user-provided values can violate those assumptions. In `strict-runtime` mode BlueTS requires an explicit, reifiable `RuntimeContract` for every type-bearing ingress and egress across those boundaries. A TypeScript assertion (`value as User`) never validates data; unchecked `any` is not an escape hatch in strict mode.

BlueTS may synthesize contract plans for a defined subset of types: primitives and literals; arrays, tuples and readonly collections; finite object records with required/optional fields; tagged unions; named recursive interfaces through a cycle-safe contract table; and generic declarations only when every runtime parameter has a supplied reifiable contract. Validation reports a bounded path, expected contract identifier, observed value category and source location without serializing sensitive input by default.

The following are not automatically reifiable: `any`, unconstrained or erased generic parameters, arbitrary conditional/mapped types, opaque/branded intersections, declaration-only ambient values, and types whose validation would require invoking an arbitrary getter, proxy, or user function. BlueTS rejects their automatic use at a strict boundary and requires a developer-supplied contract with a reviewable validator. Contracts must be pure, resource-bounded bytecode/data plans; they may not call arbitrary page code while inspecting untrusted data.

A developer may choose a registered JSON Schema as the data-plan representation for a JSON-like boundary contract. In that case BlueTS delegates compilation and validation to Phase 19's single `blueice-json-schema` engine rather than translating a schema into an ad-hoc second validator. The contract records the immutable `SchemaResourceId`, dialect, vocabulary set, registry generation and validation-plan hash in `BlueTsDebugInfo` and the BlueTSC build fingerprint. A `$schema`, `$id` or `$ref` present in page data never causes a network/file lookup or weakens the originating page's contract policy; only an operator-registered, pinned schema resource can be used. JSON Schema validates data shape at the boundary—it does not infer arbitrary TypeScript semantics, execute format/content handlers, or make a static `TypeId` into a runtime tag.

The common pattern is consequently explicit and honest:

```ts
interface User { id: string; displayName: string }

const user = await fetchJson("/api/me", User.contract);
// `user` is `User` only after the response has validated.
```

After a value passes its boundary contract, local code relies on the static checker and carries no universal runtime type tag. This concentrates dynamic cost where reality introduces untrusted values instead of checking every addition, property read, or function-local assignment. Contracts provide data-integrity checks, not a sandbox: a malicious script that already executes in a page realm is still constrained by BlueIce's origin, capability and gatekeeper policies.

### Type-aware debugger metadata is a required compiler product

Type erasure at the VM boundary does not justify discarding compiler knowledge. Each successful BlueTS compilation emits immutable `BlueTsDebugInfo` keyed by source identity, source-content hash, language version, compiler-option hash, host-typing ABI, contract-policy version and BlueJS bytecode ABI. It contains:

- original TypeScript source spans and a bytecode-safe-point map;
- module URLs, inline-script provenance and source-content hashes;
- `SymbolId` records for declarations, references, scopes and renames;
- interned `TypeId` records for declared and inferred expression types, generic substitutions and diagnostic messages;
- source-level scope membership and lowered-variable correspondence;
- contract IDs, their boundary sites, and validation-failure locations.

For a JSON-Schema-backed contract, the last group additionally links the pinned schema resource, dialect/vocabulary report and standard validation output locations. Phase 12 exposes that data as a bounded contract/schema artifact so an AI and a human diagnose the exact same failed pointer/keyword without serializing or executing the rejected value.

Phase 17's debugger maps breakpoints, stepping, stack frames and exceptions back to TypeScript locations. A scope/watch display shows the runtime BlueJS value separately from its static TypeScript type; it must never claim that a static `TypeId` is a runtime proof. Watch/evaluate expressions are parsed and checked in the paused source scope before BlueJS compilation, with the same fuel, controller-lease and audit requirements as ordinary debugger evaluation. A generic type's displayed instantiation is the checker result at that source site, not invented runtime reification.

For a BlueTS script, source maps are therefore a release prerequisite rather than a deferred DevTools nicety. Debug metadata is retained according to the script/debugger memory policy, released with its tab realm, and redacted from unprivileged automation clients just as source text and scopes are. The same metadata backs the target-aware AI MCP debugger: [Phase 12's debug-environment contract](../phase-12-mcp-server/DEBUG_ENVIRONMENT.md) exposes it only through generation-bound, paged, capability-scoped source/type/symbol/contract resources and tools.

### Performance and cache contract

Type checking is real work; BlueTS does not hide or repeat it unnecessarily. A source/module compilation cache stores bytecode, debug metadata, contract plans and dependency fingerprints under the complete key above plus source bytes. A stale dependency, declaration, import-map, option, policy, host-typing or bytecode-ABI change invalidates the dependent entry. Content-addressed output is immutable; a cache hit never reuses bytecode checked under a weaker policy.

Development mode maintains an incremental module graph and rebinds/rechecks only affected strongly connected components. Production may precompile/cache a verified graph before navigation. A type check is never performed in a VM hot loop. Contract plans are compiled once and applied only at declared crossings; validator work has explicit recursion, byte, collection-length and fuel limits. Phase 8 attributes parser, checker, metadata, cache and validator allocations to the initiating tab.

BlueTS cannot make a bytecode interpreter as fast as ahead-of-time native code. BlueJS interpreter dispatch, DOM IPC, allocation and host calls remain independent performance work. BlueTS's performance objective is instead to avoid duplicate parsing, avoid type work in hot execution paths, keep contracts boundary-local, and make cold compilation/cache costs observable in DevTools.

### Safety, analysis and resource behavior

The Phase 7 gatekeeper receives both the source-level BlueTS capability summary and the post-lowering BlueJS capability summary. Approval is based on the executable lowered behavior; source-level types, declarations and contracts add explanation but cannot hide a network call or DOM mutation introduced by a lowering transform. The summaries are linked by source span and transformed-node identity so an audit can identify their origin.

All tokenizer/parser/checker/contract operations enforce source-size, token, AST-depth, type-instantiation, union/intersection expansion, module-graph, diagnostic, compile-time and metadata budgets. Exceeding one produces a defined resource failure without executing the script, preserving the existing page/document state. BlueTS never resolves imports through arbitrary filesystem access or a checker-owned network request. It accepts only loader-supplied, origin/policy-approved module records.

## Compatibility levels

| Level | Static checker | Runtime contracts | Intended use | Promise |
| --- | --- | --- | --- | --- |
| `transpile-only` | No | Explicit contracts only | Controlled migration and compiler tests | Syntax conversion only; never advertised as type-safe. |
| `checked` | Required | Explicit opt-in contracts | Compatibility/performance measurement under operator control | Type errors prevent execution, but foreign values may remain unchecked. |
| `strict-runtime` | Required | Required at all declared foreign-data boundaries | Default direct BlueTS page mode | Static diagnostics plus validated ingress/egress for the supported contract subset. |

No level treats `as`, `!`, `any`, an unchecked cast, or a `.ts` extension as proof that a runtime value is safe. A page's active level and any unvalidated/unsupported boundary must be visible in its DevTools target metadata.

## Initial language boundary

The first executable slice targets type-bearing but eraseable syntax needed for ordinary typed page code: annotations, interfaces, type aliases, generics, union/intersection/literal types, narrowing, overload declarations, `readonly`, access modifiers, `as`, `satisfies`, non-null assertions, `declare`, `import type`/`export type`, named `export default function` declarations, `export default name` for a local runtime value, and local `export { name as publicName }` bindings. It emits only ECMAScript constructs currently supported by the selected BlueJS target.

The first slice explicitly rejects TSX/JSX, decorators, generic arrow functions, default-export expressions other than a local bare identifier and anonymous default functions, named value re-exports from another module, `enum`/`const enum`, runtime `namespace`/`module`, parameter properties, `import =`, `export =`, declaration merging with runtime emission, and custom transformers. Some are not erasable; for example, ordinary TypeScript enums emit runtime objects. They can be proposed later only with a precise lowering specification, BlueJS runtime support, debugger mappings, capability-summary coverage and differential tests. TSX additionally requires an explicit JSX factory/runtime and is a separate UI-framework decision.

## Delivery order and acceptance

### Slice 0 — JavaScript and debugger prerequisites

1. Complete Phase 13's BlueJS host/core wiring, DOM binding design and a real script-bearing page fixture.
2. Complete Phase 17's script source/safe-point debugger path and the source-map representation required by BlueTS.
3. Connect the shipped resolver-preserving `BlueJsProgramV1` script/module graph hand-off to an origin-preserving page AST/IR, with compiler/cache resource accounting and safe-point validation.
4. Generate `lib.blueice.d.ts` from implemented host bindings using the published host-typing version policy.

Acceptance: a JavaScript fixture executes through the page loader and debugger with a stable source location, while an unimplemented host API is absent from `lib.blueice.d.ts` and fails an attempted typed use.

### Slice 1 — checked TypeScript source and source-level debugging

1. Build the TypeScript tokenizer/parser, binder, URL-module resolver and checker for the initial boundary.
2. Extend direct lowering of supported typed syntax to BlueJS AST/bytecode input, without JavaScript-text round trips.
3. Reject type/resolution/lowering errors atomically and expose structured diagnostics through the script, debugger and automation APIs.
4. Emit `BlueTsDebugInfo`; prove TS source breakpoints, stepping, stack traces, scopes, symbol navigation and static-type displays map to the executing bytecode and to the Phase 12 AI MCP debug interface.

Acceptance: a local typed classic script and module execute with no generated `.js` file; a deliberate type error runs neither the entry nor dependent module; a breakpoint and exception point at original `.ts` lines; a debugger shows a static type and the separate runtime value.

### Slice 2 — BlueTSC JavaScript emission

1. Build `bluetsc check` and atomic `bluetsc build` on the shared checked/lowered IR.
2. Emit standard ESM JavaScript, source maps and the supported declaration output without reparsing or rechecking a different source representation.
3. Define reproducible output/cache fingerprints and ensure source/contract policies, host typings and resolver results cannot drift from direct BlueTS execution.
4. Make strict-runtime output retain or import its deterministic contract helper; reject targets that would silently remove required validation.

Acceptance: a checked local TS module produces deterministic ESM JavaScript and a source map pointing to original `.ts` lines; an error leaves the previous output intact and reports no successful artifact; the resulting JavaScript has the same supported observable behavior as direct BlueTS execution; a strict boundary check still fails malformed input after emission.

### Slice 3 — strict runtime contracts

1. Define the pure contract IR, its reifiable type subset, bounded validation engine and failure schema.
2. Add contracts for JSON/Fetch/XHR once Phase 17 supplies those APIs, then URL/query, storage, `postMessage`, foreign module and native/extension boundaries as each exists.
3. Make `strict-runtime` require coverage of each supported boundary; reject `any`/unreifiable types there unless a declared reviewed contract is supplied.
4. Attribute validator/cache/debug allocations to the tab and feed lowered contract behavior to the gatekeeper summary.

Acceptance: malformed JSON cannot enter a `User`-typed value; the validation error names the bounded failing path and source location; valid data reaches BlueJS without repeated local type checks; a recursive/deep/oversized input fails within the resource budget.

### Slice 4 — incremental projects and compatibility growth

1. Implement dependency-aware incremental checker/cache invalidation and trusted local declaration files. Both, plus external-oracle coverage and bounded erasable generic substitution, are complete in the standalone front end.
2. Expand the supported TypeScript feature matrix only alongside its BlueJS lowering/runtime, source mapping, contract and oracle evidence.
3. Compare the supported fixture matrix against a pinned TypeScript compiler for accepted syntax/diagnostics and add Node/BlueJS behavioral differential tests for the lowered output.
4. Expose carefully redacted project/type metadata to Phase 17 DevTools and the documented, capability-scoped Phase 12 MCP debug interface.

Acceptance: editing one module invalidates only its dependents; a cache entry checked under another policy is refused; every supported compatibility feature has parser/checker/lowering/debug/contract expectations and an independently checked fixture.

## Testing strategy

- Keep public parser, binding, checker, lowering, contract and debugger fixtures under the eventual `backend/bluets/tests/` boundary, with shared HTML page fixtures beside the Phase 13/17 integration tests.
- Test diagnostics by stable code/category, source span and semantic condition—not copied compiler-message wording. Every unsupported feature must have an explicit rejection fixture.
- Compare accepted syntax and checker behavior against the pinned TypeScript 5.9.3 reference in the CI BlueTSC compatibility-oracle job (`BLUEICE_BLUETSC_ORACLE=/absolute/path/to/tsc cargo test -p blueice-bluets --test typescript_oracle -- --ignored`); compare resulting runtime behavior against the reference JavaScript output where the selected BlueJS feature subset supports it. The reference never determines BlueTS's security policy or makes an unsupported test silently pass.
- Run BlueTSC's emitted JavaScript and direct BlueTS bytecode against the same deterministic fixtures; compare public results, exceptions, module order, source-map locations, contract failures and type-only import elision. Assert an erroneous multi-entry build publishes no partial artifact set.
- Test contracts with malformed, adversarial, recursive, cyclic, getter/proxy-like, deep, oversized and resource-exhausting inputs. Assert no arbitrary user code runs during a pure validation path.
- Test debugger round trips: source breakpoints, async/exception locations, renamed symbols, type displays, erased type-only imports, transformed spans, stale source-map rejection and privacy redaction.
- Test cache keys/invalidation, per-tab attribution, failed compilation atomicity, page/gatekeeper policy enforcement and multi-tab isolation through real processes once the host exists.

## Explicit non-goals for the first release

- Claiming full `tsc`/`tsserver` compatibility, all TypeScript syntax, all diagnostics, or all JavaScript package ecosystems.
- Using BlueTSC as an opaque wrapper around a bundled `tsc`, or permitting its emitter to drift from BlueTS's parser/checker/lowering rules.
- Replacing BlueJS with a TypeScript VM, preserving universal runtime type tags, or adding a type check to every local operation.
- Treating TypeScript types or contracts as a security sandbox, permission system, or substitute for Phase 7 review.
- Executing TSX/React, decorators, enums, runtime namespaces, CommonJS, Node builtins, arbitrary `node_modules`, arbitrary remote declarations, or custom compiler transformers.
- Advertising complete web-platform typings before the corresponding BlueIce host APIs and their runtime behavior exist.
- Exposing source text, static types, diagnostics, contracts or debugger scopes to an unauthenticated/unprivileged automation client.

## Checklist

- [x] Decide that BlueTS is a BlueJS front end, not a second VM or a `tsc` runtime process
- [x] Decide direct-AST/IR lowering, no emitted-JavaScript reparse path
- [x] Decide that BlueTSC is a shared-front-end standalone compiler emitting standard JavaScript rather than a second TypeScript implementation
- [x] Decide atomic no-emit-on-error builds, ESM-first output, source-map/declaration artifacts and strict-contract-preserving JS emission
- [x] Decide explicit non-standard BlueIce script kinds and a core-owned policy
- [x] Decide that full static semantics and TypeScript debugger metadata are required for supported direct pages
- [x] Decide `strict-runtime` boundary contracts as the default direct-page safety policy
- [x] Define the initial reifiable-contract boundary and explicit non-reifiable failures
- [x] Define cache keys, resource accounting, debugger metadata and gatekeeper visibility requirements
- [ ] Complete Phase 13 page-script host and Phase 17 source-map prerequisites
- [x] Create the standalone `blueice-bluets` crate and pin the initial `blue-ts-0.1` compatibility matrix
- [x] Publish the versioned BlueJS AST/IR hand-off, bytecode-safe-point-map and host-typing compatibility contract
- [x] Implement the first public BlueJS structured-program hand-off for bounded host-neutral classic-script and resolver-preserving ESM-module-graph subsets, without emitted-source reparsing
- [x] Generate and test `lib.blueice.d.ts` from actual host bindings using the published host-typing strategy
- [x] Implement the independent initial parser/binder/closed-module resolver/checker/type-erasure ESM emitter with atomic compile failures
- [x] Implement `bluetsc check`/staged `build`, ESM/column-provenance-source-map/declaration emission and reproducible artifact fingerprints for the initial matrix
- [x] Implement VM-independent `BlueTsDebugInfo` (source hashes, symbols, static types and spans); direct and isolated child page admission retain it only against an exact live BlueJS generation. Compiler/MCP provenance now uses a labeled SHA-256 digest, so the source-text-free compiler identity is collision-resistant; it remains a query-only metadata record, not source-read authority. Page-host v10 can enumerate a separately minted opaque metadata handle for an exact live BlueTS program, describe it with a bounded fingerprint/count summary, list only compiler-minted source-record IDs parent-bound to that handle, and—only under a distinct source-provenance request—describe one prior ID with canonical module identity plus the labeled SHA-256 digest. Debugger v9 defaults to a denied negotiated manifest; an owner must separately enable `OpaqueInventory`, dependent `OpaqueSummary`, dependent `OpaqueSourceInventory`, and/or `OpaqueSourceProvenance`, and a client must request the exact canonical set for the live realm. The public summary is handle/generation-bound and carries only language/options fingerprints plus source/type/symbol/contract counts; source inventory carries only numeric source IDs under that same handle; provenance is source-text-free metadata only. No debugger surface has source text, spans, names, type/symbol/contract records, bytecode, or values. Broader static-record disclosure, diagnostics, bytecode source mapping, and controlled debugger read exposure remain pending.
- [ ] Expose TypeScript diagnostics, symbols, types, contracts, lowering provenance and BlueTSC check/build through Phase 12's negotiated MCP debug interface. A core startup owner can now seal fixed closed projects before opening the query-only compiler listener and explicitly attach MCP's bounded v2 client; exact static source-hash provenance plus reifiable-local contract read/validation and exact-generation paged opaque-ID discovery are shipped. Phase 12 authorization, catalog distribution, lowering/bytecode provenance, broader capability/session negotiation, and output-write elevation remain absent.
- [x] Implement the pure runtime-contract IR and bounded JSON-like validator; the only installed page host result boundaries now have an explicit inventory and pre-admission primitive-string validation, while broader host-boundary discovery, JSON Schema delegation and page enforcement remain pending
- [x] Implement host-neutral dependency-aware incremental parser/checker cache invalidation; cache reuse is refused across compiler-policy changes and failed compilations preserve the last successful entry
- [x] Support root-confined, type-only local `.d.ts` modules without runtime emission or package/remote declaration acquisition
- [x] Bound the pure contract validator's depth, collection, node-fuel and string-byte work with caller-visible limits
- [x] Add an opt-in, TypeScript-5.9.3-pinned external-oracle job for the implemented initial matrix
- [ ] Add real-process page, debugger, contract, resource, policy and multi-tab regression coverage
