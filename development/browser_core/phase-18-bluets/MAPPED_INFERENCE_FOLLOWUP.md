# P0-D — Reverse mapped inference: discovery and acceptance preparation

Status: completed within the frozen scope. Final source `backend-tree:bfee3b4ec32820c6c242798b6d2d90fdb6e00600` passes
[acceptance](MAPPED_INFERENCE_ACCEPTANCE.md), with complete
[measurements](MAPPED_INFERENCE_MEASUREMENT.json) and
[artifacts](MAPPED_INFERENCE_ARTIFACTS.json). The accepted-C baseline source is
`backend-tree:687190197095253ff614e93a035b4e424b5e725a`, commit
`c971f5f5c4dff02f04fa2b3199cf337b6cb71f5f`.
All 22 broader feature rows remain open; full TypeScript parity remains OFF.

The [discovery record](MAPPED_INFERENCE_DISCOVERY.json) preserves all 16 authored
programs, pinned TypeScript 5.9.3 implementation/library hashes, exact Native
diagnostics/declarations and the accepted BlueTSC compiler's complete observations.
Native expectations were frozen before BlueTSC replay. This ES5/CommonJS/strict
probe has eight complete agreements and eight differences. It is exploratory,
has no inventory credit and is not a percentage of language compatibility.

| Original discovery observation | Native result | Accepted-C baseline result |
| --- | --- | --- |
| Boxed array literal, direct or through aliases | `[string, number]` | `unknown` |
| Boxed tuple-constrained literal | `[string, number]` | `unknown[]` |
| Boxed readonly const tuple | `readonly ["Ada", 3]` | `readonly unknown[]` |
| Boxed readonly array variable | `readonly string[]` | `unknown` |
| Unconstrained identity mapped array literal | `[string, number]` | `(string \| number)[]` |
| Empty boxed tuple | `[]` | `unknown[]` |
| Boxed record with context-sensitive callbacks | Accepted; `{ name: string; count: number; }` | Rejected with 1134 at line 2, column 148 |

The eight agreement controls remain obligations: ordinary/nested/optional/readonly
boxed records, constrained identity tuples, optional identity records, a valid
explicit tuple and an invalid explicit tuple with its complete diagnostics.
The two alias/direct array observations are separate programs, not proof of two
independent implementation defects.

Source inspection identifies two shared inference boundaries:

1. [Reverse mapped collection](../../../backend/bluets/src/checker/inference/mapped.rs)
   reconstructs record fields. Its array/tuple shortcut only handles the exact
   identity body `T[K]`; wrapped elements have no reconstruction path. Readonly
   wrappers also require explicit shape preservation. Fix the semantic candidate
   path rather than replacing `unknown` in the declaration printer.
2. [Contextual array inference](../../../backend/bluets/src/checker/module/expressions/inference/arrays.rs)
   preserves positions for a concrete tuple context, but an unresolved
   homomorphic mapped context can reach ordinary array inference first. Native
   retains tuple positions even in the unconstrained identity example. Collection
   alone cannot recover positions already lost by this earlier widening.

Four independent calibration controls isolate the callback observation to a
separate binding-name boundary. `export const number: number = 3` receives 1134
in BlueTSC but is valid in Native. Renaming only the callback's local `number`
binding to `checked` makes its complete inference and declaration agree; empty
callback bodies and an ordinary top-level binding also agree. These four controls
are retained separately in the discovery record, with no added primary/inventory
credit. The original failing program remains an obligation.

The original [binding identifier consumption](../../../backend/bluets/src/parser/type_syntax.rs)
admitted only identifiers and the contextual words `of`/`type`, rejecting a valid
type-keyword binding before inference. The shared binding/value classification is
now extended through the calibrated contextual words. Actual reserved words stay
invalid; their statement recovery and complete diagnostics are separately tested.

The acceptance matrix was frozen before production changes: 101 authored entries
in four profiles (ES5/ES2022 × CommonJS/ESNext, strict) produce 404 complete Native
comparisons, 358 accepts and 46 rejects. Each actual compiler must supply 366 exact
declaration files, 716 declaration-only consumers, 366 selected-edition syntax
checks and eight executions. Twelve separate owner/runtime/work/parser controls
pass on the unchanged baseline. See `backend/bluets/tests/fixtures/mapped_inference/`
for inputs, references, consumers and hashes. The original 16 programs remain
unchanged. Empty/homogeneous/heterogeneous arrays, readonly/optional/rest/labeled
tuples, spreads, nested wrappers, local/imported aliases, modifiers, explicit type
arguments, contextual callbacks, flags and rejected element types are included.

