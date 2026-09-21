# BlueTS and BlueTSC test report

[← Phase 18 plan](PLAN.md) · [Test interface](TEST_INTERFACE.md) · [Integration contract](INTEGRATION_CONTRACT.md)

## What is measured

BlueTS is **not** measured against a Test262-style conformance inventory: its [test interface](TEST_INTERFACE.md) is deliberately compile-only and states that the Test262 corpus is not a TypeScript conformance suite. The complete data available for BlueTS and BlueTSC is therefore three kinds of evidence, each recorded here per platform on the same source revision:

1. **Automated test suites** of the two BlueTS crates: `blueice-bluets` (the front end, checker, emitter, `bluetsc` and `bluets-test-interface` binaries) and `blueice-bluets-bluejs` (the only crate that depends on both BlueTS and BlueJS: the structured-program bridge, page-realm admission and debug attachment).
2. **The pinned TypeScript 5.9.3 compatibility oracle**, which compiles a fixture matrix with BlueTSC and compares its behaviour with the real TypeScript compiler.
3. **Rust line coverage** of the two crates from the workspace coverage run that CI's `Coverage` job uses.

The BlueTS language matrix is bounded and versioned (`blue-ts-0.1`); a passing result is a statement about that matrix, not about TypeScript as a whole.

## Summary by platform

| Platform | `blueice-bluets` tests | `blueice-bluets-bluejs` tests | TypeScript 5.9.3 oracle | Line coverage: `blueice-bluets` | Line coverage: `blueice-bluets-bluejs` |
| --- | --- | --- | --- | --- | --- |
| macOS | 180 pass / 0 fail / 1 ignored | 75 pass / 0 fail / 0 ignored | pass (68 cases) | 93.17% (7,636 / 8,196) | 81.85% (1,804 / 2,204) |
| Ubuntu | 180 pass / 0 fail / 1 ignored | 75 pass / 0 fail / 0 ignored | pass (68 cases) | 93.17% (7,636 / 8,196) | 81.85% (1,804 / 2,204) |

The one ignored `blueice-bluets` test is the TypeScript oracle, which is opt-in and is run explicitly (third column). The suites pass on every platform measured; the crates' line counts and coverage are identical on macOS and Ubuntu.

## Test targets (macOS)

| Crate | Test target | Passed | Failed | Ignored |
| --- | --- | ---: | ---: | ---: |
| `blueice-bluets` | `unittests src/lib.rs` | 105 | 0 | 0 |
| `blueice-bluets` | `unittests src/bin/bluets-test-interface.rs` | 0 | 0 | 0 |
| `blueice-bluets` | `unittests src/bin/bluetsc.rs` | 10 | 0 | 0 |
| `blueice-bluets` | `tests/cli.rs` | 6 | 0 | 0 |
| `blueice-bluets` | `tests/coverage_bluets_cli.rs` | 18 | 0 | 0 |
| `blueice-bluets` | `tests/coverage_bluets_contracts.rs` | 16 | 0 | 0 |
| `blueice-bluets` | `tests/coverage_bluets_frontend.rs` | 12 | 0 | 0 |
| `blueice-bluets` | `tests/coverage_bluets_test_interface.rs` | 12 | 0 | 0 |
| `blueice-bluets` | `tests/test_interface.rs` | 1 | 0 | 0 |
| `blueice-bluets` | `tests/typescript_oracle.rs` | 0 | 0 | 1 |
| `blueice-bluets-bluejs` | `unittests src/lib.rs` | 75 | 0 | 0 |

The target list and counts are the same on the other platforms measured. The `coverage_bluets_*` files are table-driven suites that exercise the crate through its public boundary (the `compile` entry point, the CLI and the test-interface protocol); `cli.rs` and `test_interface.rs` drive the real `bluetsc` and `bluets-test-interface` binaries as subprocesses.

## TypeScript 5.9.3 compatibility oracle

The oracle (`backend/bluets/tests/typescript_oracle.rs`, ignored by default, enabled with `BLUEICE_BLUETSC_ORACLE=tsc`) runs **68 cases** built from **71 module sources**: **48 compile-and-run cases**, whose BlueTSC-emitted JavaScript is executed and whose stdout must equal the output of the JavaScript TypeScript 5.9.3 emits, and **20 diagnostic-parity cases**, whose expected BlueTS diagnostic code and line must match the code and line TypeScript reports. It runs against Node 24.21.0 and `typescript@5.9.3` installed through `npm exec`, and is also CI's `BlueTSC TypeScript compatibility oracle` job.

