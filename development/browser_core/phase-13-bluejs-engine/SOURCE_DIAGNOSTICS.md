# Original diagnostic source positions

[← Browser integration TODO](BROWSER_INTEGRATION_TODO.md)

M1q is in progress. The TODO records verified failing page/VM fixtures,
successful foundation tests and all remaining acceptance gates. Section H
remains open until original-source reporting works across the complete path.

## Immutable code ownership

An owner constructs `DiagnosticSourceMap` for one exact
`BlueJsSourceIdentity`. Binding verifies the live generation, source identity,
code-unit ordinals and instruction boundaries before publishing anything.
Half-open ranges are sorted and non-overlapping; gaps remain unavailable.
All code units share one immutable `Rc` map. Compiled closures keep their own
association even after another program runs. No JavaScript value, live object
root or ambient mutable filename is retained by the map.

Locations contain a bounded copied filename and one-based line/UTF-16 column.
The scanner handles CRLF as one newline, as well as CR, LF, LS and PS; an
interior UTF-8 byte has no coordinate. HTML leaves error-information extraction
[implementation-defined](https://html.spec.whatwg.org/multipage/webappapis.html#report-an-exception).
This boundary uses certified compiler/parser provenance and explicit absence.

The current map limits are 65,536 ranges, an 8 KiB filename and 8 MiB of owned
map payload. Vector/string capacities participate in measurement. The page
installer charges root code and immutable map payload together against its
existing installed-program byte limit. A failed builder, invalid table or
excess combined charge removes the candidate program before execution.

## Parser and compiler statement provenance

The original prototype mapped classic JavaScript root statements. A real
fixture proved that this incorrectly located a nested block at its outer `if`
and left a function's throw unlocated. The expanded implementation captures
parser statement subtrees, including normalization, and pairs their structure
and function source ranges with the executable AST preorder. Temporary pointer
identities stay inside compilation; emitted owners contain AST ordinals only.

The diagnostic compilation mode records the active AST owner of each emitted
instruction. The page owner combines those exact owners with parser spans,
coalesces adjacent equal locations, then discards temporary instruction-owner
metadata when binding the immutable source map. Ordinary host-neutral parsing
and compilation omit this extra collection. Existing debugger AST identities
and the structured-program ABI retain their order and meaning.

The interpreter restores a caller's source context after nested execution.
An explicit `reportError` uses its active call site; an uncaught language throw
uses its captured instruction origin. The existing terminal debugger fallback
is separate: a mapped caller cannot manufacture a location for an unmapped
`eval` or `Function` body. Error events preserve their existing cancellation,
handler arguments, recursion and original-value behavior.

## Saved completions and cleanup

An abrupt finalizer saves the incoming diagnostic location, terminal debugger
site and propagation epoch with its pending completion. A normally completing
finalizer restores the saved throw with a fresh propagation epoch; an abrupt
replacement keeps its own origin. A yielded generator retains the saved
provenance in its heap-owned handler frame, independently of later scripts
and collections. Explicitly unavailable origins remain unavailable.

Iterator cleanup preserves the incoming throw's provenance when a close
failure is ignored. When closing replaces a return or jump, the first close
failure retains its own origin while outer iterators still receive cleanup.
Native collection/iterator/Promise and host cleanup paths use the same
preservation helper. Saved filename capacities participate in suspended
generator managed bytes and async/module continuation scheduling bytes.

The initial public cleanup regression failed four of five scenarios, and
the real Core/child finalizer fixture reported `:0:0`. A follow-up exposed a
cross-script generator epoch collision (14 passed, one failed). The corrected
diagnostic/error/page selection passes all 43 cases, including the original
throw, replacement, unknown-origin, GC and byte-charge controls. The complete
follow-up selection passes 1,837 cases across twenty targets, with one existing
ignored and zero failures. It covers the three related libraries, twenty
debugger groups, generator/async GC, completion, native Promise and scheduling
limits, and all sixteen real page cases. The page coordinate fixture verifies
six original-source contexts. Formatting and focused three-library Clippy
with `-D warnings` also pass (Clippy: 1m 11s). All nineteen Rust hashes in
`target/source-diagnostics-cleanup-source-20261015.json` match this complete
run; `target/source-diagnostics-cleanup-validation-20261015.json` records the
selection and remaining gates.

## Copied source records and transport

Console records now carry an optional copied `source` with filename, line
and column. Ordinary calls use their active instruction; uncaught reports
pass their captured origin explicitly after dispatching error handlers.
Unknown callbacks use an absent location even if a preceding root script
reported a mapped exception. No error-value source property is evaluated.

The private child channel is v61. The public field has a serde default, is
omitted when unknown and preserves the coarse public protocol v2. Filename
bytes are bounded at 8 KiB and share the existing 256 KiB console text limit
with messages. VM eviction and native accounting measure owned capacities;
wire validation measures copied UTF-8 text. Core and MCP each validate the
bounded record before publication. Filenames remain untrusted data inside
MCP's existing page-content frame and confer no loading/debugger authority.

The red baseline comprises two VM failures, one real Core/child failure and
three IPC failures. The first implementation passes the IPC controls and
eighteen VM cases but exposes a stale root-to-unknown-timer fallback. An
explicit root-reporting origin removes that fallback. Core's two dedicated
tests pass, including ten malformed-reply variants. The matched Core/child
page rerun passes all seventeen cases, and the unchanged Cargo-built official
MCP executable passes all three console cases (nine filtered, 1.71s). Known
JavaScript sources survive messages, tasks, native events and navigation;
direct BlueTS and unmapped code keep their explicit absence.

The final disjoint selection has 2,117 passes across twelve targets and one
existing ignored. The first real-process attempts used an older Core binary
and failed startup; rebuilding the matching private v61 Core/host binaries
resolved those attempts before acceptance. Formatting and focused six-library
Clippy with `-D warnings` pass (Clippy: 2m 48s). The thirteen Rust source hashes
and validated runtime/test hashes are in
`target/console-sources-source-20261015.json`; the per-selection results and
remaining gates are in `target/console-sources-validation-20261015.json`.

## Promise rejection provenance

A rejected native Promise retains a copied throw origin separately from its
JavaScript reason. Settlement jobs and already-settled await registration
carry that origin to later turns. A missing rejection handler and native
forwarding preserve it; a JavaScript callback captures its own new throw or
reject call, including an explicitly unavailable dynamic-code origin. Each
job displaces the ambient interpreter source and throw context. Rejected
awaits publish a fresh propagation epoch before entering catch/finally.

Native `finally` thrower thunks retain their original rejection provenance
in a weak side table owned by the thunk's heap state. Collection prunes the
metadata without rooting the Promise, reason or thunk. No JavaScript-visible
property stores diagnostic metadata, and no reason property is evaluated.
Promise records, queued/active jobs, displaced native scopes and retained
thunk filename capacities participate in document scheduling accounting.
The default rejection console report snapshots the retained origin before
event handlers run, preserving the original diagnostic if a handler throws.

The first public regression exercised nineteen forwarding/callback/await/
finally/species/async-generator scenarios: eighteen known origins were absent,
while the unknown-code control remained unavailable. The first implementation
passes all nineteen. The corresponding real MCP rejection assertion also
failed before the implementation because the original inline source was
absent. The expanded regression now passes all 32 source scenarios and all
23 diagnostic groups, including full-GC ownership and native-byte refusal
controls. Real MCP acceptance verifies the original inline line 4, column 25
after `finally` and keeps direct BlueTS unavailable. The final disjoint
selection passes 1,837 cases across twenty targets, with one existing ignored;
format and focused six-library Clippy pass. The initial sandboxed socket runs
failed during setup; the same compiled binaries pass with local socket access.
The fifteen Rust hashes and runtime/test hashes are recorded in
`target/promise-origins-source-20261015.json`; per-target results and remaining
gates are in `target/promise-origins-validation-20261015.json`.
This verifies a scoped implementation step, leaving M1q at 3/10 final gates.

Queued async-generator requests retain an immutable origin with their FIFO
entry. Queue snapshots share this native metadata; its allocation and filename
capacity are charged to the generator's managed heap bytes. Each head runs
under its own native provenance scope. Injected throws publish fresh epochs
before catch/finally can save them. A rejected return operand or a throwing
getter replaces the request origin; a fulfilled operand preserves it.
Delegate failures publish their own origin before entering the generator,
and an abrupt finalizer replaces that origin rather than retaining it.
No additional JavaScript roots or observable diagnostic properties are added.

The first request regression lost four of nine origins. A twelve-scenario
follow-up exposed two delegate/finalizer failures after the first nine passed.
The final twelve scenarios pass with normal and aggressive GC, including
unknown origins and abrupt replacement. Distinct known/unknown FIFO entries
with identical rejection values keep their own sources across later scripts;
a release assertion observes major collection before checking the weak owner.
Heap tests verify actual filename capacity charges and atomic refusal without
replacing the original request. All 26 diagnostic groups pass.

Real MCP reproduces an absent queued-throw source against the unchanged old
runtime binaries, then validates inline line 5, column 25 after `.throw()` and
`finally` with the new runtime. The final disjoint selection passes 1,841
cases across twenty targets, one existing ignored, plus formatting and focused
six-library Clippy. Launcher Python helper startup failures under concurrent
load and one serial timeout are recorded; an isolated complete replay of the
identical executable passes all 307 cases with four test threads. The seven
Rust hashes, runtime/test hashes and per-target receipts are in
`target/generator-request-origins-source-20261015.json` and
`target/generator-request-origins-validation-20261015.json`. This extends the
verified implementation; M1q's final acceptance remains at 3/10.

## Cached module completions

Module evaluation errors now retain the rejection value with immutable native
provenance. A cached import republishes the stored origin with a fresh throw
epoch. Async dependency failures share their origin across static parents,
and every waiting dynamic import receives the same certified source. Page
module error events use the stored dependency location. Unknown generated
code remains unavailable; a module path or later importer supplies no fallback.

A weak native ledger charges origin allocations and filename capacities even
while a linked graph is displaced into a Rust frame. It adds no JavaScript
roots and releases dead entries during collection. Admission includes ledger
growth before publishing a completion. Loader referrers are distinguished from
active module execution so an installed idle referrer cannot suppress host
rejection notification. Await continuations restore their ambient execution
state, and debugger resumption preserves enclosing checkpoint cleanup.

The initial public regressions report an unavailable dependency source and
zero import rejection notifications with an installed loader referrer. The
final selection passes all 1,487 BlueJS cases across twenty-six targets,
including 1,204 library cases and thirty diagnostic groups. It verifies
synchronous and awaited dependency errors, cached import replay, async parent
fan-out, rejected-await and unknown-code sources, argument getters, debugger
resumption, shared capacity charges, displaced ownership, major GC and native
admission refusal. Bridge, Launcher, real page and official MCP acceptance
bring the total to 1,981 passes over thirty targets, one existing ignored.
Formatting, matching Core/host/Gatekeeper builds and six-library Clippy pass.

Old build artifacts were cleared before validation; incremental caching and
dev/test debug info are disabled. The first MCP attempt stopped at a missing
Gatekeeper binary; the complete matched runtime passes all three cases.
The thirteen final Rust input hashes, three runtime hashes, test hashes and
receipts are in `target/module-origins-source-20261015.json` and
`target/module-origins-validation-20261015.json`. This verifies retention of
owner-certified module maps; original page-module/debugger source production
remains a separate open requirement. M1q remains at 3/10 final gates.

## Required remaining work

Precise expression/parameter/generated-code provenance, direct and emitted
BlueTS and page-module/debugger source producers still need their full acceptance.
The complete queued-origin matrix, including remaining delegate/protocol
cases and rejected-await/async-disposal finalizer
interactions, remains part of M1q.
Other copies held by asynchronous diagnostic state must join document-native
accounting. Navigation, stale generations, GC, source-map refusals and resource
containment need integrated
coverage. Broader host-buffer/network/UI/default-on policy remains separate.

No source position may be inferred from a nearby token, an unrelated later
callback, a page-selected source-map URL or an unauthenticated generation.
Unknown and generated-only locations remain explicitly unavailable. Final
related Rust/Core/MCP, formatting, workspace Clippy/build and complete pinned
Test262 gates have not been claimed.

## Foundation implementation checkpoint (2026-10-15)

Commit `ce8c45707` records the foundation. The BlueJS, BlueTS bridge and launcher
library selection plus public
registry/lifecycle targets passed 1,691 cases, with one existing ignored.
After two Clippy-only type-alias corrections, the final public diagnostic,
host-error and real Core/child page replay passed all 40 cases. All 27 Rust
source hashes in `target/source-diagnostics-foundation-source-20261015.json`
match that final replay. The combined eight-target checkpoint has 1,731
passing cases and one existing ignored. Formatting and focused three-library
Clippy with `-D warnings` pass.

The [TODO progress checklist](BROWSER_INTEGRATION_TODO.md#milestone-record)
records the verified implementation steps and remaining work. At the foundation
checkpoint, two of the ten full M1q acceptance gates were complete. That
checkpoint does not claim
workspace/all-target Clippy/build, final Core/MCP acceptance, a fresh pinned
Test262 run or fresh coverage.
