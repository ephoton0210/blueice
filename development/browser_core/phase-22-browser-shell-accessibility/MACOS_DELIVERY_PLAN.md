# macOS browser delivery milestones

The macOS browser remains an active delivery target. This plan translates the
Phase 22 native-shell requirements into reviewable commits; an initial window,
a passing smoke test, or one bridge does not complete the browser design.
Every implementation milestone includes native UI tests, focused core/protocol
regressions, a dated result record and a scoped commit. Validated commits are
pushed to the current tracked branch as authorized by the owner.

| Milestone | Delivery and required evidence | State |
| --- | --- | --- |
| Native window and chrome | SwiftUI/AppKit address/tabs/history/settings, visible real-core pixels, startup/close/resize XCUITest | Committed `c762b53a9` |
| Owned service stack | Bundled launcher/core/gatekeeper, reviewed external navigation, fail-closed review, normal/forced cleanup | Committed `32fa07f6b` |
| Page accessibility | Core semantic tree mapped into NSAccessibility, native actions, privacy, stale-element and tab isolation tests | Committed and pushed `41952a803` |
| Native text editing | Core-owned UTF-16 selection, grapheme movement/deletion, IME composition/update/commit/cancel, caret/candidate geometry, text/password/textarea editing, clipboard policy and native UI tests | Foundation committed and pushed `8b80c3c53`; physical OS IME and remaining editing behavior pending, see [results](MACOS_NATIVE_EDITING_RESULTS.md) |
| Keyboard and page interaction | Keyboard-only form completion, checkbox/radio/select/range controls, find-in-page, native context menus, drag/drop and file-selection policy tests | Keyboard increment committed and pushed `97794bb57`; native form reset committed and pushed `4c1c6e922`; GET/POST submission committed and pushed `1bf78a3d9`; find/ordered clipboard committed and pushed `614489c59`; native context menus committed and pushed `531b6b2df`; native file-input panel/content submission implemented and accepted, see [file-input results](MACOS_FILE_INPUT_RESULTS.md); remaining interactions pending |
| Windows and tab organization | Multiple native windows, tab groups, profile/context lifecycle, retained history and state handoff using core tab identities | Native groups committed and pushed `6e141967f`; shared-core windows/tab transfer committed and pushed `b318e0de8`; context lifecycle and persistent profile identities committed and pushed `ee450a3df`, see [context results](MACOS_CONTEXT_RESULTS.md); durable session restoration and full storage partitioning pending |
| Downloads and printing | Actual download manager/shelf, progress/cancel/open/reveal, print/PDF media output and native panel tests | Native manager panel and linked-file downloads committed and pushed `b62b38aa7`, see [download results](MACOS_DOWNLOAD_RESULTS.md); core print-media pagination and native print/PDF committed and pushed `0d2e3ea58`, acceptance recorded in [print results](MACOS_PRINT_RESULTS.md); automatic response downloads, destination/credential UI, quarantine, vector PDF and physical-printer acceptance pending |
| Trusted browser panels | Assistant results and human permission decisions using the private owner boundary, policy-denial and no-AI-grant tests | Pending |
| macOS display and system integration | DPI/multi-monitor, zoom, theme/high contrast/reduced motion, fullscreen, localization and native menu/shortcut tests | Retina/CSS viewport, per-tab zoom and native fullscreen committed and pushed `91f2dd78c`, see [viewport results](MACOS_VIEWPORT_RESULTS.md); persistent appearance/contrast/motion and CSS media committed and pushed `0609c8a81`, recorded in [display results](MACOS_DISPLAY_PREFERENCES_RESULTS.md); physical system/monitor transitions and localization pending |
| Full accessibility and final audit | Text ranges/live regions and supported rotor operations; actual screen-reader action; final integration, design/UI acceptance and documented remaining core limitations | Pending |

## Native editing contract

The core owns focus, document identity, committed text, selection and temporary
IME composition. Platform ranges use UTF-16 offsets, while core mutations check
Unicode boundaries and delete/move by grapheme clusters. The existing append-only
compatibility messages remain distinct from the versioned native editing path.
Native commands carry the live frame-directory source and document generation;
a late command from a replaced page must not edit a new document.

AppKit implements `NSTextInputClient` using core-originated state and geometry.
The frontend does not create a DOM or relayout the page. Candidate positioning,
selection and marked ranges must follow the actual focused control and viewport.
Protected text is editable through ordinary input but remains redacted from
representation, input-state inspection and accessibility text values. Extension
DOM-write permissions remain independently constrained.

