# P0-F — Conditional infer candidates and complete matching

Status: accepted within the frozen P0-F scope. See [complete acceptance](CONDITIONAL_INFERENCE_ACCEPTANCE.md); earlier checkpoints below retain their historical scope.
Discovery and expanded Native calibration are complete. Accepted baseline: commit
`d4694f775e8d3e1ef2b45bc438ddaca7b8ac080c`, compiler/oracle source
`backend-tree:02abdba6835124bb585bf13149d5b6dcaf072911`.

The expanded Native contract is frozen in
`backend/bluets/tests/fixtures/conditional_inference/FROZEN.json`: 384 primary
observations, 360 accepts /24 rejects, 368 physical declaration files,
720 isolated consumers, 368 selected-edition parses and 48 executions.
An independent Native replay reproduced the complete reference byte for byte.
All 24 original source strings are unchanged. The twelve separate controls
also have an independent Native calibration and no direct-agreement credit.

The initial helper-import failure and earlier profile-verdict calibration are
retained with their original inputs. Two discovery forms become rejected under
strictFunctionTypes false: the incompatible mixed-variance check and the aliased
candidate check. Their eight Native observations remain explicit negative
cases; no source or expectation was altered to match BlueTSC.

The [expanded baseline](CONDITIONAL_INFERENCE_BASELINE.json) measures the actual
accepted P0-E executable against every frozen observation: 144 complete
agreements and 240 differences across 384 comparisons. Sixteen Native-rejected
observations incorrectly publish BlueTSC artifacts. The
[baseline archive](CONDITIONAL_INFERENCE_BASELINE_EVIDENCE.tar.gz) preserves all
Native calibration attempts, the independent replay and every baseline project;
its [member manifest](CONDITIONAL_INFERENCE_BASELINE_MEMBERS.json) was checked
against the actual archived bytes. These are pre-repair measurements, not new
compatibility-inventory credit.

The [test-first Rust red run](CONDITIONAL_INFERENCE_RED.json) compiles the new
public CLI/API regression harness against unchanged production source. All 384
physical CLI observations exactly match the independent baseline, including the
same 240 differences. The frozen-contract test passes. The separate policy test
also finds the readonly tuple's normal-compilation failure before its parser
limit is applied. The red result is retained with the actual compiler, test
source, raw log and all observations; it is not a passing acceptance gate.

## Reproduced differences

The [original discovery](CONDITIONAL_INFERENCE_DISCOVERY.json) preserves all
24 authored programs, complete diagnostics with UTF-16 positions, physical
declarations, source/options hashes and both actual compiler identities. The
[raw archive](CONDITIONAL_INFERENCE_DISCOVERY_EVIDENCE.tar.gz) also retains the
authoring/probe tools and every Native/BlueTSC project and output. Its member
hashes were independently verified before this contract was written.

Pinned TypeScript 5.9.3 and its three explicit-ES5 libraries have zero authoring
errors. The accepted BlueTSC baseline has eight complete agreements and sixteen
complete differences: two incorrect acceptances, eight incorrect rejections and
six declaration differences. Discovery earns zero compatibility-inventory or
completion credit. An unchanged declaration annotation alone is insufficient:
the programs also demand concrete inferred results or rejection behavior.

| Shared cause | Required original discovery cases |
| --- | --- |
| Candidate direction, combination and completed-pattern validation | `contravariant-disjoint`, `contravariant-related-literals`, `contravariant-record-intersection`, `contravariant-reject-union`, `mixed-covariance-precedence`, `mixed-incompatible-rejection-branch`, `cross-file-contravariance`, `aliased-candidate-positions` |
| Missing function positions and constrained defaults | `function-zero-arguments`, `function-zero-arguments-reject-literal`, `function-optional-position`, `function-constrained-missing` |
| Readonly and optional/variadic tuple matching | `readonly-array`, `tuple-suffix-rest`, `tuple-optional-empty` |
| Both possible branches of an `any` conditional | `conditional-any` |

