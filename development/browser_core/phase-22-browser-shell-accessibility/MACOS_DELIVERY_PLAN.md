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
| Browser chrome keyboard focus | Enabled rendered control loop, native field/button/menu activation, page boundary handoff, scrolling and window/modal/service lifecycle fences | Implemented and accepted, see [focus results](MACOS_CHROME_FOCUS_RESULTS.md) |
| Address-bar search | Human Return/Go, exact Unicode queries, persistent shared provider Settings, URL/history preservation, native localization/keyboard and Gatekeeper refusal | Implemented and accepted; complete native suite passed 230 methods with 1 existing physical Zhuyin skip, see [search contract](MACOS_ADDRESS_SEARCH_CONTRACT.md) |
| Browser service recovery | Visible owned restart, volatile confirmed session recovery, fresh workspace/window ownership, POST/privacy and stale-input refusal | Implemented and accepted, see [recovery contract](MACOS_RECOVERY_CONTRACT.md) and [recovery results](MACOS_RECOVERY_RESULTS.md); complete native suite passed 217 methods with one existing physical Zhuyin skip |
| Owned service stack | Bundled launcher/core/gatekeeper, reviewed external navigation, fail-closed review, normal/forced cleanup | Committed `32fa07f6b` |
| Page accessibility | Core semantic tree mapped into NSAccessibility, native actions, privacy, stale-element and tab isolation tests | Committed and pushed `41952a803` |
| Native text editing | Core-owned UTF-16 selection, grapheme movement/deletion, IME composition/update/commit/cancel, caret/candidate geometry, text/password/textarea editing, clipboard policy and native UI tests | Foundation committed and pushed `8b80c3c53`; bounded native Undo/Redo implemented and accepted, see [Undo/Redo results](MACOS_UNDO_REDO_RESULTS.md); physical OS IME and remaining editing behavior pending, see [foundation results](MACOS_NATIVE_EDITING_RESULTS.md) |
| Keyboard and page interaction | Keyboard-only form completion, checkbox/radio/select/range controls, find-in-page, native context menus, drag/drop and file-selection policy tests | Keyboard increment committed and pushed `97794bb57`; native form reset committed and pushed `4c1c6e922`; GET/POST submission committed and pushed `1bf78a3d9`; find/ordered clipboard committed and pushed `614489c59`; native context menus committed and pushed `531b6b2df`; native file-input panel/content submission committed and pushed `222d7f78e`, see [file-input results](MACOS_FILE_INPUT_RESULTS.md); native select popup/typeahead/multiple selection accepted, see [select results](MACOS_SELECT_CONTROLS_RESULTS.md); remaining interactions pending |
| Windows and tab organization | Multiple native windows, tab groups, profile/context lifecycle, retained history and state handoff using core tab identities | Native groups committed and pushed `6e141967f`; shared-core windows/tab transfer committed and pushed `b318e0de8`; context lifecycle and persistent profile identities committed and pushed `ee450a3df`, see [context results](MACOS_CONTEXT_RESULTS.md); durable native session restoration implemented and accepted, see [session results](MACOS_SESSION_RESTORE_RESULTS.md); native dragging and atomic placement implemented and accepted, see [placement results](MACOS_TAB_PLACEMENT_RESULTS.md); full storage partitioning pending |
| macOS download quarantine | System-generated quarantine and sanitized Finder source metadata before atomic publication; current native status, unsupported-metadata refusal, real download/relaunch/Open/Finder UI tests | Implemented and accepted; complete native suite passed 219 methods with one existing physical Zhuyin skip and complete Rust workspace passed 7,311 cases, see [quarantine contract](MACOS_DOWNLOAD_QUARANTINE_CONTRACT.md) and [validation record](MACOS_DOWNLOAD_QUARANTINE_RESULTS.md) |
| Downloads and printing | Actual download manager/shelf, progress/cancel/open/reveal, print/PDF media output and native panel tests | Native manager panel and linked-file downloads committed and pushed `b62b38aa7`, see [download results](MACOS_DOWNLOAD_RESULTS.md); core print-media pagination and native print/PDF committed and pushed `0d2e3ea58`, acceptance recorded in [print results](MACOS_PRINT_RESULTS.md); native credential-store UI implemented and accepted, see [credential contract](MACOS_DOWNLOAD_CREDENTIALS_CONTRACT.md) and [credential results](MACOS_DOWNLOAD_CREDENTIALS_RESULTS.md); native SFTP private-key/known-hosts configuration and owned loopback encrypted-key authentication implemented and accepted, see [SFTP results](MACOS_SFTP_FILES_RESULTS.md); native destination selection with preserved history implemented and accepted, see [folder results](MACOS_DOWNLOAD_FOLDER_RESULTS.md); automatic original-response downloads implemented and accepted, see [response results](MACOS_RESPONSE_DOWNLOADS_RESULTS.md); remote SFTP/FTPS interoperability, vector PDF and physical-printer acceptance pending |
| Trusted browser panels | Assistant results and human permission decisions using the private owner boundary, policy-denial and no-AI-grant tests | Native installed-extension permission child and two-step one-shot confirmation committed and pushed `f83c94152`, see [permission results](MACOS_PERMISSION_RESULTS.md). Native assistant settings and proposal decisions committed and pushed `b53a91111`, see [assistant settings results](MACOS_ASSISTANT_SETTINGS_RESULTS.md). Native assistant result/sidebar and translation surfaces implemented and accepted, see [assistant page results](MACOS_ASSISTANT_PAGE_RESULTS.md). Remaining permission UI pending |
| macOS display and system integration | DPI/multi-monitor, zoom, theme/high contrast/reduced motion, fullscreen, localization and native menu/shortcut tests | Retina/CSS viewport, per-tab zoom and native fullscreen committed and pushed `91f2dd78c`, see [viewport results](MACOS_VIEWPORT_RESULTS.md); persistent appearance/contrast/motion and CSS media committed and pushed `0609c8a81`, recorded in [display results](MACOS_DISPLAY_PREFERENCES_RESULTS.md); English/Traditional Chinese native interface localization implemented and accepted, see [localization results](MACOS_LOCALIZATION_RESULTS.md); physical system/monitor transitions pending |
| Full accessibility and final audit | Text ranges/live regions and supported rotor operations; actual screen-reader action; final integration, design/UI acceptance and documented remaining core limitations | Native text-control ranges, geometry, selection and editing implemented and accepted, see [accessibility text results](MACOS_ACCESSIBILITY_TEXT_RESULTS.md). Bounded live-region announcements and supported native rotors implemented and accepted, see [live-region/rotor results](MACOS_LIVE_REGION_ROTOR_RESULTS.md); that earlier Rust acceptance used the recorded post-Cargo host readiness condition. Retained announcement delivery/consumption ACK is now implemented and accepted, see [delivery results](MACOS_ANNOUNCEMENT_DELIVERY_RESULTS.md). Descendant atomic/relevant settings and public author labels are implemented and accepted, see [descendant results](MACOS_DESCENDANT_LIVE_RESULTS.md). Document text selection and read-only AX text access implemented and accepted, see [document-selection results](MACOS_DOCUMENT_SELECTION_RESULTS.md). Additional rotors and physical VoiceOver remain pending |

