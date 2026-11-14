# P0-G — Higher-order generic inference and retained callable identities

Status: accepted within the frozen P0-G contract. Source: `backend-tree:7be6eb17296278ce86fe4a5d6a5de1c40259f9bb`.
Complete current acceptance, independently verified artifacts and reproduction
instructions are recorded in [acceptance](HIGHER_ORDER_INFERENCE_ACCEPTANCE.md).
Only P0-G is complete; all 22 broader rows remain open and full parity is OFF.

Accepted baseline: commit `87d53ef80beb76c77dd804b9c14f3ed2dbb8a4c5`,
compiler/oracle source `backend-tree:57ea386c3d0864ab228846f55cf39d3a3d3f5e66`.
The measured actual workspace compiler has SHA-256
`b826dc358758c6625cfc3789a947de92646cb17818422d075d4e3077c74f57e1`.

## Reproduced differences

[Discovery](HIGHER_ORDER_INFERENCE_DISCOVERY.json) compares 24 unchanged authored
programs against pinned TypeScript 5.9.3. All authored Native verdicts agree:
21 accepts and three rejects, with no unresolved calibration questions.
Complete verdicts, primary/related diagnostics with UTF-16 positions, and actual
declaration inventories/bytes yield four complete agreements and 20 differences:
13 incorrect rejections, five exact-declaration differences and two diagnostic
differences. There are no incorrect acceptances in this discovery.

For example, composing generic `list<T>(value: T): T[]` and
`box<T>(value: T): { value: T }` must retain a generic result:
`<T>(value: T) => { value: T[] }`. The baseline emits
`(value: unknown) => unknown`, then rejects an ordinary string-array demand.
Imported and re-exported versions reproduce the same loss. Merely suppressing
the diagnostic or accepting `unknown` does not repair the inferred signature.

| Required observations | Reproduced behavior |
| --- | --- |
| `compose-array-box`, `compose-array-box-call`, `compose-array-box-reject`, `compose-arrow-generics`, `compose-three-stages`, `compose-constrained-first`, `compose-concrete-first` | Generic argument evidence is lost, including concrete specialization after another generic function. |
| `lift-generic-identity`, `lift-generic-identity-call`, `lift-generic-identity-reject`, `annotated-generic-callable`, `propagated-default-parameter` | Returned functions lose their own parameter, its constraint/default or its relation to the return type. |
| `outer-parameter-shadowing` | The returned generic parameter must be independent of the outer spelling, with Native's exact declaration renaming. |
| `imported-first`, `imported-second`, `imported-all`, `reexported-generics` | Scoped signature identities must survive module loading and re-export. |
| `preserve-generic-function-control` | Semantic generic copying works, but Native retains `typeof identity` and its private declaration; an inline equivalent is insufficient. |
| `generic-constructor-factory` | Constructor signature parameters must become bound result parameters, without leaking an unbound class parameter. |
| `overloaded-result-control` | Non-propagating overload results still require Native's exact inferred `unknown` presentation. |

The four complete agreements are mandatory controls: `compose-concrete-second`,
`lift-contextual-result`, `object-result-no-propagation-control` and
`object-result-rejection-control`. Returning an object with a callable member
does not automatically grant that member a propagated generic parameter.
Concrete context and incompatible demands must keep their calibrated behavior.

The [evidence archive](HIGHER_ORDER_INFERENCE_DISCOVERY_EVIDENCE.tar.gz) retains
all original source strings, configurations, raw build output, every physical
Native/BlueTSC product, actual baseline compiler and probe/audit scripts. Its
[member manifest](HIGHER_ORDER_INFERENCE_DISCOVERY_MEMBERS.json) independently
verifies all 262 members. Accepted P0-F artifacts retain the complete pinned
oracle packages and their original licenses.

The initial recorder mislabeled the whole build inventory as declarations,
including BlueTSC's manifest and JavaScript. Its original 23-difference outcome
is preserved, receives no language-defect credit and is explicitly corrected by
the independent 20-difference declaration audit. That audit checks every physical
source and output against the raw observation before selecting declaration files;
it changes no diagnostic, declaration byte or authored input. Build-only products
remain separate evidence. This discovery earns zero inventory/completion credit.