The accepted-C actual workspace compiler (`ce7f338d40d759da3fbd69777405f41b707ff9b60ff2ddf0927c4deafa3b4fb8`)
replayed all 404 programs and agrees on 70, with all 334 differences preserved.
All 12 policy controls pass. This is the red baseline, not acceptance. The first
Rust authoring run also contained an incorrect ambient-module count assertion;
the corrected assertion requires both main and owner modules and changes no
Native expectation. Its failed raw log/source are retained separately.

Authoring measurements progressed from 70/404 to 246/404, 358/404, 368/404,
376/404 and 384/404 complete agreements. These are failed development candidates, not full
acceptance; raw observations and compiler snapshots remain in the design root.
They cannot supply acceptance credit to a later source. A CommonJS-validation
authoring build also failed on an argument type mismatch before tests started;
its compiler log is retained separately from the subsequent runnable candidate.

Four additional Native literal controls, frozen before their repair, distinguish
explicit const assertions, declared literal tuples and readonly const records
from ordinary const copies into mutable properties. They add no primary or
inventory credit. The first three initially required excessive widening to be
removed from slot reconstruction; the ordinary-copy control then exposed the
older mutable-property context path skipping the existing freshness check.
The fifth runnable candidate passes all four literal controls and all twelve
original policy controls. The 20 remaining primary differences are the four
reserved-binding programs in four profiles, plus the boxed runtime program in
four profiles. No Native expectation was changed and no package is checked.

The runtime failure intersects the existing assertion policy: an unknown or
readonly payload must not acquire an unrelated parameter's authority through a
cast. Five additional Native calibration programs are retained under
`target/mapped-inference-qualifier-calibration-20261113/`: a projection of the
same parameter, unknown and readonly sources, an unrelated parameter, and a
mixed readonly payload. Native accepts all five; the accepted C baseline refuses
all five. Four of those refusals are required owner controls and cannot be counted
as Native agreements. The same-parameter program exposes loss of projection-origin
information when an indexed generic constraint becomes `unknown`. The repair
needs a bounded proof of retained parameter identity/origin; a blanket exception
for generic casts would weaken the existing boundary.

The sixth runnable candidate passes all 404 primary observations, all four literal
controls, all twelve original policy controls and all five projection controls.
The projection implementation uses canonical parameter identity, source receiver
types where an index lost that identity, and bounded traversal of fresh sequences.
Known readonly payloads, unknown sources and unrelated parameters retain their
existing refusals. Recovery keeps invalid syntax excluded from the source graph:
only its queued references are checked against lexical value bindings for the
additional type-as-value diagnostic. Public parser failure still returns syntax
diagnostics, while the compiler adds recovery meaning in its own context.

This is focused authoring evidence, not package acceptance. Complete declaration
consumers, syntax/runtime, prior regressions and the full platform/project/map/
tooling gates remain required. No TODO item is checked and no D commit is made.

Four later lexical-identity controls exposed a false refusal through an
unannotated const alias, while keeping nested named/arrow parameter shadows and
unrelated aliases refused. The initial alias repair passed those nine combined
projection controls, but four mutation/escape calibrations then exposed three
incorrect owner grants. These failed authoring observations are retained. The
current proof traces the same binding identity only when no other runtime value
reference observes the const alias; writes, escapes, captures and alias chains
with an observed original invalidate the proof. The focused replay now passes
all thirteen projection controls. Ten are intentional owner refusals despite
Native acceptance; the three positive projections retain exact Native output.
Together with the four literal controls these are seventeen supplemental
observations with zero primary/inventory credit.

A separate typed-alias mutation exploration is rejected at its earlier generic
indexed initializer. That preexisting unmeasured initializer gap supplies no
evidence for mutation invalidation and no P0-D credit. The full primary matrix
must be repeated on the final observer-aware implementation.

The observer-aware authoring preflight completed all fourteen steps successfully
on 2026-11-13, without changing its 14,666 backend/tool file hashes. It passed
formatting, warnings-denied workspace/all-targets Clippy, the library, the full
new mapped suite (including 404 comparisons, 716 consumers, 366 syntax checks and
eight executions), G-T2/G-T3, the complete diagnostics matrix, string intrinsics,
callable/receiver utilities, NoInfer, NonNullable/Awaited and the JSX checker
matrix, including their selected Native tests. Its actual compiler SHA-256 is
`0edf01cbb878b3d95d4033da3587ebd148d89c339e69ee37a270804122aad06f`.
Raw logs, observations, source manifest and changed/new source snapshots remain
under `target/mapped-inference-design-20261113/mac-preflight-1/`. This is authoring
evidence; full frozen-source package acceptance remains outstanding.

