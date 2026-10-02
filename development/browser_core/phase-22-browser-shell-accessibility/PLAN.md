# Phase 22 — Browser Shell, Native Interaction, and Accessibility

[← Back to plan](../BROWSER_CORE_PLAN.md)

**Status**: In progress — the Unix reference frontend, an initial Windows WinUI 3 shell and a macOS SwiftUI/AppKit shell with supervised navigation and an initial macOS page NSAccessibility bridge present the core's pixels, semantics and basic input. Complete browser-shell behavior and full operating-system page accessibility remain open.

## Windows first slice (2026-10-02)

[`frontend/winui`](../../../frontend/winui/README.md) implements an unpackaged,
self-contained x64 WinUI 3 application with a native address field, tab
creation/selection/closing, back/forward/reload, settings navigation, viewport
resize and basic pointer/scroll/character input. A private child-process
stdin/stdout connection uses the existing version-two browser envelopes;
`blueice-core --stdio` owns the same `TabManager` and render/session code as
the Unix entry point. Frame pixels remain in bounded, generation-specific
memory-mapped files rather than entering the control channel.

The first slice supports core-rendered built-in pages. Windows still lacks
the launcher/gatekeeper/service transports, so external navigation returns
the existing unavailable-gatekeeper response. No review is bypassed, and
this private mode does not yet provide a shared launcher/MCP rendezvous.
Trusted permission/assistant panels, full IME/editing, group controls,
clipboard, printing, multiple windows, localization and page accessibility
bridges remain separate delivery items. The earlier Unix transport and
launcher behavior are unchanged.

Regression tests cover actual child-process rendering, request/tab identity,
fragmented pipe reads across session polls, oversized messages, unavailable
review without any outgoing HTTP connection, and ownership-preserving frame
directory cleanup. The C# adapter also tests wire framing and memory-mapped
RGBA-to-BGRA conversion. `frontend/winui/tests/WindowSmoke.ps1` drives the
real native window using UI Automation; native validation is recorded in
[`WINDOWS_RESULTS.md`](WINDOWS_RESULTS.md).

## macOS first slice (2026-10-02)

[`frontend/macos`](../../../frontend/macos/README.md) supplies a dependency-free
Xcode project using SwiftUI browser chrome and an AppKit-owned window and
pixel/input viewport. It bundles the existing private-pipe core and supports
built-in navigation, independent tabs, back/forward/reload, resizing, basic
committed text and pointer/scroll input. AppKit termination waits for owned
child-process and frame-directory cleanup. This slice shares the Windows
private-pipe service limitations and does not implement page NSAccessibility,
complete IME/document editing or the remaining shell/service features.

XCTest covers protocol/frame boundaries and a real core process. XCUITest
drives the actual native app and verifies visible pixels, history, committed
reload URLs, independent tab state, last-tab closure, resizing, fail-closed
navigation, window exit and startup failure. Native results are recorded in
[`MACOS_RESULTS.md`](MACOS_RESULTS.md).

## macOS supervised navigation (2026-10-02)

The default SwiftUI/AppKit app now bundles and starts `blueice-launcher`,
`blueice-core` and `blueice-ai-gatekeeper`. It connects over an owned private
Unix socket, enabling HTTP(S) navigation through the existing URL/content
review flow. The explicit `--core-exe` diagnostic mode retains the fail-closed
private-pipe contract. Persistent gatekeeper settings keep their normal path.

Short, mode-0700 runtime paths accommodate Darwin's socket limit. The frontend
creates a separate process group before spawning the launcher, requests normal
shutdown first, and bounds fallback cleanup to that owned group. Startup
failure, missing services and connection failure release owned resources.
An inherited lifetime pipe selects the launcher's `--exit-on-stdin-eof` mode,
which requests normal broker shutdown when the GUI disappears. The default
shared-broker lifetime remains unchanged without that opt-in.

Regression tests cover reviewed loopback HTTP pixels, malicious URL and hidden
prompt-injection denial, unchanged committed page/history after denial,
unavailable review before any HTTP fetch, startup cancellation and forced
descendant cleanup. Native XCUITest adds HTTP pixels, history/reload/recovery
and missing-launcher UI coverage. Results are recorded in
[`MACOS_SERVICE_RESULTS.md`](MACOS_SERVICE_RESULTS.md).

This does not complete shared-launcher attachment, assistant/permission panels,
IME/document editing or the page NSAccessibility bridge.

## macOS page accessibility bridge (2026-10-02)

The AppKit viewport now maps the core's existing `Representation` replies into
virtual NSAccessibility elements. The adapter exposes names, semantic roles,
hierarchy, values and disabled/required/selected/focused state, with document
scroll and backing-pixel coordinates converted into screen rectangles. Core
state supplies additive, backward-compatible `native_text_input` and
`protected` flags so the shell can restrict editing and redact password values.

Replies must match the live tab, frame-directory source, frame generation and
committed URL. Tree validation bounds node count/depth and rejects invalid
geometry, duplicate IDs, broken parent/child links and cycles. Bad semantic
payloads disable the page bridge while pixel transport continues. Reload and
navigation advance a per-tab document epoch; tab switches and document changes
invalidate old native elements. During ordinary frame refresh, elements retain
identity but actions wait for a matching representation.

