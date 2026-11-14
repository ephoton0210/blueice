# Reproduce the frozen P0-F boundary

Use `CONDITIONAL_INFERENCE_MEASUREMENT.json` and its artifact index to select the
accepted compiler/oracle source. Verify archive and member hashes before use.
The backend tree identifies Rust/fixture source; the complete frozen manifest
also identifies oracle tools. Earlier failed or canceled toolsets are preserved
as history and cannot substitute for the final manifest.

The source archive contains every backend file. The Windows evidence also
contains the complete selected workspace source, including root Cargo files.
The accepted source patch and original compilers are preserved in the artifact
bundles. Pinned TypeScript/Acorn packages include their original license notices;
restore their regular files from `oracle-dependencies.tar.gz`. The omitted npm
symlinks are unnecessary when invoking `typescript/bin/tsc` directly.

From a checkout of that source, set the following to the restored absolute paths:

```sh
export BLUEICE_BLUETSC_ORACLE=/absolute/oracles/node_modules/typescript/bin/tsc
export BLUEICE_ACORN_ORACLE=/absolute/oracles/node_modules/acorn/bin/acorn
export CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0
export CARGO_PROFILE_TEST_DEBUG=0
cargo test -p blueice-bluets --test conditional_inference -- --include-ignored
node --test development/browser_core/phase-18-bluets/tools/virtual_compiler_host.test.cjs
```

The Rust test checks 384 complete CLI observations, 24 separate alias controls,
12 API policy controls and fresh Native/BlueTSC consumers, syntax and execution.
Frozen Native references are verification-only. To use an archived compiler
without rebuilding it, run the permanent adapters directly:

```sh
node development/browser_core/phase-18-bluets/tools/record_conditional_inference.cjs --verify
export BLUEICE_CONDITIONAL_INFERENCE_COMPILER=/absolute/accepted-compiler
node development/browser_core/phase-18-bluets/tools/verify_conditional_inference.cjs
```

Set `BLUEICE_CONDITIONAL_INFERENCE_EXPECTED_COMPILER_SHA256` to the selected
measurement's actual compiler hash and `BLUEICE_CONDITIONAL_INFERENCE_SOURCE_REVISION`
to its recorded source to retain those guards. Set
`BLUEICE_CONDITIONAL_INFERENCE_OBSERVATIONS` to a fresh output directory to retain
all physical programs and products. Do not reuse an earlier observation directory.
Declarations are compared byte for byte; no sorting or newline repair is allowed.
The virtual-host helper canonicalizes lookup paths and follows the host's case
rule; it does not alter source text, output text or disk fallbacks.

These focused commands do not constitute complete acceptance. The full gate
requires the entire Native catalog with all ordinary and ignored functions,
complete workspace tests, both actual Mac compiler profiles' 8407×2 map reports,
defu/mitt/reproduction/interchange projects, format, warnings-denied all-target
workspace Clippy and Node/Python/accounting checks. The Windows gate requires
82 selected core oracles, 25 path cases, all 16 complete affected cohorts and the
six virtual-host checks. Archived orchestration scripts contain the original
machine paths; adapt work directories for a new measurement without rewriting
old receipts or treating an incomplete run as complete.

All 22 broader rows remain open. N counts suite-qualified assertions, not unique
programs or a percentage of TypeScript. Alias/path supplements, original-profile
replays and policy controls cannot be relabeled as direct language agreement.
