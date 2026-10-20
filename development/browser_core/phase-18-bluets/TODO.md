# Phase 18 — BlueTS / BlueTSC completion worklist

[← Plan](PLAN.md) · [compatibility inventory](COMPATIBILITY_INVENTORY.md) · [integration contract](INTEGRATION_CONTRACT.md) · [test interface](TEST_INTERFACE.md)

This file keeps the completed baseline concise and tracks the remaining work
against pinned TypeScript 5.9.3. Design and delivered evidence belong in
[PLAN.md](PLAN.md) and [PLAN_HISTORY.md](PLAN_HISTORY.md).

<!-- BEGIN CURRENT BLUETS STATUS -->

**Current BlueTS / BlueTSC compatibility status (2026-10-20).**
**Full TypeScript parity: OFF; do not advertise full parity.**

Pinned compiler: **TypeScript 5.9.3**. **N = 12,987 suite-qualified
case assertions**, not unique programs or a percentage of the TypeScript language.

| Current measurement | Count / scope |
| --- | --- |
| Direct agreement | 12,423/12,432 (99.9276%); 3 open, 6 refused observations |
| Separate controls | 537 native-reference + 18 BlueTS owner = 555; excluded from direct agreement |
| Case labels | 12,978 scoped closed (including controls), 3 open, 6 refused |
| Suite inventory | 107 suites / 257 ignored Rust functions; not a case denominator |
| Suite labels | 101 scoped closed, 1 open, 5 refused |
| Broader G-* feature rows | 23 total: 1 scoped closed, 22 open |
| Direct BlueJS runtime profile | Open; separate from the TypeScript comparison claim |
| Source refusal sites | 213 in 11 areas; condition-specific locations, not extra failing cases |

All 12,987 recorded case expectations pass; expected differences/refusals are not
agreements. The complete case measurement is **darwin-arm64**, not all-platform proof.
Accepted baseline runtime evidence (AC-K14.3; its recorded source): Windows
x86_64 VM **82/82** core oracles plus the complete local test gate. This is not
a new Windows run for the compiler/oracle source below. Hosted CI and coverage
are supplementary, not prerequisites. This consistency check does not rerun
those runtime gates.

Compiler/oracle source: `backend-tree:b6bb5836a062cb245d57b32b106438cf60b96d30`.
Input graph SHA-256: `f21643e02180f56e3ec0f8704fc8c895abae49e51445a1ca8edb7b3d1f2c222e`.
Versioned authorities: `COMPATIBILITY_CASES.json`, `COMPATIBILITY_SUITES.json`
and `COMPATIBILITY_MEASUREMENT.json` in `development/browser_core/phase-18-bluets/`.
Older dated measurements below retain their original scope; they are not this current summary.

<!-- END CURRENT BLUETS STATUS -->

**Current work (2026-10-20):** A–J and K.1–K.14 are the completed baseline,
not a full-parity claim. Phase L has 22 finite work packages: 21 implementation/
compatibility packages and deferred platform evidence G-M3. **G-C1, both
G-T1 stages, G-T3, G-T2 and G-T5 are complete for their scopes below; 17 packages
remain unchecked. Next: G-T6.**

