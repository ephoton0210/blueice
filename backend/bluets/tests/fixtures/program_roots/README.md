# Program-roots public library regression inputs

These unchanged sources, ordered roots and complete Native diagnostics and declaration
bytes were calibrated with TypeScript 5.9.3 before compiler implementation.
`library-contract.json` records source dataset SHA-256 digests. Its 128 primary
observations and 264 lifecycle states select profiles without map publication
comments for the in-memory library boundary. The full external acceptance still
requires all 512 primary observations, 528 lifecycle states and complete maps,
consumers, execution and owner controls. This fixture earns no inventory credit.

`owner-resolutions.json` records four exact dependency edges selected by the
pinned Native resolver over all 392 original library inputs and 50 source pools.
The test owner supplies those edges through `AuthorizedModuleLoader`; the
production compiler never guesses an extension or gains filesystem access.
The isolated owner-adapter correction preserves the original fixture and all
assertions and is documented in `PROGRAM_ROOTS_CONTRACT_AMENDMENT.json` in the
phase plan. The original frozen test and correction proof remain preserved.

To record every library observation and actual incremental work set:

```sh
cargo run -p blueice-bluets --example program_roots_measurement -- \
  backend/bluets/tests/fixtures/program_roots/library-contract.json \
  /tmp/program-roots-report.json
```

The output filename must be fresh. Completion of this measurement is separate
from agreement: inspect all reported counts and differences.
