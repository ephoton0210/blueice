# Phase 18 — BlueTS / BlueTSC: TypeScript Front End, Emitter, Type-Aware Debugging, and Runtime Contracts

[← Back to plan](../BROWSER_CORE_PLAN.md)

**Status**: In progress. `backend/bluets` now provides a standalone, host-neutral BlueTS front end and `bluetsc` command for an explicitly bounded initial language matrix. It does **not** yet execute a TypeScript page: the BlueJS public AST/IR hand-off, page-script host, bytecode safe-point map, debugger IPC and MCP project-registration boundary remain prerequisites.

## Objective

Let a BlueIce page opt in to TypeScript source without a build-time `.js` artifact, while also providing BlueTSC for projects that need to compile TypeScript to portable JavaScript. Both paths preserve the reasons to author code in TypeScript: deterministic static diagnostics, source-level debugging, and—where data crosses a trust boundary—runtime validation of an explicit, reifiable contract.

## Independent implementation status

The first implementation intentionally completes the work that has no BlueJS
dependency before introducing any page-runtime coupling:

- `blueice-bluets` accepts only caller-supplied `ModuleLoader` records. The
  library itself does not read files, URLs, DOM state or host capabilities.
  `bluetsc` is the separate, project-root-confined filesystem adapter.
- Its pinned `blue-ts-0.1` matrix parses/binds typed variable and function
  declarations, interfaces, aliases, type-only imports/exports, primitive and
  literal types, records, arrays, tuples, unions, intersections and the
  corresponding erasable annotations/assertions. It resolves a closed relative
  module graph, produces stable diagnostics for parse/unsupported syntax,
  resolution, duplicate names, unknown types and the implemented assignment /
  return checks, and never emits on an error.
- `bluetsc check` uses that shared pipeline without writes. `bluetsc build`
  stages ESM `.js`, optional line source maps and public `.d.ts` files, then
  replaces the selected output directory only after every artifact has been
  staged. Its fingerprint includes source content, the pinned language version,
  target, source-map/declaration modes, runtime-policy label and resolver
  identity.
- `bluetsc` accepts either one explicit entry or a `bluetsc.json` project file.
  The latter supports multiple `entries`, project-root-confined `outDir`,
  `sourceMap`, `declaration`, `target`, `runtimePolicy`, and exact/prefix
  `imports` mappings. Config flags cannot be mixed with client-side overrides;
  an import target, entry or output path that escapes the declared root is
  rejected before compilation. This is the standalone compiler's closed-world
  resolver, not a page/network loader or an arbitrary package-manager hook.
- Every successful build stages a root-relative `bluetsc.manifest.json` with
  language version, project fingerprint, target, runtime policy, requested
  output modes and emitted entries. A configured `imports` map also emits
  `bluetsc.importmap.json`, translating source `.ts`/`.tsx` mappings to their
  generated `.js` locations. Both files are published with the artifacts, so
  they contain no absolute host path and cannot drift from an atomic build.
- The filesystem adapter supplies root-relative source identities to the shared
  compiler. Therefore artifacts, source maps and the VM-independent
  `BlueTsDebugInfo` do not expose checkout paths, and relocating an unchanged
  project does not change its resolver or source-identity contribution to the
  build fingerprint.
- The standalone compiler emits the VM-independent portion of
  `BlueTsDebugInfo`: source-content hashes, static types, symbols and spans.
  It also contains a bounded, pure contract IR/validator for reifiable
  JSON-like values. Neither artifact claims a runtime type tag or validates a
  live page boundary yet.
- `IncrementalCompiler` is a reusable, host-neutral single-entry session for
  development hosts. It reloads the caller-authorized graph to detect changed
  source or resolution edges, reuses parsed modules with identical bytes, and
  rebinds/rechecks only changed modules plus their reverse dependencies. It
  keeps only a successful cache entry and refuses reuse when the entry or any
  compiler option differs; its work-selection result is observable without
  exposing a BlueJS VM or page state.

This is deliberately not a claim of general `tsc` compatibility. Control-flow
narrowing, overload resolution, generic substitution, decorators, enums,
classes, TSX, namespace emission, parameter properties, arbitrary JavaScript
expression typing, incremental caching and a full source-map
column/provenance model remain pending. A construct outside the
implemented matrix must be added with a parser/checker/emitter test and a
precise compatibility entry; it must not be advertised merely because its
tokens happen to be erasable.

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

