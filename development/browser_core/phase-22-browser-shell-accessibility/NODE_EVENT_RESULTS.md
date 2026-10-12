# Live node event dispatch increment

The owner-selected live file/DOM profile now installs arbitrary named node
listeners and dispatchEvent. Core supplies target-first ancestry for the exact
tab/document generation through authenticated Script IPC v9. Paths use
child-private handles and retain the existing 256-node bound; foreign,
unknown, reclaimed and stale-document handles are denied. Detached text nodes
remain valid targets and gain current ancestry after append.

The VM snapshots ancestry once per dispatch, then takes live listener snapshots
at capture, target and bubble phases. It preserves wrapper/event/detail identity,
composedPath, readonly trust, cancellation, passive/once listeners, signal removal,
object callbacks, exceptions and distinct-event reentrancy. Invalid paths dispatch
no callback and leave the event inactive. Node wrappers inherit EventTarget;
borrowed EventTarget methods validate the same exact private receiver. Dynamic
listener names are charged to collector-owned metadata and released with their
registration. Existing wrapper/listener limits remain 4,096 per family.

In that profile, owner-delivered click and file-selection events now use branded
Event state and the same node dispatch loop. Their trust and cancellation results
stay separate from script-created events. Script dispatch does not create a
native input grant. The old narrower embedding profiles retain their behavior.
The implementation follows the [DOM dispatch model](https://dom.spec.whatwg.org/#concept-event-dispatch)
for the currently exposed tree; Shadow DOM retargeting and further HTML activation,
native MouseEvent geometry and event subclasses remain future work.

BlueTS V8 has 38 verified bindings and retains every preceding profile. Its node
declarations include both specific event overloads and checked string fallbacks.
A regression exposed missing nested named-function parsing/scope/lowering. The
parser now records independent nested bodies, the checker hoists their signatures
in their lexical scope and restores surrounding declarations, and direct lowering
preserves closure captures and pre-declaration calls. Four public compiler/VM
cases verify those behaviors, nonleaking signatures and a 64-level parser bound.
This does not claim complete TypeScript syntax or full DOM conformance.

The final focused cohort passes 70 VM/type/compiler cases and nine Core/IPC cases,
with zero failures/ignores. An actual Core/isolated-child case also passes:
CustomEvent updates the live DOM through document/parent capture and bubbling;
a plain synthetic click remains untrusted with an empty file collection; the
subsequent real selected-byte input is trusted and branded, reaches document
capture, and retains the exact FileReader bytes. Only the original HTTP request
occurs. All 354 BlueTS cases pass; 60 optional pinned-oracle cases remain ignored.
All 919 BlueJS library cases and all 145 BlueTS/BlueJS bridge cases also pass.
Six-crate all-target Clippy and workspace formatting pass. The
[receipt](artifacts/node-event-results.json) records source/log hashes and failed
attempts. Fresh complete workspace/native/Windows gates remain required after
integration, independently of physical-system acceptance.
