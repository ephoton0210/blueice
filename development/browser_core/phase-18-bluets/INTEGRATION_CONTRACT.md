# BlueTS ↔ BlueJS integration contract (v1)

[← Phase 18 plan](PLAN.md)

This document publishes the versioned boundary between BlueTS and BlueJS. `blueice-bluejs` now exposes a structured-program hand-off, and `backend/bluets-bluejs` uses it for bounded, host-neutral classic-script and resolver-preserving ESM-module-graph subsets. The boundary intentionally does **not** add a dependency from `blueice-bluets` to `blueice-bluejs`. The direct bridge has a limited, in-memory `bluejs-safe-point-map-v1` foundation for one attached source module's top-level lowering spans; a page host, complete page/module-graph map, and debugger attachment do not exist yet. BlueTS and BlueTSC remain usable as standalone, host-neutral front ends.

The keywords **MUST**, **MUST NOT**, **SHOULD**, and **MAY** express the contract's requirements.

## Versions and compatibility

`backend/bluets-bluejs` is the sole crate allowed to depend on both `blueice-bluets` and `blueice-bluejs`. Neither compiler may depend on the other. BlueJS owns the initial structured-program ABI identity, `bluejs-program-v1`.

| Surface | Current wire version | Owner | Compatibility rule |
| --- | --- | --- | --- |
| BlueTS language matrix | `blue-ts-0.1` | BlueTS | A host MUST reject a language version it did not explicitly enable. |
| Lowered program hand-off | `bluejs-program-v1` | BlueJS ABI, BlueTS lowering adapter | Major/version mismatch rejects before VM compilation. |
| BlueTS static debug metadata | `blue-ts-debug-v1` | BlueTS | May be consumed without a VM; it has no bytecode offsets. |
| Bytecode safe-point map | `bluejs-safe-point-map-v1` | BlueJS ABI, bridge | It is valid only for the exact generated program. |
| Page-realm lifecycle | `bluejs-page-runtime-v1` | BlueJS host foundation | A handle belongs to one caller-authorized tab/origin realm and expires on navigation/reload/close. |
| Host typings manifest | `blueice-host-typings-v1` | Page-script host | Compiler and host schema hashes MUST match in direct-page mode. |

Every direct-page compile request MUST carry all of the following:

```text
language_version
bluejs_program_abi
resolver_fingerprint
compiler_options_fingerprint
host_typings_abi + host_typings_schema_hash
source_set_hash
```

`compiler_options_fingerprint` includes resource limits, target, runtime-policy, resolver identity, source-map/declaration modes and the language version. `source_set_hash` is a deterministic hash of each canonical module identity and its exact bytes. A cache hit, a debugger request, or a safe-point map lookup MUST reject rather than silently reuse a value when any of these identities differ.

An incompatible major ABI, an unknown required field, a source hash mismatch, or a different module set is a compile/attach failure with no partial execution. Additive optional fields may be accepted only when the receiver's minor-version policy explicitly lists them; an unrecognized field is never silently interpreted as executable behavior.

Before invoking BlueTS, a direct-page host MUST assemble a closed source graph
from caller-authorized canonical module records and explicit resolution edges.
`blueice_bluets::AuthorizedModuleLoader` is the reference in-memory carrier for
that boundary: it validates duplicate or dangling records on construction,
loads only a supplied canonical ID, and resolves only an exact supplied
`(from-module, specifier)` edge. It performs no filesystem, URL, package, or
relative-resolution fallback. The host remains responsible for canonicalizing
those records, enforcing origin and capability policy, and placing its own
resolver identity in `CompilerOptions::resolver_fingerprint`; the loader does
not claim that an arbitrary map is an authorized page load.

