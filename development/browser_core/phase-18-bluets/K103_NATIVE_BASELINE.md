# K.10.3 Native decorator typing baseline

TypeScript 5.9.3 is the pinned oracle. Thirty source forms run in both
CommonJS/node10 and ESNext/bundler at ES2022: sixty configurations, 32 accepts
and 28 rejects. Twelve configurations use legacy decorators; the other 48
use standard decorators. Every accepted physical output executes under Node
and produces `[42]`, parses under Acorn 8.15.0 and retains exact declarations,
selected inputs and artifact inventory. Each rejection has one primary
diagnostic; four related records are retained. The 243 corpus files use LF.

Replacement witnesses cover method parameters/results, getters, setters,
field initializers, accessor result objects and replacement classes. Legacy
witnesses cover replacement classes and typed method descriptors, plus void
property decorators and their rejected non-void return. Context witnesses
cover each class/member context, union context, kind/name/static/private,
access methods, metadata, initializer callbacks, wrong-kind applications,
literal kind assignments and readonly writes. This records a bounded subset,
not general decorator or TypeScript parity.

The published preimplementation source is
`26a7ce5a8c5c57b12f2320b13732f8d8e084b8b2`, already verified by the complete
1,373-test K.0 gate. Its saved public `bluetsc` has SHA-256
`a1cb845ea37f338b4b107e2c4e6e953169c276c7d5f6d15d11b42849f7b15b79`.
It accepts thirty configurations, with 34 verdict differences and ten
rejected-primary differences against Native. Fourteen accepted executions
agree before correction. Evidence: `k103-frozen-public-compiler-proof.json`
and `k103-public-native-baseline.json`.

All Native observations are recorded before production changes. Portable
recorder verification passes under ordinary and symlinked temporary roots,
reproducing all 32 actual executions in each run without changing any fixture.
The expected failing public replay passes format and warnings-denied
three-crate all-target Clippy: two tests pass (completeness and live Native),
and two fail (public verdict/diagnostic replay and accepted-output replay).
Evidence: `blueice-k103-native-baseline-proof.json`.
The test-only baseline will remain unpublished until the corrected complete
K.0 gate passes. Its committed Native source and observation bytes must stay
unchanged throughout implementation.