Clipboard and file-selection boundaries are explicit user operations. Page/AI
requests do not silently inspect OS content or authorize a native picker.
The eventual native editing milestone must include CJK and RTL input, replacement
and cancellation, focus/navigation/tab invalidation, selection/copy/cut/paste
policy, protected/disabled/readonly controls, visible caret/selection pixels,
and actual native-window automation. Deterministic composition-sequence tests
and OS input-method tests are reported separately.

The 2026-10-02 foundation includes those deterministic callbacks, range/geometry
boundaries and actual-window selection/clipboard tests. Its system Zhuyin test
is implemented but skipped on this host because `AXIsProcessTrusted()` is false
for the UI runner. Remaining editing work includes physical IME validation,
JavaScript keyboard/beforeinput/input/composition event dispatch, undo/redo,
complete bidirectional shaping, caret blink, preferred vertical caret position
and general keyboard input during asynchronous focus changes. The later
keyboard milestone buffers native events specifically across Tab handoffs;
it does not complete native editing acceptance.

## Keyboard control increment

The core derives Tab order and default actions from the current document.
Checkbox/radio state, single-select choices and stepped range values feed the
same semantic snapshot and shared layout/paint output. Native AppKit Tab
transitions retain bounded events until the core confirms the new focus, then
deliver them to the page or the SwiftUI address editor. Command-L uses native
address focus. The owned window also holds later keys until pending events have
replayed, preserving event order across responder changes. The keyboard activation boundary honors existing click
cancellation and document replacement before applying defaults.

This increment covers keyboard control completion and reviewed link activation.
The later submission increment covers GET/POST and required-field checks.
Complete validation, popup/typeahead and
multiple-select interaction, DOM key/input/focus/composition events, full chrome
Tab traversal, context menus, drag/drop and file-selection remain required.
The later find increment delivers page search.
Multiple-select direction keys deliberately preserve existing selections until
their own interaction model is implemented. Physical OS IME and screen-reader
acceptance remain separate gates.

## Native form reset increment

The core retains original control defaults separately from live native values.
Reset buttons restore their current form owner's text/password/textarea,
checkbox/radio, select and range state, including external associations and
disabled/readonly fields. Unrelated forms and tabs retain their values. A reset
retains control focus, invalidates old native input contexts, ends composition
and republishes shared pixels and semantic values without fetching.
Input reset/submit/button captions use the same core label for paint and
semantics. Existing pre-default click cancellation and document replacement
remain effective. Explicit page-script textarea textContent/appendChild changes
update the retained default; extension/native live-value edits do not.
The native shell clears a correlated editing error after a successful edit or
focus change, while preserving navigation and mandatory policy-denial notices.

This increment covers the native reset default. Full cancelable DOM reset-event
dispatch, complete constraint validation, dirty value/default
DOM property semantics remain pending. The later file-input increment adds
retained file selection and reset. The later submission increment delivers
GET/POST methods and encodings. The separate result record identifies exactly
which real-service and actual-window cases passed.

## Native form submission increment

Current-tab GET/POST submissions are constructed by the core from successful
controls in document order, including external form owners and the activated
submitter's overrides. URL-encoded, text/plain and multipart UTF-8 encodings,
CRLF normalization, required-field checks and novalidate, the document base URL,
implicit Enter and disabled/readonly/select behavior have public boundary tests.
An empty file control submits no file bytes; an HTML value never grants access
to an OS path. Native loading status and duplicate activation prevention apply
while a form navigation is pending. Default and clamped range values are shared
by paint, semantics and submission, with valid original numeric spellings
preserved to avoid losing exact large integers.

Every HTTP hop retains the mandatory URL review; forms add a metadata review
before the connection. Protected fields are excluded from GET and require
same-origin HTTPS POST under the compiled product policy, including 307/308
redirects that retain their body. POST bodies are absent from review/debug,
accessibility, extension network trace and frontend resubmission messages.
Ordinary GET entries remain part of the reviewed URL. The final response,
including an HTTP error page, is content-reviewed before parsing.

POST reload and default URL-history traversal require the native Resend/Cancel
alert. Cancel leaves the page, cursor and request count unchanged; accepting a
single-use, tab/document/navigation-bound confirmation repeats the gated request.
Reload replaces the current entry. Private bodies are held in shared zeroizing
buffers with a one-MiB request and eight-entry/eight-MiB retained-history limit
per tab. Expired entries require a new form submission and never become GET.
The reference frontend reports unsupported confirmation; MCP navigation returns
a notice and does not automatically resend. These ordinary resubmission prompts
are separate from the private human-permission boundary.