## Browser chrome keyboard focus increment

Tab/Shift-Tab follow enabled rendered controls and wrap in both directions.
Actual view lifecycle excludes disabled/collapsed members and recovers at a
surviving neighbor when a pane or tab disappears. Focused tabs scroll into view;
buttons and toggles share their normal actions. Native menus, fields, sheets and
the private permission child retain native behavior. Core still owns DOM order.

Queued keys retain model/tab/document/readiness/window ownership. Service
readiness changes discard pending input, while enabled notices remain operable
with the core unavailable. The [focus contract](MACOS_CHROME_FOCUS_CONTRACT.md)
and [dated results](MACOS_CHROME_FOCUS_RESULTS.md) record the increment. Complete
native acceptance passed 210 methods with one existing physical Zhuyin skip;
fresh gates passed 1,004 engine/IPC cases, formatting, strict Clippy and the
all-targets build. All gates share 1,804 unchanged inputs; eight signatures,
universal app architectures, product consistency and owned-process cleanup
passed. The unchanged Rust backend retains the accepted workspace baseline
from `c4070298c`; no new full workspace execution is claimed.
Physical IME/VoiceOver and the remaining browser requirements stay open.

## Native tab placement increment

Tab titles support actual native dragging before/after a destination member,
into expanded or collapsed groups, back to the ungrouped end and between live
windows in the same context. Different profiles refuse the transfer. View and
tab context menus plus Command-Control-Left/Right offer the same atomic core
placement. The existing opt-in session archive retains canonical order and
selection across normal quit/relaunch.