The first frozen formal candidate (`backend-tree:f77685c665f667dba0b2061580983fe59fa13a28`)
was retired after the unchanged mitt project regressed with TS7006 at
`src/index.ts:108:12`. Defu passed, but no whole-candidate acceptance is credited.
All known Native/Windows test processes were terminated and their states retained
before source repair. Its complete 14,070-file backend archive is independently
hashed; the Windows retirement archive additionally preserves all 14,750 selected
source files, the actual x86_64 compiler and every partial observation. The full
source/member hashes were independently checked after download. Cancelled test
rows remain cancellations, not new independent compiler defect observations.

Ten initial minimal forms passed both old and retired compilers. Combined
generic-method reductions isolated a contextual-binding intersection: a `type`
parameter newly reaches ordinary expression inference, exposing unresolved
generic identities during initial flow construction. Separate ablations ruled
out mutable-property widening, mapped indexed materialization and the additional
`as`/`satisfies` value references. Restoring the old value-name inference hid the
failure, but would abandon the frozen binding contract. Those authoring sources,
actual compilers and complete observations remain in the callback review root.

Four independently calibrated callback controls were frozen before the repair,
with zero primary/inventory credit. A new public CLI regression first failed.
Two placements of a scoped-key identity comparison still failed: later function
checking could select `Map.get`, but initial flow already retained an unresolved
result. The repair now retains lexical parameter identities and canonical
constraints in flow's declared values, resolving captures at their declaration
scope. It publishes only unique parameter IDs into inference's definitions;
ordinary source names remain lexical. Completed scope substitution maps are
cached, and equal symbolic `keyof` parameters compare before constraint expansion.
The flow/structural fingerprints change accordingly. Shadowed binders and the
existing owner/runtime/resource controls remain obligations.

The updated authoring replay on `backend-tree:fbaa93313a5b5043db5cbc729b405212a9c3bf88`
passes all six mapped-suite functions, including the complete 404 primary rows,
716 consumers, 366 syntax parses, eight executions, all 12 policy controls and
21 supplemental observations. The new four Native controls are independently
reproduced with pinned implementation/library hashes. Unchanged mitt, defu,
prerequisite reproductions and interchange also pass; mitt's complete declaration
inventory is byte-identical. An initial project launcher used the wrong argument
order and failed before compilation; the corrected fresh project run is retained
separately. These results remain authoring evidence, not package acceptance.

A separate six-program generic-key exploration remains outside this package's
primary/inventory denominator. Two unconstrained returned-method programs still
emit `unknown[] | undefined` where Native emits `void[] | undefined`; the ordinary
name version already differs on accepted C, while accepted C rejects the
contextual-name version. Accepted C and the repaired source also both accept two
programs that Native rejects: keys from an unrelated generic and a shadowed
same-spelled generic are passed to another parameter's `keyof` callback. The two
ordinary map/direct-call result controls have matching declarations. All sources,
independent Native observations and before/after compiler results are preserved
under the callback review root. These are remaining broader inference/relation
gaps, not new candidate regressions, owner-boundary agreements or P0-D closure
credit. The earlier typed-index initializer exploration likewise remains open.

The second authoring preflight passed twelve steps but failed an existing
NonNullable/Awaited policy control: the composed `NonNullable<Awaited<Box<number
| null>>>` program exceeded the unchanged normal 256-expansion budget. Five other
functions in that full suite passed; JSX was not reached. No successful whole
preflight or package acceptance is credited. The raw test-interface profile also
reproduces the failure at 256 and 320 and succeeds at 384; its initial ready frame
is excluded from request/result association. The explicit-checking Rust control,
rather than that exploratory interface, is the acceptance authority.

Flow's registry had included type-alias/interface formal parameters, making them
appear as free inference parameters outside their instantiations. Runtime flow
now retains function/class binders; alias/interface parameters remain scoped to
ordinary type instantiation. The existing public control passes again at its
original normal budget, and all configured 0/1 refusals remain required. No
budget or frozen expected output was changed. A focused final preflight covers
formatting, workspace/all-targets Clippy, library tests, the full mapped suite,
the full NonNullable/Awaited suite and JSX. Complete frozen Native/local/Windows,
project, map and tooling acceptance remains mandatory on the final source.

