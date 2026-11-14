# Reproduce the frozen P0-G boundary

Select the source and actual compiler profile from
`HIGHER_ORDER_INFERENCE_MEASUREMENT.json`. Verify its artifact-index hash, every
archive hash and every separately hashed member manifest before extracting into
a fresh directory under this checkout's `target/`. Each archive entry is a
regular relative file; reject duplicate names, absolute paths, parent traversal
and links. Do not overwrite an existing observation or acceptance directory.

The archives preserve complete backend/oracle source in `source-snapshots/files`,
accepted cases/suites/measurement in `authority-snapshots`, original compilers,
an accepted-source patch and all raw products/logs. The Windows archive also
preserves complete selected workspace source including root Cargo files.
`oracle-dependencies.tar.gz` retains TypeScript 5.9.3 and Acorn 8.15.0 regular files,
their licenses and third-party notices. No npm installation is required.

After verifying/extracting the indexed archives, run their retained helpers:

```sh
python3 -B /absolute/restored/g_restore_large_members.py
python3 -B /absolute/restored/g_verify_publication.py --archive-only
```

The first reconstructs every original large member with exact sizes, hashes and
file modes. Failed partial reconstructions are retained under distinct names.
The second verifies archive/member bytes, source/authority snapshots, both actual
compiler profiles' physical G products, all bindings and preserved earlier
failures. Archive-only verification supports a later checkout without asserting
that its current source is the historical accepted source. It does not rerun tests.

From a checkout/materialization of the recorded source, restore the oracle
archive to a separate directory and set absolute paths:

```sh
export BLUEICE_BLUETSC_ORACLE=/absolute/oracles/node_modules/typescript/bin/tsc
export BLUEICE_ACORN_ORACLE=/absolute/oracles/node_modules/acorn/bin/acorn
export CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0
export CARGO_PROFILE_TEST_DEBUG=0
cargo test -p blueice-bluets --test higher_order_inference -- --include-ignored
```

That suite checks 384 complete CLI observations,12 separate API policy controls,
four prefix-call regression controls,16 separately frozen recursive-scope
observations,12 separately frozen generic-library capture observations and fresh
Native/BlueTS declaration
consumers, selected-edition syntax and execution. Frozen references are
verification-only; they must not be replaced with BlueTS results.

To compare an archived actual compiler without rebuilding it:

```sh
node development/browser_core/phase-18-bluets/tools/record_higher_order_inference.cjs --verify
export BLUEICE_HIGHER_ORDER_INFERENCE_COMPILER=/absolute/accepted-compiler
export BLUEICE_HIGHER_ORDER_INFERENCE_EXPECTED_COMPILER_SHA256=the-selected-actual-sha256
export BLUEICE_HIGHER_ORDER_INFERENCE_SOURCE_REVISION=the-recorded-backend-tree
export BLUEICE_HIGHER_ORDER_INFERENCE_OBSERVATIONS=/absolute/fresh-observations
node development/browser_core/phase-18-bluets/tools/verify_higher_order_inference.cjs
```

These focused commands do not constitute complete acceptance. The full gate
requires all 191 Native suites/1088 functions, full workspace tests, both actual
Mac profiles' 8407×2 map reports, four real-project gates, format, warnings-denied
workspace/all-target Clippy, Node/Python and accounting checks. Windows requires
82 core oracles, 25 path cases and 27 selected complete regression cohorts and six host regressions.
Retained runtime scripts include original machine paths and fresh-run guards;
adapt paths into a new output namespace without rewriting earlier receipts.
The final Windows verifier compares every cohort's actual compiled function
catalog with every passed name and expected platform count. It requires seven
G-M2 functions because the eighth source function is Unix-only. Historical r5 verifier/supervisor failures and their separate correction,
and the r6 G-T7/owned-stop/Windows evidence remain unchanged in
`development-history/failed-r5` and `development-history/failed-r6`; they do not
replace fresh final acceptance.

All 22 broader feature rows remain open. N counts suite-qualified assertions,
not unique programs or a percentage of TypeScript language support.