Source membership, insertion anchor and destination/context/group ownership are
validated before mutation and again after provider completion. The native drag
contains only a short-lived own-process token. Same-window organization retains
the selected page, composition/focus/frame identity, committed editor text,
unsent address draft, find state and zoom. Cross-window handoff retains the live
page and rejects stale source callbacks. Insertion indicators clear when the
owned token is consumed or cancelled.

All nine new native methods and 1,004 focused engine/IPC cases pass, together
with formatting, strict Clippy and all-targets build. Complete native acceptance
passed 199 methods with one existing physical Zhuyin skip; complete Rust workspace
acceptance passed 7,307 cases with 69 ignored. Eight strict signatures, universal
app architectures, source/product consistency and owned-process cleanup passed.
All final gates share the same unchanged 1,803 inputs. Earlier unsuccessful and
superseded runs remain in [the dated results](MACOS_TAB_PLACEMENT_RESULTS.md).
General page/OS drag-and-drop, storage partitioning, physical IME/VoiceOver and
the remaining browser requirements stay open.

## Native interface localization increment

Settings offers Follow macOS, English and Traditional Chinese. Bundled string
tables cover browser controls, AppKit actions and private permission/assistant
panels. Existing windows and the owner-scoped private child observe the persisted
language without replacing the core page or confirming any decision. URLs,
editor/page text, authored names, protocol tokens and reviewed values retain
their original data. Native browser controls update immediately; standard macOS
menus/dialogs use the startup language and update after restarting BlueIce.
The boundary and resource/preference behavior are recorded in
[the localization contract](MACOS_LOCALIZATION_CONTRACT.md).

Complete native acceptance passed 190 methods with one existing physical Zhuyin
skip; complete Rust workspace acceptance passed 7302 cases with 69 ignored.
Formatting, strict Clippy, all-targets build, eight strict signatures and the
source/product/process audit passed. The final source aggregate is identical
across all gates. Earlier native failures, the corrected classification of a
pre-existing unrelated test process and internal QoS warnings remain in
[the dated results](MACOS_LOCALIZATION_RESULTS.md). Physical system/monitor
transitions, VoiceOver/IME and the remaining browser requirements stay open.

## Native document text selection increment

Core indexes ordinary public rendered text in document order and owns UTF-16
selection, grapheme/visual-line queries, geometry and selection paint. AppKit
maps native pointer/keyboard selection, Select All, Copy, context-menu commands
and read-only AX text callbacks to that state. Controls, protected/private text
and hidden/inert/AX-hidden content stay outside document Copy. Link drag selects
text; a completed click follows the existing reviewed activation path.

The bounded input queue retains mouse events and following page-input keys
through temporary frame/geometry and pointer-focus acknowledgement gaps. Source,
document, tab/window, size and responder changes discard pending input. Document
selection never permits editing an unrelated control. Collection budgets and
ownership requirements are recorded in the
[document-selection contract](MACOS_DOCUMENT_SELECTION_CONTRACT.md).

