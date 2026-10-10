# K.8.2 declaration options: recorded failing public baseline

TypeScript 5.9.3 records 48 configurations (28 accepts, 20 rejects), with 24
actual Node executions and four accepted declaration-only projects. The
portable live recorder replays all native golden observations unchanged.

A fixed copy of the K.8.1 selection-aware public CLI accepts only two cases,
reproducing 26 verdict differences and twenty rejected primary differences.
Every project input remains unchanged. Evidence: retained
`k82-public-native-baseline-proof.json`, actual outputs and the exact fixed
binary hash. This baseline precedes K.8.2 production correction.

The Rust replay is prepared in an isolated worktree; its focused compile/replay
will run after the shared K.8.1 gate releases the single Cargo target. It remains
unpushed until the corrected complete K.0 gate passes. Final hosted CI and
coverage remain pending; this record makes no broader declaration-map or
isolated-declaration parity claim.
