# P0-D — Frozen reverse mapped inference

Pinned Native TypeScript 5.9.3 expectations were recorded before compiler changes.
`FROZEN.json` binds all three input/reference/control files. The 101 authored
entries across ES5/ES2022 and CommonJS/ESNext, always strict, produce 404 complete
observations: 358 accepts, 46 rejects, 366 exact declaration files, 716 isolated
positive/negative declaration consumers and eight executions. The entries are
assertions, not unique programs or a language coverage percentage.

The original 16 discovery programs remain unchanged. Additional cases cover
array/tuple variables and literals, readonly/optional/rest/labeled shapes,
spreads, nested/function/array/tuple wrappers, aliases and imported results,
mapped modifiers, explicit arguments, contextual callbacks, flags and contextual
binding words. `require` is rejected in CommonJS but accepted in ESNext; actual
reserved words retain their complete Native recovery diagnostics.

Positive consumer annotations come exclusively from the independently emitted
Native result type and are frozen in `reference.json`, along with the source and
complete diagnostics of each consumer. Consumers select only emitted declarations.
They do not use a BlueTS-inferred type as their expectation. Executable cases
must produce the value 2; each actual JavaScript file also needs an independent
selected-edition syntax check during acceptance.

Twelve BlueTS controls are separate: two ambient owners, two runtime-authority
refusals, six zero/one-work refusals and two parser-depth refusals. They add no
Native agreement credit. Normal controls must produce output; limited/refused
controls must not produce output or disguise a resource limit as a Native error.

The unchanged accepted P0-C compiler initially agrees on 70/404 observations;
334 differences are retained under `target/mapped-inference-design-20261113/`.
All 12 separate controls pass. An earlier authoring test incorrectly excluded the
ambient declaration module from the project graph; that raw failure and its test
source are retained. The correction requires both main and owner modules and
changes no Native expectation.

Thirty-three supplemental observations have zero primary/inventory credit: four
literal-preservation comparisons, five projection-owner controls, four lexical
identity/alias controls, four mutation/escape controls and four independently
calibrated generic-map callback comparisons, plus twelve generic-argument controls. Native accepts all
thirteen projection programs; ten are intentional BlueTS owner refusals, not
Native agreements. The same-parameter positives must retain exact declarations.
Alias proofs require lexical binding identity and an unobserved, unannotated
local const initializer. Other value references, captures, writes or escapes
invalidate that proof. All supplemental bytes are pinned in `FROZEN.json`.
The callback controls exercise contextual and ordinary parameter names under
returned generic methods, with exact declarations. Their Native replay verifies
the unchanged TypeScript implementation and library hashes separately from the
404 primary observations. Flow retains each parameter's declaring scope and
constraint before initializer inference; same-parameter `keyof` relations compare
declaration identities before expanding constraints.

Complete same-source Native/local/Windows, map, project and tooling acceptance
remains required before checking and committing P0-D. All 22 broader G-* rows
remain open and full TypeScript parity remains OFF.

The twelve generic-argument controls freeze seven actual declaration regressions
against accepted C, three independently matching positives and two contextual
binder guards. They were
calibrated before the corresponding production repair. The first public CLI test
reproduces all seven regressions with unchanged production; the twelve Native replays
pass. These observations have zero primary/inventory credit and do not establish
completed acceptance. The third formal candidate is retired; its full Windows
results and partial Native results stay bound to that retired source.