Complete native acceptance passed 184 methods with one existing physical Zhuyin
skip; complete Rust workspace acceptance passed 7302 cases with 69 ignored.
Formatting, strict Clippy, the all-targets build, eight strict signatures and
source/product/process cleanup audits passed. Earlier unsuccessful runs and the
unchanged-input acceptance evidence remain in the
[dated results](MACOS_DOCUMENT_SELECTION_RESULTS.md). Physical VoiceOver/IME,
complete bidirectional shaping and the remaining milestones stay open.

## Native accessibility text-control contract

AppKit parameterized text callbacks use a private synchronous broker connection
with a bounded whole-exchange deadline. Context/window/tab membership and the
exact frame source, document generation, frame generation and node fence every
request. Core owns text, UTF-16 scalar-boundary validation, extended grapheme
ranges, visual lines, geometry, native focus, scroll offsets and transactions.
The frontend only maps CSS document coordinates to/from its native screen view.
Read queries preserve focus, selection and pixels. Frame/viewport notifications
precede a mutation's AX reply and remain owned by the normal browser reader.
Timed-out writes are discarded without retry.

Editable text inputs and textarea support AXValue and selected-text writes;
read-only fields support selection and reject edits before focus changes.
Disabled, stale or unsupported controls cannot edit through this boundary.
Password plaintext is absent from replies, values, substring/attributed/RTF
queries and selected text. Visible-range writes scroll inside the control
without moving focus or selection. Selection/editing shares native Undo/Redo
and ends existing marked composition before recording an atomic edit.
The document-selection increment above delivers paragraph text selection;
rich font attributes and physical VoiceOver remain separate delivery work.
The subsequent live-region/rotor increment below records
the bounded support and remaining announcement delivery limitations.

The earlier text-control acceptance passed: 166 native tests with one physical Zhuyin skip and
7,233 Rust workspace tests with 69 ignored. Print/AX exchanges no longer keep
idle broker readers while AppKit panels wait. The dated result and distinct
frozen input scopes are recorded in
[accessibility text results](MACOS_ACCESSIBILITY_TEXT_RESULTS.md).

## Native live-region and rotor increment

Core publishes an optional document/revision-fenced native overlay, with live
region announcements, hidden node IDs and privacy-safe name corrections. Polite,
assertive and off behavior, status/alert/log defaults, atomic/relevant inheritance,
busy coalescing, nested ownership and privacy transitions have regression tests.
Limits and the latest-layout-batch delivery condition are explicit in
[MACOS_LIVE_REGION_ROTOR_CONTRACT.md](MACOS_LIVE_REGION_ROTOR_CONTRACT.md).

AppKit advertises heading/level, link, image, list and button rotors with document
order, directional filtered search and offscreen targets. Searching does not
mutate the page. Reading focus invokes the source/document/frame/node-fenced
core reveal operation, preserving DOM focus and editor selection. Stale/foreign
items fail closed; same-document frame gaps preserve element identity while
suspending readable focus. The announcement bridge baselines documents and
drops background updates without replay. Actual-service XCTest and native-window
XCUITest cover privacy, editor focus, offscreen reveal, tab state and reload.

Complete native acceptance passed 173 cases with one physical Zhuyin skip.
Host-prepared Rust workspace acceptance passed 7,249 cases with
69 ignored; build, strict Clippy, formatting, eight signatures
and owned native process cleanup passed. Existing default-runner subprocess
startup failures are retained separately. A temporary runner checks eight
service binaries after Cargo materialization and before the first test
executable, without modifying sources, signatures or original test deadlines.
This host readiness condition does not establish clean default-runner cold-start
acceptance. The measured result and reproducible wrapper are recorded in
[MACOS_LIVE_REGION_ROTOR_RESULTS.md](MACOS_LIVE_REGION_ROTOR_RESULTS.md).

