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
    let mut print_jobs = printing::PrintJobs::default();
    let mut pending_resubmissions: HashMap<TabId, PendingResubmission> = HashMap::new();
    let (listing_tx, listing_rx) = mpsc::channel::<DownloadsListing>();
    let mut downloads_refresher = DownloadsRefresher::default();
    // Only the connection that published the native button can remove it.
    let mut extension_toolbar: Option<(u64, String, u64)> = None;
    let mut extension_popup: Option<(u64, ExtensionPopup, u64)> = None;

    let mut requests = requests;
    loop {
        print_jobs.expire(tabs);
        pending_resubmissions.retain(|tab, pending| {
            tabs.get(*tab)
                .is_some_and(|page| page.document_generation() == pending.document)
                && pending_nav_seq.get(tab) == Some(&pending.sequence)
        });
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
                let (msg, context_scope) = if let ClientMessage::BrowserContext(
                    blueice_ipc::browser_contexts::ContextAction::Command {
                        context_id,
                        message,
                    },
                ) = msg
                {
                    tabs.enable_native_contexts();
                    let context = crate::BrowserContextId::from_u64(context_id);
                    match contexts::scoped_command(tabs, context, tab_id, *message) {
                        Ok(message) => (message, Some(context)),
                        Err(message) => {
                            write_error(stream, tab_id, request_id, message)?;
                            continue;
                        }
                    }
                } else {
                    (msg, None)
                };
                // A tab can move while native callbacks are queued. Validate
                // its original window before dispatching any page operation.
                let msg =
                    if let ClientMessage::Window(blueice_ipc::windows::WindowAction::Command {
                        window_id,
                        message,
                    }) = msg
                    {
                        let target = tab_id
                            .map(TabId::from_u64)
                            .unwrap_or_else(|| tabs.default_tab());
                        if tabs.tab_window(target) != Some(crate::WindowId::from_u64(window_id))
                            || matches!(
                                *message,
                                ClientMessage::Window(_) | ClientMessage::BrowserContext(_)
                            )
                        {
                            write_error(
                                stream,
                                tab_id,
                                request_id,
                                "Native tab window is stale".into(),
                            )?;
                            continue;
                        }
                        *message
                    } else {
                        msg
                    };

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
                    ClientMessage::BrowserContext(action) => {
                        contexts::handle_context_action(tabs, stream, request_id, action)?
                    }
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
                    ClientMessage::Reload => {
                        let Some(page) = tabs.get(target) else {
                            write_unknown_tab_error(stream, request_id, target)?;
                            continue;
                        };
                        if page.post_expired {
                            write_error(
                                stream,
                                reply_tab,
                                request_id,
                                "Form data has expired; submit the form again".into(),
                            )?;
                            continue;
                        }
                        let Some(url) = page.url().map(str::to_string) else {
                            continue;
                        };
                        let navigation = page.last_navigation.clone().unwrap_or_else(|| url.into());
                        if navigation.request.method() == "POST" {
                            request_resubmission(
                                tabs,
                                stream,
                                target,
                                request_id,
                                navigation,
                                PendingKind::Reload,
                                &mut pending_nav_seq,
                                &mut pending_resubmissions,
                            )?;
                        } else {
                            begin_gated_request(
                                tabs,
                                stream,
                                frame_dir,
                                generation,
                                reply_tab,
                                request_id,
                                target,
                                navigation,
                                PendingKind::Reload,
                                &mut pending_nav_seq,
                                &mut downloads_refresher,
                                &completion_tx,
                                gatekeeper_socket,
                                extension_events,
                            )?;
                        }
                    }
                    ClientMessage::ConfirmFormResubmission {
                        confirmation_id,
                        accept,
                    } => {
                        let valid = pending_resubmissions.get(&target).is_some_and(|pending| {
                            pending.id == confirmation_id
                                && pending_nav_seq.get(&target) == Some(&pending.sequence)
                                && tabs.get(target).is_some_and(|page| {
                                    page.document_generation() == pending.document
                                })
                        });
                        if !valid {
                            write_error(
                                stream,
                                reply_tab,
                                request_id,
                                "Form resubmission confirmation is stale".into(),
                            )?;
                            continue;
                        }
                        let pending = pending_resubmissions
                            .remove(&target)
                            .expect("validated confirmation");
                        blueice_ipc::write_server_message_with_ids(
                            stream,
                            reply_tab,
                            request_id,
                            &ServerMessage::FormResubmissionResolved {
                                confirmation_id,
                                accepted: accept,
                            },
                        )?;
                        if accept {
                            begin_gated_request(
                                tabs,
                                stream,
                                frame_dir,
                                generation,
                                reply_tab,
                                request_id,
                                target,
                                pending.navigation,
                                pending.kind,
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
                        if let Some(HistoryDestination::Post { navigation, .. }) =
                            tabs.history_destination(target, direction)
                        {
                            if let Some(navigation) = navigation {
                                request_resubmission(
                                    tabs,
                                    stream,
                                    target,
                                    request_id,
                                    navigation,
                                    PendingKind::History(direction),
                                    &mut pending_nav_seq,
                                    &mut pending_resubmissions,
                                )?;
                            } else {
                                write_error(
                                    stream,
                                    reply_tab,
                                    request_id,
                                    "Form data has expired; submit the form again".into(),
                                )?;
                            }
                            continue;
                        }
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
                    ClientMessage::NavigationSession(action) => {
                        use blueice_ipc::navigation_session::{
                            NavigationSessionAction, SessionDocument,
                        };
                        let Some(page) = tabs.get(target) else {
                            write_unknown_tab_error(stream, request_id, target)?;
                            continue;
                        };
                        let source = blueice_ipc::shm::frame_source_id(frame_dir);
                        let restoring = !matches!(action, NavigationSessionAction::Inspect);
                        let mut replaced = false;
                        if let NavigationSessionAction::Restore { context, history } = action {
                            if context.tab_id != target.as_u64()
                                || context.frame_source != source
                                || context.document_generation != page.document_generation()
                            {
                                write_error(
                                    stream,
                                    reply_tab,
                                    request_id,
                                    "Session document is stale".into(),
                                )?;
                                continue;
                            }
                            match tabs.restore_navigation_session(target, history) {
                                Ok(changed) => {
                                    replaced = changed;
                                    supersede_pending_navigation(&mut pending_nav_seq, target);
                                }
                                Err(reason) => {
                                    write_error(stream, reply_tab, request_id, reason.into())?;
                                    continue;
                                }
                            }
                        }
                        match tabs.navigation_session(target) {
                            Ok(history) => {
                                let page = tabs.get_mut(target).expect("checked session tab");
                                blueice_ipc::write_server_message_with_ids(
                                    stream,
                                    reply_tab,
                                    request_id,
                                    &ServerMessage::NavigationSessionState {
                                        context: SessionDocument {
                                            tab_id: target.as_u64(),
                                            frame_source: source,
                                            document_generation: page.document_generation(),
                                        },
                                        history,
                                    },
                                )?;
                                if replaced {
                                    reply_navigated(page, stream, reply_tab, request_id)?;
                                }
                                // Inspection is read-only; restoration publishes its zoom or fixed POST notice.
                                if restoring {
                                    send_frame(
                                        page, stream, frame_dir, generation, reply_tab, request_id,
                                    )?;
                                }
                            }
                            Err(reason) => {
                                write_error(stream, reply_tab, request_id, reason.into())?
                            }
                        }
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
                            (task, None),
                            &assistant_tx,
                        )?;
                    }
                    ClientMessage::AssistantPage { context, action } => {
                        use blueice_ipc::assistant_page::AssistantPageAction;
                        let Some(page) = tabs.get_mut(target) else {
                            write_unknown_tab_error(stream, request_id, target)?;
                            continue;
                        };
                        if context.tab_id != target.as_u64()
                            || context.frame_source != blueice_ipc::shm::frame_source_id(frame_dir)
                            || context.document_generation != page.document_generation()
                        {
                            write_error(
                                stream,
                                reply_tab,
                                request_id,
                                "Assistant document is stale".into(),
                            )?;
                            continue;
                        }
                        let task = match action {
                            AssistantPageAction::Summarize => ClientMessage::SummarizePage,
                            AssistantPageAction::Organize { instruction } => {
                                ClientMessage::OrganizePage { instruction }
                            }
                            AssistantPageAction::ShowTranslation { shown } => {
                                let changed = page.set_translation_shown(shown);
                                write_translation_state(
                                    tabs, stream, reply_tab, request_id, target,
                                )?;
                                if changed {
                                    let page =
                                        tabs.get_mut(target).expect("checked immediately above");
                                    send_frame(
                                        page, stream, frame_dir, generation, reply_tab, request_id,
                                    )?;
                                }
                                continue;
                            }
                        };
                        begin_assistant_task(
                            tabs,
                            stream,
                            reply_tab,
                            request_id,
                            target,
                            (task, Some(context)),
                            &assistant_tx,
                        )?;
                    }
                    ClientMessage::SetViewport { viewport } => {
                        if tabs.get(target).is_none() {
                            write_unknown_tab_error(stream, request_id, target)?;
                            continue;
                        }
                        if let Err(message) = viewport.validate() {
                            write_error(stream, reply_tab, request_id, message)?;
                            continue;
                        }
                        let window = tabs.tab_window(target).expect("live viewport tab");
                        tabs.configure_window_viewport(window, viewport)
                            .expect("validated viewport");
                        let ids: Vec<_> = tabs.window_tabs(window).collect();
                        for id in ids {
                            send_frame(
                                tabs.get_mut(id).expect("live viewport tab"),
                                stream,
                                frame_dir,
                                generation,
                                Some(id.as_u64()),
                                request_id,
                            )?;
                        }
                    }
                    ClientMessage::SetDisplayPreferences { preferences } => {
                        if tabs.get(target).is_none() {
                            write_unknown_tab_error(stream, request_id, target)?;
                            continue;
                        }
                        tabs.set_display_preferences_all(preferences);
                        let ids: Vec<_> = tabs.ids().collect();
                        for id in ids {
                            send_frame(
                                tabs.get_mut(id).expect("live display tab"),
                                stream,
                                frame_dir,
                                generation,
                                Some(id.as_u64()),
                                request_id,
                            )?;
                        }
                    }
                    ClientMessage::GetDisplayPreferences => {
                        let Some(page) = tabs.get(target) else {
                            write_unknown_tab_error(stream, request_id, target)?;
                            continue;
                        };
                        blueice_ipc::write_server_message_with_ids(
                            stream,
                            reply_tab,
                            request_id,
                            &ServerMessage::DisplayPreferencesState(
                                page.display_preferences_state(
                                    blueice_ipc::shm::frame_source_id(frame_dir),
                                    target.as_u64(),
                                ),
                            ),
                        )?;
                    }
                    ClientMessage::SetPageZoom { zoom } => {
                        let Some(page) = tabs.get(target) else {
                            write_unknown_tab_error(stream, request_id, target)?;
                            continue;
                        };
                        if !zoom.is_finite()
                            || !(blueice_ipc::viewport::MIN_PAGE_ZOOM
                                ..=blueice_ipc::viewport::MAX_PAGE_ZOOM)
                                .contains(&zoom)
                        {
                            write_error(
                                stream,
                                reply_tab,
                                request_id,
                                "Page zoom must be between 25% and 500%".into(),
                            )?;
                            continue;
                        }
                        // Legacy sessions may have larger initial dimensions.
                        // Validate their display before opting into bounded raster.
                        let state = page.viewport_state(0, target.as_u64());
                        let display = blueice_ipc::viewport::DisplayViewport {
                            width: state.width,
                            height: state.height,
                            device_scale: state.device_scale,
                            backing_scale: state.backing_scale,
                        };
                        if let Err(message) = display.validate() {
                            write_error(stream, reply_tab, request_id, message)?;
                            continue;
                        }
                        tabs.set_page_zoom(target, zoom);
                        send_frame(
                            tabs.get_mut(target).expect("live zoom tab"),
                            stream,
                            frame_dir,
                            generation,
                            reply_tab,
                            request_id,
                        )?;
                    }
                    ClientMessage::GetViewportState => {
                        let Some(page) = tabs.get(target) else {
                            write_unknown_tab_error(stream, request_id, target)?;
                            continue;
                        };
                        blueice_ipc::write_server_message_with_ids(
                            stream,
                            reply_tab,
                            request_id,
                            &ServerMessage::ViewportState(page.viewport_state(
                                blueice_ipc::shm::frame_source_id(frame_dir),
                                target.as_u64(),
                            )),
                        )?;
                    }
                    ClientMessage::Resize { width, height } => {
                        if !(1..=4096).contains(&width) || !(1..=4096).contains(&height) {
                            write_error(
                                stream,
                                reply_tab,
                                request_id,
                                "Viewport dimensions must be between 1 and 4096".into(),
                            )?;
                            continue;
                        }
                        if tabs.get(target).is_none() {
                            write_unknown_tab_error(stream, request_id, target)?;
                            continue;
                        }
                        // Reflow all members of the addressed tab's window;
                        // other native windows keep their own environments.
                        let window = tabs.tab_window(target).expect("live resize tab");
                        tabs.resize_window(window, width as f64, height as f64);
                        let resized: Vec<TabId> = tabs.window_tabs(window).collect();
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
                    ClientMessage::Click { x, y } | ClientMessage::NativeClick { x, y, .. } => {
                        if let ClientMessage::NativeClick { context, .. } = msg {
                            let source = blueice_ipc::shm::frame_source_id(frame_dir);
                            let validation = tabs
                                .get(target)
                                .ok_or_else(|| "Unknown native click tab".to_string())
                                .and_then(|page| {
                                    page.validate_native_input_context(&context, source)
                                });
                            if let Err(message) = validation {
                                write_error(stream, reply_tab, request_id, message)?;
                                continue;
                            }
                            if !x.is_finite() || !y.is_finite() || x.abs() > 1e9 || y.abs() > 1e9 {
                                write_error(
                                    stream,
                                    reply_tab,
                                    request_id,
                                    "Invalid native click point".into(),
                                )?;
                                continue;
                            }
                        }
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
                            if let Some(href) =
                                href.map(BrowserNavigation::from).or(native_navigation)
                            {
                                begin_gated_request(
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
                    ClientMessage::FileInput(action) => {
                        use blueice_ipc::file_input::FileInputAction;
                        let source = shm::frame_source_id(frame_dir);
                        let mut dispatched = false;
                        let result = match action {
                            FileInputAction::Prepare {
                                frame_source,
                                document_generation,
                                node_id,
                            } => {
                                let available = frame_source == source
                                    && tabs.get(target).is_some_and(|page| {
                                        page.file_input_state(node_id, source, document_generation)
                                            .is_ok()
                                    });
                                if !available {
                                    Err("File control is stale or unavailable".into())
                                } else {
                                    dispatched = true;
                                    match dispatch_click_before_default(
                                        &mut page_script_runtime.javascript_executor,
                                        tabs,
                                        target,
                                        NodeId::from_u64(node_id),
                                        requests.script,
                                    ) {
                                        Ok(Some(true)) => {
                                            Err("File selection was cancelled by the page".into())
                                        }
                                        Err(_) => {
                                            Err("Page activation listener unavailable".into())
                                        }
                                        Ok(_) => {
                                            let page = tabs
                                                .get_mut(target)
                                                .expect("activation retains tab");
                                            page.file_input_state(
                                                node_id,
                                                source,
                                                document_generation,
                                            )
                                            .inspect(
                                                |_| {
                                                    page.native_focus_at(Some(NodeId::from_u64(
                                                        node_id,
                                                    )));
                                                },
                                            )
                                        }
                                    }
                                }
                            }
                            FileInputAction::Set { context, .. }
                                if context.tab_id != target.as_u64() =>
                            {
                                Err("File selection belongs to another tab".into())
                            }
                            FileInputAction::Set { context, files } => tabs
                                .get_mut(target)
                                .ok_or_else(|| "Unknown file tab".to_string())
                                .and_then(|page| page.set_file_input(&context, source, files)),
                        };
                        match result {
                            Ok(mut state) => {
                                state.context.tab_id = target.as_u64();
                                let page = tabs.get_mut(target).expect("file reply retains tab");
                                send_frame(
                                    page, stream, frame_dir, generation, reply_tab, request_id,
                                )?;
                                blueice_ipc::write_server_message_with_ids(
                                    stream,
                                    reply_tab,
                                    request_id,
                                    &ServerMessage::FileInputState(state),
                                )?;
                            }
                            Err(message) => {
                                if dispatched {
                                    if let Some(page) = tabs.get_mut(target) {
                                        send_frame(
                                            page, stream, frame_dir, generation, reply_tab,
                                            request_id,
                                        )?;
                                    }
                                }
                                write_error(stream, reply_tab, request_id, message)?;
                            }
                        }
                    }
                    ClientMessage::AccessibilityAcknowledge { delivery } => {
                        let Some(page) = tabs.get_mut(target) else {
                            write_unknown_tab_error(stream, request_id, target)?;
                            continue;
                        };
                        let source = blueice_ipc::shm::frame_source_id(frame_dir);
                        match page.accessibility_acknowledge(&delivery, source) {
                            Ok(reply) => blueice_ipc::write_server_message_with_ids(
                                stream,
                                reply_tab,
                                request_id,
                                &ServerMessage::AccessibilityAcknowledged(reply),
                            )?,
                            Err(message) => write_error(stream, reply_tab, request_id, message)?,
                        }
                    }
                    ClientMessage::AccessibilityReveal { mut context } => {
                        let Some(page) = tabs.get_mut(target) else {
                            write_unknown_tab_error(stream, request_id, target)?;
                            continue;
                        };
                        let source = blueice_ipc::shm::frame_source_id(frame_dir);
                        match page.accessibility_reveal(&context, source) {
                            Ok(bounds) => {
                                send_frame(
                                    page, stream, frame_dir, generation, reply_tab, request_id,
                                )?;
                                context.frame_generation = page.frame_generation();
                                blueice_ipc::write_server_message_with_ids(
                                    stream,
                                    reply_tab,
                                    request_id,
                                    &ServerMessage::AccessibilityRevealed(
                                        blueice_ipc::accessibility::AccessibilityRevealReply {
                                            context,
                                            bounds,
                                        },
                                    ),
                                )?;
                            }
                            Err(message) => write_error(stream, reply_tab, request_id, message)?,
                        }
                    }
                    ClientMessage::AccessibilityText { context, action } => {
                        let source = blueice_ipc::shm::frame_source_id(frame_dir);
                        let Some(page) = tabs.get_mut(target) else {
                            write_unknown_tab_error(stream, request_id, target)?;
                            continue;
                        };
                        match page.accessibility_text(&context, source, action) {
                            Ok((changed, result)) => {
                                if changed {
                                    send_frame(
                                        page, stream, frame_dir, generation, reply_tab, request_id,
                                    )?;
                                }
                                let reply = page.accessibility_text_reply(context, result);
                                blueice_ipc::write_server_message_with_ids(
                                    stream,
                                    reply_tab,
                                    request_id,
                                    &ServerMessage::AccessibilityTextState(reply),
                                )?;
                            }
                            Err(message) => write_error(stream, reply_tab, request_id, message)?,
                        }
                    }
                    ClientMessage::Print(action) => {
                        match print_jobs.handle(tabs, target, frame_dir, action) {
                            Ok(state) => blueice_ipc::write_server_message_with_ids(
                                stream,
                                reply_tab,
                                request_id,
                                &ServerMessage::PrintState(state),
                            )?,
                            Err(message) => write_error(stream, reply_tab, request_id, message)?,
                        }
                    }
                    ClientMessage::GetContextMenu {
                        tab_id,
                        frame_source,
                        frame_generation,
                        x,
                        y,
                    } => {
                        let source = blueice_ipc::shm::frame_source_id(frame_dir);
                        let Some(page) = tabs.get_mut(target) else {
                            write_unknown_tab_error(stream, request_id, target)?;
                            continue;
                        };
                        if tab_id != target.as_u64()
                            || frame_source != source
                            || frame_generation != page.frame_generation()
                            || !page.validate_menu_point(x, y)
                        {
                            write_error(
                                stream,
                                reply_tab,
                                request_id,
                                "Context menu frame is stale or outside the viewport".into(),
                            )?;
                            continue;
                        }
                        if page.prepare_context_menu(x, y) {
                            send_frame(page, stream, frame_dir, generation, reply_tab, request_id)?;
                        }
                        blueice_ipc::write_server_message_with_ids(
                            stream,
                            reply_tab,
                            request_id,
                            &ServerMessage::ContextMenu(page.context_menu_state(
                                source,
                                target.as_u64(),
                                x,
                                y,
                            )),
                        )?;
                    }
                    ClientMessage::ContextMenuLink { context, action } => {
                        use blueice_ipc::context_menu::ContextMenuLinkAction;
                        let Some(page) = tabs.get(target) else {
                            write_unknown_tab_error(stream, request_id, target)?;
                            continue;
                        };
                        if context.tab_id != target.as_u64()
                            || context.frame_source != blueice_ipc::shm::frame_source_id(frame_dir)
                            || context.document_generation != page.document_generation()
                            || context.frame_generation != page.frame_generation()
                            || !page.validate_menu_point(context.x, context.y)
                        {
                            write_error(
                                stream,
                                reply_tab,
                                request_id,
                                "Context menu is stale".into(),
                            )?;
                            continue;
                        }
                        let Some(url) = page.menu_link(context.x, context.y) else {
                            write_error(
                                stream,
                                reply_tab,
                                request_id,
                                "Context menu has no supported link".into(),
                            )?;
                            continue;
                        };
                        match action {
                            ContextMenuLinkAction::Copy => {
                                blueice_ipc::write_server_message_with_ids(
                                    stream,
                                    reply_tab,
                                    request_id,
                                    &ServerMessage::ContextMenuLink { context, url },
                                )?
                            }
                            ContextMenuLinkAction::Open => begin_gated_navigation(
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
                            )?,
                            ContextMenuLinkAction::OpenInNewTab => {
                                let window = tabs.tab_window(target).expect("live context tab");
                                handle_open_tab(
                                    tabs,
                                    stream,
                                    frame_dir,
                                    generation,
                                    request_id,
                                    Some(url),
                                    window,
                                    &mut pending_nav_seq,
                                    &mut downloads_refresher,
                                    &completion_tx,
                                    gatekeeper_socket,
                                    extension_events,
                                )?
                            }
                        }
                    }
                    ClientMessage::Find {
                        tab_id,
                        frame_source,
                        document_generation,
                        action,
                    } => {
                        let source = blueice_ipc::shm::frame_source_id(frame_dir);
                        let Some(page) = tabs.get_mut(target) else {
                            write_unknown_tab_error(stream, request_id, target)?;
                            continue;
                        };
                        if tab_id != target.as_u64()
                            || frame_source != source
                            || document_generation != page.document_generation()
                        {
                            write_error(
                                stream,
                                reply_tab,
                                request_id,
                                "Find context is stale".into(),
                            )?;
                            continue;
                        }
                        match page.find_action(action) {
                            Ok(()) => {
                                send_frame(
                                    page, stream, frame_dir, generation, reply_tab, request_id,
                                )?;
                                let state = page.find_state(source, target.as_u64());
                                blueice_ipc::write_server_message_with_ids(
                                    stream,
                                    reply_tab,
                                    request_id,
                                    &ServerMessage::FindState(state),
                                )?;
                            }
                            Err(message) => write_error(stream, reply_tab, request_id, message)?,
                        }
                    }
                    ClientMessage::GetFindState => match tabs.get(target) {
                        Some(page) => blueice_ipc::write_server_message_with_ids(
                            stream,
                            reply_tab,
                            request_id,
                            &ServerMessage::FindState(page.find_state(
                                blueice_ipc::shm::frame_source_id(frame_dir),
                                target.as_u64(),
                            )),
                        )?,
                        None => write_unknown_tab_error(stream, request_id, target)?,
                    },
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
                        let implicit_form = activation
                            .is_none()
                            .then(|| page.native_implicit_form(&action))
                            .flatten();
                        let result = if page.document_generation() != document || prevented {
                            Ok(true)
                        } else {
                            page.native_text_input(&context, source, action)
                                .and_then(|changed| {
                                    if let Some(node) = activation {
                                        if page.apply_gatekeeper_settings_control(node).is_none() {
                                            navigation = href
                                                .map(BrowserNavigation::from)
                                                .or(page.native_control_activation(node)?);
                                        }
                                        Ok(true)
                                    } else {
                                        if let Some(form) = implicit_form {
                                            navigation = Some(
                                                page.prepare_native_form_submission(form, None)?,
                                            );
                                        }
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
                                    begin_gated_request(
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
                            let native_navigation = match tabs
                                .get_mut(target)
                                .expect("checked immediately above")
                                .native_control_activation(node)
                            {
                                Ok(url) => url,
                                Err(message) => {
                                    write_error(stream, reply_tab, request_id, message)?;
                                    continue;
                                }
                            };
                            if let Some(href) =
                                href.map(BrowserNavigation::from).or(native_navigation)
                            {
                                begin_gated_request(
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
                            if prevented.is_some()
                                || tabs
                                    .get(target)
                                    .is_some_and(|page| page.native_focusable(node))
                            {
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
                    ClientMessage::Window(action) => {
                        use blueice_ipc::windows::WindowAction;
                        tabs.enable_native_windows();
                        if let WindowAction::OpenTab { window_id, url } = action {
                            handle_open_tab(
                                tabs,
                                stream,
                                frame_dir,
                                generation,
                                request_id,
                                url,
                                crate::WindowId::from_u64(window_id),
                                &mut pending_nav_seq,
                                &mut downloads_refresher,
                                &completion_tx,
                                gatekeeper_socket,
                                extension_events,
                            )?;
                        } else {
                            windows::handle_window_action(
                                tabs, stream, frame_dir, generation, request_id, target, action,
                            )?;
                        }
                    }
                    ClientMessage::OpenTab { url } => handle_open_tab(
                        tabs,
                        stream,
                        frame_dir,
                        generation,
                        request_id,
                        url,
                        crate::WindowId::from_u64(1),
                        &mut pending_nav_seq,
                        &mut downloads_refresher,
                        &completion_tx,
                        gatekeeper_socket,
                        extension_events,
                    )?,
                    ClientMessage::CloseTab => {
                        pending_resubmissions.remove(&target);
                        let window = tabs.tab_window(target);
                        if tabs.close_tab(target) {
                            if let Some(window) = window {
                                windows::write_window_state(
                                    tabs,
                                    stream,
                                    None,
                                    blueice_ipc::windows::WindowEvent::TabClosed {
                                        tab_id: target.as_u64(),
                                        window_id: window.as_u64(),
                                    },
                                )?;
                            }
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
                            .filter(|&id| {
                                context_scope
                                    .is_none_or(|context| tabs.tab_context(id) == Some(context))
                            })
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
                        let context =
                            context_scope.unwrap_or_else(|| crate::BrowserContextId::from_u64(1));
                        let id = match tabs.create_group_in_context(context, name, color) {
                            Ok(id) => id,
                            Err(message) => {
                                write_error(stream, None, request_id, message)?;
                                continue;
                            }
                        };
                        contexts::write_context_state(
                            tabs,
                            stream,
                            None,
                            blueice_ipc::browser_contexts::ContextEvent::Snapshot,
                        )?;
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
                        if let Err(message) = tabs.assign_tab_group(target, group_id) {
                            write_error(stream, reply_tab, request_id, message)?;
                            continue;
                        }
                        windows::write_window_state(
                            tabs,
                            stream,
                            None,
                            blueice_ipc::windows::WindowEvent::Snapshot,
                        )?;
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
                        contexts::write_context_state(
                            tabs,
                            stream,
                            None,
                            blueice_ipc::browser_contexts::ContextEvent::Snapshot,
                        )?;
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
                        contexts::write_context_state(
                            tabs,
                            stream,
                            None,
                            blueice_ipc::browser_contexts::ContextEvent::Snapshot,
                        )?;
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
                        contexts::write_context_state(
                            tabs,
                            stream,
                            None,
                            blueice_ipc::browser_contexts::ContextEvent::Snapshot,
                        )?;
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
                        windows::write_window_state(
                            tabs,
                            stream,
                            None,
                            blueice_ipc::windows::WindowEvent::Snapshot,
                        )?;
                    }
                    ClientMessage::ListTabGroups => {
                        let groups = tabs
                            .context_groups(
                                context_scope
                                    .unwrap_or_else(|| crate::BrowserContextId::from_u64(1)),
                            )
                            .map(tab_group_summary)
                            .collect();
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
