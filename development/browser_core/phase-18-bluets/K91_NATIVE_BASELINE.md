# K.9.1 package feature native failing baseline

The isolated Native TypeScript 5.9.3 corpus has 54 configurations: 43 accepts,
11 rejects and 43 actual Node executions. It covers typesVersions range/order,
exact/wildcard/fallback mappings, Node16 explicit extensions, exports precedence,
package self-name/subpaths/shadowing, .d.mts/.d.cts/.mts/.cts package entry points
and classic relative/sibling/ancestor lookup. Exact primary diagnostics, selected
source inputs, emitted inventory, declarations and execution/syntax observations
are retained. Every new source fixture has the MPL-2.0 header. The licensed
54-case default-read-only portable recorder agrees with every observation.
Initial unlicensed preparation was superseded before repository golden recording.

An independent fixed copy of the already compiled K.8.2 public CLI replays all
54 inputs without Cargo or snapshot mutation. It accepts 15 configurations,
with 28 verdict and 11 primary differences. All mutually accepted selected
source graphs and declarations agree. Source/binary hashes remain unchanged.
Evidence: k91-native-public-baseline-proof.json and
k91-public-native-baseline.json, including actual diagnostics and declaration pairs.
The binary source snapshot is blueice-k82-options-fourth; its SHA-256 is
605761b253d3e7b63fe9159d18fabba46d86784a87110c51db6f9b5e40d8e9fe.

Commit this test-only baseline before production correction and keep it isolated
and unpushed until the complete corrected K.0 gate passes. Rust focused baseline
compilation/replay remains pending while the single shared Cargo target finishes
K.8.2. Integrate verified K.8.2 before that replay. K.9.1 remains open.
