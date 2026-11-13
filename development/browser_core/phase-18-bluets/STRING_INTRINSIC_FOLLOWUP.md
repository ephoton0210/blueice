# P0-C — String intrinsics frozen implementation contract

Status: completed within the frozen scope. See [acceptance](STRING_INTRINSIC_ACCEPTANCE.md), [measurement](STRING_INTRINSIC_MEASUREMENT.json) and [artifacts](STRING_INTRINSIC_ARTIFACTS.json). All22 broader feature rows remain open; full parity remains OFF.

Fixtures: `backend/bluets/tests/fixtures/string_intrinsics/`. Native TypeScript5.9.3, Unicode17.0/ICU78.3; local Node26.7.0 and Windows Node24.21.0 were independently checked for matching scalar mappings.

| Frozen obligation | Minimum result |
| --- | --- |
| Primary inputs | 245 authored forms × four profiles =980 complete comparisons |
| Accept/reject | 772 Native accepts and208 rejects; full primary/related diagnostics and UTF-16 positions |
| Exact declarations | 772 accepted inventories /804 physical UTF-8 declaration files, per actual compiler |
| Isolated consumers | 1544 positive/negative consumers selecting declarations only |
| Syntax/runtime | 804 selected-edition parses and16 actual executions |
| Separate controls | Four ambient owners, four runtime-authority refusals and12 zero/one-work/parser-depth controls |
| Accounting | Preserve all40636 prior cases/classes, add980 direct+20 policy assertions=1000; expected N41636 |

Frozen SHA-256 values:

- `cases.json`: `207dc94343b3baf6b4fbf22aec53f040f6a1897204e38c2113c51907ece22eb7`.
- `reference.json`: `8edcaacd4711bd30cec21f0d18e1e4b5524bdf4e4d3fbd4d5ffc8289ddddb1c3`.
- `boundary-controls.json`: `e6faf88af1373f4163391be8295dee5a3dc9e1c23d0035212e8329becfd0f83a`.

Implementation requirements:

1. Implement Uppercase/Lowercase/Capitalize/Uncapitalize through one shared operation model. Preserve symbolic general-string/any/generic/index/template operations, union distribution/order, constraints, same-symbol idempotence and mixed-operation order.
2. Keep lossless UTF-16 semantic units distinct from source and generated presentation. Direct malformed-unit identity controls must reject high/low surrogates versus U+FFFD and versus each other, while escaped pairs and scalar source spellings remain equal. Physical declaration bytes follow Native UTF-8 writing, including replacement of malformed units only at serialization.
3. Full-string upper/lower mappings retain contextual sigma and expanding mappings. Capitalize/Uncapitalize transform the first code unit, not the first Rust char. Preserve the frozen control/quote/escape printer cases, including null-before-digit, vertical tab and uppercase hex escapes.
4. Validate explicit source intrinsic syntax by Native defining-name/arity rules. Correctly named source string intrinsics and NoInfer/iterator regressions are valid; Fake and wrong arities receive2795. Ordinary local/imported owners keep their own bodies. Renaming and defining-symbol identity must survive inference and declaration reachability.
5. Reuse bounded substitution, inference, relations and demand handling. Work/depth exhaustion remains ResourceLimit with no Native counterpart or output, never an any/unknown fallback. Ambient declarations and static names grant no runtime authority.
6. Preserve all earlier callable/receiver/NoInfer/NonNullable/Awaited, project, CLI/diagnostic and source-map regressions. Keep every new source module at or below1500 lines and include MPL headers.

Completion requires complete same-source Native catalog execution (all ordinary and ignored functions), full local workspace, both actual local compiler profiles' fresh8407×2 map reports, project regressions, formatting, warnings-denied workspace/all-targets Clippy and Node/Python/accounting checks. The Windows x86_64 boundary remains82 core oracles plus the existing25 paths/regressions and the complete new C cohort. Do not add hosted-CI or extra OS prerequisites.

Run library/format/Clippy preflight before freezing a long full-acceptance candidate, so stale structural assertions are found early. Commit C implementation and corresponding full measurements together only after actual acceptance; push under the standing authorization. All22 broader feature rows and full TypeScript parity remain open.

The authoring evidence originated under the main repository's
`target/string-intrinsic-design-20261112/`. The preflights below retain their
historical scope; final same-source acceptance is recorded separately in the
linked acceptance and archived measurements.

