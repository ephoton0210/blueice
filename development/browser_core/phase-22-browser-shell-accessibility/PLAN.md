# Phase 22 — Browser Shell, Native Interaction, and Accessibility

[← Back to plan](../BROWSER_CORE_PLAN.md)

**Status**: In progress — the Unix reference frontend, an initial Windows WinUI 3 shell and a macOS SwiftUI/AppKit shell present the core's pixels. macOS includes supervised navigation, an initial page NSAccessibility bridge and core-owned native text editing. Actual OS IME verification, complete browser-shell behavior and full operating-system page accessibility remain open. The [macOS delivery milestones](MACOS_DELIVERY_PLAN.md) track scoped commits and remaining work.

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

## macOS native editing foundation (2026-10-02)

The additive `blueice_ipc::input` protocol now carries core-owned UTF-16
selection, replacement, marked-text update/commit/cancel, grapheme movement and
deletion, and pointer selection. Commands identify the current frame source,
document and focus generations. Stale commands cannot target another document
or another focused field. Legacy committed-text commands remain available for
existing platform clients.

The core edits ordinary text/password inputs and textareas, preserves multiline
whitespace, lays out masked password runs and emits caret/selection geometry.
Paint/raster clipping confines content, marked underlines and selection/caret
pixels to the resolved control content box. Password plaintext is absent from
input-state replies and semantic values; readonly writes and disabled focus are
rejected. Extension DOM-write capabilities remain independently restricted.

AppKit implements `NSTextInputClient` from core state and serializes callbacks
through acknowledgements. Native candidate positioning, selection and explicit
copy/cut/paste use that state; the frontend retains only temporary IME range
metadata while awaiting core replies. A native SwiftUI command menu selects the
current responder's keyboard input source. Results and limitations are recorded
in [`MACOS_NATIVE_EDITING_RESULTS.md`](MACOS_NATIVE_EDITING_RESULTS.md).

The recorded run passes 33 XCTest and 14 native-window XCUITest cases, with one
physical system-Zhuyin test explicitly skipped because the Runner lacks macOS
Accessibility permission. Direct AppKit composition tests passed, but actual
OS IME acceptance remains pending. JavaScript input/composition/key events,
undo/redo, complete bidirectional shaping, keyboard-only forms and the remaining
native-shell/accessibility milestones are still required.

## macOS keyboard control increment (2026-10-02)

The core-owned native input path now carries closed Tab/Enter/Space/arrow/Home/End
intents. Core chooses sequential focus from the current document, skips
disabled/hidden/inert/negative-tabindex controls and retains DOM starting
position for programmatic negative focus. Disabled fieldsets preserve the first
legend exception and do not disable ordinary links. Checkbox/radio defaults,
form-owner radio grouping, single-select option changes and decimal range keys
produce values and pixels through the shared core pipeline.

Keyboard activation calls the existing pre-default click listener; stale
contexts are rejected before event dispatch, and cancellation or document
replacement suppresses the old default. Native AppKit buffers bounded events
across Tab acknowledgement, then replays them to the newly focused page control
or SwiftUI address field. Command-L focuses the address editor. Real-service
XCTest and actual-window XCUITest cover rapid callbacks, controls, link
navigation and immediate typing after reverse page exit. Public Rust IPC
regressions also check label/pointer parity and shared static/interactive paint.
Evidence is recorded in [`MACOS_KEYBOARD_RESULTS.md`](MACOS_KEYBOARD_RESULTS.md).

This is a control/focus increment. Form submission/reset and validation,
select popup/typeahead/multiple interaction, DOM keyboard/input/focus events,
complete chrome traversal and the remaining Phase 22 acceptance remain open.

## macOS native form reset increment (2026-10-02)

Core-owned original defaults now survive live native and extension edits.
Keyboard, pointer and ordinary node activation reset the current form owner's
controls, including external associations, and keep unrelated forms/tabs intact.
Reset discards composition and invalidates old editing contexts while keeping
the document identity. Shared core paint and semantics expose input-button
captions. Page-script textarea markup changes update its retained default.
See [`MACOS_FORM_RESET_RESULTS.md`](MACOS_FORM_RESET_RESULTS.md) for the measured
native and public-boundary evidence. Full DOM reset-event dispatch, GET/POST
submission, validation, dirty value/default properties and file selection remain
required before full form acceptance.

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

The macOS GET/POST and resubmission increment is recorded in
[MACOS_FORM_SUBMISSION_RESULTS.md](MACOS_FORM_SUBMISSION_RESULTS.md). It delivers
core-owned current-tab submissions and a native POST confirmation while the
[macOS delivery plan](MACOS_DELIVERY_PLAN.md) retains the remaining browser scope.

The macOS find increment adds core-owned text/geometry search, native
NSSearchField focus and keyboard UI, lifecycle/privacy bounds and ordered
clipboard commands. Its public, real-service and actual-window evidence is
recorded in [MACOS_FIND_RESULTS.md](MACOS_FIND_RESULTS.md). The full macOS
[delivery plan](MACOS_DELIVERY_PLAN.md) remains active.