The first executable slice targets type-bearing but eraseable syntax needed for ordinary typed page code: annotations, interfaces, type aliases, generics, union/intersection/literal types, narrowing, overload declarations, `readonly`, access modifiers, `as`, `satisfies`, non-null assertions, `declare`, and `import type`/`export type`. It emits only ECMAScript constructs currently supported by the selected BlueJS target.

The first slice explicitly rejects TSX/JSX, decorators, `enum`/`const enum`, runtime `namespace`/`module`, parameter properties, `import =`, `export =`, declaration merging with runtime emission, and custom transformers. Some are not erasable; for example, ordinary TypeScript enums emit runtime objects. They can be proposed later only with a precise lowering specification, BlueJS runtime support, debugger mappings, capability-summary coverage and differential tests. TSX additionally requires an explicit JSX factory/runtime and is a separate UI-framework decision.

## Delivery order and acceptance

### Slice 0 — JavaScript and debugger prerequisites

1. Complete Phase 13's BlueJS host/core wiring, DOM binding design and a real script-bearing page fixture.
2. Complete Phase 17's script source/safe-point debugger path and the source-map representation required by BlueTS.
3. Define a public `BlueJsProgram` hand-off that accepts an origin-preserving AST/IR without reparsing emitted source, plus compiler/cache resource accounting.
4. Define a pinned BlueTS language-version matrix and generate `lib.blueice.d.ts` from implemented host bindings.

Acceptance: a JavaScript fixture executes through the page loader and debugger with a stable source location, while an unimplemented host API is absent from `lib.blueice.d.ts` and fails an attempted typed use.

### Slice 1 — checked TypeScript source and source-level debugging

1. Build the TypeScript tokenizer/parser, binder, URL-module resolver and checker for the initial boundary.
2. Lower supported typed syntax straight to BlueJS AST/bytecode input, without JavaScript-text round trips.
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

1. Implement dependency-aware incremental checker/cache invalidation and trusted local declaration files. The standalone parser/checker cache is complete; trusted local declaration files remain pending.
2. Expand the supported TypeScript feature matrix only alongside its BlueJS lowering/runtime, source mapping, contract and oracle evidence.
3. Add opt-in comparison against a pinned TypeScript compiler for accepted syntax/diagnostics and Node/BlueJS behavioral differential tests for the lowered output.
4. Expose carefully redacted project/type metadata to Phase 17 DevTools and the documented, capability-scoped Phase 12 MCP debug interface.

Acceptance: editing one module invalidates only its dependents; a cache entry checked under another policy is refused; every supported compatibility feature has parser/checker/lowering/debug/contract expectations and an independently checked fixture.

## Testing strategy

- Keep public parser, binding, checker, lowering, contract and debugger fixtures under the eventual `backend/bluets/tests/` boundary, with shared HTML page fixtures beside the Phase 13/17 integration tests.
- Test diagnostics by stable code/category, source span and semantic condition—not copied compiler-message wording. Every unsupported feature must have an explicit rejection fixture.
- Compare accepted syntax and checker behavior against a pinned external TypeScript reference in an opt-in test job; compare resulting runtime behavior against the reference JavaScript output where the selected BlueJS feature subset supports it. The reference never determines BlueTS's security policy or makes an unsupported test silently pass.
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
- [ ] Define the public BlueJS AST/IR hand-off (without emitted-source reparsing) and its compatibility matrix
- [ ] Generate and test `lib.blueice.d.ts` from actual host bindings
- [x] Implement the independent initial parser/binder/closed-module resolver/checker/type-erasure ESM emitter with atomic compile failures
- [x] Implement `bluetsc check`/staged `build`, ESM/line-source-map/declaration emission and reproducible artifact fingerprints for the initial matrix
- [x] Implement VM-independent `BlueTsDebugInfo` (source hashes, symbols, static types and spans); bytecode source mapping and controlled debugger retention remain pending
- [ ] Expose TypeScript diagnostics, symbols, types, contracts, lowering provenance and BlueTSC check/build through Phase 12's negotiated MCP debug interface
- [x] Implement the pure runtime-contract IR and bounded JSON-like validator; host-boundary discovery, JSON Schema delegation and page enforcement remain pending
- [x] Implement host-neutral dependency-aware incremental parser/checker cache invalidation; cache reuse is refused across compiler-policy changes and failed compilations preserve the last successful entry
- [ ] Add trusted local declaration files and opt-in external-oracle jobs
- [ ] Add real-process page, debugger, contract, resource, policy and multi-tab regression coverage
