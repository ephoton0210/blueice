# Array.fromAsync iterator-close GC follow-up

Date: 2026-10-13. The integrated `ec4217440` Windows workspace run
exposes `array_from_async_allocations_after_a_user_hook` using a collected
ObjectId at a 376,704-byte heap ceiling. The same existing regression
fails in an isolated macOS build with the identical ceiling and object
serial 307, so the failure is unrelated to VM test-disk capacity.

A mapper can throw while closing an async iterator whose `return()`
produces a fresh object. Saving the original error can allocate and
collect that object before Await adopts it. The close path now roots
the return value across that error-state write. Existing close ordering,
original-error precedence and eventual Promise adoption are retained.

The original allocation-ceiling regression changes from failure to success.
All 48 cases across `array_from_async`, `cov_g2_final` and
`promise_gc_lifecycle` pass with zero failures/ignored, as do BlueJS
strict all-target Clippy and workspace format checks. An additional
major-collection semantic case checks original reason identity, one close
and one then-getter read; it passed before the fix and is not the red
regression. Source hashes match throughout the completed focused gate.

The [receipt](artifacts/array-from-async-gc-results.json) records exact
commands, source/log hashes and the Windows failure. The correction
remains isolated while pre-fix complete platform gates finish; those
failed runs are retained. Focused success does not establish complete
Rust, native or Windows acceptance of the corrected source.