## Frozen contract and test-first baseline

The fixture's `FROZEN.json` fixes 48 forms × eight profiles before production
changes: 384 comparisons, 352 accepts /32 rejects, 424 physical declaration files,
704 isolated positive/negative consumers, 424 selected-edition parses and 48
executions. All 24 original programs and module files are unchanged. The Native
reference has zero authoring questions; an independent replay reproduces every
observation and all twelve separate boundary controls without modifying the
reference. The pinned implementation and three explicit-ES5 libraries are hashed.

Six new executable forms use actual higher-order implementations, including
imported modules. CommonJS/ESNext execution uses copies of the actual emitted
module graph. Four additional runtime-harness checks pass, including deliberate
execution failure in both formats; they add no inventory credit.

The [complete baseline](HIGHER_ORDER_INFERENCE_BASELINE.json) retains 48 complete
agreements and 336 differences: 116 incorrect rejections, 204 declaration
differences and sixteen diagnostic differences. No Native-rejected input
published artifacts. This is a verdict/diagnostic/declaration baseline;
baseline consumer/syntax/runtime behavior is not final acceptance evidence.

The [test-first red run](HIGHER_ORDER_INFERENCE_RED.json) compiles the public
Rust regression against unchanged production source and reproduces every one of
the 384 independent baseline observations. The frozen-contract test passes;
the public behavior and policy tests fail, with one external-oracle test ignored
in this ordinary red run. Fresh Native replay is recorded separately. The
actual newly built CLI has SHA-256
`fed688b54009ae9fd19baf60afed05d53e6079c41a27e4e2fd87b582bacba04b`;
its compiler bytes and observed behavior are retained, rather than assumed to
match the accepted workspace executable merely from source identity.

The [baseline evidence](HIGHER_ORDER_INFERENCE_BASELINE_EVIDENCE.tar.gz) and
[member manifest](HIGHER_ORDER_INFERENCE_BASELINE_MEMBERS.json) independently
verify 13,232 archived files: authoring/calibration/replay sources, Native
producers and consumers, all baseline/red projects and outputs, original
compilers, the public Rust test and raw logs. Every earlier 42,716-case inventory
row is also retained for the eventual preservation check. These results add no
completion or compatibility-inventory credit.

## Implementation design

The baseline collector handles ordinary/ordinary and generic/generic function
pairs, but lacks the ordinary-parameter/generic-argument inference path.
Its result carries substitutions and a rejected constraint, without a way to
bind introduced parameters onto a returned signature. Filling those slots with
`unknown` loses relationships before declaration emission or a subsequent call.

Introduce a bounded higher-order inference module alongside the existing
collector. Instantiate incoming generic signatures against accumulated argument
evidence, then carry eligible independent parameters onto the returned callable.
Later arguments must use evidence already supplied by earlier arguments;
concrete context must specialize parameters instead of blindly propagating them.
Check candidate-slot overlap before introducing fresh parameters: an unrelated
fixed slot must not disable all propagation, while an already inferred slot must
not be replaced by a new independent binder. The concrete-second and contextual
controls distinguish these behaviors from unconditional generic wrapping.
Preserve constraints, defaults, scoped identities and constructor/call polarity.
Keep object/overloaded return boundaries and overload candidate isolation under
the Native-calibrated rules. Do not recognize special function spellings.

The result-instantiation boundary must retain introduced binders through direct
calls, contextual calls and returned-call chains. Preserve private/imported
callable identity where Native emits a type query, and allocate presentation
names without capturing an outer parameter. Exact inferred overload results
must normalize `unknown` according to the calibrated observations. These are
separate required paths; fixing only the false rejections leaves five declaration
and two diagnostic failures unresolved.

All traversal and candidate work uses the existing expansion budget. Keep owner,
ambient and runtime authority separate, and change inference/cache identities
when semantics change. New modules and existing production files stay below
1500 lines. Shared work primarily covers G-T2/G-T4/G-T6/G-T8; imported identities
and declarations also exercise G-T10/G-D1, with one constructor composition in
G-T9. Those intersections do not close broad rows.