The native context menu increment adds AppKit link/editor/page actions backed by
core hit testing, exact lifecycle fencing, protected/readonly clipboard policy,
reviewed new-tab navigation and correlated visible denial. Its Rust boundary,
real-service and actual-window verification is recorded in
[MACOS_CONTEXT_MENU_RESULTS.md](MACOS_CONTEXT_MENU_RESULTS.md). Image/media
actions, page-text selection, drag/drop and the full delivery plan remain open.

The macOS display preference increment adds persistent native appearance,
contrast and motion controls, system observation, core inline media-query
evaluation and sRGB output. Actual backing density remains available to CSS
resolution even when raster density is capped. Native/core and actual-window
evidence is recorded in [MACOS_DISPLAY_PREFERENCES_RESULTS.md](MACOS_DISPLAY_PREFERENCES_RESULTS.md).
Physical system/monitor transitions and the full macOS delivery plan remain open.

The macOS native tab-group increment connects the Phase 16 shared group state
with SwiftUI sheets, native tab/group/View menus, collapse and nullable membership.
Core IDs, pages, editor values, history and zoom remain intact when groups change
or dissolve. Native validation/cancel, empty/stale groups and dedicated error
notices are recorded in [MACOS_TAB_GROUP_RESULTS.md](MACOS_TAB_GROUP_RESULTS.md).
The later shared-core window increment delivers multiple-window viewports. Profile lifecycle and the full delivery plan remain open.

The macOS shared-core window increment adds an opt-in canonical window registry,
window-local viewports and existing-page tab transfer. Command-N, Window menu
activation and tab context-menu transfer create actual AppKit windows over one
owned service session. Closing the source window leaves the destination live;
active editing/find/group menus follow it. Transfer retains committed values,
selection, history, group membership, zoom and find state, while fencing stale
source-window commands and ending composition. New tab membership precedes review,
so denied pages remain visible and closable. The native and core regression record
is [MACOS_WINDOW_RESULTS.md](MACOS_WINDOW_RESULTS.md). Session restoration, drag
reordering and the other delivery milestones remain open.

The subsequent native profile increment adds core context ownership, scoped
window/group lifecycle and native create/rename/remove/open actions. Same-context
transfer retains the page; cross-context transfer is rejected. Bounded preferences
retain names and logical UUID keys across fresh runtime context IDs, without
restoring pages or fetching URLs. MCP can read the same canonical registry through
list_browser_contexts. Acceptance is recorded in
[MACOS_CONTEXT_RESULTS.md](MACOS_CONTEXT_RESULTS.md). Durable tab restoration,
storage partitioning and private browsing remain separate requirements.

The native download increment connects a shared on-demand manager to SwiftUI
controls and AppKit linked-file actions. It shares browsing's owned Gatekeeper,
persists paused transfers after normal/forced GUI exit, and opens/reveals actual
completed files through macOS. The restart revision counter preserves monotonic
updates so recovered controls remain live. Acceptance is recorded in
[MACOS_DOWNLOAD_RESULTS.md](MACOS_DOWNLOAD_RESULTS.md); automatic response
downloads, native destination/credential UI, quarantine and print/PDF remain open.

The macOS live-region/rotor increment adds bounded core-owned announcements,
privacy-safe native names, and document/frame-fenced reading-target reveal.
AppKit advertises directional filtered rotors while retaining editor focus and
rejecting stale/foreign items. Full native acceptance and host-prepared Rust
verification, including unsuccessful default-runner startup attempts, are
recorded in [MACOS_LIVE_REGION_ROTOR_RESULTS.md](MACOS_LIVE_REGION_ROTOR_RESULTS.md).
The subsequent retained-delivery increment preserves ready announcements through
layout gaps and releases only source/document-scoped observed prefixes. Its
complete native and normal-runner Rust acceptance is recorded in
[MACOS_ANNOUNCEMENT_DELIVERY_RESULTS.md](MACOS_ANNOUNCEMENT_DELIVERY_RESULTS.md).
The descendant increment preserves each contributor's atomic/relevant scope,
expands public groups/author labels once in document order and retains label
privacy provenance. Its complete native and normal-runner Rust acceptance is in
[MACOS_DESCENDANT_LIVE_RESULTS.md](MACOS_DESCENDANT_LIVE_RESULTS.md).
The document-selection increment supplies core-owned UTF-16 selection and paint,
pointer/keyboard Select All and Copy, read-only AppKit AX text ranges, link-drag
isolation and bounded input buffering through frame/focus gaps. Complete native
acceptance passed 184 methods with one existing physical Zhuyin skip; complete
Rust workspace acceptance passed 7302 cases with 69 ignored. Earlier unsuccessful
attempts and the unchanged-source/product audit are recorded in
[MACOS_DOCUMENT_SELECTION_RESULTS.md](MACOS_DOCUMENT_SELECTION_RESULTS.md).
Complete ARIA semantics, physical VoiceOver and the remaining browser requirements
stay open in the macOS delivery plan.
