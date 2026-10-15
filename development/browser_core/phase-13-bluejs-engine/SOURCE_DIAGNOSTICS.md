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

## Required remaining work

Precise expression/parameter/generated-code provenance, direct and emitted
BlueTS, modules and debugger-driven execution still need their full acceptance.
Queued Promise rejection/await origins and rejected-await/async-disposal
finalizer interactions remain part of M1q.
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
