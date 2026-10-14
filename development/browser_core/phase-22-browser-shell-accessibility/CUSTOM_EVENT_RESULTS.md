# CustomEvent increment

CustomEvent is now an owner-installed Event subtype. It retains arbitrary
detail values by identity, preserves those values through nursery/major GC,
and charges string, Symbol and BigInt payloads to the existing managed budget.
Its prototype/constructor inheritance, readonly enumerable detail getter,
subclasses and legacy initCustomEvent follow the
[DOM Standard](https://dom.spec.whatwg.org/#interface-customevent).
Legacy initialization converts arguments before checking active dispatch,
cannot mutate a dispatched event, and clears cancellation/target state after
dispatch. Base initEvent leaves custom detail intact. Native getters and
methods reject sibling event brands. Script-created events remain untrusted.

The additive BlueTS V7 profile has 37 bindings and leaves all previous
profiles unchanged. The actual launcher selects that verified profile and
installs the same global. The initial declaration's parameter name `type`
exposed an existing reserved-name parsing limitation; renaming the parameter
to `eventType` preserves its type and runtime argument behavior.

Four original missing-interface failures are retained. The final cohort
passes 102 cases with zero failures/ignores across twelve VM/type groups,
including five CustomEvent cases and preceding event, abort, FileReader,
stream and URL regressions. Actual Core/isolated-child integration also
passes: a generic EventTarget receives the exact custom payload and writes
`custom 中文:false` into the same live Core DOM while existing selected-byte
and cancellation assertions still pass. Strict all-target Clippy passes for
BlueJS, BlueTS/BlueJS and launcher, and workspace formatting passes. The
[receipt](artifacts/custom-event-results.json) records source and log hashes,
commands/scopes and retained failures.

General synthetic node dispatch, capture/bubbling across Core-owned trees,
additional DOM/lifecycle APIs and broader WPT conformance remain open.
This focused increment does not establish fresh complete workspace, native,
Windows or physical-system acceptance after integration.
