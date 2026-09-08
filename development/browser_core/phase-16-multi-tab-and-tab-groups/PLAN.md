# Phase 16 — Multi-Tab and Tab-Group Support

[← Back to plan](../BROWSER_CORE_PLAN.md)

**Status**: In progress — minimal first slice done: multi-tab support inside `core` (wire-protocol `tab_id` addressing, `TabManager`, no navigation history beyond what a single `Page` already lacks). `frontend-reference`'s tab-strip UI and tab groups are both explicitly deferred — see "What this slice deliberately does not do" below.

## Objective

Let `core` manage more than one navigable context ("tab") at once, addressed independently by every `blueice-ipc` client, and eventually let tabs be organized into named/colored groups the same way a human browser's tab groups work — while keeping the project's core invariant intact: a human's `frontend` and an AI's `mcp-server` must be able to observe (and act on) tabs independently, never forced through one shared "active tab" pointer that would silently reintroduce the dual-track human/AI observation gap `CLAUDE.md`'s core goal exists to reject.

## Why this reverses a settled decision

`phase-2-mvp-scope/PLAN.md` originally excluded multi-tab explicitly: "there is no multi-tab concept anywhere in `Page`'s design (one `Page` is one navigable context)." That exclusion is reversed here, at the user's explicit request. `Page` itself is *not* redesigned — it remains exactly "one navigable context, one instance." What changes is that `core` now owns a *collection* of `Page`s instead of exactly one.

## Framing correction that shapes this design

