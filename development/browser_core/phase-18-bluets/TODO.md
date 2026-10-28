# Phase 18 — BlueTS / BlueTSC completion worklist

[← Plan](PLAN.md) · [compatibility inventory](COMPATIBILITY_INVENTORY.md) · [integration contract](INTEGRATION_CONTRACT.md) · [test interface](TEST_INTERFACE.md)

This file keeps the completed baseline concise and tracks the remaining work
against pinned TypeScript 5.9.3. Design and delivered evidence belong in
[PLAN.md](PLAN.md) and [PLAN_HISTORY.md](PLAN_HISTORY.md).

<!-- BEGIN CURRENT BLUETS STATUS -->

**Current BlueTS / BlueTSC compatibility status (2026-10-28).**
**Full TypeScript parity: OFF; do not advertise full parity.**

Pinned compiler: **TypeScript 5.9.3**. **N = 13,966 suite-qualified
case assertions**, not unique programs or a percentage of the TypeScript language.

| Current measurement | Count / scope |
| --- | --- |
| Direct agreement | 13,380/13,387 (99.9477%); 3 open, 4 refused observations |
| Separate controls | 537 native-reference + 42 BlueTS owner = 579; excluded from direct agreement |
| Case labels | 13,959 scoped closed (including controls), 3 open, 4 refused |
| Suite inventory | 113 suites / 269 ignored Rust functions; not a case denominator |
| Suite labels | 108 scoped closed, 1 open, 4 refused |
| Broader G-* feature rows | 23 total: 1 scoped closed, 22 open |
| Direct BlueJS runtime profile | Open; separate from the TypeScript comparison claim |
| Source refusal sites | 214 in 11 areas; condition-specific locations, not extra failing cases |

All 13,966 recorded case expectations pass; expected differences/refusals are not
agreements. The complete case measurement is **darwin-arm64**, not all-platform proof.
Accepted baseline runtime evidence (AC-K14.3; its recorded source): Windows
x86_64 VM **82/82** core oracles plus the complete local test gate. This is not
a new Windows run for the compiler/oracle source below. Hosted CI and coverage
are supplementary, not prerequisites. This consistency check does not rerun
those runtime gates.

Compiler/oracle source: `backend-tree:9223a6fc09845e5990303f2ce8babd52df673dce`.
Input graph SHA-256: `c138108a6968ac71c6b35d9396301e38f3e51b09789e3aa490e6a3e5f4e043f9`.
Versioned authorities: `COMPATIBILITY_CASES.json`, `COMPATIBILITY_SUITES.json`
and `COMPATIBILITY_MEASUREMENT.json` in `development/browser_core/phase-18-bluets/`.
Older dated measurements below retain their original scope; they are not this current summary.

<!-- END CURRENT BLUETS STATUS -->

**Current work (2026-10-28):** A–J and K.1–K.14 are the completed baseline,
not a full-parity claim. Phase L has 22 finite work packages: 21 implementation/
compatibility packages and deferred platform evidence G-M3. **G-C1, both
G-T1 stages, G-T3, G-T2, G-T5, G-T6, G-T4, G-T8, G-T7, G-T10 and G-M1 are complete for their scopes below;
11 packages remain unchecked. Next: G-T9.**

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