## Development history — earlier focused results

All candidates retain their source snapshots, available compiler/test
executables, raw logs and all 384 public observations under
`target/higher-order-inference-contract-20261114/`. The difference counts fall
from the frozen baseline's 336 to 184, 80, 24, sixteen and finally zero.
Candidate 5 repairs all 336 baseline differences without regressing any of the
48 baseline agreements. Its ordinary Rust run has three passing tests, zero
failures and one ignored external-oracle test. Native-rejected inputs publish
no artifacts. The actual candidate compiler SHA-256 is
`78f43ee5498d5e45781131d48745508accf98dc1e819b1a9bfad1eac430d70f7`.

The fresh `downstream-1` run reproduces the complete Native reference: 384
comparisons, 424 declarations, 704 consumers, 424 parses, 48 executions and
twelve separate boundary controls. BlueTSC's downstream run fails on the first
ES5 parse: `function box(value) { return { value }; }` retains shorthand syntax.
An additional read-only scan of the candidate's 424 emitted JavaScript files
finds 68 ES5 parse failures, including an unlowered generic object method.
The failed downstream receipt and physical outputs remain preserved.

Candidate 8 repairs the ES5 shorthand and generic-method paths. Parser-retained
property keys survive lexical value renaming; CommonJS reference rewriting
consumes an existing key expansion rather than duplicating it. Supplementary
checks also exposed and repaired labelled braced statements absorbing later
declarations and confusing block references with object shorthand. All eight
supplementary CommonJS/ESNext observations now match Native declarations,
selected-edition syntax and actual execution, with zero inventory credit.
The old eight-failure run and intermediate six-agreement/two-failure run remain
retained separately. The frozen 48-form contract is unchanged.

The actual candidate 8 compiler has SHA-256
`ac5ac56a827900237777cd92c8d06664541be0cde9e35a42ec2edb784b65f047`.
The fresh `downstream-3` run passes the complete Native and BlueTSC matrices:
384 comparisons, 424 declarations, 704 isolated consumers, 424 parses and
48 executions per compiler. All 281 library tests and seven complete affected
cohorts pass, together with formatting, warnings-denied workspace/all-target
Clippy, 323 Node tests (three explicit skips) and thirteen Python tests.
The initial Python failure caused by stale generated refusal-line references
is preserved; refreshing those document references changes no source or count.

The first full acceptance ran from frozen source
`backend-tree:39664c49cb745e1dd73b9ee4ffa4eefc5c52fd25` under
`target/higher-order-inference-acceptance-20261114-r1/`. It fails the unchanged
`NonNullable/Awaited` inferred recursive-call control: TS2589 is correct, but
the diagnostic covers the six-character argument range instead of Native's
thirteen-character full call. That entire attempt remains failed. Its owned
Windows test process tree was cancelled, all six recorded processes were
independently verified absent, and all 14,818 guest source files plus actual
compiler and partial outputs were archived. No Windows acceptance is claimed
for that interrupted attempt.

Candidate 9 separates callable origin from the deferred diagnostic origin.
The unchanged six-case demand regression now passes, alongside all 384 primary
comparisons and the complete downstream matrix. Its actual compiler SHA-256 is
`4cbebdac4b8da78e78146b38d3878b1edb9ea0d3b80adfc13f4241c193a64678`.
Fresh preflight passes all 281 library tests and eight complete affected
cohorts, including `NonNullable/Awaited`, plus formatting, warnings-denied
workspace/all-target Clippy, 323 Node passes (three explicit skips) and thirteen
Python tests. All 580 production Rust/declaration files across BlueTS and its
BlueJS adapter remain below 1500 lines (maximum 1477).

The second complete acceptance ran under
`target/higher-order-inference-acceptance-20261114-r2/`, from source
`backend-tree:14d3fff9226074576f7ab1e05277e7abbe4d8649`. Its supervised Mac and
Windows processes are separate; only one Cargo pipeline runs on each host.
The prior Windows interruption archive has been received and independently
checked: 24,349 archive members, including all 14,818 source files and the
actual x86_64 compiler. Its acceptance credit remains zero. Native catalog,
workspace, both actual Mac profiles, Windows and publication verification must
all finish before checking P0-G or committing.

