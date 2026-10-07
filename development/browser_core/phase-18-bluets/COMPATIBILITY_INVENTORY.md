# BlueTS compatibility inventory against pinned TypeScript 5.9.3 (J.6.1)

**Status: full `tsc` parity is not claimed and must not be advertised.** BlueTS is a checked,
emitting TypeScript front end for a defined subset of the language. Every form in that subset is
compared with the pinned 5.9.3 compiler by the differential suites in section 1, and everything
outside it is refused with a diagnostic (section 4) or is listed as an open gap (section 3). The
claim "full parity" is the conjunction of J.6.1 to J.6.3 *and* no open required gap; section 3
has required gaps open, so it stays closed.

The authority for every expectation is the pinned `typescript@5.9.3` executable (`BLUEICE_BLUETSC_ORACLE`),
not documentation. The inventory below is regenerated, not remembered: section 1 is the list of `#[ignore]`
test files (`grep -rl '#\[ignore' backend/bluets*/tests`), section 4 is
`python3 tools/inventory_refusals.py` over the source, and CI runs every ignored suite on Linux and macOS
(`.github/workflows/ci.yml`, job `typescript-oracle`).

## 1. What is compared with `tsc`, and how

| Suite (`backend/bluets/tests/`, `backend/bluets-bluejs/tests/`) | Compares | Size |
| --- | --- | --- |
| `typescript_oracle.rs` | accepted/rejected verdicts, diagnostics, emitted JavaScript, declarations and runtime output of the core language subset (functions, arrows, async, generators, tuples, destructuring, records, generics, modules) | 109 cases, plus the fixture directories below |
| `class_checker_matrix.rs` | accept/reject of class forms (fields, accessors, visibility, heritage, parameter properties, private names, static blocks) | 523 entries |
| `enum_checker_matrix.rs`, `enum_oracle.rs` | accept/reject of enum forms; emitted objects, reverse mappings, `const enum` options run under Node | 57 entries |
| `namespace_checker_matrix.rs`, `namespace_oracle.rs`, `namespace_declaration_oracle.rs` | accept/reject of namespace forms; ES2022/ES2020 emit run under Node; `.d.ts` output | 76 entries, 2 targets |
| `class_downlevel_oracle.rs` | private names, fields and static blocks below ES2022, under both field semantics | ES2020 and ES2022 |
| `commonjs_oracle.rs` | `--module commonjs` output (live bindings, cycles, `export =`, `import = require`, interop) run under Node | 6 programs |
| `jsx_checker_matrix.rs`, `jsx_oracle.rs` | accept/reject of JSX against the declared `JSX` namespace (53 entries); every `jsx` mode, factory, pragma and runtime option run under Node | 53 entries, 11 programs |
| `decorators_checker_matrix.rs`, `decorators_oracle.rs` | standard decorators: accept/reject (32); evaluation/application order, contexts, metadata, class replacement, errors at class definition, run under Node | 32 entries, 20 programs |
| `legacy_decorators_checker_matrix.rs`, `legacy_decorators_oracle.rs` | `experimentalDecorators`/`emitDecoratorMetadata`: accept/reject (19); `__decorate`/`__param`/metadata call order and values, ES2022 and ES2020, ESM and CommonJS | 19 entries, 18 programs |
| `unknown_name_checker_matrix.rs` | accept/reject through `check` and `build` for lexical value/type names, hoisting, TDZ, imports, namespaces and shadowing; one program compares Node output and declarations | 108 entries, 1 program |
| `immutable_checker_matrix.rs` | accept/reject through `check` and `build` for const/import rebinding, compound/update/pattern/iteration writes, shadowing and readonly members; one program compares Node output and declarations | 173 entries, 1 program |
| `imported_value_checker_matrix.rs` | accept/reject through `check` and `build` for named/default/namespace and CommonJS imports, retained checked value surfaces, private type identities and circular inference; linked Node programs and exact inferred declarations | 81 entries, 2 programs, 15 declaration cases |
| `standard_library_checker_matrix.rs` | selected ECMAScript types and methods, matching target/lib, owner/runtime policy and versioned manifest; exact inferred declarations and linked Node execution | 123 entries, 1 program, 5 declaration cases |
| `inferred_return_checker_matrix.rs` | unannotated function/member return signatures, freshness, recursion and completion, async/generators, lexical/default/pattern scopes and importers; exact declarations and Node execution | 121 entries, 1 program, 23 exact declaration cases, 2 union type-equality cases |
| `project_config.rs` | JSONC inheritance, normalized compiler options and selected files; owner overlays and confinement; exact declarations and Node runtime, including strict-helper relocation | 69 configurations (59 accept, 10 reject), 2 linked programs |
| `strictness_flags.rs`, `strictness_controls.rs` | independent parent/strict-family and additional diagnostics, lexical/return/call/index boundaries, unchanged valid JavaScript and cache policy identity; pinned Node execution and exact declarations | 32 configurations (16 accept, 16 reject), 39 boundary controls, 1 program in 2 policies |
| `cli_surface.rs` | native project CLI flags and overrides, default project discovery, normalized configuration, source/emitted lists, noEmit and pretty/exit observations; input preservation, Node execution and exact declarations | 30 observations (24 accept, 6 reject), 2 emission layouts |
| `option_combinations_oracle.rs` | one linked multi-feature program over target, module system, default/explicit class-field policy, preservation, isolated modules, sourceMap, declaration, noEmit and strict; project verdicts, artifact inventory, Node output, exact declarations and source-map structure | 768 configurations (384 emitted, 384 noEmit) |
| `bluets-bluejs/tests/namespace_parity.rs`, `jsx_direct.rs`, `decorators_direct.rs` | the direct runtime (BlueJS) against Node running `tsc`'s output | 7 + 7 + 6 programs |
| `diagnostic_source_families.rs` | additional lexical/semantic diagnostic source families, exact primary codes and positions | 97 controls |
| `diagnostics_matrix.rs` | verdicts, TypeScript primary code/template, rendered message, UTF-16 position and related information | 1,670 programs, 147 templates, 912 rejected primaries |
| `diagnostics_projects.rs` | project/configuration, CLI and option-combination diagnostic positions | 867 observations |
| `diagnostics_presentation.rs` | actual plain/pretty CLI presentation, summaries, related source context, exit status and artifacts | 22 observations |
| `guards_checker_matrix.rs` | predicate/assertion signatures, lexical call effects, receiver guards and exhaustive completion; exact primary/related diagnostics, Node behavior and every emitted declaration | 75 programs, 5 runtime/declaration witnesses |
| `inference_checker_matrix.rs` | bounded argument/contextual candidates, constraints/defaults, explicit arguments, generic callable/class/interface signatures and imported boundaries; exact primary/related diagnostics, every emitted declaration, Node behavior and owner budget refusal | 74 programs, 4 runtime/declaration witnesses |
| `narrowing_checker_matrix.rs` | lexical control-flow verdicts and exact primary diagnostics; Node behavior and exact declarations | 123 programs, 14 runtime/declaration witnesses |

