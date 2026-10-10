# macOS native hover contract

Status: accepted 2026-10-10. Complete macOS delivery remains in progress.

AppKit tracking areas deliver page movement and entry without requiring page
keyboard focus. Coordinates convert from flipped view bounds into CSS viewport
coordinates, independently of backing density. Leaving explicitly clears shared
core hover state; it does not simulate a point outside a scrolled document.

`NativeHover` carries a versioned pointer document context and displayed
frame generation. Core validates source and document identity, current frame
generation and finite in-viewport coordinates for movement, and the existing browser-context/window/tab wrappers
before changing state. A null point means leave and is document-fenced,
independent of keyboard focus and intervening frame changes. A public regression
reproduced a rejected leave after actual keyboard focus and frame changes;
its unchanged method remains required for acceptance. Successful commands are silent,
like the existing `Hover`; `GetRepresentation` observes the same core-owned
`NodeState.hovered`. Pointer movement grants no activation, navigation, editor,
file picker or permission authority. Core retains ordinary inert/paint-order hit
resolution. This increment does not establish CSS pseudo-class rendering or DOM
pointer event dispatch.

The model coalesces queued movement while retaining ordered leaves for previous
owners. It requests a fresh representation after sending hover, including when
an earlier representation request is already in flight. Selection changes,
interaction suspension, viewport changes, loss of key window, sheets and view
removal invalidate native hover. Late commands remain subject to core fences.

Verification uses an independent read-only core connection scoped to the owned
runtime, browser context, window and tab. A native XCTest reproduced the missing
bridge on the accepted parent: both link movements left actual core hover empty.
That unchanged-method regression, public core fence/movement/scroll tests,
real OS pointer XCUITest, full native acceptance and required Rust gates
passed on the same frozen program/build inputs. Physical host input and
remaining macOS delivery requirements retain their existing separate status.

Focused evidence now passes 74 public tests across native keyboard, viewport and
window targets, plus all 12 selected native methods (six XCTest and six actual
XCUITest), without failures, ignored cases or skips. A second public red-to-green
proof preserves the complete window-handoff test file and changes exactly one
production file: editor transfer clears hover for both MoveTab and cross-window
PlaceTab, while same-window organization/refused transfer retain hover and old
window callbacks are rejected.

The OS UI observer uses the existing external fixture harness because the
XCUITest Runner rejects lsof PID enumeration and private Unix-socket
connections. A PID-scoped descriptor probe also returned no usable list;
its specific kernel cause was not established. Its token-authenticated loopback lookup checks exact built GUI and
launcher paths, their parent relationship before/after discovery, and private
runtime ownership/mode. Observation sends only Hello/GetRepresentation with
explicit browser-context/window/tab scope to the ordinary core socket. Native
XCTest separately observes the same core state directly. Earlier setup/observer
failures remain preserved; no sandbox, entitlement or system setting changed.

Complete gates passed on the same 1,837 unchanged program/build inputs.
Fresh Rust acceptance passed 7,406 cases with 69 ignored; unfiltered native
acceptance passed 279 methods with the same physical Zhuyin skip. See the
[dated results](MACOS_NATIVE_HOVER_RESULTS.md) and
[machine-readable receipt](artifacts/macos-native-hover-results.txt). The whole
macOS delivery goal remains open under MACOS_DELIVERY_PLAN.md.
