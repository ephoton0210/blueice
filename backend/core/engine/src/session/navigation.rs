// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

/// Which reply variant a background gated navigation's eventual
/// success produces: `Navigate`/`Click`/`ActOn`'s href-resolution sites
/// all want a plain `Navigated`; `OpenTab` wants `TabOpened` instead --
/// threaded through the whole async path (spawned thread ->
/// [`Completion`] -> the poll loop's reply-writing) purely to pick the
/// right variant once the outcome is known.
pub(super) enum PendingKind {
    Navigate,
    OpenTab,
    Reload,
    /// A URL-only Back/Forward traversal. The cursor advances only after its
    /// gated fetch succeeds, unlike a new navigation which creates a branch.
    History(HistoryDirection),
}

/// One background gated-navigation's eventual result, delivered over
/// the loop's completion channel -- always tagged with the *original*
/// `tab_id`/`request_id`/`seq` captured when the async op began, since
/// none of those can be assumed still "current" by the time this
/// arrives (see [`apply_completion`]).
pub(super) struct Completion {
    pub(super) tab_id: TabId,
    pub(super) seq: u64,
    pub(super) request_id: Option<u64>,
    pub(super) kind: PendingKind,
    pub(super) outcome: NavOutcome,
    /// The assistant's translation of a cleared page's text, obtained on the
    /// navigation thread; `None` keeps the page as fetched.
    pub(super) translations: Option<Vec<String>>,
}

/// Advances a tab's navigation epoch and returns it. Any outstanding fetch
/// tagged with an earlier epoch is no longer allowed to apply: this is used
/// not only for a new URL navigation, but also for restoring a history entry
/// while an earlier URL fetch is still in flight.
pub(super) fn supersede_pending_navigation(
    pending_nav_seq: &mut HashMap<TabId, u64>,
    tab_id: TabId,
) -> u64 {
    let seq = pending_nav_seq.entry(tab_id).or_insert(0);
    *seq += 1;
    *seq
}

/// Starts one Back/Forward traversal. URL-only entries deliberately take the
/// same validation, gatekeeper, and asynchronous fetch path as a normal
/// navigation; a retained snapshot is the explicit exception and can be
/// restored immediately. Crucially, a URL entry's cursor is not moved until
/// its fetch has cleared, so a failed/blocked history reload leaves both the
/// visible page and the history position untouched.
#[allow(clippy::too_many_arguments)]
pub(super) fn begin_history_navigation<S: Write>(
    tabs: &mut TabManager,
    stream: &mut S,
    frame_dir: &Path,
    generation: &mut u64,
    reply_tab: Option<u64>,
    request_id: Option<u64>,
    tab_id: TabId,
    direction: HistoryDirection,
    pending_nav_seq: &mut HashMap<TabId, u64>,
    downloads_refresher: &mut DownloadsRefresher,
    completion_tx: &mpsc::Sender<Completion>,
    gatekeeper_socket: &Path,
    extension_events: Option<&mpsc::SyncSender<ExtensionRuntimeEvent>>,
) -> io::Result<()> {
    let direction_name = match direction {
        HistoryDirection::Back => "back",
        HistoryDirection::Forward => "forward",
    };
    let Some(destination) = tabs.history_destination(tab_id, direction) else {
        return write_error(
            stream,
            reply_tab,
            request_id,
            format!("cannot go {direction_name}: no {direction_name} history entry"),
        );
    };
    let kind = PendingKind::History(direction);
    match destination {
        HistoryDestination::Post { .. } => write_error(
            stream,
            reply_tab,
            request_id,
            "POST history requires resubmission confirmation".into(),
        ),
        HistoryDestination::Snapshot => {
            // A snapshot restoration is a newer navigation for this tab. A
            // delayed fetch that predates it must never overwrite the restored
            // historical document.
            assert!(tabs.restore_history_snapshot(tab_id, direction));
            supersede_pending_navigation(pending_nav_seq, tab_id);
            if tabs
                .get(tab_id)
                .and_then(Page::url)
                .is_some_and(is_downloads_url)
            {
                downloads_refresher.begin_visit(tab_id);
            }
            reply_success(
                tabs,
                stream,
                frame_dir,
                generation,
                reply_tab,
                request_id,
                &kind,
                tab_id,
                extension_events,
            )
        }
        HistoryDestination::Reload(None) => {
            // The initial blank document has no URL to fetch, but it still
            // moves exactly one history position and never creates a branch.
            assert!(tabs.navigate_history_to_blank(tab_id, direction));
            supersede_pending_navigation(pending_nav_seq, tab_id);
            reply_success(
                tabs,
                stream,
                frame_dir,
                generation,
                reply_tab,
                request_id,
                &kind,
                tab_id,
                extension_events,
            )
        }
        HistoryDestination::Reload(Some(url)) => {
            if tabs.navigate_history_to_built_in(tab_id, direction, &url) {
                supersede_pending_navigation(pending_nav_seq, tab_id);
                if is_downloads_url(&url) {
                    downloads_refresher.begin_visit(tab_id);
                }
                return reply_success(
                    tabs,
                    stream,
                    frame_dir,
                    generation,
                    reply_tab,
                    request_id,
                    &kind,
                    tab_id,
                    extension_events,
                );
            }
            if let Err(e) = blueice_net::validate_url_scheme(&url) {
                return write_error(stream, reply_tab, request_id, e.to_string());
            }
            let navigation_rules = tabs.extension_navigation_block_rule_snapshot();
            if extension_navigation_rules_block_url(&navigation_rules, &url) {
                return write_error(
                    stream,
                    reply_tab,
                    request_id,
                    format!("navigation blocked by a declarative extension rule: {url}"),
                );
            }

            let seq = supersede_pending_navigation(pending_nav_seq, tab_id);
            let tx = completion_tx.clone();
            let socket = gatekeeper_socket.to_path_buf();
            let translation = tabs.translation_config();
            thread::spawn(move || {
                let outcome = gatekeeper_client::check_and_fetch_with_navigation_rules(
                    tab_id,
                    url,
                    &socket,
                    navigation_rules,
                );
                let translations = translate_cleared(&outcome, translation.as_ref());
                let _ = tx.send(Completion {
                    tab_id,
                    seq,
                    request_id,
                    kind,
                    outcome,
                    translations,
                });
            });
            Ok(())
        }
    }
}

