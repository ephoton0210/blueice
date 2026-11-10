# Callable and constructor utilities — G-T7 / G-T2

The original 52 authored forms run under three fixed strict profiles:
ES5/CommonJS with ES5 libraries, ES2022/CommonJS with ES2022 libraries, and
ES2022/ESNext with ES5 libraries. The 156 observations include 75 accepts and
81 rejects. `reference.json` was recorded independently with pinned TypeScript
5.9.3 before the corresponding BlueTS implementations; its input, compiler and
selected library hashes are checked on every Native replay.

The ordinary public CLI test requires all verdicts, complete primary/related
messages and UTF-16 positions, all accepted declaration bytes, and no publication
from rejected `noEmitOnError` builds. The ignored pinned-tool test additionally
reproduces every Native result, consumes only emitted `.d.ts` files from separate
source-free directories (150 observations), parses all 75 accepted JavaScript
outputs with Acorn 8.15.0 at their selected edition, and compares six actual
Node executions. Runtime forms explicitly supply their console declaration.

The equivalent local aliases exercise general conditional/infer behavior rather
than a built-in-name shortcut. Nine separate owner/local/runtime checks require
owner precedence, exact authorized source graphs, and the absence of implicit
runtime bindings; they do not add direct Native agreement cases.

To record/check the Native fixture, run
`development/browser_core/phase-18-bluets/tools/record_callable_utilities.cjs`
with `BLUEICE_BLUETSC_ORACLE` pointing to pinned 5.9.3. Only the explicit
`BLUEICE_WRITE_CALLABLE_UTILITIES=1` mode writes the reference. Ordinary and
acceptance replays never enable it. `BLUEICE_CALLABLE_UTILITIES_OBSERVATIONS`
retains source, Native/BlueTS outputs, diagnostics and consumer/runtime receipts.
`verify_callable_utilities.cjs` also requires the actual compiler in
`BLUEICE_CALLABLE_UTILITIES_COMPILER` and pinned `BLUEICE_ACORN_ORACLE`.

This finite package does not close the broader library/operator rows or claim
full TypeScript parity. Higher-rank constructor-pattern binders, arbitrary
recursive/dependent compositions, other flag combinations and the remaining
standard-library catalog are outside this versioned case list.