G-C1 acceptance includes the source-bound 102-suite / 247-oracle measurement,
complete ordinary two-crate tests, rustfmt and warnings-denied Clippy. The
broader G-C1 inventory row remains open for unmeasured CLI/recovery coverage;
the finite checklist does not claim full parity. See the
[acceptance record](PLAN.md#phase-l--g-c1-implementation).

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
   sequentially on the shared target. Final/milestone validation also needs the
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

- [ ] **G-T6 — Structural compatibility across nested expression boundaries.**
  - **Scope:** computed/conditional/spread expressions at assignment, argument and
    return boundaries; nested generic callback variance and structural protocols.
  - **Pass:** freshness and variance agree with strictFunctionTypes on/off and
    exactOptionalPropertyTypes/noUncheckedIndexedAccess controls; native-accepted
    programs stay accepted and native-rejected programs are not hidden by opaque
    compatibility types.

- [ ] **G-T4 — Overload selection with generic/optional call chains.**
  - **Scope:** call/construct/method overloads combined with generic callbacks,
    variance, optional calls and non-null assertions; merged declaration ordering.
  - **Pass:** selected result types, no-match/implementation diagnostics and
    declarations agree for both signature-order controls; accepted calls execute
    identically and retain receiver/evaluation-order semantics.

- [ ] **G-T8 — Contextual return/getter inference and literal freshness.**
  - **Scope:** local/imported literal freshness, contextual object/union returns,
    getter/method bodies, async/generator results and recursive local functions.
  - **Pass:** inferred declarations and downstream consumer diagnostics agree;
    every fixture with a concrete native result retains that precision instead of
    an accidental `any`/`unknown` fallback. Recursive/resource failures stay bounded.

- [ ] **G-T7 — Expand the original standard-library API manifest.**
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

### P2 — modules, packages, classes, declarations and projects

- [ ] **G-T10 — Import type expressions and remaining module/type compositions.**
  - **Scope:** `import('specifier').Type`, `typeof import('specifier')`, qualified/
    generic projections, remaining merged declaration/import-elision forms and the
    recorded value import of a type-only namespace.
  - **Pass:** the frozen ESM/CommonJS type/value matrix matches native verdicts,
    declarations, import retention/elision and execution. Newly discovered type
    dependencies enter the authorized graph, budgets and fingerprints; a type
    expression never grants runtime or outside-root access.

- [ ] **G-M1 — Package/mapped JavaScript, JSDoc and resolver compositions.**
  - **Scope:** package and paths-mapped `allowJs/checkJs` inputs, JSDoc types,
    combined exports/typesVersions/conditions/rootDirs/typeRoots selection and
    missing-type-library diagnostics.
  - **Pass:** selected canonical files, diagnostics, emitted/runtime suffixes,
    declarations and execution agree for the frozen resolution matrix; changed/
    missing manifests invalidate correctly. Include owner escape, symlink, ordered
    fallback and TypeScript-sibling precedence controls before any outside read.

- [ ] **G-T9 — Remaining class representation and checking.**
  - **Scope:** unresolved getter bodies, generic merged interfaces/heritage,
    computed heritage, generator/auto-accessor members, and stateful class-expression
    keys involving await/yield. Setter-destructuring and compatible method/property
    false rejections fixed by G-C1 remain regression controls.
  - **Pass:** native verdicts, declarations and high-target execution agree for the
    named forms. Parameter-property/branch-super cases cover both field modes and
    retain the documented native-runtime defect boundary rather than publishing
    unsafe output. Decorator-specific and low-target lowering combinations belong
    to G-E3/G-E1 and are not counted twice as new class work.

- [ ] **G-D1 — Anonymous, contextual and cross-module declaration precision.**
  - **Scope:** anonymous object unions, imported/local freshness, const/contextual
    inference, overload combinations and merged declaration surfaces.
  - **Pass:** exact declarations and a separate pinned-TypeScript consumer agree
    for each named form, including inaccessible/unnameable external-type diagnostics;
    import retention and stable symbol identities survive repeated builds.

- [ ] **G-D2 — Non-scalar declaration maps and isolated-declaration checking.**
  - **Scope:** class, generic, namespace, inferred and merged declaration maps,
    plus their `isolatedDeclarations`/declaration-only output combinations.
  - **Pass:** complete decoded maps, names/source metadata, declarations and artifact
    inventories match native fixtures; unsupported scalar-only refusals are removed
    only for implemented forms. CLI output-root/symlink and corrupt-map bounds stay
    enforced; accepted declarations compile in the native consumer.

- [ ] **G-M2 — Native build continuation, state interchange and watch integration.**
  - **Scope:** default `--build` error continuation (distinct from
    `--stopBuildOnErrors`), pinned native `.tsbuildinfo` interchange, direct source-file
    CLI invocations, and imported/package watch edit/add/delete/burst sequences.
  - **Pass:** ordered build/skip events, exit codes, outputs and declarations agree;
    native/BlueTS state cross-use is demonstrated, not assumed from a filename.
    Foreign/unverifiable state rebuilds safely. Watch results equal clean builds,
    and bare-source options follow the frozen CLI matrix. Keep owner-current
    manifest/nearer-package/repointed-symlink invalidation and owner-escape refusal
    as explicit policy differences, not weakened success criteria.

### P3 — remaining emit, mapping and JSX/decorator combinations

- [ ] **G-E1 — Unsupported downlevel control-flow and class combinations.**
  - **Scope:** the recorded `opaque-generator-es5` graph; computed heritage/`super`,
    constructor spreads, nested binding patterns, async arrows/methods,
    template-contained suspension and combined suspension/exception paths.
  - **Pass:** every frozen supported target configuration matches native Node
    completion/stdout/stderr and declarations, and parses at its selected edition;
    include rejected shapes, iterator/finally completion and helper-collision controls.
    Remove the named ES5 refusal only after real execution agrees.

- [ ] **G-E2 — Node ESM import-equals and module/helper ABI combinations.**
  - **Scope:** ESM Node import-equals lowering, Node declaration inputs and the
    currently unmeasured module/target/importHelpers/noEmitHelpers combinations.
  - **Pass:** loader execution, live/cyclic bindings, helper imports/ABI, output
    suffixes, declarations and rejection diagnostics match the frozen configurations;
    helper source/version and cache identity remain bound to the artifact.

- [ ] **G-E3 — Literal/private-super decorators and imported metadata.**
  - **Scope:** literal decorated names, super in decorated private callables,
    static-initializer super writes/computed super keys, decorated class expressions,
    and imported/aliased legacy metadata types.
  - **Pass:** frozen standard/legacy × target × field-mode configurations agree on
    evaluation/replacement/initializer order, receivers, execution, declarations and
    diagnostics; imported metadata resolves through existing owner authorization.
    Preserve versioned helper identity, source provenance and precise remaining refusals.

- [ ] **G-E4 — Nested declaration and output-option interactions.**
  - **Scope:** nested/inferred declaration members with `stripInternal`,
    `isolatedDeclarations`, newline/BOM/comment options and class-field modes.
  - **Pass:** exact declarations, artifact lists/bytes where specified, and actual
    execution agree across the frozen option products. Strict-runtime helper
    fingerprints and publication bounds remain unchanged; declaration-map coverage
    itself is owned by G-D2/G-E5 rather than duplicated here.

- [ ] **G-E5 — Maps for newly supported constructs and generated helpers.**
  - **Scope:** every new class/async/generator/module/decorator lowering admitted
    by G-E1–G-E3 and the non-scalar declaration forms admitted by G-D2.
  - **Pass:** complete decoded token/name/mapped-unmapped transitions, metadata,
    UTF-16/CRLF/astral coordinates and repeated builds agree with pinned native maps.
    Re-run K.11's real-process positive/negative matrix: stale/substituted/corrupt
    maps cannot arm a breakpoint or disclose source, and helper frames never acquire
    user-source coordinates. Do not reopen already-complete metadata authorization.

- [ ] **G-C3 — Remaining JSX provider and decorator-type compositions.**
  - **Scope:** JSX overload/union/recursive props, conditional/re-exported and missing
    runtime providers, imported/global declaration contexts, preserve/native JSX;
    decorator replacement/context inference, abstract/generic constraints and
    customized Symbol.metadata/accessor receiver types.
  - **Pass:** verdicts, primary/related diagnostics, inferred declarations and
    execution of emitted modes agree with the frozen fixtures; preserved JSX is
    validated as preserved syntax, not falsely reported as directly executable.
    Provider loading, metadata typing and helper use acquire no additional authority.

### Deferred — broader platform evidence, not a new release gate

- [ ] **G-M3 — Optional platform/symlink parity evidence.**
  - **Status:** deferred unless the user requests a broader platform claim.
    Windows VM 82 core oracles plus complete local tests remain sufficient;
    neither Windows arm64 nor hosted all-leg success becomes mandatory here.
  - **Scope / pass if activated:** inventory the current named Unix-only symlink
    exclusions (currently 25), run the explicitly authorized applicable cases on
    the requested platform, and record exact revision, toolchain, selected names,
    results and skip reasons. A skipped test is not parity evidence; historical
    hosted runs do not prove a later source. Do not enable VM privileges or broaden
    filesystem authority merely to change a skip into a pass.

## Measurement and policy notes

- The 22 G-* areas overlap; their evidence tables are not independent case counts.
  The present six open observations are three missing diagnostics plus three
  deliberate rebuild-policy differences. Twelve refused observations include
  duplicate suite observations, language/diagnostic limits and an owner-policy
  refusal; they are not twelve unique missing language features.
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
