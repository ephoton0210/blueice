// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

/// Shared session implementation for the observer-only direct host and the
/// explicitly enabled inline runners. [`PageScriptRuntime`] rejects multiple
/// hosts at once: independent hosts would allocate separate page realms for
/// one page.
pub(super) fn run_session_with_script_runtime<S: Read + Write + ReadTimeout>(
    tabs: &mut TabManager,
    stream: &mut S,
    frame_dir: &Path,
    generation: &mut u64,
    gatekeeper_socket: &Path,
    requests: CoreSessionRequests<'_>,
    mut page_script_runtime: PageScriptRuntime<'_>,
) -> io::Result<()> {
    if [
        page_script_runtime.direct_page_host.is_some(),
        page_script_runtime.inline_page_executor.is_some(),
        page_script_runtime.javascript_executor.is_some(),
    ]
    .into_iter()
    .filter(|enabled| *enabled)
    .count()
        > 1
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "page-script runtime owners cannot share one session",
        ));
    }
    // Best-effort: on at least one real platform, setting a read
    // timeout on a Unix domain socket whose peer has *already*
    // disconnected (a client that connects and drops the connection
    // before this thread even starts) can itself fail with `EINVAL`,
    // even though nothing is actually wrong with the connection from
    // this session's own perspective -- the very next read below
    // simply returns immediately with a real disconnect error either
    // way. Propagating that failure via `?` here would turn a client
    // that never sent anything at all into a spurious session error,
    // instead of the ordinary clean-disconnect outcome every other
    // never-sent-anything case already gets. A failure here just means
    // the loop falls back to plain blocking reads (no completion-
    // draining poll tick between messages) rather than ending the
    // session outright.
    let _ = stream.set_read_timeout(Some(POLL_INTERVAL));
    if !perform_handshake(stream)? {
        return Ok(());
    }
    synchronize_page_script_runtime(&mut page_script_runtime, tabs, requests.script)?;
    let extension_requests = requests.extension;
    let extension_events = requests.extension_events;

    let (completion_tx, completion_rx) = mpsc::channel::<Completion>();
    let (assistant_tx, assistant_rx) = mpsc::channel::<AssistantCompletion>();
    let mut pending_nav_seq: HashMap<TabId, u64> = HashMap::new();
    let (listing_tx, listing_rx) = mpsc::channel::<DownloadsListing>();
    let mut downloads_refresher = DownloadsRefresher::default();
    // Only the connection that published the native button can remove it.
    let mut extension_toolbar: Option<(u64, String, u64)> = None;
    let mut extension_popup: Option<(u64, ExtensionPopup, u64)> = None;

    let mut requests = requests;
    loop {
        let incoming = blueice_ipc::read_client_message_with_ids(stream);
        if !matches!(&incoming, Err(error) if !is_timeout(error)) {
            prune_stale_extension_effects(
                tabs,
                stream,
                &mut extension_toolbar,
                &mut extension_popup,
            )?;
        }
        match incoming {
            Ok((tab_id, request_id, msg)) => {
                let target = tab_id
                    .map(TabId::from_u64)
                    .unwrap_or_else(|| tabs.default_tab());
                // Every per-tab reply below echoes `Some(target.as_u64())`,
                // the *resolved* tab -- not the raw (possibly `None`, if
                // the request left it defaulted) `tab_id` the request
                // carried. Echoing the ambiguous original back would
                // defeat the whole point of this field: a client watching
                // a shared, multi-tab, broadcast connection (`blueice-
                // launcher`'s broker) needs every reply to self-disclose
                // which concrete tab it's about, including one produced
                // by a request that left it implicit.
                let reply_tab = Some(target.as_u64());
                match msg {
                    ClientMessage::Hello { protocol_version } => {
                        reply_hello(stream, request_id, protocol_version)?
                    }
                    ClientMessage::Navigate { url } => {
                        if tabs.get(target).is_none() {
                            write_unknown_tab_error(stream, request_id, target)?;
                        } else {
                            begin_gated_navigation(
                                tabs,
                                stream,
                                frame_dir,
                                generation,
                                reply_tab,
                                request_id,
                                target,
                                url,
                                PendingKind::Navigate,
                                &mut pending_nav_seq,
                                &mut downloads_refresher,
                                &completion_tx,
                                gatekeeper_socket,
                                extension_events,
                            )?;
                        }
                    }
                    history_message @ (ClientMessage::GoBack | ClientMessage::GoForward) => {
                        if tabs.get(target).is_none() {
                            write_unknown_tab_error(stream, request_id, target)?;
                            continue;
                        }
                        let direction = if matches!(history_message, ClientMessage::GoBack) {
                            HistoryDirection::Back
                        } else {
                            HistoryDirection::Forward
                        };
                        begin_history_navigation(
                            tabs,
                            stream,
                            frame_dir,
                            generation,
                            reply_tab,
                            request_id,
                            target,
                            direction,
                            &mut pending_nav_seq,
                            &mut downloads_refresher,
                            &completion_tx,
                            gatekeeper_socket,
                            extension_events,
                        )?;
                    }
                    ClientMessage::GetHistoryState => {
                        let Some(can_go_back) = tabs.can_go_back(target) else {
                            write_unknown_tab_error(stream, request_id, target)?;
                            continue;
                        };
                        let can_go_forward = tabs
                            .can_go_forward(target)
                            .expect("a live tab has a history state");
                        blueice_ipc::write_server_message_with_ids(
                            stream,
                            reply_tab,
                            request_id,
                            &ServerMessage::HistoryState {
                                can_go_back,
                                can_go_forward,
                            },
                        )?;
                    }
                    ClientMessage::SetTranslationLanguage { target_language } => {
                        if tabs.get(target).is_none() {
                            write_unknown_tab_error(stream, request_id, target)?;
                            continue;
                        }
                        if let Some(tag) = &target_language {
                            if let Err(reason) = blueice_ipc::assistant::validate_language_tag(tag)
                            {
                                write_error(stream, reply_tab, request_id, reason)?;
                                continue;
                            }
                        }
                        if !tabs.set_translation_language(target_language) {
                            write_error(
                                stream,
                                reply_tab,
                                request_id,
                                "translation is unavailable: blueice-core was started without an assistant"
                                    .to_string(),
                            )?;
                            continue;
                        }
                        write_translation_state(tabs, stream, reply_tab, request_id, target)?;
                    }
                    ClientMessage::ShowTranslation { shown } => {
                        let Some(page) = tabs.get_mut(target) else {
                            write_unknown_tab_error(stream, request_id, target)?;
                            continue;
                        };
                        let changed = page.set_translation_shown(shown);
                        write_translation_state(tabs, stream, reply_tab, request_id, target)?;
                        if changed {
                            let page = tabs.get_mut(target).expect("checked immediately above");
                            send_frame(page, stream, frame_dir, generation, reply_tab, request_id)?;
                        }
                    }
                    ClientMessage::GetTranslationState => {
                        if tabs.get(target).is_none() {
                            write_unknown_tab_error(stream, request_id, target)?;
                            continue;
                        }
                        write_translation_state(tabs, stream, reply_tab, request_id, target)?;
                    }
                    task @ (ClientMessage::SummarizePage | ClientMessage::OrganizePage { .. }) => {
                        begin_assistant_task(
                            tabs,
                            stream,
                            reply_tab,
                            request_id,
                            target,
                            task,
                            &assistant_tx,
                        )?;
                    }
                    ClientMessage::Resize { width, height } => {
                        if tabs.get(target).is_none() {
                            write_unknown_tab_error(stream, request_id, target)?;
                            continue;
                        }
                        // A native frontend has one physical content viewport,
                        // not one viewport per selected tab. Eagerly reflowing
                        // every Page here keeps a background tab display-ready
                        // when that frontend later selects it; there is still
                        // no core "active tab" state.
                        tabs.resize_all(width as f64, height as f64);
                        let resized: Vec<TabId> = tabs.ids().collect();
                        for id in resized {
                            let page = tabs.get_mut(id).expect("ids only yields live tabs");
                            send_frame(
                                page,
                                stream,
                                frame_dir,
                                generation,
                                Some(id.as_u64()),
                                (id == target).then_some(request_id).flatten(),
                            )?;
                        }
                    }
                    ClientMessage::Click { x, y } => {
                        let Some((node, event_target, href)) = tabs.get(target).map(|page| {
                            (
                                page.click_target(x, y),
                                page.click_event_target(x, y)
                                    .map(|node| (node, page.document_generation())),
                                page.click(x, y),
                            )
                        }) else {
                            write_unknown_tab_error(stream, request_id, target)?;
                            continue;
                        };
                        // A BlueJS click listener (when one is configured)
                        // runs before any default action and may prevent it.
                        let mut prevented = None;
                        if let Some((event_node, clicked_generation)) = event_target {
                            match dispatch_click_before_default(
                                &mut page_script_runtime.javascript_executor,
                                tabs,
                                target,
                                event_node,
                                requests.script,
                            ) {
                                Err(_) => {
                                    write_error(
                                        stream,
                                        reply_tab,
                                        request_id,
                                        "page click listener unavailable".to_string(),
                                    )?;
                                    continue;
                                }
                                Ok(result) => prevented = result,
                            }
                            let page = tabs.get_mut(target).expect("hit-tested tab is live");
                            if page.document_generation() != clicked_generation {
                                send_frame(
                                    page, stream, frame_dir, generation, reply_tab, request_id,
                                )?;
                                continue;
                            }
                        }
                        let mut focus_changed = false;
                        if prevented != Some(true) {
                            focus_changed = tabs
                                .get_mut(target)
                                .expect("checked immediately above")
                                .focus_native_editor_at(node);
                            if node.is_some_and(|node| {
                                tabs.get_mut(target)
                                    .expect("checked immediately above")
                                    .apply_gatekeeper_settings_control(node)
                                    .is_some()
                            }) {
                                let page = tabs
                                    .get_mut(target)
                                    .expect("a settings control cannot close a core-owned tab");
                                send_frame(
                                    page, stream, frame_dir, generation, reply_tab, request_id,
                                )?;
                                continue;
                            }
                            let native_navigation = if let Some(node) = node {
                                match tabs
                                    .get_mut(target)
                                    .expect("checked above")
                                    .native_control_activation(node)
                                {
                                    Ok(url) => url,
                                    Err(message) => {
                                        write_error(stream, reply_tab, request_id, message)?;
                                        continue;
                                    }
                                }
                            } else {
                                None
                            };
                            if let Some(href) = href.or(native_navigation) {
                                begin_gated_navigation(
                                    tabs,
                                    stream,
                                    frame_dir,
                                    generation,
                                    reply_tab,
                                    request_id,
                                    target,
                                    href,
                                    PendingKind::Navigate,
                                    &mut pending_nav_seq,
                                    &mut downloads_refresher,
                                    &completion_tx,
                                    gatekeeper_socket,
                                    extension_events,
                                )?;
                                continue;
                            }
                        }
                        if prevented.is_some() || focus_changed || node.is_some() {
                            let page = tabs
                                .get_mut(target)
                                .expect("a click cannot close a core-owned tab");
                            send_frame(page, stream, frame_dir, generation, reply_tab, request_id)?;
                        }
                    }
                    ClientMessage::Scroll { delta_y } => match tabs.get_mut(target) {
                        Some(page) => {
                            page.scroll_by(delta_y);
                            send_frame(page, stream, frame_dir, generation, reply_tab, request_id)?;
                        }
                        None => write_unknown_tab_error(stream, request_id, target)?,
                    },
                    ClientMessage::InsertText { text } => {
                        let changed = match tabs.get_mut(target) {
                            Some(page) => page.insert_focused_text(&text),
                            None => {
                                write_unknown_tab_error(stream, request_id, target)?;
                                continue;
                            }
                        };
                        if changed {
                            let page = tabs
                                .get_mut(target)
                                .expect("a text edit cannot close a core-owned tab");
                            send_frame(page, stream, frame_dir, generation, reply_tab, request_id)?;
                        }
                    }
                    ClientMessage::DeleteBackward => {
                        let changed = match tabs.get_mut(target) {
                            Some(page) => page.delete_focused_text_backward(),
                            None => {
                                write_unknown_tab_error(stream, request_id, target)?;
                                continue;
                            }
                        };
                        if changed {
                            let page = tabs
                                .get_mut(target)
                                .expect("a text edit cannot close a core-owned tab");
                            send_frame(page, stream, frame_dir, generation, reply_tab, request_id)?;
                        }
                    }
                    ClientMessage::GetTextInputState => match tabs.get(target) {
                        Some(page) => {
                            let mut state = page.native_text_input_state(
                                blueice_ipc::shm::frame_source_id(frame_dir),
                            );
                            state.tab_id = target.as_u64();
                            blueice_ipc::write_server_message_with_ids(
                                stream,
                                reply_tab,
                                request_id,
                                &ServerMessage::TextInputState(state),
                            )?;
                        }
                        None => write_unknown_tab_error(stream, request_id, target)?,
                    },
                    ClientMessage::TextInput { context, action } => {
                        let source = blueice_ipc::shm::frame_source_id(frame_dir);
                        let Some(page) = tabs.get(target) else {
                            write_unknown_tab_error(stream, request_id, target)?;
                            continue;
                        };
                        if let Err(message) = page.validate_native_input_context(&context, source) {
                            write_error(stream, reply_tab, request_id, message)?;
                            continue;
                        }
                        let activation = page.native_key_activation(&action);
                        let document = page.document_generation();
                        let href = activation.and_then(|node| page.native_activation_link(node));
                        let prevented = if let Some(node) = activation {
                            match dispatch_click_before_default(
                                &mut page_script_runtime.javascript_executor,
                                tabs,
                                target,
                                node,
                                requests.script,
                            ) {
                                Ok(prevented) => prevented == Some(true),
                                Err(_) => {
                                    write_error(
                                        stream,
                                        reply_tab,
                                        request_id,
                                        "page activation listener unavailable".into(),
                                    )?;
                                    continue;
                                }
                            }
                        } else {
                            false
                        };
                        let page = tabs
                            .get_mut(target)
                            .expect("activation retains its tab owner");
                        let mut navigation = None;
                        let result = if page.document_generation() != document || prevented {
                            Ok(true)
                        } else {
                            page.native_text_input(&context, source, action)
                                .and_then(|changed| {
                                    if let Some(node) = activation {
                                        if page.apply_gatekeeper_settings_control(node).is_none() {
                                            navigation =
                                                href.or(page.native_control_activation(node)?);
                                        }
                                        Ok(true)
                                    } else {
                                        Ok(changed)
                                    }
                                })
                        };
                        match result {
                            Ok(changed) => {
                                if changed {
                                    send_frame(
                                        page, stream, frame_dir, generation, reply_tab, request_id,
                                    )?;
                                }
                                let mut state = page.native_text_input_state(source);
                                state.tab_id = target.as_u64();
                                blueice_ipc::write_server_message_with_ids(
                                    stream,
                                    reply_tab,
                                    request_id,
                                    &ServerMessage::TextInputState(state),
                                )?;
                                if let Some(url) = navigation {
                                    begin_gated_navigation(
                                        tabs,
                                        stream,
                                        frame_dir,
                                        generation,
                                        reply_tab,
                                        request_id,
                                        target,
                                        url,
                                        PendingKind::Navigate,
                                        &mut pending_nav_seq,
                                        &mut downloads_refresher,
                                        &completion_tx,
                                        gatekeeper_socket,
                                        extension_events,
                                    )?;
                                }
                            }
                            Err(message) => write_error(stream, reply_tab, request_id, message)?,
                        }
                    }
                    ClientMessage::Hover { x, y } => {
                        if let Some(page) = tabs.get_mut(target) {
                            page.hover_at(x, y);
                        } else {
                            write_unknown_tab_error(stream, request_id, target)?;
                        }
                    }
                    ClientMessage::GetRepresentation => match tabs.get_mut(target) {
                        Some(page) => {
                            let mut snapshot =
                                page.snapshot(page.frame_generation(), target.as_u64());
                            snapshot.frame_source = blueice_ipc::shm::frame_source_id(frame_dir);
                            blueice_ipc::write_server_message_with_ids(
                                stream,
                                reply_tab,
                                request_id,
                                &ServerMessage::Representation(snapshot),
                            )?;
                        }
                        None => write_unknown_tab_error(stream, request_id, target)?,
                    },
                    ClientMessage::GetDom => match tabs.get_mut(target) {
                        Some(page) => blueice_ipc::write_server_message_with_ids(
                            stream,
                            reply_tab,
                            request_id,
                            &ServerMessage::Dom(page.dom_dump()),
                        )?,
                        None => write_unknown_tab_error(stream, request_id, target)?,
                    },
                    ClientMessage::GetBlueTsScriptReports => match tabs.get(target) {
                        Some(_) => {
                            if let Some(executor) =
                                page_script_runtime.inline_page_executor.as_deref_mut()
                            {
                                blueice_ipc::write_server_message_with_ids(
                                    stream,
                                    reply_tab,
                                    request_id,
                                    &ServerMessage::BlueTsScriptReports(inline_execution_reports(
                                        executor.drain_reports_for_tab(target),
                                    )),
                                )?;
                            } else if let Some(executor) =
                                page_script_runtime.javascript_executor.as_deref_mut()
                            {
                                if executor.supports_blue_ts_page_execution() {
                                    blueice_ipc::write_server_message_with_ids(
                                        stream,
                                        reply_tab,
                                        request_id,
                                        &ServerMessage::BlueTsScriptReports(
                                            child_blue_ts_execution_reports(
                                                executor.drain_blue_ts_reports_for_tab(target),
                                            ),
                                        ),
                                    )?;
                                } else {
                                    write_error(
                                        stream,
                                        reply_tab,
                                        request_id,
                                        "inline BlueTS execution is not enabled".to_string(),
                                    )?;
                                }
                            } else {
                                write_error(
                                    stream,
                                    reply_tab,
                                    request_id,
                                    "inline BlueTS execution is not enabled".to_string(),
                                )?;
                            }
                        }
                        None => write_unknown_tab_error(stream, request_id, target)?,
                    },
                    ClientMessage::GetBlueJsScriptReports => match tabs.get(target) {
                        Some(_) => match page_script_runtime.javascript_executor.as_deref_mut() {
                            Some(executor) => blueice_ipc::write_server_message_with_ids(
                                stream,
                                reply_tab,
                                request_id,
                                &ServerMessage::BlueJsScriptReports(
                                    inline_javascript_execution_reports(
                                        executor.drain_reports_for_tab(target),
                                    ),
                                ),
                            )?,
                            None => write_error(
                                stream,
                                reply_tab,
                                request_id,
                                "inline JavaScript execution is not enabled".to_string(),
                            )?,
                        },
                        None => write_unknown_tab_error(stream, request_id, target)?,
                    },
                    ClientMessage::ActOn { id, action } => {
                        if tabs.get(target).is_none() {
                            write_unknown_tab_error(stream, request_id, target)?;
                            continue;
                        }
                        let node = NodeId::from_u64(id);
                        let is_click = matches!(&action, NodeAction::Click);
                        let clicked_generation = if is_click {
                            tabs.get(target).map(Page::document_generation)
                        } else {
                            None
                        };
                        // A click listener (when configured) runs before the
                        // default action and may prevent it.
                        let dispatch = if is_click {
                            tabs.get(target)
                                .and_then(|page| page.event_element_target(node))
                                .map(|event_node| {
                                    dispatch_click_before_default(
                                        &mut page_script_runtime.javascript_executor,
                                        tabs,
                                        target,
                                        event_node,
                                        requests.script,
                                    )
                                })
                        } else {
                            None
                        };
                        if matches!(dispatch, Some(Err(_))) {
                            write_error(
                                stream,
                                reply_tab,
                                request_id,
                                "page click listener unavailable".to_string(),
                            )?;
                            continue;
                        }
                        let prevented = dispatch.and_then(Result::ok).flatten();
                        let page = tabs
                            .get_mut(target)
                            .expect("a click listener cannot close a core-owned tab");
                        if is_click
                            && (clicked_generation != Some(page.document_generation())
                                || prevented == Some(true))
                        {
                            send_frame(page, stream, frame_dir, generation, reply_tab, request_id)?;
                            continue;
                        }
                        let href = page.act(node, action);
                        if is_click {
                            if tabs
                                .get_mut(target)
                                .expect("checked immediately above")
                                .apply_gatekeeper_settings_control(node)
                                .is_some()
                            {
                                let page = tabs
                                    .get_mut(target)
                                    .expect("a settings control cannot close a core-owned tab");
                                send_frame(
                                    page, stream, frame_dir, generation, reply_tab, request_id,
                                )?;
                                continue;
                            }
                            if let Some(href) = href {
                                begin_gated_navigation(
                                    tabs,
                                    stream,
                                    frame_dir,
                                    generation,
                                    reply_tab,
                                    request_id,
                                    target,
                                    href,
                                    PendingKind::Navigate,
                                    &mut pending_nav_seq,
                                    &mut downloads_refresher,
                                    &completion_tx,
                                    gatekeeper_socket,
                                    extension_events,
                                )?;
                                continue;
                            }
                            if prevented.is_some() {
                                let page = tabs
                                    .get_mut(target)
                                    .expect("a click listener cannot close a core-owned tab");
                                send_frame(
                                    page, stream, frame_dir, generation, reply_tab, request_id,
                                )?;
                            }
                        } else {
                            send_frame(page, stream, frame_dir, generation, reply_tab, request_id)?;
                        }
                    }
                    ClientMessage::Highlight { id } => match tabs.get_mut(target) {
                        Some(page) => {
                            page.set_highlight(id.map(NodeId::from_u64));
                            send_frame(page, stream, frame_dir, generation, reply_tab, request_id)?;
                        }
                        None => write_unknown_tab_error(stream, request_id, target)?,
                    },
                    ClientMessage::OpenTab { url } => handle_open_tab(
                        tabs,
                        stream,
                        frame_dir,
                        generation,
                        request_id,
                        url,
                        &mut pending_nav_seq,
                        &mut downloads_refresher,
                        &completion_tx,
                        gatekeeper_socket,
                        extension_events,
                    )?,
                    ClientMessage::CloseTab => {
                        if tabs.close_tab(target) {
                            blueice_ipc::write_server_message_with_ids(
                                stream,
                                reply_tab,
                                request_id,
                                &ServerMessage::TabClosed {
                                    tab_id: target.as_u64(),
                                },
                            )?;
                            if extension_popup
                                .as_ref()
                                .is_some_and(|(_, popup, _)| popup.tab_id == target.as_u64())
                            {
                                extension_popup = None;
                                blueice_ipc::write_server_message_with_ids(
                                    stream,
                                    None,
                                    None,
                                    &ServerMessage::ExtensionPopup { popup: None },
                                )?;
                            }
                        } else {
                            write_unknown_tab_error(stream, request_id, target)?;
                        }
                    }
                    ClientMessage::ListTabs => {
                        let summaries: Vec<TabSummary> = tabs
                            .ids()
                            .map(|id| TabSummary {
                                id: id.as_u64(),
                                url: tabs.get(id).and_then(Page::url).map(str::to_string),
                                group_id: tabs.tab_group(id).map(GroupId::as_u64),
                            })
                            .collect();
                        blueice_ipc::write_server_message_with_id(
                            stream,
                            request_id,
                            &ServerMessage::Tabs(summaries),
                        )?;
                    }
                    ClientMessage::CreateTabGroup { name, color } => {
                        let name = match validate_group_name(name) {
                            Ok(name) => name,
                            Err(message) => {
                                write_error(stream, None, request_id, message)?;
                                continue;
                            }
                        };
                        let color = match validate_group_color(color) {
                            Ok(color) => color,
                            Err(message) => {
                                write_error(stream, None, request_id, message)?;
                                continue;
                            }
                        };
                        let id = tabs.create_group(name, color);
                        let summary = tab_group_summary(
                            tabs.group(id).expect("a just-created group is live"),
                        );
                        blueice_ipc::write_server_message_with_id(
                            stream,
                            request_id,
                            &ServerMessage::TabGroupCreated(summary),
                        )?;
                    }
                    ClientMessage::SetTabGroup { group_id } => {
                        if tabs.get(target).is_none() {
                            write_unknown_tab_error(stream, request_id, target)?;
                            continue;
                        }
                        let group_id = group_id.map(GroupId::from_u64);
                        if let Some(group_id) = group_id {
                            if tabs.group(group_id).is_none() {
                                write_error(
                                    stream,
                                    reply_tab,
                                    request_id,
                                    format!("unknown tab group {}", group_id.as_u64()),
                                )?;
                                continue;
                            }
                        }
                        tabs.set_tab_group(target, group_id);
                        blueice_ipc::write_server_message_with_ids(
                            stream,
                            reply_tab,
                            request_id,
                            &ServerMessage::TabGroupAssigned {
                                tab_id: target.as_u64(),
                                group_id: group_id.map(GroupId::as_u64),
                            },
                        )?;
                    }
                    ClientMessage::RenameTabGroup { group_id, name } => {
                        let id = GroupId::from_u64(group_id);
                        if tabs.group(id).is_none() {
                            write_error(
                                stream,
                                None,
                                request_id,
                                format!("unknown tab group {group_id}"),
                            )?;
                            continue;
                        }
                        let name = match validate_group_name(name) {
                            Ok(name) => name,
                            Err(message) => {
                                write_error(stream, None, request_id, message)?;
                                continue;
                            }
                        };
                        tabs.rename_group(id, name);
                        let summary =
                            tab_group_summary(tabs.group(id).expect("group remains live"));
                        blueice_ipc::write_server_message_with_id(
                            stream,
                            request_id,
                            &ServerMessage::TabGroupUpdated(summary),
                        )?;
                    }
                    ClientMessage::SetTabGroupColor { group_id, color } => {
                        let id = GroupId::from_u64(group_id);
                        if tabs.group(id).is_none() {
                            write_error(
                                stream,
                                None,
                                request_id,
                                format!("unknown tab group {group_id}"),
                            )?;
                            continue;
                        }
                        let color = match validate_group_color(color) {
                            Ok(color) => color,
                            Err(message) => {
                                write_error(stream, None, request_id, message)?;
                                continue;
                            }
                        };
                        tabs.set_group_color(id, color);
                        let summary =
                            tab_group_summary(tabs.group(id).expect("group remains live"));
                        blueice_ipc::write_server_message_with_id(
                            stream,
                            request_id,
                            &ServerMessage::TabGroupUpdated(summary),
                        )?;
                    }
                    ClientMessage::SetTabGroupCollapsed {
                        group_id,
                        collapsed,
                    } => {
                        let id = GroupId::from_u64(group_id);
                        if tabs.group(id).is_none() {
                            write_error(
                                stream,
                                None,
                                request_id,
                                format!("unknown tab group {group_id}"),
                            )?;
                            continue;
                        }
                        tabs.set_group_collapsed(id, collapsed);
                        let summary =
                            tab_group_summary(tabs.group(id).expect("group remains live"));
                        blueice_ipc::write_server_message_with_id(
                            stream,
                            request_id,
                            &ServerMessage::TabGroupUpdated(summary),
                        )?;
                    }
                    ClientMessage::CloseTabGroup { group_id } => {
                        let id = GroupId::from_u64(group_id);
                        if !tabs.close_group(id) {
                            write_error(
                                stream,
                                None,
                                request_id,
                                format!("unknown tab group {group_id}"),
                            )?;
                            continue;
                        }
                        blueice_ipc::write_server_message_with_id(
                            stream,
                            request_id,
                            &ServerMessage::TabGroupClosed { group_id },
                        )?;
                    }
                    ClientMessage::ListTabGroups => {
                        let groups = tabs.groups().map(tab_group_summary).collect();
                        blueice_ipc::write_server_message_with_id(
                            stream,
                            request_id,
                            &ServerMessage::TabGroups(groups),
                        )?;
                    }
                    ClientMessage::GetExtensionToolbar => {
                        blueice_ipc::write_server_message_with_ids(
                            stream,
                            None,
                            request_id,
                            &ServerMessage::ExtensionToolbar {
                                label: extension_toolbar
                                    .as_ref()
                                    .map(|(_, label, _)| label.clone()),
                            },
                        )?;
                    }
                    ClientMessage::ActivateExtensionToolbar => {
                        #[allow(clippy::unnecessary_unwrap)]
                        let result = if extension_toolbar.is_none() {
                            Err("no extension toolbar button is installed".to_string())
                        } else if tabs.get(target).is_none() {
                            Err(format!("unknown tab {}", target.as_u64()))
                        } else if let Some(events) = extension_events {
                            events
                                .try_send(ExtensionRuntimeEvent::ToolbarActivated {
                                    tab_id: target.as_u64(),
                                    grant_generation: extension_toolbar.as_ref().unwrap().2,
                                })
                                .map_err(|_| "extension event queue is unavailable".to_string())
                        } else {
                            Err("the extension runtime is unavailable".to_string())
                        };
                        if let Err(message) = result {
                            blueice_ipc::write_server_message_with_ids(
                                stream,
                                reply_tab,
                                request_id,
                                &ServerMessage::Error { message },
                            )?;
                        }
                    }
                    ClientMessage::GetExtensionPopup => {
                        blueice_ipc::write_server_message_with_ids(
                            stream,
                            None,
                            request_id,
                            &ServerMessage::ExtensionPopup {
                                popup: extension_popup.as_ref().map(|(_, popup, _)| popup.clone()),
                            },
                        )?;
                    }
                    ClientMessage::DismissExtensionPopup => {
                        if extension_popup
                            .as_ref()
                            .is_some_and(|(_, popup, _)| popup.tab_id == target.as_u64())
                        {
                            extension_popup = None;
                            blueice_ipc::write_server_message_with_ids(
                                stream,
                                None,
                                None,
                                &ServerMessage::ExtensionPopup { popup: None },
                            )?;
                        }
                    }
                    ClientMessage::ActivateExtensionPopupAction { popup_id } => {
                        let valid_popup = extension_popup.as_ref().is_some_and(|(_, popup, _)| {
                            popup.id != 0
                                && popup.id == popup_id
                                && popup.tab_id == target.as_u64()
                                && popup.action_label.is_some()
                        });
                        let result = if !valid_popup || tabs.get(target).is_none() {
                            Err("no matching live extension popup action".to_string())
                        } else if let Some(events) = extension_events {
                            events
                                .try_send(ExtensionRuntimeEvent::PopupActionActivated {
                                    tab_id: target.as_u64(),
                                    grant_generation: extension_popup.as_ref().unwrap().2,
                                })
                                .map_err(|_| "extension event queue is unavailable".to_string())
                        } else {
                            Err("the extension runtime is unavailable".to_string())
                        };
                        match result {
                            Ok(()) => {
                                extension_popup = None;
                                blueice_ipc::write_server_message_with_ids(
                                    stream,
                                    None,
                                    None,
                                    &ServerMessage::ExtensionPopup { popup: None },
                                )?;
                            }
                            Err(message) => blueice_ipc::write_server_message_with_ids(
                                stream,
                                reply_tab,
                                request_id,
                                &ServerMessage::Error { message },
                            )?,
                        }
                    }
                    // Chrome commands (window show/hide) operate on
                    // `frontend`'s own window, not on anything `core`
                    // owns -- see module docs.
                    ClientMessage::Chrome(_) => {}
                    ClientMessage::Shutdown => return Ok(()),
                    // Forward-compatibility fallback (plan §3): a variant
                    // this build doesn't recognize is ignored rather than
                    // treated as a protocol violation.
                    ClientMessage::Unknown => {}
                }
            }
            Err(e) if is_timeout(&e) => {} // no message yet -- fall through to drain completions
            Err(_) => return Ok(()),       // client disconnected without an explicit Shutdown
        }

        while let Ok(done) = assistant_rx.try_recv() {
            finish_assistant_task(tabs, stream, frame_dir, generation, done)?;
        }

        let mut synchronized_after_completion = false;
        while let Ok(completion) = completion_rx.try_recv() {
            synchronized_after_completion |= apply_completion(
                tabs,
                stream,
                frame_dir,
                generation,
                &pending_nav_seq,
                completion,
                &mut downloads_refresher,
                &mut page_script_runtime,
                requests.script,
                extension_events,
            )?;
        }

        // Keep any open `about:downloads` tab current, off this thread.
        downloads_refresher.tick(tabs, &listing_tx, Instant::now());
        while let Ok(listing) = listing_rx.try_recv() {
            downloads_refresher.apply(tabs, stream, frame_dir, generation, listing)?;
        }

        if let Some(extension_requests) = extension_requests {
            while let Ok(request) = extension_requests.try_recv() {
                handle_extension_page_request(
                    tabs,
                    stream,
                    frame_dir,
                    generation,
                    &mut extension_toolbar,
                    &mut extension_popup,
                    request,
                )?;
            }
        }
        if let Some(script_requests) = requests.script {
            script_requests.dispatch_pending(tabs);
        }
        if let Some(debugger_requests) = requests.debugger {
            let debugger_executor = page_script_runtime.javascript_executor.as_deref_mut();
            debugger_requests.dispatch_pending(tabs, debugger_executor);
        }
        if let Some(compiler_requests) = requests.compiler.as_mut() {
            compiler_requests
                .service
                .dispatch_pending(compiler_requests.receiver);
        }
        // `apply_completion` already synchronized the just-admitted document
        // before publishing its navigation reply. Do not immediately run a
        // second lifecycle turn here: that would make a newly admitted
        // root-entry debugger program execute before its peer can even ask
        // for the opaque location needed to arm it.
        if !synchronized_after_completion {
            synchronize_page_script_runtime(&mut page_script_runtime, tabs, requests.script)?;
        }
    }
}