The result record covers the bundled services and actual native-window tests.
Full HTML validity (type/pattern/length/numeric constraints), validation bubble
and invalid-field focus, DOM submit/formdata/reset events, dirty/default property
semantics, image submitters/coordinates, dirname, alternate form targets, dialog
forms and real file selection remain required. Multiple-select submission is
covered; native multiple-select interaction remains pending. Physical IME,
VoiceOver and the other browser milestones retain their separate acceptance.

## Native find increment

The core indexes its current layout text runs, including public native values
and button/select captions, and retains grapheme geometry in document space.
Soft wrapping and inline font changes can split one match across rectangles;
block boundaries do not create phrases. Queries are literal, canonically
normalized and whitespace-normalized; default matching uses Unicode simple
case folding with an explicit Match case option. Hidden, non-content,
opacity-zero and protected input subtrees do not enter the index. Results carry
only the user's query, count, position, lifecycle identity and active geometry.

The SwiftUI search row embeds AppKit NSSearchField for reliable native focus,
selection, Return/Shift-Return and Escape. The Edit > Find menu implements
Command-F, Command-G and Shift-Command-G; arrow buttons and a close button have
native accessibility labels. Core paint marks all matches and emphasizes the
current one, then scrolls it into the viewport. Search state is per tab, rebases
on layout/live edits, and clears on navigation, including retained history
snapshots. Find commands check the tab, frame directory source and document
identity. The query/index/geometry/match bounds disclose partial results rather
than silently presenting a complete search.

A complete native regression exposed rapid Select All followed by Copy being
dropped while selection acknowledgement was pending. Copy/Cut/Paste now share
the bounded, document/focus-fenced input queue. Paste reads at its turn, Copy/Cut
wait for the confirmed selection, password text stays excluded, readonly Cut is
inert, and queued clipboard commands are discarded after switching tabs.
Real-service regression tests and the existing clipboard XCUITest retain the
original content and privacy assertions.

This completes the current layout's page-find UI increment. It does not add
regex search, locale-tailored/full multi-character case folding, accent-insensitive
matching, complete text shaping/bidi, overflow scrolling, iframe content, or
searching text the current core does not paint. The following increment delivers
link/editor/page context menus; drag/drop, file selection and the other browser
milestones remain required.

## Native context menu increment

AppKit owns the native popup and keyboard/menu navigation. Right-click,
Control-click and Shift-F10 on a focused editor request a core hit test using
the live tab/frame-directory/frame identity and viewport coordinates. A text
editor gains native focus without executing a click default or changing its
existing selection. Links, buttons and blank areas do not activate while a
menu is opened. The reply includes the resolved public link and only the hit
editor's validated native state, including redacted protected controls.

Link Open and Open in New Tab revalidate the document/frame and rederive the
target in core before ordinary URL/content review. Copy Link Address replies
without fetching. Only the native user's correlated Copy action writes to the
OS clipboard; the protocol does not access it. An already-loaded new tab retains
its committed URL instead of navigating again to credits. A denied new-tab
destination is reported on the source page, and its unpublished empty tab is
closed. Independent pending new-tab replies survive opening another menu.

Cut/Copy/Paste and Select All use the existing ordered input queue. Password
Copy/Cut and readonly Cut/Paste are disabled; readonly selection and Copy work.
The menu never inspects the clipboard just to decide whether Paste is enabled.
Page Back/Forward/Reload and Find retain their existing gated and resubmission
behavior. The native menu and its callbacks expire after frame/document/tab
replacement, resize or scroll. The exact supported window operations and
core/protocol/MCP evidence are recorded separately.

Image/media actions, general page-text selection/copy, contextmenu DOM event
dispatch, drag/drop, file selection, physical IME and the other full-browser
milestones remain required; this increment does not complete those gates.

## Native display and zoom increment

Native logical window dimensions, raster backing density and per-tab page zoom
are distinct core inputs. CSS viewport dimensions are logical size divided by
zoom; physical bitmap edges are rounded from logical size times backing density.
The rasterizer transforms document geometry and rerasterizes fonts directly into
that bounded viewport, rather than allocating a tall full-page bitmap or scaling
an existing low-density bitmap. Physical dimensions are supplied explicitly to
the rasterizer to avoid a zoom-dependent floating-point rounding discrepancy.
The native shell adapts raster density when a backing edge would exceed 4096.

