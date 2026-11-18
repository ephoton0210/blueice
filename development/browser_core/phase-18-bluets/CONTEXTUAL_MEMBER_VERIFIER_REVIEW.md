# P0-I — Native ES5 concise-arrow grammar

Status: **Correction applied after public red; complete public helper suites pass.
Full acceptance with the corrected tool bytes is running. P0-I remains unchecked.**

The first source61 Native run finished with77 successful suites and two failed
suites; the remaining suites were not scheduled after failure. No case
measurement was published. Both failures were the helper-collision verifier's
requirement for an additional generated pair of parentheses, across75 of its363
unchanged configurations.

The frozen Native output and current BlueTS output have the same callback:

```js
function (value) { return value + tslib_1 + __blueice_target_generator_0; }
```

The old verifier required BlueTS to emit a parenthesized return expression,
even when Native did not. A new public unit test feeds the unchanged frozen
Native program/map into that verifier. Before correction, it fails with
`closed generated concise-arrow precedence grouping`:7 of8 unit tests pass.
The red test source, old verifier and log are retained under
`target/contextual-member-acceptance-source61-20261117-1/verifier-arrow-review/`.

The correction accepts the direct Native return expression. If an additional
group exists, both delimiters still must be unmapped generated characters.
The source's complete binary-expression shape, identifiers/operators, every
callable token and source role, expression endpoint and callable endpoint retain
their previous checks. The complete-module comparator and mutation suite are
unchanged. No compiler or frozen source, expected output, map segment, resource
budget or owner authority changes.

The only maintained original tools are:

- `tools/ge5_helper_collision_arrow.cjs`: accept the Native direct-return grammar.
- `tools/ge5_helper_collision_frames.test.cjs`: retain all four old tests and add
  four frozen-Native/direct-expression regressions, including wrong source role,
  wrong endpoint and changed operator rejection.

All8 unit tests now pass. Both unchanged public Rust suites independently rerun
all363 configurations on the actual compiler; both pass without ignored or
filtered functions. The full-module suite preserves25,822 negative mutation
checks. The public result is
`verifier-arrow-review/public-focused/result.json`; its captured observations
and reports retain every emitted file and map. Private diagnostic probes have
no formal acceptance credit.

The compiler is unchanged:
`backend-tree:0f91c383d4f5471683d6d69638248769b9ed7534`, executable SHA256
`cd1da98a5c36b974ecc5e35d49c0032b0a4f24d80a9bad5f281364e6f773fc17`.
All618 previous tool names remain;616 files are byte identical and the two
maintained files have explicit before/after hashes. Four additional Node unit
tests add no compatibility-inventory credit. A new621-tool snapshot and fresh
Native192/workspace/original-profile acceptance run are under
`target/contextual-member-acceptance-source61-tools62-20261117-1/`.
Both formerly failing suites also pass inside that formal Native run, with no
ignored/filtered functions and all25,822 mutation checks retained. The
`formal-helper-collision-checkpoint.json` records those exact suite logs and
the complete363-configuration report; the rest of that Native192 attempt was later stopped for the production regression described below.

The existing Windows job tests this same backend/compiler. The independent
`windows-source-dependency-binding.json` under the new acceptance root verifies
all14,227 backend files and Cargo inputs,245 unchanged accepted dependency files,
and the three new contextual tools through an AST require closure. Neither
maintained verifier is reachable from the selected Windows runtime tools. Its
only changed original Rust entrypoint updates the diagnostic version literal;
the new contextual entrypoint invokes the two audited contextual verifiers.
The Windows archive retains the tool bytes actually used by that job. Complete
82/25/72 execution and the sealed-archive audit remain required; no Windows
completion credit is granted: the job ended with a real ES5/CommonJS emission failure after44 complete cohorts. Core82 and25 paths did not run. The same invalid JavaScript was reproduced locally; source62 corrects shorthand-prefix coordination. Both original helper verifiers remain strict. See [current follow-up](CONTEXTUAL_MEMBER_FOLLOWUP.md) for the corrected-source validation status.
