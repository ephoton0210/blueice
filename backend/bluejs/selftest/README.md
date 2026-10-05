# BlueJS self-test workspace

The workspace connects changed source files to measured Rust test targets and
Test262 runs. A local dashboard displays the graph, selection reasons, retained
timings, live task status, logs and complete coverage results. Python 3.9 or later
and the existing Cargo/LLVM tools are sufficient; the UI has no build step or
external asset dependencies.

From the repository root:

```sh
python3 -m backend.bluejs.selftest index \
  --baseline target/test262-macos-20261005-corrections-batch-r28
python3 -m backend.bluejs.selftest serve
```

The index reads retained profiles and executable maps. It executes no Rust tests
or JavaScript cases. Executable hashes must match the recorded measurement;
missing or changed binaries leave their targets unmeasured and conservatively
selected. The dashboard listens on `http://127.0.0.1:8765` and opens the local
browser. Use `--no-open` to start it without opening a browser.

## Workflow

1. Prepare the entire source and fixture change batch.
2. Analyze the Git changes, or enter explicit repository paths in the UI.
3. Run the selected targets. Failures stop queued work; logs remain available.
4. After the affected tests pass for the same snapshot, run the full inventory.
5. Read the complete report and publish it to the repository report when ready.

The **Affected → full verification** action starts the full stage only after
the selected stage succeeds. A targeted selection and the full stage use
separate fresh profiles. If conservative selection already covers the complete
engine inventory, that successful current-run execution becomes the full stage
without repeating its cases. Failed, cancelled or partial profiles cannot
contribute to completion counters.

CLI equivalents:

```sh
python3 -m backend.bluejs.selftest plan
python3 -m backend.bluejs.selftest plan --base HEAD~1
python3 -m backend.bluejs.selftest run --mode impact \
  --files backend/bluejs/src/vm/builtins/generators.rs
python3 -m backend.bluejs.selftest run --mode pipeline --workspace
python3 -m backend.bluejs.selftest publish --run <run-id>
```

`--workspace` adds the remaining workspace runtime tests and BlueJS Rustdoc.
The dashboard also provides this choice. Default execution uses four target
workers and four Rust test threads per target; `--jobs` and `--test-threads`
configure them. Estimated time is retained serial test work, rather than a
promise of wall-clock duration on a shared host.

## Graph and selection contracts

- Observed edges mean that a target's LLVM profiles contain positive execution
  counters for that source file. Each subsequent successful run retains profiles
  in a target-specific directory and adds measured relationships to the graph.
- Static edges resolve explicit Rust `crate::` paths, `#[path]` modules and
  `include!` sources. Their transitive dependents extend the observed selection.
- Changes to shared engine entry, dispatch, compiler, parser, heap, build or
  conformance contracts select the full engine inventory. A new/deleted
  production source, changed function structure or missing measurement does
  likewise. Unmeasured targets are included for production changes.
- A changed integration-test file selects its own target; reused fixture paths
  reach their owning targets through static and measured relationships.
- Documentation-only changes select no engine tests. Tool changes select the
  Python self-test contracts.
- Observations describe paths exercised by prior tests. The static parser is
  not a complete Rust call graph. The final full run verifies paths outside the
  observed selection; core edits can legitimately retain most of the suite.

Results carry a fingerprint of source, fixture and tool inputs. A source change
during execution stops the owned processes and marks the run stale. A changed
snapshot or test selection invalidates the prerequisite for a full run. Source
and executable hashes are checked before publishing measured results.

## State and reports

The default state directory is `target/bluejs-selftest/`:

```text
graph.json                 Versioned nodes, edges, observations and acceptance scope
index-cache/               Reusable exports from verified retained profiles
runs/<run-id>/plan.json    Selection and reasons at run start
runs/<run-id>/run.json     Live stage and task status
runs/<run-id>/events.jsonl Append-only run events
runs/<run-id>/logs/        Complete command output
runs/<run-id>/impact/      Targeted artifacts and profiles
runs/<run-id>/full/        Full-run artifacts and fresh coverage
runs/<run-id>/report.md    Complete result for one verified snapshot
```

Mutable state must remain outside source directories. `--state` can select a
different directory. Existing complete measurement artifacts remain available.

The report uses raw LLVM line/function/region counts. It compares Test262
outcome contracts and preserves the cumulative modified-file acceptance scope
across commits. The checked-in `acceptance-scope.json` retains the selected
files, cumulative production scope and original complete-file identities even
without a local state cache. Report publication requires a successful full run with unchanged
sources; an affected-only pass cannot overwrite the complete report.

The local API exposes `/api/state`, `/api/graph`, `/api/plan`, `/api/run`,
`/api/cancel`, `/api/index`, `/api/log`, `/api/report` and `/api/publish`.
Actions use the local dashboard token and fixed runner commands. The versioned
JSON graph and event stream support future case-level collectors and additional
BlueJS test adapters without coupling the UI to Cargo output parsing.

## Verify the tool

```sh
python3 -m unittest discover -s backend/bluejs/tests -p test_selftest.py -v
```

These contracts cover selection, fallback, profile provenance, snapshot
invalidation, cancellation, full-run gating, semantic comparison and the local
HTTP interface. They run small Python fixtures instead of the BlueJS suite.