At that increment, retained announcement delivery/acknowledgement remained
pending. The subsequent delivery milestone below resolves that limitation.
Initial alert creation, descendant-scoped atomic/relevant overrides, complete
ARIA/name computation, additional rotor kinds and physical VoiceOver remain
pending. Document paragraph selection is accepted in the increment above.
This does not complete the full browser design.

## Retained announcement delivery increment

Core now retains ready announcements across reads, resize and later mutations
until a source/document-scoped prefix acknowledgement releases them. The bounded
FIFO reports overflow and queued clipping, and purges invalid/private/off
contributors. AppKit delivers each batch once, acknowledges deliberately dropped
baselines/background updates and coalesces only idempotent ACK retries off the
main thread. Older cores without the capability receive no new command.

Acceptance passed 176 native cases with one existing physical Zhuyin skip and
7,260 workspace cases with 69 ignored across 473 suites. The complete Rust run
used the normal repository signing runner after the unchanged executables again
started normally; no auxiliary readiness helper or deadline change was used.
Earlier incomplete startup attempts, the one cfg(test)-only assertion correction
after native acceptance and all frozen input scopes remain documented in
[MACOS_ANNOUNCEMENT_DELIVERY_RESULTS.md](MACOS_ANNOUNCEMENT_DELIVERY_RESULTS.md).
Consumption ACK does not establish physical VoiceOver speech completion.

## Descendant live-region increment

This increment preserves per-contributor atomic/relevant scope, groups nearest
atomic changes once in document order and shares public author-label provenance
with native names. Explicit false, removal overrides, suppressed baselines,
external labels, privacy, busy coalescing, clipping and group limits have regression
coverage. Complete acceptance passed 178 native cases with one existing physical
Zhuyin skip and 7,279 workspace cases with 69 ignored across 473 suites. Application,
core and native-test inputs stayed unchanged; the results record the later MCP
`cfg(test)` fixture correction and both incomplete workspace attempts. Assistant
tests passed after readiness observation; the startup timeout cause remains unproved.
See [MACOS_DESCENDANT_LIVE_RESULTS.md](MACOS_DESCENDANT_LIVE_RESULTS.md).
Initial alerts, complete ARIA/name computation, additional rotors, document text
selection and physical VoiceOver remain separate work.

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
JavaScript keyboard/beforeinput/input/composition event dispatch,
complete bidirectional shaping, caret blink, preferred vertical caret position
and general keyboard input during asynchronous focus changes. The later
keyboard milestone buffers native events specifically across Tab handoffs;
it does not complete native editing acceptance.

## Native Undo/Redo increment

The core stores private, zeroizing before/after values and UTF-16 selections for
each editable control, bounded globally per page to 128 transactions and 4 MiB.
Atomic paste/cut and committed compositions coexist with one-second typing and
same-direction deletion groups. Movement, selection and focus boundaries end a
group. AppKit forwards native Edit commands, keyboard shortcuts and context-menu
actions to the same ordered, document/focus-fenced input boundary; its proxy
disables Cocoa undo registration and stores no text transactions. Cocoa address
and find editors retain their own responder behavior.

Marked composition disables replay; cancellation preserves the earlier redo
branch. Password transactions remain private in the core, with only availability
and selection/length metadata exposed. Transfer keeps the existing page's
history with fresh focus ownership. Navigation/reload, form reset, external
value/text writes and removed or no-longer-editable controls discard affected
histories. A new edit drops that control's redo branch; eviction is reported in
the page context menu. Session restoration never persists editing histories.
The full native gate passed with 155 passes and one physical Zhuyin skip; the
Rust workspace passed 7,217 cases. The increment also stabilizes the session
Settings status row and verifies Forget through GUI state, normal exit, disk
removal and relaunch. The dated acceptance record is
[MACOS_UNDO_REDO_RESULTS.md](MACOS_UNDO_REDO_RESULTS.md).

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
Complete validation, DOM key/input/focus/composition events, full chrome
Tab traversal, context menus, drag/drop and file-selection remain required.
The later find increment delivers page search.
The native select increment below implements multiple-select interaction. Physical OS IME and screen-reader
acceptance remain separate gates.

