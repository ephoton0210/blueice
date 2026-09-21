# Phase 18 — BlueTS / BlueTSC: TypeScript Front End, Emitter, Type-Aware Debugging, and Runtime Contracts

[← Back to plan](../BROWSER_CORE_PLAN.md)

**Status**: In progress. `backend/bluets` provides a standalone, host-neutral BlueTS front end and `bluetsc` command for an explicitly bounded initial language matrix. `backend/bluets-bluejs` now also proves direct, host-neutral classic-script and resolver-preserving ESM-module-graph paths: it checks caller-supplied BlueTS, lowers a deliberately small runtime subset to the public BlueJS program AST, and compiles it to BlueJS bytecode without reparsing BlueTSC's emitted JavaScript. It does **not** yet execute a TypeScript page: the page-script host, bytecode safe-point map, debugger IPC and MCP project-registration boundary remain prerequisites.

The prioritized completion worklist is [TODO.md](TODO.md). Update it with this plan when an implementation or acceptance condition changes.

## Objective

Let a BlueIce page opt in to TypeScript source without a build-time `.js` artifact, while also providing BlueTSC for projects that need to compile TypeScript to portable JavaScript. Both paths preserve the reasons to author code in TypeScript: deterministic static diagnostics, source-level debugging, and—where data crosses a trust boundary—runtime validation of an explicit, reifiable contract.

## Standalone implementation and first BlueJS integration status

The direct bridge lowers parenthesized `new Identifier(args)` expressions with normal or spread arguments. Member constructors and omitted parentheses remain excluded.

The direct bridge lowers `delete` only for ordinary dot or bracket property references (without optional chaining). Identifier and non-reference operands remain outside the v1 subset.

The direct bridge lowers relational `in` and `instanceof` expressions. BlueTSC gives both a `boolean` result in the bounded checker; narrowing and custom-instance analysis remain future work.

Direct calls may use identifier, dot-member, or bracket-member callees with normal or spread arguments, preserving the BlueJS receiver. Optional calls remain excluded.

The current direct bridge subset supersedes earlier phase summaries: literals/identifiers, bounded quoted-string and template escapes, template slots containing supported direct expressions, direct calls/constructors with normal or spread arguments, supported unary/binary/logical/conditional/assignment/sequence expressions including relational `in`/`instanceof`, non-hole arrays with spread elements, identifier/string/numeric/computed object keys with identifier shorthand and spread properties, dot/bracket reads, property assignments/updates/deletion. Nested templates, optional chaining, object methods, and accessors remain outside the subset.

The direct bridge accepts identifier, quoted-string, numeric, and computed object keys plus spread properties. A computed key lowers its supported direct expression to BlueJS `PropertyKey::Computed`; shorthand remains identifier-only, while methods and accessors remain excluded. BlueTSC merges an identifier-bound known record spread source into an inferred literal; computed keys, unknown/non-record spread sources, and general spread expressions remain `unknown` under the bounded static rule.

The direct bridge lowers normal and spread array elements while retaining holes as an explicit exclusion. BlueTSC infers a known `T[]` spread source as `T` for a surrounding array literal; unknown/non-array sources retain the bounded checker's `unknown` element type.

The direct bridge lowers normal and spread call/constructor arguments. For calls to a known local function, BlueTSC expands only a tuple-typed spread into fixed parameter types; ordinary arrays, unknown values, and general iterable analysis remain outside that static rule.

Named local functions may use optional identifier parameters and a final identifier rest parameter. An optional parameter without an initializer lowers to an ordinary JavaScript parameter; within the function body BlueTSC treats its value as `T | undefined`. The direct bridge uses BlueJS's rest-parameter AST form; with an explicit `T[]` annotation, BlueTSC checks every normal or tuple-spread tail argument against `T` and infers generic `T` from the same tail. General array/iterable spread analysis remains outside the direct rule.

Named local functions may use a default parameter whose initializer is in the direct expression subset. BlueTSC retains the original initializer tokens, verifies a typed initializer against the parameter type, and the bridge lowers it directly to BlueJS `Param::default`; default expressions outside the direct subset remain excluded.

Named local function bodies retain ordered initialized local declarations, semicolon-terminated direct-expression statements, and `return` statements. The bridge lowers each structured expression statement to BlueJS in source order, so assignments and direct calls may have their normal local side effects. BlueTSC applies its existing bounded direct-call, property-access, and arithmetic checks to those statements; control flow, exception handlers, and every other unstructured body statement remain opaque and are rejected by the direct bridge.