Native press/focus uses the ordinary core click pipeline; text commits use
`InsertText`. The adapter does not use `ActOn::SetValue` to mutate the DOM.
Checkboxes, sliders and selects currently expose read-only state. XCTest
exercises native accessibility actions against a real owned service stack;
XCUITest verifies OS-visible semantics, typing/privacy, reviewed link navigation
and tab isolation. Results are recorded in
[`MACOS_ACCESSIBILITY_RESULTS.md`](MACOS_ACCESSIBILITY_RESULTS.md).

Interactive VoiceOver speech/navigation, full ARIA and accessible-name behavior,
text ranges/selection, live regions, rotor search, IME and the remaining shell
features remain open. The macOS bridge does not complete the cross-platform
Phase 22 acceptance criteria.

## Objective

Deliver a human browser experience that can use the same pages an AI can inspect: correct native input and text editing, tabs/windows/downloads/printing/permissions, system accessibility and display adaptation. The shell is a client of `core`; it does not own DOM, navigation policy, layout or a second page instance.

## Decisions

### Native input and editing

Define `blueice_ipc::input` as the canonical event stream from any frontend, automation client or permitted candidate presenter. It carries physical/logical keyboard state, IME composition/commit/cancel, pointer/touch/pen contacts, wheel/gesture, focus, selection, drag/drop, clipboard and file-selection results with viewport/device-scale/window/document generations. Phase 20 dispatches it through the real DOM event/default-action model; the frontend never guesses a form control's semantics.

Text selection, caret/selection geometry, copy/cut/paste, drag/drop payloads, virtual keyboard and input-method candidates remain document-originated state with a privacy policy. A page cannot read arbitrary system clipboard/file content without permission and a user gesture. An AI may request a normal interaction under a controller lease, but it cannot impersonate human IME, disclose clipboard/file bytes, or dismiss a permission/file-picker dialog.

### Browser chrome and operating-system integration

Provide a first-party shell contract for tab strip/group UI, multiple top-level windows, profiles/contexts, address/search UI, navigation controls, zoom/text scale, fullscreen/picture-in-picture, context menus, find-in-page, print/print preview, downloads shelf, file picker, crash/reload UI and per-site permission prompts. Phase 16's `TabManager` remains the source of truth for tabs/groups; the shell only renders and controls it through versioned IPC. Window state, DPI/multi-monitor changes, color space, dark/light mode, high contrast and reduced-motion settings are inputs to one viewport/media-query/compositor state.

Downloads use Phase 10's separate manager. Print output is generated from a print media/layout profile with resource/pagination limits and follows the same document/security policy as screen rendering. A browser-chrome permission prompt is human-only: MCP may inspect its existence and request that the human be prompted, but only the shell's visible user decision creates a capability grant.

### Accessibility as a platform boundary

Phase 1's accessibility-tree-shaped representation becomes the single source for AT-SPI, UI Automation, NSAccessibility and equivalent platform bridges. It maps DOM/ARIA/HTML semantics, names, states, relations, bounds, focus, selection, values, live regions, actions and text ranges to platform APIs, with stable IDs and document generations. Assistive-technology actions return through the same input/default-action path as ordinary user interaction; no platform bridge directly mutates DOM.

The bridge follows WCAG 2.2-relevant keyboard, focus, contrast, reflow, motion and alternative-content expectations, with the exact supported criteria/version exposed as a capability report. Human and AI diagnostics must see the same resolved accessible name/role/state and the source/provenance that produced it; private text and password values remain redacted according to caller scope.

### AI MCP integration

`shell_*` and `accessibility_*` MCP capabilities expose window/tab/profile metadata, viewport/zoom/theme state, bounded input/focus/selection traces, download/print states, permission-prompt state and platform-independent accessibility snapshots/actions. A control action needs the normal controller lease; accessibility actions follow the same policy as their equivalent user action. There is no MCP operation to silently grant permission, read a password/clipboard/file, fabricate a trusted user gesture, or control an unregistered OS window.

## Delivery and acceptance

1. Implement canonical input/IME/selection/clipboard/file-picker protocol and deterministic event-sequence tests.
2. Build tab/window/profile/download/print/permission shell UI on the existing launcher/core boundary; retain same-page/core guarantees across every shell surface.
3. Add OS accessibility bridges and test with real screen-reader/automation adapters on supported platforms.
4. Add `shell_*`/`accessibility_*` MCP resources and prove they correlate to the same DOM/frame/action generations visible to a human.

Acceptance requires CJK/RTL IME editing, keyboard-only form completion, selection/clipboard policy denial, touch/pen/drag input, DPI/theme/reduced-motion transition, a screen-reader action, download/print/permission flow and multi-window/tab handoff. Tests must prove no AI action bypasses the visible human permission or privacy boundary.

## Explicit non-goals

- A shell-owned parallel DOM or renderer.
- Treating programmatic MCP input as a trusted human gesture.
- Exposing OS credentials, clipboard, files, notifications or permissions by default.