That focused preflight completed all seven named steps successfully on
2026-11-13, retaining all 14,669 backend/tool hashes. The actual authoring compiler
SHA-256 is `6510aef608bceb16b8b7069909dc3518515e64e429fad6e3b62d3b746ad27415`.
It passed 281 library functions, all six mapped functions, all six existing
NonNullable/Awaited functions and all five JSX checker functions, including the
selected Native replays. Raw inputs, outputs, source overlays and logs are in
`target/mapped-inference-design-20261113/mac-preflight-3/`. This authoring pass
permits a new source freeze; it supplies no whole-package acceptance credit.

The next frozen candidate (`backend-tree:d2e2861487aacaaba5e449776b8b8ddc4ba3dcd5`)
was retired when the existing G-T5 composition suite exposed six assertion-chain
regressions. Runtime-expression recognition treated the `const` ending an erased
`as const` as unable to precede another assertion, so a following `satisfies` type
was left in the runtime token stream. This produced extra readonly/primitive
value diagnostics and lost expected rejection diagnostics. The complete G-T5
raw log and the candidate's exact backend snapshot are retained; all partial
Native/Windows work has no whole-candidate pass credit.

Assertion collection now retains the exact end position of its preceding erased
assertion. A contiguous `as`/`satisfies` continues that expression without making
the reserved word `const` an ordinary expression or weakening contextual binding
recognition. The module/export fingerprint changes. All six existing G-T5 suite
functions, including the complete Native comparisons and declaration-consumer/
syntax/runtime replay, pass on the repaired source. Frozen expectations remain
unchanged. The next focused preflight includes this full G-T5 suite before another
complete same-source Native/local/Windows acceptance attempt.

The implementation must preserve tuple labels/optional/rest/readonly facts and
literal widening rules; reconstruct candidates through the existing bounded
collector and substitutions; preserve defining-symbol, owner and resource-limit
boundaries; and never manufacture a successful `any`/`unknown` fallback when
inference work is exhausted. Native's legitimate `unknown` results remain valid.
Each new source module must retain its MPL header and stay at or below 1500 lines.

Minimum completion requires every frozen input's verdict, all primary/related
diagnostics and UTF-16 positions, byte-exact declarations, declaration-only
positive/negative consumers and applicable syntax/runtime checks. Retain owner
and budget controls separately. Then pass the complete same-source Native/local
catalogs, both actual compiler profiles' fresh 8407×2 map reports, project/tooling
regressions and Windows 82 core oracles plus paths and the new full cohort.
Publish implementation and corresponding measurements together, check only P0-D,
commit and push. Hosted CI and additional OS profiles are supplementary.

The repaired source completed all eight focused preflight steps on 2026-11-13:
formatting, warnings-denied workspace/all-target Clippy, build, 281 library
functions, six mapped-inference functions, six NonNullable/Awaited functions,
five JSX functions and six G-T5 functions. All 14,669 backend/tool hashes remained
unchanged. Authoring compiler SHA-256:
`ea86e562bbca83229e28d6555ef793c1ed508f14ba4aa7888c19c1c5887f8ec4`.
The first driver exited on `BrokenPipeError` while reporting the already-passed
NonNullable/Awaited result. Its failed receipt is preserved; the continuation
verified every earlier source/log hash and ran the two not-yet-started suites
without changing source or expectations. The complete receipt and interrupted
receipt are under `target/mapped-inference-design-20261113/mac-preflight-4/`.
This permits a new source freeze and carries no full-package acceptance credit.

The third frozen candidate,
`backend-tree:f576b2a5e05d0eee4421b25992414ceb53d4562b`, was retired after independent
reduction probes found seven declaration regressions against the actual accepted-C
compiler. C returns exact `void`/`void[]`; this candidate returns `unknown`/`unknown[]`.
The comparison retains all 54 authored observations and both actual compiler
binaries. The 24-source discovery expansion's thirteen disagreements are existing
broader gaps, but the seven additional reduction failures are current P0-D repair
obligations and cannot be classified as preexisting debt.

R3 passed all Windows 82 core oracles, 25 paths and ten complete cohorts. Its
57,523,183-byte Windows archive, actual x86_64 compiler and all 14,753 source files
were independently verified. Its Mac Native catalog was partial when explicitly
retired; cancellation is not an independent failure observation. All source bytes,
raw results and the retirement receipt remain under
`target/mapped-inference-acceptance-20261113-r3/`, with no whole-candidate pass credit.
All known Mac readers were terminal before subsequent source changes.

