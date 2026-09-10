# Test262 verification

This POSIX runner inventories every test in the pinned official snapshot, including staging/proposals and ECMA-402. It reports every requested execution mode. **A full inventory run is not a passing conformance result.** Dependency-free modules execute, while module graphs and async hosts, harness scripts requiring unsupported grammar or APIs, and much of the language remain unsupported.

Prerequisites: Rust, Python 3, and the pinned Python dependency below. Node is only required by the separate differential oracle. The default Rust subprocess fault tests also use Python 3 on POSIX.

```sh
python3 -m venv /tmp/bluejs-conformance-venv
/tmp/bluejs-conformance-venv/bin/pip install -r backend/bluejs/test262/requirements.txt
cargo build -p blueice-bluejs --bins --offline
/tmp/bluejs-conformance-venv/bin/python -m unittest discover -s backend/bluejs/test262 -v
/tmp/bluejs-conformance-venv/bin/python backend/bluejs/test262/run.py --fetch
```

Use `--fetch` only when the corpus destination does not exist; it verifies the archive and complete file manifest against `snapshot.json`. Subsequent runs omit it. Every run checks all 56,778 archive files, rejects additional test files, and records executable and runner hashes. The archive contains 53,683 JavaScript resources: 53,404 test files and 279 `_FIXTURE` resources. The snapshot selection is time based, not an assertion that every proposal belongs to ECMA-262 edition 17.

Outputs are `target/test262/results.jsonl` (one record per test/mode, source hash, features, expected and actual phase/type/status) and `summary.json` (counts and feature/group breakdown). `--filter intl402/Collator` runs a clearly labelled partial inventory; a filter that selects no test files is a configuration error. `--jobs` defaults to 8, `--timeout` to 2 seconds, and `--instruction-budget` to 100,000 VM dispatches per mode. Exit 0 requires every scheduled mode to pass; exit 1 means non-passing cases and exit 2 indicates a configuration/argument error. Unexpected Python errors terminate with a traceback and must not be treated as a completed report.

The adapter creates a fresh VM for each case. Raw source is unchanged and receives no assertion harness. Other cases receive native overrides for `sta.js`, `assert.js`, `propertyHelper.js`, and `isConstructor.js`. Remaining includes execute as separate classic scripts in the same VM realm: successful top-level `var` and function declarations publish to the global object while lexical declarations stay script-local. Harness assertions and descriptor helpers fail closed, compare signed zero and NaN correctly, and keep resource failures distinct from JavaScript exceptions. The `IsHTMLDDA` feature installs a Test262-only Annex B host exotic in that fresh realm; it does not expose the hook to unmarked tests. A dependency-free module executes with module strictness, undefined top-level `this`, and declarations that do not publish classic global properties. Import/export parsing and linking, dynamic import, and top-level await remain explicit gaps. Empty async declarations can establish lexical bindings, but invoking any async closure remains explicitly unsupported until Promise jobs exist. Missing `$262`, `$DONE` and `print` hooks cannot satisfy negative ReferenceError tests. The subset parser's unclassified rejections never count as successful negative SyntaxError tests. Arbitrary thrown values currently report `ThrownValue`; they are not guessed to have the expected error class. The baseline interpreter budget is 100,000 dispatches; fixtures declaring `tail-call-optimization` receive at least 3,000,000 dispatches so Test262's required 100,000 tail calls are measured without removing the bounded-resource guard.

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
