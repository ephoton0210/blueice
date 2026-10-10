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
| `class_modifiers_checker_matrix.rs` and `class_modifiers.rs` | abstract/implements/override origins, structural verdicts and exact primary/related diagnostics, constructor capability aliases, runtime and declarations | 95 programs, 12 further capability controls, 5 runtime and 6 declaration witnesses |
| `class_dynamic_checker_matrix.rs` and `bluets-bluejs/tests/class_dynamic_direct.rs` | generic heritage/methods, captured class expressions and constructor aliases, computed/indexed members, exact primary/related diagnostics, both field modes and ES2020/ES2022 expression lowering | 160 programs, 30 runtime and 36 declaration witnesses, 4 direct tests |
| `class_retirement_checker_matrix.rs` and `bluets-bluejs/tests/class_retirement_direct.rs` | separate accessor read/write contracts, inferred getters, imported member kinds, merged interface heritage and constructor branch initialization; exact diagnostics, execution and declarations | 35 programs, 16 runtime and 17 declaration witnesses, 3 direct tests |
| `module_exports_checker_matrix.rs` and `bluets-bluejs/tests/module_exports_direct.rs` | default expressions/declarations, live named/star/namespace re-exports, exact diagnostics, execution and declarations, incremental type refresh and direct default-name/snapshot semantics | 74 programs, 41 runtime and 47 declaration witnesses, 5 direct tests and 2 boundary/cache controls |
| `module_types_checker_matrix.rs`, `module_type_resolution.rs`, `contextual_type_imports.rs`, `json_default_binding.rs` and `bluets-bluejs/tests/module_types_direct.rs` | whole/inline type forms, attributes, qualified static surfaces, UMD declarations, exact diagnostics, Node/declarations, per-edge conditions and contextual binding names | 86 matrix programs, 54 runtime and 60 declaration witnesses, 8 further pinned programs and 5 direct controls |
| `ambient_checker_matrix.rs`, `ambient_static_boundaries.rs`, `ambient_static_cache.rs`, `ambient_incremental.rs` and `ambient_reexport_boundaries.rs` | named/merged/shorthand ambient surfaces, module/global augmentation, imported declaration contexts, bounded path/type/lib directives, type queries and re-exports; exact diagnostics, Node/declarations and owner/resource/runtime/cache controls | 80 programs, 44 runtime and declaration witnesses, 16 public controls |
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
| `declaration_inference.rs` | exact inferred/generic/overload/accessor/merged-namespace/export-assignment/ambient declarations, literal freshness and const-parameter positions; exact rejected primaries and every actual emitted execution/syntax observation | 84 configurations (77 accept, 7 reject), public checked namespace-symbol regression |
| `declaration_options.rs` and `declaration_option_boundaries.rs` | exact declaration-only/directory/map artifacts and isolated-declaration diagnostics; actual emitted execution/syntax, live Native recorder, CLI override and owner-root publication guards | 48 configurations (28 accept, 20 reject), three CLI boundary tests |
| `project_config.rs` | JSONC inheritance, normalized compiler options and selected files; owner overlays and confinement; exact declarations and Node runtime, including strict-helper relocation | 69 configurations (59 accept, 10 reject), 2 linked programs |
| `strictness_flags.rs`, `strictness_controls.rs` | independent parent/strict-family and additional diagnostics, lexical/return/call/index boundaries, unchanged valid JavaScript and cache policy identity; pinned Node execution and exact declarations | 32 configurations (16 accept, 16 reject), 39 boundary controls, 1 program in 2 policies |
| `cli_surface.rs` | native project CLI flags and overrides, default project discovery, normalized configuration, source/emitted lists, noEmit and pretty/exit observations; input preservation, Node execution and exact declarations | 30 observations (24 accept, 6 reject), 2 emission layouts |
| `option_combinations_oracle.rs` | one linked multi-feature program over target, module system, default/explicit class-field policy, preservation, isolated modules, sourceMap, declaration, noEmit and strict; project verdicts, artifact inventory, Node output, exact declarations and source-map structure | 768 configurations (384 emitted, 384 noEmit) |
| `bluets-bluejs/tests/namespace_parity.rs`, `jsx_direct.rs`, `decorators_direct.rs` | the direct runtime (BlueJS) against Node running `tsc`'s output | 7 + 7 + 6 programs |
| `diagnostic_source_families.rs` | additional lexical/semantic diagnostic source families, exact primary codes and positions | 97 controls |
| `diagnostics_matrix.rs` | verdicts, TypeScript primary code/template, rendered message, UTF-16 position and related information | 2,544 programs, 217 templates, 1,289 rejected primaries |
| `diagnostics_projects.rs` | project/configuration, CLI and option-combination diagnostic positions | 867 observations |
| `diagnostics_presentation.rs` | actual plain/pretty CLI presentation, summaries, related source context, exit status and artifacts | 22 observations |
| `guards_checker_matrix.rs` | predicate/assertion signatures, lexical call effects, receiver guards and exhaustive completion; exact primary/related diagnostics, Node behavior and every emitted declaration | 75 programs, 5 runtime/declaration witnesses |
| `inference_checker_matrix.rs` | bounded argument/contextual candidates, constraints/defaults, explicit arguments, generic callable/class/interface signatures and imported boundaries; exact primary/related diagnostics, every emitted declaration, Node behavior and owner budget refusal | 74 programs, 4 runtime/declaration witnesses |
| `overloads_checker_matrix.rs` | ordered call/construct/method candidates, callback context, literal specialization, merged interface groups and implementation compatibility; exact primary/related diagnostics, every emitted declaration and Node behavior | 74 programs, 6 runtime/declaration witnesses |
| `narrowing_checker_matrix.rs` | lexical control-flow verdicts and exact primary diagnostics; Node behavior and exact declarations | 123 programs, 14 runtime/declaration witnesses |
| `operators_checker_matrix.rs` | bounded keyof/indexed/conditional/infer/mapped/template/recursive operators, exact diagnostics and declarations, literal ordering and expansion limits | 82 programs, 3 runtime/declaration witnesses, 6 budget controls |
| `more_types_checker_matrix.rs` | readonly/indexed containers, bigint/symbol, const/satisfies, tuple metadata, unknown/never, enum and void/undefined forms; exact diagnostics, execution and declarations | 100 programs, 6 runtime/declaration witnesses |
| `compatibility_checker_matrix.rs` | freshness, weak records, function/method variance, optional/undefined, generic and callable/indexed structural relations; exact diagnostics, execution and declarations | 88 programs, 4 runtime/declaration witnesses |