/// Starts a gated navigation to `url` for `tab_id`. Built-in `about:`
/// pages ([`crate::page::built_in_page`]) are handled entirely
/// synchronously here -- loaded directly and replied to before this
/// returns, exactly like `Page::navigate` always did, and *never* going
/// through the gatekeeper at all (per `phase-7-local-ai/PLAN.md`, these
/// are BlueIce's own trusted pages, never fetched). A syntactically
/// invalid scheme is also rejected synchronously, with no thread
/// spawned -- this must happen *before* any gatekeeper round trip, not
/// be deferred into the background thread's own fetch attempt: a test
/// double (or a genuinely down) gatekeeper would otherwise turn an
/// ordinary "invalid URL" error into a fail-closed `GatekeeperBlocked`,
/// which is the wrong outcome for a request that was never going to
/// reach the network either way.
///
/// A well-formed http(s) URL bumps `tab_id`'s entry in
/// `pending_nav_seq` and hands the rest of the work (both gatekeeper
/// stages, the fetch) to a background thread that reports its outcome
/// over `completion_tx`, tagged with the sequence number just bumped
/// to -- this is what lets [`apply_completion`] later recognize and
/// discard a stale/superseded completion.
#[allow(clippy::too_many_arguments)]
pub(super) fn begin_gated_navigation<S: Write>(
    tabs: &mut TabManager,
    stream: &mut S,
    frame_dir: &Path,
    generation: &mut u64,
    reply_tab: Option<u64>,
    request_id: Option<u64>,
    tab_id: TabId,
    url: String,
    kind: PendingKind,
    pending_nav_seq: &mut HashMap<TabId, u64>,
    downloads_refresher: &mut DownloadsRefresher,
    completion_tx: &mpsc::Sender<Completion>,
    gatekeeper_socket: &Path,
    extension_events: Option<&mpsc::SyncSender<ExtensionRuntimeEvent>>,
) -> io::Result<()> {
    begin_gated_request(
        tabs,
        stream,
        frame_dir,
        generation,
        reply_tab,
        request_id,
        tab_id,
        url.into(),
        kind,
        pending_nav_seq,
        downloads_refresher,
        completion_tx,
        gatekeeper_socket,
        extension_events,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn begin_gated_request<S: Write>(
    tabs: &mut TabManager,
    stream: &mut S,
    frame_dir: &Path,
    generation: &mut u64,
    reply_tab: Option<u64>,
    request_id: Option<u64>,
    tab_id: TabId,
    navigation: BrowserNavigation,
    kind: PendingKind,
    pending_nav_seq: &mut HashMap<TabId, u64>,
    downloads_refresher: &mut DownloadsRefresher,
    completion_tx: &mpsc::Sender<Completion>,
    gatekeeper_socket: &Path,
    extension_events: Option<&mpsc::SyncSender<ExtensionRuntimeEvent>>,
) -> io::Result<()> {
    let url = navigation.request.url().to_string();
    tabs.get_mut(tab_id).expect("live tab").submission_pending = false;
    // Every navigation supersedes an older in-flight fetch for this tab
    // before choosing its synchronous or asynchronous path, so a built-in
    // page or an invalid-URL response cancels a stale fetch just as an
    // http(s) navigation does.
    let this_seq = supersede_pending_navigation(pending_nav_seq, tab_id);
    let built_in = if matches!(kind, PendingKind::Reload) {
        tabs.reload_built_in(tab_id, &url)
    } else {
        tabs.navigate_to_built_in(tab_id, &url)
    };
    if built_in {
        if is_downloads_url(&url) {
            downloads_refresher.begin_visit(tab_id);
        }
        return reply_success(
            tabs,
            stream,
            frame_dir,
            generation,
            reply_tab,
            request_id,
            &kind,
            tab_id,
            extension_events,
        );
    }
    if let Err(e) = blueice_net::validate_url_scheme(&url) {
        return write_error(stream, reply_tab, request_id, e.to_string());
    }
    let navigation_rules = tabs.extension_navigation_block_rule_snapshot();
    if extension_navigation_rules_block_url(&navigation_rules, &url) {
        // The initial URL is evaluated synchronously before any gatekeeper
        // review or fetch. The same immutable snapshot follows the background
        // worker and is checked again before every redirect connection.
        return write_error(
            stream,
            reply_tab,
            request_id,
            format!("navigation blocked by a declarative extension rule: {url}"),
        );
    }

    if navigation.form.is_some() {
        tabs.get_mut(tab_id).expect("live tab").submission_pending = true;
        blueice_ipc::write_server_message_with_ids(
            stream,
            reply_tab,
            request_id,
            &ServerMessage::NavigationStarted {
                url: url.clone(),
                method: navigation.request.method().into(),
            },
        )?;
    }

    let tx = completion_tx.clone();
    let socket = gatekeeper_socket.to_path_buf();
    let translation = tabs.translation_config();
    thread::spawn(move || {
        let outcome = gatekeeper_client::check_and_fetch_request(
            tab_id,
            navigation,
            &socket,
            navigation_rules,
        );
        let translations = translate_cleared(&outcome, translation.as_ref());
        let _ = tx.send(Completion {
            tab_id,
            seq: this_seq,
            request_id,
            kind,
            outcome,
            translations,
        });
    });
    Ok(())
}

/// Writes the success reply for a gated navigation -- `Navigated` or
/// `TabOpened`, per `kind` -- followed by a fresh frame. Shared between
/// [`begin_gated_navigation`]'s synchronous built-in-page path and
/// [`apply_completion`]'s asynchronous cleared-navigation path so the
/// two can never disagree about what a successful navigation's reply
/// looks like.
#[allow(clippy::too_many_arguments)]
pub(super) fn reply_success<S: Write>(
    tabs: &mut TabManager,
    stream: &mut S,
    frame_dir: &Path,
    generation: &mut u64,
    reply_tab: Option<u64>,
    request_id: Option<u64>,
    kind: &PendingKind,
    tab_id: TabId,
    extension_events: Option<&mpsc::SyncSender<ExtensionRuntimeEvent>>,
) -> io::Result<()> {
    let page = tabs
        .get_mut(tab_id)
        .expect("a navigation reply requires a live tab");
    // Search UI is document-local. A history snapshot must not resurrect
    // highlights after the frontend has dismissed the previous find panel.
    page.clear_find();
    match kind {
        PendingKind::Navigate | PendingKind::History(_) | PendingKind::Reload => {
            reply_navigated(page, stream, reply_tab, request_id)?
        }
        PendingKind::OpenTab => blueice_ipc::write_server_message_with_ids(
            stream,
            reply_tab,
            request_id,
            &ServerMessage::TabOpened {
                tab_id: tab_id.as_u64(),
                url: page.url().map(str::to_string),
            },
        )?,
    }
    send_frame(page, stream, frame_dir, generation, reply_tab, request_id)?;
    if let Some(events) = extension_events {
        // Navigation events are advisory. A stalled extension has at most 16
        // queued notifications and never blocks the session's render owner.
        let _ = events.try_send(ExtensionRuntimeEvent::NavigationCommitted {
            tab_id: tab_id.as_u64(),
        });
    }
    Ok(())
}

/// Applies one background gated-navigation's [`Completion`], if it's
/// still current: discarded silently (no reply, no state change) if
/// `completion`'s tab has since closed, or if a *newer* navigation to
/// the same tab has since superseded it (`pending_nav_seq`'s entry for
/// that tab no longer matches the sequence number this completion was
/// tagged with) -- matching ordinary browser "a new navigation cancels
/// the in-flight one" behavior. The background thread that produced
/// `completion` never touched `Page`/`TabManager` state itself; this
/// (called only from the main loop) is the one place a gated
/// navigation's result actually lands.
#[allow(clippy::too_many_arguments)]
pub(super) fn apply_completion<S: Write>(
    tabs: &mut TabManager,
    stream: &mut S,
    frame_dir: &Path,
    generation: &mut u64,
    pending_nav_seq: &HashMap<TabId, u64>,
    completion: Completion,
    downloads_refresher: &mut DownloadsRefresher,
    page_script_runtime: &mut PageScriptRuntime<'_>,
    script_requests: Option<&ScriptRequestReceiver>,
    extension_events: Option<&mpsc::SyncSender<ExtensionRuntimeEvent>>,
) -> io::Result<bool> {
    let Completion {
        tab_id,
        seq,
        request_id,
        kind,
        outcome,
        translations,
    } = completion;
    if pending_nav_seq.get(&tab_id) != Some(&seq) {
        return Ok(false); // superseded by a later navigation to this tab
    }
    if tabs.get(tab_id).is_none() {
        return Ok(false); // the tab closed while this navigation was pending
    }
    tabs.get_mut(tab_id).expect("live tab").submission_pending = false;
    let reply_tab = Some(tab_id.as_u64());
    match outcome {
        NavOutcome::Cleared {
            clearance,
            final_url,
            html,
            status,
            content_type,
            request_url,
            redirects,
            navigation,
        } => {
            let response = blueice_ipc::extension::NetworkResponseInfo {
                method: navigation.request.method().to_string(),
                final_url: final_url.clone(),
                status,
                content_type,
            };
            let trace = blueice_ipc::extension::NetworkTraceInfo {
                request_url,
                redirects,
                response,
            };
            let committed = match &kind {
                PendingKind::History(direction) => tabs.apply_fetched_history_navigation(
                    tab_id,
                    *direction,
                    clearance,
                    &final_url,
                    &html,
                    translations.as_deref(),
                    trace,
                ),
                PendingKind::Reload => {
                    tabs.apply_fetched_reload(
                        tab_id,
                        clearance,
                        &final_url,
                        &html,
                        translations.as_deref(),
                        trace,
                    );
                    true
                }
                PendingKind::Navigate | PendingKind::OpenTab => {
                    tabs.apply_fetched_navigation(
                        tab_id,
                        clearance,
                        &final_url,
                        &html,
                        translations.as_deref(),
                        trace,
                    );
                    true
                }
            };
            if !committed {
                // The history entry disappeared before completion. This should
                // only be reachable if an internal caller changes the cursor
                // without advancing `pending_nav_seq`; fail safely rather than
                // applying the response to an unrelated document.
                return Ok(false);
            }
            tabs.remember_navigation(tab_id, navigation);
            if is_downloads_url(&final_url) {
                downloads_refresher.begin_visit(tab_id);
            }
            // A configured runner observes the loaded document before its
            // first success reply/frame. This preserves future DOM script
            // semantics while the default session has no runner at all.
            synchronize_page_script_runtime(page_script_runtime, tabs, script_requests)?;
            reply_success(
                tabs,
                stream,
                frame_dir,
                generation,
                reply_tab,
                request_id,
                &kind,
                tab_id,
                extension_events,
            )?;
            Ok(true)
        }
        NavOutcome::GatekeeperBlocked {
            reason,
            category,
            url,
        } => blueice_ipc::write_server_message_with_ids(
            stream,
            reply_tab,
            request_id,
            &ServerMessage::GatekeeperBlocked {
                reason,
                category,
                url,
            },
        )
        .map(|()| false),
        NavOutcome::ExtensionRuleBlocked { url } => write_error(
            stream,
            reply_tab,
            request_id,
            format!("navigation blocked by a declarative extension rule: {url}"),
        )
        .map(|()| false),
        NavOutcome::FetchFailed { message } => {
            write_error(stream, reply_tab, request_id, message).map(|()| false)
        }
    }
}

/// `OpenTab`'s handler: always creates the tab (there's no failure mode
/// for that itself); a requested navigation then goes through the same
/// gated two-phase path every other navigate-capable action does (see
/// module docs) -- so, unlike before gating existed, this function
/// itself no longer necessarily writes `OpenTab`'s own success/failure
/// reply: a blank tab (`url: None`) still gets an immediate
/// `TabOpened`, but a requested navigation's `TabOpened`/`Error`/
/// `GatekeeperBlocked` reply is deferred to [`apply_completion`] (or,
/// for a built-in page/invalid scheme, written synchronously inside
/// [`begin_gated_navigation`] itself).
#[allow(clippy::too_many_arguments)]
pub(super) fn handle_open_tab<S: Write>(
    tabs: &mut TabManager,
    stream: &mut S,
    frame_dir: &Path,
    generation: &mut u64,
    request_id: Option<u64>,
    url: Option<String>,
    window: crate::WindowId,
    pending_nav_seq: &mut HashMap<TabId, u64>,
    downloads_refresher: &mut DownloadsRefresher,
    completion_tx: &mpsc::Sender<Completion>,
    gatekeeper_socket: &Path,
    extension_events: Option<&mpsc::SyncSender<ExtensionRuntimeEvent>>,
) -> io::Result<()> {
    let new_id = match tabs.open_tab_in_window(window) {
        Ok(id) => id,
        Err(message) => return write_error(stream, None, request_id, message),
    };
    // Membership precedes review: a denied or failed new page still belongs
    // to its window and must remain visible/closable in native chrome.
    super::windows::write_window_state(
        tabs,
        stream,
        None,
        blueice_ipc::windows::WindowEvent::TabOpened {
            tab_id: new_id.as_u64(),
            window_id: window.as_u64(),
        },
    )?;
    let Some(url) = url else {
        return blueice_ipc::write_server_message_with_ids(
            stream,
            Some(new_id.as_u64()),
            request_id,
            &ServerMessage::TabOpened {
                tab_id: new_id.as_u64(),
                url: None,
            },
        );
    };
    begin_gated_navigation(
        tabs,
        stream,
        frame_dir,
        generation,
        Some(new_id.as_u64()),
        request_id,
        new_id,
        url,
        PendingKind::OpenTab,
        pending_nav_seq,
        downloads_refresher,
        completion_tx,
        gatekeeper_socket,
        extension_events,
    )
}

pub(super) struct PendingResubmission {
    pub(super) id: u64,
    pub(super) sequence: u64,
    pub(super) document: u64,
    pub(super) navigation: BrowserNavigation,
    pub(super) kind: PendingKind,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn request_resubmission<S: Write>(
    tabs: &TabManager,
    stream: &mut S,
    tab: TabId,
    request: Option<u64>,
    navigation: BrowserNavigation,
    kind: PendingKind,
    sequences: &mut HashMap<TabId, u64>,
    pending: &mut HashMap<TabId, PendingResubmission>,
) -> io::Result<()> {
    let mut bytes = [0; 8];
    getrandom::fill(&mut bytes).map_err(|error| io::Error::other(error.to_string()))?;
    let id = u64::from_le_bytes(bytes);
    let url = navigation.request.url().to_string();
    let sequence = supersede_pending_navigation(sequences, tab);
    let document = tabs.get(tab).expect("live tab").document_generation();
    pending.insert(
        tab,
        PendingResubmission {
            id,
            sequence,
            document,
            navigation,
            kind,
        },
    );
    blueice_ipc::write_server_message_with_ids(
        stream,
        Some(tab.as_u64()),
        request,
        &ServerMessage::FormResubmission {
            confirmation_id: id,
            url,
        },
    )
}
