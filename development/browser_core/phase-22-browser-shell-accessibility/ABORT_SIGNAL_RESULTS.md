# Abort and listener lifecycle increment

AbortController and AbortSignal are now owner-installed browser APIs. Signals
inherit EventTarget, preserve exact reason identity, expose readonly state and
throwIfAborted, and support abort/any/timeout plus onabort. Default reasons use
AbortError/TimeoutError DOMException objects. Abort events use the ordinary
branded Event interfaces, with trusted, noncancelable delivery.

Generic EventTarget, FileReader and admitted native click/file/drag listeners
accept signal options. Aborting removes listeners before abort event delivery,
including callbacks pending in an active dispatch. Removal/once detach abort
algorithms. Cancelling a FileReader listener leaves its data read running.

Reasons, callbacks and cancellation links are GC-owned and charged to the
managed-byte budget. Source/dependent graph edges are weak. The collector's
existing ephemeron worklist conditionally preserves observed dependent signals
from independently live sources. Source-less and unobserved graphs are reclaimed.
Timeout tasks use monotonic deadlines in the existing bounded browser scheduler;
future deadlines yield without blocking. Timer roots follow live observers,
including observed dependent signals. Existing task/listener limits remain.

BlueTS uses a new 36-binding abort V6 profile. Its changed listener-options
schema is confined to that profile; all preceding profiles keep their original
bytes/inventories. Type tests use named callbacks supported by the current direct
lowerer. The initial anonymous zero-parameter arrow fixture exposed an existing
unsupported-expression limitation; broader direct BlueTS arrow lowering remains
open and is not claimed by this API/type increment.

The final cohort passes 88 tests with no failures or ignores across 13 groups.
It includes 13 abort/lifecycle cases, preceding FileReader/stream/URL coverage,
all selected prior/new host profiles, Core/context fencing, and actual
Core/isolated-child browser cases. The real selected-file flow verifies that
cancelled native input and FileReader load listeners never run while selected
bytes still reach the live DOM. The Blob URL browser flow still passes.
Strict all-target Clippy passes for BlueJS, BlueTS/BlueJS, launcher and engine;
workspace format check passes. The [receipt](artifacts/abort-signal-results.json)
records final hashes and retained failed attempts. New complete workspace/native/
Windows acceptance remains pending after integration.

Further DOM work includes general synthetic node events, CustomEvent and broader
lifecycle/tree APIs, followed by additional WPT coverage and resource/Fetch
cancellation integration. Actual physical-system acceptance is independent.
Behavioral reference: [DOM Standard](https://dom.spec.whatwg.org/#aborting-ongoing-activities).
