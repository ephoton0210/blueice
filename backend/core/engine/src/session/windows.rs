// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use crate::WindowId;
use blueice_ipc::windows::{WindowAction, WindowEvent, WindowState, WindowSummary};

pub(super) fn write_window_state<S: Write>(
    tabs: &TabManager,
    stream: &mut S,
    request: Option<u64>,
    event: WindowEvent,
) -> io::Result<()> {
    if !tabs.native_windows_enabled() {
        return Ok(());
    }
    let windows = tabs
        .window_ids()
        .map(|id| WindowSummary {
            id: id.as_u64(),
            viewport: tabs.window_viewport(id).expect("live window"),
            tabs: tabs
                .window_tabs(id)
                .map(|tab| TabSummary {
                    id: tab.as_u64(),
                    url: tabs.get(tab).and_then(Page::url).map(str::to_string),
                    group_id: tabs.tab_group(tab).map(GroupId::as_u64),
                })
                .collect(),
        })
        .collect();
    blueice_ipc::write_server_message_with_id(
        stream,
        request,
        &ServerMessage::WindowState(WindowState { windows, event }),
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn handle_window_action<S: Write>(
    tabs: &mut TabManager,
    stream: &mut S,
    directory: &Path,
    generation: &mut u64,
    request: Option<u64>,
    tab: TabId,
    action: WindowAction,
) -> io::Result<()> {
    let result = match action {
        WindowAction::List => Ok((WindowEvent::Snapshot, vec![])),
        WindowAction::Create { viewport } => tabs.create_window(viewport).map(|id| {
            (
                WindowEvent::Created {
                    window_id: id.as_u64(),
                },
                vec![],
            )
        }),
        WindowAction::Resize {
            window_id,
            viewport,
        } => {
            let id = WindowId::from_u64(window_id);
            tabs.configure_window_viewport(id, viewport).map(|()| {
                (
                    WindowEvent::Resized { window_id },
                    tabs.window_tabs(id).collect(),
                )
            })
        }
        WindowAction::MoveTab { window_id } => {
            let source = tabs.tab_window(tab);
            tabs.move_tab_to_window(tab, WindowId::from_u64(window_id))
                .map(|()| {
                    (
                        WindowEvent::TabMoved {
                            tab_id: tab.as_u64(),
                            from_window: source.expect("validated source").as_u64(),
                            to_window: window_id,
                        },
                        vec![tab],
                    )
                })
        }
        WindowAction::Close { window_id } => {
            tabs.close_window(WindowId::from_u64(window_id))
                .map(|closed| {
                    // Ordinary observers retain their existing close notifications.
                    (WindowEvent::Closed { window_id }, closed)
                })
        }
        WindowAction::OpenTab { .. } | WindowAction::Command { .. } => {
            unreachable!("navigation action routed by session")
        }
    };
    let (event, ids) = match result {
        Ok(result) => result,
        Err(message) => return write_error(stream, None, request, message),
    };
    if matches!(event, WindowEvent::Closed { .. }) {
        for id in ids {
            blueice_ipc::write_server_message_with_ids(
                stream,
                Some(id.as_u64()),
                None,
                &ServerMessage::TabClosed {
                    tab_id: id.as_u64(),
                },
            )?;
        }
        return write_window_state(tabs, stream, request, event);
    }
    write_window_state(tabs, stream, request, event)?;
    for id in ids {
        send_frame(
            tabs.get_mut(id).expect("live window member"),
            stream,
            directory,
            generation,
            Some(id.as_u64()),
            request,
        )?;
    }
    Ok(())
}