The eight original agreements are mandatory controls: covariant unions, double
contravariance, ordinary returns, final overload selection, tuple prefixes,
distributed `never`, non-distributive tuple wrappers and constrained covariance.
Do not drop a discovery difference or weaken its expected output to complete
this package.

## Implementation design

The accepted baseline combines every same-name `infer` observation with a union.
The implementation introduces explicit covariant/contravariant candidate collections,
track nested parameter polarity and retain the distinction between ordinary
function properties and bivariant method patterns. Covariant candidates take
priority when present; pure contravariant candidates combine as intersections.
Finish inference before validating the fully instantiated extends pattern.
Candidate collection alone does not establish that the conditional is true.
Use the existing `TypeField.method` provenance for method patterns. Preserve
named-alias parameter variance under the checking flags: blindly expanding a
named callback alias into an inline function loses the calibrated loose-flag
behavior. Nested parameter directions and declaration-site alias variance must
remain distinct inputs to candidate collection.

Align function positions using required/optional/rest obligations. A source
function may ignore caller arguments; an absent position contributes no evidence
and uses the infer constraint or `unknown`. Preserve explicit receiver handling,
final-signature selection, complete rest tuples and NoInfer barriers.

Match readonly containers without granting write compatibility. For tuples,
match fixed prefixes/suffixes and infer the remaining middle under the existing
work budget. Preserve optional absence and readonly direction. Keep genuinely
deferred generic checks deferred. Concrete `any` branches must retain Native's
possible true/false results; this is not an unresolved-name fallback.

Put candidate collection and container matching in separate modules; all
production files remain below 1500 lines. Every traversal uses the shared work
budget. Cache identity changes accompany semantic changes. Keep template-infer,
NoInfer, symbolic generic identities and imported alias behavior under regression
coverage. The implementation must be derived from these observable rules;
frozen Native results are never regenerated from BlueTSC output.

This is P0 shared work across generic inference/type operators/structural and
library compositions (G-T2/G-T3/G-T5/G-T6/G-T7/G-T8). Imported aliases and exact
declarations also exercise G-T10/G-D1. Those connections do not close broad rows.

## Completion checklist and minimum acceptance

1. **Freeze the complete expanded contract before production changes.** Preserve
   all original 24 source strings. Author 24 additional forms, including method
   versus function variance, nested/repeated inference, missing/optional/rest
   positions, imported aliases, readonly direction, fixed tuple suffixes and
   `any`/`never`/unknown controls. Use ES5/ES2022 × CommonJS/ESNext × explicit
   strictFunctionTypes true/false: 48 forms × eight profiles =384 primary
   observations. Keep strict null checking enabled. Freeze all Native diagnostics,
   exact declaration inventories and pinned implementation/library hashes after
   resolving authoring errors; retain every unsuccessful calibration attempt.
2. **Repair every reproduced difference and preserve the controls.** All 384
   complete observations must agree. Independently replay the unchanged original
   24-program discovery profile, including all sixteen prior differences and
   eight prior agreements. A missing counterpart, ignored case, verdict-only
   agreement or normalized declaration is a failure. Rejected inputs publish no
   artifacts.
3. **Validate public declarations and executable forms.** Every accepted
   inventory matches byte for byte. Each accepted primary observation has
   isolated authored positive/negative declaration consumers and a selected
   edition JavaScript parse. Include six executable authored forms across all
   eight profiles (48 executions per actual compiler). Freeze the exact resulting
   accepts/rejects/declaration/consumer/parse totals before production changes.
4. **Retain distinct policy boundaries.** Add twelve separately identified
   controls: six expansion-work limits, two parser-depth limits, two ambient
   owners and two runtime-authority refusals. Resource exhaustion must produce
   ResourceLimit with no successful output. These controls have no direct
   agreement credit. Opaque/default-deny host authority remains unchanged.