For the initial direct classic-script slice,
`DirectScript::attach_in_page_realm` submits the already-lowered
`BlueJsProgramV1` to `BlueJsPageRuntime` under one caller-authorized tab and
origin. It then verifies that the live generation has the direct artifact's
sole canonical source identity and bytecode before publishing provenance or a
safe-point map. A failed provenance or static-debug attachment MUST discard
that generation through the page runtime, including its bytecode accounting;
it MUST NOT leave an executable but unpaired direct program behind. This is a
host-neutral admission seam only: it neither discovers a page script nor
installs host bindings or page lifetime automation. `DirectModuleGraph` applies
the same source/bytecode/provenance check transactionally to every closed
runtime ESM module, then calls `BlueJsPageRuntime::execute_module_graph` with
only those attached canonical module IDs. Navigation/reload/close invalidation
therefore makes every graph handle unusable instead of resolving an import
again under a successor page policy. Its optional static-debug admission
creates one module-local `BlueTsDebugInfo` record per live generation: the
record's source, symbols, and referenced type IDs match that module's
single-source safe-point map. A rejected record forgets every earlier graph
record and discards every graph program; it never leaves a partially
debuggable ESM graph. `DirectPageRealmOwner` owns both the page runtime and
static registry for a language-side host adapter, and prunes invalid records
after its navigation/reload and close operations. It has no page discovery,
DOM binding, transport, cache, or hibernation authority; a real host must
route those lifecycle events through this owner or enforce the same rule.

`blueice_engine::script::direct_page::DirectPageScriptHost` is the first
core-side adapter for that lifecycle rule. A direct request carries a typed
`TabId`; it MUST NOT carry a caller-selected origin. Before an attachment, the
adapter resolves the live `Page` from `TabManager`, accepts only a loaded
HTTP(S) URL, and derives the network layer's canonical tuple origin. It records
the page's core-private document generation alongside that origin. A changed
generation MUST call realm navigation even if the tuple origin is unchanged,
so a same-origin document replacement cannot retain the preceding document's
programs or static metadata. A missing tab, URL-less page, or unsupported
scheme closes any tracked realm for that tab and fails the request. A
lifecycle owner MAY call `synchronize_tabs` to release realms for closed tabs;
the optional
`run_session_with_script_requests_and_direct_page_host` entry point invokes it
after each session batch. The default session and production binary do not yet
construct a host, so this is not a claim of automatic HTML page-script
execution or launcher-managed process wiring.

The parsed core `Page` exposes `BlueTsPageScriptDeclaration` values in document
order for exactly `application/x-blueice-typescript` (classic) and
`application/x-blueice-typescript-module` (module). A declaration retains
either inline source or its raw external `src`; it MUST NOT cause a fetch,
filesystem read, canonical module-ID minting, resolver construction, profile
selection, or program admission by itself. A page loader must perform those
policy steps and submit an `AuthorizedModuleLoader` to the host before any
declaration can execute. Plain JavaScript and `text/typescript` are not an
implicit BlueTS opt-in.

One page realm owns at most one live or previously linked program for each
canonical ESM module ID. BlueJS retains module cells by that ID, so a second
artifact with the same identity is rejected even after its original handle was
discarded; only navigation/reload produces a fresh module identity set. This
prevents a newly attached artifact from executing against cells linked from an
older graph.

## AST/IR hand-off

The bridge lowers `new Identifier(args)` directly to BlueJS `New` data, including normal and spread arguments. Constructor members and omitted parentheses remain excluded.

Direct calls may target identifier, dot-member, or bracket-member expressions and accept normal or spread arguments. Member calls preserve the BlueJS receiver reference; optional calls remain excluded.

Named local functions may declare optional identifier parameters and a final identifier rest parameter. An optional parameter without a default lowers to an ordinary BlueJS parameter; the BlueTSC function-body scope gives it type `T | undefined`, while its direct-call signature accepts omission or explicit `undefined`. The direct bridge lowers a final rest parameter to the BlueJS rest-parameter AST shape; BlueTSC's bounded call rule uses an explicit `T[]` annotation to check every supplied tail argument and infer a generic `T`.

Named local functions may also declare default parameters whose initializer is in the direct expression subset. BlueTSC retains the original initializer tokens, checks a typed initializer against its parameter annotation, and BlueTS-to-BlueJS lowers it to BlueJS `Param::default`.