That second Windows attempt passed all four P0-G tests, then failed two G-T10
external-oracle tests because its virtual host compared Windows paths without
TypeScript's separator/case canonicalization. All 14,818 old source files,
actual compiler and partial results are retained and independently verified in
a 26,915-member failure archive. The owned Mac test tree was stopped before
changing its frozen oracle tooling; those partial results earn zero acceptance
credit. The failed generated suite catalog is retained separately.

`record_gt10_import_types.cjs` now uses the existing cross-platform virtual host.
Only its virtual project prefix is adapted as path metadata; Native source,
gold, declaration bytes and diagnostic spans are unchanged. Windows independently
reproduces all 133 Native observations, and the complete original nine-test API
cohort passes (no ignored/filtered tests), including the 58 accepted graphs,
consumers, syntax and executions. An earlier isolated probe incorrectly sent
Native-only `DOM` options to the CLI; that probe failure remains recorded and
receives no acceptance credit. The successful probe uses the original API test
binary and actual compiler, and restores the old guest oracle afterward.

Fresh `regressions-3` passes the library, eight affected cohorts, format,
workspace/all-target Clippy, Node and Python checks. The compiler backend is
unchanged at `backend-tree:14d3fff9226074576f7ab1e05277e7abbe4d8649`; the third
complete acceptance under `target/higher-order-inference-acceptance-20261114-r3/`
binds the repaired oracle tooling separately. It subsequently failed the
unchanged diagnostics and standard-library cohorts: unary `-value().toFixed(1)`
was incorrectly inferred as a string, and generator `.next(1)` lost its pinned
tuple diagnostic presentation. Newly routing all nested-call receivers through
the returned-call checker bypassed their existing member-call handling.

The owned Mac and Windows process trees were stopped and independently verified
terminal before further production edits. Mac source/actual compiler/logs are
retained; the interrupted Windows archive independently verifies 26,929 members,
including all 14,818 source files and its actual x86_64 compiler. Its SHA-256 is
`31109af8fbf25523dbd556cca5621dc36db4267c42b173a1b388a8d1b7100a5c`.
No part of that interrupted acceptance receives completion credit.

The repair limits new returned-call routing to direct `factory()(argument)`
calls; ordinary member calls retain their existing diagnostics. Prefix operators
retain the whole expression's type while their callable operands still check
arguments. Four public CLI prefix controls have permanent regression coverage
and zero additional compatibility-inventory credit. The earlier ad hoc
seven-case probe also retained an unrelated pre-existing script declaration
gap; its raw result is preserved. Separate module controls and the unchanged
original script cohorts are used for their respective complete checks.
The same seven module controls compare complete diagnostics, positions and
declaration bytes: the preserved old compiler has four differences, and
candidate 11 agrees on all seven. Its unchanged 384 primary observations,
twelve policy controls, 704 consumers, 424 selected-edition parses and 48
executions pass, together with all eight emission controls. These focused
results do not replace the required complete same-source regression acceptance.

The next preflight passed 281 library tests, nine complete affected caller
cohorts, the ordinary diagnostics/library/return regressions, formatting,
warnings-denied workspace/all-target Clippy and Node checks (323 passed,
three skipped). Its final Python step exposed a generated-catalog mismatch
left after the interrupted third acceptance: the catalog named that failed
source while cases and the accepted measurement still named the accepted
parent. The `r4` launcher stopped before launching either platform. Its failure,
generated files and actual compiler are preserved. The compiler and all oracle
source bytes were unchanged.