5. **Preserve earlier accounting and all same-source gates.** Retain all 42,320
   old suite-qualified IDs and their comparison/status/scope/reason classes.
   Audit every new assertion. Require the complete Native catalog, full local
   workspace, both complete 8407×2 map reports, defu/mitt/reproduction/interchange
   project gates, rustfmt, warnings-denied workspace/all-target Clippy and
   Node/Python/accounting tools. Windows x86_64 requires 82 core oracles,
   25 paths and the affected complete regression cohorts. Keep modules below
   1500 lines. The accepted local/Windows boundary remains unchanged; hosted CI
   and extra platform combinations are supplementary.
6. **Publish, verify, then deliver.** Preserve baseline and failed-development
   sources/logs, actual compilers and complete final products. Independently
   verify archive members and every source/measurement reference. Only after all
   requirements pass, check P0-F and commit implementation plus matching
   measurements together, then automatically push. All 22 broader rows remain
   open and full TypeScript parity remains OFF.

## Development checkpoints (not final acceptance)

The frozen primary matrix has a passing focused checkpoint: 384 exact CLI
observations and twelve separate policy controls. A separately frozen alias
identity supplement adds 24 comparisons (three programs across eight profiles),
24 exact declarations and 24 Native syntax checks, with zero inventory credit.
The supplement preserves the distinction between unused scalar-alias parameters,
record-alias argument identity and identity aliases. Its Native expectations
were frozen before repairing the reproduced alias regression; they are separate
from the unchanged primary `FROZEN.json` contract.

Permanent Native and BlueTSC record/verify adapters have also passed the complete
384-observation primary matrix and 24 alias comparisons on an earlier development
candidate, including the authored consumers, declaration bytes, parses and
executions. This result is bound to that candidate and does not establish final
acceptance for subsequent source changes.

Earlier regression preflights exposed generic-callable erasure and Awaited
failures. Those failed candidates and logs remain retained. The latest four
unchanged Awaited probes now pass with the original work limit: strict single,
strict nested and loose single infer `number`; the overloaded thenable infers
`string`. The complete affected-cohort preflight now passes all twelve stages:
336 tests, zero failures, ignored tests or filtered tests. It includes the
library, Awaited, P0-F, callable/receiver utilities, generic object methods,
NoInfer, string intrinsics, mapped inference, symbolic keys, JSX and generic
compositions. Its actual compiler SHA-256 is
`08d66bd57b732ddc13737264ef4343566a9d9b33aaa42e846c10e2541bfc972c`.
No failed candidate is counted as a passing gate, and these preflight results
do not close P0-F.

The first formal candidate, `backend-tree:e6c4831c72322340382f8e28cb3589a6a5639a75`,
passed formatting and warnings-denied workspace/all-target Clippy but failed
complete acceptance. Native G-T3 reproduced resource exhaustion in both original
`shared-well-known-symbol-identity` verdict controls; the accepted E executable
still produces the original exact declaration and TS2322 rejection. Windows
passed all four ordinary P0-F functions, including every primary observation and
alias control, but its Native replay omitted all library hashes because a raw
prefix comparison mixed Windows separators with TypeScript's source paths.
Those failed sources, actual compilers and raw results are retained separately.

The repair routes conditionals without infer parameters through one complete
assignability relation, while preserving the explicit `any` branch rules.
Inference-bearing patterns still collect candidates and validate the completed
pattern. Library collection now checks containment with host path operations
and hashes the actual loaded files. A new development preflight has passed the
complete G-T3 cohort under the original work limit; the remaining critical
cohorts and fresh final acceptance are pending. Neither failed candidate nor an
earlier passing checkpoint receives current-source acceptance credit.

Complete same-source Native, workspace, Windows, map/project and tooling gates,
artifact publication and independent verification remain required. The TODO
checkbox stays open until implementation and matching final measurements are
complete together.


## Final acceptance

The complete same-source Native/local/Windows, map, project and tooling gates
pass. The [measurement](CONDITIONAL_INFERENCE_MEASUREMENT.json) binds every
accepted result to actual compiler bytes and the final oracle-tool manifest.
Earlier failed or canceled attempts remain separate. Only P0-F is checked;
all 22 broader rows remain open and full TypeScript parity remains OFF.
