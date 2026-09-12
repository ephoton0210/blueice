# Test262 verification

This POSIX runner inventories every test in the pinned official snapshot, including staging/proposals and ECMA-402. It reports every requested execution mode. **A full inventory run is not a passing conformance result.** Static module graphs, Module Namespace Exotic Objects, literal dynamic imports and a bounded top-level-await job path are implemented; complete async execution, thenable assimilation, host loading and much of the language remain unsupported.

Prerequisites: Rust, Python 3, and the pinned Python dependency below. Node is only required by the separate differential oracle. The default Rust subprocess fault tests also use Python 3 on POSIX.

```sh
python3 -m venv /tmp/bluejs-conformance-venv
/tmp/bluejs-conformance-venv/bin/pip install -r backend/bluejs/test262/requirements.txt
cargo build -p blueice-bluejs --bins --offline
/tmp/bluejs-conformance-venv/bin/python -m unittest discover -s backend/bluejs/test262 -v
/tmp/bluejs-conformance-venv/bin/python backend/bluejs/test262/run.py --fetch
```

Use `--fetch` only when the corpus destination does not exist; it verifies the archive and complete file manifest against `snapshot.json`. Subsequent runs omit it. Every run checks all 56,778 archive files, rejects additional test files, and records executable and runner hashes. The archive contains 53,683 JavaScript resources: 53,404 test files and 279 `_FIXTURE` resources. The snapshot selection is time based, not an assertion that every proposal belongs to ECMA-262 edition 17.

Outputs are `target/test262/results.jsonl` (one record per test/mode, source hash, features, expected and actual phase/type/status) and `summary.json` (counts and feature/group breakdown). `--filter intl402/Collator` runs a clearly labelled partial inventory; a filter that selects no test files is a configuration error. `--jobs` defaults to 8, `--timeout` to 2 seconds, and `--instruction-budget` to 100,000 VM dispatches per mode. Every five seconds the runner reports completed files, result counts, and each active `path [mode, elapsed]`; use `--progress-interval 0` for non-interactive output or a different positive interval for more or less detail. Exit 0 requires every scheduled mode to pass; exit 1 means non-passing cases and exit 2 indicates a configuration/argument error. Unexpected Python errors terminate with a traceback and must not be treated as a completed report.

The adapter creates a fresh VM for each case. Raw source is unchanged and receives no assertion harness. Other cases receive native overrides for `sta.js`, `assert.js`, `propertyHelper.js`, and `isConstructor.js`. The generated CharacterClassEscape fixtures preserve their original full-string `RegExp.prototype.test` calls, but use a native helper to stop at the first failed result instead of running their diagnostic loop once per code point. The three staging JIT stress fixtures retain one complete semantic iteration; the TypedArray helper calls the real `set` builtin and checks the entire result without one interpreter dispatch per element. Remaining includes execute as separate classic scripts in the same VM realm: successful top-level `var` and function declarations publish to the global object while lexical declarations stay script-local. Harness assertions and descriptor helpers fail closed, compare signed zero and NaN correctly, and keep resource failures distinct from JavaScript exceptions. The `IsHTMLDDA` feature installs a Test262-only Annex B host exotic in that fresh realm; it does not expose the hook to unmarked tests. Static modules run with module strictness, undefined top-level `this`, and declarations that do not publish classic global properties. The runner supplies reachable relative static and literal-dynamic fixture sources to the adapter; it links named/default/namespace imports, local/indirect/star exports and cycles before evaluation. Module namespaces have live export-cell descriptors, `null` prototype, UTF-16 export ordering, `Symbol.toStringTag`, symbol-key handling and non-extensibility. Dynamic import resolves only in the supplied registry, completes in the Promise job queue, and caches one namespace object per realm module. Top-level `await`, ordinary async functions, and independent async-generator requests preserve their suspended frames across pending Promise jobs; fulfilled awaits also resume in a later job turn. Async generators expose `next`/`return`/`throw`, await yielded values, drive `for await`, and delegate `yield*` through sync or async iterator methods. Host-driven module loading remains a separate unimplemented suspension boundary. Missing `$262`, `$DONE` and `print` hooks cannot satisfy negative ReferenceError tests. The subset parser's unclassified rejections never count as successful negative SyntaxError tests. Arbitrary thrown values currently report `ThrownValue`; they are not guessed to have the expected error class. The baseline interpreter budget is 100,000 dispatches; fixtures declaring `tail-call-optimization` receive at least 3,000,000 dispatches so Test262's required 100,000 tail calls are measured without removing the bounded-resource guard.

The two-second wall deadline remains the default. The exact immutable finite
stress-fixture list in the runner receives a 90-second per-mode deadline for
its documented TypedArray and interpreter stress loops. The seven exact
historical RegExp BMP enumerations use bounded native adapters that retain
their matcher validation without dispatching one JavaScript eval per unit.
Cases including Test262's
`testTypedArray.js` receive a 30-second wall allowance because that shared
harness deliberately invokes a callback across all numeric constructors and
the byte-conversion matrix. They retain the ordinary bounded interpreter
budget, so this allowance cannot turn an unbounded execution into a pass.

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