Ten new supplementary controls are independently frozen in
`generic-argument-controls.json`: seven accepted-C regressions and three positive
controls. The public CLI test fails on all seven expected rows while the three
positives pass, with production unchanged from R3. The separate Native replay
passes all ten expectations and pins 57 loaded libraries. Red source, actual CLI,
all raw observations and exact failed IDs are retained under
`target/mapped-generic-argument-review-20261113/red/`. The 404 primary expectations
and 12 policy assertions remain unchanged; the supplemental total becomes 31
with zero additional inventory credit. P0-D is still unchecked and uncommitted.

The reductions preserve Map lookup, array element identity and alias values.
Calling the same handler with an any/never assertion recovers the real void
result, while ordinary generic arguments can lose it. Direct callable aliases
can agree even when an inline generic function annotation fails. The next repair
must trace candidate signatures and actual parameter identities at that boundary,
preserve unrelated/shadowed binders, and retain the existing normal 256-expansion
control. A forced void result or an any assertion is not the production repair.

Trace on the unchanged failing source confirms that final return inference compares
a lexical signature parameter `P` with the same declaration's canonical flow
parameter `#typeparam@main.ts:23:P`, rejects its own call candidate and returns
unknown. The repair normalizes authored parameter annotations at their declaring
position before body inference. Inferred contextual annotations keep their
context's own binders; rebinding them would incorrectly capture an outer same-name
parameter. A first focused repair exposed exactly that contextual guard regression;
its red source/result and independent C/Native agreement remain preserved.

The final twelve controls retain the seven R3 regressions, three positives and
two newly calibrated contextual-binder guards. Both red stages are preserved;
all twelve complete CLI observations now match Native without changing expected
results. The supplemental total is 33, including ten intentional owner refusals
and 23 Native agreements, with zero additional primary/inventory credit.

The final source completes all eight focused preflight steps (format, workspace/
all-target Clippy, build, 281 library functions, all seven mapped functions, all
six NonNullable/Awaited functions, all five JSX functions and all six G-T5
functions), retaining all 14,670 backend/tool hashes. Its actual authoring compiler
is `b8616170636450126952d5c06f896d29fe274112b6446e84f9c6c5556b1f7b09`.
Defu, mitt, reproductions and interchange pass again on that actual compiler.
The fresh 60-observation comparison retains 46 complete agreements, 14 broader
remaining differences and zero regressions against the actual accepted C binary.
These are authoring facts only; complete new-source acceptance remains required.

The fourth frozen candidate,
`backend-tree:f9b632e822e72070c084777e2ebe7558b2ef639f`, was retired when the
existing G-C1 `inferred-method-result` control emitted
`export declare const value: number;` instead of the Native/accepted-C
`export declare const value = 7;`. Two functions in that suite failed. Its
partial Windows run and actual x86_64 compiler are preserved with all 14,754
guest source files independently hashed under
`target/mapped-inference-acceptance-20261113-r4/`. Neither platform's partial
results give whole-candidate acceptance credit.

The reduction locates the failure in literal freshness: result-parameter checks
recognized lexical parameter names but missed the same signature's scoped
parameter identity. Substitution already supported both identities. The repair
matches only a signature's own declared parameter name or exact scoped identity,
and uses that shared check for result inference, call-chain freshness and array
element freshness. It preserves the authored-versus-contextual annotation boundary
and does not rebind captured parameters.

On this repaired source, the complete five-function G-C1 suite, seven-function
mapped-inference suite and six-function NonNullable/Awaited suite pass, including
their ignored Native replays. The authoring compiler is
`310dee4e2bf8c5f433a38d070a4aee85434fde884d072f90ad71b4442e08244e`;
red source/results and green source/results remain under
`target/mapped-literal-freshness-review-20261113/`. The next focused preflight
adds G-C1 explicitly to its nine steps. Full new-source acceptance remains pending;
P0-D stays unchecked and uncommitted, with all frozen expectations unchanged.

Final acceptance on 2026-11-13: `backend-tree:bfee3b4ec32820c6c242798b6d2d90fdb6e00600` passes all 188 Native suites /
1071 functions, the complete local workspace, both fresh 8407×2
map reports, projects/tooling and Windows 82 core oracles plus 25 paths and ten
full cohorts. The 404 direct comparisons and 12 policy assertions add 416,
raising N from 41,636 to 42,052 without changing previous case classes. The 33
supplemental observations add no inventory credit. All four retired candidates and
all known broader counterexamples remain preserved in the linked evidence.
