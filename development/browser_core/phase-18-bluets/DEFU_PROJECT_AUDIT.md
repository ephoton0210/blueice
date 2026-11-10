# Second real-project audit — defu

The later [completed compiler repair](DEFU_REPAIR_ACCEPTANCE.md) and
[multi-source state evaluation](DEFU_INTERCHANGE_ACCEPTANCE.md) supersede the
project blockage below. This dated audit retains its original source, failed
observations and acceptance criteria.

This completed investigation is **blocked for BlueTSC compatibility**. It does
not count a failed cold build or unexecuted dependent checks as a project pass.
[Measurement](DEFU_PROJECT_MEASUREMENT.json) ·
[raw evidence](DEFU_PROJECT_ARTIFACTS.json) ·
[frozen source/profile](../../../differential-testing/bluets-projects/defu-v6.1.4.json)

## Source and fixed acceptance

All three unchanged production TypeScript sources come from
[upstream commit `1b9fcab2c1479f0295a5f867c6ec36a01fda2dfb`](https://github.com/unjs/defu/tree/1b9fcab2c1479f0295a5f867c6ec36a01fda2dfb/src).
The tag is `v6.1.4`; the original package metadata says `6.1.3`. The manifest
records both facts and all file/archive hashes. Original MIT/provenance notices,
package metadata, source and original configuration remain byte-for-byte.
The audit uses an explicitly authored CommonJS/ES2022/strict production profile,
without upstream package scripts, dev dependencies or the full upstream suite.

The fixed acceptance compares three exact declarations and their consumers,
twelve named runtime scenarios, initial/warm incremental output, imported
implementation edits, exported-type edits with an independently changed
dependent-consumer verdict, added/missing/restored imported sources, publication
preservation on failure, configured state presence and edited/clean artifact
equality. All emitted artifacts except the separately recorded path-dependent
public state file participate in warm/clean comparisons. Empty output is not
declaration equality. Native declaration bytes and every stage's input/artifact
observations are frozen before the first BlueTSC run and revalidated thereafter.

## Measured result

Accepted compiler source for this investigation:
`backend-tree:3567e7e2748a08e22f3422a47709fb1cd7211ec0`.
Its actual executable is retained by the existing
[completed implementation evidence](PROJECT_FOLLOWUP_ARTIFACTS.json); the new
audit records that compiler's hash and verifies the backend graph before and
after execution. No production Rust source changed in this investigation.

| Property | Pinned Native 5.9.3 | BlueTSC |
| --- | --- | --- |
| Complete unchanged production graph | Pass, all three sources | Cold build rejected, 13 diagnostics |
| Complete declarations | Three frozen byte-exact files | Blocked; empty output is unequal |
| Twelve runtime scenarios | Pass, also after implementation/addition/restoration edits | Blocked |
| Positive/negative and dependent type consumers | Pass; type edit changes the dependent verdict | Blocked |
| Complete incremental/edit/missing/restore/clean acceptance | Pass | Blocked |

The initial authored prototype smoke expected stronger sanitization than this
original library implements. Its failed Native calibration is retained. The
corrected observation records the inherited enumerable result and separately
checks that `Object.prototype` is unchanged; it makes no input-sanitization
claim. No upstream source or compiler output was altered. That invalid initial
calibration is not credited as acceptance.

## Independently reproduced prerequisites

The [eight frozen controls](../../../differential-testing/bluets-projects/defu-reproductions.json)
are separate from the project and global case denominator. Raw Native and
BlueTSC diagnostics remain in the evidence. Negative syntax controls compare
verdicts and do not claim complete diagnostic parity.

| Family / existing G-ID | Minimal observation | Native | BlueTSC |
| --- | --- | --- | --- |
| Contextual parameter names — G-T2 / G-C1 | `namespace` as a default parameter and a generic function-type parameter | Accept both | Reject both |
| Conditional tuple infer rest — G-T2 | `[unknown, ...infer Rest]` inside a conditional extends pattern | Accept | Refuse |
| Unknown-object guard order — G-T1 / G-C1 | Null check followed by typeof check in an early-return OR guard | Accept | Spurious possible-null diagnostic |
| Reordered guard control | The same checks with typeof first | Accept | Accept |
| Three rejected controls | Nullable object without null guard, reserved `class` parameter, infer outside a conditional | Reject | Reject; diagnostic differences retained |

Four false refusals across three feature families reproduce the initial
project boundary. Thirteen diagnostics are not thirteen independent defects.

## Repair gates

1. Preserve all frozen project/reproduction sources and Native expectations.
   Remove the four false refusals while retaining all three rejected controls.
   Contextual names must work at parameter and function-type boundaries;
   infer-rest support must retain conditional-infer restrictions; both pure
   object/null guard orders must narrow without dropping nullable failures.
2. Pass the full unchanged project's cold build, all raw declaration bytes,
   consumers and runtime scenarios. Later independent blockers stay explicit;
   fixing only the first three families does not complete compatibility.
3. Pass every frozen incremental phase, including dependent type changes and
   missing/restored sources. Determine whether a remaining difference belongs
   to resolver/state behavior or intentional owner-current rebuild policy.
4. Complete the existing source-bound Native cohort, full local workspace,
   formatting/Clippy and Windows VM 82-oracle gate before completing a compiler
   repair item. No new hosted-CI or all-platform requirement is introduced.

Multi-source Native state interchange remains a separately evaluated extension.
Safe fallback rebuilding is not a newly discovered state bug. Broader G-* rows,
the 39,512-case denominator and full-parity-OFF status remain unchanged by this
project audit.