The accepted-parent catalog was restored and accounting/status metadata
regenerated. `regressions-5` explicitly reuses the six earlier successful
command/log pairs against the identical source manifest and freshly passes all
13 Python checks. The original failed Python outcome is unchanged. Fresh
Mac/Windows complete acceptance ran under
`target/higher-order-inference-acceptance-20261114-r5/`, with frozen compiler
source `backend-tree:20ed07888d4b564c37e93fcbc0f0e742e99d9c41`, all 618 oracle
tool files and every platform runtime driver bound separately. It requires
191 Native suites /1086 functions, the complete workspace and both Mac map
profiles, plus Windows82/25paths/25completecohorts. None is credited before its
terminal result and independent final artifact audit.
The final Windows test run passes82 core oracles,25 paths and25 complete
cohorts. The received59,920,074-byte archive has SHA-256
`4b17a0a3488d053ccb0238779cff21357b1974a762ce625870ab4a671d8a9bbb`.
Its original artifact verifier then expected eight G-M2 functions, although
`state_and_parent_symlinks_cannot_publish_outside_owner` is guarded by
`#[cfg(unix)]`. A read-only `--list` of the same hashed Windows test executable
proves all seven actual functions ran and passed, with zero ignored/filtered
tests. That verifier failure and source remain unchanged.

Separately named `verify_windows_r2.py` rechecks the unchanged archive with that
platform-correct count and independently verifies all14,818 selected source
files, actual x86_64 compiler and complete products. The correction has zero
test reruns and changes no compiler/oracle source or test output. Original
Windows supervisor/launcher failures remain visible; final source-bound
acceptance requires the successful correction plus all complete Mac gates.
That intermediate result did not complete P0-G; final acceptance is recorded above.

The `r5` Mac run finished all 191 Native suites /1086 functions successfully,
with zero failed, ignored or filtered functions. Its unchanged original
24-program Native replay also passed. The next mandatory gate then failed:
`defu` cold compilation rejects the recursive `_defu(value, ...)` call in
`src/defu.ts:36` with TS2345 (`unknown` is not assignable to `T`), although the
pinned Native baseline accepts the unchanged project. The Mac supervisor,
launcher and both delivery continuations are terminal failures. Workspace,
its second map profile, remaining projects and publication did not run; the
Native result alone does not satisfy minimum acceptance.

`target/higher-order-inference-contract-20261114/defu-cold-parent-comparison-1/`
replays the identical cold project with both retained actual compilers. The
accepted-parent compiler `b826dc358758c6625cfc3789a947de92646cb17818422d075d4e3077c74f57e1`
accepts and publishes ten artifacts; the `r5` compiler rejects and publishes
none. This confirms a new regression, with zero replacement acceptance credit.
`defu-localization-1/` retains all reduction programs, diagnostics and results:
renaming only `_defu`'s generic `T` to `Source`, or explicitly annotating the
indexed element as `any`, removes the erroneous rejection. Removing the
merger-call block or nullish guard does not. These observations narrow the
investigation to generic ownership/inference interactions; they are not a
production repair or a justification for changing the frozen upstream fixture.
Eight simpler recursive and same-name controls in `recursive-call-red-2/`
agree for both parent and current compilers, so those alone do not reproduce
the defect. The rejected source-snapshot probe and the reduction with an
invalid array-spread branch carry zero credit. Preserve every `r5` source,
compiler, runtime driver, log and outcome unchanged before any fresh repair
attempt. The task remains active; these partial results do not permit a
completion checkbox, commit or push.

Before the next production repair, `r5-project-failure-preservation-1/`
independently verifies a 34,382,106-byte archive containing all 14,734 frozen
source/build-control files, SHA-256
`e800343edbd77612e0fba49172757263298ce43895ffcb8f8dcf7ac404a6f52b`.
It retains the actual failed compiler and original driver/failure/map results.

The reduced regression involves a closed generic callback whose own `T` and
dependent `K extends keyof T` are unrelated to the recursive function's `T`.
Dependency discovery previously resolved those callback-local names through
the caller's lexical aliases, reopening an outer inference candidate. The
repair shields local names and scoped identities throughout the callback's
constraints, defaults, parameters and result while retaining genuine outer
captures. Sixteen supplementary ES5/ES2022 observations are frozen separately
before repair, with fourteen accepts and two rejects. Their initial complete
comparison has four agreements and twelve differences. The public regression
test reproduces the failure before production changes.

