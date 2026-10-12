# Readable stream asynchronous iteration increment

The live file/DOM profile now exposes `values()` and its identical
`Symbol.asyncIterator` method. Native iterator objects inherit the shared
AsyncIterator prototype and use actual default readers. Their `next()` and
`return()` calls serialize through Promise jobs. EOF and source errors release
the reader; early return cancels by default, while `preventCancel` releases the
reader with unread data intact. Return values and chunks retain identity,
including Promise-valued chunks. Invalid iterator receivers reject Promises;
invalid options or an already-locked source reject iterator creation.

The implementation uses the [Streams asynchronous iteration
model](https://streams.spec.whatwg.org/#rs-asynciterator), the
[Web IDL iterator machinery](https://webidl.spec.whatwg.org/#es-asynchronous-iterable)
and [ECMAScript AsyncIteratorClose](https://tc39.es/ecma262/multipage/abstract-operations.html#sec-asynciteratorclose)
as behavior references. A local Node v26.7.0 comparison confirms raw
Promise-chunk identity; this is reference evidence rather than browser acceptance.

The first executable candidate passes nine of ten iteration cases and exposes
an existing for-await defect: its abrupt cleanup calls return synchronously
without awaiting completion. The compiler now wraps the whole for-await loop
in an asynchronous finalizer. Its appended bytecode operation preserves prior
opcode identities and resumes only after the close Promise settles. A body
throw keeps precedence over close failure; a break or return observes that
failure. A failed next does not close the iterator. The local loop exit remains
inside the try region so break cleanup cannot reenter an already-completed
finalizer. Finished iterators release their last operation and ignore unused
next arguments, allowing temporary values to be collected.

BlueTS V10 adds a 40-binding profile for checked values/next/return calls, while
preserving earlier profiles. Inspection also finds the classic/module compiler
option route selecting a newer profile with the old V7 verifier. Both routes
now use the same V10 declarations; an authorized module preparation regression
covers iteration, tee and node dispatch together. Keyword-named method calls
such as `.return()` now retain result inference, argument validation and direct
lowering. Three compiler regressions cover valid and invalid calls; a typed
native iterator-return case also executes through the bridge. Anonymous arrow
syntax remains a known direct-bridge limitation; that execution case uses a
named callback without changing the API operation it verifies.

The actual Core/isolated-child regression uses for-await over Blob bytes to
update the live DOM and checks that EOF releases its lock, alongside the
existing independent tee branches, selected FileReader bytes, CustomEvent,
node events and native input trust. The software cohort passes all 1,110
BlueJS library/related cases, including 17 iteration cases, and all 148 bridge
cases. The complete BlueTS crate passes 357 cases with 60 optional oracle cases
ignored. The authorized module-profile preparation case and the actual Core/isolated-
child browser case also pass. The latter retains exact `3,0,255` Blob bytes,
reports an unlocked stream at EOF and issues only the original HTTP request.
Five-crate all-target strict Clippy and workspace formatting pass. No private
Core/BlueJS process or file-reader fixture remains after the browser test.
The [receipt](artifacts/stream-iteration-results.json) records source and log
hashes, earlier failures and the remaining acceptance scope. Writable/transform/pipe streams, wider conformance and fresh
integrated workspace/native/Windows plus physical-system acceptance remain
required.
