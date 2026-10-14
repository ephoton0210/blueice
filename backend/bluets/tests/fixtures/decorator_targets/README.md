# K.7.4 decorator target witnesses

The corpus contains 880 original programs: 20 forms, eleven targets from ES5 to
ESNext, CommonJS and ESNext modules, and both class-field modes. TypeScript 5.9.3
accepts 816 configurations and rejects 64. Rejected programs retain their exact
diagnostics and publish no artifacts with `noEmitOnError`.

Every accepted program has exact native declaration output and actual emitted
JavaScript execution evidence. Of these, 782 execute directly under Node. The
remaining 34 ESNext outputs preserve decorators or auto-accessors that the current
Node and Acorn versions cannot parse. Their raw JavaScript, native parser verdicts,
proposal feature counts and Node syntax errors remain recorded.

For those 34 outputs, a test adapter lowers the actual emitted JavaScript with
TypeScript 5.9.3 to ES2023 and executes that result. It never recompiles the original
TypeScript source. The public BlueTSC replay applies the same adapter to actual
BlueTSC output and compares proposal preservation and observed effects. This is
execution evidence for the emitted proposals through a recorded adapter; it does
not assert direct Node or BlueJS support for proposal syntax.

`record_decorator_targets.cjs` replays the live pinned compiler, validates source
hashes and compares the complete reference. Set
`BLUEICE_WRITE_DECORATOR_TARGETS_MATRIX=1` to regenerate the reference and verdict
matrix. `observe.cjs` validates target syntax, parses the actual JavaScript,
executes it and reports direct or adapted observations.

Forms cover standard class/method/getter/setter/field decorators, auto-accessors,
decorated auto-accessors and private members, computed names, namespace and
default-export classes, static `super`, class expressions, legacy declarations,
constructor parameter properties and legacy metadata. Fixtures use the unique
`decemit-` prefix so unrelated matrices do not claim them.
