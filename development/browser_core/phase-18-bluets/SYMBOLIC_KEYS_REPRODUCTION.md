# Reproduce the frozen P0-E contract

Use the implementation revision recorded in `SYMBOLIC_KEYS_MEASUREMENT.json`.
The fixture contract is
`backend/bluets/tests/fixtures/symbolic_keys/FROZEN.json`. Do not regenerate its
expectations from BlueTSC or normalize diagnostic/declaration text.

The recorded oracle packages are TypeScript 5.9.3 and Acorn 8.15.0. Their exact
files and original license notices are preserved in `oracle-dependencies.tar.gz`
inside the published evidence. The fixture references independently pin the
Native implementation and loaded library hashes. The replay tools reject a
different implementation, library, frozen fixture or declaration byte.

From the repository root, choose a fresh observation directory and set paths to
the pinned packages:

```sh
export BLUEICE_BLUETSC_ORACLE=/absolute/path/oracles/node_modules/typescript/bin/tsc
export BLUEICE_ACORN_ORACLE=/absolute/path/oracles/node_modules/acorn/bin/acorn
export BLUEICE_SYMBOLIC_KEYS_OBSERVATIONS="$(mktemp -d)"
export BLUEICE_SYMBOLIC_KEYS_SOURCE_REVISION="backend-tree:$(git rev-parse HEAD:backend)"
```

Independently replay Native without modifying the reference:

```sh
node development/browser_core/phase-18-bluets/tools/record_symbolic_keys.cjs --verify
```

Build the actual compiler in the shared workspace target and compare its complete
outputs, isolated declaration consumers, selected-edition syntax and execution:

```sh
CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 cargo build -p blueice-bluets --bin bluetsc --locked
export BLUEICE_SYMBOLIC_KEYS_COMPILER="$PWD/target/debug/bluetsc"
node development/browser_core/phase-18-bluets/tools/verify_symbolic_keys.cjs
```

Both tools also verify all 16 separately frozen compound-key controls. These
controls receive zero additional inventory credit. The normal Rust tests add
the public CLI matrix and the separate owner/runtime/resource policies:

```sh
CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 \
  cargo test -p blueice-bluets --test symbolic_keys --locked -- --include-ignored
```

These commands reproduce the focused P0-E contract. Complete acceptance also
requires the unfiltered Native catalog, complete workspace, both full 8407×2 map
reports, four project gates, tooling and the recorded Windows x86_64 gate.
The measurement and archived drivers retain those commands and their actual
compiler/source bindings. A new run needs its own complete results.

`SYMBOLIC_KEYS_ARTIFACTS.json` lists each archive and its SHA-256. Separately
hashed `*_MEMBERS_*.json` manifests list every member's path, byte count and
SHA-256. Preserve the directory structure when restoring evidence. The original
Windows archive has a separate complete-source/product/PE-compiler audit.
The archived `check_publication.py` verifies published bytes and report bindings;
`audit_acceptance.py` checks the original acceptance workspace and source index.