Frame-correlated viewport metadata supplies the CSS geometry used for pointer
input, wheel deltas, accessibility clipping/actions, find and IME candidate
positioning. The core owns zoom per tab, including navigation/reload and retained
history; a new tab starts at 100%. Native View-menu presets, zoom shortcuts and a
reset percentage control operate from 25% to 500%. Fullscreen uses NSWindow and
updates the same viewport contract. Existing version-two clients retain their
original frame stream until they opt into native display/zoom commands; MCP
ignores display broadcasts while awaiting ordinary navigation completion.

Real-core tests exercise synthetic backing-density changes, fractional edges,
pixel caps, stale/closed-tab rejection and zoomed editing/find/menu geometry.
Actual-window tests verify Retina CSS width, zoom shortcuts and menu presets,
中文 clipboard editing, tab/reload retention and fullscreen entry/exit. These
tests do not establish physical monitor handoff, older/Intel macOS runtime,
physical IME or VoiceOver acceptance. The following increment supplies native
appearance, contrast and motion preferences. Localization and the remaining
browser milestones are still required.

## Native display preference increment

The native Settings scene and View > Appearance menu persist application-owned
appearance, contrast and motion choices. System choices observe AppKit
effective appearance and accessibility display option changes. Window overrides,
explicit high-contrast chrome borders and reduced SwiftUI animation transactions
consume the same resolved preferences supplied to the core.

Core inline styles evaluate nested screen media conditions and style-element
media attributes against preferences, CSS dimensions and actual screen density
times page zoom. The optional backing density remains separate from a raster
density reduced by the physical pixel cap. Core pixels are explicitly sRGB.
Preference changes repaint existing documents and retained tab/history state
without fetching or replacing edited text. Legacy protocol-two clients retain
their existing stream until preference opt-in; new metadata preserves ordinary
MCP completion barriers.

Native search synchronization preserves AppKit marked text and suppresses
programmatic delegate feedback. Rapid zoom writes are ordered and coalesced;
inbound frame and display metadata are delivered in reader order. Regression
evidence includes repeated real-core zoom/tab and preference cases, actual
light/dark pixels, visible contrast, native menu actions and preference persistence
through normal app termination/relaunch. The result record distinguishes these
checks from physical system-option/monitor changes and OS IME/screen-reader
acceptance. General CSS media support, external stylesheets, matchMedia,
automatic UA color-scheme recoloring and the full animation pipeline remain open.

## Native tab group increment

The SwiftUI tab strip renders Phase 16 core groups and original tab IDs. Its
native sheet validates names and hex colors before create/rename/recolor;
core replies supply the trimmed name, normalized color, membership and collapsed
state. Toolbar, tab/group context menus, View > Tab Groups and Command-Option-G
share that state. Collapse hides member buttons while the selected page continues
to render and edit. Ungroup/remove dissolves metadata without closing tabs.
Empty groups remain editable, and a closed tab/group disables an open editor.

Group mutations wait for the exact request and global/tab-bound reply; ownership
is registered before writing so immediate errors cannot overwrite navigation or
policy-denial notices. Inbound state remains in reader order, and mutations do
not fetch or replace documents. The result record covers retained text, history,
zoom, identity, native menus, validation/cancel and empty/stale group UI.

## Native shared-core window increment

Command-N and Window menu activation manage actual AppKit windows over one
owned launcher/core/gatekeeper session. The core registry supplies monotonic
window IDs, canonical ordered tab membership and independent viewport routing.
Tab context menus transfer the same page to an existing or new window, retaining
document identity, committed text, selection, history, zoom and group. Find
queries and pending resubmission prompts follow the tab. Transfer ends temporary
composition and rejects queued page commands from the source window.

Closing one window closes only its members; closing the last window exits the
app and tears down its owned services. Failed navigation retains closable
membership, and malformed registry metadata offers Retry without destroying
the windows. Recovery refreshes history, pixels, semantics and editing state.
Real-service and actual-window evidence is recorded in
[window results](MACOS_WINDOW_RESULTS.md).

The next increment supplies context lifecycle and persistent profile identities.
Restart restoration of tabs, drag reorder and physical IME/screen-reader
acceptance remain separate delivery requirements.

## Native context and profile identity increment