## Native select controls increment

AppKit presents single-choice selects with an NSMenu built from the core's
option identities, visible labels, optgroup labels, enabled state and selected
choice. Mouse and Enter/Space activation wait for acknowledged native focus.
Choices must still belong to the live focused control; popup actions additionally
fence the frame that supplied their labels. Navigation, tab/window ownership,
focus changes, native chrome editing and newer frames invalidate tracking.

Multiple and size-based controls render clipped option rows through shared
layout/paint fragments, with matching pointer and accessibility bounds. Core
interaction handles ordinary selection, Command toggle/movement, Space toggle,
Shift ranges, Select All, Home/End/PageUp/PageDown, Unicode prefix search and
repeated-letter cycling. Wheel scrolling works over an enabled list without
changing page focus, selectedness or outer scroll. Keyboard movement reveals the
active row. Focus changes end prefix/range sessions; form reset and document
replacement clear transient interaction state. The multiple-select public value
is the first selected option, and the existing form entry list preserves all
selected enabled values in document order. An unselected size-based list has no
implicit first choice.

Accessibility exposes a native list with selected children and supported option
selection actions. AX focus preserves existing selections; disabled, clipped or
invalidated elements cannot mutate a control. The native state is bounded to
1,024 choices with 256 Unicode scalars per label/group; the menu reports omitted
choices, and keyboard interaction retains access to the complete core option
set. The optional state uses indirection to keep the existing server message
and reference-frontend event sizes bounded. Older text-state payloads continue
to decode without select metadata.

Complete acceptance passed: 161 native tests with one physical Zhuyin skip and
7,226 Rust tests; all-targets build, strict Clippy, formatting, eight signatures
and owned-process cleanup passed with unchanged frozen inputs. The result record
is [MACOS_SELECT_CONTROLS_RESULTS.md](MACOS_SELECT_CONTROLS_RESULTS.md).
This increment does not complete cancelable keyboard/beforeinput/input/change/
focus events, HTML dirty-selectedness/defaultSelected IDL, customizable selects,
full option CSS/layout, complete validation or physical VoiceOver acceptance.

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

The context increment below supplies lifecycle and persistent profile identities;
the later session and placement increments provide restart restoration and
native tab ordering. Physical IME/screen-reader acceptance remains separate.

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
IDs. Profile-catalog restoration recreates empty named contexts with fresh runtime
IDs and preserves their UUIDs. Page restoration is an independent opt-in choice
provided by the durable session increment below. Invalid saved
catalogs are retained and profile management is disabled. The durable native
session increment below adds opt-in tab/window restoration. Cookies, cache and authentication are not yet
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
Each print exchange handshakes on its own connection and closes it before the
panel waits; unrelated Accessibility broadcasts cannot fill an idle print reader.

The AppKit panel supplies paper size, orientation, scale and page selection.
Its PDF destination uses the actual system Save dialog.
The document-modal panel yields the MainActor until AppKit's completion callback;
both Save PDF and Cancel release the captured job and permit another invocation.
Preview callbacks read the owning operation's settings even without a current
thread-local operation. Numeric controls can
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
[file-input results](MACOS_FILE_INPUT_RESULTS.md). Selected-file FileList/File/Blob values and
input/change/cancel events are now accepted, see [file API results](MACOS_FILE_API_RESULTS.md).
Native label activation is now accepted, see [label results](MACOS_LABEL_ACTIVATION_RESULTS.md).
Directory/capture, drag/drop, remaining File API interfaces, private owner panels
and the other delivery gates remain open.

## Completion audit

