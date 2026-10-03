// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
#![cfg(unix)]
mod common;
use blueice_ipc::navigation_session::*;
use blueice_ipc::{ClientMessage, ServerMessage};
use common::*;

fn inspect(core: &mut Core) -> (SessionDocument, NavigationHistory) {
    core.send(&native_command(ClientMessage::NavigationSession(
        NavigationSessionAction::Inspect,
    )));
    match core.read() {
        ServerMessage::NavigationSessionState { context, history } => (context, history),
        other => panic!("{other:?}"),
    }
}
fn restore(core: &mut Core, context: SessionDocument, history: NavigationHistory) -> ServerMessage {
    core.send(&native_command(ClientMessage::NavigationSession(
        NavigationSessionAction::Restore { context, history },
    )));
    core.read()
}
#[test]
fn restored_history_preserves_cursor_forward_branch_and_zoom_without_refetching() {
    let url = web_server();
    let (gatekeeper, reviewed) = recording_gatekeeper();
    let mut core = Core::start(&gatekeeper, &[]);
    core.navigate(&format!("{url}/first"));
    core.navigate(&format!("{url}/second"));
    let (_, mut saved) = inspect(&mut core);
    saved.cursor -= 1;
    saved.zoom = 1.5;
    core.navigate(&format!("{url}/first"));
    let (document, _) = inspect(&mut core);
    let before = reviewed.lock().unwrap().len();
    assert!(
        matches!(restore(&mut core,document,saved.clone()),ServerMessage::NavigationSessionState { history, .. } if history == saved)
    );
    assert!(matches!(core.read(), ServerMessage::FrameReady { .. }));
    assert!(matches!(core.read(), ServerMessage::ViewportState(_)));
    assert_eq!(reviewed.lock().unwrap().len(), before);
    core.send(&ClientMessage::GetHistoryState);
    assert!(matches!(
        core.read(),
        ServerMessage::HistoryState {
            can_go_back: true,
            can_go_forward: true
        }
    ));
    core.send(&ClientMessage::GoForward);
    assert!(
        matches!(core.read(),ServerMessage::Navigated { url: actual } if actual == format!("{url}/second"))
    );
    assert!(matches!(core.read(), ServerMessage::FrameReady { .. }));
    assert!(matches!(core.read(), ServerMessage::ViewportState(_)));
    assert_eq!(reviewed.lock().unwrap().len(), before + 1);
}
#[test]
fn stale_sources_unreviewed_current_urls_and_invalid_archives_are_rejected() {
    let (gatekeeper, _) = recording_gatekeeper();
    let mut core = Core::start(&gatekeeper, &[]);
    core.navigate("about:credits");
    let (document, saved) = inspect(&mut core);
    for wrong in [
        SessionDocument {
            tab_id: 2,
            ..document.clone()
        },
        SessionDocument {
            frame_source: document.frame_source.wrapping_add(1),
            ..document.clone()
        },
        SessionDocument {
            document_generation: document.document_generation + 1,
            ..document.clone()
        },
    ] {
        assert!(matches!(
            restore(&mut core, wrong, saved.clone()),
            ServerMessage::Error { .. }
        ));
    }
    let mut wrong = saved.clone();
    wrong.entries[wrong.cursor].url = Some("https://unreviewed.test/".into());
    assert!(matches!(
        restore(&mut core, document.clone(), wrong),
        ServerMessage::Error { .. }
    ));
    for (url, was_post) in [
        ("about:credits", true),
        ("file:///private/test", false),
        ("https://name:password@example.test/", false),
    ] {
        let mut wrong = saved.clone();
        wrong.entries[wrong.cursor] = NavigationEntry {
            url: Some(url.into()),
            was_post,
        };
        assert!(matches!(
            restore(&mut core, document.clone(), wrong),
            ServerMessage::Error { .. }
        ));
    }
    let mut wrong = saved;
    wrong.cursor = usize::MAX;
    assert!(matches!(
        restore(&mut core, document, wrong),
        ServerMessage::Error { .. }
    ));
    assert_eq!(core.snapshot().url.as_deref(), Some("about:credits"));
}
#[test]
fn post_tombstones_never_fetch_or_downgrade_to_get() {
    let (gatekeeper, reviewed) = recording_gatekeeper();
    let mut core = Core::start(&gatekeeper, &[]);
    let (context, _) = inspect(&mut core);
    let saved = NavigationHistory {
        entries: vec![
            NavigationEntry {
                url: Some("https://example.test/form".into()),
                was_post: false,
            },
            NavigationEntry {
                url: Some("https://example.test/received".into()),
                was_post: true,
            },
        ],
        cursor: 1,
        zoom: 1.0,
    };
    assert!(
        matches!(restore(&mut core,context,saved.clone()),ServerMessage::NavigationSessionState { history, .. } if history==saved)
    );
    assert!(matches!(core.read(), ServerMessage::Navigated { .. }));
    assert!(matches!(core.read(), ServerMessage::FrameReady { .. }));
    assert!(matches!(core.read(), ServerMessage::ViewportState(_)));
    assert!(names(&core.snapshot())
        .join(" ")
        .contains("POST page could not be restored"));
    core.send(&ClientMessage::Reload);
    assert!(matches!(core.read(), ServerMessage::Error { .. }));
    assert!(reviewed.lock().unwrap().is_empty());
    let (_, actual) = inspect(&mut core);
    assert_eq!(actual, saved);
}