The core owns named contexts and their windows, tabs and groups. Native Profiles
menus create, rename, open and remove profiles; new windows use the active
profile. Removing a nondefault profile closes only its members. Same-context tab
transfer retains the existing page; cross-context transfer/group assignment is
rejected before mutation. Context and window IDs fence stale native callbacks
but confer no controller lease or human permission. All contexts share one
owned core and the mandatory navigation/content review.

An additive BrowserContext registry precedes window snapshots and carries
canonical group ownership. Context-scoped Lists cannot erase another window's
tab values: only canonical window membership removes them. Malformed metadata
preserves native windows and provides Retry. MCP list_browser_contexts reads the
same registry and waits for its exact request; unsolicited snapshots do not
complete it. Legacy tab/open wire shapes remain unchanged.

Bounded preferences persist names and logical UUID keys independently of runtime
IDs. Relaunch recreates empty named contexts with fresh runtime IDs and preserves
their UUIDs; it does not restore pages or fetch remembered URLs. Invalid saved
catalogs are retained and profile management is disabled. Full session/tab
restoration remains Phase 16 work. Cookies, cache and authentication are not yet
implemented in the network layer; this increment does not add private browsing
or per-profile storage, assistant, extension, download or display preferences.
Final acceptance is recorded in [context results](MACOS_CONTEXT_RESULTS.md).

## Native print and PDF increment

File > Print and Command-P capture a cloned core DOM under the current tab,
window, frame-directory source and document generation. The same CSS/layout/
paint/raster pipeline uses print media and the native printable paper area.
Preview paper/orientation/scaling changes reflow that frozen document without
fetching, running scripts or mutating the live page. Protected native text stays
masked, and focus/caret/find overlays are excluded. Page cuts avoid text-line
and native-control interiors; output limits fail visibly rather than truncate.

A dedicated broker connection lets synchronous AppKit preview callbacks read
core pages without blocking on the MainActor browser reader. The broker retains
its ordinary broadcast behavior: a random high request namespace and exact
request/tab/ticket/revision checks distinguish the job's replies. Tickets name
read-only jobs and confer no private owner or human authority. Job pixels use a
private subdirectory, separate from screen generations and frame retention.
Release, invalidation, timeout expiry and core teardown remove the print files.

The AppKit panel supplies paper size, orientation, scale and page selection.
Its PDF destination uses the actual system Save dialog. Numeric controls can
publish intermediate values (8 while entering 80); a valid later profile clears
preview failure, while an invalid source or final profile cancels delivery.
Printable default names remove path separators/control characters. Actual UI
checks save and inspect a three-page A4 landscape PDF at 80% scaling, confirm
print-media ink, cancel/reopen the panel, and retain the edited browser document.

The bounded output currently consists of 192 DPI core raster images in native
PDF pages. Full @page/fragmentation, vector/searchable PDF, custom margin and
background UI, and physical printer jobs remain separate. The following
file-input increment delivers regular-file selection; private owner permission
panels and the other delivery gates stay open.

## Native file-input increment

Core owns selected file contents separately from markup and publishes only
basenames in shared pixels and semantics. A local native pointer/press/Enter/
Space action requests validated core hints, then opens the actual AppKit Open
panel. Ordinary browser/AI replies never open a picker. Native reads are bounded
regular-file reads of the panel's returned URLs; no selected path is sent to
core as reader instructions or metadata. HTML value attributes do not select files.

Selection replies retain tab/window/source/document/node/revision fences.
Cancel preserves selected files; reset, explicit empty value assignment and
navigation invalidate stale selections. File count/content budgets are atomic.
Multipart keeps binary bytes and ordered entries; other encodings use names,
and mandatory form/URL review and body limits still apply. Public content IPC
does not attest consent or confer private owner/permission authority.

Actual UI tests choose multiple files through the system folder/path panel,
submit and inspect binary HTTP bodies, cancel/reopen with Space, and reset while
retaining ordinary editor content. The acceptance record is
[file-input results](MACOS_FILE_INPUT_RESULTS.md). Directory/capture, label click
forwarding, drag/drop, full File/Blob/FileList and input/change/cancel events,
private owner panels and the other delivery gates remain open.

## Completion audit

Before final completion, check every milestone against its code, native app,
result bundle, result record and commit/push state. Verify the same core owns
all inspected/rendered/edited pages. Scope native UI assertions to the exact
feature they exercise; do not use a broad green test count as evidence for
untested IME, screen-reader, permission, print, profile or window behavior.
The Phase 22 cross-platform work remains separate from this macOS delivery;
its unfinished items must not be reported as achieved by the macOS frontend.
