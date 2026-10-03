# BlueIce macOS frontend

The macOS frontend uses SwiftUI for browser chrome and AppKit for the
window lifecycle, core pixel viewport, pointer/scroll input, native text
editing and the page NSAccessibility adapter. By default it launches the bundled `blueice-launcher`, which
supervises the real core and gatekeeper. A private Unix socket carries the
browser protocol; the Rust core owns pages, history, tab identities and rendering.
HTTP(S) navigation follows the existing URL and content review policy.

## Build and run

Requires macOS 14 or newer, a full Xcode installation with its command-line
tools selected, and the repository's Rust 1.96 toolchain. No third-party
Swift packages or project generator are required. Builds target the host's
architecture; Intel builds have not been validated on this Apple Silicon host.

```sh
frontend/macos/build.sh
open frontend/macos/.build/Build/Products/Debug/BlueIce.app
```

`build.sh Release` builds optimized services and frontend. The Xcode project
copies and locally signs `blueice-core`, `blueice-launcher` and
`blueice-ai-gatekeeper`, plus the on-demand `blueice-downloads` and
`blueice-extension-host` and `blueice-ai-assistant`, inside the app bundle. It
reuses `.build/core-target` for Rust output, keeping native frontend builds
independent of the workspace's larger cache; an explicit `CARGO_TARGET_DIR`
overrides that directory. Builds use an ad hoc identity for local development;
distribution signing and notarization are separate work. To build from Xcode, first build the services
with the script, then open `BlueIce.xcodeproj` and select the shared `BlueIce`
scheme. `BLUEICE_BACKEND_DIR` overrides the directory of those six binaries
when invoking Xcode directly. When running the app executable directly,
`--launcher-exe /absolute/path/to/blueice-launcher` overrides the launcher;
the core, gatekeeper and optional download service must reside beside it. The explicit diagnostic option
`--core-exe /absolute/path/to/blueice-core` retains the private-pipe mode,
which denies external navigation when review is unavailable.

## Automated validation

```sh
frontend/macos/test.sh
# Run only the protocol/real-process tests:
frontend/macos/test.sh -only-testing:ProtocolTests
# Run only the actual native window tests:
frontend/macos/test.sh -only-testing:BrowserUITests
# On this Apple Silicon host, validate the launcher against the native bundle:
CARGO_TARGET_DIR="$PWD/frontend/macos/.build/core-target" \
    CARGO_TARGET_AARCH64_APPLE_DARWIN_RUNNER="$PWD/frontend/macos/TestSupport/run-signed-test.sh" \
    BLUEICE_TEST_LAUNCHER_EXE="$PWD/frontend/macos/.build/Build/Products/Debug/BlueIce.app/Contents/MacOS/blueice-launcher" \
    "$HOME/.cargo/bin/rustup" run 1.96.0 cargo test \
    -p blueice-launcher --bin blueice-launcher --test owner_lifetime --locked
```

XCUITest requires an unlocked graphical login session and macOS approval for
Xcode's test runner to automate the UI. The tests operate the BlueIce
application; explicit download actions also inspect Finder and the default file
handler using uniquely named test documents. They use stable accessibility identifiers on native controls,
launch the bundled service stack, and inspect screenshots of the actual viewport.
They do not replace the core with a simulated renderer.

Protocol tests cover fragmented/truncated/oversized messages, Rust envelope
encoding, uncorrelated broadcasts, frame path/size/symlink boundaries,
startup failure, real core rendering, policy-reviewed HTTP pixels, malicious URL
and hidden prompt-injection denial, preservation of committed page/history,
unavailable review without fetching, cancellation during startup, and normal
and forced owned process-group cleanup. UI tests cover visible core pixels, settings
and back/forward history, reload of the committed URL, independent tabs and
closing the last tab, resize, visible navigation denial, window-close exit,
and startup failure with disabled controls. A loopback HTTP fixture supplies
safe and rejected HTML; the actual compiled gatekeeper rules review it.
The native-window tests also verify HTTP pixels, back/forward, reload of a
committed external URL, and recovery after content denial. Page accessibility
coverage includes typed representation decoding, bounded tree validation,
source/tab/frame/URL correlation, core-reported input capabilities, protected
value redaction, hierarchy/roles/state, scroll and Retina coordinate mapping,
native hit testing, stale element rejection, and object identity across frame
refreshes. A real-core XCTest invokes native accessibility focus/press and
checks ordinary input plus policy-reviewed link navigation. XCUITest reads the
OS-visible page elements and verifies typing, privacy and tab isolation.
The XCUITest runner's test-only network-server entitlement permits this
loopback fixture inside Xcode's runner sandbox.
Native editing tests exercise the real core's UTF-16 selection, CJK/RTL
composition callbacks, replacement/cancellation, multiline caret and candidate
geometry, password redaction, readonly rejection and stale-document fences.
Native-window tests add keyboard selection/deletion and copy/cut/paste with
multiline CJK/RTL and emoji grapheme deletion. They save and restore the test
clipboard and keyboard input source.
Keyboard tests issue consecutive Tab/Space/arrow callbacks without waiting for
each acknowledgement, then inspect actual core values. Native-window tests
complete text, checkbox, radio, single-select and decimal-range interactions,
activate a reviewed link with Enter, and type immediately after Shift-Tab
returns to the address field. Command-L focuses the native address editor.