The fixture corpus under `tests/fixtures/typescript_oracle/` has 1880 top-level directories; each is an
entry whose verdict was recorded from the pinned compiler by an ignored test
(`BLUEICE_WRITE_*_MATRIX=1`), and the ordinary (non-ignored) tests replay the recorded verdicts offline.

BTS diagnostics retain BlueTS's own codes and raw messages. Their separate TypeScript
counterparts compare exact primary codes, text, original UTF-16 positions and related information
in the measured corpus. Source maps retain line-level provenance and are validated structurally;
emitted JavaScript compares behavior under Node, while declarations compare deterministic text.

The K.4.1 Linux verification covers all 33 differential suite files and all 128 ignored oracles.
All 123 narrowing cases and fourteen runtime/declaration witnesses pass, and the CLI tracker
compares 840 rejected primaries with zero message or related-information differences.

The K.4.2 final frozen-source Linux gate passes all 1,105 tests in 69 groups,
covering 34 differential suite files and all 130 ignored oracles. All 75 guard
cases and five runtime/declaration witnesses pass; the 123-case narrowing
regression and fourteen runtime/declaration witnesses also pass. The shared
diagnostic corpus contains 1,596 programs and 145 templates.

The K.4.3 final frozen-source Linux gate passes all 1,110 tests in 70 groups, including all 132 ignored oracles in 35 differential suite files. All 74 generic inference cases,
four runtime/declaration witnesses and the contextual budget refusal pass.
The shared diagnostic corpus contains 1,670 programs and 147 templates.

### Measured pass rate (2026-10-06, Linux aarch64 in Colima on Apple silicon, pinned `typescript@5.9.3`, Node 26)

Every differential suite of section 1 was run against the pinned compiler with none skipped:
**28 of 28 suites pass, 122 ignored oracle tests pass, 0 failures.** The versioned case list is the
repository itself at the commit that carries this file: 69 project configurations
(59 accept, 10 reject), 32 strictness configurations (16 accept, 16 reject) and 39 checking boundary controls, 30 native CLI observations (24 accept, 6 reject), 768 option combinations (all accept), separately from 1366 recorded accepted/rejected verdicts (class
523, namespace 76, enum 57, JSX 53, decorators 32, legacy decorators 19, unknown names 108, immutables 173, imported values 81, standard library 123, inferred returns 121), 109 core-subset
cases in `typescript_oracle.rs`, and the emit-and-run programs (JSX 11, standard decorators 20,
legacy decorators 18, CommonJS 6, unknown names 1, immutables 1, imported values 2, standard library 1, inferred returns 1, option combinations 768 (384 emitted, 384 noEmit),
namespace/enum/class downlevel under both targets, direct-runtime parity 20). The same suites run in CI on
Ubuntu 24.04 and macOS 15; Windows is not covered (G-M3). The percentage means only: *of the forms inside
the subset, every one agrees with `tsc`*. It says nothing about the forms in section 3, which are refused.