The fixture corpus under `tests/fixtures/typescript_oracle/` has 2754 top-level directories; each is an
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

The K.4.4 final frozen-source Linux gate passes all 1,114 tests in 71 groups,
including all 134 ignored oracles in 36 differential suite files. All 74 ordered
overload programs and six runtime/declaration witnesses pass. The shared
diagnostic corpus contains 1,744 programs and 147 templates.

The K.4.5 final frozen-source Linux gate passes all 1,120 tests in 72 groups,
including all 137 ignored oracles in 37 differential suite files. All 82 operator
programs, three exact runtime/declaration witnesses, both pinned recorders and
six budget controls pass. The shared diagnostic corpus contains 1,826 programs
and 152 templates.

The K.4.6 frozen-source Linux gate passes all 1,124 tests in 73 groups,
including all 139 ignored oracles in 38 differential suite files. All 100 more-type
programs and six exact runtime/declaration witnesses pass. The shared diagnostic
corpus contains 1,926 programs and 164 templates.

The K.4.7 frozen-source Linux gate passes all 1,128 tests in 74 groups,
including all 141 ignored oracles in 39 differential suite files. All 88
compatibility programs and four exact runtime/declaration witnesses pass. The
shared diagnostic corpus contains 2,014 programs and 167 templates.

The K.5.2 frozen-source Linux gate passes all 1,151 tests in 79 groups,
including all 146 ignored oracles in 42 suite files. Its 160 pinned class
programs, 30 runtime and 36 exact declaration witnesses pass.