Before final completion, check every milestone against its code, native app,
result bundle, result record and commit/push state. Verify the same core owns
all inspected/rendered/edited pages. Scope native UI assertions to the exact
feature they exercise; do not use a broad green test count as evidence for
untested IME, screen-reader, permission, print, profile or window behavior.
The Phase 22 cross-platform work remains separate from this macOS delivery;
its unfinished items must not be reported as achieved by the macOS frontend.


## Durable native session restoration increment

The native Settings scene provides independent Remember and Reopen-on-startup
choices, Restore Last Session, and explicit Forget. A versioned, bounded archive
stores logical profile/window UUIDs, groups (including empty groups), tab order,
selection, history cursor and forward branch, zoom and window geometry. Runtime
IDs, frame/document identities, page HTML, form values, selected files, POST
bodies and permission grants are excluded. Initial default-page startup preserves
a previous saved session for manual restoration. User changes debounce saving;
normal termination flushes all live windows before stopping owned services.
Malformed or over-limit archives are retained until explicit Forget.

Restoration recreates core-owned windows/tabs and profile groups. Every current
GET uses ordinary mandatory URL/content review before history metadata can attach
to its live document. Redirected current URLs use the reviewed final destination.
Rejected navigation preserves its native denial and original saved URL. Imported
POST entries are expired markers: a current POST becomes a fixed warning in a
blank tab, and reload/history never silently resubmit it as GET. Forward/backward
GET history remains gated. An additive, document-bound NavigationSession protocol
keeps request, tab, window and profile ownership checks. MCP and the reference
frontend tolerate the additional reply without changing legacy wire shapes.

Native UI coverage exercises automatic and manual restoration, three windows
across two profiles, selected/collapsed groups, forward history, zoom and geometry,
private POST-body exclusion, expired-form behavior, invalid preferences and
changed-content denial. Acceptance is recorded in
[session restoration results](MACOS_SESSION_RESTORE_RESULTS.md). Full network
storage partitioning, fullscreen/miniaturized session state, restored form contents
and the other unfinished milestones remain outside this increment.


## Native download credential-store increment

Downloads > Credentials uses the shared transfer parser to review SFTP/FTPS
account identity, then an explicit human action sends the existing private
Keychain Save/Remove commands. Native SecureField masking, cleared drafts,
owned-session fences, independent credential namespaces, normal relaunch and
English/Traditional Chinese controls are accepted. No resolver network request,
transfer, stored-secret readback, AI tool or permission bypass is introduced.

Complete native acceptance passed 235 methods with one existing
physical Zhuyin skip; complete Rust workspace passed 7,314 cases with
69 existing ignored cases. Formatting, strict Clippy and all-targets
build passed. Eight strict signatures, two universal Swift apps and the
source/product/owned-process audit passed. Rust/static and full native gates
share the same 1,809 inputs; the earlier focused native scope differs
only in the documented Clippy String-borrow correction. See
[the dated results](MACOS_DOWNLOAD_CREDENTIALS_RESULTS.md). Native private-key
file/configuration UI, authenticated live-server acceptance, destination
selection and all other unfinished browser requirements remain open.


## Native SFTP file configuration increment

Native NSOpenPanel file selection, explicit path-only Apply/Restore, owned service
checkpoint/restart, retained core identity, paused transfers across relaunch,
fresh credential-review ownership and English/Traditional Chinese controls are
accepted. An actual encrypted private key, strict generated known hosts and a
Keychain passphrase deliver exact bytes through BlueIce SFTP; native credential
removal and test-harness daemon cleanup are verified. Complete native acceptance
passed 245 methods with one existing physical Zhuyin skip; exact scope is 246
methods and focused scope passed all 25. Eight signatures, two universal Swift
apps, source/product consistency and zero owned processes are verified.
All accepted Rust inputs remain unchanged, retaining the actual 7,314 passed,
69 ignored workspace evidence from the credential milestone. No new workspace
run or remote deployment acceptance is claimed. See
[the dated results](MACOS_SFTP_FILES_RESULTS.md). The full browser goal and all
remaining download, accessibility, display and physical-system work stay open.


