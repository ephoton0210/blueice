# Readable stream tee and Promise collection increment

The live file/DOM profile now exposes `ReadableStream.prototype.tee()` with
two actual streams, Promise-based readers and a locked original source. Generic
chunks retain identity between branches. Byte streams copy data for the other
branch and forward actual BYOB demand to the source, preserving transferred
buffers, view types, element alignment, minimum reads and zero-length EOF
responses. Source subclasses retain their constructor prototype; tee branches
use the ordinary ReadableStream prototype.

Each branch can read independently. Cancelling one branch leaves the other
active and waits for its completion. Cancelling both calls the original source
once, with ordered reasons, and adopts its cancellation Promise. Source errors
retain the exact reason and precede queued chunk delivery. Reader switching
ignores errors from a released former reader. The existing 4,096-chunk queue
bound remains enforced; a slow branch hitting that bound errors both branches
with the same reason and cancels the source.

That quota regression exposed an existing VM lifetime defect: every Promise
record was an unconditional GC root. Internal stream reactions exhausted the
unchanged 16 MiB heap before reaching the queue bound. Promise results and
reactions now trace from their live heap owner through the collector worklist;
unreachable records and cycles can be reclaimed. Retained resolving functions,
queued jobs and active async calls still keep their required state alive.
Minor collections also trace young reaction targets from old Promise owners.
Wider async regressions exposed missing temporary roots during synchronous
iterator adaptation and async call setup. Active Promise jobs now retain their
edges across module/async operand-stack replacement, and waiting dynamic imports
have explicit roots. These lifetimes are now explicit.

BlueTS V9 adds a verified 39-binding profile with a checked two-stream tuple.
Earlier host profiles remain available. The actual Core/child regression reads
the left Blob branch through BYOB, mutates its bytes, and reads the original
bytes from the right default-reader branch into the same live DOM.

This increment does not establish full Streams conformance. WritableStream,
TransformStream, pipeTo/pipeThrough and async iteration remain open, together
with broader browser and physical-system acceptance. All 919 BlueJS library
cases and 174 related Promise/async/module/GC/stream cases pass, with zero
failures or ignores. That cohort includes six new Promise collection cases,
14 tee cases and all 20 existing readable-stream cases. All 146 bridge cases, including the V9 profile, also pass. The actual
Core/isolated-child case passes with exact independent branch bytes, existing
FileReader/CustomEvent/node-event behavior, trusted native selection and only
the original HTTP request. Four-crate all-target strict Clippy and workspace
formatting pass. The test leaves no private Core/BlueJS process or file-reader
fixture directory. Fresh complete workspace/native/Windows acceptance remains
required after integration. The [receipt](artifacts/stream-tee-results.json)
records source and log hashes, failed attempts and the remaining gates.
