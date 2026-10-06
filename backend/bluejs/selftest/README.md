# BlueJS self-test workspace

The workspace connects changed source files to measured Rust test targets and
Test262 runs. A local dashboard displays the graph, selection reasons, retained
timings, live task status, logs and complete coverage results. Python 3.9 or later
and the existing Cargo/LLVM tools are sufficient; the UI has no build step or
external asset dependencies.

From the repository root:

```sh
python3 -m backend.bluejs.selftest index
python3 -m backend.bluejs.selftest serve \
  --corpus target/bluejs-selftest/corpus/test262-72faf8ec1445c55149615e8b35187830783aba1a
```

The index chooses the newest successful complete measurement, including runs
created by this dashboard. `--baseline` selects an explicit retained round.
It reads retained profiles and executable maps. It executes no Rust tests
or JavaScript cases. Executable hashes must match the recorded measurement;
missing or changed binaries leave their targets unmeasured and conservatively
selected. The dashboard listens on `http://127.0.0.1:8765` and opens the local
browser. Use `--no-open` to start it without opening a browser.

## Workflow

1. Prepare the entire source and fixture change batch.
2. Analyze the Git changes, or enter explicit repository paths in the UI.
3. Run the affected Rust partitions. Failures stop queued work; logs remain available.
4. After the affected tests pass for the same snapshot, run the full inventory.
5. Read the complete report and publish it to the repository report when ready.

### Rust partitions and the correction graph

The **Run affected Rust partitions** action compares current Rust sources with
the latest retained snapshot whose entire Rust inventory passed. That Rust anchor
may come from a round whose later Test262 stage failed; it establishes Rust
selection evidence only, and cannot establish conformance or complete coverage.
Source hashes are checked against its frozen texts.
Every new run automatically retains the exact Rust source texts alongside their
hashes. The anchor remains available even when it is older than the dashboard's
30-run history window, so repeated correction attempts do not force a full rerun.

The first function and opcode contract connects Reference resolution changes to
with, eval, capture, destructuring and global-binding tests. Public test names
and bodies are inspected independently of Cargo target names, so a reference
contract inside an operators or coverage target is selected as well. New fixture tests
connect to their native unit names. The UI shows changed boundaries, selected
filters and reasons. Unknown boundaries use the broader recorded target graph;
their fallback is visible. This is an extensible contract graph, rather than a
claim to have resolved every Rust indirect call statically.

TypedArray species and foreign-buffer refresh functions have a separate contract
for unit namespaces, public TypedArray/buffer script matrices and the related
Test262 directories. Script matrices declared outside test bodies retain all
cases in their owning target. Changes outside the mapped function bodies,
including imports or shared constants, use the broader graph. Changed internal
fixtures also select the public ordinary-library cases that call their verification
entry points, preserving both Rust instantiations without running unrelated
cases in the same integration target.

Global property deletion has a contract covering captured eval bindings,
public deletion cases and Test262 delete/eval/global-code directories.
Both qualified property deletion and unqualified DeleteBinding reach the same
VM cleanup boundary, so their tests participate in one selection.

The runner builds only selected owning targets, rechecks known failed cases,
lists independent harnesses concurrently within the configured worker limit
without executing test names, deduplicates the selection, and
splits large unit targets by their Rust module namespace. Every partition runs
its discovered names once with native `--exact` filters. Counts must match the
selection. Fresh partition profiles create measured source-to-partition edges;
they remain separate from full coverage. Test listing profiles are excluded too.

Passing partitions enable **Run full verification** for the same source snapshot.
That full stage runs the entire Rust inventory and Test262 once, without repeating
the correction stage first. The prerequisite checks the selected case filters
and required Test262 modes as well as the source snapshot. A source edit or a
different selection invalidates the gate. Partition-only
runs cannot publish the canonical complete report.

Tool-only changes run the Python contracts without invoking Cargo, the regex
worker or LLVM export. Build inputs and dependencies outside the BlueJS crate
participate in selection; they are not silently omitted from the changed inputs.

