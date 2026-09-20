# BlueTS ↔ BlueJS integration contract (v1)

[← Phase 18 plan](PLAN.md)

This document publishes the versioned boundary between BlueTS and BlueJS. `blueice-bluejs` now exposes a structured-program hand-off, and `backend/bluets-bluejs` uses it for bounded, host-neutral classic-script and resolver-preserving ESM-module-graph subsets. The boundary intentionally does **not** add a dependency from `blueice-bluets` to `blueice-bluejs`. A page host, bytecode safe-point map and debugger attachment do not exist yet; BlueTS and BlueTSC remain usable as standalone, host-neutral front ends.

The keywords **MUST**, **MUST NOT**, **SHOULD**, and **MAY** express the contract's requirements.

## Versions and compatibility

`backend/bluets-bluejs` is the sole crate allowed to depend on both `blueice-bluets` and `blueice-bluejs`. Neither compiler may depend on the other. BlueJS owns the initial structured-program ABI identity, `bluejs-program-v1`.

| Surface | Current wire version | Owner | Compatibility rule |
| --- | --- | --- | --- |
| BlueTS language matrix | `blue-ts-0.1` | BlueTS | A host MUST reject a language version it did not explicitly enable. |
| Lowered program hand-off | `bluejs-program-v1` | BlueJS ABI, BlueTS lowering adapter | Major/version mismatch rejects before VM compilation. |
| BlueTS static debug metadata | `blue-ts-debug-v1` | BlueTS | May be consumed without a VM; it has no bytecode offsets. |
| Bytecode safe-point map | `bluejs-safe-point-map-v1` | BlueJS ABI, bridge | It is valid only for the exact generated program. |
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

## AST/IR hand-off

The program boundary is structured data, not generated JavaScript text. BlueJS owns `BlueJsProgramV1`, whose current variants wrap its public `Program` (classic script) and `Module` ASTs; its `compile` method dispatches to BlueJS's compiler. BlueTS lowers supported TypeScript syntax once through the bridge to that BlueJS-owned AST. BlueJS remains the authority for ECMAScript semantics, bytecode generation, realm ownership, GC accounting, capability summary and execution.

The shipped bridge accepts either one non-declaration source module or a closed ESM module graph in classic-script or ESM-module mode. It erases type aliases, interfaces and type exports; it directly lowers initialized `var`, `let` and `const` declarations; named local functions with required identifier parameters; ordered local-declaration/`return` function bodies; and literal, identifier, parenthesized, direct-call, unary, arithmetic, relational, equality, `&&`, `||`, `?:` and identifier-only assignment expressions. In ESM mode, local named/default exports and runtime import bindings become BlueJS module entries. A graph request uses the exact canonical target that BlueTS retained from its caller-authorized resolver; it is not resolved a second time from the original TypeScript specifier. Only runtime-reachable modules are materialized, so type-only declaration modules remain static. The parser retains any unstructured function-body token as opaque data, and the bridge MUST reject it instead of silently dropping it. Re-exports remain outside BlueTS's accepted matrix; member access, optional/default/rest parameters and every other unlowered runtime shape likewise fail explicitly. The bridge consumes BlueTS's checked, tokenized declarations and **MUST NOT** call a BlueJS source parser on BlueTSC's emitted JavaScript. Its integration tests execute the resulting BlueJS bytecode, not a reparsed generated text artifact.

The current wrapper does not assign stable AST node IDs, code-unit IDs or bytecode safe points. Those are required before direct-page source/debug attachment. The following logical envelope is therefore the target direct-page extension, not a representation currently transferred by the host-neutral script/module bridge:

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

BlueTSC Source Map v3 is a portable JavaScript artifact. It is not a bytecode debugger map. Once BlueJS exposes instruction safe points and code-unit IDs, the bridge will combine them with `BlueTsLoweringProvenanceV1` and publish the following generation-bound map:

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

Only source spans that explain a reachable instruction may be attached. An erased annotation can be recorded as nearby provenance, but MUST NOT become a standalone breakpoint target. A source breakpoint resolves to the nearest following safe point in the same module and source range policy; if none exists, the debugger reports an unbound breakpoint. A stale request—wrong generation, ABI, fingerprint, source-set hash, code unit, or instruction boundary—MUST be rejected, never remapped heuristically.

Source locations use UTF-8 byte offsets at this boundary. BlueTSC's Source Map v3 conversion continues to use UTF-16 generated/original columns, including a single line transition for CRLF. Consumers must convert explicitly at the display boundary and must not mix these two coordinate systems.

## Host-generated `lib.blueice.d.ts`

`lib.blueice.d.ts` is a generated, host-supplied declaration root. It is not a hand-maintained substitute for `lib.dom.d.ts`, and it must describe only APIs that the current BlueIce page-script host has actually exposed.

The future host owns a declarative `HostTypeSurfaceV1` schema adjacent to the binding definitions that create each value. Each schema item records:

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
  enabled_feature_profile,
  binding_ids: [...]
}
```

The manifest is emitted with a stable field order, normalized LF text and no timestamps, checkout paths or machine-specific IDs. The `.d.ts` generator sorts declarations by stable binding ID and uses the same normalization rules, making the host-produced typing root reproducible. A direct BlueTS compile MUST be given this manifest and declaration source by the host's authorized module loader; it MUST reject an unavailable profile, a schema hash mismatch, or a declaration source whose bytes do not match the manifest. BlueTSC may consume an explicitly supplied local `.d.ts` module under its existing root-confined policy, but that does not claim a BlueIce host API.

Adding a host API is additive only when it preserves existing binding IDs and declaration meanings. Removing or changing a public declaration requires a new host API major version and a new compatible feature profile. A compiler may target a declared older profile only when the host explicitly supplies its matching generated manifest; it may never infer API availability from the installed BlueJS version.

## Remaining implementation gate

The first structured classic-script and resolver-preserving module-graph bridge has landed. Direct-page activation remains gated on its owner providing:

1. Public, tested BlueJS node IDs, code-unit IDs and safe-point validation APIs for the page/module AST surface.
2. Bridge conformance fixtures extending the shipped no-emitted-JavaScript-reparse proof to exact origin/module preservation and deterministic bytecode-map ordering.
3. A host schema generator proving each generated `lib.blueice.d.ts` binding exists in the corresponding feature profile and that an absent binding is rejected by both checker and host.
4. Debugger tests for breakpoint binding, step/exception locations, stale-map rejection and the distinction between a static TypeScript type and a runtime BlueJS value.

Until those gates are satisfied, `BlueTsDebugInfo` remains VM-independent and contains static source/type/symbol data only. This document records the shipped bounded hand-off and fixes the rejection behavior for the still-missing page APIs.
