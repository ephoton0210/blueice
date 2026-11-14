# Phase 18 — BlueTS / BlueTSC completion worklist

[← Plan](PLAN.md) · [compatibility inventory](COMPATIBILITY_INVENTORY.md) · [integration contract](INTEGRATION_CONTRACT.md) · [test interface](TEST_INTERFACE.md)

This file keeps the completed baseline concise and tracks the remaining work
against pinned TypeScript 5.9.3. Design and delivered evidence belong in
[PLAN.md](PLAN.md) and [PLAN_HISTORY.md](PLAN_HISTORY.md).

<!-- BEGIN CURRENT BLUETS STATUS -->

**Current BlueTS / BlueTSC compatibility status (2026-11-14).**
**Full TypeScript parity: OFF; do not advertise full parity.**

Pinned compiler: **TypeScript 5.9.3**. **N = 42,716 suite-qualified
case assertions**, not unique programs or a percentage of the TypeScript language.

| Current measurement | Count / scope |
| --- | --- |
| Direct agreement | 41,797/41,803 (99.9856%); 3 open, 3 refused observations |
| Separate controls | 633 native-reference + 280 BlueTS owner = 913; excluded from direct agreement |
| Case labels | 42,710 scoped closed (including controls), 3 open, 3 refused |
| Suite inventory | 190 suites / 395 ignored Rust functions; not a case denominator |
| Suite labels | 186 scoped closed, 1 open, 3 refused |
| Broader G-* feature rows | 23 total: 1 scoped closed, 22 open |
| Direct BlueJS runtime profile | Open; separate from the TypeScript comparison claim |
| Source refusal sites | 230 in 11 areas; condition-specific locations, not extra failing cases |

All 42,716 recorded case expectations pass; expected differences/refusals are not
agreements. The complete case measurement is **darwin-arm64**, not all-platform proof.
Accepted baseline runtime evidence (AC-K14.3; its recorded source): Windows
x86_64 VM **82/82** core oracles plus the complete local test gate. This is not
a new Windows run for the compiler/oracle source below. Hosted CI and coverage
are supplementary, not prerequisites. This consistency check does not rerun
those runtime gates.

Compiler/oracle source: `backend-tree:57ea386c3d0864ab228846f55cf39d3a3d3f5e66`.
Input graph SHA-256: `a7cbc686748f1c4eb50b977d4e91b621635c40ead22cadd3f811a1aa363ff8e9`.
Versioned authorities: `COMPATIBILITY_CASES.json`, `COMPATIBILITY_SUITES.json`
and `COMPATIBILITY_MEASUREMENT.json` in `development/browser_core/phase-18-bluets/`.
Older dated measurements below retain their original scope; they are not this current summary.

<!-- END CURRENT BLUETS STATUS -->

<!-- BEGIN GM3 CURRENT PLATFORM EVIDENCE -->

**Earlier modularization platform regression (2026-11-09):** source `backend-tree:ad7f2dc43c2b7f2eefcaadf97312ee7d8da68fb3` passes Windows x86_64 symlink 25/25 and core oracle 82/82 plus the complete local gates after the module split. All 14,605 files in the recorded source-archive-plus-patches manifest match their expected hashes. [Current modularization acceptance](MODULARIZATION_MEASUREMENT.json) retains fresh commands and raw evidence; the [original G-M3 acceptance](GM3_CURRENT_INTEGRATION_MEASUREMENT.json) keeps its prior recorded source. Broader platform/toolchain combinations remain open.

<!-- END GM3 CURRENT PLATFORM EVIDENCE -->

**Current work (2026-11-09):** A–J and K.1–K.14 remain the completed baseline.
All **22 existing Phase L checklist packages are complete within their frozen
scopes**, including G-T1's exact-output second stage, G-C3 and the bounded
Windows x86_64 G-M3 profile. Broader unmeasured combinations remain open and
full TypeScript parity stays OFF. The accepted Windows 82-core/local-workspace
boundary is unchanged; no new hosted-CI or arm64 gate is required.