Compile-and-run cases: `generic-property`, `optional-default`, `optional-parameter-expression`, `default-parameter-expression`, `optional-record`, `generic-declaration-module`, `generic-constraint-default-declaration-module`, `explicit-generic-call`, `generic-interface-heritage`, `generic-interface-heritage-declaration-module`, `function-overload`, `generic-function-overload`, `default-function-export`, `default-value-export`, `named-value-export`, `boolean-conditional-expression`, `typeof-void-expression`, `nullish-coalescing-expression`, `bitwise-shift-expression`, `exponentiation-expression`, `compound-assignment-expression`, `update-expression`, `comma-sequence-expression`, `array-literal-expression`, `array-spread-expression`, `object-literal-property-expression`, `object-spread-expression`, `template-literal-expression`, `template-substitution-expression`, `property-assignment-expression`, `function-expression-statement`, `function-throw-statement`, `function-braced-if-statement`, `function-braced-else-if-statement`, `object-shorthand-expression`, `string-escape-expression`, `template-escape-expression`, `template-identifier-expression`, `object-literal-key-expression`, `computed-object-property-expression`, `member-call-expression`, `constructor-expression`, `spread-argument-expression`, `rest-parameter-expression`, `property-delete-expression`, `property-update-expression`, `relational-membership-expression`, `generic-arithmetic-expression`.

Diagnostic-parity cases: `arithmetic-operand-error`, `bitwise-operand-error`, `exponentiation-operand-error`, `unary-exponentiation-base-error`, `strict-equality-disjoint-primitive-error`, `typeof-assignment-error`, `nullish-coalescing-assignment-error`, `nullish-logical-mixing-error`, `assignment-error`, `call-argument-error`, `function-expression-statement-call-error`, `function-throw-statement-error`, `function-braced-if-statement-call-error`, `function-braced-else-if-statement-call-error`, `optional-record-error`, `generic-constraint-error`, `explicit-generic-constraint-error`, `generic-interface-heritage-error`, `interface-heritage-override-error`, `function-overload-error`.

## Line coverage by file

From the workspace `cargo llvm-cov` run on macOS (the per-file lines are identical on Ubuntu).

### `blueice-bluets`: 93.17% (7,636 / 8,196 lines)

| Source file | Lines | Missed | Coverage |
| --- | ---: | ---: | ---: |
| `checker.rs` | 1,172 | 177 | 84.90% |
| `emitter.rs` | 861 | 81 | 90.59% |
| `parser/declarations.rs` | 1,084 | 72 | 93.36% |
| `checker/module/binding.rs` | 811 | 45 | 94.45% |
| `authorized_loader.rs` | 165 | 36 | 78.18% |
| `bin/bluetsc.rs` | 872 | 36 | 95.87% |
| `parser/runtime_syntax.rs` | 280 | 25 | 91.07% |
| `compiler.rs` | 943 | 23 | 97.56% |
| `syntax.rs` | 244 | 17 | 93.03% |
| `checker/module/expressions.rs` | 595 | 16 | 97.31% |
| `checker/project.rs` | 179 | 12 | 93.30% |
| `parser.rs` | 43 | 10 | 76.74% |
| `contracts.rs` | 427 | 6 | 98.59% |
| `parser/type_syntax.rs` | 265 | 2 | 99.25% |
| `bin/bluets-test-interface.rs` | 130 | 1 | 99.23% |
| `diagnostic.rs` | 31 | 1 | 96.77% |
| `debug_info.rs` | 88 | 0 | 100.00% |
| `lib.rs` | 6 | 0 | 100.00% |

### `blueice-bluets-bluejs`: 81.85% (1,804 / 2,204 lines)

| Source file | Lines | Missed | Coverage |
| --- | ---: | ---: | ---: |
| `lib.rs` | 868 | 209 | 75.92% |
| `expression.rs` | 939 | 141 | 84.98% |
| `debug_attachment.rs` | 160 | 30 | 81.25% |
| `page_runtime.rs` | 237 | 20 | 91.56% |

`blueice-bluets-bluejs` is the least covered BlueTS crate. The CI gate applies to the whole workspace (90%) and to `blueice-bluejs` (88%), not per crate, so this does not fail CI, but it is the largest coverage gap in the BlueTS work and should be closed with public-boundary tests of the bridge before further Phase 18 expansion.

## Windows-specific finding

The Windows CI legs of the merge commit `eaeb5c1` failed in `blueice-engine`: `script::host_typings::tests::current_empty_profile_matches_the_checked_in_artifacts` compared the freshly generated `lib.blueice.d.ts` (LF) with the checked-in artifact, which a Windows checkout with `core.autocrlf=true` had converted to CRLF. Those generated host-typing artifacts are hash-verified and must be byte-identical everywhere, so a root `.gitattributes` now pins `lib.blueice.d.ts`, `lib.blueice.manifest.json` and `backend/core/engine/tests/fixtures/host_typings/**` to `eol=lf`. Because `cargo test` stops at the first failing crate, any later Windows-only failure would have been hidden behind it; the Windows run recorded here uses `--no-fail-fast` on a tree converted to CRLF to reproduce CI's checkout.

## Reproduce

```sh
cargo test -p blueice-bluets --no-fail-fast
cargo test -p blueice-bluets-bluejs --no-fail-fast
npm exec --yes --package typescript@5.9.3 -- env BLUEICE_BLUETSC_ORACLE=tsc \
  cargo test -p blueice-bluets --test typescript_oracle -- --ignored
cargo llvm-cov --workspace --summary-only   # per-file rows for the two crates
```

The oracle needs Node 24 and network access to install `typescript@5.9.3`; on Windows set `BLUEICE_BLUETSC_ORACLE=tsc` in the environment instead of using `env`.
