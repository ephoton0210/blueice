// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

/// A summarize or organize task that finished on its background thread.
pub(super) struct AssistantCompletion {
    pub(super) reply_tab: Option<u64>,
    pub(super) request_id: Option<u64>,
    pub(super) kind: blueice_ipc::AssistantTaskKind,
    pub(super) context: Option<blueice_ipc::assistant_page::AssistantDocument>,
    pub(super) source_url: Option<String>,
    pub(super) request: Option<String>,
    pub(super) outcome: Result<String, String>,
}

/// Starts a summarize or organize task for `target`'s shown text.
///
/// Refusals that can be decided immediately (no assistant, unknown tab, an
/// invalid instruction, a page with no text) are answered with an `Error` at
/// once. Otherwise the assistant is asked on a background thread, so `core`
/// keeps serving every other tab and client, and the result arrives later as a
/// reply carrying the same `request_id` (see [`finish_assistant_task`]).
pub(super) fn begin_assistant_task<S: Write>(
    tabs: &mut TabManager,
    stream: &mut S,
    reply_tab: Option<u64>,
    request_id: Option<u64>,
    target: TabId,
    task: (
        ClientMessage,
        Option<blueice_ipc::assistant_page::AssistantDocument>,
    ),
    assistant_tx: &mpsc::Sender<AssistantCompletion>,
) -> io::Result<()> {
    let (task, context) = task;
    let Some(page) = tabs.get(target) else {
        return write_unknown_tab_error(stream, request_id, target);
    };
    let Some(socket) = tabs.assistant_socket() else {
        return write_error(
            stream,
            reply_tab,
            request_id,
            "the assistant is unavailable: blueice-core was started without one".to_string(),
        );
    };
    let (kind, instruction) = match task {
        ClientMessage::OrganizePage { instruction } => {
            (blueice_ipc::AssistantTaskKind::Organized, Some(instruction))
        }
        _ => (blueice_ipc::AssistantTaskKind::Summary, None),
    };
    let text = page.visible_text();
    if text.is_empty() {
        return write_error(
            stream,
            reply_tab,
            request_id,
            "this page has no text for the assistant to read".to_string(),
        );
    }
    // Validate exactly what will be sent, so an out-of-bounds instruction is an
    // immediate error rather than a failure reported only after a thread ran.
    let checked = match &instruction {
        Some(instruction) => blueice_ipc::assistant::AssistantRequest::Organize {
            request_id: 0,
            text: text.clone(),
            instruction: instruction.clone(),
        },
        None => blueice_ipc::assistant::AssistantRequest::Summarize {
            request_id: 0,
            text: text.clone(),
        },
    };
    if let Err(reason) = checked.validate() {
        return write_error(stream, reply_tab, request_id, reason);
    }
    let source_url = page.url().map(str::to_string);
    let tx = assistant_tx.clone();
    thread::spawn(move || {
        let deadline = crate::assistant_client::DEFAULT_TASK_DEADLINE;
        let outcome = match &instruction {
            Some(instruction) => {
                crate::assistant_client::organize_text(&socket, deadline, &text, instruction)
            }
            None => crate::assistant_client::summarize_text(&socket, deadline, &text),
        };
        let _ = tx.send(AssistantCompletion {
            reply_tab,
            request_id,
            kind,
            context,
            source_url,
            request: instruction,
            outcome,
        });
    });
    Ok(())
}

/// Answers the original request. Legacy tasks also update the shared
/// `about:assistant` panel; document-bound native results stay with their
/// requester and do not publish page text to other profiles' panel tabs.
pub(super) fn finish_assistant_task<S: Write>(
    tabs: &mut TabManager,
    stream: &mut S,
    frame_dir: &Path,
    generation: &mut u64,
    done: AssistantCompletion,
) -> io::Result<()> {
    let panel_kind = match done.kind {
        blueice_ipc::AssistantTaskKind::Summary => crate::assistant_page::PanelKind::Summary,
        blueice_ipc::AssistantTaskKind::Organized => crate::assistant_page::PanelKind::Organized,
    };
    let legacy = done.context.is_none();
    if legacy {
        tabs.assistant_panel().push(
            panel_kind,
            done.source_url,
            done.request,
            done.outcome.clone(),
        );
    }
    match done.outcome {
        Ok(text) => {
            let reply = match done.context {
                Some(context) => ServerMessage::AssistantPageResult {
                    context,
                    kind: done.kind,
                    text,
                },
                None => ServerMessage::AssistantResult {
                    kind: done.kind,
                    text,
                },
            };
            blueice_ipc::write_server_message_with_ids(
                stream,
                done.reply_tab,
                done.request_id,
                &reply,
            )?;
        }
        Err(reason) => write_error(stream, done.reply_tab, done.request_id, reason)?,
    }
    for id in if legacy {
        tabs.refresh_assistant_panels()
    } else {
        Vec::new()
    } {
        let page = tabs.get_mut(id).expect("refresh only returns live tabs");
        send_frame(page, stream, frame_dir, generation, Some(id.as_u64()), None)?;
    }
    Ok(())
}

/// Replies with the core-wide translation language and the addressed tab's
/// translation availability and shown/original state.
pub(super) fn write_translation_state<S: Write>(
    tabs: &TabManager,
    stream: &mut S,
    reply_tab: Option<u64>,
    request_id: Option<u64>,
    target: TabId,
) -> io::Result<()> {
    let page = tabs.get(target).expect("the caller checked the tab exists");
    blueice_ipc::write_server_message_with_ids(
        stream,
        reply_tab,
        request_id,
        &ServerMessage::TranslationState {
            language: tabs.translation_language().map(str::to_string),
            available: page.has_translation(),
            shown: page.translation_shown(),
        },
    )
}

/// Translates a *cleared* page on the navigation thread, so the session loop
/// never waits on the assistant. The gatekeeper has already reviewed the
/// original HTML by the time an outcome is `Cleared`; every other outcome, an
/// unset assistant, or any assistant failure yields `None` (the original page).
pub(super) fn translate_cleared(
    outcome: &NavOutcome,
    config: Option<&crate::assistant_client::AssistantConfig>,
) -> Option<Vec<String>> {
    match (outcome, config) {
        (NavOutcome::Cleared { html, .. }, Some(config)) => {
            crate::assistant_client::translate_html(config, html)
        }
        _ => None,
    }
}
