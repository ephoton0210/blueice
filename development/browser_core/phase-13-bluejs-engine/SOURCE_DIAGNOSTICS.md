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

## Required remaining work

Precise expression/parameter/generated-code provenance, direct and emitted
BlueTS, modules and debugger-driven execution still need their full acceptance.
Queued Promise rejection/await origins, completion/finally preservation and
copied Core/MCP source locations remain part of M1q. Copies held by asynchronous
diagnostic state must join document-native accounting. Navigation, stale
generations, GC, source-map refusals and resource containment need integrated
coverage. Broader host-buffer/network/UI/default-on policy remains separate.

No source position may be inferred from a nearby token, an unrelated later
callback, a page-selected source-map URL or an unauthenticated generation.
Unknown and generated-only locations remain explicitly unavailable. Final
related Rust/Core/MCP, formatting, workspace Clippy/build and complete pinned
Test262 gates have not been claimed.

## Verified implementation checkpoint (2026-10-15)

The BlueJS, BlueTS bridge and launcher library selection plus public
registry/lifecycle targets passed 1,691 cases, with one existing ignored.
After two Clippy-only type-alias corrections, the final public diagnostic,
host-error and real Core/child page replay passed all 40 cases. All 27 Rust
source hashes in `target/source-diagnostics-foundation-source-20261015.json`
match that final replay. The combined eight-target checkpoint has 1,731
passing cases and one existing ignored. Formatting and focused three-library
Clippy with `-D warnings` pass.

The [TODO progress checklist](BROWSER_INTEGRATION_TODO.md#milestone-record)
records eight verified implementation steps and the remaining work. Two of
the ten full M1q acceptance gates are complete. This checkpoint does not claim
workspace/all-target Clippy/build, final Core/MCP acceptance, a fresh pinned
Test262 run or fresh coverage.