The M6 workspace milestone passed CI run [37387952696](https://github.com/ephoton0210/blueice/actions/runs/37387952696)
on `8343929b7`, including all 29 jobs and both pinned oracle platforms. Workspace
line coverage is 90.48%; independent BlueJS line coverage is 93.00%.
The K.2 workspace verification passed CI run [37428756632](https://github.com/ephoton0210/blueice/actions/runs/37428756632)
on `62f6a440d`: all 29 jobs succeeded, including both pinned oracle platforms.
Workspace line coverage is 90.43%; independent BlueJS is 93.00%.
The recorded subset measurements above remain the scope of the compatibility claim.

## 2. What the supported subset is (summary)

Statements and expressions with a typed core (numbers, strings, booleans, literals, arrays, tuples,
records and interfaces, unions, intersections, bounded generics, function types, async functions,
generators); ES module and CommonJS emit; classes (fields, accessors, visibility, static blocks, private
names, parameter properties, class+interface merging); enums and `const enum`; namespaces (nested, merged
with classes, enums, functions); `.tsx` JSX in every `jsx` mode; standard and legacy decorators; installed
packages through `node10`/`node16`/`bundler` resolution inside owner-authorized roots; pinned remote
declarations. The precise per-feature descriptions, with their recorded gaps, are the `J.x`, `K.1.1`, `K.1.2`, `K.1.3`, `K.1.4` and `K.1.5` sections of
`PLAN.md`.

## 3. Open gaps against `tsc` 5.9.3

Every row is a form `tsc` accepts and BlueTS does not (or accepts with different output), by area. All are
**required** for a full-parity claim. None is hidden: each is either refused with a diagnostic (a
section 4 entry) or is a silent divergence listed as such. IDs are stable; closing one means a differential
suite entry, then removing the row.

### 3.1 Type system

| ID | Gap |
| --- | --- |
| G-T1 | K.4.1 supplies lexical control-flow facts for the 123 pinned `typeof`/`instanceof`/`in`/literal/nullish/discriminant/truthiness, optional-chain, switch, loop, assignment and closure witnesses. Property facts retain their owning lexical binding and path. The three K.3.3 iterator-flow wording gaps are resolved and their message allowances removed. K.4.2 adds 75 pinned user-defined guard/assertion and exhaustive-completion witnesses, including imported and function-valued signatures, receiver predicates, explicit-call-target diagnostics and known `never` calls. Broader inferred predicate synthesis, exceptional-flow and dotted capture precision remain unmeasured. |
| G-T2 | K.4.3 adds 74 pinned argument/contextual candidate, widening, constraint/default, explicit argument and generic callable/class/interface witnesses, including imported signatures and dependent defaults. Concrete record `keyof`/indexed projections supply the measured prerequisites, and contextual budget exhaustion remains a precise refusal. Remaining: inference across arbitrary recursive/operator forms, higher-kinded patterns and broader variance; full operators and compatibility remain K.4.5/K.4.7, generic heritage/methods K.5.2. |
| G-T3 | Conditional types, `infer`, mapped types, template-literal types, indexed access and `keyof`/`typeof` type operators in full, recursive types beyond the type budget. |
| G-T4 | Overload resolution in full (call and construct signatures, contextual signature selection), optional chaining and non-null assertion typing in all positions. |
| G-T5 | Index signatures, `readonly` arrays/tuples, `unique symbol`, `bigint`, `symbol`, `never`/`unknown` flow rules, `satisfies`, `as const`, enum-like literal inference. |
| G-T6 | Structural-compatibility details: excess-property checks in every position, weak types, optional/exactOptional rules, `strictFunctionTypes` and method bivariance, `exactOptionalPropertyTypes`, `noUncheckedIndexedAccess`. K.1.2 enforces lexical binding immutability and readonly write targets in the supported expression forms. |
| G-T7 | K.1.4 adds an original, versioned minimum ECMAScript library selected by target, with owner replacement and no runtime grant. The tested Array/ReadonlyArray, boxed/primitive methods, Object/Function, Promise, collections, Math/JSON/Symbol, errors, iteration and Date/RegExp forms match the same pinned target/lib. Remaining catalogs, constructors, computed iteration, callback result precision and opaque compatibility names are enumerated in `STANDARD_LIBRARY.md`; DOM remains owner supplied. |
| G-T8 | K.1.5 infers the tested unannotated function/getter/method signatures, unions, void/never completion, async Promise and generator types, with literal widening and a bounded recursion guard. Imports and declaration emit retain those signatures. Remaining: literal freshness and widening in all positions, broader contextual and flow-sensitive inference, unproven unknown getter bodies and declaration precision; K.4/K.8 retain these gaps. |
| G-T9 | `abstract` classes, `implements` clauses, generic heritage/methods and static-member rules beyond the K.4.3 constructor witnesses, class expressions, computed class member names, `declare` fields, `override`/`noImplicitOverride`, index-signature members. |
| G-T10 | K.1.3 supplies checked value types for the tested named/default/namespace and CommonJS import forms, with inferred variable declarations and module-owned type identities. Remaining: imported-type query parsing; `import type`/`export type` forms beyond the supported ones, `export default <expression>`, value re-exports (`export * from`, `export { x } from`), import attributes, `export as namespace`, `declare module` augmentation and ambient module declarations, global augmentation, triple-slash directives, `unique` declaration merging beyond class+interface+namespace. |

### 3.2 Emit

| ID | Gap |
| --- | --- |
| G-E1 | Targets other than ES2020 and ES2022 (`ES5`, `ES2015`-`ES2019`, `ES2021`, `ES2023`+, `ESNext`) and the `lib` option; downlevel helpers (`__awaiter`, `__generator`, `__spreadArray`, `__values`) for async/generators/spread/iteration. |
| G-E2 | Module systems other than ES modules and CommonJS (`AMD`, `UMD`, `System`, `node16`/`nodenext` per-file module kind and `.mts`/`.cts`), `esModuleInterop` helper forms beyond default/namespace import, `verbatimModuleSyntax`, `importHelpers`, `noEmitHelpers`. |
| G-E3 | Decorators and auto-accessors for targets other than ES2022 with class fields defined; decorated private members, computed names, classes in namespaces and `export default @d class`; decorator metadata for imported/unresolved class types and unannotated methods (section 4). |
| G-E4 | `removeComments`, `preserveValueImports`, `importsNotUsedAsValues`, `stripInternal`, `newLine`, BOM/`emitBOM`, `inlineSources`, `sourceRoot`/`mapRoot`, `downlevelIteration`, `useDefineForClassFields` interactions beyond the two supported targets, `isolatedDeclarations`. |
| G-E5 | Source maps: line-level provenance only; no token-level names, `sourcesContent` parity or declaration maps. |

### 3.3 Declarations

| ID | Gap |
| --- | --- |
| G-D1 | K.1.3 compares inferred imported variable declarations, including nested callable/tuple types and unnameable external types. K.1.5 adds 23 exact inferred-return declaration comparisons and two pinned type-equality comparisons for union member printing order; that order can differ even when the types agree. Wider inferred declarations still need explicit types where `tsc` infers them (section 4, `emitter`); generic declarations, overloads of arbitrary shape, `declare module`, namespaces with merged symbols in every shape, `export =` of non-trivial forms. |
| G-D2 | Declaration emit options (`declarationDir`, `emitDeclarationOnly`, `declarationMap`, `stripInternal`). |

### 3.4 Modules, packages, projects

| ID | Gap |
| --- | --- |
| G-M1 | Resolution features not read: `typesVersions`, `.d.mts`/`.d.cts`/`.mts`/`.cts` entry points, `paths`/`baseUrl`/`rootDirs`, package self-name imports, `moduleResolution: classic`, automatic `@types` inclusion (`types`/`typeRoots`), `resolveJsonModule`, `allowJs`/`checkJs`, `.js` files as sources. |
| G-M2 | K.2.1 reads canonical JSONC projects, relative/package inheritance and file selectors with owner overlays. Sibling precedence preserves native Windows parent/prefix paths. K.2.3 provides native project/default discovery, `--project`, `--showConfig`, `--noEmit`, project source/emitted lists, pretty selection and exit codes; 30 pinned observations agree; K.2.4 verifies the supported options together in 768 Cartesian configurations. Remaining: project references, `--build`, `--watch`, `--incremental`/`.tsbuildinfo`, `composite`, bare-source native CLI invocations, the full installed standard-library file catalog and options beyond the documented compiler/resolver subset. |
| G-M3 | Windows: all five workspace build/test/lint jobs pass, including replay of the pinned project and CLI matrices and native sibling precedence. Live TypeScript oracle suites run on Linux and macOS; the harness does not launch `tsc.cmd`, and symlink tests remain Unix-only. |

### 3.5 Diagnostics and CLI

| ID | Gap |
| --- | --- |
| G-C1 | K.3.1 provides pinned TypeScript codes and message templates through `--diagnostics-json` and the public diagnostic API, preserving BTS aliases and enumerated owner/subset reasons. All 1,398 existing checker entries agree on primary code/template; 97 additional source witnesses cover further diagnostic families. K.3.2 matches primary UTF-16 positions and source module identity across 1,398 language/strictness and 867 project/CLI/option cases. K.3.3 adds related codes/messages/module identity/UTF-16 coordinates, contextual rendered wording, plain/pretty formatting, source and related context, per-file error tables, diagnostics summary fields and measured native exit/emission semantics. K.4.1 resolves the three measured G-T1 wording gaps; `--explainFiles` and the full `tsc` command line remain outside this measured subset. |
| G-C2 | Closed for the compared subset by K.2.2: parent `strict`, its nine family flags, unused locals/parameters, implicit returns, switch fallthrough, exact optional properties and unchecked indexed reads are independently selectable through projects and `CheckingOptions`. Thirty-two on/off configurations and 39 boundary controls agree with pinned TypeScript; valid JavaScript stays identical and legacy API defaults remain unchanged. Broader type-system behavior remains in G-T1/G-T2 and the other open rows; full `tsc` parity is not claimed. |
| G-C3 | JSX: `jsx` type options `jsxImportSource` runtime typing (the automatic runtime's `JSX` namespace is not read from the package), `JSX.ElementType`, `LibraryManagedAttributes`, `defaultProps`, generic components and type arguments on tags (section 4 and `PLAN.md` J.5.1). |

### 3.6 Direct runtime (BlueJS) profile

The direct profile (`compile_direct_*`) is a deliberately narrower route than emit and is not part of the
`tsc` parity claim, but each prerequisite of what it advertises is recorded: its expression subset
(no arrow functions in the bridge, calls only on identifiers and properties, object keys, optional
chaining forms) is in section 4 (`expression`, `lowering`); CommonJS, installed packages, remote declarations,
legacy decorators, preserved and automatic JSX are refused with a diagnostic naming the `bluetsc build` route.

## 4. Refusals: what BlueTS knows it does not do

Generated by `python3 tools/inventory_refusals.py` from the source (malformed-input diagnostics are
excluded). A refusal is never silent: the program is rejected with this text.

164 refusal sites in 10 areas

### bin (1)

- `backend/bluets/src/bin/bluetsc/native_cli/mod.rs:187` — BlueTSC configuration or an explicit owner policy refused this input.

### checker (22)

- `backend/bluets/src/checker/module/binding/modules.rs:174` — `export =` cannot be used when the module system is ECMAScript; use `--module commonjs`
- `backend/bluets/src/checker/module/binding/modules.rs:166` — `import x = require()` cannot be used when the module system is ECMAScript; use `--module commonjs`
- `backend/bluets/src/checker/module/binding.rs:236` — a class member other than a constructor, method, field or accessor \ (a computed, generator or `accessor` member) is not supported yet
- `backend/bluets/src/checker/module/binding/enums.rs:214` — a computed initializer that refers to the member `{}` must write it \ as `{}.{}`
- `backend/bluets/src/checker/module/decorators.rs:116` — a decorator can only decorate a method implementation, not an overload
- `backend/bluets/src/checker/module/binding/classes/fields.rs:327` — a field initializer that refers to a later field inside a nested \ function is not supported yet
- `backend/bluets/src/checker/module/binding/classes/fields.rs:258` — a static block that refers to a later static field inside a nested \ function is not supported yet
- `backend/bluets/src/checker/module/binding/classes/visibility.rs:499` — cannot prove that access to `{}` is permitted for this receiver
- `backend/bluets/src/checker/module/binding/classes/visibility.rs:580` — cannot prove that access to `{}` is permitted for this receiver
- `backend/bluets/src/checker/module/binding/classes/fields.rs:113` — class field `{}` needs a type annotation unless its initializer \ or default is a number, string or boolean literal
- `backend/bluets/src/checker/module/binding/classes.rs:645` — class tuple rest annotation cannot be specialized within the type budget
- `backend/bluets/src/checker/module/binding/names.rs:167` — cyclic tuple spread cannot be resolved
- `backend/bluets/src/checker/module/binding/classes/fields.rs:383` — definite assignment of `{}` through a branch is not supported yet
- `backend/bluets/src/checker/module/binding/classes/fields.rs:438` — field `{}` redeclares a member of an imported base class, which is not supported yet
- `backend/bluets/src/checker/module/binding/classes/accessors.rs:143` — getter `{}` needs a return type annotation; inferring it from the \ body is not supported yet
- `backend/bluets/src/checker/module/binding.rs:551` — interface heritage {name} must name an interface declaration
- `backend/bluets/src/checker/module/binding/namespaces.rs:963` — namespace `{source}` has no run-time members; import it with `import type`
- `backend/bluets/src/checker/module/binding/classes/accessors.rs:364` — redeclaring `{name}` as an accessor over a member of an \ imported base class is not supported yet
- `backend/bluets/src/checker/module/binding/classes/visibility.rs:302` — redeclaring the protected member `{name}` of an imported base \ class is not supported yet
- `backend/bluets/src/checker/module/binding/classes/accessors.rs:195` — the getter and setter of `{name}` have different types, \ which is not supported yet
- `backend/bluets/src/checker/module/binding/names.rs:171` — tuple spread names an unresolved type
- `backend/bluets/src/checker/module/binding/names.rs:175` — tuple spread requires one concrete tuple or array type

### diagnostic (1)

- `backend/bluets/src/diagnostic/mapping.rs:123` — BlueTSC deliberately refuses a documented subset boundary.

### emitter (37)

- `backend/bluets/src/emitter/decorators.rs:455` — `super` in a static member of a class with decorators is not lowered yet
- `backend/bluets/src/emitter/decorators.rs:697` — a constructor of a decorated class needs its `super(...)` call as a top-level statement
- `backend/bluets/src/emitter/legacy_decorators.rs:87` — a decorated class inside a namespace is not lowered yet
- `backend/bluets/src/emitter/decorators.rs:181` — a decorated class or auto-accessor inside a namespace is not lowered yet
- `backend/bluets/src/emitter/decorators.rs:374` — a decorated private method or accessor is not lowered yet
- `backend/bluets/src/emitter/decorators.rs:310` — a decorator can only decorate a method implementation, not an overload
- `backend/bluets/src/emitter/decorators.rs:286` — a decorator expression with a line comment is not lowered
- `backend/bluets/src/emitter/legacy_decorators.rs:156` — a decorator expression with a line comment is not lowered
- `backend/bluets/src/emitter/class_lowering.rs:377` — a derived constructor whose class needs statements at its start must have a top-level \ `super(...)` statement to place them after
- `backend/bluets/src/emitter/class_lowering.rs:82` — a private name in a class inside a namespace is not lowered for this target yet
- `backend/bluets/src/emitter/class_lowering.rs:386` — a static initializer or block that uses `super` cannot be lowered for this target
- `backend/bluets/src/emitter/namespaces.rs:344` — an exported namespace variable with several declarators is not supported yet
- `backend/bluets/src/emitter/namespaces.rs:359` — an exported namespace variable's name could not be located
- `backend/bluets/src/emitter/commonjs.rs:242` — an exported variable with several declarators is not supported in CommonJS output yet
- `backend/bluets/src/emitter/commonjs.rs:259` — an exported variable's name could not be located
- `backend/bluets/src/emitter/legacy_decorators.rs:215` — auto-accessors are not combined with experimentalDecorators
- `backend/bluets/src/emitter/classes.rs:60` — declaration output for this string literal field needs an annotation
- `backend/bluets/src/emitter/classes.rs:267` — declaration output requires a class field type
- `backend/bluets/src/emitter/classes.rs:204` — declaration output requires a parameter property type
- `backend/bluets/src/emitter/classes.rs:384` — declaration output requires an explicit class method return type
- `backend/bluets/src/emitter/classes.rs:338` — declaration output requires an explicit getter return type
- `backend/bluets/src/emitter/legacy_decorators.rs:569` — decorator metadata cannot serialize `{name}`: only types declared in this module are supported
- `backend/bluets/src/emitter/legacy_decorators.rs:430` — decorator metadata needs this accessor's type annotation
- `backend/bluets/src/emitter/legacy_decorators.rs:378` — decorator metadata needs this field's type
- `backend/bluets/src/emitter/legacy_decorators.rs:391` — decorator metadata needs this method's return type annotation
- `backend/bluets/src/emitter/decorators.rs:187` — decorators and auto-accessors are lowered for target ES2022 with class fields defined (the default); other targets are not supported yet
- `backend/bluets/src/emitter/decorators.rs:349` — decorators are not valid here
- `backend/bluets/src/emitter/legacy_decorators.rs:263` — decorators cannot be applied to both the getter and the setter of the same name
- `backend/bluets/src/emitter/legacy_decorators.rs:246` — decorators on private names are not valid with experimentalDecorators
- `backend/bluets/src/emitter/private_lowering.rs:657` — the identifier `{helper}` is reserved for the private-name helpers
- `backend/bluets/src/emitter/private_lowering.rs:167` — the identifier `{variable}` is needed to lower this class's private names but is already used
- `backend/bluets/src/emitter/decorators.rs:358` — this decorated member shape (computed or literal name, or a form the class parser does not structure) is not lowered
- `backend/bluets/src/emitter/legacy_decorators.rs:226` — this decorated member shape is not lowered
- `backend/bluets/src/emitter/decorators.rs:303` — this decorated method shape is not lowered
- `backend/bluets/src/emitter/private_lowering.rs:326` — this private member could not be located in its own text
- `backend/bluets/src/emitter/private_lowering.rs:318` — this private member has no lowering
- `backend/bluets/src/emitter/private_lowering.rs:564` — {what} cannot be lowered for a target below ES2022

### expression (23)

- `backend/bluets-bluejs/src/expression.rs:349` — a unary expression cannot be the unparenthesized base of exponentiation
- `backend/bluets-bluejs/src/expression.rs:504` — array literals cannot combine holes and spread elements in the v1 direct bridge subset
- `backend/bluets-bluejs/src/expression.rs:515` — array literals cannot combine holes and spread elements in the v1 direct bridge subset
- `backend/bluets-bluejs/src/expression/calls.rs:127` — only a direct identifier receiver may use optional dot access
- `backend/bluets-bluejs/src/expression/calls.rs:140` — only an identifier property is in the first optional dot subset
- `backend/bluets-bluejs/src/expression/calls.rs:53` — only constructor calls with parentheses are in the v1 direct bridge subset
- `backend/bluets-bluejs/src/expression/calls.rs:206` — only direct identifier and property calls are in the v1 direct bridge subset
- `backend/bluets-bluejs/src/expression/writes.rs:71` — only identifier and property assignment targets are in the v1 direct bridge subset
- `backend/bluets-bluejs/src/expression/writes.rs:128` — only identifier and property update targets are in the v1 direct bridge subset
- `backend/bluets-bluejs/src/expression/calls.rs:21` — only identifier constructors are in the v1 direct bridge subset
- `backend/bluets-bluejs/src/expression/calls.rs:161` — only identifier dot property names are in the v1 direct bridge subset
- `backend/bluets-bluejs/src/expression.rs:627` — only identifier object keys may use shorthand; methods are not in the v1 direct bridge subset
- `backend/bluets-bluejs/src/expression.rs:609` — only identifier, string, numeric, and computed object property keys are in the v1 direct bridge subset
- `backend/bluets-bluejs/src/expression.rs:404` — only property delete targets are in the v1 direct bridge subset
- `backend/bluets-bluejs/src/expression.rs:103` — parentheses are required when mixing `??` with `&&` or `||`
- `backend/bluets-bluejs/src/expression.rs:111` — parentheses are required when mixing `??` with `&&` or `||`
- `backend/bluets-bluejs/src/expression.rs:857` — unsupported expression token `?.` in a template substitution
- `backend/bluets-bluejs/src/expression.rs:30` — unsupported expression token `{}`
- `backend/bluets-bluejs/src/expression.rs:451` — unsupported keyword `{}` in a runtime expression
- `backend/bluets-bluejs/src/expression.rs:436` — unsupported numeric literal
- `backend/bluets-bluejs/src/expression.rs:600` — unsupported numeric object key
- `backend/bluets-bluejs/src/expression.rs:478` — unsupported runtime expression token `{}`
- `backend/bluets-bluejs/src/expression.rs:730` — unsupported string escape in the v1 direct bridge subset

### jsx_direct (8)

- `backend/bluets-bluejs/src/jsx_direct.rs:110` — JSX needs the `jsx` option
- `backend/bluets-bluejs/src/jsx_direct.rs:275` — a JSX element has fewer embedded expressions than its syntax
- `backend/bluets-bluejs/src/jsx_direct.rs:132` — a JSX element is missing its expression group
- `backend/bluets-bluejs/src/jsx_direct.rs:140` — a JSX element is missing its expression group
- `backend/bluets-bluejs/src/jsx_direct.rs:150` — a JSX element's expression group is unterminated
- `backend/bluets-bluejs/src/jsx_direct.rs:113` — cannot use JSX unless the `jsx` option is provided
- `backend/bluets-bluejs/src/jsx_direct.rs:115` — preserved JSX is not executable; choose `jsx: react` for direct execution
- `backend/bluets-bluejs/src/jsx_direct.rs:121` — the automatic JSX runtime imports a runtime module, which the direct bridge does not link; use the classic mode with an in-program factory

### lib (11)

- `backend/bluets-bluejs/src/lib.rs:742` — BlueTS did not retain a canonical target for this runtime import
- `backend/bluets-bluejs/src/lib.rs:346` — BlueTS rejected the direct script with {} diagnostic(s)
- `backend/bluets-bluejs/src/lib.rs:813` — a declaration module cannot be executed directly
- `backend/bluets-bluejs/src/lib.rs:696` — a declaration module cannot be the direct module-graph entry
- `backend/bluets-bluejs/src/lib.rs:727` — a declaration module cannot be the direct module-graph entry
- `backend/bluets-bluejs/src/lib.rs:770` — the direct bridge executes ECMAScript modules only; a CommonJS project is run as \ `--module commonjs` emitted output in a realm with a host-provided CommonJS loader
- `backend/bluets-bluejs/src/lib.rs:748` — the direct bridge links no installed packages: a runtime import of a package \ needs the host-provided module loader of the `bluetsc build` route
- `backend/bluets-bluejs/src/lib.rs:807` — the requested entry was not retained in the checked source graph
- `backend/bluets-bluejs/src/lib.rs:721` — the requested module-graph entry was not retained in the checked source graph
- `backend/bluets-bluejs/src/lib.rs:796` — the v1 direct bridge supports exactly one executable source module
- `backend/bluets-bluejs/src/lib.rs:363` — {}:{}:{} cannot lower to the current BlueJS bridge: {message}

### lowering (32)

- `backend/bluets-bluejs/src/lowering/classes.rs:29` — BlueJS implements the standard decorators; `experimentalDecorators` programs run as `bluetsc build --experimental-decorators` output
- `backend/bluets-bluejs/src/lowering.rs:227` — BlueTS did not retain a canonical target for this runtime import
- `backend/bluets-bluejs/src/lowering.rs:52` — ESM default exports require the module bridge
- `backend/bluets-bluejs/src/lowering.rs:58` — ESM named exports require the module bridge
- `backend/bluets-bluejs/src/lowering/classes.rs:268` — a derived constructor needs a top-level super call for the statements that must follow it
- `backend/bluets-bluejs/src/lowering.rs:576` — a nested function declaration is outside the direct try subset
- `backend/bluets-bluejs/src/lowering.rs:629` — a nested function declaration is outside the direct while subset
- `backend/bluets-bluejs/src/lowering.rs:404` — a rest parameter must be the final direct function parameter
- `backend/bluets-bluejs/src/lowering/namespaces.rs:46` — an ambient const enum needs its uses inlined, which the direct bridge does not do
- `backend/bluets-bluejs/src/lowering.rs:97` — an exported class requires the module bridge
- `backend/bluets-bluejs/src/lowering.rs:125` — an exported enum requires the module bridge
- `backend/bluets-bluejs/src/lowering.rs:103` — an exported namespace requires the module bridge
- `backend/bluets-bluejs/src/lowering.rs:557` — block-local declarations are outside the direct try subset
- `backend/bluets-bluejs/src/lowering.rs:582` — body syntax is outside the direct try subset
- `backend/bluets-bluejs/src/lowering.rs:70` — declared or exported variables require a non-script bridge mode
- `backend/bluets-bluejs/src/lowering.rs:262` — declared or overloaded functions cannot be lowered to a direct module
- `backend/bluets-bluejs/src/lowering.rs:256` — declared variables cannot be lowered to a direct module
- `backend/bluets-bluejs/src/lowering.rs:86` — declared, overloaded, or exported functions require a non-script bridge mode
- `backend/bluets-bluejs/src/lowering/classes.rs:117` — decorators and auto-accessors need class fields to be defined (the default for ES2022)
- `backend/bluets-bluejs/src/lowering.rs:398` — destructured parameters are not yet in the v1 direct bridge subset
- `backend/bluets-bluejs/src/lowering.rs:471` — function body syntax is not yet in the v1 direct bridge subset
- `backend/bluets-bluejs/src/lowering.rs:635` — loop body syntax is outside the direct while subset
- `backend/bluets-bluejs/src/lowering.rs:608` — loop-local declarations are outside the direct while subset
- `backend/bluets-bluejs/src/lowering.rs:563` — loops are outside the direct try subset
- `backend/bluets-bluejs/src/lowering.rs:465` — nested function declarations are not yet in the v1 direct bridge subset
- `backend/bluets-bluejs/src/lowering.rs:614` — nested loops are outside the direct while subset
- `backend/bluets-bluejs/src/lowering.rs:569` — nested try statements are outside the direct try subset
- `backend/bluets-bluejs/src/lowering.rs:218` — runtime imports require the direct module-graph bridge
- `backend/bluets-bluejs/src/lowering.rs:64` — runtime imports require the module bridge
- `backend/bluets-bluejs/src/lowering/classes.rs:144` — this class member has no direct lowering
- `backend/bluets-bluejs/src/lowering/namespaces.rs:376` — this declaration cannot appear in a namespace body lowered directly
- `backend/bluets-bluejs/src/lowering.rs:620` — try statements are outside the direct while body subset

### namespace_analysis (1)

- `backend/bluets/src/namespace_analysis.rs:542` — a template literal that reads an exported namespace variable is not lowered directly yet

### parser (28)

- `backend/bluets/src/parser/declarations/erasure_audit.rs:83` — TypeScript annotations on functions nested in expressions are not supported yet
- `backend/bluets/src/parser/declarations.rs:401` — TypeScript assertions outside a supported declaration are not in the initial BlueTS matrix
- `backend/bluets/src/parser/declarations.rs:222` — `abstract` declarations are not in the initial BlueTS matrix
- `backend/bluets/src/parser/declarations.rs:310` — `{}` is not in the initial BlueTS matrix
- `backend/bluets/src/parser/declarations.rs:257` — an async enum is not valid
- `backend/bluets/src/parser/declarations.rs:269` — an async enum is not valid
- `backend/bluets/src/parser/declarations/typed_declarations.rs:211` — an async generator is not supported yet
- `backend/bluets/src/parser/declarations.rs:294` — an async namespace is not valid
- `backend/bluets/src/parser/declarations/enums.rs:53` — an enum member name with an escape sequence is not supported yet
- `backend/bluets/src/parser/declarations.rs:150` — an import, `export default`, `export =`, `export *` or export list inside a namespace body is not supported
- `backend/bluets/src/parser/declarations.rs:164` — anonymous default function exports are not in the initial BlueTS matrix
- `backend/bluets/src/parser/declarations.rs:242` — declared and async classes are not in the first class form
- `backend/bluets/src/parser/declarations.rs:325` — decorators and TSX/JSX are not in the initial BlueTS matrix
- `backend/bluets/src/parser/declarations/imports_exports.rs:293` — default aliases in named value exports are not in the initial BlueTS matrix
- `backend/bluets/src/parser/declarations.rs:183` — default export expressions are not in the initial BlueTS matrix
- `backend/bluets/src/parser/declarations/class.rs:58` — generic, computed, and implemented class heritage is not in the first class form
- `backend/bluets/src/parser/declarations/typed_declarations.rs:55` — interface heritage supports only named interface types
- `backend/bluets/src/parser/declarations/imports_exports.rs:147` — mixed value/type imports are not in the initial BlueTS matrix; use a separate `import type` declaration
- `backend/bluets/src/parser/declarations/imports_exports.rs:183` — only `export = name;` of a local declaration is supported
- `backend/bluets/src/parser/declarations/imports_exports.rs:27` — only `import name = require("module")` is supported as an `import =` form
- `backend/bluets/src/parser/declarations/typed_declarations.rs:135` — optional variables are not valid TypeScript declarations
- `backend/bluets/src/parser/type_syntax.rs:17` — rest parameters in method signatures are not supported
- `backend/bluets/src/parser/declarations/typed_declarations.rs:380` — this destructuring pattern is not supported yet
- `backend/bluets/src/parser/type_syntax.rs:286` — tuple rest element must have an array or named tuple annotation
- `backend/bluets/src/parser/declarations/imports_exports.rs:278` — type-only bindings in a value export are not in the initial BlueTS matrix; use `export type`
- `backend/bluets/src/parser/declarations/source_edits.rs:85` — typed arrow parameters are not in the initial BlueTS matrix
- `backend/bluets/src/parser/declarations.rs:196` — value re-exports from another module are not in the initial BlueTS matrix
- `backend/bluets/src/parser/declarations/imports_exports.rs:309` — value re-exports from another module are not in the initial BlueTS matrix
