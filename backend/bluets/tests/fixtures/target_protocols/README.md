# Target helper protocol boundaries

`native.mjs` is original source for 37 observable iterator/generator boundaries.
`input.ts` adds only static annotations and a console declaration. The recorder
checks and emits it with TypeScript 5.9.3 for eleven targets and both existing
module kinds, executes every output under Node and checks its Acorn 8.15.0
syntax edition. It records all 22 accepted verdicts and exact declarations.

The native observations exercise close getter/call failures, non-object close
results, awaited rejection/settlement, throw/return/break precedence,
generator return/throw before first execution, reentrancy and completed state.
Dynamic iterator receivers are explicitly `any`; malformed return values
are intentional runtime protocol controls.

The semantic basis is ECMA-262 2024:
[IteratorClose](https://tc39.es/ecma262/2024/multipage/abstract-operations.html#sec-iteratorclose),
[AsyncIteratorClose](https://tc39.es/ecma262/2024/multipage/abstract-operations.html#sec-asynciteratorclose)
and [GeneratorResumeAbrupt](https://tc39.es/ecma262/2024/multipage/control-abstraction-objects.html#sec-generatorresumeabrupt).
The recorder independently asserts native exception priority, closing calls,
awaited settlement and generator results.

Pinned TypeScript has 20 measured differences in eight emitted programs:
ES5 ignores a primitive synchronous close result after break/return; ES5,
ES2015, ES2016 and ES2017 ignore a primitive async close result. Both module
kinds are affected. `reference.json` preserves those actual upstream outputs.
The authored BlueTS runtime test uses native observations as its semantic
expectation. Existing target parity fixtures retain their exact TypeScript
expectations; no allowance is added to those fixtures.

Run `development/browser_core/phase-18-bluets/tools/record_target_protocols.cjs`
with `BLUEICE_BLUETSC_ORACLE` and `BLUEICE_ACORN_ORACLE` selecting the pinned
tools. Set `BLUEICE_WRITE_TARGET_PROTOCOLS=1` only to regenerate the source and
record; otherwise the recorder compares both byte-for-byte. Upstream helper
implementations stay in temporary output and are removed after recording.
