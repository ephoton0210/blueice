# Test262 verification

This POSIX runner inventories every test in the pinned official snapshot, including staging/proposals and ECMA-402. It reports every requested execution mode. **A full inventory run is not a passing conformance result.** Static module graphs, Module Namespace Exotic Objects, literal dynamic imports and a bounded top-level-await job path are implemented; complete async execution, thenable assimilation, host loading and much of the language remain unsupported.

Prerequisites: Rust, Python 3, and the pinned Python dependency below. Node is only required by the separate differential oracle. The default Rust subprocess fault tests also use Python 3 on POSIX.

```sh
python3 -m venv /tmp/bluejs-conformance-venv
/tmp/bluejs-conformance-venv/bin/pip install -r backend/bluejs/test262/requirements.txt
cargo build -p blueice-bluejs --bins --offline
/tmp/bluejs-conformance-venv/bin/python -m unittest discover -s backend/bluejs/test262 -v
/tmp/bluejs-conformance-venv/bin/python backend/bluejs/test262/run.py --fetch --corpus /tmp/blueice-test262-72faf8ec
```

The workspace development and test profiles compile `blueice-bluejs` at
`opt-level = 3`. This is intentional: the adapter is a bytecode interpreter,
and an unoptimised interpreter makes ordinary finite `testIntl.js` matrices
miss the default wall-clock gate. The command above therefore produces the
optimised default `target/debug/bluejs-test262`; do not substitute a separately
compiled release binary just to make a conformance run finish.

Use `--fetch` only when the corpus destination does not exist; it verifies the archive and complete file manifest against `snapshot.json`. It then starts the requested inventory. Subsequent runs omit `--fetch` but retain the same `--corpus` path. If a pre-existing corpus was fetched for an older snapshot, do not replace or edit it: fetch the current snapshot into a new directory, as above. Every run checks all 56,979 archive files, rejects additional test files, and records executable and runner hashes. The archive contains 54,026 JavaScript resources: 53,582 test files and 294 `_FIXTURE` resources. The snapshot selection is time based, not an assertion that every proposal belongs to ECMA-262 edition 17.

Outputs are `target/test262/results.jsonl` (one record per test/mode, source hash, features, expected and actual phase/type/status) and `summary.json` (counts and feature/group breakdown). `--filter intl402/Collator` runs a clearly labelled partial inventory. `--exclude intl402/Temporal/` removes one or comma-separated path substrings after filtering; both selection strings are retained in the summary and therefore cannot be mistaken for a complete inventory. A selection that contains no tests is a configuration error. `--jobs` defaults to the smaller of eight and the host's logical CPU count (never fewer than one), so a six-vCPU Linux worker does not turn the ordinary per-case deadline into scheduler-delay detection; pass `--jobs` to override it deliberately. `--timeout` defaults to 2 seconds and `--instruction-budget` to 100,000 VM dispatches per mode. Every five seconds the runner reports completed files, result counts, and each active `path [mode, elapsed]`; use `--progress-interval 0` for non-interactive output or a different positive interval for more or less detail. Exit 0 requires every scheduled mode to pass; exit 1 means non-passing cases and exit 2 indicates a configuration/argument error. Unexpected Python errors terminate with a traceback and must not be treated as a completed report.

