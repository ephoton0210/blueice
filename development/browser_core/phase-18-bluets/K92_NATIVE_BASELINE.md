# K.9.2 path and implicit type resolution native failing baseline

The isolated TypeScript 5.9.3 corpus records 46 configurations in CommonJS/node10
and ESNext/bundler modes: 38 accepts, eight rejects and 38 actual Node executions.
It measures exact/wildcard/longest-prefix paths, ordered target fallback and
precedence, baseUrl with and without paths, rootDirs relative suffix lookup and
local precedence, automatic/scoped @types, explicit types selection/exclusion
and custom/empty typeRoots. Exact selected source inputs, declarations, artifact
inventory, primary diagnostics and source positions are retained. Every source
fixture has the MPL-2.0 header. The default-read-only portable recorder agrees
with all observations. Four real tsc --project controls independently confirm
automatic/explicit type discovery uses the actual configuration directory.

An independent frozen CLI copied from the final K.9.1 full-gate source accepts
none of the 46 cases, with 38 verdict and eight primary differences. It records
all actual diagnostics and leaves every Native source/binary hash unchanged.
Evidence: k92-native-public-preparation-proof.json, k92-public-native-baseline.json
and k92-prepared-native-cli-proof.json. The source snapshot is
blueice-k91-full-k0-third, verified by its complete 1,336-test K.0 gate and
published as 8b629548e. The fixed binary SHA-256 is
d588e664f8c5b2b917428e2c926416d89abccc45047c3bd45d4ef66343561ec8.

The initial research API call omitted configFilePath; it was corrected and
cross-checked against the actual CLI before recording any repository golden.
The superseded research output remains outside the repository. Commit this
test-only failing replay before implementation, and keep it unpushed until the
complete corrected K.0 gate passes. K.9.2 remains open.

The corrected immutable 12,450-file Linux replay passes format, warnings-denied
three-crate all-target Clippy, completeness and the live Native recorder. The
public verdict/output tests fail as expected (two passes, two failures).
Evidence: blueice-k92-baseline-third-proof.json and retained source hashes,
status and logs. Earlier runs found copied completeness constants; only those
counts were corrected to the recorded 46 cases and 38 accepts, without changing
Native goldens or production.
