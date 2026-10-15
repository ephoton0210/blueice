# Page exception reporting

[← Browser integration TODO](BROWSER_INTEGRATION_TODO.md)

M1o completed on 2026-10-15. Public VM/profile and real Core/MCP acceptance,
all 24 related Rust targets (3,101 passed; one existing ignored), fresh focused
MCP acceptance (one passed), formatting and workspace/all-target Clippy/build
pass. All 102,926 pinned Test262 modes match M1n's accepted source/result
inventory. See the [milestone record](BROWSER_INTEGRATION_TODO.md#milestone-record).

## Owner and profile

The owner-selected File DOM profile advances to
`core-script-dom-errors-v15`. Its 58 bindings extend the unchanged v14 console
inventory with `ErrorEvent`, `reportError`, `onerror` and the three global
EventTarget methods. Generated window members use the same declarations.
Older artifacts remain immutable and reject these absent globals.

Installation requires the owner's EventTarget and console bindings, refuses
existing global names and attaches collector-owned listener state to the
existing realm global. It does not install a `Window` constructor, nested
browsing contexts or a navigation-stable WindowProxy. Replacement creates a
fresh realm with fresh handlers, listeners and diagnostics.

## Events and exceptions

The [HTML exception-reporting algorithm](https://html.spec.whatwg.org/multipage/webappapis.html#report-an-exception)
defines the event/cancellation/recursion behavior. Reports synchronously fire
a trusted, cancelable, non-bubbling `ErrorEvent` at the global object, retaining
the original error value in the event. The global handler receives five
arguments; a true return cancels default reporting. Listener cancellation,
once/passive/signal options and handler replacement use existing EventTarget
storage and dispatch. Script-created events remain untrusted.

An active reporting guard suppresses recursive error-event dispatch while
still allowing uncanceled nested errors to reach diagnostics. Error dispatch
does not start a nested microtask checkpoint. Existing instruction, heap and
scheduling failures remain document failures and cannot be canceled by a
page handler.

Script, task, microtask and native-listener boundaries share the reporter.
Page module records retain a one-time reporting flag until their evaluation
finishes, including later top-level-await rejection. Dynamic imports retain
their Promise rejection behavior; general `unhandledrejection` tracking is
the separate open item in section B. Loader/compiler admission failures keep
their existing rejection reports.

The original public execution error remains observable to the host. Reporting
preserves native debugger throw-site metadata and roots thrown objects through
allocation and callback dispatch. Event/listener edges use the existing heap
tracer; copied diagnostic messages retain no live error value.

## Diagnostic metadata and transport

HTML leaves extraction metadata implementation-defined. This implementation
uses the existing bounded, getter-free value preview for `message`; generated
reports use an empty `filename` and zero `lineno`/`colno` because this boundary
does not yet have an authorized original source position. It does not invent
BlueTS locations; section H's source-mapping follow-up remains open. Explicit
`ErrorEvent` construction converts the supplied dictionary members.

Uncanceled output uses the console's `error` method/level with an `Uncaught `
prefix and the existing message/record/byte limits. Native payload accounting,
exact-document Core validation and MCP's untrusted-content framing apply to
the same copied batch. No new wire field or private protocol version is needed;
the current bounded drain remains private v60. Draining never executes script
or resumes jobs and returns no live exception handle.
