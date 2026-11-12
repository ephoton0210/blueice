# P0-B — NonNullable and ES5 Awaited

Status: completed within the frozen scope. See [acceptance](NONNULLABLE_AWAITED_ACCEPTANCE.md),
[measurements](NONNULLABLE_AWAITED_MEASUREMENT.json) and the
[artifact manifest](NONNULLABLE_AWAITED_ARTIFACTS.json).

The package connects G-T2/G-T3/G-T5/G-T6/G-T7/G-T8 and preserves G-T1 diagnostic
regressions. Its fixtures are `backend/bluets/tests/fixtures/nonnullable_awaited/`.

| Frozen scope | Required and verified result |
| --- | --- |
| Primary | 91 forms × four profiles = 364 comparisons |
| Ordinary operators | Eight Present/Unwrap forms × four profiles = 32 comparisons |
| Complete direct scope | 396 full comparisons; 315 exact declaration inventories / 331 files |
| Declaration consumers | 630 isolated positive/negative Native consumers |
| Syntax/runtime | 331 selected-edition parses and twelve executions |
| Additional Native demand | Six independent recursive/guarded/decreasing/adjacent-error controls |
| Separate boundaries | Six owner/runtime, nine original resource and six supplementary zero/one-work controls |

All authored inputs and Native expectations were frozen before production changes.
Original frozen SHA-256 values remain unchanged:

- `operator-cases.json`: `3d167988d15229502f99c6e8c6a51e486c6c8d94c9f96f9630fc8ed7e3883b99`.
- `boundary-controls.json`: `99c569b66c1407f4258a2c9290580d0a4fa8d19e891b2767bb7ceace1289d258`.
- `operator-reference.json`: `e45a361c9fe6a67178c38732d2f45cdb12ec112885a6845ff5cf6b1ec628abda`.
- `reference.json`: `e04f4ef6723b9d3d73cbd1eb8dc8651ce62d888178c16cdba51ca73f14b205c5`.
- `cases.json`: `d40499c6371615b3e4010cff3b3513766a531681219881980e1d3a1b13e9a256`.

Supplementary demand references and zero/one-work controls are separately hashed
in the measurement. Native semantic errors and configured policy refusals retain
distinct boundaries. The ordinary Present/Unwrap controls share the same
intersection and conditional machinery; matching a utility name does not supply
its behavior. Each profile requires exact primary/related diagnostics, UTF-16
positions and declaration bytes, including the explicit LF contract on Windows.

The baseline has 29/396 complete agreements and 82 accepts. Complete accepted
measurements reach 396/396 and 315 accepts. Intermediate checkpoints and the
retired G-C1 failure remain in the raw development/audit archives. Acceptance
requires complete same-source Native, local workspace, both map profiles,
projects/tooling and Windows 82-core/path/regression gates. Those gates have now
passed; implementation and measurements are committed together. P0-C and all
22 broader rows retain their open scope.