`core`'s own doc comment says it "accepts exactly one client connection, then exits" — it would be easy to assume multi-tab needs that to change. It doesn't. `blueice-launcher` (`phase-8-live-core-hotswap/PLAN.md`'s minimal first slice) already solved "many external clients sharing one `core` connection": the launcher holds the one connection `core` will ever see and fans arbitrarily many external clients (`frontend`, `mcp-server`, tests) through it. Multi-tab is entirely about what *that one already-existing connection's session loop* manages state-wise — many `Page`s instead of one — and needs zero change to how many connections `core` accepts.

## Minimal first slice: multi-tab inside `core`, no `frontend` UI yet

**Wire protocol** (`backend/ipc/src/lib.rs`): extends the envelope pattern already built for `request_id` (`phase-8-live-core-hotswap/PLAN.md`'s protocol_version/request-ID follow-up) — `ClientEnvelope`/`ServerEnvelope` gain `tab_id: Option<u64>` the same way (`#[serde(default, skip_serializing_if = "Option::is_none")]`). `None` means "the default tab" (the one tab `core` creates at startup), reproducing pre-Phase-16 single-`Page` behavior byte-for-byte — every pre-existing test in `blueice-ipc`/`session.rs`/the launcher's own tests keeps passing unmodified, the same backward-compatibility property `request_id` already established.

New lifecycle messages: `ClientMessage::{OpenTab { url: Option<String> }, CloseTab, ListTabs}` / `ServerMessage::{TabOpened { tab_id, url }, TabClosed { tab_id }, Tabs(Vec<TabSummary>)}`. `CloseTab` deliberately carries no inline `tab_id` field -- it's addressed via the envelope's `tab_id`, the same as every other per-tab message, rather than a redundant second addressing mechanism. A `tab_id` that doesn't resolve on any per-tab message replies `ServerMessage::Error` — a protocol-addressing error, not the harmless no-op a stale `NodeId` already gets in `Page::act`.

**Every reply echoes the *resolved* tab, never the raw (possibly absent) request field.** `ServerEnvelope.tab_id` on a reply is always `Some(target.as_u64())` -- the tab `run_session` actually resolved the request to (`tab_id.unwrap_or(tabs.default_tab())`) -- never the original envelope value verbatim. This matters specifically for a request that left `tab_id` implicit: echoing `None` back would defeat the whole point of adding this field, since a second client sharing the connection via `blueice-launcher`'s broker would have no way to tell which concrete tab an untagged client's broadcasted reply was actually about. Proven by `session::tests::a_reply_to_an_untagged_request_still_echoes_the_resolved_default_tab_id`.

**Deliberately no `SwitchTab`/"active tab" message.** Which tab a *particular window* is currently showing is frontend-local chrome state, the same category `ChromeCommand::SetVisible` already establishes (window visibility has no cross-observer semantic content `core` needs to track). A human's `frontend` and an AI's `mcp-server` may legitimately be looking at *different* tabs simultaneously — an AI doing background research in tab 3 while a human keeps browsing tab 1 is normal multi-tab usage, not an edge case, and a single core-tracked "active tab" pointer would silently reintroduce the exact human/AI observation asymmetry this project exists to reject.

`AiSnapshot` (`backend/ipc/src/ai.rs`) gains `pub tab_id: u64` — not just the envelope's field, because `mcp-server` unwraps `ServerMessage::Representation(snapshot)` before handing it to its own MCP tool result, at which point the envelope is gone; without this a snapshot has no self-contained way to say which tab it's from. Same justification already used for `AiSnapshot.generation`.

**`NodeId` stays page-local, unchanged.** Two tabs' node `7` are unrelated nodes, disambiguated by `tab_id` carried alongside every action — not by making `NodeId` globally unique, which would ripple through `blueice_dom`/`blueice_layout`/`blueice_paint` for no real benefit.

**`TabManager`** (`backend/core/engine/src/tabs.rs`): sits at the same layer `Page` does — testable domain logic, no wire-protocol knowledge — mirroring the existing `Page`/`session.rs` split. `TabId(u64)` is a newtype (`from_u64`/`as_u64`, mirroring `blueice_dom::NodeId`'s own pattern) with a monotonic, never-reused allocator (same discipline `NodeId`'s own allocator already uses across navigations). `TabManager::new(width, height)` creates exactly one initial tab, so a client that never sends `OpenTab` sees identical behavior to before this phase.

**`run_session`** (`backend/core/engine/src/session.rs`): `page: &mut Page` becomes `tabs: &mut TabManager`. `generation` and `frame_dir` **stay single/session-wide, not per-tab** — every tab's `FrameReady` still gets the next global monotonic number, so `backend/ipc/src/shm.rs` needed zero changes (filenames stay collision-free by construction) and the AI-facing "same generation = same render pass" property still holds across tabs. The loop resolves `tab_id.unwrap_or(tabs.default_tab())` and dispatches to that `Page` exactly as before, or replies `Error` if the tab doesn't exist.

## What this slice deliberately does not do

- **`frontend-reference`'s tab-strip UI** — a `HashMap<tab_id, CurrentFrame>` plus a `selected_tab`, stdin tab commands (`tab-new`/`tab-close`/`tab N`) as a next step before a real graphical tab strip, following the same "stdin stands in for a real AI-facing/UI control channel" precedent the reference frontend's existing `show`/`hide`/`credits` commands already set. Deferred as its own follow-up slice — the engine/protocol side needed to prove out first.
- **Tab groups** — `TabGroup { id, name, color, collapsed }`, core-owned session state exposed over IPC the same way `Highlight` already is (not `frontend`-local: an AI agent organizing multi-tab work needs its groupings visible to a human watching the same session, and vice versa — the same reasoning that already keeps `Highlight` core-owned rather than frontend-local, per `phase-1-ai-representation-layer/PLAN.md` §4's "AI-to-human sync" framing). `TabManager` would gain `groups: HashMap<GroupId, TabGroup>` and each tab a `group_id: Option<u64>`; `TabSummary`/`ListTabs` already reserves a `group_id` field for this. New messages: `CreateTabGroup`/`SetTabGroup`/`RenameTabGroup`/`SetTabGroupColor`/`SetTabGroupCollapsed`/`CloseTabGroup`/`ListTabGroups`. Strictly downstream of tabs existing as a collection; not started.
- **Per-tab navigation history** (back/forward) — no navigation history exists for a single page today either; multi-tab doesn't newly require it, and it's out of scope here unless separately requested.
- **Per-tab viewport/relayout policy for background tabs** — today every `Resize` is `tab_id`-tagged and only relayouts that one tab, so a background tab keeps its last-known layout until next addressed; whether that's the right policy long-term (vs. eagerly relaying out background tabs) is left for when the frontend tab-strip work makes it observable.

## Checklist

**Minimal first slice (multi-tab inside `core`) — built:**

- [x] Add `tab_id: Option<u64>` to `ClientEnvelope`/`ServerEnvelope`, wire-compatible (`None` = default tab) — `backend/ipc/src/lib.rs`, plus `write_/read_client_message_with_ids`/`write_/read_server_message_with_ids` alongside the existing `_with_id` (request-id-only) and unchanged-signature functions
- [x] Add `OpenTab`/`CloseTab`/`ListTabs` client messages and `TabOpened`/`TabClosed`/`Tabs`/`TabSummary` server replies
- [x] Add `AiSnapshot.tab_id`
- [x] Implement `TabManager`/`TabId` — `backend/core/engine/src/tabs.rs`, 10 unit tests (ID-never-reused, default-tab semantics, close-then-reopen, window-size inheritance for new tabs)
- [x] Rewrite `run_session` to dispatch through `TabManager` instead of a single `Page`, echoing the *resolved* tab on every reply (see "Every reply echoes the resolved tab" above) — `backend/core/engine/src/session.rs`, 8 new multi-tab tests (`open_tab_creates_a_second_tab_visible_in_list_tabs`, `open_tab_with_a_url_navigates_it_and_sends_a_frame`, `open_tab_with_a_failing_url_replies_error_not_tab_opened`, `an_action_addressed_to_one_tab_never_affects_another_tabs_state`, `a_message_addressed_to_an_unknown_tab_replies_error_not_a_silent_no_op`, `close_tab_removes_it_and_a_later_message_to_it_becomes_an_error`, `a_reply_to_an_untagged_request_still_echoes_the_resolved_default_tab_id`), all 21 pre-existing single-tab-shaped tests kept passing completely unmodified beyond the mechanical `Page::new` → `TabManager::new` rename
- [x] Update `blueice-core.rs` to construct/drive a `TabManager`
- [x] Real-subprocess test: two tabs opened (one via `Navigate`, one via `OpenTab { url: Some(..) }`), navigated to different URLs, `FrameReady`/`Representation` addressed independently and never cross-contaminate — `backend/core/engine/tests/core_binary.rs`'s `real_subprocess_serves_two_independently_addressed_tabs_without_cross_contamination`

**Deferred (recorded, not forgotten):**

- [ ] `frontend-reference` tab-strip UI
- [ ] Tab groups (`TabGroup`, `CreateTabGroup`/etc., core-owned session state)
- [ ] Revisit background-tab relayout policy once the frontend tab strip makes it observable