A mode's status is one of `pass`, `fail`, `timeout`, `harness_error`, `unsupported`, `excluded` or `stale_corpus`. `unsupported` means the adapter tried the fixture and reported a capability BlueJS's engine genuinely lacks (a missing host hook, an unsupported harness include). `excluded` is a different, narrower thing: it means the fixture's own Test262 metadata declares it applicable only to a host with the opposite of a capability this host has deliberately chosen, so the runner never dispatches it to the adapter at all. The only current case is `CanBlockIsTrue`/`CanBlockIsFalse` (`HOST_CAN_BLOCK` in `run.py`): BlueJS's `Atomics.wait` genuinely suspends the agent, so every `CanBlockIsTrue` fixture is dispatched and already passes, while a `CanBlockIsFalse` fixture is excluded — making it pass instead would mean making `Atomics.wait` always throw, which would break every `CanBlockIsTrue` fixture. `stale_corpus` is narrower still and orthogonal to both: it means a fixture's own assertions were verified, directly against the live ECMA-262 draft text (not merely inferred from disagreement), to contradict the *current* spec — and that upstream Test262 has already independently identified and drafted a fix for exactly that contradiction. This host is not missing a capability (`unsupported`) or declaring an incompatible one (`excluded`); it already matches the current draft the same way every other conformant engine does, and the fixture is expected to start passing on its own, with no BlueJS change, once the upstream fix lands. Each entry in `STALE_CORPUS_FIXTURES` (`run.py`) is exact-path only and must cite both the live spec section verified and the upstream Test262 issue/PR — never "we disagree with this test" as its own justification. The only current case is `annexB/language/function-code/block-decl-func-skip-arguments.js` (verified against `FunctionDeclarationInstantiation`'s Annex B web-compat insertion point; see `tc39/test262#5113`/`#5112`). All three of `unsupported`/`excluded`/`stale_corpus` still count as non-passing for the exit code above; the distinction is for triage (`TEST262_ANALYSIS_REPORT.md`'s `host-capability-declared` and `corpus-stale-pending-upstream-fix` blockers vs. `compiler-unsupported`), not for hiding any of them from the inventory.

The adapter creates a fresh VM for each case. Raw source is unchanged and receives no assertion harness. Other cases receive native overrides for `sta.js` and `assert.js`; the native `assert` family follows the upstream source step for step (same messages, coercions and exceptions), stays native because a JavaScript `assert.js` costs enough dispatches per assertion to push loop-driven fixtures such as `built-ins/Math/sqrt/results.js` past the ordinary budget, and is checked against the unchanged upstream `assert.js` by a differential crate test (`tests/test262_host/harness.rs`). `propertyHelper.js` and `isConstructor.js` are not overridden: they run their upstream source like any other include, so `verifyProperty` really attempts the deletion, write and `for-in` probes, honours `restore`, and reports upstream's messages. The one pinned NumberFormat precision matrix has the separately documented, source-shape-checked native adapter. The sole `formatRangeToParts/temporal-objects-resolved-time-zone.js` fixture also retains a Test262-only iterative `assert.deepEqual` for arrays of plain part records: its checked shape compares array length, record keys, and every primitive value, has positive and mismatch regressions, and avoids exhausting the VM's finite recursive-call resource in the upstream general-purpose helper. All other `deepEqual.js` includes run the upstream source unchanged. The generated CharacterClassEscape fixtures preserve their original full-string `RegExp.prototype.test` calls, but use a native helper to stop at the first failed result instead of running their diagnostic loop once per code point. The three staging JIT stress fixtures retain one complete semantic iteration; the TypedArray helper calls the real `set` builtin and checks the entire result without one interpreter dispatch per element. Remaining includes execute as separate classic scripts in the same VM realm: successful top-level `var` and function declarations publish to the global object while lexical declarations stay script-local. Harness assertions and descriptor helpers fail closed, compare signed zero and NaN correctly, and keep resource failures distinct from JavaScript exceptions. The `IsHTMLDDA` feature installs a Test262-only Annex B host exotic in that fresh realm; it does not expose the hook to unmarked tests. Static modules run with module strictness, undefined top-level `this`, and declarations that do not publish classic global properties. The runner supplies reachable relative static and literal-dynamic fixture sources to the adapter; it links named/default/namespace imports, local/indirect/star exports and cycles before evaluation. A request's `with { type }` attribute (static declaration or literal `import()`) selects how its target is loaded: `json`, `text` (WHATWG UTF-8 decode) and `bytes` (raw bytes) resources are supplied to the adapter as data keyed by path and type, never parsed as JavaScript, so one file can be both a module and, for example, its own text (`import-attributes/text-self.js`). A test that names the host Module Source specifier `<module source>` in a static `import source` or a dynamic `import.source()` has it registered as a source-phase record. Module namespaces have live export-cell descriptors, `null` prototype, UTF-16 export ordering, `Symbol.toStringTag`, symbol-key handling and non-extensibility. Dynamic import resolves only in the supplied registry, completes in the Promise job queue, and caches one namespace object per realm module. Top-level `await`, ordinary async functions, and independent async-generator requests preserve their suspended frames across pending Promise jobs; fulfilled awaits also resume in a later job turn. Async generators expose `next`/`return`/`throw`, await yielded values, drive `for await`, and delegate `yield*` through sync or async iterator methods. Host-driven module loading remains a separate unimplemented suspension boundary. Missing `$262`, `$DONE` and `print` hooks cannot satisfy negative ReferenceError tests. The subset parser's unclassified rejections never count as successful negative SyntaxError tests. Arbitrary thrown values currently report `ThrownValue`; they are not guessed to have the expected error class. Setting `BLUEJS_TEST262_NURSERY_CAPACITY=1` in the environment runs every case with a one-object nursery (GC stress): each allocation may collect, so a native function that leaves an object unrooted fails deterministically; outcomes must equal an ordinary run. A major collection is triggered by managed-byte growth (for example a property write that grows an object), which the one-object nursery does not move, so a value left unrooted across such a write can survive that mode: additionally setting `BLUEJS_TEST262_MAJOR_THRESHOLD=<bytes>` (for instance a sweep from 20000 to 300000) changes where the first major collection lands, and outcomes must again equal an ordinary run. The baseline interpreter budget is 100,000 dispatches; fixtures declaring `tail-call-optimization` receive at least 3,000,000 dispatches so Test262's required 100,000 tail calls are measured without removing the bounded-resource guard.

The two-second wall deadline remains the default. The pinned
`intl402/NumberFormat/test-option-roundingPriority-mixed-options.js` fixture
uses a Test262-only native adapter for its fixed 5,040-output matrix: every
formatter still follows the VM option and number-format bridges, while the
adapter replaces only the helper's repeated `forEach`/RegExp/string-replace
diagnostics. It keeps the ordinary 100,000-dispatch and two-second per-mode
limits; the runner rejects a changed helper call rather than silently applying
the adapter. The exact immutable finite
stress-fixture list in the runner receives a 90-second per-mode deadline for
its documented TypedArray and interpreter stress loops; that list includes
`ArrayBuffer/prototype/sliceToImmutable/argument-coercion.js`, whose
argument-coercion matrix is finite but far above the default fuel. The three
`TypedArray/prototype/copyWithin/coerced-values-{start,end}-detached*.js`
fixtures have their own exact-path 50,000,000-dispatch / 120-second envelope
(`TYPED_ARRAY_DETACH_COERCION_FIXTURES`): the shared `testTypedArray.js`
byte-copy loop runs about 27 million dispatches for them, measured at about
27 seconds per mode on an idle debug adapter and about 50 seconds under load.
Sibling fixtures keep the generic 10,000,000-dispatch / 60-second TypedArray
harness envelope. `dynamic-import/await-import-evaluation.js`
(`WALL_CLOCK_BUSY_WAIT_FIXTURES`) has an exact-path 10,000,000-dispatch budget
and keeps the ordinary two-second wall deadline: its fixture spins until
`Date.now()` has advanced 100 ms, which costs about 0.3M-1M dispatches here
and proportionally more on a faster host, so no fixed default fits it; the
allowance is an order of magnitude over the measurement and applies to no
other path.
`annexB/built-ins/String/prototype/substr/start-and-length-as-numbers.js`
(`STRING_SUBSTR_NUMBER_MATRIX_FIXTURES`) checks `substr` against a reference
implementation for a fixed matrix of 4 strings x 35 starts x 36 lengths
(5,040 calls, each with a per-character comparison loop and several
assertions). It needs 1,496,386 dispatches, identically in both modes, so it
gets an exact-path 6,000,000-dispatch allowance (4x the measurement, the
factor the Temporal table fixtures use) while keeping the ordinary two-second
wall deadline (it takes under a second at that cost); `test_runner.py` pins
that a sibling `substr` path and a same-named path elsewhere keep the
default.
Four fixed, finite staging fixtures whose size only just exceeds the default
share one exact-path table, `FINITE_FIXTURE_INSTRUCTION_BUDGETS`, each entry
4x its measured minimum (identical in sloppy and strict mode) and each
finishing in a small fraction of a second, so the ordinary two-second wall
deadline stays in force: `staging/sm/Array/with-dense.js` (178,125
dispatches, 750,000 allowed), `staging/sm/JSON/parse-reviver-array-delete.js`
(185,937; 750,000), `staging/sm/Math/log2-approx.js` (325,000; 1,300,000) and
`staging/sm/extensions/es5ish-defineGetter-defineSetter.js` (110,156;
450,000). The table is not a way to admit slow fixtures: a fixture that is too
slow for the wall deadline even with unlimited fuel (for example
`staging/sm/Array/toSpliced-dense.js`, `staging/sm/Date/two-digit-years.js`
under load, and every `staging/sm/Date/dst-offset-caching-N-of-8.js` part)
is deliberately absent and stays a reported failure.
`staging/sm/String/unicode-braced.js` is a resource-size case rather than a
dispatch one: it evaluates a source string built from 2**24 zeros (32 MiB of
UTF-16), so it needs a string limit of at least 33,558,528 bytes against the
ordinary 1 MiB, and then runs in about a second with the default dispatch budget
and heap. `FIXTURE_STRING_LIMITS` gives that exact path a 64 MiB string limit
(twice the requirement; a data size gains nothing from more headroom) beside
the existing 8 MiB limits of the RegExp property-escape fixtures. The other
string-limit failures (`staging/sm/String/replace-math.js`, which builds 2**36
units and expects a catchable out-of-memory error, `staging/sm/JSON/parse-mega-huge-array.js`,
`staging/sm/RegExp/unicode-class-braced.js`, `staging/sm/regress/regress-610026.js`)
are not admitted: they need hundreds of megabytes or gigabytes, tens of seconds,
or a catchable language-level string-length error the host does not provide. The exact
`Function/prototype/toString/built-in-function-object.js` graph traversal has
its separately measured 180-second bound, and the two exact RegExp
match-indices warm-up fixtures have 30 seconds; neither broadens the ordinary
deadline. The one
`supportedLocalesOf-unicode-extensions-ignored.js` matrix has its own bounded
180-second envelope: it invokes every Intl constructor across both matching
strategies and three Unicode-extension spellings for every locale. The seven exact
historical RegExp BMP enumerations use bounded native adapters that retain
their matcher validation without dispatching one JavaScript eval per unit.
Cases including Test262's
`testTypedArray.js` receive a 60-second wall allowance because that shared
harness deliberately invokes a callback across all numeric constructors and
the byte-conversion matrix. They retain the ordinary bounded interpreter
budget, so this allowance cannot turn an unbounded execution into a pass.

The two immutable `formatToParts/compare-to-temporal` calendar matrices are
separately bounded at 10,000,000 dispatches and 360 seconds per mode. They
exercise a hundred years of non-ISO calendar conversion, including lunisolar
leap months, and must not cause the ordinary Intl or Temporal timeout policy
to become permissive.

Seven exact intl402 Temporal fixtures walk a fixed calendar table through the
real `Temporal.*.from` path and receive a 2,000,000-dispatch allowance (the
default is 100,000): `PlainDate/from/hebrew-keviah.js`,
`PlainDate/from/persian-new-year-dates.js`, the two
`roundtrip-from-property-bag.js` fixtures, and the three
`{PlainDate,PlainDateTime,ZonedDateTime}/prototype/dayOfYear/
non-iso-calendar-basic.js` fixtures, which step through every day of one year
in fifteen calendars (about 240,000 dispatches at minimum). The list is exact
and pinned by `test_runner.py`; a neighbouring fixture keeps the default, and an
unbounded loop still exhausts the allowance.

Four finite `staging/sm` fixtures need more than the default per-string
(1 MiB), managed-heap (16 MiB, which also caps an ArrayBuffer), dispatch or
wall-clock ceiling and have exact-path allowances in `LARGE_FIXTURE_RESOURCES`
("Large finite fixtures" below): every value is 3-4x the measured minimum on the
optimised adapter and never unlimited, `test_runner.py` pins the table and
that a neighbouring or same-named path keeps every default, and a larger
caller-supplied default is never reduced.

| Fixture | Real size (measured minimum) | Allowance |
| --- | --- | --- |
| `extensions/dataview.js` | `new ArrayBuffer(20 * 1024 * 1024)`; heap 21 MiB | heap 64 MiB |
| `RegExp/unicode-braced.js`, `RegExp/unicode-class-braced.js` | a 2**24-zero braced escape in a regular expression built by eval and by `RegExp`; 32 MiB strings, heap 36 MiB, about 2.2 s idle (3-4 s under the runner) | string 128 MiB, heap 128 MiB, 20 s |
| `regress/regress-610026.js` | eval of 2**21, about 2**22 and about 2**22 empty blocks; 16 MiB strings, heap 36 MiB, 20-30 million dispatches, about 25 s per mode | string 64 MiB, heap 128 MiB, 100,000,000 dispatches, 90 s |

The regular-expression pair also depends on the matcher request being small:
`strip_braced_leading_zeros` removes runs of leading zeros of a braced escape
(with the `u` or `v` flag) before a pattern goes to the matcher process, whose
requests are capped at 16 MiB. `staging/sm/JSON/parse-mega-huge-array.js` is
not on this list on purpose: a 2**21-element array costs more than 128 MiB of
managed heap (index properties are individually accounted), so it needs an
array-storage change, not a limit.

Each supervising worker owns a process group. A whole-case timeout or adapter crash kills and reaps that group, including a regex child, before the next case creates a replacement. Regex operations additionally have their own engine-level deadline. Transport uses nonblocking bounded IO, so a blocked pipe does not disable the case deadline.

The checked-in [summary](../../../development/browser_core/phase-13-bluejs-engine/test262-summary.json) records the current complete inventory. Full passing conformance, complete host hooks, the remaining ECMA-402 constructors, and unsupported language/API subsystems remain open work.

## Architecture-first triage

Use the [dependency backlog](../../../development/browser_core/phase-13-bluejs-engine/TEST262_ARCHITECTURE.md)
to choose implementation slices; the [recorded analysis](../../../development/browser_core/phase-13-bluejs-engine/TEST262_ANALYSIS_REPORT.md)
contains the latest category counts and representative blockers. Classify a completed run with:

```sh
python3 backend/bluejs/test262/analyze.py --run target/test262 --output target/test262/analysis
```

The analyzer reconciles all path/mode pairs, source hashes, metadata and summary
counts before publishing `items.jsonl`, `analysis.json` and `REPORT.md`. Each mode
retains its raw outcome and gains its specification ID, description, includes,
target workstream, architectural dependencies and first observed blocker.
Targets and blockers are independent: an Array test can first fail in a harness
include or on missing environment support. Heuristic categories are not verified
root causes. Passed negatives, unsupported modes, timeouts and staging/Annex B/
Intl/host-dependent items all remain visible.

Use distinct output directories for before/after runs. A filtered run remains
explicitly partial; missing or duplicate outcomes and changed sources are errors,
not silently omitted tests. Regression-test both runner and analyzer with
`python3 -m unittest discover -s backend/bluejs/test262 -v`.
