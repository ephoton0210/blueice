# BlueJS self-test workspace

BlueJS uses a source-to-test graph to select the affected test targets after a
completed change batch. The selected tests provide the first verification gate;
one complete run then supplies the conformance, preservation and raw coverage
evidence used by the current platform report.

The implementation lives in [`backend/bluejs/selftest/`](../../../backend/bluejs/selftest/README.md).
It includes a local dashboard for impact analysis, relationship exploration,
task progress, cancellation, retained logs and complete reports.

## Data model

Graph schema 1 contains source nodes, test-target nodes, observed execution
edges, static dependency edges, measured durations, source hashes and the
cumulative coverage acceptance scope. Observed edges are collected from each
target's fresh LLVM profiles. Initial graph construction can use an already
verified complete measurement without rerunning its test cases.

Run events have stable identifiers and stages. Each result belongs to a source
fingerprint and retained executable maps. Cancelled, failed and stale runs remain
inspectable. Complete coverage uses fresh profiles from the same frozen run;
a conservative complete selection can be promoted without repeating its cases.
Editing sources invalidates earlier affected-test success.

## Selection boundaries

The initial granularity is a Cargo test target and a complete Test262 runner
node. Shared engine and unmeasured changes select a conservative full scope.
Explicit Rust dependencies and fixture includes extend measured relationships.
Observed execution does not establish that every possible affected path has
been discovered; a final full run remains required.

## Extension points

The collector can add individual Rust cases or measured Test262 partitions as
new node kinds while preserving file-to-target observations. A runner adapter
provides the command, fresh profiles, result contract and live events. The UI
consumes this versioned graph and event model through the local API.

Future additions can include case-level collection, changed-region selection,
duration-aware scheduling and regression-first ordering. Any added collector
must retain source provenance, conservative treatment of missing observations
and the complete-run acceptance gate.
