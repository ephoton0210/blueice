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

The correction graph now includes native Rust case partitions and related
Test262 fixture sets. A Reference-resolution contract maps changed interpreter
opcodes and operations to with, eval, capture, destructuring and global-binding
cases. New public fixtures map to their native unit names. The graph inspects
public test names and script bodies independently of Cargo target names, so
operator and coverage targets participate when they contain the same contract.
The graph compares
sources with a hashed snapshot whose complete Rust inventory passed; this anchor
does not claim that later conformance or complete coverage passed.
Each run retains the Rust texts and hashes automatically. Source anchors remain
available beyond the UI's 30-run history window. Build inputs and dependency
changes outside the BlueJS crate also participate in the plan.

Selected owning targets are built, failed cases are rechecked, and native test
listing identifies the exact case set without executing tests. The runner
deduplicates cases and groups unit tests by Rust module. Fresh partition profiles
add measured source relationships. Partition success enables one complete run
for the same snapshot, without another correction pass first. Partial and test
listing profiles never enter complete coverage.
The full-run prerequisite also compares the case filters and required Test262
modes; sharing an owning Cargo target is insufficient. Tool-only changes execute
the Python contracts without building or executing the Rust engine.

Unknown boundaries still use the existing target graph and show their fallback
in the UI. Shared engine and unmeasured changes can require a wider scope.
Explicit Rust dependencies and fixture includes extend measured relationships.
Observed execution does not establish that every possible affected path has
been discovered; a final full run remains required.

## Extension points

The collector can add individual Rust cases or measured Test262 partitions as
new node kinds while preserving file-to-target observations. A runner adapter
provides the command, fresh profiles, result contract and live events. The UI
consumes this versioned graph and event model through the local API.

Future additions can extend the boundary contracts, refine changed-region
selection and use retained partition durations for scheduling. Any added collector
must retain source provenance, conservative treatment of missing observations
and the complete-run acceptance gate.
