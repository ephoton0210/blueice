# macOS live regions and rotors

This increment keeps accessibility semantics, privacy and navigation fences in
the browser core. AppKit advertises native rotors and posts application-level
announcements; it does not reconstruct a DOM or use clicks to move the reading
cursor. Complete native and host-prepared Rust verification is recorded in
[MACOS_LIVE_REGION_ROTOR_RESULTS.md](MACOS_LIVE_REGION_ROTOR_RESULTS.md).

## Core contract

`AiSnapshot.accessibility` is an optional, backwards-readable overlay with a
document generation, monotonically increasing announcement revision, the latest
layout mutation's announcements, excluded native node IDs and native name
corrections. The inspection tree retains its existing source-observability
contract. Native names exclude hidden or protected descendant content, including
hidden associated labels. Password controls retain their declared accessible
label and secure role, without exposing their value.

Explicit `aria-live=polite/assertive/off` and implicit `status`, `alert` and `log`
regions are supported. Status and alert default to atomic announcements. Busy
regions defer and coalesce updates against their last ready content; ancestor
atomic/relevant settings inherit, and relevant additions, text and removals are
recognized. Text relevance includes new text nodes and text alternatives;
additions distinguish a newly added element from text added to an existing
element. A nested live region owns its own content; a nested `off` subtree
does not enter an outer announcement. Hidden, inert, `aria-hidden`, display-none,
zero-opacity and hidden/collapsed-visibility content is excluded. Passwords,
file values, payment fields and one-time codes are excluded from announcements.
Content becoming hidden or private is removed from the comparison baseline
before computing removals, so a removal announcement cannot disclose it.

The stream contains the latest layout mutation, not a retained text history.
Repeated reads retain the same revision. Initial document content establishes
a baseline. A subsequent layout replaces the batch, including when that layout
has no new announcement; a frontend read between the layouts is required to
observe the earlier batch. Retained delivery and acknowledgement remain pending.
Limits are 256 regions, 64 announcements per layout, 4096 Unicode
scalars per region/announcement, 50,000 inspected nodes and depth 256. Truncation
is explicit. No Unicode scalar is split.

`AccessibilityReveal` carries accessibility context version, frame-directory source,
document generation, frame generation and node ID inside the existing
context/window/tab envelope. The core validates the current semantic node and
visibility, minimally scrolls it vertically into view and emits a new frame
before the fenced reply. It does not activate a link, issue a navigation,
change DOM focus, alter editor selection or confer trusted gesture authority.

## AppKit contract

The page advertises headings, heading levels 1–6, links, images, lists and
buttons. Searches use document order, forward/backward direction and
case/diacritic-insensitive text filtering. A nil current item starts at the
appropriate end; searches do not wrap. Offscreen items are searchable. Search
alone has no side effect; setting the reading target's AX focus invokes the
core's reveal operation. Old-document rotors, invalidated elements and foreign
current items fail closed. Ordinary control actions retain their existing
native input behavior.

Swift 6 actor-isolated protocol conformances keep callbacks and element state on
the main actor. Each reveal opens a fresh two-second private IPC exchange; it
closes on success or failure and never retries an uncertain mutation. AppKit
keeps reading focus separate from the core's editor focus. Leaving a rotor item
clears its reading focus without blurring the core editor. A transient frame gap
temporarily removes the readable focus target while retaining its same-document
element identity; new editing focus clears the reading target.

The native announcement bridge baselines a newly selected document, drops
inactive-window updates while advancing its revision and does not replay old
messages after a tab switch, reload, background interval or repeated frame read.
Polite/assertive map to native low/high announcement priorities. The default
bridge posts `announcementRequested` on the application accessibility element.

## Validation scope and remaining work

Core regression tests cover privacy transitions, busy coalescing, nested
regions, relevant removals, resource limits, native name corrections and reveal
ownership. Actual-process tests exercise the context/window envelope, frame
reply and network non-activation. XCTest invokes real AppKit rotor callbacks
against an actual launcher/core and checks announcement delivery at the native
bridge. XCUITest checks native page controls, private text exclusion, tab state
and document reload. These automated checks do not establish physical VoiceOver
speech or interactive rotor acceptance.

Initial alert creation before the first observed snapshot, descendant-scoped
atomic/relevant overrides, full ARIA role/accessibility-name computation,
landmark/table/visited-link/text-field rotors, and physical VoiceOver remain
separate work. Paragraph selection and font fallback also remain pending.

Primary contracts: [WAI-ARIA 1.2](https://www.w3.org/TR/wai-aria-1.2/),
[AppKit custom rotors](https://developer.apple.com/documentation/appkit/nsaccessibilitycustomrotor),
[rotor search delegate](https://developer.apple.com/documentation/appkit/nsaccessibilitycustomrotoritemsearchdelegate),
and [native announcements](https://developer.apple.com/documentation/appkit/nsaccessibility-swift.struct/notification/announcementrequested).
