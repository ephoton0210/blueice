# macOS native tab placement

This increment implements native tab dragging within a window, between groups and
between same-context windows, with a keyboard/menu ordering alternative and
durable session order. General page/OS drag-and-drop remains separate browser
delivery work. Core/protocol, complete native/workspace and final
source/product/signature/process acceptance passed on the same frozen inputs;
see [the dated results](MACOS_TAB_PLACEMENT_RESULTS.md).

The canonical native window registry owns tab order and group membership.
`WindowAction::PlaceTab` carries the source window, destination window, optional
insertion tab and optional destination group. Source membership, destination
membership and context/group ownership are checked before any mutation. A stale
source, missing window/tab/group, foreign insertion anchor or different context
cannot partially move or regroup a tab. Same-window organization leaves the
existing page, editor focus/composition, document and frame generations intact.
Cross-window placement reuses the existing page and the established native
editor/display handoff; it does not reload or create another document.

Owned `WindowState` advertises `tab_placement_v1`. Older snapshots omit it and
new controls remain unavailable until this capability is present. Existing
MoveTab behavior and ordinary tab protocol retain their shapes. Native placement
emits `TabPlaced`; the parent updates canonical snapshots before selecting an
incoming cross-window tab. Same-window ordering preserves the selected tab.
The source window ID is a lifecycle fence, not a human gesture or access grant.

The native provider exposes only a random workspace-owned drag token
with own-process visibility. No URL, page text, editor value, credential, file or
permission decision travels in the drag payload. The source and target are
revalidated after provider completion; cancelled, expired, foreign, stale and
already consumed tokens cannot mutate tabs. Source/destination context ownership
must match. Delayed provider callbacks do not select or reopen a closed tab.
The current insertion edge is re-resolved: a live reference does not validate an
obsolete after-reference neighbor or group append position. Shared placement
validity also fences the after-reference relation for queued menu actions.
Authenticated stale drops end their insertion indicators; forged or replaced
payloads cannot cancel the current owned drag.

Dropping on a tab places before/after that tab within its group. A group header
accepts membership placement even when collapsed, retaining its collapse state.
The end/new-tab target places an ungrouped tab at the end. Indicators show the
current insertion edge and clear when the owned drag is consumed or cancelled.
Keyboard/menu moves use the same core placement command;
disabled/stale boundaries cannot create a page or navigation request.

Acceptance requires core atomicity/ownership tests, real-session preservation of
composition and stale-source rejection, protocol capability/typed-ID tests,
native source-token/target validation, actual-window drag ordering and groups,
cross-window transfer and cross-profile refusal, keyboard alternative and normal
restart/session order, existing native regressions, full native/workspace gates,
formatting, strict Clippy, all-targets build and source/product/signature/process
audits. Native screenshots/results remain ignored; dated text records retain all
attempts and distinguish physical IME/VoiceOver from deterministic checks.
