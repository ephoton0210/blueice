# P0-C string-intrinsic fixtures

`cases.json` contains 245 independently authored forms under four compiler
profiles. `reference.json` freezes TypeScript 5.9.3's 980 verdicts, complete
diagnostics and UTF-16 positions, 804 physical declaration files, 1,544 isolated
consumer results and 16 executions. `FROZEN.json` retains the original hashes
and the then-pending P0-B source; later acceptance must record its actual source
separately. Never regenerate the reference to match BlueTS output.

`boundary-controls.json` contains 20 separate owner, runtime-authority and
configured-resource controls. They do not enter direct Native agreement.
`utf16-native-vectors.rs` retains 94 independently measured semantic values and
physical literal tokens. `utf16-native-escapes.rs` adds three parser calibration
vectors outside the primary 980 denominator.

`template-native-vectors.rs` adds ten membership and two inference observations;
`template-numeric-vectors.rs` adds 52 placeholder observations. They calibrate
code-unit splitting, delimiter selection and numeric string validation as unit
regressions; they do not increase the primary comparison denominator.

The public Rust harness is `tests/string_intrinsics.rs`. Its ordinary tests use
the actual BlueTSC CLI for all 980 projects and the public compiler API for all
20 separate controls. The ignored test runs the pinned Native recorder in
read-only `--verify` mode, then checks the actual compiler's complete diagnostics,
declarations, isolated consumers, selected-edition syntax and execution through
`verify_string_intrinsics.cjs`. Supply `BLUEICE_BLUETSC_ORACLE` and
`BLUEICE_ACORN_ORACLE`; optional `BLUEICE_STRING_INTRINSICS_OBSERVATIONS` retains
raw projects, outputs and reports. A reference-only calibration or isolated core
test does not establish BlueTSC acceptance.

`literal-context-regressions.json` separately retains ten Native observations
for generated-literal array/tuple diagnostics and accepted array/call/overload
controls. Its native compiler, library and original observation hashes were
recorded before the array diagnostic repair. The CLI regression compares full
diagnostics, positions and physical declarations. These rows add no credit to
the frozen 980 primary or 20 policy assertions.
