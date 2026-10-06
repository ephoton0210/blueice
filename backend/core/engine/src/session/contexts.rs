// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use crate::{BrowserContextId, WindowId};
use blueice_ipc::browser_contexts::{ContextAction, ContextEvent, ContextState, ContextSummary};
use blueice_ipc::windows::{WindowAction, WindowEvent};

pub(super) fn write_context_state<S: Write>(
    tabs: &TabManager,
    stream: &mut S,
    request: Option<u64>,
    event: ContextEvent,
) -> io::Result<()> {
    if !tabs.native_contexts_enabled() {
        return Ok(());
    }
    let contexts = tabs
        .contexts()
        .map(|context| ContextSummary {
            id: context.id().as_u64(),
            name: context.name().into(),
            windows: tabs
                .context_windows(context.id())
                .map(WindowId::as_u64)
                .collect(),
            groups: tabs
                .context_groups(context.id())
                .map(tab_group_summary)
                .collect(),
        })
        .collect();
    blueice_ipc::write_server_message_with_id(
        stream,
        request,
        &ServerMessage::BrowserContextState(ContextState { contexts, event }),
    )
}

pub(super) fn handle_context_action<S: Write>(
    tabs: &mut TabManager,
    stream: &mut S,
    request: Option<u64>,
    action: ContextAction,
) -> io::Result<()> {
    tabs.enable_native_contexts();
    let result = match action {
        ContextAction::List => Ok(ContextEvent::Snapshot),
        ContextAction::Create { name } => {
            tabs.create_context(name).map(|id| ContextEvent::Created {
                context_id: id.as_u64(),
            })
        }
        ContextAction::Rename { context_id, name } => tabs
            .rename_context(BrowserContextId::from_u64(context_id), name)
            .map(|()| ContextEvent::Renamed { context_id }),
        ContextAction::Close { context_id } => match tabs
            .close_context(BrowserContextId::from_u64(context_id))
        {
            Ok(closed) => {
                for id in closed.tabs {
                    blueice_ipc::write_server_message_with_ids(
                        stream,
                        Some(id.as_u64()),
                        None,
                        &ServerMessage::TabClosed {
                            tab_id: id.as_u64(),
                        },
                    )?;
                }
                for id in closed.groups {
                    blueice_ipc::write_server_message_with_id(
                        stream,
                        None,
                        &ServerMessage::TabGroupClosed {
                            group_id: id.as_u64(),
                        },
                    )?;
                }
                write_context_state(tabs, stream, request, ContextEvent::Closed { context_id })?;
                return windows::write_window_state(tabs, stream, None, WindowEvent::Snapshot);
            }
            Err(message) => Err(message),
        },
        ContextAction::Command { .. } => unreachable!("scoped command already unwrapped"),
    };
    match result {
        Ok(event) => write_context_state(tabs, stream, request, event),
        Err(message) => write_error(stream, None, request, message),
    }
}

/// Validate before dispatch. A context wrapper is an ownership/lifetime check,
/// never a controller lease, URL review, or private permission grant.
pub(super) fn scoped_command(
    tabs: &TabManager,
    context: BrowserContextId,
    tab: Option<u64>,
    message: ClientMessage,
) -> Result<ClientMessage, String> {
    if tabs.context(context).is_none() {
        return Err("Browser context is stale".into());
    }
    let target = tab
        .map(TabId::from_u64)
        .unwrap_or_else(|| tabs.default_tab());
    let owns_tab = || tabs.tab_context(target) == Some(context);
    let owns_group = |id| {
        tabs.group(GroupId::from_u64(id))
            .is_some_and(|group| group.context_id() == context)
    };
    let owns_window = |id| tabs.window_context(WindowId::from_u64(id)) == Some(context);
    let valid = match &message {
        ClientMessage::CreateTabGroup { .. }
        | ClientMessage::ListTabGroups
        | ClientMessage::ListTabs => true,
        ClientMessage::RenameTabGroup { group_id, .. }
        | ClientMessage::SetTabGroupColor { group_id, .. }
        | ClientMessage::SetTabGroupCollapsed { group_id, .. }
        | ClientMessage::CloseTabGroup { group_id } => owns_group(*group_id),
        ClientMessage::SetTabGroup { group_id } => owns_tab() && group_id.is_none_or(owns_group),
        ClientMessage::Window(action) => match action {
            WindowAction::List | WindowAction::Create { .. } => true,
            WindowAction::CreateInContext { context_id, .. } => *context_id == context.as_u64(),
            WindowAction::Close { window_id }
            | WindowAction::Resize { window_id, .. }
            | WindowAction::OpenTab { window_id, .. } => owns_window(*window_id),
            WindowAction::MoveTab { window_id } => owns_tab() && owns_window(*window_id),
            WindowAction::Command { window_id, message } => {
                owns_tab()
                    && owns_window(*window_id)
                    && !matches!(
                        **message,
                        ClientMessage::Window(_) | ClientMessage::BrowserContext(_)
                    )
                    && scoped_command(tabs, context, tab, *message.clone()).is_ok()
            }
        },
        ClientMessage::Navigate { .. }
        | ClientMessage::Reload
        | ClientMessage::GoBack
        | ClientMessage::GoForward
        | ClientMessage::ConfirmFormResubmission { .. }
        | ClientMessage::GetHistoryState
        | ClientMessage::AssistantPage { .. }
        | ClientMessage::NavigationSession(_)
        | ClientMessage::GetTranslationState
        | ClientMessage::SetTranslationLanguage { .. }
        | ClientMessage::Find { .. }
        | ClientMessage::GetFindState
        | ClientMessage::Print(_)
        | ClientMessage::AccessibilityAcknowledge { .. }
        | ClientMessage::AccessibilityReveal { .. }
        | ClientMessage::AccessibilityText { .. }
        | ClientMessage::GetContextMenu { .. }
        | ClientMessage::ContextMenuLink { .. }
        | ClientMessage::Resize { .. }
        | ClientMessage::SetViewport { .. }
        | ClientMessage::SetPageZoom { .. }
        | ClientMessage::GetViewportState
        | ClientMessage::SetDisplayPreferences { .. }
        | ClientMessage::GetDisplayPreferences
        | ClientMessage::Click { .. }
        | ClientMessage::NativeClick { .. }
        | ClientMessage::Hover { .. }
        | ClientMessage::Scroll { .. }
        | ClientMessage::InsertText { .. }
        | ClientMessage::DeleteBackward
        | ClientMessage::FileInput(_)
        | ClientMessage::GetTextInputState
        | ClientMessage::TextInput { .. }
        | ClientMessage::GetRepresentation
        | ClientMessage::ActOn { .. }
        | ClientMessage::Highlight { .. }
        | ClientMessage::GetDom
        | ClientMessage::CloseTab => owns_tab(),
        ClientMessage::OpenTab { .. } => context.as_u64() == 1,
        _ => false,
    };
    if !valid {
        return Err("Command belongs to another browser context or is not context-scoped".into());
    }
    if let ClientMessage::Window(WindowAction::Create { viewport }) = message {
        return Ok(ClientMessage::Window(WindowAction::CreateInContext {
            context_id: context.as_u64(),
            viewport,
        }));
    }
    Ok(message)
}