Candidate 14 passes all sixteen complete observations, including exact
diagnostic messages/locations and declaration bytes. Two accompanying repairs
explain an incompatible generic return under equivalent alpha-renamed binders
and emit an empty object generic default as `{}`. Inference/diagnostic/
declaration cache versions are advanced. The earlier build failure and the
intermediate candidate's two empty-object declaration differences remain
preserved. Supplementary controls add zero inventory credit.

The candidate also passes all original 384 comparisons, twelve policy controls,
704 consumers, 424 selected-edition parses and 48 executions per compiler.
The full unchanged defu audit, mitt, reproduction and declaration interchange
checks pass against actual compiler SHA-256
`31e3323c3678a37562f6e2c78c0148f6972a8f14ec93dc8c0aa73c6982dd1077`
and source `backend-tree:c746a2387175ac4df07d8a4ec91e983fb5d5ecec`.
All seven fresh quality steps pass in `regressions-6`, including complete
affected cohorts, diagnostic/library controls, format, warnings-denied
workspace/all-target Clippy and Node/Python checks. Full `r6` acceptance started
at 2026-11-14 09:15 UTC from that same frozen source. It requires 191 Native suites /
1087 functions because of the added public regression test, plus the entire
workspace, both map profiles and fresh Windows acceptance. The Windows driver
now records every actual compiled cohort function name and requires exact
agreement with passed names; the Unix-only eighth G-M2 test remains excluded
by its original platform condition. The original `r5` driver and verifier are
unchanged. P0-G remains unchecked and uncommitted.

The new run freezes all 14,115 backend files, 618 oracle-tool files and seventeen
platform runtime drivers. Its actual Native-profile compiler is
`ef0fc66fedcf8d1fea028b1a9ae8a1350ce2bd1aec9fcb22f0b4f2d5bbc102e6`;
the earlier `31e332...` binary is preflight evidence only. Complete final
Native/workspace/Windows results, both process-bound map profiles, product
audits and publication verification are still pending. No earlier-source pass
replaces any of these gates.

The `r6` complete Native run subsequently fails two original G-T7 Promise
adoption controls: a callback returning `Promise<string>` incorrectly yields
`Promise<Promise<string>>`. Its actual compiler, failed G-T7 test executable,
all 14,736 frozen source/build-control files and original partial outcomes are
preserved in `r6-gt7-failure-preservation-1/`. Source archive SHA-256:
`86442683b08712af16e3799f20363e1ace256573e89783023dcd117670b4203a`.
Only the remaining owned Mac test subtree is stopped; the native and Mac
supervisors record terminal failure normally. Windows continues collecting
that unchanged source's evidence. No publication or completion follows a
failed Mac run.

Twelve independent read-only comparisons localize the regression: the previous
compiler agrees on all twelve, while `r6` differs on six. Library generic
parameters named `TResult1`/`TResult2` lose their enclosing owner when expanded
through a generic alias containing same-spelled method binders. Equivalent
`T`, `U` and `Item` controls agree. The original programs and full diagnostics/
declarations are retained before repair. A separate twelve-observation fixture
is frozen in an isolated worktree, preserving the original 384 and sixteen
supplementary controls unchanged.

Repair worktree: `/private/tmp/blueice-bluets-p0g-promise-20261114-r1`.
The main checkout retains all 14,733 frozen compiler/oracle bytes while Windows
verification is active. The worktree shares the existing Cargo target, with
only one local Cargo pipeline running. Its inference-version-16 repair binds
alias arguments to their enclosing candidate identities before nested alias
signatures can shadow their spelling. All twelve new complete comparisons pass
through the permanent public test; complete higher-order and G-T7 cohorts are
running. The repair is not integrated into the main checkout yet. P0-G remains
unchecked and uncommitted.

The isolated repair is now source `backend-tree:9f6e4f033200e519f62f3573685b6cbe04f873a3`.
Both complete cohorts pass: G-T7 seven functions and higher-order seven
functions, with zero failed, ignored or filtered functions. The unchanged
384-case matrix, twelve policy controls, full downstream checks, original
sixteen scope observations and twelve new observations all pass. Defu, mitt,
reproductions and declaration interchange also pass on the same actual binary.