| Actual preflight | Verified scope / current result |
| --- | --- |
| Unchanged B compiler | 32/980 complete agreements; retained before C production changes |
| First integrated Windows candidate | Library276 passed; primary816/980, with164 complete differences retained |
| Primitive/template repair | All64 affected CLI cases, library277 and all20 API controls passed |
| Parameter/diagnostic/name repair | All96 affected CLI cases, library278 and all20 API controls passed |
| Integrated C4, Windows | Complete980/980 verdict/diagnostic/physical-DTS comparisons, library279 and all20 API controls passed |
| Integrated C4, macOS | Complete980/980 comparisons, library279 and all20 API controls passed; formatting passed |
| Independent consumer/syntax/runtime verifier | Failed at `es5-commonjs-strict/uppercase-printer-line-separator`: an unescaped U+2028 produces invalid ES5 JavaScript. Its raw failure is retained; the complete verifier has no pass credit |
| Full workspace/all-targets Clippy | Found a duplicated test-vector module, complex tuple types and a redundant closure; retained and repaired before C5 preflight |
| Complete C6 preflight | Full workspace/all-targets Clippy, formatting, library281 and all20 API controls pass; complete980 comparisons,804 declarations,1544 consumers,804 syntax checks and16 executions pass |
| Complete C7 preflight | All C6 preflight obligations pass again; all6 G-T3 and all4 diagnostic-matrix tests, including ignored tests, also pass |
| Supplementary literal contexts | Ten independently calibrated array/tuple/call/overload rows: C6 and C7 agree6/10; first array repair agrees7/10; the completed repair passes all10 through the public CLI regression test |
| Complete C8 preflight | Formatting, full workspace/all-targets Clippy, library281, all6 G-T3 tests, API20 and supplementary10 pass; complete980/DTS804/consumers1544/syntax804/runtime16 verifier passes |

A new nested-template regression initially exposed duplicate traversal; that
failed C5 test is retained and the corrected C6 library suite passes.

The JavaScript repair preserves string values and escaped line continuations,
handles strings inside nested template expressions, and keeps comments and
raw template chunks intact. It applies before ES2019 and uses the existing
mapped-edit path. A dedicated implementation version participates in compiler
fingerprints. Source/library numeric intrinsic review probes separately agree
8/8 with Native and carry no additional primary or inventory credit.

The implementation retains lossless UTF-16 values separately from source and
generated presentation, defining-symbol identity, rigid parameter identities,
shared bounded operators, target-library allocation order, and type/value
namespace separation. Debugger type interning compares semantic identities,
including distinct malformed units that share a physical display token.

The read-only Native replay independently passed980 observations,804 declarations,
1544 consumers and16 executions without changing the three frozen JSON files.
Additional calibration includes94 semantic/printer vectors, three escape cases,
12 template and52 numeric-placeholder observations. Earlier standalone harness
results remain preparation evidence; actual integrated results above supersede
the former statement that C had not compiled. The recursive oracle-hash tool
suite passed17 tests, including both nested helper modules.

The main-branch bridge verified all81 authored files and14645 backend/tool files
against C4 after B was pushed; subsequent source edits for C5/C6 are recorded in new manifests.
At that C4 bridge, no completed C case measurement had been published. Inventory
adapters planned980 direct plus20 separate control assertions. Final acceptance
subsequently verified those1000 additions and N41636 without changing any earlier
case classes; see the measurement linked above.

Code-size checks cover all new/modified source files. Two unchanged older test
harnesses remain separate maintenance debt: `coverage_bluets_frontend.rs`
(7070 lines) and `typescript_oracle.rs` (5399 lines). They are outside the
543-production-file P0-B audit. Final C evidence covers559 production files,
with a maximum of1477 lines and none above1500. All22 broader feature rows remain
open.

The first formal candidate,
`backend-tree:86bd94191c963ffe6a22ba61672f01aedbef77eb`, was retired after
existing G-T3 and diagnostic-matrix regressions failed. Generated string
literals bypassed two older literal-context and template-suggestion checks.
The raw failures and retirement record remain under
`target/string-intrinsics-acceptance-20261113-r1/`; its 13 successful suites
do not constitute whole-candidate acceptance. Its Windows run was also stopped,
and a subsequent process query confirmed no remaining candidate or build jobs.

The C7 repair recognizes generated literal union members and reads template
suggestion strings through their lossless UTF-16 values. No frozen expectation
changed. Its complete preflight passed formatting, full workspace/all-targets
Clippy,281 library tests, all6 G-T3 and all4 diagnostic-matrix tests, including
ignored tests, and the whole980/DTS804/consumers1544/syntax804/runtime16 verifier.

A bounded review of the remaining literal classifiers found four array/tuple
diagnostic differences among ten supplementary Native observations. The six
accepted call/overload/array controls already agreed. The first array repair
fixed the singleton case; preserving the original contextual type for element
inference and the existing template spelling helper resolved the remaining
three. The final public CLI regression test passes all10 and compares complete
diagnostics, positions and exact declarations. These supplementary rows have
no additional primary or compatibility-inventory credit. Raw red results and
Native observations remain in `literal-classifier-review-r7/` under the design
root; expectations were frozen before this repair.

The complete C8 authoring preflight passed before source freeze. Final source
`backend-tree:687190197095253ff614e93a035b4e424b5e725a` passed all187 Native suites /1064 functions, the full local workspace,
both fresh8407×2 map reports, all projects and tooling checks, and Windows82 core
oracles plus25 paths and nine full regression cohorts. See the acceptance and
measurement above for exact compiler/source/artifact bindings. The supplementary
ten rows remain outside the primary/inventory denominator; all40636 old case
classes are unchanged and1000 new assertions produce N41636.