## Native download folder selection increment

Downloads > Folder uses SwiftUI drafts and an AppKit picker with explicit Apply.
Transfer IDs, completed files and paused partials stay in their original folders;
new transfers use the selected folder. The shared browser core remains running.
Malformed preferences or an unapproved original root refuse startup before
catalog changes; explicit original-folder selection recovers preserved history.

The complete native scope contains 255 methods: 254 passed, zero failed,
1 existing physical Zhuyin skip. Nine focused cases passed. Fresh Rust fmt/strict Clippy/all-targets
build and workspace tests passed (7,321 passed, 69 ignored,
475 groups). Actual screenshots, source/product/process audits
and earlier failed/interrupted attempts are retained in
[the dated results](MACOS_DOWNLOAD_FOLDER_RESULTS.md). Other pending milestones
and the whole browser goal remain in progress.

## Automatic original response downloads increment

Normal Return, link and one-shot POST navigation hands the actual response to the
owned download manager and preserves document/history identity. SwiftUI/AppKit
shows progress, cancellation, quarantine and Finder actions in the originating
window and selected folder. Header-first classification, bounded explicit stream
completion, HTTP/TLS cancellation, stale-tab/window refusal and nonreplayable
restart are verified at public Rust and actual native boundaries.

The fresh full Rust and exact unfiltered native suites passed, with the existing
physical Zhuyin limitation recorded. See the [contract](MACOS_RESPONSE_DOWNLOADS_CONTRACT.md)
and [dated results](MACOS_RESPONSE_DOWNLOADS_RESULTS.md). General page/OS drag-and-drop,
storage partitioning, remote protocol interoperability, physical IME/VoiceOver
and remaining browser requirements stay open.

## Ordinary selected-file API and event increment

Normal macOS browsing now starts the owned isolated BlueJS host and includes its
RegExp worker. Admitted page scripts can inspect selected FileList/File snapshots,
construct and slice Blob/File values, read their exact bytes/text asynchronously
and observe bubbling input/change/cancel events. Script clear and form reset keep
retained snapshots without selection events. Tab/document/revision checks reject
stale native callbacks, while one owner DOM channel serves isolated window realms.

The complete Rust gates and exact unfiltered native suite passed. See the
[contract](MACOS_FILE_API_CONTRACT.md) and [dated results](MACOS_FILE_API_RESULTS.md).
Remaining File API stream methods, FileReader/object URLs, BlueTS numeric index
signatures/host new-expression inference and general page/OS drag-and-drop stay
open alongside storage partitioning, remote protocol and physical hardware gates.

## Native label activation increment

Core now resolves explicit and implicit labels, skips hidden inputs and blocks
forwarding from interactive descendants. Label and associated-control clicks run
before the existing default; cancellation, changed association, disabled targets
and replaced documents suppress stale activation. A correlated native gesture can
present the existing file picker after validating the actual control again.
Unsolicited replies have no presentation effect.

Forwarding currently requires an enabled control with a layout fragment and no
`hidden` or `inert` restriction on the control or its ancestors. Labels targeting
file controls without a layout fragment remain pending.

Focused public session and actual macOS UI regressions exercise file selection,
cancel, text editing, checkbox/radio state, implicit buttons and association
changes. The complete Rust gates and exact unfiltered 269-method native suite
passed. Full/focused native inputs match; the sole later change from Rust inputs
is an existing Swift AX test's fresh-frame synchronization barrier. Production
Swift/Rust and build inputs are unchanged, as recorded in the dated results.
Native file presentation additionally
matches an independent echoed gesture identifier, so another client's colliding
request/tab IDs do not authorize a picker. See the
[label activation contract](MACOS_LABEL_ACTIVATION_CONTRACT.md) and
[dated results](MACOS_LABEL_ACTIVATION_RESULTS.md).
