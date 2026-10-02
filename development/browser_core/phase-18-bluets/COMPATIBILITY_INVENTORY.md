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
| `option_combinations_oracle.rs` | one program over every combination of `target`, module system, `useDefineForClassFields`, `preserveConstEnums`, `isolatedModules` | 48 combinations |
| `bluets-bluejs/tests/namespace_parity.rs`, `jsx_direct.rs`, `decorators_direct.rs` | the direct runtime (BlueJS) against Node running `tsc`'s output | 7 + 7 + 6 programs |

The fixture corpus under `tests/fixtures/typescript_oracle/` has 994 directories; each is an
entry whose verdict was recorded from the pinned compiler by an ignored test
(`BLUEICE_WRITE_*_MATRIX=1`), and the ordinary (non-ignored) tests replay the recorded verdicts offline.

What is **not** compared byte for byte, by design: diagnostics are matched by verdict and BlueTS's own
code (`BTSnnnn`), not by TypeScript's `TSnnnn` number or text; source maps are line-level provenance, not
`tsc`'s token-level maps (they are validated structurally and by line, not diffed); emitted text is compared
by running it (and, for declarations, by text where TypeScript's output is deterministic), not by
whitespace.

### Measured pass rate (2026-10-02, macOS 26 / Apple silicon, pinned `typescript@5.9.3`, Node 26)

Every differential suite of section 1 was run against the pinned compiler with none skipped:
**19 of 19 suites pass, 0 failures.** The versioned case list is the repository itself at the commit that
carries this file: 760 recorded accepted/rejected verdicts (class 523, namespace 76, enum 57, JSX 53,
decorators 32, legacy decorators 19), 109 core-subset cases in `typescript_oracle.rs`, and the emit-and-run
programs (JSX 11, standard decorators 20, legacy decorators 18, CommonJS 6, option combinations 48,
namespace/enum/class downlevel under both targets, direct-runtime parity 20). The same suites run in CI on
Ubuntu 24.04 and macOS 15; Windows is not covered (G-M3). The percentage means only: *of the forms inside
the subset, every one agrees with `tsc`*. It says nothing about the forms in section 3, which are refused.

## 2. What the supported subset is (summary)

Statements and expressions with a typed core (numbers, strings, booleans, literals, arrays, tuples,
records and interfaces, unions, intersections, bounded generics, function types, async functions,
generators); ES module and CommonJS emit; classes (fields, accessors, visibility, static blocks, private
names, parameter properties, class+interface merging); enums and `const enum`; namespaces (nested, merged
with classes, enums, functions); `.tsx` JSX in every `jsx` mode; standard and legacy decorators; installed
packages through `node10`/`node16`/`bundler` resolution inside owner-authorized roots; pinned remote
declarations. The precise per-feature descriptions, with their recorded gaps, are the `J.x` sections of
`PLAN.md`.

## 3. Open gaps against `tsc` 5.9.3

Every row is a form `tsc` accepts and BlueTS does not (or accepts with different output), by area. All are
**required** for a full-parity claim. None is hidden: each is either refused with a diagnostic (a
section 4 entry) or is a silent divergence listed as such. IDs are stable; closing one means a differential
suite entry, then removing the row.

### 3.1 Type system

| ID | Gap |
| --- | --- |
| G-T1 | Control-flow narrowing (`typeof`/`instanceof`/`in`/discriminant/truthiness guards, assertion functions, user-defined type guards) beyond the few shapes the checker models. |
| G-T2 | Generic inference in general: inference from arguments for arbitrary generic functions and classes, constraints with `keyof`/indexed access, higher-kinded patterns, variance annotations. |
| G-T3 | Conditional types, `infer`, mapped types, template-literal types, indexed access and `keyof`/`typeof` type operators in full, recursive types beyond the type budget. |
| G-T4 | Overload resolution in full (call and construct signatures, contextual signature selection), optional chaining and non-null assertion typing in all positions. |
| G-T5 | Index signatures, `readonly` arrays/tuples, `unique symbol`, `bigint`, `symbol`, `never`/`unknown` flow rules, `satisfies`, `as const`, enum-like literal inference. |
| G-T6 | Structural-compatibility details: excess-property checks in every position, weak types, optional/exactOptional rules, `strictFunctionTypes` and method bivariance, `exactOptionalPropertyTypes`, `noUncheckedIndexedAccess`. |
| G-T7 | `lib.d.ts`: there is no standard library declaration set; host types come from the owner-supplied declarations, so ECMAScript/DOM globals are not typed as in `tsc`. Unknown identifiers are not diagnosed (`PLAN.md`, recorded gap). |
| G-T8 | Inferred return types of declarations without annotations (getters, methods, exported functions in `.d.ts` emit), widening and literal freshness in all positions. |
| G-T9 | `abstract` classes, `implements` clauses, generic classes and heritage, class expressions, computed class member names, `declare` fields, `override`/`noImplicitOverride`, index-signature members. |
| G-T10 | `import type`/`export type` forms beyond the supported ones, `export default <expression>`, value re-exports (`export * from`, `export { x } from`), import attributes, `export as namespace`, `declare module` augmentation and ambient module declarations, global augmentation, triple-slash directives, `unique` declaration merging beyond class+interface+namespace. |

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
| G-D1 | `.d.ts` emit requires explicit types where `tsc` infers them (section 4, `emitter`); generic declarations, overloads of arbitrary shape, `declare module`, namespaces with merged symbols in every shape, `export =` of non-trivial forms. |
| G-D2 | Declaration emit options (`declarationDir`, `emitDeclarationOnly`, `declarationMap`, `stripInternal`). |

### 3.4 Modules, packages, projects

| ID | Gap |
| --- | --- |
| G-M1 | Resolution features not read: `typesVersions`, `.d.mts`/`.d.cts`/`.mts`/`.cts` entry points, `paths`/`baseUrl`/`rootDirs`, package self-name imports, `moduleResolution: classic`, automatic `@types` inclusion (`types`/`typeRoots`), `resolveJsonModule`, `allowJs`/`checkJs`, `.js` files as sources. |
| G-M2 | Project-level features: `tsconfig.json` (BlueTSC reads its own `bluetsc.json`, not `tsconfig`), `extends`, `include`/`exclude`/`files`, project references, `--build`, `--watch`, `--incremental`/`.tsbuildinfo`, `--noEmit`/`--listFiles`/`--showConfig`, `composite`. |
| G-M3 | Windows: the oracle suites are not run on Windows (the harness does not launch `tsc.cmd`; symlink tests are Unix-only); the path code uses `std::fs::canonicalize` and `Path` and has been type-checked, not run, there. |

### 3.5 Diagnostics and CLI

| ID | Gap |
| --- | --- |
| G-C1 | Diagnostic parity: BlueTSC reports `BTSnnnn` with its own wording and spans; TypeScript's `TSnnnn` codes, messages, related information and suggestions, `--pretty`, `--diagnostics`, `--explainFiles`, and the full `tsc` command line are not provided. |
| G-C2 | Strictness flags are fixed (the strict family is on): `strict`, `noImplicitAny`, `strictNullChecks`, `noUnusedLocals`/`Parameters`, `noImplicitReturns`, `noFallthroughCasesInSwitch`, `useUnknownInCatchVariables` and the rest are not independently selectable. |
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

162 refusal sites in 8 areas

### checker (22)

- `backend/bluets/src/checker/module/binding.rs:531` — `export =` cannot be used when the module system is ECMAScript; use `--module commonjs`
- `backend/bluets/src/checker/module/binding.rs:523` — `import x = require()` cannot be used when the module system is ECMAScript; use `--module commonjs`
- `backend/bluets/src/checker/module/binding.rs:226` — a class member other than a constructor, method, field or accessor \ (a computed, generator or `accessor` member) is not supported yet
- `backend/bluets/src/checker/module/binding/enums.rs:214` — a computed initializer that refers to the member `{}` must write it \ as `{}.{}`
- `backend/bluets/src/checker/module/decorators.rs:116` — a decorator can only decorate a method implementation, not an overload
- `backend/bluets/src/checker/module/binding/classes/fields.rs:318` — a field initializer that refers to a later field inside a nested \ function is not supported yet
- `backend/bluets/src/checker/module/binding/classes/fields.rs:249` — a static block that refers to a later static field inside a nested \ function is not supported yet
- `backend/bluets/src/checker/module/binding/classes/visibility.rs:490` — cannot prove that access to `{}` is permitted for this receiver
- `backend/bluets/src/checker/module/binding/classes/visibility.rs:544` — cannot prove that access to `{}` is permitted for this receiver
- `backend/bluets/src/checker/module/binding/classes/fields.rs:104` — class field `{}` needs a type annotation unless its initializer \ or default is a number, string or boolean literal
- `backend/bluets/src/checker/module/binding/classes.rs:621` — class tuple rest annotation cannot be specialized within the type budget
- `backend/bluets/src/checker/module/binding.rs:1249` — cyclic tuple spread cannot be resolved
- `backend/bluets/src/checker/module/binding/classes/fields.rs:371` — definite assignment of `{}` through a branch is not supported yet
- `backend/bluets/src/checker/module/binding/classes/fields.rs:426` — field `{}` redeclares a member of an imported base class, which is not supported yet
- `backend/bluets/src/checker/module/binding/classes/accessors.rs:125` — getter `{}` needs a return type annotation; inferring it from the \ body is not supported yet
- `backend/bluets/src/checker/module/binding.rs:1026` — interface heritage {name} must name an interface declaration
- `backend/bluets/src/checker/module/binding/namespaces.rs:1038` — namespace `{source}` has no run-time members; import it with `import type`
- `backend/bluets/src/checker/module/binding/classes/accessors.rs:346` — redeclaring `{name}` as an accessor over a member of an \ imported base class is not supported yet
- `backend/bluets/src/checker/module/binding/classes/visibility.rs:302` — redeclaring the protected member `{name}` of an imported base \ class is not supported yet
- `backend/bluets/src/checker/module/binding/classes/accessors.rs:177` — the getter and setter of `{name}` have different types, \ which is not supported yet
- `backend/bluets/src/checker/module/binding.rs:1253` — tuple spread names an unresolved type
- `backend/bluets/src/checker/module/binding.rs:1257` — tuple spread requires one concrete tuple or array type

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
- `backend/bluets/src/emitter/classes.rs:265` — declaration output requires a class field type
- `backend/bluets/src/emitter/classes.rs:202` — declaration output requires a parameter property type
- `backend/bluets/src/emitter/classes.rs:382` — declaration output requires an explicit class method return type
- `backend/bluets/src/emitter/classes.rs:336` — declaration output requires an explicit getter return type
- `backend/bluets/src/emitter/legacy_decorators.rs:563` — decorator metadata cannot serialize `{name}`: only types declared in this module are supported
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

- `backend/bluets-bluejs/src/expression.rs:419` — a unary expression cannot be the unparenthesized base of exponentiation
- `backend/bluets-bluejs/src/expression.rs:631` — array literals cannot combine holes and spread elements in the v1 direct bridge subset
- `backend/bluets-bluejs/src/expression.rs:642` — array literals cannot combine holes and spread elements in the v1 direct bridge subset
- `backend/bluets-bluejs/src/expression.rs:901` — only a direct identifier receiver may use optional dot access
- `backend/bluets-bluejs/src/expression.rs:914` — only an identifier property is in the first optional dot subset
- `backend/bluets-bluejs/src/expression.rs:715` — only constructor calls with parentheses are in the v1 direct bridge subset
- `backend/bluets-bluejs/src/expression.rs:980` — only direct identifier and property calls are in the v1 direct bridge subset
- `backend/bluets-bluejs/src/expression.rs:117` — only identifier and property assignment targets are in the v1 direct bridge subset
- `backend/bluets-bluejs/src/expression.rs:532` — only identifier and property update targets are in the v1 direct bridge subset
- `backend/bluets-bluejs/src/expression.rs:683` — only identifier constructors are in the v1 direct bridge subset
- `backend/bluets-bluejs/src/expression.rs:935` — only identifier dot property names are in the v1 direct bridge subset
- `backend/bluets-bluejs/src/expression.rs:853` — only identifier object keys may use shorthand; methods are not in the v1 direct bridge subset
- `backend/bluets-bluejs/src/expression.rs:835` — only identifier, string, numeric, and computed object property keys are in the v1 direct bridge subset
- `backend/bluets-bluejs/src/expression.rs:474` — only property delete targets are in the v1 direct bridge subset
- `backend/bluets-bluejs/src/expression.rs:173` — parentheses are required when mixing `??` with `&&` or `||`
- `backend/bluets-bluejs/src/expression.rs:181` — parentheses are required when mixing `??` with `&&` or `||`
- `backend/bluets-bluejs/src/expression.rs:1224` — unsupported expression token `?.` in a template substitution
- `backend/bluets-bluejs/src/expression.rs:27` — unsupported expression token `{}`
- `backend/bluets-bluejs/src/expression.rs:578` — unsupported keyword `{}` in a runtime expression
- `backend/bluets-bluejs/src/expression.rs:563` — unsupported numeric literal
- `backend/bluets-bluejs/src/expression.rs:826` — unsupported numeric object key
- `backend/bluets-bluejs/src/expression.rs:605` — unsupported runtime expression token `{}`
- `backend/bluets-bluejs/src/expression.rs:1097` — unsupported string escape in the v1 direct bridge subset

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

- `backend/bluets-bluejs/src/lowering.rs:401` — BlueJS implements the standard decorators; `experimentalDecorators` programs run as `bluetsc build --experimental-decorators` output
- `backend/bluets-bluejs/src/lowering.rs:221` — BlueTS did not retain a canonical target for this runtime import
- `backend/bluets-bluejs/src/lowering.rs:46` — ESM default exports require the module bridge
- `backend/bluets-bluejs/src/lowering.rs:52` — ESM named exports require the module bridge
- `backend/bluets-bluejs/src/lowering.rs:806` — a derived constructor needs a top-level super call for the statements that must follow it
- `backend/bluets-bluejs/src/lowering.rs:1021` — a nested function declaration is outside the direct try subset
- `backend/bluets-bluejs/src/lowering.rs:1074` — a nested function declaration is outside the direct while subset
- `backend/bluets-bluejs/src/lowering.rs:849` — a rest parameter must be the final direct function parameter
- `backend/bluets-bluejs/src/lowering.rs:611` — an ambient const enum needs its uses inlined, which the direct bridge does not do
- `backend/bluets-bluejs/src/lowering.rs:91` — an exported class requires the module bridge
- `backend/bluets-bluejs/src/lowering.rs:119` — an exported enum requires the module bridge
- `backend/bluets-bluejs/src/lowering.rs:97` — an exported namespace requires the module bridge
- `backend/bluets-bluejs/src/lowering.rs:1002` — block-local declarations are outside the direct try subset
- `backend/bluets-bluejs/src/lowering.rs:1027` — body syntax is outside the direct try subset
- `backend/bluets-bluejs/src/lowering.rs:64` — declared or exported variables require a non-script bridge mode
- `backend/bluets-bluejs/src/lowering.rs:256` — declared or overloaded functions cannot be lowered to a direct module
- `backend/bluets-bluejs/src/lowering.rs:250` — declared variables cannot be lowered to a direct module
- `backend/bluets-bluejs/src/lowering.rs:80` — declared, overloaded, or exported functions require a non-script bridge mode
- `backend/bluets-bluejs/src/lowering.rs:489` — decorators and auto-accessors need class fields to be defined (the default for ES2022)
- `backend/bluets-bluejs/src/lowering.rs:843` — destructured parameters are not yet in the v1 direct bridge subset
- `backend/bluets-bluejs/src/lowering.rs:916` — function body syntax is not yet in the v1 direct bridge subset
- `backend/bluets-bluejs/src/lowering.rs:1080` — loop body syntax is outside the direct while subset
- `backend/bluets-bluejs/src/lowering.rs:1053` — loop-local declarations are outside the direct while subset
- `backend/bluets-bluejs/src/lowering.rs:1008` — loops are outside the direct try subset
- `backend/bluets-bluejs/src/lowering.rs:910` — nested function declarations are not yet in the v1 direct bridge subset
- `backend/bluets-bluejs/src/lowering.rs:1059` — nested loops are outside the direct while subset
- `backend/bluets-bluejs/src/lowering.rs:1014` — nested try statements are outside the direct try subset
- `backend/bluets-bluejs/src/lowering.rs:212` — runtime imports require the direct module-graph bridge
- `backend/bluets-bluejs/src/lowering.rs:58` — runtime imports require the module bridge
- `backend/bluets-bluejs/src/lowering.rs:516` — this class member has no direct lowering
- `backend/bluets-bluejs/src/lowering.rs:1331` — this declaration cannot appear in a namespace body lowered directly
- `backend/bluets-bluejs/src/lowering.rs:1065` — try statements are outside the direct while body subset

### namespace_analysis (1)

- `backend/bluets/src/namespace_analysis.rs:542` — a template literal that reads an exported namespace variable is not lowered directly yet

### parser (28)

- `backend/bluets/src/parser/declarations/erasure_audit.rs:83` — TypeScript annotations on functions nested in expressions are not supported yet
- `backend/bluets/src/parser/declarations.rs:397` — TypeScript assertions outside a supported declaration are not in the initial BlueTS matrix
- `backend/bluets/src/parser/declarations.rs:218` — `abstract` declarations are not in the initial BlueTS matrix
- `backend/bluets/src/parser/declarations.rs:306` — `{}` is not in the initial BlueTS matrix
- `backend/bluets/src/parser/declarations.rs:253` — an async enum is not valid
- `backend/bluets/src/parser/declarations.rs:265` — an async enum is not valid
- `backend/bluets/src/parser/declarations/typed_declarations.rs:204` — an async generator is not supported yet
- `backend/bluets/src/parser/declarations.rs:290` — an async namespace is not valid
- `backend/bluets/src/parser/declarations/enums.rs:53` — an enum member name with an escape sequence is not supported yet
- `backend/bluets/src/parser/declarations.rs:146` — an import, `export default`, `export =`, `export *` or export list inside a namespace body is not supported
- `backend/bluets/src/parser/declarations.rs:160` — anonymous default function exports are not in the initial BlueTS matrix
- `backend/bluets/src/parser/declarations.rs:238` — declared and async classes are not in the first class form
- `backend/bluets/src/parser/declarations.rs:321` — decorators and TSX/JSX are not in the initial BlueTS matrix
- `backend/bluets/src/parser/declarations/imports_exports.rs:285` — default aliases in named value exports are not in the initial BlueTS matrix
- `backend/bluets/src/parser/declarations.rs:179` — default export expressions are not in the initial BlueTS matrix
- `backend/bluets/src/parser/declarations/class.rs:49` — generic, computed, and implemented class heritage is not in the first class form
- `backend/bluets/src/parser/declarations/typed_declarations.rs:49` — interface heritage supports only named interface types
- `backend/bluets/src/parser/declarations/imports_exports.rs:143` — mixed value/type imports are not in the initial BlueTS matrix; use a separate `import type` declaration
- `backend/bluets/src/parser/declarations/imports_exports.rs:179` — only `export = name;` of a local declaration is supported
- `backend/bluets/src/parser/declarations/imports_exports.rs:27` — only `import name = require("module")` is supported as an `import =` form
- `backend/bluets/src/parser/declarations/typed_declarations.rs:129` — optional variables are not valid TypeScript declarations
- `backend/bluets/src/parser/type_syntax.rs:17` — rest parameters in method signatures are not supported
- `backend/bluets/src/parser/declarations/typed_declarations.rs:369` — this destructuring pattern is not supported yet
- `backend/bluets/src/parser/type_syntax.rs:194` — tuple rest element must have an array or named tuple annotation
- `backend/bluets/src/parser/declarations/imports_exports.rs:270` — type-only bindings in a value export are not in the initial BlueTS matrix; use `export type`
- `backend/bluets/src/parser/declarations/source_edits.rs:52` — typed arrow parameters are not in the initial BlueTS matrix
- `backend/bluets/src/parser/declarations.rs:192` — value re-exports from another module are not in the initial BlueTS matrix
- `backend/bluets/src/parser/declarations/imports_exports.rs:301` — value re-exports from another module are not in the initial BlueTS matrix