The separate system Zhuyin test sends physical key codes to the unique active
BlueIce test process. This requires Accessibility permission for
`.build/Build/Products/Debug/BrowserUITests-Runner.app` under System Settings →
Privacy & Security → Accessibility, plus an enabled Traditional Zhuyin input
source. It explicitly skips when either prerequisite is missing. Deterministic
`NSTextInputClient` composition callbacks do not establish that an actual OS
input method passed. The scripts do not change macOS privacy permissions.
The backend command uses the signed native service bundle and a Cargo runner
that locally signs test executables. Without the binary override, the Rust
tests use isolated copies of the Cargo-built sibling services.

Result bundles are under `.build/`. XCUITest retains a window screenshot as
an attachment; `test.sh` exports the attachments beside the result bundle.
Build products, result bundles, local Xcode state and screenshots are ignored
by Git. See [the service integration validation record](../../development/browser_core/phase-22-browser-shell-accessibility/MACOS_SERVICE_RESULTS.md)
for those measured results, and [the page accessibility validation record](../../development/browser_core/phase-22-browser-shell-accessibility/MACOS_ACCESSIBILITY_RESULTS.md)
for the later native bridge checks. The
[native editing validation record](../../development/browser_core/phase-22-browser-shell-accessibility/MACOS_NATIVE_EDITING_RESULTS.md)
distinguishes passed native checks from the pending physical input-method test.
The [keyboard interaction validation record](../../development/browser_core/phase-22-browser-shell-accessibility/MACOS_KEYBOARD_RESULTS.md)
records the later control defaults and native focus handoff.
The [native form reset record](../../development/browser_core/phase-22-browser-shell-accessibility/MACOS_FORM_RESET_RESULTS.md)
records real-service composition/context invalidation and actual-window
keyboard/pointer reset, external form associations, tab isolation and reload.