Named local function bodies may also use `throw expression;`. BlueTSC retains and checks the direct expression, rejects an omitted value or a line terminator immediately after `throw`, and the bridge lowers it to BlueJS `Stmt::Throw` so the VM preserves the thrown JavaScript value. `try`/`catch`/`finally` and all other exception control flow remain outside the v1 direct subset.

Named local function bodies may use a direct-expression `if` condition with braced consequent, braced `else if` branches, and an optional braced `else` body. BlueTSC retains each branch structurally and the bridge emits BlueJS `Stmt::If` nodes with explicit blocks for braced bodies, while lowering an `else if` as a direct alternate rather than inventing an artificial block scope. The checker applies its existing bounded direct-expression checks to every condition and recursively to the branch bodies; it does not claim control-flow narrowing. Unbraced branches, an `else if` with an unbraced branch, loops, and every other control-flow form remain opaque and fail closed at the direct bridge.

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
- The standalone compiler emits the VM-independent portion of `BlueTsDebugInfo`: source-content hashes, static types, symbols and spans. It also contains a bounded, pure contract IR/validator for reifiable JSON-like values. Neither artifact claims a runtime type tag or validates a live page boundary yet.
- BlueTSC source maps use Source Map v3 segments at copied, rewritten and erased-source boundaries rather than line-only placeholders. Generated and original columns use UTF-16 code units; CRLF is represented as one source line transition. This gives portable JavaScript builds column-level TypeScript provenance without claiming a BlueJS bytecode safe-point map.
- `IncrementalCompiler` is a reusable, host-neutral single-entry session for development hosts. It reloads the caller-authorized graph to detect changed source or resolution edges, reuses parsed modules with identical bytes, and rebinds/rechecks only changed modules plus their reverse dependencies. It keeps only a successful cache entry and refuses reuse when the entry or any compiler option differs; its work-selection result is observable without exposing a BlueJS VM or page state.
- Generic aliases, interfaces and direct function calls retain their declarations' type parameters, including through local type-only imports. An interface may extend one or more named interfaces, including a generic instantiation; inherited fields participate in bounded structural checking, property lookup, declaration emission and reifiable contract conjunction. Its exported type-only surface carries inherited local declaration fields so an authorized consumer need not import private parent declarations. Direct local calls may either infer or explicitly supply their type arguments. They instantiate bounded structural checks, enforce `extends` constraints, and resolve trailing default type arguments (including in declaration modules). A declaration's type parameter never leaks into surrounding module scope. The checker additionally infers array literal element types, boolean comparisons, boolean-only `&&`/`||` chains, nullish coalescing after removing left-side `null`/`undefined`, unary `typeof`/`void`/boolean/numeric expressions, conditional branch joins, numeric `+`/`-`/`*`/`/`/`%`/`**` expressions, `<<`/`>>`/`>>>`/`&`/`^`/`|` expressions and known-string concatenation. It rejects numeric, exponentiation or bitwise/shift operators whose two operands are known incompatible primitives; it reports an unparenthesized unary exponent base as an ECMAScript early error; and it rejects a strict equality operator whose known `number`, `string` or `boolean` operands are disjoint. Unknown, `any`, union and structural operand rules remain outside this narrow initial rule; method/callback overload resolution and all other general expression inference remain pending.
- The parser rejects a `.tsx` module at its source-identity boundary, even if it has not yet reached a JSX tag. This prevents a TSX project from being treated as ordinary erasable TypeScript; tagged JSX is rejected by the same stable `UnsupportedSyntax` path. It also reports the ECMAScript early errors for an unparenthesized `??` mixed with `&&` or `||`, and a unary expression used directly as an exponentiation base, including in runtime spans that the bounded front end otherwise preserves for emission.
- Legacy CommonJS-oriented `import =` and `export =` forms are likewise rejected as `UnsupportedSyntax`, rather than being emitted as invalid ESM.
- For a direct call to a locally declared function, the checker verifies the accepted argument count and each annotated parameter after bounded generic substitution, whether type arguments are inferred or explicitly supplied. The same rule applies to semicolon-terminated direct-expression statements in a function body. Optional and default-initialized parameters are omittable while preserving a known return type. In a function body, a bare optional parameter has type `T | undefined`, while a default-initialized parameter has type `T`. Signature-only local overload declarations are resolved in declaration order for direct calls and erased from JavaScript; a non-declaration signature must have a compatible local implementation. This intentionally does not claim method calls, callback analysis, constructors, or general expression inference.
- Direct property access on an inferred record or a local/interface type alias is resolved to the declared field type (including a generic instantiation). Optional fields produce `T | undefined`; chained/member-call analysis and arbitrary JavaScript property semantics remain outside this static subset.
- The standalone `ContractPlan` validator accepts per-boundary `ValidationLimits` for depth, collection entries, visited-node fuel and string bytes. These checks remain pure data validation; host-boundary discovery and enforcement are still deliberately separate work.
- `backend/bluets/tests/typescript_oracle.rs` is the BlueTSC compatibility-oracle job run on every push and pull request. Its test-only `BLUEICE_BLUETSC_ORACLE` environment variable names the pinned TypeScript 5.9.3 `tsc` executable; the all-caps spelling is environment-variable convention, while BlueTSC is the BlueIce compiler name. The job verifies that pin before executing, then runs a fixture matrix covering generic properties, constraints/defaults, explicit direct-call type arguments and generic interface heritage (including local `.d.ts` parents and rejected conflicting inherited fields), optional record fields and optional/default/explicit-`undefined` parameters, local constrained generic function overloads, ordered direct-expression function-body statements and rejected calls within them, function throws and invalid bare throws, braced function `if`/`else if`/`else` bodies and rejected direct calls in their conditions, array/object literals with spread elements/properties and computed object keys, direct-expression default and array-typed rest parameters plus direct calls/constructors with spread arguments, simple object literal property reads, templates with supported expression substitutions, generic arithmetic, exponentiation, numeric bitwise/shift, compound assignments, identifier/property updates and comma sequences, boolean comparisons and relational membership (`in`/`instanceof`), boolean logical chains, `??`, `typeof`/`void` expressions and conditional branch joins, named default-function, local-value default, and local named-value ESM exports, rejected assignment/call arguments including incompatible known primitive arithmetic, exponentiation or bitwise operands, disjoint strict primitive comparisons, `typeof` annotation mismatch, `??` annotation mismatch, unparenthesized `??`/logical mixing and unparenthesized unary exponent bases, Source Map v3 shape and accepted Node output. It uses ES2022 modules and a temporary `type: module` package boundary, so both BlueTSC and external `tsc` ESM artifacts execute under Node; the local default and named-value fixtures additionally compare each compiler's exact public `.d.ts` output. Rejected fixtures assert the exact BlueTS diagnostic count, stable code and source line, then require the pinned compiler to report the same count and source lines. Node and the external `tsc` are test tools only; neither is linked, spawned, or discovered by BlueTSC or BlueTS.
- `CompilerLimits` makes source bytes/tokens/type nesting, module count/edge count/import depth, aggregate source bytes, generic-expansion work and source-map segments explicit compiler policy. Every limit participates in cache and artifact fingerprints; adversarial unit tests require a stable `ResourceLimit` failure with no output.
- `blueice-bluejs` owns the public `BlueJsProgramV1` wrapper and its `bluejs-program-v1` identity. `backend/bluets-bluejs` is the sole crate that depends on both compilers. It consumes a checked, already-tokenized BlueTS source module or closed module graph and directly constructs the BlueJS AST before BlueJS compiles bytecode; it never gives BlueTSC-emitted JavaScript to the BlueJS parser. Its executable v1 subset is the authoritative current direct-bridge subset summarized above, which supersedes the historical phase-level lists in this document. It retains ECMAScript grammar boundaries by rejecting unparenthesized `??` mixed with `&&` or `||`, and an unparenthesized unary exponent base. ESM mode maps local named/default exports and runtime imports to BlueJS module entries. Every runtime request uses the exact canonical target retained by BlueTS's caller-authorized resolver; only runtime-reachable modules become BlueJS nodes, so type-only `.d.ts` inputs remain static. The parser represents initialized local declarations, direct-expression statements, throws, returns, braced `if` bodies, and braced `else if` chains in functions; it records every other body token as opaque so the bridge rejects rather than silently drops a statement it cannot lower. Re-exports and other runtime shapes not listed in the current subset fail explicitly until their direct lowering is implemented.
- The public [BlueTS ↔ BlueJS integration contract](INTEGRATION_CONTRACT.md) records the shipped structured-program bridge and its still-pending source-provenance, safe-point-map and generated host-typings requirements without adding a BlueJS dependency to BlueTS itself.
- `blueice_engine::script::direct_page::DirectPageScriptHost` binds the in-process direct bridge to a live core `TabManager`: a typed `TabId` resolves the current `Page`, its HTTP(S) URL is canonicalized to a realm origin, and a private replacement-document generation forces realm recreation even for same-origin navigation. A core owner may inject a validated `DirectPageRealmOwner` at construction, keeping VM, realm, program-count, bytecode, and static-debug retention limits outside every script request; an over-budget direct attachment leaves no retained program or debug record. Its `realm_stats` exposes only the owning tab's retained program count, bytecode charge, and VM heap statistics; navigation releases the old charge before a successor document can admit code. The optional `run_session_with_script_requests_and_direct_page_host` lifecycle seam synchronizes only previously admitted realms after each session batch. `DirectPageInlineExecutor` is the separate, explicitly configured lifecycle owner for inline declarations: it generates its selected profile itself, runs a document's opted-in inline declarations once in document order, and retains only bounded source-free reports. `run_session_with_script_requests_and_inline_page_executor` invokes that owner after each session batch. BlueJS now provides a realm-local, GC-safe primitive host-function ABI plus a page-runtime registrar that cannot execute bytecode or inspect the VM; registrations disappear with the realm on navigation. `DirectPageScriptHost` installs the checked-in `core-script-document-text-v1` profile and the composed `core-script-document-context-v1` profile only: the latter adds `blueiceDocumentOrigin(): string`, a copied canonical tuple origin with no path, query, fragment, URL object, DOM node, event, or mutation capability. Both values are rebound after replacement. This is not a general `document` object, automatic production script owner, or BlueJS process launcher.
- A parsed `Page` now discovers the explicit non-portable `application/x-blueice-typescript` and `application/x-blueice-typescript-module` declarations in document order, retaining inline source or an external `src` as data. It never grants loading authority or evaluates them: a future page loader must apply origin, feature-profile, integrity, resolver, and resource policy before assembling the `AuthorizedModuleLoader` for direct admission.
- As a deliberately bounded normal-page fixture seam, `DirectPageScriptHost::execute_inline` may admit one inline declaration only. It derives a canonical module ID from core tab/document-generation/declaration identities and creates a one-module closed loader, so it neither embeds caller text in an identity nor reads an external source. `DirectPageInlineExecutor` can invoke that seam automatically only when a core owner explicitly selects it. By default it rejects and reports external `src` without reflecting the page-controlled URL; `PageScriptSourceAuthorizer` is the sole optional core-owned authority that can turn that declaration into a supplied closed graph plus resolver fingerprint. The executor never fetches, resolves, or falls back itself. A real fetch/cache/integrity implementation, remaining host bindings, and an out-of-process host remain open.
- [`bluets-test-interface`](TEST_INTERFACE.md) now exposes the same persistent JSON-lines ready/request/reply transport as BlueJS's test adapter. It is intentionally compile-only, accepts BlueJS's `sloppy` mode as a `raw` alias, and has stable BlueTS diagnostic codes/spans and caller-controlled compiler limits; Test262 runtime execution remains a future bridge concern rather than a hidden BlueJS dependency.

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
- [x] Implement VM-independent `BlueTsDebugInfo` (source hashes, symbols, static types and spans); bytecode source mapping and controlled debugger retention remain pending
- [ ] Expose TypeScript diagnostics, symbols, types, contracts, lowering provenance and BlueTSC check/build through Phase 12's negotiated MCP debug interface
- [x] Implement the pure runtime-contract IR and bounded JSON-like validator; host-boundary discovery, JSON Schema delegation and page enforcement remain pending
- [x] Implement host-neutral dependency-aware incremental parser/checker cache invalidation; cache reuse is refused across compiler-policy changes and failed compilations preserve the last successful entry
- [x] Support root-confined, type-only local `.d.ts` modules without runtime emission or package/remote declaration acquisition
- [x] Bound the pure contract validator's depth, collection, node-fuel and string-byte work with caller-visible limits
- [x] Add an opt-in, TypeScript-5.9.3-pinned external-oracle job for the implemented initial matrix
- [ ] Add real-process page, debugger, contract, resource, policy and multi-tab regression coverage