G-C1 acceptance includes the source-bound 102-suite / 247-oracle measurement,
complete ordinary two-crate tests, rustfmt and warnings-denied Clippy. The
broader G-C1 inventory row remains open for unmeasured CLI/recovery coverage;
the finite checklist does not claim full parity. See the
[acceptance record](PLAN.md#phase-l--g-c1-implementation).

**Latest completed follow-up:** P0-F repairs conditional inference candidate
variance, completed-pattern checks, missing positions and tuple matching.
[Acceptance](CONDITIONAL_INFERENCE_ACCEPTANCE.md) binds complete
Native/local/Windows, maps, projects and tooling to the frozen source and oracle
tools. Earlier packages retain their dated evidence. All 22 broader rows remain
open; their remaining scope needs separate frozen contracts and measurements.

## Open gaps and implementation priority — 2026-11-11

Reviewed against `eafbd9680` and its accepted compiler source
`backend-tree:4687d8d818e1d6b08bb437b41b19c9d84813356f`.
The [inventory](COMPATIBILITY_INVENTORY.md) contains 23
feature rows; G-C2 is scoped closed and the following 22 remain open. Each row
already has accepted finite coverage. The remaining scope below summarizes
unmeasured combinations and explicit limitations, rather than declaring the
whole feature unimplemented.

The accounting tools currently **fix G-C2 to closed and every other G-ID to
open**: [row generation](tools/account_inventory_rows.py) and
[status validation](tools/check_compatibility_status.py). Thus 22 is a category
count, not a decreasing count of reproduced defects. Track delivery through
finite work packages and their evidence. Changing a broader row's status needs
an explicit closure contract and consistent accounting rules; accepted subitems
alone do not establish arbitrary TypeScript compatibility.

| ID | Feature and remaining scope | Primary priority |
| --- | --- | --- |
| G-T1 | Control-flow narrowing: unmeasured guard, capture/write invalidation, exceptional-flow and declaration compositions. | P1 |
| G-T2 | Generic/contextual inference: recursive conditional/reverse-mapped inference, imported/quantified contexts and variance compositions. | P0 |
| G-T3 | Type operators: recursive/composed keyof, indexed, mapped and template operators, queries and library/key augmentation. | P0 |
| G-T4 | Overload resolution: candidate isolation/order, contextual callbacks, imported/computed chains and variance/nullability combinations. | P0 |
| G-T5 | Advanced type compositions: computed keys, unions/intersections, optional/rest/variadic tuples, const/satisfies and scope/flag interactions. | P0 |
| G-T6 | Structural compatibility: computed/non-literal members, callback/generic protocols, optional/rest parameters and library/variance relations. | P0 |
| G-T7 | Standard library: wider catalogs and inference/operator compositions beyond the accepted NoInfer, NonNullable, Awaited and four string-intrinsic packages. | P0 |
| G-T8 | Return/context inference: nested/imported/recursive returns, async/generator/getter results, flow and operator/library compositions. | P0 |
| G-T9 | Classes: recursive/imported getters and merges, computed generic calls, constructor contexts and decorated class expressions. | P3 |
| G-T10 | Module types: declaration merging, import queries/attributes/resolution modes, private/generic/ambient compositions and metadata import retention. | P1 |
| G-E1 | Target lowering: unmeasured target/library/field/suspension compositions, including async super writes. | P3 |
| G-E2 | Module/helper emission: module/Node format, target, library, helper-provider ABI and option compositions. | P2 |
| G-E3 | Decorator emission: suspended logical super writes, private parameter-initializer super, legacy computed auto-accessors and metadata boundaries. | P3 |
| G-E4 | Emit options: nested/inferred declaration members, isolatedDeclarations, stripInternal and target/field/option interactions. | P1 |
| G-E5 | Source-map precision: arbitrary constructs outside the frozen source-role matrices; the accepted 572 Native corrections remain regression evidence. | P3 |
| G-D1 | Declaration inference: anonymous/contextual/const-generic types, overload/merge surfaces and external-name compositions. | P1 |
| G-D2 | Declaration maps: isolated/nested/inferred declarations, stripInternal compositions and complete map provenance. | P1 |
| G-M1 | Resolution: package/mapped JS, declaration/library/flag combinations and unsupported nested runtime-body JSDoc. | P2 |
| G-M2 | Build/state/watch: multi-project graphs, state/option combinations, unlisted/package input changes and unmeasured Native warm behavior. | P2 |
| G-M3 | Platform/toolchain combinations beyond the accepted Windows x86_64 profile; the accepted 82-core/local gate remains the required boundary. | Common gate |
| G-C1 | Diagnostics/CLI: unmeasured recovery/CLI forms and physical default-library/package tracing, with exact messages and UTF-16 locations. | P2; common diagnostic acceptance |
| G-C3 | JSX/decorator checking: further props/provider/overload/context, source/library/target/option compositions. | P3 |

Prioritize shared implementation paths, reproduced differences and prerequisites.
Do not rank a package higher merely because its tests mention more G-IDs.
Exact declarations, diagnostics and platform checks are common acceptance
obligations; they are not three additional implementation fixes per feature.

| Order | Shared work and overlap | Concrete reason for this order |
| --- | --- | --- |
| P0 | Inference/type operators/library semantics: G-T2 through G-T8. NoInfer directly connects G-T2/G-T4/G-T6/G-T7/G-T8; union, intersection, template and thenable work connects G-T3/G-T5. | The original name-probe failures led to accepted P0-A/B/C. The new mapped-inference discovery reproduces lost array/tuple candidates and contextual shape widening through shared collection/substitution paths. P0-D completes that finite contract; prioritize newly frozen cross-file and generic-flow counterexamples next. |
| P1 | Flow, imported identities and exact declarations: G-T1/G-T10/G-D1/G-D2/G-E4, with G-T8/G-T9 interactions. | Stable inferred types and retained identities feed both public declarations and declaration maps. Freeze new cross-file/flow counterexamples before extending the existing accepted forms. |
| P2 | One dependency graph across resolution, modules, build/watch and tracing: G-M1/G-M2/G-E2/G-C1, with G-T10 interactions. | Canonical resolved/missing inputs, package decisions and their fingerprints drive invalidation and explainFiles. Share those facts while preserving owner confinement. |
| P3 | Class/decorator/JSX and target lowering: G-T9/G-E1/G-E3/G-E5/G-C3, with G-E2/G-E4 interactions. | These compositions depend on earlier type/identity work and involve several separate lowering/runtime paths. Add new mapping families with the corresponding constructs; retain the already accepted 572 corrections. |

Code supporting these dependencies:

- [Generic inference](../../../backend/bluets/src/checker/inference.rs),
  [candidate collection](../../../backend/bluets/src/checker/inference/collect.rs),
  [contextual calls](../../../backend/bluets/src/checker/module/expressions/generic_inference.rs)
  and [return inference](../../../backend/bluets/src/checker/module/return_inference.rs)
  connect argument evidence, callback/overload contexts and inferred results.
- [Type operators](../../../backend/bluets/src/checker/type_operators.rs),
  [substitution](../../../backend/bluets/src/checker/type_relations/substitution.rs)
  and [declaration rendering](../../../backend/bluets/src/emitter/inferred_declarations/render.rs)
  connect normalization with named public type identities and exact output.
- [Library selection](../../../backend/bluets/src/standard_library/manifest.rs)
  admits [static thenable foundations](../../../backend/bluets/src/standard_library/ecma-thenable.v3.d.ts)
  from ES5, including PromiseLike and Awaited. The
  [runtime Promise constructor](../../../backend/bluets/src/standard_library/ecma-promise.v3.d.ts)
  remains ES2015. Frozen visibility, owner/runtime and structural controls verify
  those separate boundaries; wider thenable compositions remain open.
- [Resolution fingerprints](../../../backend/bluets/src/bin/bluetsc/build_graph/freshness/resolution_inputs.rs)
  already bind canonical, missing and changed inputs. Future resolver/watch/CLI
  work should extend the same graph facts rather than maintain unrelated copies.

### Next implementation packages — P0 first

The [current presence witnesses](STRING_INTRINSIC_MEASUREMENT.json#/presenceInventory/corrected)
pass all22 names in both selected profiles. These44 witnesses carry no semantic
denominator credit; broader catalogs and compositions remain open. P0-A/B/C/D/E/F have
complete source-bound acceptance within their frozen contracts.

- [x] **P0-A — NoInfer and shared candidate boundaries.** Delivered the frozen
  282 primary and four supplementary diagnostic comparisons across
  G-T2/G-T4/G-T6/G-T7/G-T8. All diagnostics and declaration bytes match;
  each actual compiler passes 159 declaration files, 306 isolated consumers,
  159 syntax checks and six executions. Complete same-source Native/local,
  Windows-82, maps, project and tooling gates pass. Three owner controls and
  nine budget refusals remain separate. [Acceptance](NOINFER_ACCEPTANCE.md) ·
  [measurement](NOINFER_MEASUREMENT.json). Broader rows remain open.
- [x] **P0-B — NonNullable and ES5 Awaited foundations.** Delivered 396 complete
  direct comparisons plus six independent Native demand controls. Each actual
  compiler passes 315 exact declaration inventories / 331 files, 630 consumers,
  331 parses and twelve executions. Six owner/runtime and fifteen resource
  controls remain separate. Complete same-source Native/local/Windows, maps,
  project and tooling gates pass. All previous cases remain unchanged; 423 new
  assertions bring N to 40,636. [Acceptance](NONNULLABLE_AWAITED_ACCEPTANCE.md) ·
  [measurement](NONNULLABLE_AWAITED_MEASUREMENT.json). Broader rows remain open.
- [x] **P0-C — One string-intrinsic implementation for four utilities.** Delivered
  245 forms × four profiles =980 complete comparisons, with772 accepted exact
  inventories /804 declaration files,1544 isolated consumers,804 syntax checks
  and16 executions per actual compiler. Twenty owner/runtime/resource controls
  remain separate; ten supplemental context regressions add no inventory credit.
  Complete same-source Native/local/Windows, maps, projects and tooling pass.
  All40636 earlier cases/classes remain unchanged;1000 new assertions bring N to
  41636. [Acceptance](STRING_INTRINSIC_ACCEPTANCE.md) ·
  [measurement](STRING_INTRINSIC_MEASUREMENT.json). Broader rows remain open.

- [x] **P0-D — Reverse mapped inference and contextual array shapes.** Delivered
  101 forms × four profiles =404 complete comparisons, with 358 accepts /46
  rejects, 366 exact declaration files, 716 consumers, 366 parses and eight
  executions per actual compiler. Twelve policy controls remain separate;
  33 supplemental observations include ten intentional owner refusals and add
  no inventory credit. Complete same-source Native/local/Windows, maps,
  projects and tooling pass. All 41636 earlier case classes remain unchanged;
  416 new assertions bring N to 42052. [Acceptance](MAPPED_INFERENCE_ACCEPTANCE.md) ·
  [measurement](MAPPED_INFERENCE_MEASUREMENT.json). Broader rows remain open.

- [x] **P0-E — Symbolic key relations and exact presentation.** Delivered
  64 forms ×four profiles =256 complete comparisons: 172 accepts /84 rejects,
  176 exact declarations, 344 isolated consumers, 176 parses and 24 executions
  per actual compiler. Twelve policy controls remain separate; the 16-case
  compound-key supplement and original 32-program/57-library replay add zero
  inventory credit. Complete same-source Native/local/Windows, maps, projects
  and tooling pass. All 42052 prior assertion IDs/classes remain unchanged;
  268 additions bring N to 42320. [Acceptance](SYMBOLIC_KEYS_ACCEPTANCE.md) ·
  [measurement](SYMBOLIC_KEYS_MEASUREMENT.json). Broader rows remain open.

- [x] **P0-F — Conditional infer candidates and complete matching.** Delivered
  48 forms ×eight profiles =384 complete comparisons: 360 accepts /24 rejects,
  368 exact declarations, 720 isolated consumers, 368 parses and 48 executions
  per actual compiler. Twelve policy controls remain separate; 24 alias identity
  controls and the original 24-program replay add zero inventory credit.
  Complete same-source Native/local/Windows, maps, projects and tooling pass.
  All 42320 prior assertion IDs/classes remain unchanged; 396 additions bring
  N to 42716. [Acceptance](CONDITIONAL_INFERENCE_ACCEPTANCE.md) ·
  [measurement](CONDITIONAL_INFERENCE_MEASUREMENT.json). Broader rows remain open.

Each package needs frozen authored inputs and Native expectations before
production changes, complete primary/related diagnostics and UTF-16 positions,
byte-exact accepted declaration inventories, isolated positive/negative
consumers and applicable executable checks. Retain owner/default-deny/budget
controls separately. Record exact case counts from the frozen matrix; name
presence alone is insufficient. Run focused checks during implementation, then
the complete same-source Native/local/Windows-82, map and tooling acceptance
for the finished package. Commit its implementation and matching measurements
together, then push. Broader row closure remains a separate scoped decision.

## Maintenance and real-project follow-up — 2026-11-09

User-authorized follow-up to the completed finite Phase L packages:

- [x] Reconcile inventory descriptions with the completed mitt follow-up: generic methods, flow, diagnostics, explicit incremental configuration, complete map evidence and the 26-source/25-Windows symlink distinction. Preserve all case identities/counts, broad open rows and original source/platform bindings. [Documentation-only checks](INVENTORY_FOLLOWUP_CHECKS.json).

- [x] Split `page_runtime.rs` by test responsibility and extract the decorator
  class-lowering module. Preserve all 51 original page-runtime test bodies,
  helper identity, emitted output and owner boundaries. Accept only with the
  complete source-bound Native/local workspace gates, Windows 82 core oracles,
  module-size measurements and retained raw logs.
  Accepted source `backend-tree:ad7f2dc43c2b7f2eefcaadf97312ee7d8da68fb3`: 51 unchanged page-runtime test bodies;
  decorator parent 664 lines and class module 788; page-runtime parent 80 and
  six test modules at most 408 lines. Native: 179 suites / 1,027 functions;
  workspace: 7,827 passed; Windows: 25 symlink cases + 82 core oracles.
  All final gates pass. [Measurement](MODULARIZATION_MEASUREMENT.json).
- [x] Establish a fixed real-project audit using unchanged mitt 3.0.1 source at
  `b240473b5707857ba2c6a8e6d707c28d1e39da49`. Freeze CommonJS/ES2022/strict,
  byte-exact declarations, eight named runtime scenarios, declaration consumers,
  and warm/edit/clean rebuild checks before recording. Audit completion means
  a reproducible measured result with explicit blockers; project compatibility
  requires every acceptance property to pass and is reported separately.
- [x] Map each reproduced project blocker to the existing G-ID, with its minimal
  source, pinned Native result, BlueTSC diagnostic and a concrete repair gate.
  This analysis does not assert that a newly discovered language gap is fixed.

  [Measured audit and repair gates](REAL_PROJECT_AUDIT.md),
  [exact results](REAL_PROJECT_MEASUREMENT.json) and
  [raw evidence](REAL_PROJECT_ARTIFACTS.json). The original investigation
  recorded **two independent blockers** and blocked downstream BlueTSC results.
  Both are repaired in the current follow-up below; the historical results and
  frozen project/reproduction bytes remain unchanged.

### Current implementation — G-T1 → G-C1 → G-M2

G-T1's existing first and byte-exact second stages remain the finite baseline.
The unchanged project exposed follow-up flow issues: erased annotation commas,
optional collection assignments and postfix non-null calls. Their repairs retain
owner readonly data through record, array and Map aliases. Object-method parsing
and generic inference are G-T2 dependencies of the requested G-C1 repair.

- [x] **Object-literal generic methods — G-T2 / G-C1 follow-up.**
  - Reproduce the frozen unconstrained and `extends string` object methods;
    preserve the accepted generic-class and ordinary-object controls.
  - Preserve method type parameters/constraints through parse, check and emit.
    Valid programs must not receive nested-function refusals or spurious
    `?`/type/`:` errors. Add wrong-key/payload and malformed-syntax controls
    against pinned 5.9.3 complete diagnostics and UTF-16 positions.
  - Pass the unchanged full mitt cold build, byte-exact declarations and their
    consumers, and all eight runtime scenarios. Preserve subsequent blockers
    explicitly; the incremental flag is separately tracked below.
  - Frozen additions: 53 Native programs plus three separately classified
    readonly/mutable owner controls. Local generic aliases, dependent keys,
    string/symbol mapped domains and complete related diagnostics are included.
- [x] **Explicit incremental configuration — G-M2 / G-C1 follow-up.**
  - The frozen scalar project builds in both compilers; enabling
    `incremental: true` alone reproduced BlueTSC's former unsupported-option error.
  - Implement valid option/state semantics and invalid-option diagnostics;
    verify configured state paths, cold/warm/edit/clean output and execution,
    and corrupt/unverifiable-state recovery within existing owner boundaries.
  - Pass the scalar control and original project's full incremental acceptance.
    Preserve the intentional owner-current rebuild differences.
  - Both `--build` and direct TypeScript `build --config` use the same state
    implementation; `--showConfig` is read-only. Eighteen frozen configurations
    cover valid/invalid options, default/custom paths and declaration/map forms.
    Twelve owner control families cover recovery, artifact hashes, collisions and
    confinement. Native `-p` reconstructs its library graph from source-only
    exported metadata and may re-emit; no Native warm no-op agreement is claimed.

Both repairs pass the fresh complete gates on `backend-tree:3567e7e2748a08e22f3422a47709fb1cd7211ec0`:
**181 Native suites / 1,040 tests**, **7,838 local workspace
passes**, both complete **8,407 × two** map checks, Windows **25 path checks +
82 core oracles + the two new cohorts and project-configuration tests**, formatting and warnings-denied Clippy.
Native expectations were frozen before their corresponding implementations;
all old case identities remain. [Acceptance and retained evidence](PROJECT_FOLLOWUP_ACCEPTANCE.md).

### Multi-file project follow-up — 2026-11-10

- [x] Freeze and execute the second independent project audit using all three
  unchanged defu sources at 1b9fcab2c1479f0295a5f867c6ec36a01fda2dfb
  (tag v6.1.4, original package metadata 6.1.3). Original licenses,
  Native expectations, the thirteen-diagnostic blocked result and its exact
  compiler binding remain in [the historical audit](DEFU_PROJECT_AUDIT.md).
- [x] **Repair the reproduced defu prerequisites — G-T1 / G-T2 / G-C1.**
  All 49 independently frozen Native controls pass their recorded verdict and
  accepted-declaration comparisons, including readonly/wrong-result/budget
  boundaries and two emitted-runtime controls. The complete unchanged project
  passes all three byte-exact declarations, twelve authored runtime scenarios,
  positive/negative and dependent consumers, incremental initial/warm builds,
  imported implementation/type edits, dependency addition/missing/restoration
  and exact edited-clean inventories. Fresh gates: 182 Native suites /
  1,042 functions, 7,839 local workspace passes, complete
  8,407 × two map reports for both actual compilers, Windows 82 core oracles +
  25 path checks + four regression cohorts, formatting and warnings-denied
  all-target Clippy. Original failed attempts and Windows console CRLF are
  retained, including the complete workspace's RegExp wall-clock and sandbox
  socket failures, both permission probes and successful unchanged complete-target
  replays; declaration/runtime byte comparisons remain exact.
  [Implementation, measurements and limits](DEFU_REPAIR_ACCEPTANCE.md).
- [x] **Evaluate multi-source incremental interchange — G-M1 / G-M2.**
  The frozen two-profile/two-direction matrix passes 92 required checks and
  three owner-path rejection checks. Three admitted production roots can
  preserve verified Native artifacts; a single root with transitive imports
  rebuilds from current owner inputs. Native reverse consumption rebuilds in
  both profiles. Implementation/type/dependency edits, consumers, runtime,
  clean artifacts and corrupt/missing-state recovery pass separately.
  The current policy required no compiler eligibility/authority extension.
  Native warm no-op and unmeasured profiles remain outside this result.
  [Finite evaluation and raw evidence](DEFU_INTERCHANGE_ACCEPTANCE.md).

### G-T7 / G-T2 — callable utility follow-up

[Fixed scope and acceptance](CALLABLE_UTILITY_FOLLOWUP.md).

- [x] Supply `Parameters`, `ConstructorParameters` and `InstanceType` through
  original static library declarations and the ordinary conditional/infer
  engine. The frozen input contains **52 forms × three profiles = 156
  observations**: ES5/CommonJS/ES5 library, ES2022/CommonJS/ES2022 library and
  ES2022/ESNext/ES5 library. Accepted/rejected controls cover fixed, optional,
  rest, receiver, overload, generic/bounded, `any`, `never`, abstract constructor
  and wrong-callable constraints, plus equivalent owner-defined aliases.
- [x] Require full primary/related diagnostics and UTF-16 positions for all
  156 observations, all 75 accepted declaration inventories byte-exact,
  independently authored positive/negative consumers of only emitted `.d.ts`
  files, selected-edition syntax and six actual runtime observations. Preserve
  the frozen Native result and original failing BlueTS replay. No built-in-name
  shortcuts, unknown widening, changed budget or omitted rejection may replace
  the declared behavior.
- [x] Complete the source-bound Native/local workspace/all-target Clippy and
  existing Windows 82-oracle gates, keep owner/library precedence and runtime
  authority unchanged, and update measured inventory facts. Commit the finished
  implementation and corresponding measurements together, then push. This finite
  package leaves broader G-T7/G-T2 compositions open.

Accepted on `backend-tree:8325633e249e34109b7622acbdd5a19f6f194992`: **183 Native suites / 1,045
tests**, **7,841 local workspace passes**, both complete **8,407 ×
two** map reports, and Windows **82 core oracles + 25 paths + five complete
regression cohorts**. All previous 39,561 case identities/classifications remain;
156 new direct observations bring the generated inventory to 39,717. Nine owner
checks remain outside the direct denominator. [Measurements and scope limits](CALLABLE_UTILITY_ACCEPTANCE.md).

### G-T7 / G-T2 — receiver utility follow-up

[Fixed scope and minimum acceptance](RECEIVER_UTILITY_FOLLOWUP.md).

- [x] Supply `ThisParameterType` and `OmitThisParameter` with ordinary
  conditional/infer semantics, preserving explicit/absent receivers, final
  overloads, generic bounds and original generic signatures when required.
  Freeze 70 forms × three strict profiles = 210 independent Native observations
  before production changes; retain the previous accepted compiler's replay.
- [x] Match every complete diagnostic and UTF-16 primary/related position, all
  105 accepted declaration inventories byte-exact, 210 isolated declaration
  consumers, 105 selected-edition syntax checks and six actual executions.
  Retain rejected noEmitOnError publication checks and six separate owner checks.
- [x] Pass the source-bound complete Native/local workspace/Clippy/map/project
  and existing Windows 82-oracle gates. Publish corresponding measurements and
  preserve prior cases and owner policies before committing and pushing.

At the receiver package's accepted source, the separate 22-name/two-profile
probe measured presence only: after those two names, `NonNullable`, `Uppercase`, `Lowercase`, `Capitalize`, `Uncapitalize`,
`NoInfer`, and ES5 `Awaited` still required implementation and semantic measurement.
The newer P0-A/P0-B acceptance above records their subsequent delivery.
This finite package does not close the 22 broader G-* rows.

Accepted on `backend-tree:4687d8d818e1d6b08bb437b41b19c9d84813356f`: **184 Native suites / 1,048
tests**, **7,843 local workspace passes**, both complete **8,407 ×
two** map reports, and Windows **82 core oracles + 25 paths + six complete
regression cohorts**. All previous 39,717 case identities/classifications remain;
210 new direct observations bring the inventory to 39,927. Six owner checks and
44 name-presence witnesses stay outside direct agreement.
[Measurements and remaining scope](RECEIVER_UTILITY_ACCEPTANCE.md).

## Completed baseline (condensed)

The 89 checked entries in the pre-refresh worklist remain completed only
within their recorded scope. Their original criteria, sub-items and evidence
are retained in the [full TODO at commit 2115412ca](https://github.com/ephoton0210/blueice/blob/2115412ca1e71baf5a7ed229e165ba15544ea105/development/browser_core/phase-18-bluets/TODO.md),
[PLAN.md](PLAN.md) and [PLAN_HISTORY.md](PLAN_HISTORY.md). A–J's earlier
leaf-level history is also retained at `1f3dbc935`; it is not repeated here.

| Completed IDs | Delivered scope / evidence location |
| --- | --- |
| A–G | Supervised page host and IPC, owner-selected DOM/events, capability-scoped debugger metadata and opaque handles, contracts, sealed compiler/MCP service and real-process acceptance; PLAN / PLAN_HISTORY sections A–G. |
| H–J | Bounded control flow, functions, classes, modules, JSX/decorators and the first compatibility inventory; PLAN sections H–J. |
| K.1–K.3, M6–M7 | Name/mutation/import checking, minimum library, return inference, project configuration, independent checking flags and diagnostic presentation; dated milestone evidence in PLAN / PLAN_HISTORY. |
| K.4–K.6, M8 | Measured type-system, class and module extensions; all recorded source splits and milestone gates. |
| K.7–K.10 | Measured target/module/helper output, declarations, resolution, JSX and decorator configurations. |
| K.11 / AC-K11.A–F | Frozen 11-row decoded source-map matrix and real launcher/core/child debugger acceptance, including malformed/substituted-map rejection and helper-frame non-disclosure; PLAN K.11 and archived K.11 journal. |
| K.12 | Owner-confined project references, ordered builds, declaration dependencies, invalidation, clean/incremental equivalence, corrupt-state recovery and bounded watch tests. |
| K.13 | Windows command/path handling and explicit symlink exclusions, historical four-leg oracle evidence, launcher diagnostics and per-file coverage; historical results keep their source/platform scope. |
| AC-K14.1–AC-K14.6 | Source-derived inventory, versioned case measurement, accepted local/VM evidence, scoped row accounting, full-parity-OFF decision and four-document consistency; PLAN AC-K14.1–AC-K14.6. |

The current acceptance criterion remains **Windows x86_64 VM 82/82 core
TypeScript oracles plus the complete local test gate**. AC-K14.3 records
commands, revisions and log digests; AC-K14.4 records the later complete
two-crate K.0 run. Hosted CI/coverage remain supplementary, not a new
all-platform or single-run prerequisite.

## Execution and acceptance rules

1. **Use the existing G-ID and stated scope.** The 22 entries below are the
   remaining areas, not 22 independent bugs. Work in priority/dependency order;
   do not invent unrelated features or extra checkpoints. A dependency-driven
   scope split needs an explicit reviewed checklist update, not an improvised
   definition of done.
2. **Freeze a finite oracle matrix before implementation.** For each stated
   form, name the source/configuration, target/lib/module/field/checking flags,
   accepted and rejected controls, and compared properties. Record N from the
   versioned list; “more combinations” or a green suite is not acceptance.
   Only pinned-5.9.3 programs establish TypeScript requirements.
3. **Test first, commit after completion.** Reproduce the failing public-boundary
   test locally, implement, and finish all acceptance checks before ticking the
   item or committing. Do not commit an unfinished/failing replay. A reviewed
   documentation-only decision is not a claim that a runtime gap was fixed.
4. **Compare the relevant observable boundary.** Checker work compares verdicts,
   primary/related codes, messages and UTF-16 module positions. Emit work adds
   Node exit/stdout/stderr and selected-edition syntax checks. Declaration work
   compares exact output and checks a consumer with pinned TypeScript. Map work
   compares complete decoded segments/metadata, not raw VLQ or selected tokens.
   The reviewed G-T1 first-stage exception below is temporary: it does not
   replace the final `.d.ts` exact-output requirement or complete G-T1.
5. **No hidden successes.** Remove a known-gap/refusal allowance only after its
   actual behavior agrees. Native-accepted language forms must not pass merely
   because BlueTS's old refusal was asserted. Keep case selectors, controls,
   reasons and evidence; regenerate counts, never hand-edit the denominator.
6. **Keep authority and deliberate policy boundaries.** Canonical owner roots,
   symlink confinement, default-denied metadata, opaque handles, runtime budgets
   and helper provenance stay enforced. The three observed-input rebuild
   differences and reference-owner escape are not bugs to erase for a pass rate.
   Document genuine native-runtime defects separately; do not copy unsafe output.
7. **Implementation gates.** Run focused regressions, both BlueTS crates' complete
   tests and every ignored oracle with `BLUEICE_BLUETSC_ORACLE=<tsc 5.9.3>`,
   `BLUEICE_ACORN_ORACLE=<acorn 8.15.0>` and `FORCE_COLOR=0`, plus workspace
   rustfmt and warnings-denied two-crate all-target Clippy. Run Cargo commands
   sequentially on the shared target. Complete Native recordings also need the
   immutable CLI hash and explicit capture variables documented in the
   [G-M3 reproduction profile](PLAN.md#reproducing-the-complete-g-m3-local-native-cohort).
   Final/milestone validation also needs the
   complete local workspace tests/all-target Clippy and the accepted Windows VM
   82 core oracles; do not add a mandatory GitHub Actions gate. Existing coverage
   thresholds/exclusions are not weakened.
8. **Evidence, status and commits.** Record source-bound commands/results and
   remaining limitations in PLAN. Update affected inventory/refusal accounting;
   publish new measurements only when actually measured and source-bound.
   Checking a scoped item does not automatically close its broader inventory row
   or turn full parity ON. Make one coherent English commit for a completed
   checklist item, cite its ID, then automatically push as authorized.
9. **Maintainability.** Review production files at 1,200 lines and split near or
   above 1,300 before adding responsibilities; exceptions require user approval.
   Preserve behavior/provenance and MPL-2.0 headers. No new blanket-refusal,
   `any`/`unknown` fallback or budget increase may substitute for the promised
   behavior. Documentation-only changes run the relevant consistency/tool checks;
   they must not claim old runtime results as a fresh test run.

## Phase L — remaining compatibility work

The source of truth is [inventory section 3](COMPATIBILITY_INVENTORY.md#3-open-gaps-against-tsc-593).
G-C2 remains closed for its measured strictness subset and is not reopened.
The separate direct BlueJS runtime profile is not a twenty-third TypeScript
gap and is not broadened by this worklist.

Each **Pass** below includes the common rules above. Freeze the actual case
list before coding; a newly found unsupported named form must remain visible.
If a package cannot meet its stated scope, leave it unchecked and request a
reviewed split instead of silently declaring a smaller success.

### P0 — reproducible wrong acceptance, rejection and diagnostics

- [x] **G-C1 — Known diagnostic differences and bounded CLI explanations.**
  - **Scope:** the nine existing source-family observations: `tuple-index`,
    `throw-newline`, `throw-empty`; native-accepted `rest-alias`,
    `compatible-function-field`, `setter-destructure`; and `catch-type`,
    `name-type`, `name-area`. Add `--explainFiles` for explicit-file,
    import, reference and library inclusion reasons within an authorized project.
  - **Pass:** the three missing diagnostics match TS2493/TS1142/TS1109 with exact
    positions; the three native-accepted forms compile; the other three match
    recorded primary/related diagnostics rather than an unmapped subset refusal.
    All nine leave their old gap/refusal allowance only after agreement.
    CLI reason lists/exit codes match the pinned matrix after path normalization,
    without leaking outside-root paths or source bodies.
  - **Evidence:** nine old observations repaired; 18 source controls and four
    exact CLI fixtures pass. All old case identities remain; the regenerated
    [measurement](COMPATIBILITY_MEASUREMENT.json) is bound to the same immutable
    backend tree as the implementation. [Commands/results](PLAN.md#phase-l--g-c1-implementation).

### P1 — core type behavior and usable standard-library precision

Order: G-T1 → G-T3 → G-T2 → G-T5 → G-T6 → G-T4 → G-T8 → G-T7.
Existing measured behavior is reused; it is not implemented again.

- [x] **G-T1 — Inferred predicates and exceptional/captured flow.**
  - **Scope:** boolean-returning named functions/arrows without explicit predicates;
    `try/catch/finally` exit/assignment flow; dotted captures with mutable aliases.
  - **Pass:** inferred predicates and narrowed expression/declaration types agree
    with 5.9.3 for every listed form, with negative controls for reassignment,
    shadowing, escaping closures and exceptional exits; no stale narrowing escapes
    its owning lexical binding. Final acceptance also requires exact `.d.ts`
    output under the same pinned compiler/options; type equivalence alone is
    not final declaration compatibility.
  - [x] **First stage — bounded type agreement (user-approved).** For the pinned
    45-program list, retain exact verdicts and complete diagnostics. Only the
    recorded `finally-return-override` declaration may temporarily differ in
    union-member order: a structural check must prove an exact permutation of
    unchanged members, with every other part of the declaration unchanged,
    and TypeScript 5.9.3 consumers must independently confirm type equivalence.
    Adding/removing/widening a member, changing grouping, or reordering tuples,
    parameters, overloads or intersection members must fail. Preserve raw native
    and BlueTS declarations and report this as an open exact-output observation,
    not a byte-for-byte agreement. Run the common complete implementation gates
    and source-bound measurements before checking or committing this stage;
    implementation and its corresponding measurement stay in the same commit.
  - [x] **Second stage — exact `.d.ts` compatibility (required).** Repair the
    declaration ordering, remove the temporary exception, and require exact
    output for every accepted program in the matrix plus the existing exact
    declaration regressions. Retain all raw observations and negative controls;
    do not substitute semantic normalization, sorted snapshots or rewritten
    native expectations. Existing explicitly documented platform normalization
    rules are not broadened. Repeat the complete gates and source-bound
    measurements. Only then may the parent G-T1 item be checked; the broader
    inventory/full-parity claim still needs its own evidence.
  - **Both stages accepted (2026-10-20):** all 45 verdict/diagnostic observations
    agree, all 35 accepted flow declarations match exactly, and the added
    90 target/library configurations match exact native declaration bytes.
    Pinned consumers, four executions, unchanged JavaScript and all negative
    controls pass. The temporary order exception is removed; original native
    observations and first-stage raw history remain unchanged. Source-frozen
    replay: 104 suites / 544 tests, including all 251 ignored functions;
    ordinary two-crate gate: 1,313 passed. Formatting, warnings-denied Clippy
    and evidence consistency pass. This closes the finite package, not broader
    unmeasured declaration/flow compositions or full TypeScript parity.
    [Evidence and reviewed two-stage acceptance](PLAN.md#phase-l--g-t1-implementation).

- [x] **G-T3 — Composed type operators and key/query precision.**
  - **Scope:** nested conditional/mapped/indexed types, template-`infer` and escaped
    template patterns, overloaded callable queries, primitive/library/symbol keys.
  - **Pass:** frozen operator compositions agree on accepted/rejected assignments
    and declarations; diagnostic spans and escape normalization agree. Below/above
    depth/expansion controls retain bounded, precise failures rather than widening
    the budget or silently substituting `unknown`.
  - **Frozen acceptance:** 76 programs (38 accept/reject pairs) across nested
    operators, escaped template/infer patterns, overloaded queries, primitive/
    library keys and symbol indices. Compare complete diagnostics/UTF-16 positions
    and exact accepted declarations; require native consumers, selected-edition
    syntax and three executions. Six budget/authority controls and 22 independent
    native key profiles are counted separately, not as direct agreements.
  - **Completed (2026-10-20):** repaired CommonJS declarations, distributed
    branch-member substitution and shared well-known-symbol identities.
    Replacement source-frozen replay passes 105 suites / 550 tests, including
    all 253 ignored functions; the stopped earlier run is not credited.
    Final no-exclusion two-crate gate passes 1,317 tests. Workspace formatting,
    warnings-denied Clippy and tool/evidence/document checks pass. The broader
    inventory row stays open for unmeasured compositions; full parity stays OFF.
    [Matrix and evidence](PLAN.md#phase-l--g-t3-implementation).

- [x] **G-T2 — Recursive/contextual generic candidate inference.**
  - **Scope:** recursive record/operator arguments, callback return context,
    dependent defaults/constraints and competing covariant/contravariant candidates.
    “Higher-kinded patterns” must be concrete pinned-TypeScript programs, not a
    newly invented language feature.
  - **Pass:** inferred type arguments, selected signatures, constraint diagnostics
    and exported types agree across the frozen candidate/context matrix, including
    rejected candidates and budget controls.
  - **Frozen acceptance:** 76 strict ES2022 programs (38 accepted/rejected pairs)
    across recursive/operator structures, callback context and body checks,
    dependent defaults/constraints, directed candidates and selected signatures.
    Require complete primary/related diagnostics with UTF-16 positions, 38 raw
    byte-exact declarations and bidirectional native consumers, selected-edition
    syntax, four executions, and three separately counted owner budget controls.
  - **Completed (2026-10-20):** candidate/cycle/context fixes pass complete
    source-frozen replay: 106 suites / 555 tests, including all 255 ignored
    functions. All 12,817 prior assertions and original 66 native rows remain
    unchanged. Final no-exclusion two-crate gate passes 1,320 tests. Workspace
    formatting, warnings-denied Clippy and tool/evidence/document checks pass.
    The broader inventory row stays open; full parity remains OFF.
    [Matrix and evidence](PLAN.md#phase-l--g-t2-implementation).

- [x] **G-T5 — Computed symbols, tuple and const/satisfies compositions.**
  - **Scope:** computed symbol/index access, optional/rest/named and variadic tuple
    combinations, nested `as const`/`satisfies` and `unknown`/`never` flow.
  - **Pass:** read/write and assignment verdicts, tuple positions and declarations
    agree with native controls, including readonly, optional and unchecked-index
    flag pairs; the discarded tuple-read diagnostic from G-C1 remains fixed.
  - **Frozen acceptance:** 43 paired forms / 86 programs, complete primary and
    related UTF-16 diagnostics, 43 byte-exact declarations with native consumers,
    six executions, two budget controls and three separate readonly/opaque owner
    controls. Strict/legacy policy pairs retain mutable acceptance; the deliberate
    owner-policy differences do not inflate TypeScript agreement.
  - **Completed (2026-10-20):** corrected source-frozen replay passes 107 suites /
    561 tests, including all 257 ignored functions. Final no-exclusion two-crate
    gate passes 1,324 tests across 167 targets; formatting, warnings-denied Clippy
    and evidence/document/tool checks pass. All 12,896 prior assertions remain
    unchanged. The broader row stays open; full parity remains OFF.
    [Matrix and evidence](PLAN.md#phase-l--g-t5-implementation).

- [x] **G-T6 — Structural compatibility across nested expression boundaries.**
  - **Scope:** computed/conditional/spread expressions at assignment, argument and
    return boundaries; nested generic callback variance and structural protocols.
  - **Pass:** freshness and variance agree with strictFunctionTypes on/off and
    exactOptionalPropertyTypes/noUncheckedIndexedAccess controls; native-accepted
    programs stay accepted and native-rejected programs are not hidden by opaque
    compatibility types.
  - **Frozen minimum:** `gt6_structural/cases.json` / `reference.json`: 28
    accepted/rejected pairs (56 observations), complete diagnostics including
    UTF-16 primary/related spans, 28 byte-exact declarations with bidirectional
    native consumers and ES2022 syntax checks, and two Node executions. Cover
    computed literal keys, conditional branches, explicit versus spread/stale
    properties at all three boundaries; nested function/generic/higher-order
    callbacks, method/generic protocols, strictFunctionTypes on/off, and both
    optional/indexed flag values. Two owner-budget controls are not native
    compatibility assertions. No old expectation or policy allowance may change.
  - **Completion gate:** source-bound full oracle measurement, complete ordinary
    tests for both compiler crates, cache-epoch invalidation controls, rustfmt,
    warnings-denied Clippy, and evidence/document/tool consistency checks;
    implementation and measured evidence are submitted together. The broader
    G-T6 row remains open beyond this finite matrix; full parity remains OFF.
  - **Completed (2026-10-21):** all 56 native observations and 28 raw declarations
    pass, with two executions and two separate owner-budget controls. The frozen
    replay passes 108 suites / 566 tests, including all 259 ignored functions;
    complete ordinary tests pass 168 targets / 1,327 tests. Final formatting,
    warnings-denied Clippy and evidence/tool/document checks pass. All 12,987
    prior assertions remain unchanged. [Source and acceptance](PLAN.md#phase-l--g-t6-implementation).

- [x] **G-T4 — Overload selection with generic/optional call chains.**
  - **Scope:** call/construct/method overloads combined with generic callbacks,
    variance, optional calls and non-null assertions; merged declaration ordering.
  - **Pass:** selected result types, no-match/implementation diagnostics and
    declarations agree for both signature-order controls; accepted calls execute
    identically and retain receiver/evaluation-order semantics.
  - **Frozen minimum:** `gt4_overload_chains/cases.json` / `reference.json`:
    51 accepted/rejected pairs (102 observations), both signature orders for
    direct/generic/literal-priority, construct, method, optional and merged
    declarations; strictFunctionTypes on/off; optional receiver/member/function,
    non-null callee/receiver/argument/result and parenthesized calls. Include
    contextual-candidate isolation, no-match/implementation origins, null/undefined
    guards, strictNullChecks on/off, nested argument/construction checks and a
    unary-`!` runtime control.
  - **Acceptance:** full diagnostic arrays with UTF-16 primary/related spans,
    51 byte-exact declarations with native bidirectional consumers and ES2022
    Acorn syntax, plus four Node exit/stdout/stderr comparisons. Two budget and
    two readonly/never policy controls (strict and legacy, mutable positives)
    are separate from TypeScript agreement. All prior native observations and
    policy allowances remain intact; no opaque/any/unknown widening is added.
  - **Completion gate:** full source-bound oracle cohort and complete ordinary
    two-crate tests, cache epoch invalidation, formatting, warnings-denied
    Clippy and preserved-case/log/tool/document checks pass before submitting
    implementation together with its measured data. Broader unmeasured
    combinations remain open; full parity stays OFF.
  - **Completed (2026-10-21):** all 102 native observations, 51 exact declarations,
    bidirectional consumers, four executions and four separate owner controls
    pass. The source-frozen replay passes 109 suites / 572 tests, including all
    261 ignored functions; the complete ordinary two-crate gate passes 169
    targets / 1,331 tests. Formatting, warnings-denied Clippy and case/log/tool/
    document checks pass. All 13,045 previous assertions remain unchanged.
    [Source, commands and acceptance](PLAN.md#phase-l--g-t4-implementation).

- [x] **G-T8 — Contextual return/getter inference and literal freshness.**
  - **Scope:** local/imported literal freshness, contextual object/union returns,
    getter/method bodies, async/generator results and recursive local functions.
  - **Pass:** inferred declarations and downstream consumer diagnostics agree;
    every fixture with a concrete native result retains that precision instead of
    an accidental `any`/`unknown` fallback. Recursive/resource failures stay bounded.
  - **Frozen minimum:** `gt8_return_context/cases.json` / `reference.json`:
    40 accepted/rejected pairs (80 native observations), strict ES2022/ESNext
    with an explicit ES2022 library. Cover local/aliased/imported fresh versus
    regular literals, const assertions, branch/record returns, contextual record/
    discriminated-union/array/callback returns, object/class getters and methods
    with locals/this/forward calls, async/await/imported results, generator yield/
    return/next, async generators and local tail/forward/embedded/mutual recursion.
  - **Acceptance:** full diagnostic arrays with UTF-16 primary/related spans;
    40 exact declaration graphs (47 `.d.ts` files), 80 type-only downstream
    accept/reject diagnostic controls, native bidirectional consumers and ES2022
    Acorn syntax, plus four Node executions. Type-only consumers grant no runtime
    counterpart; retain one separate declaration-value-import refusal control
    in strict/legacy checking and two configured budget/depth controls. Every
    original producer observation, old fixture and policy allowance stays intact.
  - **Completion gate:** complete source-bound oracle cohort, no-exclusion
    ordinary tests for both compiler crates, cache invalidation, formatting,
    warnings-denied Clippy and case/log/tool/document audits must pass before
    checking this item or committing implementation with its measured data.
    The broader G-T8 row stays open beyond this frozen matrix; full parity is OFF.
  - **Completed (2026-10-22):** all 80 native observations, 40 declaration graphs /
    47 exact files, 80 downstream diagnostics, four executions and three separate
    owner/budget controls pass. The final source-frozen replay passes 110 suites /
    578 tests, including all 263 ignored functions; the post-publication ordinary
    two-crate gate passes 170 targets / 1,335 tests. Cache epoch 15, formatting,
    warnings-denied Clippy and case/log/tool/document checks pass. All 13,151 old
    assertions and 80 original G-T8 producer observations remain unchanged.
    [Source, commands and acceptance](PLAN.md#phase-l--g-t8-implementation).

- [x] **G-T7 — Expand the original standard-library API manifest.**
  - **Scope:** freeze the remaining constructor/overload/member/protocol list from
    STANDARD_LIBRARY omissions: Array/ReadonlyArray callbacks, Promise aggregates
    and chaining, collections/iteration, primitive/Object/Function utilities,
    Math/JSON, Symbol, errors, Date/RegExp, Reflect/Intl/BigInt and buffer/typed-array/
    Atomics typing. DOM/host APIs remain owner supplied.
  - **Pass:** every manifest entry has native accept/reject and inferred-result
    coverage under its eligible target/lib; generic map/chaining and declared
    constructors no longer rely on the recorded opaque fallbacks. Owner replacement,
    content/version fingerprints and no-runtime-grant controls pass. Correct
    STANDARD_LIBRARY.md's stale v1/construct-signature claims against the existing
    v2 profiles/parser before adding declarations; do not reimplement existing forms.
  - **Frozen minimum:** `gt7_library_manifest/cases.json` / `reference.json`:
    217 accepted/rejected pairs (434 native observations) in 19 named API
    families, strict ES2022/ESNext with an explicit ES2022 library. Cover generic
    Array/ReadonlyArray callbacks and constructors, Promise adoption/chaining/
    aggregates, collection constructors and computed iterator protocols, primitive/
    Object/Function utilities, Math/JSON/Symbol, callable/new error constructors
    and cause options, Date/RegExp, Reflect/Intl/BigInt, buffers/DataView, all eleven
    numeric/bigint typed arrays and Atomics number/bigint results.
  - **Acceptance:** 224 byte-exact declaration files (217 API programs plus
    seven explicit-library profiles), 434 type-only downstream accept/reject
    diagnostic observations, native bidirectional consumers, syntax checks and
    five Node executions. Seven ES2015–ES2022 profiles use ES5 output with the
    eligible library and with the immediately preceding library (14 native
    observations); no missing library may acquire an opaque fallback. Keep five
    separate owner controls for replacement, runtime denial, source confinement,
    exact content/version fingerprints and configured generic-work budgets.
  - **Completion gate:** preserve all 13,234 previous assertion identities and
    expectations; run the complete source-frozen oracle cohort and ordinary
    two-crate tests, cache invalidation, formatting, warnings-denied Clippy and
    all source/case/log/tool/document audits before checking or submitting this
    item. Submit implementation and completed measurement together. The broader
    G-T7 row stays open beyond the frozen manifest; full parity remains OFF.
  - **Completed (2026-10-22):** all 448 native API/profile observations, 224 exact
    declaration graphs/files, 434 downstream diagnostics, five executions and
    five separate owner controls pass. Source-frozen replay: 111 suites / 585
    tests, including all 265 ignored functions. Final no-exclusion ordinary gate:
    171 targets / 1,341 tests. All 13,234 old suite-qualified assertion identities
    and expectations remain unchanged; the denominator adds exactly 453. Cache
    epoch 16, formatting, warnings-denied Clippy and source/case/log/tool/document
    audits pass. All modified production modules remain below 1,200 lines.
    [Source, commands and acceptance](PLAN.md#phase-l--g-t7-implementation).

### P2 — modules, packages, classes, declarations and projects

- [x] **G-T10 — Import type expressions and remaining module/type compositions.**
  - **Scope:** `import('specifier').Type`, `typeof import('specifier')`, qualified/
    generic projections, remaining merged declaration/import-elision forms and the
    recorded value import of a type-only namespace.
  - **Pass:** the frozen ESM/CommonJS type/value matrix matches native verdicts,
    declarations, import retention/elision and execution. Newly discovered type
    dependencies enter the authorized graph, budgets and fingerprints; a type
    expression never grants runtime or outside-root access.
  - **Frozen minimum:** `gt10_import_types/cases.json` / `reference.json` records
    30 accept/reject forms and nine additional rejection forms: **133 native
    observations** under ES2022 / ESM and CommonJS (verbatim forms use ESM).
    Cover named/default/qualified/generic imports, defaults/constraints, whole
    and qualified value queries, generic function queries, indexed/keyof/mapped/
    conditional projections, recursion/type cycles, named/star re-exports,
    merged interfaces/function/class/namespaces, ordinary unused/mixed/type-only
    namespace imports, declaration-file edges and verbatim retention/diagnostics.
  - **Acceptance:** 58 byte-exact declaration graphs (**118 emitted `.d.ts`
    files**), 116 type-only downstream accept/reject observations, native
    bidirectional consumers, complete emitted import retention/elision, ES2022
    syntax and **15 Node executions**. Native options explicitly select ES2022 /
    DOM libraries; only TypeScript's vendor default-lib checking is skipped,
    never owned declaration checking. This grants no DOM/runtime authority.
    Normalize only the recorder's virtual root in native diagnostic paths;
    preserve messages and all primary/related UTF-16 spans otherwise.
  - **Completion gate:** preserve all 13,687 old suite-qualified identities and
    native expectations. Only the two recorded type-only-namespace value-import
    refusal allowances may become agreements, after their actual native behavior
    matches; no case is deleted or removed from the denominator.
    Retain both newly exercised ES2022/ES2020 execution configurations of that
    same legacy namespace program; they are not extra frozen G-T10 forms. Then
    run the full source-bound native cohort, ordinary two-crate gate, cache
    invalidation, formatting, warnings-denied Clippy and evidence audits. Five
    separate owner controls cover canonical confinement, declaration/runtime
    separation, unchanged work limits, changed/missing/restored type-edge
    invalidation and non-execution. No new opaque fallback, runtime grant or
    budget increase is allowed. Implementation and completed measurement must
    be submitted together; broader unmeasured compositions and full parity
    remain open.
  - **Accepted (2026-10-22):** all 133 native observations, 58 exact declaration
    graphs / 118 `.d.ts` files, 116 downstream observations, 15 executions and
    five separate owner controls pass. The two old refusal classifications are
    repaired without deleting any old ID/expectation; both newly exercised
    legacy namespace targets pass. Final immutable-source replay: **112 suites /
    594 tests / all 267 ignored functions**; complete ordinary two-crate gate:
    **172 targets / 1,349 passed**, zero failures. Epoch 17 invalidation,
    formatting, warnings-denied Clippy and case/source/log/tool/document audits
    pass; all 39 changed production modules stay below 1,200 lines. This closes
    the finite package only: broader G-T10 stays open and full parity stays OFF.
    [Source, commands and acceptance](PLAN.md#phase-l--g-t10-implementation).

- [x] **G-M1 — Package/mapped JavaScript, JSDoc and resolver compositions.**
  - **Scope:** package and paths-mapped `allowJs/checkJs` inputs, JSDoc types,
    combined exports/typesVersions/conditions/rootDirs/typeRoots selection and
    missing-type-library diagnostics.
  - **Pass:** selected canonical files, diagnostics, emitted/runtime suffixes,
    declarations and execution agree for the frozen resolution matrix; changed/
    missing manifests invalidate correctly. Include owner escape, symlink, ordered
    fallback and TypeScript-sibling precedence controls before any outside read.
  - **Frozen minimum (2026-10-28):** `gm1_resolver_compositions/cases.json` /
    `reference.json` pins **37 forms / 134 observations** (67 accept / 67 reject)
    against TypeScript 5.9.3. Cover package main/CommonJS/`.cjs`, mapped `.mjs`,
    JSDoc type/param/return/typedef/property/template/optional/rest/array/union/
    import types, checkJs and per-file directives, declaration/@types precedence,
    package JS depth, explicit-JS versus extensionless sibling selection, ordered/
    longest-prefix/baseUrl/rootDirs mapping, typesVersions, exports/conditions,
    scoped typeRoots, missing explicit/implicit libraries and a combined graph.
  - **Acceptance:** 67 exact declaration graphs (**118 `.d.ts`/`.d.mts` files**),
    134 downstream observations, complete diagnostics/related UTF-16 positions,
    selected inputs, artifact/suffix inventories, retained imports, ES2022 syntax
    and **67 Node executions**. Paths/rootDirs use separately frozen, confined
    owner runtime mappings shared by both compilers; they do not claim TypeScript
    or BlueTS rewrites those specifiers. ES2022 is the explicit library profile;
    vendor/owned declaration checking is not skipped.
  - **Completion gate:** preserve all 13,827 old case identities, classifications
    and native expectations. Five separately counted owner controls cover
    canonical confinement, terminal authority failures, native sibling priority,
    manifest change/missing/restoration invalidation and shared JSDoc/type-edge
    work/runtime bounds. Freeze the final source only after broad preflight
    regressions, then complete the source-bound cohort, no-exclusion ordinary
    two-crate gate, formatting, warnings-denied Clippy and all evidence audits.
    No commit/checkmark before complete implementation and matching measured
    evidence; broader unmeasured combinations/full parity remain open.
  - **Result (2026-10-28, darwin-arm64):** all frozen observations, declarations,
    consumers, executions and five owner controls pass. Immutable **113-suite /
    606-test** measurement executes all **269** selected ignored functions;
    the no-exclusion ordinary gate passes **173 targets / 1,359 tests**.
    Format, warnings-denied Clippy and source/case/tool audits pass. The old
    13,827 case identities/classifications and native fixtures are preserved.
    See [implementation and measured evidence](PLAN.md#phase-l--g-m1-implementation).

- [x] **G-T9 — Remaining class representation and checking.**
  - **Scope:** unresolved getter bodies, generic merged interfaces/heritage,
    computed heritage, generator/auto-accessor members, and stateful class-expression
    keys involving await/yield. Setter-destructuring and compatible method/property
    false rejections fixed by G-C1 remain regression controls.
  - **Pass:** native verdicts, declarations and high-target execution agree for the
    named forms. Parameter-property/branch-super cases cover both field modes and
    retain the documented native-runtime defect boundary rather than publishing
    unsafe output. Decorator-specific and low-target lowering combinations belong
    to G-E3/G-E1 and are not counted twice as new class work.
  - **Frozen minimum (2026-10-28):** `gt9_classes/cases.json` / `reference.json`
    pins **28 forms / 252 native observations**, CommonJS/ESNext, both field
    modes, ES2022 and ES2020 suspension controls, strict ES2022 libraries and no
    skipped declarations. Getter calls/flow/records/inheritance; own, parent,
    imported and inherited generic merges; parenthesized/called/indexed/imported
    heritage; explicit/inferred/async generators; instance/static/inherited
    auto-accessors; await/yield keys with private/static state; root/branch super;
    existing setter-destructuring and method/property regressions are named.
  - **Acceptance:** 124 safe declaration graphs / **136 byte-exact files**,
    248 downstream observations, retained imports, selected inputs, complete
    diagnostics/related UTF-16 positions, selected-edition syntax and **124 actual
    executions**. Two additional native-accepted define-mode branch-super rows
    reproduce ReferenceError before super; record their two declaration files
    and four consumers as native references, with BlueTS refusing publication.
    Assignment-mode branch super retains native TS2401. These safe refusals are
    not counted as TypeScript agreements or fixed runtime bugs.
  - **Owner/gates:** five separate controls cover charged class-type work,
    constructor-type/runtime-global separation, per-evaluation key/store effects,
    precise getter/member origins and refusal of the native runtime defect.
    Preserve all 13,966 old case identities/native fixtures and unrelated labels;
    change an existing allowance only with measured repaired behavior. Complete
    broad preflight, an immutable full cohort, no-exclusion ordinary two-crate
    tests, format, warnings-denied Clippy and all source/case/tool audits before
    checking this item and committing implementation with matching measurement.
  - **Accepted 2026-10-28:** 114 source-bound suites / 618 tests, including all
    271 ignored functions; 1,369 ordinary tests across 174 targets, format,
    Clippy and source/status audits pass. All 13,966 old case identities and
    classifications remain intact. See [the acceptance record](PLAN.md#phase-l--g-t9-implementation).

- [x] **G-D1 — Anonymous, contextual and cross-module declaration precision.**
  - **Scope:** anonymous object unions, imported/local freshness, const/contextual
    inference, overload combinations and merged declaration surfaces.
  - **Pass:** exact declarations and a separate pinned-TypeScript consumer agree
    for each named form, including inaccessible/unnameable external-type diagnostics;
    import retention and stable symbol identities survive repeated builds.
  - **Frozen minimum (2026-10-28):** `gd1_declarations/cases.json` /
    `reference.json` records **28 forms / 416 native observations**, ES2022 with
    explicit ES2022 libraries, CommonJS/ESNext, both field modes and both
    `exactOptionalPropertyTypes` settings, strict checking and no skipped libraries.
    Six anonymous object/array/tuple union forms; four local/imported/re-exported
    literal/enum freshness forms; two const-generic and three contextual forms;
    three overload compositions; four function/class/interface/ambient merges;
    two public external-type naming forms; four private external record/tuple/
    return/generic diagnostic forms are named before production changes.
  - **Acceptance:** native verdicts and complete diagnostics, related information
    and UTF-16 positions agree for all 416 observations. The 192 accepted graphs
    contain **248 byte-exact declaration files**, **384 separate consumer
    observations** and **192 actual executions**, with selected inputs, retained
    imports and ES2022 syntax checked. Source-rejected pairs and TS4023/TS4058
    controls publish no partial output. No unknown/any substitution, declaration
    normalization or rewritten native expectation may hide a difference.
  - **Owner/gates:** five separate controls cover charged declaration work,
    declaration/runtime authority separation, cached/fresh/repeated symbol and
    import stability, failed-publication atomicity and canonical confinement.
    Preserve all 14,223 old suite-qualified case identities/native fixture bytes
    and unrelated classifications. Complete focused and broad preflight,
    immutable full-cohort measurement, unfiltered ordinary two-crate tests,
    format, warnings-denied Clippy and source/case/tool/status audits before
    checking this item or committing implementation with matching evidence.
  - **Completion (2026-10-28):** all 416 native observations, 248 exact declaration
    files, 384 consumer observations, 192 actual executions and five owner
    controls pass. The immutable full cohort passes **115 suites / 630 tests /
    all 273 ignored functions**; the unfiltered ordinary gate passes **175
    targets / 1,379 tests**. All 14,223 previous case identities/classifications
    and native fixture bytes remain unchanged. Formatting, warnings-denied
    Clippy and source/case/tool/status audits pass. Backend tree `7dadd56b` and
    complete evidence are recorded in [PLAN](PLAN.md#phase-l--g-d1-implementation).
    The broader G-D1 row stays open; full parity remains OFF.

- [x] **G-D2 — Non-scalar declaration maps and isolated-declaration checking.**
  - **Scope:** class, generic, namespace, inferred and merged declaration maps,
    plus their `isolatedDeclarations`/declaration-only output combinations.
  - **Pass:** complete decoded maps, names/source metadata, declarations and artifact
    inventories match native fixtures; unsupported scalar-only refusals are removed
    only for implemented forms. CLI output-root/symlink and corrupt-map bounds stay
    enforced; accepted declarations compile in the native consumer.
  - **Frozen minimum (2026-10-28):** `gd2_declaration_maps/cases.json` and
    `reference.json` name **31 forms / 392 native observations** before production
    changes: 18 class/generic/interface/alias/namespace/merged/inferred/imported/
    Unicode map forms with source-rejected pairs, plus 13 isolated-inference
    boundaries. Cross CommonJS/ESNext, declaration-only/runtime output and
    isolated on/off under strict ES2022 with explicit ES2022 libraries and no
    skipped declarations; include field assignment mode, CRLF, declarationDir,
    mapRoot/sourceRoot and Unicode coordinates in their named configurations.
  - **Acceptance:** all verdicts and complete primary/related diagnostics with
    original UTF-16 positions agree. The 196 accepted graphs contain **204 exact
    declaration files / 204 complete decoded declaration maps / 4,824 segments**;
    compare every metadata field, artifact key and retained import. Require **392
    separate consumer observations** and **98 actual Node executions**, selected
    inputs and ES2022 syntax. Raw VLQ spelling and sampled mappings are not the
    comparator. The metadata form's JS maps are inventory-only here; broader JS
    mapping precision remains G-E5.
  - **Owner/gates:** six separate controls cover charged map work, canonical
    source/declaration authority, output-root/symlink confinement, atomic failed
    publication, cached/fresh/repeated map-symbol stability, and bounded rejection
    of corrupt maps. Preserve all **14,644 previous case identities and
    classifications** and all old native fixture bytes. Complete focused/broad
    preflight, immutable full-cohort measurement, unfiltered ordinary two-crate
    tests, format, warnings-denied Clippy and all source/case/tool/status audits
    before checking this item or committing implementation with matching data.
  - **Completion (2026-10-28):** all 392 native observations, 204 byte-exact
    declarations, 204 complete decoded declaration maps / 4,824 segments,
    392 consumer observations, 98 actual executions and six owner controls pass.
    The immutable cohort passes **116 suites / 643 tests / all 275 ignored
    functions**; the unfiltered ordinary gate passes **176 targets / 1,390
    tests**. All 14,644 previous case identities/classifications and old native
    fixture bytes remain unchanged. Format, warnings-denied Clippy and all
    source/case/tool/status audits pass. Backend tree `aa78e531` and complete
    evidence are in [PLAN](PLAN.md#phase-l--g-d2-implementation). The broader
    G-D2 row stays open; full parity remains OFF.

- [x] **G-M2 — Native build continuation, state interchange and watch integration.**
  - **Scope:** default `--build` error continuation (distinct from
    `--stopBuildOnErrors`), pinned native `.tsbuildinfo` interchange, direct source-file
    CLI invocations, and imported/package watch edit/add/delete/burst sequences.
  - **Pass:** ordered build/skip events, exit codes, outputs and declarations agree;
    native/BlueTS state cross-use is demonstrated, not assumed from a filename.
    Foreign/unverifiable state rebuilds safely. Watch results equal clean builds,
    and bare-source options follow the frozen CLI matrix. Keep owner-current
    manifest/nearer-package/repointed-symlink invalidation and owner-escape refusal
    as explicit policy differences, not weakened success criteria.
  - **Frozen minimum (2026-10-29):** `gm2_build_interop/cases.json` and the native
    reference name **296 observations** before production changes: 96 cold,
    retry, recovery and warm build observations across chain/parallel/branch,
    CommonJS/ESNext, `noEmitOnError` and `stopBuildOnErrors`; 160 observations
    across four state forms, both modules, runtime/declaration-only output and
    default/custom state paths; 20 bare-source CLI controls; and 20 actual
    imported/package watch cycles. Watch uses named polling options and explicit
    declaration input selection, updated together on add/delete. Unlisted-input
    watch combinations are outside this finite matrix.
  - **Acceptance:** exact ordered build/current/signature-current/timestamp/
    blocked/skip events, exit codes,
    complete primary/related diagnostics and UTF-16 positions agree. Compare
    selected inputs, artifact inventories, **506 byte-exact declarations**, retained
    imports, **556 independent consumer observations** and **408 actual executions**.
    State cross-use requires real pinned `readBuilderProgram` parsing of BlueTS
    version/signature maps, native no-op and edit consumption, and BlueTS reuse
    of native-produced states; a renamed foreign file is not acceptance. Every
    watch cycle also equals its fresh build. Do not normalize declarations or
    erase error/pending state to turn a failure into a cache hit.
  - **Owner/gates:** six controls cover canonical continuation authority, bounded
    state/hash/path validation, failed/pending-check invalidation, bare-source
    root/output atomicity, owner-current watch resolution and bounded watch
    lifecycle. Preserve all **15,042 previous case identities/classifications**
    and old native fixture bytes. Complete focused and broad regressions,
    immutable full-cohort measurement, unfiltered ordinary two-crate tests,
    format, warnings-denied Clippy and all source/case/tool/status audits before
    checking the item or committing implementation with matching measurements.
  - **Completion (2026-10-29):** all 296 native observations, 506 byte-exact
    declarations, 556 consumer observations, 408 actual executions and six owner
    controls pass. Real native no-op/edit consumption and BlueTS reuse prove
    bidirectional state interchange; every watch cycle equals its fresh build.
    The immutable cohort passes **117 suites / 660 tests / all 277 ignored
    functions**; the final unfiltered ordinary gate passes **177 targets / 1,405
    tests**. All 15,042 previous case identities/classifications and old native
    fixture bytes remain unchanged. Format, warnings-denied Clippy and all
    source/case/tool/status audits pass. Backend tree `281dd018` and complete
    evidence are in [PLAN](PLAN.md#phase-l--g-m2-implementation). The broader
    G-M2 row stays open; full parity remains OFF.

### P3 — remaining emit, mapping and JSX/decorator combinations

- [x] **G-E1 — Unsupported downlevel control-flow and class combinations.**
  - **Scope:** the recorded `opaque-generator-es5` graph; computed heritage/`super`,
    constructor spreads, nested binding patterns, async arrows/methods,
    template-contained suspension and combined suspension/exception paths.
  - **Pass:** every frozen supported target configuration matches native Node
    completion/stdout/stderr and declarations, and parses at its selected edition;
    include rejected shapes, iterator/finally completion and helper-collision controls.
    Remove the named ES5 refusal only after real execution agrees.
  - **Frozen minimum (2026-10-29):** `ge1_downlevel/cases.json` names **38
    forms / 576 native observations** before production changes: 500 accepted
    and 76 rejected configurations across ES5/ES2015/ES2016/ES2017/ES2018/ES2022,
    CommonJS/ESNext, both class-field modes for class forms, and named array/
    iterator binding controls. The original opaque-generator program remains
    visible. Native ES5 super-accessor rejection is a rejected control, not a
    requested accepted lowering.
  - **Acceptance:** complete primary/related diagnostics and UTF-16 positions,
    selected inputs, artifact inventories, **524 byte-exact declarations**,
    retained imports, **1,000 independent consumer observations** and **500 actual
    Node completions/stdout/stderr** agree; every output parses at its selected
    edition. Include switch/fallthrough, conditional/binary/logical/computed
    suspension, return/throw/break/continue through finally, delegation, computed
    heritage/super calls, constructor spreads, nested/defaulted bindings, async
    arrows/methods, nested awaits and template-contained suspension. Preserve the
    frozen list; do not replace a native-accepted case with an asserted refusal.
  - **Owner/gates:** six separate controls retain canonical source/import/output
    authority, atomic failed publication, parser/lowering work budgets, helper
    collision/cache identity, existing runtime/helper policy and repeated iterator/
    finally output stability. Preserve all 15,344 old case IDs; change only the
    named opaque-generator refusal to agreement after actual execution proves it.
    All other old classifications and native fixture bytes stay unchanged.
    Complete focused/broad regressions, immutable full-cohort measurement,
    unfiltered ordinary tests, format, warnings-denied Clippy and source/case/
    tool/status audits before checking the item or committing implementation and
    its matching measurement together.
  - **Delivered (2026-10-29):** all 576 native observations and six owner controls
    pass, including all exact declarations, consumers and actual executions above.
    Immutable full measurement: **118 suites / 673 tests / all 279 ignored
    functions executed**; final ordinary gate: **178 targets / 1,416 passed**,
    zero failed/filtered. Format, warnings-denied Clippy and all source/case/tool/
    status audits pass. [Acceptance record](PLAN.md#phase-l--g-e1-implementation).
    The broader G-E1 row stays open; full parity remains OFF.

- [x] **G-E2 — Node ESM import-equals and module/helper ABI combinations.**
  - **Scope:** ESM Node import-equals lowering, Node declaration inputs and the
    currently unmeasured module/target/importHelpers/noEmitHelpers combinations.
  - **Pass:** loader execution, live/cyclic bindings, helper imports/ABI, output
    suffixes, declarations and rejection diagnostics match the frozen configurations;
    helper source/version and cache identity remain bound to the artifact.
  - **Frozen minimum (2026-10-29):** `ge2_module_helpers/cases.json` retains
    **2,604 configurations / 24 forms**. Fourteen helper families cover nine
    module/Node-format profiles, five target boundaries and all four
    `importHelpers`/`noEmitHelpers` pairs (2,520 rows); 68 Node input/ESNext
    controls and 16 missing/incomplete-provider controls complete the list.
    Native accepts 2,545 and rejects 59. All ten unmodified tslib 2.8.1 provider
    files retain their original notices and recorded hashes.
  - **Acceptance:** all safe accepted outputs compare **3,095 byte-exact
    declarations, 5,054 independent consumer observations and 3,424 actual loader
    scenarios**, including missing globals, both UMD branches and live cycles.
    Compare selected inputs, imported ABI, suffixes, syntax, complete primary/
    related diagnostics and UTF-16 positions. Eighteen native System outputs
    omit `__rest`/`__read`; retain their actual errors as references and require
    BlueTS's precise refusal rather than copying unsafe output. Six owner
    controls preserve canonical helper/source authority, atomic publication,
    charged work, source/version/cache identity, Node-format invalidation and
    existing runtime/map policy. Keep all 15,926 old case identities,
    classifications and native fixture bytes. Finish all common gates and
    source-bound complete measurement before checking or committing this item.
  - **Delivered (2026-10-29):** all 2,604 frozen configurations and six owner
    controls pass their stated boundaries, including all exact declarations,
    consumers and actual loader scenarios above. The 18 unsafe native System
    outputs remain references with precise BlueTS refusals. Immutable full
    measurement: **119 suites / 686 tests / all 281 ignored functions executed**;
    final ordinary gate: **179 targets / 1,428 passed**, zero failed/filtered.
    Format, warnings-denied Clippy and source/case/tool/status audits pass.
    [Acceptance record](PLAN.md#phase-l--g-e2-implementation). The broader G-E2
    row stays open; full parity remains OFF.

- [x] **G-E3 — Literal/private-super decorators and imported metadata.**
  - **Scope:** literal decorated names, super in decorated private callables,
    static-initializer super writes/computed super keys, decorated class expressions,
    and imported/aliased legacy metadata types.
  - **Pass:** frozen standard/legacy × target × field-mode configurations agree on
    evaluation/replacement/initializer order, receivers, execution, declarations and
    diagnostics; imported metadata resolves through existing owner authorization.
    Preserve versioned helper identity, source provenance and precise remaining refusals.
  - **Frozen minimum (2026-10-29):** `ge3_decorator_compositions/cases.json`
    retains **1,000 configurations**: 20 decorator forms across both modes,
    four targets, both modules and field modes (640); 16 imported/aliased
    metadata families (256), isolated/metadata-flag controls (64), and 40
    independent native private-home-object counterparts. Native accepts 803
    and rejects 197. All 960 original configuration IDs remain present.
  - **Acceptance:** compare complete primary/related diagnostics and UTF-16
    positions, selected inputs/imports, artifacts, **1,158 byte-exact declarations**
    and **1,606 consumer observations**. Execute all 757 non-preserved BlueTS
    outputs and compare actual exit/stdout/stderr. For 76 broken native lowerings,
    retain the actual native failures as references and require valid BlueTS
    syntax plus execution matching the explicitly paired native safe authority;
    an old refusal is not success. The 46 proposal-preserving outputs compare
    complete decorated-class ASTs; their emitted-JavaScript-only native adapter
    has 38 successful executions and eight recorded native private-super syntax
    failures, with no direct-Node claim for preserved syntax.
  - **Owner/gates:** six controls retain canonical source/import/metadata authority,
    atomic publication, charged work, helper/source/version/cache identity, graph
    invalidation and runtime/map/default-denied debugger policy. Preserve all
    18,536 earlier suite-qualified observations and old native fixture bytes.
    Finish focused regressions, an immutable complete cohort, unfiltered ordinary
    two-crate tests, format, warnings-denied Clippy and source/case/tool/status
    audits before checking this item or submitting implementation and matching
    measurement together. [Frozen record](PLAN.md#phase-l--g-e3-implementation).
  - **Completed (2026-10-30, darwin-arm64):** all 1,000 configurations and six
    owner controls pass. The corrected immutable cohort passes **120 suites /
    700 tests / all 283 ignored functions**; the final unfiltered ordinary gate
    passes **180 targets / 1,440 tests**, zero failures or filtered functions.
    Workspace formatting, warnings-denied Clippy and source/case/tool/status
    audits pass. All 18,536 earlier observations and 12,041 old fixture files
    remain intact. The failed first cohort is not credited. The broader row
    stays open for unmeasured compositions; full parity stays OFF.

- [x] **G-E4 — Nested declaration and output-option interactions.**
  - **Scope:** nested/inferred declaration members with `stripInternal`,
    `isolatedDeclarations`, newline/BOM/comment options and class-field modes.
  - **Pass:** exact declarations, artifact lists/bytes where specified, and actual
    execution agree across the frozen option products. Strict-runtime helper
    fingerprints and publication bounds remain unchanged; declaration-map coverage
    itself is owned by G-D2/G-E5 rather than duplicated here.
  - **Frozen minimum (2026-10-30):** `ge4_declaration_options/cases.json`
    fixes 12 nested/inferred/class/namespace/imported declaration families ×
    CommonJS/ESNext × all 64 combinations of `stripInternal`,
    `isolatedDeclarations`, LF/CRLF, BOM, comment removal and class-field mode:
    **1,536 configurations / 1,152 native accepts / 384 rejects**, strict ES2022
    target/library, Node10 resolution and no skipped declaration checking.
  - **Acceptance:** complete primary/related UTF-16 diagnostics, selected inputs,
    retained imports/artifact inventories, **1,216 raw byte-exact declarations**,
    **3,456 positive/negative/internal-root consumer observations** and **1,152
    actual Node executions**. JavaScript compares exact BOM/interior-BOM and
    terminal-newline properties, newline kinds outside literal tokens, and exact
    source-comment token types/payloads. Only the exact versioned BlueTS class
    helper marker has separately checked provenance; no declaration bytes are
    normalized. Reader-only consumer options are explicitly frozen in the input.
  - **Owner/gates:** six controls retain canonical source/output authority,
    atomic publication, bounded declaration work, every option/source cache
    identity, type-only runtime denial and strict helper/map policy. Preserve all
    19,542 earlier observations and original fixture bytes. Complete the matching
    immutable cohort, unfiltered two-crate ordinary suite, format/Clippy and all
    source/case/tool/status audits before checking or submitting this item.
    [Frozen authority and implementation](PLAN.md#phase-l--g-e4-implementation).
  - **Completed (2026-10-30, darwin-arm64):** all 1,536 configurations and six
    owner controls pass. The immutable cohort passes **121 suites / 713 tests /
    all 285 selected ignored functions**, with zero failed, ignored or filtered
    functions. The final unfiltered ordinary gate passes **181 targets / 1,451
    tests**, with zero failures or filtered functions. Workspace formatting,
    both-crate all-target warnings-denied Clippy and source/case/tool/status
    audits pass. All 19,542 earlier observations and 12,043 old fixture files
    remain intact. The broader feature row stays open; full parity stays OFF.

- [x] **G-E5 — Maps for newly supported constructs and generated helpers.**
  - **Accepted (2026-11-06):** frozen source42/cache70 passes **177/177** complete
    oracle suites. The declaration/map suite passes **64/64** across **8,407**
    configurations, each built twice; exact coverage accepts **10,193 JavaScript
    maps** and **10,397 declaration maps**. Raw reports, observations and suite
    logs are retained in the durable cohort archive. All **21,084** earlier
    observations and **39,118** Main41 observations retain their identities and
    classifications in the matching source42 publication. Source/case/status,
    inventory, format, all-target Clippy and the final complete unfiltered
    workspace pass. K.11 passes API **28/28** and real-process **22/22**; original
    Native fingerprints and fixture authorities remain unchanged. The ordinary
    accessor map-on/off mismatch and unauthorized-constructor budget fixture
    are repaired with their strict regressions retained. Original failed runs
    and source41 evidence keep their original scope; full TypeScript parity stays OFF.
    [Final integration evidence](GE5_CURRENT_INTEGRATION_MEASUREMENT.json),
    [complete coverage](GE5_COHORT_COVERAGE.json),
    [durable artifact index](GE5_COHORT_ARTIFACTS.json) and
    [source41 history](history/ge5-main41/GE5_COHORT_ARTIFACTS.json).
  - **Scope:** every new class/async/generator/module/decorator lowering admitted
    by G-E1–G-E3 and the non-scalar declaration forms admitted by G-D2.
  - **Pass:** complete decoded token/name/mapped-unmapped transitions, metadata,
    UTF-16/CRLF/astral coordinates and repeated builds agree with pinned native maps.
    Re-run K.11's real-process positive/negative matrix: stale/substituted/corrupt
    maps cannot arm a breakpoint or disclose source, and helper frames never acquire
    user-source coordinates. Do not reopen already-complete metadata authorization.
  - **Frozen minimum (2026-10-30):** preserve all **3,950** previously admitted
    direct configurations from G-E1/G-E2/G-E3/G-D2, each with original source and
    a CRLF/astral coordinate variant. Add 12 function-signature/enum regressions
    across both modules and all four LF/CRLF/astral products, plus three additional
    publication profiles for each of the 137 suite-qualified source families:
    **8,407 configurations**, every artifact and two builds per implementation.
    Parent case/reference bytes are hash-bound; old fixtures are not rewritten.
  - **Comparison:** preserve complete diagnostics and artifact inventories, raw
    declaration bytes, every decoded segment in order (including same-position
    Native entries), names, metadata and mapped/unmapped transitions. Identical
    JavaScript requires complete decoded-map equality. Divergent lowerings require
    a complete ordered source-role/generated-gap contract; a missing role is a
    failure. A source-anchor inventory, sampled tokens, or an arbitrary snapshot
    of BlueTSC maps cannot establish parity. All coordinate/publication variants
    and repeated builds must pass; failed configurations remain in the matrix.
  - **Owner/gates:** retain six canonical-output/atomicity/work/cache/helper/process
    controls, re-run the unchanged eleven-row K.11 real-process matrix and its
    stale/substituted/corrupt-map negatives, and preserve all 21,084 old observations
    and prior fixture bytes. Complete the immutable cohort, unfiltered ordinary
    gate, format/Clippy and source/case/tool/status audits before checking or
    submitting this item. [Frozen record](PLAN.md#phase-l--g-e5-implementation).
  - **Native defect correction (2026-10-30):** all **572 map occurrences**
    (452 configurations / 138 distinct raw maps / 14 families) have one verified
    System execute-block root cause. The independent local TypeScript 5.9.3
    patch passes all 8,407 configurations and 20,590 maps twice, changing only
    the generated-brace EOF/EOF+1 segments. BlueTSC passes all 452 corresponding
    configurations and 572 maps twice, including strict bounds and unmapped
    execute braces. Stock references remain unchanged; upstream is not submitted.
    [Complete 572-row ledger](GE5_NATIVE_MAP_DEFECTS.md) and
    [per-map measurements](GE5_NATIVE_MAP_MEASUREMENT.json). This resolves the
    identified Native coordinate defects; final G-E5 acceptance is recorded above.
  - **Evidence boundaries:** historical v20 full-family claims for fourteen
    families were withdrawn after the complete endpoint audit; its 1,389 failing
    checks are retained in [the class endpoint audit](GE5_CLASS_ENDPOINT_AUDIT.json).
    The separate metadata audit found 424 missing provider enum endpoints;
    [the correction](GE5_METADATA_ENUM_ENDPOINT_AUDIT.md) and
    [compiler-bound repaired replay](GE5_METADATA_INTEGRATION_MEASUREMENT.json)
    retain their original scope. Earlier standalone and shared results cannot
    substitute for the current complete same-compiler run.
  - **Historical detail:** compiler-specific progress and worktree deliveries
    remain in [PLAN](PLAN.md#phase-l--g-e5-implementation),
    [Main32](GE5_MAIN32_SHARED_MEASUREMENT.json),
    [Main33](GE5_MAIN33_SHARED_MEASUREMENT.json),
    [Main34](GE5_MAIN34_SHARED_MEASUREMENT.json), and
    [the family inventory](GE5_FAMILY_EVIDENCE_INVENTORY.json).
    Failed [Main35](GE5_MAIN35_FAILED_MEASUREMENT.json),
    [Main36](GE5_MAIN36_FAILED_MEASUREMENT.json),
    [Main37](GE5_MAIN37_FAILED_MEASUREMENT.json) and
    [Main38](GE5_MAIN38_FAILED_MEASUREMENT.json) measurements are preserved.
    Their partial results do not establish integrated acceptance.
  - **Completion boundary:** the complete 177-suite measurement, exact JS coverage,
    durable evidence publication and final unfiltered workspace gates are verified.
    Submit this completed item together with its implementation and measurement data.
    No additional TODO identifier or platform requirement is introduced.

- [x] **G-C3 — Remaining JSX provider and decorator-type compositions.**
  - **Accepted (2026-11-06):** all **220/220** direct comparisons pass within the
    complete 296-row physical replay. All **76 owner controls**, **12** actual
    Native runtime failures and **148** earlier K.10 configurations / **995**
    fixture files retain their scope and bytes. The complete **179/179-suite**
    Native catalog passes **1,024** functions with zero failures, skips or filters.
    Independent consumers match **118** declaration graphs / **208** declarations
    per implementation (**236** programs): **108** accept and **10** retain the
    same Native TS7016 owner-publication diagnostics. All **38** public boundary
    tests and the complete unfiltered workspace pass (**7,824** tests;
    **386** existing opt-in ignored tests, with the pinned Native
    catalog run separately). Both map-suite runs compare **8,407** configurations
    twice with zero differences. Format, warnings-denied all-target Clippy,
    tool/accounting/source/case/status checks and durable artifact verification pass.
    All **39,118** old identities/classifications remain; **308** new scoped cases
    produce **39,426** total. Original code-regression and sandbox failures retain
    their actual outcomes in the archived history; full TypeScript parity stays OFF.
    [Final integration evidence](GC3_CURRENT_INTEGRATION_MEASUREMENT.json),
    [consumer evidence](GC3_CONSUMER_MEASUREMENT.json) and
    [durable cohort index](GC3_COHORT_ARTIFACTS.json).
  - **Frozen matrix (2026-11-06):** [296 new configurations](GC3_FROZEN_MATRIX.json)
    across 86 source forms, plus all **148** existing K.10 configurations and
    **995** unchanged fixture files. All original **212** draft observations are
    retained. **220** new rows are direct comparisons; **76** retain existing
    host-library / project-declaration runtime policy as separate owner controls,
    never Native agreements. Same-source ES2022 decorator rows and explicitly
    authored runtime JSX rows expose every blocked named semantic composition
    without granting runtime authority to declarations or host libraries.
    Explicit base `Decorators` spelling remains a direct implementation requirement.
    The Native reference records **140** accepts / **156** rejects and **12**
    actual runtime failures; preserve/native JSX remains syntax-only. All direct
    rows must pass the complete diagnostics, raw declarations and physical-output
    comparisons below. [Initial raw evidence](GC3_BASELINE_ARTIFACTS.json).
  - **Scope:** JSX overload/union/recursive props, conditional/re-exported and missing
    runtime providers, imported/global declaration contexts, preserve/native JSX;
    decorator replacement/context inference, abstract/generic constraints and
    customized Symbol.metadata/accessor receiver types.
  - **Pass:** verdicts, primary/related diagnostics, inferred declarations and
    execution of emitted modes agree with the frozen fixtures; preserved JSX is
    validated as preserved syntax, not falsely reported as directly executable.
    Provider loading, metadata typing and helper use acquire no additional authority.

### Completed — bounded Windows x86_64 platform evidence

- [x] **G-M3 — Windows x86_64 platform/symlink parity evidence.**
  - **Frozen minimum:** execute the original 24 test functions and the unchanged
    repointed-symlink oracle row on the requested Windows profile, with original
    assertions, owner confinement, cache invalidation and no unexplained skips.
    Require fresh Windows 82 core oracles and the complete local ordinary/Native,
    workspace, format, warnings-denied Clippy and source/case/status gates.
  - **Accepted (2026-11-09):** Windows 25/25 named cases and 82/82 core oracles;
    all 14,598 source files match their expected hashes. Local named 25/25,
    complete 179 Native suites / 1,027 functions with zero failures,
    ignored or filtered tests, and the complete workspace
    (7,827 passed, 0 unresolved failures, 386 existing ignored) pass.
    The initial workspace invocation retains its one tool-path setup failure;
    the unchanged complete target passes after correcting the oracle path,
    with identical source, test binary and compiler hashes.
    Formatting, workspace all-target Clippy and publication checks pass.
  - **Scope:** test-only `windows-symlink-tests` uses existing file/directory
    link capability on the pb60g VM. Production code, original oracle bytes and
    owner authority stay unchanged. Default Windows exclusions remain explicitly
    reported; broader unmeasured platform combinations remain open.
  - **Evidence:** [frozen 25-case matrix](GM3_FROZEN_MATRIX.json),
    [accepted measurement](GM3_CURRENT_INTEGRATION_MEASUREMENT.json) and
    [verified raw evidence index](GM3_COHORT_ARTIFACTS.json). Both initial Windows
    fixture failures, measurement setup failures and same-source continuations
    are retained. No skipped case is credited as parity.

## Measurement and policy notes

- The 22 G-* areas overlap; their evidence tables are not independent case counts.
  The current three open observations retain deliberate observed-input rebuild
  differences. Three refused observations retain two language/diagnostic subset
  boundaries and one reference-owner escape. Cross-suite observations can share
  source selectors; these counts are not independent missing features.
- Preserve scoped controls and historical source bindings. A future complete
  measurement must regenerate the case/suite artifacts and update the current
  numeric block in CLAUDE.md, PLAN.md, TODO.md and COMPATIBILITY_INVENTORY.md
  together. This worklist edit changes no measurement, source grant or parity claim.
- Feature-row labels are reviewed policy, not inferred from a green harness:
  `account_inventory_rows.py` currently closes only G-C2. Change a row's policy
  only with evidence for its declared scope; never relabel it to manufacture parity.
- From the repository root, check documentation/accounting without recording
  a fresh runtime result:

```sh
python3 development/browser_core/phase-18-bluets/tools/check_compatibility_status.py --check
python3 development/browser_core/phase-18-bluets/tools/account_inventory_rows.py --check
cargo run -p blueice-bluets --example compatibility_inventory -- --check
cargo run -p blueice-bluets --example compatibility_inventory -- --check-cases
```