The isolated quality run retains an initial missing-oracle-path failure and a
later workspace Clippy failure caused by the uninitialized Test262 submodule.
The existing main-checkout corpus at pinned commit
`72faf8ec1445c55149615e8b35187830783aba1a` is linked read-only into the worktree.
Compiler and oracle source bytes are unchanged. `promise-worktree-quality-3`
explicitly retains the four successful same-source library/caller/diagnostic/
format command-log pairs and freshly passes Clippy, Node and Python, completing
all seven required quality steps without rewriting either earlier failure.

`integrate_promise_and_launch_r7.py` observes the original `r6` Windows run and
requires its terminal source/archive verification before changing the main
checkout. It then verifies the entire old frozen source, integrates exactly
the five reviewed repair/test/fixture files, verifies every resulting source
byte against the quality manifest, restores accepted-parent accounting
coherence and launches `r7` exactly once. The prepared final boundary is
191 Native suites /1088 functions, full workspace, both 8407×2 map profiles,
and fresh Windows82/25paths/26 complete cohorts. G-T7 is the additional affected
Windows cohort. Sixteen plus twelve supplementary observations add zero direct
inventory credit. No commit or push occurs in that integration controller.

## Completion checklist and minimum acceptance

1. [x] **Freeze before implementation.** Preserve all 24 discovery source strings.
   Add 24 authored forms covering nested/multiple generic parameters, dependent
   constraints/defaults, rest/optional positions, generic function properties,
   constructor results, imported aliases, outer-name collisions, contextual
   specialization, overload isolation, NoInfer and non-propagating boundaries.
   Freeze 48 forms across ES5/ES2022 × CommonJS/ESNext × explicit
   strictFunctionTypes true/false: 384 primary comparisons, with strict null
   checking enabled. Resolve authoring errors and independently replay the
   pinned Native reference before production edits. Freeze exact accepts,
   rejects, declaration-file, consumer, syntax and execution totals then.
2. [x] **Repair the entire contract.** Require all 384 complete observations to
   agree, including every original difference and all controls. Replay the
   original 24 sources/options unchanged. Compare every primary/related
   diagnostic and UTF-16 location and every physical declaration byte; retain
   `typeof` references, private declarations and parameter spelling/renaming.
   No sorting, newline repair, erased generic fallback, missing counterpart or
   verdict-only credit is allowed. Rejected inputs publish no artifacts.
3. [x] **Validate declarations and execution.** Every accepted primary observation
   needs authored isolated positive and negative consumers that exercise its
   concrete and generic relationships. Parse all accepted JavaScript in its
   selected edition. Include six new executable forms across eight profiles
   (48 executions per actual compiler), with real higher-order implementations;
   the unchanged ambient discovery functions are not runtime evidence.
4. [x] **Keep policy boundaries distinct.** Freeze twelve separate controls: six
   expansion-work, two parser-depth, two ambient/owner and two runtime-authority
   checks. Configured exhaustion must yield ResourceLimit without successful
   output. Parameters must not escape their owner or install runtime bindings.
   These controls have no direct-agreement credit.
5. [x] **Complete same-source regression acceptance.** Preserve all 42,716 earlier
   suite-qualified IDs and comparison/status/scope/reason classes. Require the
   entire Native catalog, full local workspace, both actual Mac compiler
   profiles' complete 8407×2 map reports, defu/mitt/reproduction/interchange,
   format, warnings-denied workspace/all-target Clippy, Node/Python and
   accounting tools. Windows x86_64 requires 82 core oracles, 25 path cases and
   complete affected cohorts. Keep every production file below 1500 lines.
   Hosted CI and additional platforms remain supplementary.
6. [x] **Publish before delivery.** Preserve the baseline and every failed attempt,
   their sources/logs/actual compilers, and complete final products. Independently
   verify all archive members and source/measurement bindings. Only then check
   P0-G and commit its implementation and matching measurements together,
   followed by automatic push. All 22 broader rows remain open and full parity
   remains OFF unless separately proven by a closure contract.