The K.5.3 frozen-source Linux gate passes all 1,158 tests in 81 groups,
including all 148 ignored oracles in 43 suite files. Its 35 pinned programs,
16 Node runtime, 17 exact declaration witnesses and three direct regressions
pass. Three obsolete deferred class fixtures return to the ordinary matrix.
The shared diagnostic corpus contains 2,304 programs and 195 templates.
Hosted workspace verification remains open after the macOS IPC fixture race
in [CI 37845418066](https://github.com/ephoton0210/blueice/actions/runs/37845418066);
the isolated fixture correction passes all 136 IPC tests on Linux.

The K.6.1 frozen-source Linux gate passes all 1,169 tests in 83 groups,
including every one of the 150 ignored oracles in 44 suite files. All 74
module-export programs, 41 Node and 47 exact declaration witnesses pass, as
do the five direct tests and incremental/private boundary controls. The shared
diagnostic corpus contains 2,378 programs and 198 templates. The existing
direct arrow and measured ES2020 anonymous-private default-class refusals
remain; broader default/re-export compositions stay in G-T10.

The K.6.2 frozen-source Linux gate passes all 1,192 tests in 92 groups,
including every one of the 156 ignored oracles in 49 suite files. Its 86 pinned
module-type programs, 54 Node and 60 exact declaration witnesses pass, together
with eight supplemental pinned programs, direct controls and owner-bounded
JSON asset/resource-limit/cache controls. The shared diagnostics record has
2,464 programs and 211 templates and matches the actual recorder byte for byte.
Final-source hosted workspace/platform/coverage CI remains open.

The final K.3/K.4 source CI passes all 29 jobs on `40de14abd`; workspace line
coverage is 90.65%, independent BlueJS 93.00%, with no added exclusions.
Evidence: [CI 37702047861](https://github.com/ephoton0210/blueice/actions/runs/37702047861). The remaining M8 leaves
and recorded unmeasured combinations stay open.

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
| G-T2 | K.4.3 adds 74 pinned argument/contextual candidate, widening, constraint/default, explicit argument and generic callable/class/interface witnesses, including imported signatures and dependent defaults. Concrete record `keyof`/indexed projections supply the measured prerequisites, and contextual budget exhaustion remains a precise refusal. Remaining: inference across arbitrary recursive/operator forms, higher-kinded patterns and broader variance; K.4.5/K.4.7 provide measured operator/compatibility relations, and K.5.2 measures generic heritage/methods; unmeasured compositions remain. |
| G-T3 | K.4.5 supplies 82 pinned record/array key-set, value-query, indexed, distributive/non-distributive conditional, infer, mapped modifier/remap/filter, template product/pattern and recursive-alias witnesses, with three exact runtime/declaration comparisons and six budget controls. Default ES2022 numeric products retain the measured 652-entry native cache order. Remaining: unmeasured nested patterns, overloaded queries, escape normalization and broader primitive/library keys; K.4.6 supplies measured index/readonly/symbol and homomorphic array/tuple composition; K.4.7 supplies measured structural relations; broader combinations remain open. Expansion beyond the configured budget remains a precise refusal. |
| G-T4 | K.4.4 supplies 74 pinned ordered call/construct/method overload, callback-context, direct literal-priority, merged-interface-group and implementation-compatibility witnesses, with six exact runtime/declaration comparisons. Anonymous signatures retain data members and generic binders, and obsolete two-tag/named-callback restrictions are retired. Remaining: unmeasured candidate/operator/variance interactions and optional chaining/non-null assertion typing in every position. |
| G-T5 | K.4.6 supplies 100 pinned index-signature, readonly array/tuple, bigint, symbol/unique-symbol, satisfies/const-assertion, tuple metadata, unknown/never, enum and void/undefined witnesses, with six exact runtime/declaration comparisons. Explicit strict unknown preserves legacy opaque compatibility; original Array/ReadonlyArray and homomorphic mapped container forms compose. Remaining: unmeasured computed symbol/index combinations, broader tuple and const/satisfies contexts, unknown/never flow combinations and unmeasured structural combinations beyond K.4.7. |
| G-T6 | K.4.7 supplies 88 pinned freshness, weak-record, strict function/method variance, optional/undefined, declared variance, rigid generic parameter, predicate/assertion and callable/indexed alias witnesses, with four exact runtime/declaration comparisons. Freshness covers initializer/variable/member/element assignment, direct/member argument and explicit return boundaries, with nested/array/spread/union/generic controls. Existing exactOptionalPropertyTypes/noUncheckedIndexedAccess and readonly write controls remain green. Remaining: unmeasured computed/nested expression contexts, composed variance and broader library structural relations. |
| G-T7 | K.1.4 adds an original, versioned minimum ECMAScript library selected by target, with owner replacement and no runtime grant. The tested Array/ReadonlyArray, boxed/primitive methods, Object/Function, Promise, collections, Math/JSON/Symbol, errors, iteration and Date/RegExp forms match the same pinned target/lib. Remaining catalogs, constructors, computed iteration, callback result precision and opaque compatibility names are enumerated in `STANDARD_LIBRARY.md`; DOM remains owner supplied. |
| G-T8 | K.1.5 infers the tested unannotated function/getter/method signatures, unions, void/never completion, async Promise and generator types, with literal widening and a bounded recursion guard. Imports and declaration emit retain those signatures. Remaining: literal freshness and widening in all positions, broader contextual and flow-sensitive inference, unproven unknown getter bodies and declaration precision; K.4/K.8 retain these gaps. |
| G-T9 | K.5.1 measures abstract classes/members, structural `implements`, `override`, constructor capabilities and imported aliases. K.5.2 measures generic heritage/methods/defaults, static lexical restrictions, named/anonymous class expressions with captured self types and constructor aliases, computed/string/numeric keys, index signatures and `declare` fields in 160 pinned programs, 30 runtime and 36 exact declaration witnesses. Same-named binders preserve declaration identities and TS2208 origins. ES2020 expression wrappers retain per-evaluation private/static stores, computed keys and contextual names; both field modes reach the direct VM. K.5.3 adds 35 pinned imported-base member-kind, accessor read/write, inferred getter, bounded merged-interface heritage and constructor branch/early-return witnesses, with 16 runtime, 17 exact declarations and three direct regressions. Remaining: unresolved getter bodies, generic merged interfaces or heritage outside bounded records; the precise branch-super parameter-property refusal (pinned assignment mode rejects TS2401; ES2022 define output throws before super); generators/auto-accessors, computed heritage expressions, stateful ES2020 expression keys containing await/yield, decorated class expressions and unmeasured combinations (section 4). |
| G-T10 | K.1.3 supplies checked value types for the tested named/default/namespace and CommonJS import forms, with inferred variable declarations and module-owned type identities. K.6.1 measures 74 pinned default-expression, anonymous/named default declaration and named/star/namespace re-export programs under ESM/CommonJS, including live aliases, snapshot defaults, cycles, conflict/diamond origins and exact declaration output. Direct execution retains anonymous default names and snapshot versus alias semantics; incremental re-exports refresh dependency types. K.6.2 measures whole and inline type-only imports/exports, qualified static namespaces, imported value type queries, runtime/type-only attributes, UMD namespace declarations and contextual import names in its pinned witnesses. K.6.3 measures 80 pinned named/merged/shorthand ambient, module/global augmentation and path/type/lib directive programs, with declaration-local imports, static re-export aliases/stars/qualified/transitive surfaces and constant/function type queries. Referenced roots retain canonical owner bounds, source/edge/depth budgets and cache invalidation; ordinary imports used solely by the measured value queries are elided. Library directives select the owned ES2020/ES2022 subset; the full library set remains in G-L1/K.10. Remaining: `import("specifier")` type expressions, broader declaration merging, implicit import elision beyond the measured query forms and unmeasured type/module compositions. Anonymous ES2020 default classes with private names remain precisely refused: pinned TypeScript accepts them but its output fails under Node in both module/field modes. |

### 3.2 Emit

| ID | Gap |
| --- | --- |
| G-E1 | K.7.1 compares all eleven ECMAScript targets in CommonJS and ESM: 638 pinned verdicts, 600 accepted Node/declaration/Acorn observations and 600 actual outputs parsed through BlueJS at the selected edition. Explicit `lib` and `downlevelIteration` are recorded in artifact/cache identity; original versioned helpers implement the measured lexical/default/rest/template/pattern/iteration/spread/class/generator/async forms. Broader computed heritage and `super` use, nested binding patterns, constructor spreads, async arrows/methods, template-contained suspension, complex suspension graphs and other combinations remain unmeasured. Unsupported named ES5 suspension graphs are explicit refusals. K.7.1 complete hosted CI passes all 29 jobs. K.7.2 adds measured AMD/UMD/System bindings, 24 Node16/NodeNext per-file/verbatim configurations and helper/script policies. All 106 native configurations (82 accept, 24 reject), including early System cyclic imported-function calls and default Node module detection without imports/exports, pass execution, declarations, suffixes and primary diagnostics. The final complete leaf gate passes all 1,297 tests, including all 190 ignored oracles; additional module/helper/target combinations remain unmeasured, including ESM Node import-equals lowering and broader helper ABI selection. |
| G-E2 | K.7.2 measures AMD/UMD/System and owner-selected Node16/NodeNext formats, verbatim policies and helper/script controls in 106 pinned configurations. Broader module/target/helper compositions remain unmeasured, including ESM Node import-equals lowering, helper ABI selection beyond the recorded forms and Node declaration-input contexts. |
| G-E3 | K.7.4 measures 1,144 standard/legacy decorator configurations across every K.7.1 target, both module modes and both class-field modes. Complete K.0 replay passes all 1,056 native accepted emitted outputs (execution, target/proposal syntax and exact declarations), plus 88 rejected primary diagnostics. Private auto-accessors/setters, replacement-class static field/block/getter/setter super receivers supplement the original computed names, namespaces, default exports and class expressions. All 1,316 K.0 tests pass; final-source hosted CI remains pending. Literal decorated names, super in decorated private callables, static-initializer super writes/computed super keys, imported/unresolved metadata types and unmeasured compositions retain precise refusals. |
| G-E4 | K.7.3 records comment removal, LF/CRLF, BOM, inline source content, source/map roots, declaration documentation/internal stripping and removed-option TS5102 diagnostics in 176 native configurations, plus 18 command/config validation controls. Class, namespace, interface, enum and exported record-alias member witnesses retain exact native declarations. Callable/indexed and empty alias controls reproduce 40 declaration differences before correction; all 176 configurations, 170 actual outputs and exact declarations pass focused verification. Strict helper sites preserve all eight target/newline/BOM controls. The final complete K.0 gate passes format, Clippy, all 1,111 ordinary tests and all 194 ignored oracles; final-source hosted CI is pending. Remaining: nested or inferred declaration-member compositions, `isolatedDeclarations`, field-mode interactions across the full target range (K.7.4), and other unmeasured option compositions. |
| G-E5 | Source maps retain line-level provenance and measured `sourceRoot`, `mapRoot`, relative sources and optional `sourcesContent` metadata. K.8.2 measures exact declaration maps for annotated top-level scalar variables/functions. Token-level names, complete JavaScript mapping byte parity and broader declaration-map forms remain unmeasured. |

### 3.3 Declarations

| ID | Gap |
| --- | --- |
| G-D1 | K.1.3 compares inferred imported variable declarations, including nested callable/tuple types and unnameable external types. K.1.5 adds 23 exact inferred-return declaration comparisons and two pinned type-equality printing comparisons. K.8.1 records 84 native configurations (77 accepts, seven rejects) covering inferred objects/readonly tuples/arrays, generic functions/classes, overloads, accessor pairs, merged/nested namespaces, export assignments and ambient type-only dependencies. Explicit/const generic and literal annotations retain measured freshness; mutable enum arrays widen to their owning enum. Const parameters on functions and constructor types are accepted; interface/generic alias parameters reject with exact TS1277. The corrected 84-configuration focused replay, live recorder, format and three-crate Clippy pass, including lexical callable and ordered-overload controls; the complete 1,321-test K.0 gate passes, including all 200 ignored oracles. Remaining: broader contextual/const inference, imported/local freshness and arbitrary anonymous object unions, overload compositions and declaration merging beyond these controls. |
| G-D2 | K.8.2 records 48 configurations (28 accepts, 20 rejects) for declarationDir, declaration-only publication, measured scalar declaration maps and isolated-declaration diagnostics. Exact maps/declarations/artifact inventory and all actual output executions agree; three CLI override/escape/symlink guards pass. Format/Clippy and the complete 1,328-test K.0 gate pass, including all 202 ignored oracles. Broader class/generic/namespace/inferred declaration maps retain a precise refusal, and arbitrary isolated-declaration compositions remain unmeasured. K.7.3 measures `stripInternal` for top-level and named class/namespace/interface/enum/exported record-alias members; broader nested and inferred declaration forms remain unmeasured. The 48 callable/indexed/empty alias controls pass focused correction verification; the complete corrected K.0 gate passes all 1,305 tests. |

### 3.4 Modules, packages, projects

| ID | Gap |
| --- | --- |
| G-M1 | K.6.2 supports owner-enabled explicit relative JSON data with canonical-root bounds, static types, content/observation identity and raw asset publication; strict-runtime/direct execution still requires a supported data runtime profile. Its per-edge type-only import/require conditions select independently. K.7.2 adds owner-selected Node16/NodeNext per-file formats, `.mts`/`.cts` sources, `.d.mts`/`.d.cts` output suffixes and canonical nearest-package observations. K.9.1's 54 Native controls cover ordered `typesVersions` ranges, exact/wildcard/array-fallback targets, exports precedence, `.d.mts`/`.d.cts`/`.mts`/`.cts` package entries, modern package self-name imports and `moduleResolution: classic` file lookup. Package manifests are confined before body reads and retain canonical/hash cache observations. Unmeasured range/composition cases and broader declaration-input contexts remain open. K.9.2's 46 Native controls cover exact/wildcard/longest-prefix paths, ordered target fallback, baseUrl precedence, rootDirs relative lookup, automatic/scoped @types, explicit/empty types and custom/empty typeRoots. Mapped paths and type libraries remain confined to canonical owner roots; inherited path origins and explicitly authorized external declaration identities are tested. Broader mapping/library compositions, missing-type-library diagnostics and JavaScript source checking remain unmeasured. `allowJs`/`checkJs` and `.js` sources remain open. The complete 1,348-test K.0 gate passes; canonical temporary-root replay preserves the Native graphs across path aliases. |
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

Generated by `tools/inventory_refusals.py` from current production source.

213 refusal sites in 11 areas

### bin (1)

- `backend/bluets/src/bin/bluetsc/native_cli/mod.rs:200` — BlueTSC configuration or an explicit owner policy refused this input.

### checker (24)

- `backend/bluets/src/checker/module/binding/classes/super_calls.rs:96` — Only public and protected methods of the base class are accessible via the 'super' keyword.
- `backend/bluets/src/checker/module/binding/classes.rs:231` — Private identifiers are only available when targeting ECMAScript 2015 and higher.
- `backend/bluets/src/checker/module/binding/classes.rs:246` — Properties with the 'accessor' modifier are only available when targeting ECMAScript 2015 and higher.
- `backend/bluets/src/checker/module/binding/modules.rs:248` — `export =` cannot be used when the module system is ECMAScript; use `--module commonjs`
- `backend/bluets/src/checker/module/binding/modules.rs:238` — `import x = require()` cannot be used when the module system is ECMAScript; use `--module commonjs`
- `backend/bluets/src/checker/module/binding/classes/expressions.rs:198` — a class expression member has no structured runtime representation
- `backend/bluets/src/checker/module/binding/enums.rs:216` — a computed initializer that refers to the member `{}` must write it \ as `{}.{}`
- `backend/bluets/src/checker/module/decorators.rs:145` — a decorator can only decorate a method implementation, not an overload
- `backend/bluets/src/checker/module/binding/classes/fields.rs:332` — a field initializer that refers to a later field inside a nested \ function is not supported yet
- `backend/bluets/src/checker/module/binding/classes/fields.rs:263` — a static block that refers to a later static field inside a nested \ function is not supported yet
- `backend/bluets/src/checker/module/binding/classes.rs:266` — an anonymous default class with private names on ES2020 is not supported: the pinned TypeScript 5.9.3 output fails at runtime; use a named default class or ES2022
- `backend/bluets/src/checker/module/binding.rs:271` — an unstructured class member (a generator or auto-accessor) is not supported yet
- `backend/bluets/src/checker/module/binding/classes/accessors.rs:144` — cannot infer getter `{}` within the supported body boundary
- `backend/bluets/src/checker/module/binding/classes/visibility.rs:478` — cannot prove that access to `{}` is permitted for this receiver
- `backend/bluets/src/checker/module/binding/classes/visibility.rs:559` — cannot prove that access to `{}` is permitted for this receiver
- `backend/bluets/src/checker/module/binding/classes/fields.rs:119` — class field `{}` needs a type annotation unless its initializer \ or default is a number, string or boolean literal
- `backend/bluets/src/checker/module/binding/classes.rs:559` — class tuple rest annotation cannot be specialized within the type budget
- `backend/bluets/src/checker/module/binding/names.rs:212` — cyclic tuple spread cannot be resolved
- `backend/bluets/src/checker/module/decorators.rs:124` — decorators are not valid here: they decorate a class or a class member
- `backend/bluets/src/checker/module/binding.rs:640` — interface heritage {name} must name an interface declaration
- `backend/bluets/src/checker/module/binding/classes/expressions.rs:54` — merged interface heritage must resolve to a bounded record of instance members
- `backend/bluets/src/checker/module/binding/namespaces.rs:1045` — namespace `{source}` has no run-time members; import it with `import type`
- `backend/bluets/src/checker/module/binding/names.rs:216` — tuple spread names an unresolved type
- `backend/bluets/src/checker/module/binding/names.rs:220` — tuple spread requires one concrete tuple or array type

### compiler (5)

- `backend/bluets/src/compiler/project_builder/references.rs:71` — library reference `{name}` is not supported by the owned declaration profiles
- `backend/bluets/src/compiler/node_modules.rs:32` — module declaration is incompatible with the selected Node file format
- `backend/bluets/src/compiler/project_builder.rs:213` — owner must select ESM or CommonJS for a Node file
- `backend/bluets/src/compiler/project_builder/helper_providers.rs:97` — owner-resolved helper provider does not declare the supported __extends ABI
- `backend/bluets/src/compiler.rs:701` — strict-runtime JSON assets require a supported runtime profile

### diagnostic (1)

- `backend/bluets/src/diagnostic/mapping.rs:123` — BlueTSC deliberately refuses a documented subset boundary.

### emitter (80)

- `backend/bluets/src/emitter/targets/es5.rs:210` — ES5 lowering: {message}
- `backend/bluets/src/emitter/targets/es5.rs:212` — ES5 lowering: {message}
- `backend/bluets/src/emitter/targets/es5/templates.rs:57` — ES5 template expressions exceed the token budget
- `backend/bluets/src/emitter/system.rs:21` — System does not support export assignment
- `backend/bluets/src/emitter/system.rs:144` — System function body is missing
- `backend/bluets/src/emitter/system.rs:146` — System function body is unbalanced
- `backend/bluets/src/emitter/system.rs:159` — System variable binding requires retained names
- `backend/bluets/src/emitter/decorators.rs:751` — a constructor of a decorated class needs its `super(...)` call as a top-level statement
- `backend/bluets/src/emitter/decorators.rs:346` — a decorator can only decorate a method implementation, not an overload
- `backend/bluets/src/emitter/decorators.rs:312` — a decorator expression with a line comment is not lowered
- `backend/bluets/src/emitter/legacy_decorators.rs:152` — a decorator expression with a line comment is not lowered
- `backend/bluets/src/emitter/class_lowering.rs:478` — a derived constructor whose class needs statements at its start must have a top-level \ `super(...)` statement to place them after
- `backend/bluets/src/emitter/class_lowering.rs:134` — a private name in a class inside a namespace is not lowered for this target yet
- `backend/bluets/src/emitter/class_lowering.rs:487` — a static initializer or block that uses `super` cannot be lowered for this target
- `backend/bluets/src/emitter/decorators/super_members.rs:51` — a static super property has no key
- `backend/bluets/src/emitter/class_lowering/expressions.rs:46` — an ES2020 class expression with state and an await/yield key needs lexical suspension lowering
- `backend/bluets/src/emitter/namespaces.rs:352` — an exported namespace variable with several declarators is not supported yet
- `backend/bluets/src/emitter/namespaces.rs:367` — an exported namespace variable's name could not be located
- `backend/bluets/src/emitter/commonjs.rs:313` — an exported variable with several declarators is not supported in CommonJS output yet
- `backend/bluets/src/emitter/commonjs.rs:332` — an exported variable's name could not be located
- `backend/bluets/src/emitter/targets/es5.rs:67` — arrow body was not retained
- `backend/bluets/src/emitter/targets/es5.rs:64` — arrow head was not retained
- `backend/bluets/src/emitter/legacy_decorators.rs:211` — auto-accessors are not combined with experimentalDecorators
- `backend/bluets/src/emitter/targets/es5/patterns.rs:49` — binding pattern initializer was not retained
- `backend/bluets/src/emitter/targets/es5/loops.rs:64` — captured loop completion requires a retained completion record
- `backend/bluets/src/emitter/targets/es5/classes.rs:64` — class body was not retained
- `backend/bluets/src/emitter/targets/es5/classes.rs:57` — class heritage requires one retained base binding
- `backend/bluets/src/emitter/targets/es5/classes.rs:109` — class member requires a retained named method
- `backend/bluets/src/emitter/targets/es5/classes.rs:118` — class method body was not retained
- `backend/bluets/src/emitter/targets/es5/objects.rs:52` — computed literal prototype/super semantics require retained home objects
- `backend/bluets/src/emitter/targets/es5/classes.rs:98` — computed member key was not retained
- `backend/bluets/src/emitter/decorators.rs:328` — computed or literal member names in decorator lowering are not supported yet
- `backend/bluets/src/emitter/targets/es5/objects.rs:37` — computed property value was not retained
- `backend/bluets/src/emitter/targets/es5/spreads.rs:61` — constructor spread requires retained construction semantics
- `backend/bluets/src/emitter/declaration_maps.rs:259` — declaration maps currently require annotated top-level scalar variables and functions
- `backend/bluets/src/emitter/declaration_maps.rs:261` — declaration maps currently require annotated top-level scalar variables and functions
- `backend/bluets/src/emitter/classes.rs:111` — declaration output for this string literal field needs an annotation
- `backend/bluets/src/emitter/classes.rs:403` — declaration output requires a class field type
- `backend/bluets/src/emitter/classes.rs:286` — declaration output requires a parameter property type
- `backend/bluets/src/emitter/classes.rs:546` — declaration output requires an explicit class method return type
- `backend/bluets/src/emitter/classes.rs:487` — declaration output requires an explicit getter return type
- `backend/bluets/src/emitter/legacy_decorators.rs:582` — decorator metadata cannot serialize `{name}`: only types declared in this module are supported
- `backend/bluets/src/emitter/legacy_decorators.rs:431` — decorator metadata needs this accessor's type annotation
- `backend/bluets/src/emitter/legacy_decorators.rs:379` — decorator metadata needs this field's type
- `backend/bluets/src/emitter/legacy_decorators.rs:392` — decorator metadata needs this method's return type annotation
- `backend/bluets/src/emitter/decorators.rs:387` — decorators are not valid here
- `backend/bluets/src/emitter/legacy_decorators.rs:259` — decorators cannot be applied to both the getter and the setter of the same name
- `backend/bluets/src/emitter/legacy_decorators.rs:242` — decorators on private names are not valid with experimentalDecorators
- `backend/bluets/src/emitter/commonjs.rs:263` — default export expression has no initializer
- `backend/bluets/src/emitter/targets/es5/parameters.rs:33` — default parameter head was not retained
- `backend/bluets/src/emitter/targets/es5/classes.rs:329` — derived constructor returns require a retained constructor completion
- `backend/bluets/src/emitter/targets/es5/classes.rs:296` — derived constructor super properties require a retained receiver
- `backend/bluets/src/emitter/targets/es5/suspension.rs:68` — generator declaration was not retained
- `backend/bluets/src/emitter/helper_selection.rs:57` — helper selection does not support generated ABI `{name}`
- `backend/bluets/src/emitter/targets.rs:91` — logical assignment has no right operand
- `backend/bluets/src/emitter/targets/es5/patterns.rs:78` — object rest was not lowered before ES5 binding projection
- `backend/bluets/src/emitter/targets/optional.rs:44` — optional receiver is outside the target lowering subset
- `backend/bluets/src/emitter/targets/es5/spreads.rs:54` — spread callee was not retained
- `backend/bluets/src/emitter/decorators/super_members.rs:79` — static super property writes need receiver-aware assignment lowering
- `backend/bluets/src/emitter/decorators/private_members.rs:39` — super in a decorated private callable needs lexical heritage lowering
- `backend/bluets/src/emitter/targets/es5/classes/super_members.rs:40` — super key was not retained
- `backend/bluets/src/emitter/targets/es5/classes/super_members.rs:60` — super property writes need receiver-aware assignment lowering
- `backend/bluets/src/emitter/targets/exponentiation.rs:36` — target exponentiation lowering requires an update or unary operand
- `backend/bluets/src/emitter/targets/optional.rs:90` — target nullish lowering requires both operands
- `backend/bluets/src/emitter/targets/object_spread.rs:46` — target object spread does not yet lower prototype setters or super properties
- `backend/bluets/src/emitter/targets/object_spread.rs:40` — target object spread requires an operand
- `backend/bluets/src/emitter/targets/object_rest.rs:36` — target object-rest lowering requires an initializer
- `backend/bluets/src/emitter/targets/optional.rs:53` — target optional lowering requires a named property
- `backend/bluets/src/emitter/targets/optional.rs:65` — target optional property lowering requires a terminal value read
- `backend/bluets/src/emitter/private_lowering.rs:657` — the identifier `{helper}` is reserved for the private-name helpers
- `backend/bluets/src/emitter/private_lowering.rs:167` — the identifier `{variable}` is needed to lower this class's private names but is already used
- `backend/bluets/src/emitter/decorators.rs:396` — this decorated member shape (computed or literal name, or a form the class parser does not structure) is not lowered
- `backend/bluets/src/emitter/legacy_decorators.rs:222` — this decorated member shape is not lowered
- `backend/bluets/src/emitter/decorators.rs:339` — this decorated method shape is not lowered
- `backend/bluets/src/emitter/private_lowering.rs:326` — this private member could not be located in its own text
- `backend/bluets/src/emitter/private_lowering.rs:318` — this private member has no lowering
- `backend/bluets/src/emitter/decorators/super_members.rs:45` — this static super reference needs a retained named property
- `backend/bluets/src/emitter/targets/es5/classes/super_members.rs:50` — this super reference needs a retained property
- `backend/bluets/src/emitter/targets/es5/suspension.rs:87` — this suspension control-flow shape cannot be lowered to ES5
- `backend/bluets/src/emitter/private_lowering.rs:564` — {what} cannot be lowered for a target below ES2022

### expression (23)

- `backend/bluets-bluejs/src/expression.rs:377` — a unary expression cannot be the unparenthesized base of exponentiation
- `backend/bluets-bluejs/src/expression.rs:551` — array literals cannot combine holes and spread elements in the v1 direct bridge subset
- `backend/bluets-bluejs/src/expression.rs:562` — array literals cannot combine holes and spread elements in the v1 direct bridge subset
- `backend/bluets-bluejs/src/expression/calls.rs:127` — only a direct identifier receiver may use optional dot access
- `backend/bluets-bluejs/src/expression/calls.rs:140` — only an identifier property is in the first optional dot subset
- `backend/bluets-bluejs/src/expression/calls.rs:53` — only constructor calls with parentheses are in the v1 direct bridge subset
- `backend/bluets-bluejs/src/expression/calls.rs:206` — only direct identifier and property calls are in the v1 direct bridge subset
- `backend/bluets-bluejs/src/expression/writes.rs:71` — only identifier and property assignment targets are in the v1 direct bridge subset
- `backend/bluets-bluejs/src/expression/writes.rs:128` — only identifier and property update targets are in the v1 direct bridge subset
- `backend/bluets-bluejs/src/expression/calls.rs:21` — only identifier constructors are in the v1 direct bridge subset
- `backend/bluets-bluejs/src/expression/calls.rs:161` — only identifier dot property names are in the v1 direct bridge subset
- `backend/bluets-bluejs/src/expression.rs:674` — only identifier object keys may use shorthand; methods are not in the v1 direct bridge subset
- `backend/bluets-bluejs/src/expression.rs:656` — only identifier, string, numeric, and computed object property keys are in the v1 direct bridge subset
- `backend/bluets-bluejs/src/expression.rs:432` — only property delete targets are in the v1 direct bridge subset
- `backend/bluets-bluejs/src/expression.rs:131` — parentheses are required when mixing `??` with `&&` or `||`
- `backend/bluets-bluejs/src/expression.rs:139` — parentheses are required when mixing `??` with `&&` or `||`
- `backend/bluets-bluejs/src/expression.rs:904` — unsupported expression token `?.` in a template substitution
- `backend/bluets-bluejs/src/expression.rs:58` — unsupported expression token `{}`
- `backend/bluets-bluejs/src/expression.rs:498` — unsupported keyword `{}` in a runtime expression
- `backend/bluets-bluejs/src/expression.rs:483` — unsupported numeric literal
- `backend/bluets-bluejs/src/expression.rs:647` — unsupported numeric object key
- `backend/bluets-bluejs/src/expression.rs:525` — unsupported runtime expression token `{}`
- `backend/bluets-bluejs/src/expression.rs:777` — unsupported string escape in the v1 direct bridge subset

### jsx_direct (8)

- `backend/bluets-bluejs/src/jsx_direct.rs:116` — JSX needs the `jsx` option
- `backend/bluets-bluejs/src/jsx_direct.rs:281` — a JSX element has fewer embedded expressions than its syntax
- `backend/bluets-bluejs/src/jsx_direct.rs:138` — a JSX element is missing its expression group
- `backend/bluets-bluejs/src/jsx_direct.rs:146` — a JSX element is missing its expression group
- `backend/bluets-bluejs/src/jsx_direct.rs:156` — a JSX element's expression group is unterminated
- `backend/bluets-bluejs/src/jsx_direct.rs:119` — cannot use JSX unless the `jsx` option is provided
- `backend/bluets-bluejs/src/jsx_direct.rs:121` — preserved JSX is not executable; choose `jsx: react` for direct execution
- `backend/bluets-bluejs/src/jsx_direct.rs:127` — the automatic JSX runtime imports a runtime module, which the direct bridge does not link; use the classic mode with an in-program factory

### lib (14)

- `backend/bluets-bluejs/src/lib.rs:756` — BlueTS did not retain a canonical target for this runtime import
- `backend/bluets-bluejs/src/lib.rs:346` — BlueTS rejected the direct script with {} diagnostic(s)
- `backend/bluets-bluejs/src/lib.rs:727` — JSON data cannot be a direct executable module-graph entry
- `backend/bluets-bluejs/src/lib.rs:849` — a declaration module cannot be executed directly
- `backend/bluets-bluejs/src/lib.rs:696` — a declaration module cannot be the direct module-graph entry
- `backend/bluets-bluejs/src/lib.rs:733` — a declaration module cannot be the direct module-graph entry
- `backend/bluets-bluejs/src/lib.rs:806` — the direct bridge executes ECMAScript modules only; a CommonJS project is run as \ `--module commonjs` emitted output in a realm with a host-provided CommonJS loader
- `backend/bluets-bluejs/src/lib.rs:797` — the direct bridge executes ECMAScript modules only; a {} project requires emitted output and a host-provided {} loader
- `backend/bluets-bluejs/src/lib.rs:769` — the direct bridge has no JSON data module runtime profile
- `backend/bluets-bluejs/src/lib.rs:762` — the direct bridge links no installed packages: a runtime import of a package \ needs the host-provided module loader of the `bluetsc build` route
- `backend/bluets-bluejs/src/lib.rs:843` — the requested entry was not retained in the checked source graph
- `backend/bluets-bluejs/src/lib.rs:721` — the requested module-graph entry was not retained in the checked source graph
- `backend/bluets-bluejs/src/lib.rs:832` — the v1 direct bridge supports exactly one executable source module
- `backend/bluets-bluejs/src/lib.rs:363` — {}:{}:{} cannot lower to the current BlueJS bridge: {message}

### lowering (33)

- `backend/bluets-bluejs/src/lowering/classes.rs:50` — BlueJS implements the standard decorators; `experimentalDecorators` programs run as `bluetsc build --experimental-decorators` output
- `backend/bluets-bluejs/src/lowering.rs:302` — BlueTS did not retain a canonical target for this runtime import
- `backend/bluets-bluejs/src/lowering.rs:59` — ESM default exports require the module bridge
- `backend/bluets-bluejs/src/lowering.rs:66` — ESM named exports require the module bridge
- `backend/bluets-bluejs/src/lowering/classes.rs:334` — a derived constructor needs a top-level super call for the statements that must follow it
- `backend/bluets-bluejs/src/lowering.rs:674` — a nested function declaration is outside the direct try subset
- `backend/bluets-bluejs/src/lowering.rs:727` — a nested function declaration is outside the direct while subset
- `backend/bluets-bluejs/src/lowering.rs:501` — a rest parameter must be the final direct function parameter
- `backend/bluets-bluejs/src/lowering/namespaces.rs:46` — an ambient const enum needs its uses inlined, which the direct bridge does not do
- `backend/bluets-bluejs/src/lowering.rs:105` — an exported class requires the module bridge
- `backend/bluets-bluejs/src/lowering.rs:133` — an exported enum requires the module bridge
- `backend/bluets-bluejs/src/lowering.rs:111` — an exported namespace requires the module bridge
- `backend/bluets-bluejs/src/lowering.rs:655` — block-local declarations are outside the direct try subset
- `backend/bluets-bluejs/src/lowering.rs:680` — body syntax is outside the direct try subset
- `backend/bluets-bluejs/src/lowering.rs:78` — declared or exported variables require a non-script bridge mode
- `backend/bluets-bluejs/src/lowering.rs:343` — declared or overloaded functions cannot be lowered to a direct module
- `backend/bluets-bluejs/src/lowering.rs:337` — declared variables cannot be lowered to a direct module
- `backend/bluets-bluejs/src/lowering.rs:94` — declared, overloaded, or exported functions require a non-script bridge mode
- `backend/bluets-bluejs/src/lowering/classes.rs:167` — decorators and auto-accessors need class fields to be defined (the default for ES2022)
- `backend/bluets-bluejs/src/lowering.rs:495` — destructured parameters are not yet in the v1 direct bridge subset
- `backend/bluets-bluejs/src/lowering.rs:568` — function body syntax is not yet in the v1 direct bridge subset
- `backend/bluets-bluejs/src/lowering.rs:733` — loop body syntax is outside the direct while subset
- `backend/bluets-bluejs/src/lowering.rs:706` — loop-local declarations are outside the direct while subset
- `backend/bluets-bluejs/src/lowering.rs:661` — loops are outside the direct try subset
- `backend/bluets-bluejs/src/lowering.rs:562` — nested function declarations are not yet in the v1 direct bridge subset
- `backend/bluets-bluejs/src/lowering.rs:712` — nested loops are outside the direct while subset
- `backend/bluets-bluejs/src/lowering.rs:667` — nested try statements are outside the direct try subset
- `backend/bluets-bluejs/src/lowering.rs:243` — re-exports require a retained module-graph edge
- `backend/bluets-bluejs/src/lowering.rs:293` — runtime imports require the direct module-graph bridge
- `backend/bluets-bluejs/src/lowering.rs:72` — runtime imports require the module bridge
- `backend/bluets-bluejs/src/lowering/classes.rs:199` — this class member has no direct lowering
- `backend/bluets-bluejs/src/lowering/namespaces.rs:377` — this declaration cannot appear in a namespace body lowered directly
- `backend/bluets-bluejs/src/lowering.rs:718` — try statements are outside the direct while body subset

### namespace_analysis (1)

- `backend/bluets/src/namespace_analysis.rs:542` — a template literal that reads an exported namespace variable is not lowered directly yet

### parser (23)

- `backend/bluets/src/parser/declarations/erasure_audit.rs:83` — TypeScript annotations on functions nested in expressions are not supported yet
- `backend/bluets/src/parser/declarations.rs:419` — TypeScript assertions outside a supported declaration are not in the initial BlueTS matrix
- `backend/bluets/src/parser/declarations.rs:313` — `{}` is not in the initial BlueTS matrix
- `backend/bluets/src/parser/declarations/class.rs:111` — a computed class heritage expression is not supported yet
- `backend/bluets/src/parser/declarations.rs:260` — an async enum is not valid
- `backend/bluets/src/parser/declarations.rs:272` — an async enum is not valid
- `backend/bluets/src/parser/declarations/typed_declarations.rs:258` — an async generator with a synchronous iterator annotation is not supported yet
- `backend/bluets/src/parser/declarations/typed_declarations.rs:249` — an async generator without an explicit return annotation is not supported yet
- `backend/bluets/src/parser/declarations.rs:297` — an async namespace is not valid
- `backend/bluets/src/parser/declarations/enums.rs:53` — an enum member name with an escape sequence is not supported yet
- `backend/bluets/src/parser/declarations.rs:167` — an import, `export default`, `export =`, `export *` or export list inside a namespace body is not supported
- `backend/bluets/src/parser/declarations.rs:245` — declared and async classes are not in the first class form
- `backend/bluets/src/parser/declarations.rs:328` — decorators and TSX/JSX are not in the initial BlueTS matrix
- `backend/bluets/src/parser/declarations/typed_declarations.rs:123` — exported or ambient variable binding patterns are not supported yet
- `backend/bluets/src/parser/declarations/typed_declarations.rs:55` — interface heritage supports only named interface types
- `backend/bluets/src/parser/declarations/imports_exports.rs:295` — only `export = name;` of a local declaration is supported
- `backend/bluets/src/parser/declarations/imports_exports.rs:98` — only `import name = require("module")` is supported as an `import =` form
- `backend/bluets/src/parser/declarations/typed_declarations.rs:146` — optional variables are not valid TypeScript declarations
- `backend/bluets/src/parser/declarations/typed_declarations.rs:479` — optional, defaulted and rest this parameters are not supported
- `backend/bluets/src/parser/declarations/typed_declarations.rs:414` — this destructuring pattern is not supported yet
- `backend/bluets/src/parser/declarations/typed_declarations.rs:135` — this variable binding pattern is not supported yet
- `backend/bluets/src/parser/type_syntax.rs:360` — tuple rest element must have an array or named tuple annotation
- `backend/bluets/src/parser/declarations/source_edits.rs:100` — typed arrow parameters are not in the initial BlueTS matrix