```sh
python3 -m backend.bluejs.selftest partitions
python3 -m backend.bluejs.selftest run --mode partition
# Only after the selected partitions pass for the current snapshot:
python3 -m backend.bluejs.selftest run --mode full --workspace
```

The **Affected → full verification** action starts the full stage only after
the selected stage succeeds. A targeted selection and the full stage use
separate fresh profiles. If conservative selection already covers the complete
engine inventory, that successful current-run execution becomes the full stage
without repeating its cases. Failed, cancelled or partial profiles cannot
contribute to completion counters.

Within each stage, tooling contracts run first, followed by the retained
critical Rust targets, the remaining selected Rust targets and Test262. A
failed prerequisite skips subsequent groups, so conformance does not start
while a Rust gate is still running. Targets run once in their selected stage.

After a failed round, the runner first builds only the relevant failing targets
and rechecks their named Rust cases with `--exact`. Test262 failures build only
the adapter and worker, then select the failed fixture paths with their normal
metadata, modes and resource limits. Every previously failed mode must pass;
missing or newly excluded modes cannot satisfy this prerequisite. A remaining failure stops
the affected and full builds. These checks appear as a separate **recheck**
stage in the UI, with retained case names, logs and timings. Their partial
profiles never contribute to the complete coverage export. Passing rechecks
still require every selected target and the final complete inventory. An
ignored failed case blocks the gate; a removed case is marked skipped and its
current target still runs. A successful complete round clears the older failure
prerequisite.

Test262 runs explicitly budgeted long fixtures in a separate group with at most
two workers, after ordinary fixtures. This limits contention without increasing
their wall deadlines or instruction budgets. The summary records both scheduling
groups, and dispatched rows retain per-mode execution times for future graph
collectors. Timing metadata is excluded from semantic outcome comparisons;
fixture hashes, modes, flags, features, dispositions and actual error kind/phase
still participate. All fixtures still execute exactly once in a full inventory run.

Keep the pinned corpus under workspace artifacts; temporary host directories
can lose their files between sessions. Before any build, a run requiring
Test262 validates the pinned snapshot, manifest and every fixture hash. The
conformance runner repeats validation before dispatch. An existing corpus is
never overwritten.

The regex worker is built for every Rust selection and checked with empty
input before cases start. The READY handshake executes no JavaScript, and its
profiles are excluded from coverage. Most tests receive the exact built worker
path. The public worker-reuse target and Test262 also exercise sibling discovery
from the Cargo deps and adapter directories with the verified worker.
For a complete affected selection, graph observations are exported once after
the full gate, avoiding a duplicate per-target coverage export.

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
runs/<run-id>/source-state.json Frozen source hashes and fingerprint
runs/<run-id>/frozen-rust-sources.json Exact Rust texts for subsequent selection
runs/<run-id>/events.jsonl Append-only run events
runs/<run-id>/logs/        Complete command output
runs/<run-id>/impact/      Targeted artifacts and profiles
runs/<run-id>/full/        Full-run artifacts and fresh coverage
runs/<run-id>/report.md    Complete result for one verified snapshot
```

Mutable state must remain outside source directories. `--state` can select a
different directory. Existing complete measurement artifacts remain available.
Restarting the dashboard restores the latest retained progress and logs. An
unfinished run whose owner has stopped is displayed as interrupted.

The report uses raw LLVM line/function/region counts. It compares Test262
outcome contracts and preserves the cumulative modified-file acceptance scope
across commits. The checked-in `acceptance-scope.json` retains the selected
files, cumulative production scope and original complete-file identities even
without a local state cache. Report publication requires a successful full run with unchanged
sources; an affected-only pass cannot overwrite the complete report. Core
classification, grouped TypedArray results, Intl/Temporal totals, per-file
coverage and runtime gate tables are regenerated from the same completed
snapshot. The coverage reference retains each metric's highest verified
percentage for regression comparison only; completion uses the current raw
export. Re-indexing preserves those thresholds and the original complete files.

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