Apple documents the native adapters and test entry points in
[NSViewRepresentable](https://developer.apple.com/documentation/swiftui/nsviewrepresentable)
and [XCUIApplication](https://developer.apple.com/documentation/xcuiautomation/xcuiapplication).

## Current scope

The shell supports built-in pages (`about:credits`, `about:settings`), reviewed
HTTP(S) navigation, native
address editing, tab creation/selection/closing, back/forward/reload,
viewport resize and core-owned page text editing. IPC uses the existing version-two
length-prefixed browser envelopes with an 8 MiB limit. Frame files are
mapped read-only within a fresh private runtime directory and validated before
image creation. Newer per-tab generations supersede older frames. Shutdown
first requests normal service exit, then terminates the owned process group if
needed, and removes its private sockets and frames. The runtime root has mode
0700 and short paths for Darwin's socket limit. Child-only `TMPDIR` and
`XDG_RUNTIME_DIR` select it; persistent gatekeeper settings retain their normal
owner-selected location. Missing services and unavailable review fail closed.
An inherited lifetime pipe and the launcher's opt-in `--exit-on-stdin-eof`
also request normal service shutdown if the GUI is killed. An abrupt GUI
exit can leave an empty runtime parent; normal frontend shutdown removes it.

The viewport sends native logical dimensions separately from backing density.
At 100% zoom, a CSS pixel occupies one native point on both standard and Retina
displays; the core rerasterizes glyphs at the target density. View > Page Zoom,
Command-plus/equal, Command-minus and Command-zero control 25–500% per-tab page
zoom. Navigation, reload and history retain the tab's zoom; new tabs start at
100%. The status-bar percentage resets to actual size. Pointer, accessibility,
find and IME geometry use the same CSS viewport. Control-Command-F toggles the
native full-screen window. Pixel edges remain bounded to 4096; larger backing
outputs reduce raster density while retaining logical layout. See
[viewport results](../../development/browser_core/phase-22-browser-shell-accessibility/MACOS_VIEWPORT_RESULTS.md)
for actual-window and fractional-dimension regression evidence. Physical
multi-monitor handoff and older/Intel macOS runtime acceptance remain pending.

BlueIce > Settings (Command-comma) persists appearance, contrast and motion
choices, each with a System option; View > Appearance changes light/dark mode.
System choices observe the current macOS appearance and accessibility display
options. Overrides apply to BlueIce windows, with visible high-contrast borders
and reduced SwiftUI animation transactions. Inline page CSS can respond through
`@media` and `<style media>` preferences and viewport/resolution queries. Changes
preserve the current document and edited text without fetching. Actual backing
scale stays separate from a capped raster density, and core images use sRGB.
See [display preference results](../../development/browser_core/phase-22-browser-shell-accessibility/MACOS_DISPLAY_PREFERENCES_RESULTS.md)
for native/core and actual-window evidence and the supported CSS subset.

The default app owns its launcher; attaching to an existing shared launcher,
assistant result panels and remaining permission surfaces remain separate work. The
[macOS delivery plan](../../development/browser_core/phase-22-browser-shell-accessibility/MACOS_DELIVERY_PLAN.md)
tracks the remaining milestones. The page exposes the core's semantic representation through virtual
NSAccessibility elements: names, roles, hierarchy, values/state and screen
bounds. Labels and text use the existing core name algorithm; this is not a
complete HTML/ARIA implementation. On macOS 26 and newer, headings use the
native heading role; older systems fall back to static text with a heading-level
role description. Link/button activation, checkbox/radio press and supported
text-input focus use the ordinary core click pipeline. Direct AXValue writes
remain unavailable; slider/select changes use ordinary keyboard input.
Protected input values are redacted.

The AppKit viewport implements `NSTextInputClient` over a versioned core editing
state. Text/password inputs and textareas support UTF-16 selection, grapheme
movement/deletion, marked-text update/commit/cancel, replacement ranges,
document-derived caret/selection geometry and clipped control scrolling. The
native Edit → Input Source menu selects the current responder's input context,
including the address field. Copy/cut omit password contents; paste is an
explicit user operation with a bounded payload. Readonly controls permit
selection but reject writes, and disabled controls cannot become editors.
Commands are fenced by frame source, document and focus generations and
serialized through core acknowledgements. The shell retains only pending IME
range metadata while waiting for the core; it has no parallel DOM or layout.

Edit → Undo/Redo, Command-Z/Shift-Command-Z and the native page context menu
replay core-owned text transactions and UTF-16 selections. AppKit's undo manager
is a forwarding proxy with undo registration disabled. Address/find fields keep
their ordinary Cocoa editing. Paste, cut and committed IME sequences are atomic;
continuous typing or same-direction deletion groups within one second until
selection, movement, focus or another command ends the group. Each page retains
at most 128 transactions and 4 MiB of UTF-8 snapshots across its controls, with
older edits evicted and a context-menu notice when the history was limited.
Snapshots use zeroizing storage and are never serialized. Password state exposes
only availability, lengths and selection. Marked composition disables replay;
cancel keeps the existing redo branch. Window transfer retains histories while
fencing old commands; navigation/reload/reset, successful external writes and
removed or no-longer-writable controls invalidate the affected history. New edits
discard the current control's redo branch. See
[Undo/Redo results](../../development/browser_core/phase-22-browser-shell-accessibility/MACOS_UNDO_REDO_RESULTS.md).

The core also owns sequential Tab/Shift-Tab order, positive and negative
tabindex handling, disabled/hidden/inert exclusions, radio groups with form
owners, checkbox Space, radio arrows, single-select arrows/Home/End and range
step arithmetic. Native form pixels come from the shared layout/paint pipeline.
Keyboard activation uses the existing cancellable click-listener boundary;
document replacement and stale context suppress old defaults. This additive
key intent is not yet a complete physical/logical DOM keyboard event stream.
While Tab awaits core acknowledgement, AppKit retains a bounded set of native
events pinned to the current tab/document. It replays them to the acknowledged
page control or native address editor; tab/document changes discard them.
The owned window keeps later keys behind pending events during responder
changes, preserving their order even after the address editor acquires focus.

Native reset buttons restore their core-owned form defaults, including externally
associated controls, while unrelated forms and tabs keep their values. Reset
clears native composition and rejects old input contexts; it does not navigate.
Input-button captions share the paint and accessibility label source. Native
reset uses the existing cancellable click boundary.
The shell also clears the previous native editing error after accepted input
or a focus change, preserving navigation and gatekeeper denial messages.
GET/POST submissions now use core-owned successful controls, submitter overrides,
UTF-8 form encodings, required checks and reviewed redirects. POST reload/history
uses a SwiftUI Resend/Cancel alert with core-bound, single-use confirmations.
The original request body remains private and bounded; cancellation does not
fetch or move the history cursor. See [submission results](../../development/browser_core/phase-22-browser-shell-accessibility/MACOS_FORM_SUBMISSION_RESULTS.md).
Full DOM reset/submit/formdata events and complete HTML validation remain pending.

Page find uses an AppKit search field in the SwiftUI row, available from
Edit > Find or Command-F. Command-G/Shift-Command-G, Return/Shift-Return and the
arrow buttons move between matches; Escape or Close dismisses the row.
Match case is optional. The core searches its painted public text, highlights
matches, scrolls the current result and updates counts after live edits/resize.
Searches remain local to each tab and clear on navigation. Hidden/password and
protected payment inputs are excluded; bounded partial results are labelled.
See [find results](../../development/browser_core/phase-22-browser-shell-accessibility/MACOS_FIND_RESULTS.md).

Copy/Cut/Paste now wait in the input queue for the core's confirmed selection.
Rapid shortcuts retain their order; password Copy/Cut and readonly Cut remain
inert, and pending clipboard operations cannot run after a tab switch.

Right-click or Control-click opens an AppKit menu for the core's current hit
target. Shift-F10 opens it on a focused editor. Links offer Open, Open in New
Tab, Copy Link Address and Download Linked File; editors offer Cut, Copy, Paste and Select All with
password/readonly policy. Page Back, Forward, Reload and Find keep their
ordinary behavior. Opening a menu does not activate a page link/button, and
an existing editor selection survives right-click. Copy Link Address does not
fetch; opening links retains mandatory review, including visible denial for a
new-tab destination. Stale menu commands cannot act after a new frame/tab or
document. See [context menu results](../../development/browser_core/phase-22-browser-shell-accessibility/MACOS_CONTEXT_MENU_RESULTS.md).

Native tab groups use the core's existing shared group identities and membership.
The tab strip and View > Tab Groups offer creation, naming, palette/hex colors,
collapse/expand and ungroup/remove; Command-Option-G opens the native editor.
Tab context menus create a group for that captured tab or move it to an existing
group/No Group. Collapsing hides member buttons while the selected page stays
live. Removing a group leaves all its tabs open, with their histories, edited
values and zoom. Empty groups remain editable. Invalid names/colors and closed
editor targets cannot save. Core replies update the chrome, and group errors
have their own notice without replacing navigation or policy messages.
See [tab group results](../../development/browser_core/phase-22-browser-shell-accessibility/MACOS_TAB_GROUP_RESULTS.md).
Groups currently live in the running core session and belong to one browser
context; restart restoration and drag reordering remain pending.

Native windows share one owned launcher/core/gatekeeper session. Command-N and
File > New Window open a real AppKit window; Window menu entries activate it.
Tab context menus move an existing tab to another window or to a new empty
window. The core keeps the same page, document, history, values, group and zoom;
transfer does not fetch the page again. Each window has an independent logical
viewport and backing density, including its background tabs and history pages.
Closing a window closes only its tabs; closing the last window stops the owned
services. Native editing, find and group commands follow the active window.

An additive `Window` action/`WindowState` registry records canonical ordered
membership. Existing tab summaries/open replies keep their wire shape, and
legacy clients receive registry metadata only after opting into window actions.
New membership is published before policy review, so denied pages remain visible
and closable. Native page commands include their originating window ID; stale
source-window callbacks cannot reach a moved tab. Transfer also invalidates the
native focus generation and ends temporary composition, preserving committed
text and selection. Window IDs are lifecycle checks and confer no permission.
Malformed registry metadata disables management with a Retry notice while
preserving native windows. Find queries and pending resubmission prompts move
with their tab; session restoration and drag reordering remain open.
See [multi-window results](../../development/browser_core/phase-22-browser-shell-accessibility/MACOS_WINDOW_RESULTS.md).

Profiles in the toolbar and native Profiles menu create, rename and remove named
core contexts or open a window in one. Command-N uses the active profile. Windows,
tabs and groups belong to exactly one context; cross-context transfer and group
assignment are rejected. Removing a profile closes only its windows and tabs;
the default context cannot be removed. Same-context transfer retains the same
page and edited state. Every profile uses the same owned core and mandatory
navigation review. Context IDs are ownership/lifetime checks, not permissions.

Profile names and logical UUID keys persist in a bounded preferences catalog;
runtime IDs are recreated on relaunch. Additional saved profiles reopen empty
and create a page only when a window is explicitly opened; the default window
still starts on about:credits. Page/history restoration, storage
partitioning and private browsing remain pending. Current networking has no
cookies, cache or authentication; display/assistant/extension/download settings
are not partitioned by this increment. Invalid saved catalogs are retained while
profile management is disabled. The read-only MCP list_browser_contexts tool
observes the same core context, window and group registry.

File > Downloads (Command-Option-L) and the toolbar open a shared SwiftUI download
panel. It starts the bundled manager only when explicitly opened, a linked file
is downloaded, or about:downloads is visited. The panel displays live byte,
speed, connection, ETA and segment details; Pause, Resume, Cancel and Remove use
the real service. Completed items open with the current macOS file handler or
select their file in Finder. Removing history preserves completed files.
Only checked regular files inside the configured downloads folder can open.

Download Linked File revalidates the hit target in core and keeps the source tab
and clipboard intact. Every download and explicit Resume uses the same owned
Gatekeeper checkpoint as browsing. The default file root is ~/Downloads/BlueIce;
the bounded persistent catalog lives in ~/Library/Application Support/BlueIce/Downloads.
Diagnostic options --downloads-directory and --downloads-data-directory select
explicit roots. Normal shutdown stops downloads before core/review; the inherited
owner pipe checkpoints active transfers after abrupt GUI exit. Relaunch restores
paused history and never automatically resumes. Core about:downloads and native
controls share one manager and catalog. See
[download results](../../development/browser_core/phase-22-browser-shell-accessibility/MACOS_DOWNLOAD_RESULTS.md).
Automatic response/attachment downloads, destination chooser, credential/settings
UI and quarantine integration remain separate delivery work.
The catalog and preferences are currently shared across browser profiles.

File > Print (Command-P) captures the current core document and opens the native
AppKit print panel. Paper size, orientation and scaling reflow the frozen DOM
using print media; the live page, viewport, editor, find state and history stay
unchanged. PDF uses the system Save dialog and the selected native destination.
Print previews recover when a numeric control passes through an unsupported
intermediate value. Replaced/closed/transferred source documents invalidate the
job, and unsupported final settings cancel the operation with a visible error.

Core emits paginated 192 DPI RGBA surfaces, which AppKit places into the PDF or
print job. Text lines and native form controls stay intact at page cuts; this is
not complete CSS paged-media fragmentation. Jobs are limited to 32 pages and
64 million pixels each, at most two frozen documents and a ten-minute lifetime.
PDF pages contain raster images; searchable/vector text, @page rules, custom
margins/background controls and physical-printer acceptance remain open. Printing
adds no MCP filesystem operation or human permission grant. See
[print results](../../development/browser_core/phase-22-browser-shell-accessibility/MACOS_PRINT_RESULTS.md).

Native file buttons open the system Open dialog from local mouse, accessibility
press, Enter or Space actions. Cancel retains the previous selection; form reset
clears it. Selection supports regular files, single/multiple mode and supported
`accept` hints. Core receives names and content, with no selected path in reader
instructions or metadata; pixels and accessibility show only basenames.
Multipart retains raw binary content and the existing reviewed
navigation policy. Limits are 16 files/1 MiB per input and 64 files/4 MiB per
document, with the existing 1 MiB final form body limit. Directory/capture,
label forwarding, drag/drop and full web File API/events remain open. See
[file input results](../../development/browser_core/phase-22-browser-shell-accessibility/MACOS_FILE_INPUT_RESULTS.md).

The Permissions and assistant settings toolbar button opens the native SwiftUI/AppKit panel app
spawned by this browser's launcher. That exact child inherits the existing
private permission pipes; the ordinary browser and operator sockets cannot
carry its decisions. The bundled launcher resolves a fixed nested app, with no
caller-selected frontend path. Unpackaged launchers retain their sibling frontend.
Only this launcher's process group and bundle URL are activated from the toolbar.

The panel displays the installed package name, version, digest, capability,
origin scope and confirmed grant state. Allow/Revoke first shows a confirmation,
then waits for core's reply for that exact package and generation. Cancel does
not grant. A one-time DOM read separately reviews the live tab/document URL and
requires a second Allow one read action; a changed document is rejected and the
bearer stays internal to core and launcher. Closing the panel hides it and cancels
an unfinished decision. Losing the private child stops the broker and disables
browser actions. Permission grants remain process-lifetime.

There is no installed extension by default. For an owner-selected validated
package, launch the main app executable with `--extension-manifest /absolute/path/to/extension.json`.
This startup option goes through the existing launcher/core package validation;
neither a page nor browser/MCP messages can install a package or replace the path.
The included extension host executes the existing package implementation.
The native Assistant Settings tab inspects the same launcher's settings in
force and pending AI proposal. It edits backend, resource limits, loopback
provider/base/model and Candle file/context fields, with native file choosers.
Review changes shows complete before/after values; Confirm and apply is a
separate action. Proposed settings use the same two-step approval, bound to the
displayed proposal ID and digest. Deny leaves current settings unchanged.
Refresh, switching tabs or closing cancels unfinished confirmation. Controls
and bidirectional formatting in agent-influenced values appear as explicit
Unicode escapes; model output is not executed or interpreted as consent.

The missing settings file defaults to Off. Confirmed changes are validated and
atomically persisted by the launcher at
`~/Library/Application Support/BlueIce/assistant-settings.json`; startup
`--assistant-settings /absolute/path/to/settings.json` selects a different file.
The main app's owner-only `--control-socket /absolute/path/to/operator.sock`
option exposes the existing operator protocol at that address for proposals
and inspection; it adds no approval or grant route. Tests use independent
short temporary paths and test-only networking entitlements. Loopback endpoints
remain credential-free HTTP to numeric localhost with an explicit port and
`/v1/`. In-process inference requires a Candle-enabled service build and local
compatible model files; the default build enables the loopback backend.
See [assistant settings results](../../development/browser_core/phase-22-browser-shell-accessibility/MACOS_ASSISTANT_SETTINGS_RESULTS.md).

The native Assistant menu and toolbar open a resizable SwiftUI sidebar.
Summarize and Organize read the core's current visible page text; the result is
selectable plain text, including literal HTML/Markdown characters. Results and
pending tasks follow the core tab when it moves to another window. Navigation,
tab close, stop-waiting and translation changes invalidate obsolete results.
Each task checks the tab, frame source, document generation, request ID and kind.
Document-bound tasks do not publish text into the legacy shared assistant page.
Failures appear in the sidebar and preserve mandatory navigation denials.

Translation Apply/Off chooses the language for future navigations across all
windows and profiles. Only a confirmed core reply saves the language tag in the
native preference domain; startup confirms it before initial navigation.
Show translated page switches an available translation without refetching.
The existing private settings/permissions child remains the place for model
configuration and human permission decisions. The sidebar grants no permission.
UI tests use a deterministic loopback model endpoint with the real bundled
assistant, core and mandatory gatekeeper. Model quality remains outside that
acceptance. Extension installation UI, explicit panel appearance/localization
integration and general site/OS permission prompts remain delivery work.
See [assistant page results](../../development/browser_core/phase-22-browser-shell-accessibility/MACOS_ASSISTANT_PAGE_RESULTS.md).

Frame refresh temporarily suspends semantic actions until the representation
matches the current tab, source, generation and URL. Native element identities
survive a refresh within one document; reload, navigation and tab changes
invalidate old elements. The accessibility bridge does not expose writable
AXValue, accessibility text ranges, live-region announcements or rotor search.
Native editing geometry is currently available through `NSTextInputClient`,
independently of those AX text APIs. Actual OS IME verification is pending
runner Accessibility permission on the recorded host. JavaScript
keyboard/beforeinput/input/composition event dispatch, complete
bidirectional shaping and caret blink remain open, as do full form event/validity behavior,
select popup/typeahead/multiple-selection interaction, complete toolbar Tab
traversal, image/media context actions, page-text selection/copy, drag/drop,
full file API/events, automatic attachment downloads, remaining trusted panels and localization. Automated checks cover the recorded features; an
interactive VoiceOver session remains unvalidated.
