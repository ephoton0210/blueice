# Test262 verification

This POSIX runner inventories every test in the pinned official snapshot, including staging/proposals and ECMA-402. It reports every requested execution mode. **A full inventory run is not a passing conformance result.** Module/async hosts, harness scripts requiring unsupported grammar or APIs, and much of the language remain unsupported.

Prerequisites: Rust, Python 3, and the pinned Python dependency below. Node is only required by the separate differential oracle. The default Rust subprocess fault tests also use Python 3 on POSIX.

```sh
python3 -m venv /tmp/bluejs-conformance-venv
/tmp/bluejs-conformance-venv/bin/pip install -r backend/bluejs/test262/requirements.txt
cargo build -p blueice-bluejs --bins --offline
/tmp/bluejs-conformance-venv/bin/python -m unittest discover -s backend/bluejs/test262 -v
/tmp/bluejs-conformance-venv/bin/python backend/bluejs/test262/run.py --fetch
```

Use `--fetch` only when the corpus destination does not exist; it verifies the archive and complete file manifest against `snapshot.json`. Subsequent runs omit it. Every run checks all 56,778 archive files, rejects additional test files, and records executable and runner hashes. The archive contains 53,683 JavaScript resources: 53,404 test files and 279 `_FIXTURE` resources. The snapshot selection is time based, not an assertion that every proposal belongs to ECMA-262 edition 17.

Outputs are `target/test262/results.jsonl` (one record per test/mode, source hash, features, expected and actual phase/type/status) and `summary.json` (counts and feature/group breakdown). `--filter intl402/Collator` runs a clearly labelled partial inventory. `--jobs` defaults to 8, `--timeout` to 2 seconds, and `--instruction-budget` to 100,000 VM dispatches per mode. Exit 0 requires every scheduled mode to pass; exit 1 means non-passing cases and exit 2 indicates a configuration/argument error. Unexpected Python errors terminate with a traceback and must not be treated as a completed report.

The adapter creates a fresh VM for each case. Raw source is unchanged and receives no assertion harness. Other cases receive native overrides for `sta.js`, `assert.js`, `propertyHelper.js`, and `isConstructor.js`. Remaining includes execute as separate classic scripts in the same VM realm: successful top-level `var` and function declarations publish to the global object while lexical declarations stay script-local. Harness assertions and descriptor helpers fail closed, compare signed zero and NaN correctly, and keep resource failures distinct from JavaScript exceptions. Module and async modes remain visible as unsupported. Missing `$262`, `$DONE` and `print` hooks cannot satisfy negative ReferenceError tests. The subset parser's unclassified rejections never count as successful negative SyntaxError tests. Arbitrary thrown values currently report `ThrownValue`; they are not guessed to have the expected error class.

Each supervising worker owns a process group. A whole-case timeout or adapter crash kills and reaps that group, including a regex child, before the next case creates a replacement. Regex operations additionally have their own engine-level deadline. Transport uses nonblocking bounded IO, so a blocked pipe does not disable the case deadline.

The checked-in [summary](../../../development/browser_core/phase-13-bluejs-engine/test262-summary.json) records the current complete inventory. Full passing conformance, complete host hooks, the remaining ECMA-402 constructors, and unsupported language/API subsystems remain open work.