Named local function bodies may contain initialized local declarations, semicolon-terminated direct-expression statements, and `return` statements in source order. The bridge lowers each structured expression statement directly to BlueJS `Stmt::Expr`, preserving assignment and call side effects without reparsing emitted JavaScript. BlueTSC applies its existing bounded direct-call, property-access, and arithmetic checks to those statements. Control flow, exception handlers, and all other unstructured body syntax remain opaque and are rejected by this v1 direct bridge.

Named local function bodies may also use `throw expression;`. BlueTSC retains and applies its existing bounded expression checks to the throw value, rejects an omitted value or a line terminator immediately after `throw`, and directly lowers it to BlueJS `Stmt::Throw`. BlueJS preserves the resulting thrown JavaScript value. `try`/`catch`/`finally` and all other exception control flow remain opaque and rejected by this v1 bridge.

Named local function bodies may use a direct-expression `if` condition with braced consequent, braced `else if` branches, and an optional braced `else` body. BlueTSC retains the condition and each branch as structured body items; the bridge lowers braced bodies to explicit BlueJS blocks and each `else if` to a direct nested `Stmt::If` alternate, avoiding an artificial block scope. The checker recursively applies its existing bounded direct-expression checks to every condition and branch without claiming control-flow narrowing. Unbraced branches, an `else if` with an unbraced branch, loops, and every other control-flow form remain opaque and rejected by this v1 bridge.

The current direct expression subset supersedes earlier phase summaries: literals and identifiers; quoted-string and template escapes; template slots containing supported direct expressions; direct calls and constructors with normal/spread arguments; unary, arithmetic, relational (`in` and `instanceof`), comparison, logical, conditional, assignment, and sequence expressions; non-hole arrays with spread elements; objects with identifier/string/numeric/computed keys, identifier shorthand, and spread properties; dot/bracket reads; property assignments, updates, and deletion. Nested templates, optional chaining, and object methods/accessors remain excluded.

For a checked direct call to a known local function, BlueTSC expands a spread argument only if it has a tuple type. This preserves fixed parameter arity and supports tuple spread calls; regular arrays, unknown values, and general iterable analysis remain outside the bounded rule.

Object literal keys may be identifiers, quoted strings, numbers, or direct expressions in `[...]`. Only identifier keys may use shorthand; methods and accessors remain excluded, while spread properties are supported.

The bridge tokenizes a template interpolation with BlueTS's lexer and lowers every expression in the current direct subset; nested templates and embedded syntax outside that subset remain unsupported.

Non-substituted template literals share the bridge's ordinary escape decoder and are never reparsed as generated JavaScript.

The bridge decodes simple quoted-string escapes (`\\`, quote, `\n`, `\r`, `\t`, `\b`, `\f`, `\v`, and `\0`) before constructing BlueJS string data. Hexadecimal, Unicode, legacy octal, and line-continuation escapes remain explicit direct-bridge exclusions.

Object literals accept identifier/string/numeric/computed `key: value` properties, local-binding shorthand `{ key }`, and spread properties. A computed key lowers its supported direct expression to BlueJS `PropertyKey::Computed`; methods and accessors remain outside the direct subset. BlueTSC merges fields from an identifier-bound known record spread source; computed keys, unknown/non-record spread sources, and general spread expressions retain the checker's bounded `unknown` result.

Array literals accept normal and spread elements, but holes remain excluded. BlueTSC infers a known `T[]` spread source as element type `T`; a non-array or unknown spread source remains `unknown` in this bounded rule.

The direct bridge also lowers simple and compound assignments plus prefix/postfix updates whose target is an ordinary dot or bracket property reference. Template, object, and member-call limits continue to apply independently.

The `delete` unary operator is lowered only when its operand is an ordinary dot or bracket property reference (without optional chaining). Identifier and non-reference delete operands remain excluded, preserving a deliberately narrow v1 boundary around BlueJS property-reference semantics.

