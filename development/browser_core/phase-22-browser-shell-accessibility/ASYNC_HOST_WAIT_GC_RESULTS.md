# Async host-wait Promise GC follow-up

Date: 2026-10-13. Complete macOS and Windows workspace runs for
`ec4217440` both fail `atomics_wait_async`. Four original Test262
fixture/mode executions report `TypeError: invalid Promise receiver`: the
Int32 and BigInt `undefined-for-timeout.js` fixtures in sloppy and strict
mode. Separate major-collection regressions fail before the correction:
a pending wait Promise disappears from WeakRef, and its inline
Promise.all batch cannot settle after collection.

An OS wait thread must retain the Promise whose completion it owns, even
when JavaScript retains only reactions. Registration now roots that
Promise; the thread carries an opaque root token, not a Heap or VM.
The owning VM keeps every dequeued wait/timer event rooted across earlier
callbacks, then releases the whole batch even when a callback or
settlement fails. A completed unreferenced wait Promise is collectible.

All 138 related cases pass with zero failures/ignored across binary
allocation sweeps, GC stress, Promise lifecycle and Test262 host targets.
The exact four original fixture/mode executions also pass through the
real parser, compiler, VM, native notify and host event loop; they are
not skipped. Strict BlueJS all-target Clippy and workspace format checks
pass, and all three affected source hashes remain unchanged during the
completed focused gate.

The [receipt](artifacts/async-host-wait-gc-results.json) records red/green
logs, exact source and fixture hashes, commands and pending platform
gates. The correction remains isolated while the original complete
runs finish. Previous `33dfdb3b7` source archives/helpers are superseded
for the next acceptance; the final corrected commit must be rebuilt
and packaged. Focused success does not establish complete current-source
Rust, native or Windows acceptance.
