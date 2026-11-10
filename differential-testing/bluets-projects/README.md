# BlueTS real-project audit

This independent audit compiles the complete, unchanged source of
[mitt 3.0.1](https://github.com/developit/mitt/tree/b240473b5707857ba2c6a8e6d707c28d1e39da49)
against TypeScript 5.9.3 and BlueTSC. The frozen manifest is
[`mitt-3.0.1.json`](mitt-3.0.1.json). It records the exact revision, original
source/package/license hashes, CommonJS/ES2022/strict options, incremental
options and minimal diagnostic controls.

The fixture archive contains the original `src/index.ts`, `package.json` and
MIT `LICENSE` byte-for-byte. The upstream package scripts are not part of the
audit. The original MIT notice remains in the archive; the audit tools and
authored reproductions carry their own MPL-2.0 headers.

From the repository root, using a built compiler and installed pinned reference:

```sh
node --test differential-testing/bluets-projects/audit.test.cjs
node differential-testing/bluets-projects/audit.cjs \
  --compiler target/debug/bluetsc \
  --typescript /absolute/path/node_modules/typescript/bin/tsc \
  --output /absolute/path/new-empty-audit-directory
```

The default source binding verifies every tracked backend blob against
`HEAD:backend`. Before a commit, `--source-tree <backend-tree-sha>` selects an
existing immutable backend tree and performs the same working-file and tracked
path checks. A caller must also retain the compiler build evidence linking
those source bytes to the executable hash. The output directory must not exist;
this prevents a preceding Native build from supplying missing BlueTSC outputs.

Acceptance requires all of the following:

1. Both compilers accept the unchanged complete upstream source.
2. Complete declaration inventories and every declaration byte agree.
3. Both actual outputs pass all eight authored event-runtime scenarios.
4. Pinned TypeScript accepts each emitted declaration's positive consumer and
   rejects its wrong-payload consumer with TS2345.
5. Explicit incremental builds preserve warm output and execution; an edited
   build matches a clean rebuild. Cold compilation and the explicit incremental
   configuration are measured separately, so one rejection cannot conceal the
   other boundary.

The command exits **0** only when every project property passes, **2** for a
completed audit with project incompatibility, and **1** for an audit/setup error.
A refused cold build leaves its declaration/runtime/consumer/rebuild checks
explicitly blocked. Missing outputs never count as matching declarations.

The five authored minimal programs distinguish generic object methods from
ordinary object methods, generic class methods and a scalar configuration
control. Their diagnostics and the separate incremental-option comparison are
retained even when the full project is blocked. These observations are separate
from the versioned compatibility denominator and from upstream's full test suite.

The measured blockers and repair gates are recorded in the Phase 18
[real-project audit](../../development/browser_core/phase-18-bluets/REAL_PROJECT_AUDIT.md).

The [completed mitt follow-up](../../development/browser_core/phase-18-bluets/PROJECT_FOLLOWUP_ACCEPTANCE.md) retains the repaired project result and the earlier blocked audit separately. Broad unmeasured project combinations remain open.

## Second project: defu

[The frozen defu manifest](defu-v6.1.4.json) selects every production TypeScript
source at commit `1b9fcab2c1479f0295a5f867c6ec36a01fda2dfb`. The upstream tag
is `v6.1.4`; its original `package.json` says `6.1.3`, which is preserved
verbatim. All original MIT notices, including the helper's upstream provenance,
remain in the archive. No package script or upstream test runner is invoked.

```sh
node --test differential-testing/bluets-projects/defu-audit.test.cjs
node differential-testing/bluets-projects/defu-audit.cjs \
  --compiler /absolute/path/bluetsc \
  --typescript /absolute/path/node_modules/typescript/bin/tsc \
  --output /absolute/path/new-empty-defu-audit
node differential-testing/bluets-projects/defu-reproduce.cjs \
  --compiler /absolute/path/bluetsc \
  --typescript /absolute/path/node_modules/typescript/bin/tsc \
  --output /absolute/path/new-empty-defu-reproductions
```

The fixed CommonJS/ES2022/strict production profile discovers all three original
sources. Twelve authored runtime scenarios, complete byte-exact declarations,
positive/negative consumers, a dependent type-consumer verdict change and
implementation/type/dependency edits establish the project gate. The configured
public incremental-state file is recorded separately from emitted artifacts;
its path-dependent bytes are not a portable Native-byte claim. All other public
artifacts, including extra manifests, remain in warm/clean comparisons.

The [Native reference](defu-v6.1.4.native.json) was frozen before observing
BlueTSC. Every later audit requires its exact Native declarations and per-phase
input/artifact observations; compiler/library byte changes invalidate it.
The separate [minimal controls](defu-reproductions.json) and
[Native observations](defu-reproductions.native.json) compare verdicts and retain
complete raw diagnostics, without claiming diagnostic parity for rejected
syntax. Both commands use fresh directories, record the actual compiler hash
and verify the selected immutable backend source before and after execution.
Both accept `--source-tree <backend-tree-sha>` with the same source/build
binding requirements as the first project audit. Exit 2 records
observed incompatibility; exit 1 means an invalid audit/setup. A blocked cold
build cannot supply downstream project successes.

[The measured audit and repair gates](../../development/browser_core/phase-18-bluets/DEFU_PROJECT_AUDIT.md)
keep the new project's result separate from mitt and the global case denominator.

## Multi-source state evaluation

The [frozen evaluation matrix](defu-interchange.json) covers both state
directions with all three production sources selected as roots, and with one
root whose imports supply the other two sources. It evaluates the current
policy without expanding state authority or promising a Native warm no-op.

```sh
node --test differential-testing/bluets-projects/defu-interchange.test.cjs
node differential-testing/bluets-projects/defu-interchange.cjs \
  --compiler /absolute/path/bluetsc \
  --typescript /absolute/path/node_modules/typescript/bin/tsc \
  --output /absolute/path/new-empty-state-evaluation \
  --source-tree <backend-tree-sha>
```

Each direction starts with a separate frozen Native baseline and fresh donor
output. The handoff records whether the receiver retained every donor artifact
or rebuilt its own complete clean inventory. Later implementation/type edits,
added/missing/restored dependencies, corrupted/missing public state, dependent
consumers, runtime and clean rebuilds must pass separately. External state paths
and input/output collisions must be rejected without altering owner inputs or
the outside sentinel. Public state bytes and hashes are recorded separately
from artifact equality. Exit 2 records differences; exit 1 is an invalid setup.

The prerequisite Native recorder preserves Windows diagnostic console streams
under `BLUEICE_DEFU_NATIVE_OBSERVATIONS` (or a retained temporary directory).
Only diagnostic console CRLF terminators are compared as LF. All declaration
and runtime bytes retain exact comparisons.

[Current repair acceptance](../../development/browser_core/phase-18-bluets/DEFU_REPAIR_ACCEPTANCE.md) and [finite state evaluation](../../development/browser_core/phase-18-bluets/DEFU_INTERCHANGE_ACCEPTANCE.md) retain the source-bound results and limits.