The `in` and `instanceof` relational operators lower to the corresponding BlueJS binary operations. BlueTSC infers each supported expression as `boolean`; type narrowing and custom `Symbol.hasInstance` analysis remain outside this bounded static rule.

The program boundary is structured data, not generated JavaScript text. BlueJS owns `BlueJsProgramV1`, whose current variants wrap its public `Program` (classic script) and `Module` ASTs; its `compile` method dispatches to BlueJS's compiler. BlueTS lowers supported TypeScript syntax once through the bridge to that BlueJS-owned AST. BlueJS remains the authority for ECMAScript semantics, bytecode generation, realm ownership, GC accounting, capability summary and execution.

The shipped bridge accepts either one non-declaration source module or a closed ESM module graph in classic-script or ESM-module mode. It erases type aliases, interfaces and type exports; it directly lowers initialized `var`, `let` and `const` declarations; named local functions with required or optional identifier parameters, direct-expression defaults, and a final rest parameter; ordered local-declaration/direct-expression/throw/braced-`if`/braced-`else if`/`return` function bodies; and the authoritative current direct-expression subset described above, which supersedes historical phase-level lists. It MUST reject an unparenthesized mix of `??` with `&&` or `||`, and a unary expression used directly as an exponentiation base, matching ECMAScript's grammar restrictions; an explicit parenthesized expression is a valid boundary. In ESM mode, local named/default exports and runtime import bindings become BlueJS module entries. A graph request uses the exact canonical target that BlueTS retained from its caller-authorized resolver; it is not resolved a second time from the original TypeScript specifier. Only runtime-reachable modules are materialized, so type-only declaration modules remain static. The parser retains every unstructured function-body token as opaque data, and the bridge MUST reject it instead of silently dropping it. Re-exports and every other runtime shape outside the current subset likewise fail explicitly. The bridge consumes BlueTS's checked, tokenized declarations and **MUST NOT** call a BlueJS source parser on BlueTSC's emitted JavaScript. Its integration tests execute the resulting BlueJS bytecode, not a reparsed generated text artifact.

BlueJS now assigns stable executable AST node IDs (the structured-program root plus each statement/expression, in deterministic pre-order), but not general source spans. Its limited `bluejs-program-debug-v1` registry foundation lets a caller-authorized host install a structured program with an immutable canonical module/source-hash identity, receive a monotonically fresh program generation, enumerate root-first AST/code-unit IDs and instruction boundaries, and validate an AST node or `(code_unit, offset)` tuple against that exact live generation. The compiler additionally records each root statement's actual first instruction, or an explicit unbound result when it emitted none; the registry resolves a verified top-level AST statement only through that compiler-produced record. The host-neutral bridge hands off its direct script/module structured program and exact BlueTS-retained source identity without a generated-JavaScript parse; on attachment it pairs every ordered top-level BlueTS lowering span with the corresponding verified top-level BlueJS AST statement, then emits bound entries into an in-memory safe-point map or preserves an explicit unbound result. Replacing or invalidating a program removes its old generation, so stale IDs fail closed rather than being reinterpreted for successor AST or bytecode. This foundation does not yet provide complete nested-expression provenance, a page lifetime, multi-module attachment ownership, a pause hook, or stack/scope inspection. Those remain required before direct-page source/debug attachment. The following logical envelope is therefore the target direct-page extension, not a representation currently transferred by the host-neutral script/module bridge:

The v1 logical envelope is:

```text
BlueJsProgramV1 {
  program_abi: "bluejs-program-v1",
  language_version: "blue-ts-0.1",
  compiler_options_fingerprint: String,
  source_set_hash: String,
  modules: [
    ModuleProgramV1 {
      canonical_module_id: String,
      source_hash: String,
      is_entry: bool,
      ecmascript_program: BlueJsAstV1,
      provenance: [BlueTsLoweringProvenanceV1]
    }
  ]
}
```

The future `BlueJsAstV1` is defined and versioned by BlueJS; BlueTS must never duplicate or privately reinterpret it. `BlueTsLoweringProvenanceV1` will associate an AST/IR node supplied to BlueJS with a half-open original TypeScript byte span:

