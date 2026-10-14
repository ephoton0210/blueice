# K.9.3 JSON and JavaScript source Native failing baseline

The isolated TypeScript 5.9.3 corpus records 28 configurations in
CommonJS/node10 and ESNext/bundler: 18 accepts, ten rejects and 18 actual
Node executions. It covers mixed TypeScript/JavaScript graphs, JavaScript
roots, allowJs/checkJs selection, unchecked assignments, checked assignment
and implicit-any diagnostics, inferred JavaScript declarations, and typed
JSON objects, nulls, arrays and disabled JSON imports. JSDoc types are outside
this measured subset. Exact selected physical inputs, declarations, artifact
inventory, JSON asset values and rejected primaries/positions are retained.
The read-only portable recorder agrees with all observations; four actual
tsc CLI controls independently check JavaScript option behavior.

The independently frozen, verified K.9.2 public CLI accepts six cases. All six
JSON accepts retain exact graphs/declarations and actual runtime results.
JavaScript inputs produce twelve verdict differences; eight rejected primaries
differ. Evidence: `k93-public-native-baseline.json` and
`k93-prepared-native-cli-proof.json`. The source is published as `d352031fd`:
complete 1,348-test K.0 and final 17-test Native path follow-up. Frozen binary
SHA-256: cd291615e8559eab9166b50787af8169c19f1001eb801e3a94498270ed038bb2.

Commit this test-only failing replay before implementation and keep it unpushed
until the corrected complete K.0 gate passes. K.9.3 remains open.

The immutable 12,588-file Linux test-only replay passes format, warnings-denied
three-crate all-target Clippy, completeness and the live Native recorder. The
public decision/output tests fail as expected (two passes, two failures). All
production source files match verified K.9.2. Evidence:
`blueice-k93-baseline-first-proof.json`, retained hashes/status/logs.
