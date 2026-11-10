# Defu multi-file compiler repair

Completed within the frozen CommonJS/ES2022/strict project and control matrices.
Accepted source: backend-tree:afe0fa1eb02894b7d618fc90edaad8af4bee46b4.

[Measurement](DEFU_REPAIR_MEASUREMENT.json) ·
[raw evidence and hashes](DEFU_REPAIR_ARTIFACTS.json) ·
[state cross-use evaluation](DEFU_INTERCHANGE_ACCEPTANCE.md) ·
[publication consistency checks](DEFU_REPAIR_PUBLICATION_CHECKS.json)

The complete unchanged defu production source now compiles, emits all three
Native-byte-exact declarations and passes all twelve authored Node runtime
scenarios. Positive/negative declaration consumers, the exported-type dependent
consumer change, incremental initial/warm builds, imported implementation/type
edits, dependency addition/missing/restoration and edited-clean output equality
all pass. The original blocked audit remains in DEFU_PROJECT_AUDIT.md with its
original compiler binding.

| Fresh acceptance gate | Result |
| --- | --- |
| Complete pinned Native cohort | 182 suites / 1,042 tests; zero failed, ignored or filtered |
| Complete local workspace | 7,839 passed / 391 existing ignored / 715 result groups; complete target coverage with unchanged-target replays, zero final failed or filtered |
| Native and workspace map reports | Each covers 8,407 configurations × two builds, zero differences; actual compiler and process hashes retained |
| Windows x86_64 VM | 82/82 selected core oracles + 25 path checks + all four regression cohorts; 14,647 source-manifest files and every canonical backend path verified |
| Original unchanged mitt regression | Exact declaration, eight runtime scenarios, consumers and all incremental stages pass |
| Original eight defu reproductions | All verdicts agree; original sources and Native observations unchanged |
| New prerequisite controls | 49 Native-frozen verdict/declaration controls, including two independently observed emitted-runtime controls |
| Format, all-target workspace Clippy and evidence tools | Pass; warnings denied, all Node/Python and source/case/account/status checks retained |
| Changed production module sizes | 22 files below 1,200 lines; maximum 1094 |

## Implemented behavior

Contextual parameter names and conditional rest-infer syntax retain their
TypeScript meaning. Lexical negative null/undefined facts survive subsequent
typeof refinement, remain tied to the owning binding, and are invalidated by
writes and intersected at branch joins. Unknown values still require the
appropriate guard before an in operation.

Quantified contextual functions keep declaration identities, actual inferred
results, readonly bounds and exact generic rest tails. Conditional true branches
retain proven bounds and their infer binders, including shared repeated infer
names. Generic argument checks use current lexical definitions. Symbolic index
domains must be covered by the existing index signatures before their declared
values provide a bound.

Recursive record compositions avoid expanding symbolic mapped properties when
their object shape already satisfies an explicit-any broad index target. No
unknown-to-any fallback or generic-expansion limit increase is introduced.
The unchanged 256-expansion boundary and the existing negative/budget controls
remain measured.

CommonJS default value exports use the final import/export storage reference.
This fixes the exported-const ReferenceError and preserves mutable default
snapshot behavior. Declaration rendering preserves nested mapped-type layout,
and private type dependencies include generic constraints/defaults.

## Evidence boundaries

The fixed-tool Native batch recorded one 1,000ms VM timeout while loading
fixed tslib. The exact product passed ten isolated repetitions, each retaining
all sixteen runtime variants and the original time limit. The complete failed
family was rerun alone; the unrun suites completed at reduced concurrency.
The complete catalog combines only successful source/tool/compiler-bound
segments, rechecks every retained test set and log hash, and preserves the
original timeout and each segment separately. Every catalog suite and selected
test is required; no failing observation is counted as agreement.

The complete local workspace command retained every target with no-fail-fast.
It exited 101 with seventeen failed targets. One BlueJS RegExp test correctly
returned RegexTimeout but exceeded its existing 2-second wall-clock assertion.
The other failures occurred in the socket-dependent engine, extension, IPC,
launcher, MCP and network targets while the execution sandbox refused local
socket binding. The identical Unix socket probe records errno 1 inside that
sandbox and succeeds with the approved local-test permissions.

Every failed target was replayed in full with local sockets permitted, using
its exact original executable hash, original package working directory and
four test threads. Every selected name, result count and log hash is retained;
none is filtered out. The RegExp target additionally passed five serial and
ten four-thread repetitions. Its executable and worker hashes, 40ms regex
deadline and 2-second assertion remain unchanged. The final local measurement
combines the successful initial groups and all successful complete-target
replays. It does not claim that the original Cargo command exited zero or that
a scheduling cause was proven. Both socket probes, the verbatim initial failure
state and all target replay logs remain in the evidence archive.

All 39,512 previous suite-qualified case identities and classifications remain.
The 49 new direct controls bring the inventory to 39,561 assertions. Rejected
prerequisite controls compare verdicts and retain Native diagnostics; they do
not establish full diagnostic parity. The project source, Native expectations,
declaration bytes and runtime expectations were frozen independently before the
corresponding implementations.

The first Windows attempt passed the 82 core oracles and 25 path checks but
exposed platform console CRLF in the new Native recorder. Both that raw attempt
and the corrected complete replay are retained. The recorder preserves raw
diagnostic streams and compares console terminators using LF. Declaration and
runtime bytes remain exact. The backend tree and compiler stayed unchanged
during this tool correction; the portable recorder was also revalidated with
the existing Native-recipe test binary.

Public incremental-state presence, size and hashes are recorded separately
from artifact equality because its bytes contain owner paths. All other public
artifact names and bytes remain in the comparisons. These authored scenarios
are separate from the upstream full test suite. The 22 broader feature rows
remain open, the three open and three refused observations retain their
classifications, and full TypeScript parity remains OFF.