```text
BlueTsLoweringProvenanceV1 {
  node_id: BlueJsNodeId,
  source: CanonicalModuleId,
  start_byte: u32,
  end_byte: u32,
  kind: Copied | ErasedTypeBoundary | LoweredSyntax
}
```

The source span refers to the exact UTF-8 source bytes supplied to the compiler; `end_byte` is exclusive. A type-only construct may have provenance but MUST NOT produce an executable node merely to make a debugger location. The adapter MUST preserve the host-authorized canonical module IDs, source hashes, origin metadata and module ordering. It MUST NOT reopen files, fetch URLs, invoke a package resolver, mint capabilities, or evaluate a module.

The v1 adapter accepts only BlueTS's documented subset. An unsupported lowering, a missing BlueJS feature, or a provenance node absent from the resulting program is an `UnsupportedRuntimeTarget`-class integration failure; it is not a request to emit JavaScript and parse it again.

## TypeScript span to BlueJS safe-point map

BlueTSC Source Map v3 is a portable JavaScript artifact. It is not a bytecode debugger map. The host-neutral direct bridge now produces the following generation-bound map in memory, but only for one attached direct source module and its top-level lowering spans. The full page-host map must extend this exact format rather than infer offsets from emitted JavaScript:

```text
BlueTsSafePointMapV1 {
  format: "bluejs-safe-point-map-v1",
  program_abi: "bluejs-program-v1",
  program_generation: u64,
  compiler_options_fingerprint: String,
  source_set_hash: String,
  entries: [SafePointEntryV1]
}

SafePointEntryV1 {
  code_unit: BlueJsCodeUnitId,
  bytecode_offset: u32,
  source: CanonicalModuleId,
  start_byte: u32,
  end_byte: u32,
  provenance_kind: Copied | ErasedTypeBoundary | LoweredSyntax
}
```

The tuple `(code_unit, bytecode_offset)` names a BlueJS-defined instruction boundary at which execution may safely pause. Entries MUST be sorted by `(code_unit, bytecode_offset, source, start_byte, end_byte)`, be unique for an identical tuple, and be validated by BlueJS against the generated code unit. `program_generation` is opaque and changes whenever that program is replaced; safe-point identifiers are not persisted across reloads or cache eviction.

Only source spans that explain a reachable instruction may be attached. An erased annotation can be recorded as nearby provenance, but MUST NOT become a standalone breakpoint target. The current direct attachment maps only a verified top-level AST statement to its compiler-recorded root instruction; if it emitted none, it retains an explicit unbound result. A future source-breakpoint resolver may select the nearest following permitted safe point under a declared same-module/range policy; if none exists, the debugger reports an unbound breakpoint. A stale request—wrong generation, ABI, fingerprint, source-set hash, code unit, or instruction boundary—MUST be rejected, never remapped heuristically.

Source locations use UTF-8 byte offsets at this boundary. BlueTSC's Source Map v3 conversion continues to use UTF-16 generated/original columns, including a single line transition for CRLF. Consumers must convert explicitly at the display boundary and must not mix these two coordinate systems.

## Host-generated `lib.blueice.d.ts`

`lib.blueice.d.ts` is a generated, host-supplied declaration root. It is not a hand-maintained substitute for `lib.dom.d.ts`, and it must describe only APIs that the current BlueIce page-script host has actually exposed.

The core script boundary now owns the initial declarative `HostTypeSurfaceV1`
schema and deterministic generator adjacent to the binding definitions that
will create each value. Each schema item records:

- the global/module/member name and TypeScript declaration;
- its value/type/namespace role and overload surface;
- capability, origin and runtime-policy requirements;
- feature flag and first host API version;
- stable documentation/diagnostic identifier.

The same schema generates both the runtime binding registry and the typings. The distribution contains these deterministic outputs:

