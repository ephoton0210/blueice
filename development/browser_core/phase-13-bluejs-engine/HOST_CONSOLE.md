# Page console and diagnostic transport

[← Browser integration TODO](BROWSER_INTEGRATION_TODO.md)

M1m adds `console` to the owner-selected `core-script-dom-console-v14`
File DOM profile. It extends v13's 51 inventory entries with one namespace
and matching `BlueIceConsole`/`window.console` declarations. Older artifacts
remain immutable. Neither the standalone VM nor another page profile gains
implicit browser globals, and reading diagnostics never enables execution.

## VM behavior

The namespace implements the 19 methods in the
[Console Standard](https://console.spec.whatwg.org/): logging, assertions,
inspection, trace, groups, counts and timers. Methods are non-constructible
native functions and can be called without a console receiver. The namespace
has the required empty intermediate prototype and `console` string tag.

The text printer supports string/integer/float substitutions, object previews
and consumed CSS arguments. Explicit substitutions perform the corresponding
JavaScript conversions and propagate conversion exceptions. Reentrant logs
from those conversions appear before the outer record. Ordinary object
printing copies bounded stored data without invoking getters, proxy traps or
custom stringification; unsupported shapes have an explicit partial preview.
`dir`/`table` use that text preview, rather than an interactive inspector or
table UI. Trace output includes up to eight active function names. CSS styling
and inspector options do not affect the text printer.

Counts and timers use lossless UTF-16 label keys, so lone surrogates do not
alias replacement characters. Display text replaces lone surrogates for UTF-8
transport. Timers and record timestamps use the existing injected monotonic
document clock, including manual time for tests. Duplicate/missing timer and
count labels report warnings. `clear` clears queued output and group state;
it preserves counters and timers. Group records carry a bounded display depth.

| Retained resource | Limit and behavior |
| --- | --- |
| Queued records | 256; evict oldest records. |
| Queued message allocations | 256 KiB in total; evict oldest records. |
| One UTF-8 message | 8 KiB, with an explicit truncation flag. |
| Count labels / timer labels | 128 each; refuse further labels with a warning. |
| Label display | 256 UTF-8 bytes; refuse longer labels without merging keys. |
| Display group depth | 32; balance deeper groups without retaining a label stack. |

The fixed queue slots, message capacities and label payloads enter the existing
document-owned native/scheduling byte ledger. The shared 8 MiB document limit
still applies. Console records contain copied strings and scalar metadata;
they retain no Promise, callable, DOM wrapper or other managed JS object.
Draining transfers records and refreshes retained-byte accounting. Replacement
and close discard that realm's records, counters and timers.

## Core, wire and MCP

Private protocol **v60** adds `DrainConsole` and a bounded console reply for
one exact `(tab, document generation)`. The child drains the VM and returns
updated owned-byte accounting. Core checks document identity, record shape,
count/byte bounds, sequence/timestamp order and accounting before publication.
Malformed replies revoke the addressed realm through existing containment,
without replaying its scripts or disturbing another tab.

The public control protocol adds `GetConsoleMessages`/`ConsoleMessages`.
Its coarse version remains v2 under the existing additive-variant rule.
The response names the current document, includes record sequence, time,
method, level, group depth, message/truncation and the overflow count since
the previous drain or clear. An unavailable owner returns a stable error.

MCP's `get_console_messages` tool accepts an optional tab id, drains only
that document's bounded snapshot and wraps the serialized result in the
existing untrusted-page-content boundary. The adapter rejects wrong-tab,
malformed and unexpected correlated replies. This operation never evaluates
source, resumes a debugger continuation or runs a task/microtask checkpoint.
Normal session work can continue between requests. A clear record describes
page-requested clearing; the tool does not provide a separate execution path.

Acceptance covers public VM/profile behavior, diagnostic resource limits,
Core containment and MCP framing/transport. The real Core/child fixture uses
an official MCP client, mixed JavaScript/checked BlueTS, a Promise job, a timer
and a native click; it verifies draining and navigation isolation. Unhandled
rejection reporting, source-map positions and complete external host-buffer
accounting remain separate TODO items; M1o supplies uncaught exception reporting.

M1o's follow-up is accepted. Its v15 profile inherits this complete
console surface and sends uncanceled page exceptions through the same bounded
`error` records and untrusted Core/MCP route. Error events/cancellation,
recursive reporting and retained values stay inside the realm; no wire field
or protocol version changes. See [the error contract](HOST_ERROR_REPORTING.md)
and the [live acceptance record](BROWSER_INTEGRATION_TODO.md#milestone-record).
