# K.10.1 native JSX typing baseline

The immutable corpus in `backend/bluets/tests/fixtures/jsx_typing_extras`
records TypeScript 5.9.3 decisions for thirty forms in CommonJS/node10 and
ESNext/bundler, targeting ES2022 with the classic `h` factory and its own
`h.JSX` namespace. Sixty configurations contain 26 accepts and 34 rejections.
Every accepted native output executes under Node and returns `[42]`, parses
as ES2022 with pinned Acorn 8.15.0, and retains its exact declaration output.

The forms cover explicit, inferred, constrained and defaulted generic
function tags; generic class tags; ElementType; managed attributes and
function/class defaultProps; IntrinsicClassAttributes; duplicate attributes;
spread replacement; children collisions; and declared, unknown and hyphenated
namespaced attributes. Primary diagnostics and sixteen related records keep
their native codes, text and source positions. Selected inputs, full output
inventories and source hashes are also recorded.

The public BlueTSC binary from `95c6bcd7aa78ee3ce5264ae99245cab135b5db02`
accepts 22 configurations, with 28 verdict and sixteen rejected-primary
differences. Ten matching accepted programs execute successfully, with no
declaration differences. Related-origin differences are retained by the
Rust replay, including managed props and quoted namespaced field names.

Before implementation, the frozen 12,835-file Linux Docker snapshot passes
format and warnings-denied all-target Clippy for BlueTS, its direct bridge
and BlueJS. The four-test replay has two passes (corpus completeness and
live native recorder) and two expected failures (public typing and emitted
program replay). Independent native replay succeeds both with the ordinary
temporary directory and a symlinked temporary root; all fixture and shared
oracle-support hashes remain unchanged.

Evidence under `/private/tmp/blueice-k14-linux`: `k101-public-native-baseline.json`,
`k101-canonical-corrected-proof.json`, `blueice-k101-native-red-status.json`
and `blueice-k101-native-red-source-hashes.json`. Commit this test-only failing
baseline locally before implementation; publish it together with the tested
implementation after the complete K.0 gate passes.
