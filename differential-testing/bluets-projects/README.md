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