```text
lib.blueice.d.ts
lib.blueice.manifest.json {
  format: "blueice-host-typings-v1",
  language_version,
  host_api_version,
  schema_hash,
  declaration_hash,
  enabled_feature_profile,
  binding_ids: [...]
}
```

The manifest is emitted with a stable field order, normalized LF text and no timestamps, checkout paths or machine-specific IDs. The `.d.ts` generator sorts declarations by stable binding ID and uses the same normalization rules, making the host-produced typing root reproducible. A direct BlueTS compile MUST be given this manifest and declaration source by the host's authorized module loader; it MUST reject an unavailable profile, a schema hash mismatch, or a declaration source whose bytes do not match the manifest. BlueTSC may consume an explicitly supplied local `.d.ts` module under its existing root-confined policy, but that does not claim a BlueIce host API.

The current `core-script-empty-v1` profile provides a checked-in,
byte-for-byte reproducible empty declaration root and manifest. Core's narrow
script IPC dispatcher is not a BlueJS object binding, so this profile MUST NOT
declare `document`, DOM node types, or any other global until the long-lived
BlueJS host installs it from the same runtime-binding inventory. The profile
catalog and artifact validator already reject unknown profiles, ABI/identity
and schema drift, binding-inventory drift, and declaration-byte drift without
fallback. The generated artifact additionally verifies that a host's installed
runtime registration records equal the schema-derived inventory regardless of
registration order; missing, duplicate, extra, or capability/identity-drifted
bindings are rejected. `verify_for_direct_compiler` now performs all of those
checks before producing one canonical `.d.ts` `ModuleSource` for
`CompilerOptions::ambient_declaration_modules`. BlueTS parses this source under
the normal source/module limits, includes its bytes in compiler fingerprints
and static metadata, forbids imports/re-exports from it, and exposes only its
static declarations; it produces no JavaScript or host capability. The
standalone `bluetsc` leaves this host-only input empty. This establishes direct
compiler consumption without advertising a page API before the BlueJS host has
installed matching bindings.

The direct bridge also retains `BlueTsDebugInfo` only through its exact live
BlueJS generation when a caller opts into `DirectDebugRegistry`. Retention
validates the safe-point map, language version, compiler-options fingerprint,
and canonical source hashes; it has explicit program/source/symbol/type limits
and stores no TypeScript source text or BlueJS runtime values. A host prunes
the record after BlueJS invalidation. This is static metadata only: it does
not provide page-lifetime automation, diagnostics/contracts retention, source
authorization, stack locations, scopes, runtime type inspection, pause
mechanics, or debugger IPC.

Adding a host API is additive only when it preserves existing binding IDs and declaration meanings. Removing or changing a public declaration requires a new host API major version and a new compatible feature profile. A compiler may target a declared older profile only when the host explicitly supplies its matching generated manifest; it may never infer API availability from the installed BlueJS version.

## Remaining implementation gate

The first structured classic-script and resolver-preserving module-graph bridge has landed. Direct-page activation remains gated on its owner providing:

1. Integrate the shipped public, tested BlueJS node IDs, code-unit IDs, and safe-point validation APIs into the process-owned page host and native debugger channel. `BlueJsPageRuntime` already validates them for the exact live tab-owned generation, but it does not expose a debugger wire protocol or pause execution.
2. Bridge conformance fixtures extending the shipped no-emitted-JavaScript-reparse proof to exact origin/module preservation and deterministic bytecode-map ordering.
3. A host schema generator proving each generated `lib.blueice.d.ts` binding exists in the corresponding feature profile and that an absent binding is rejected by both checker and host. The generator, profile catalog, byte-exact manifest validator, and deliberately empty initial fixture are shipped; matching BlueJS bindings and direct-page compiler enforcement are still required.
4. Debugger tests for breakpoint binding, step/exception locations, stale-map rejection and the distinction between a static TypeScript type and a runtime BlueJS value.

Until those gates are satisfied, `BlueTsDebugInfo` remains VM-independent and contains static source/type/symbol data only. This document records the shipped bounded hand-off and fixes the rejection behavior for the still-missing page APIs.
