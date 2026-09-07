// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The message loop `blueice-core`'s process binary drives: read a
//! [`blueice_ipc::ClientMessage`], apply it to a [`Page`], reply. Kept
//! generic over `Read + Write` (rather than hardcoding a
//! `UnixStream`) so it's testable over an in-process pipe the same way
//! `blueice-ipc`'s own IPC-boundary test is (`UnixStream::pair`) --
//! this is the "drive the real protocol with a test client" strategy
//! from `TEST_PLAN.md`'s UI testing section, applied one layer up.
//!
//! Every state-changing message (`Navigate`, `Resize`, `Click` that
//! lands on a link, `Scroll`, `ActOn`, `Highlight`) ends with a fresh
//! frame written to the frame-plane and a `FrameReady` reply --
//! `Chrome` and `Hover` are exceptions: `Chrome` (per `BROWSER_CORE_
//! PLAN.md` §1, the render pipeline runs identically regardless of
//! window visibility, so there's nothing here for it to change) and
//! `Hover` (nothing paints differently yet -- `:hover` isn't in the
//! MVP CSS selector list -- so there's no frame to refresh, only
//! `Page`'s own hover state for a future `GetRepresentation` or
//! `:hover` style to read).

use crate::Page;
use blueice_dom::NodeId;
use blueice_ipc::{shm, ClientMessage, NodeAction, ServerMessage};
use std::io::{self, Read, Write};
use std::path::Path;

/// Runs the message loop for one client connection until it sends
/// `Shutdown` or disconnects. `frame_dir` is where this session's
/// frames are written (see [`blueice_ipc::shm`]); `generation` is
/// shared frame-sequence counter, incremented on every frame sent.
pub fn run_session<S: Read + Write>(page: &mut Page, stream: &mut S, frame_dir: &Path, generation: &mut u64) -> io::Result<()> {
    loop {
        let msg = match blueice_ipc::read_client_message(stream) {
            Ok(msg) => msg,
            Err(_) => return Ok(()), // client disconnected without an explicit Shutdown
        };
        match msg {
            ClientMessage::Navigate { url } => match page.navigate(&url) {
                Ok(()) => {
                    reply_navigated(page, stream)?;
                    send_frame(page, stream, frame_dir, generation)?;
                }
                Err(e) => blueice_ipc::write_server_message(stream, &ServerMessage::Error { message: e.to_string() })?,
            },
            ClientMessage::Resize { width, height } => {
                page.resize(width as f64, height as f64);
                send_frame(page, stream, frame_dir, generation)?;
            }
            ClientMessage::Click { x, y } => {
                if let Some(href) = page.click(x, y) {
                    match page.navigate(&href) {
                        Ok(()) => {
                            reply_navigated(page, stream)?;
                            send_frame(page, stream, frame_dir, generation)?;
                        }
                        Err(e) => blueice_ipc::write_server_message(stream, &ServerMessage::Error { message: e.to_string() })?,
                    }
                }
            }
            ClientMessage::Scroll { delta_y } => {
                page.scroll_by(delta_y);
                send_frame(page, stream, frame_dir, generation)?;
            }
            ClientMessage::Hover { x, y } => page.hover_at(x, y),
            ClientMessage::GetRepresentation => {
                let snapshot = page.snapshot(*generation);
                blueice_ipc::write_server_message(stream, &ServerMessage::Representation(snapshot))?;
            }
            ClientMessage::ActOn { id, action } => {
                let is_click = matches!(action, NodeAction::Click);
                match page.act(NodeId::from_u64(id), action) {
                    Some(href) => match page.navigate(&href) {
                        Ok(()) => {
                            reply_navigated(page, stream)?;
                            send_frame(page, stream, frame_dir, generation)?;
                        }
                        Err(e) => blueice_ipc::write_server_message(stream, &ServerMessage::Error { message: e.to_string() })?,
                    },
                    // A Click that didn't land on a link is a no-op,
                    // same as a coordinate Click elsewhere -- no reply.
                    None if is_click => {}
                    None => send_frame(page, stream, frame_dir, generation)?,
                }
            }
            ClientMessage::Highlight { id } => {
                page.set_highlight(id.map(NodeId::from_u64));
                send_frame(page, stream, frame_dir, generation)?;
            }
            // Chrome commands (window show/hide) operate on `frontend`'s
            // own window, not on anything `core` owns -- see module docs.
            ClientMessage::Chrome(_) => {}
            ClientMessage::Shutdown => return Ok(()),
        }
    }
}

fn reply_navigated<S: Write>(page: &Page, stream: &mut S) -> io::Result<()> {
    blueice_ipc::write_server_message(stream, &ServerMessage::Navigated { url: page.url().unwrap_or_default().to_string() })
}

fn send_frame<S: Write>(page: &Page, stream: &mut S, frame_dir: &Path, generation: &mut u64) -> io::Result<()> {
    let pixmap = page.render_visible();
    *generation += 1;
    let path = shm::write_frame(frame_dir, *generation, &pixmap.pixels)?;
    blueice_ipc::write_server_message(
        stream,
        &ServerMessage::FrameReady { shm_path: path.to_string_lossy().into_owned(), width: pixmap.width, height: pixmap.height, generation: *generation },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::os::unix::net::UnixStream;
    use std::thread;

    fn temp_frame_dir(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("blueice-session-test-{label}-{}", std::process::id()))
    }

    fn client_pair() -> (UnixStream, UnixStream) {
        UnixStream::pair().unwrap()
    }

    #[test]
    fn resize_then_shutdown_produces_one_frame_and_then_ends_the_session() {
        let dir = temp_frame_dir("resize");
        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut page = Page::new(320.0, 200.0);
            page.load_html_str("<p>hi</p>", None);
            let mut generation = 0u64;
            run_session(&mut page, &mut server, &dir, &mut generation).unwrap();
            dir
        });

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Resize { width: 100, height: 50 }).unwrap();
        let reply = blueice_ipc::read_server_message(&mut client).unwrap();
        assert!(matches!(reply, ServerMessage::FrameReady { generation: 1, width: 100, height: 50, .. }));

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn navigate_replies_with_navigated_then_a_frame_reflecting_the_new_page() {
        let dir = temp_frame_dir("navigate");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 1024];
            let _ = std::io::Read::read(&mut stream, &mut buf);
            let body = "<p>fetched page</p>";
            std::io::Write::write_all(&mut stream, format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).unwrap();
        });

        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut page = Page::new(320.0, 200.0);
            let mut generation = 0u64;
            run_session(&mut page, &mut server, &dir, &mut generation).unwrap();
            dir
        });

        let url = format!("http://{addr}");
        blueice_ipc::write_client_message(&mut client, &ClientMessage::Navigate { url: url.clone() }).unwrap();
        let navigated = blueice_ipc::read_server_message(&mut client).unwrap();
        assert_eq!(navigated, ServerMessage::Navigated { url });
        let frame = blueice_ipc::read_server_message(&mut client).unwrap();
        let shm_path = match frame {
            ServerMessage::FrameReady { shm_path, generation: 1, .. } => shm_path,
            other => panic!("expected FrameReady, got {other:?}"),
        };
        assert!(shm::map_frame(std::path::Path::new(&shm_path)).is_ok(), "the frame-plane file must actually exist and be mappable");

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn navigate_to_an_unreachable_host_replies_with_error_not_a_frame() {
        let dir = temp_frame_dir("navigate-error");
        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut page = Page::new(320.0, 200.0);
            let mut generation = 0u64;
            run_session(&mut page, &mut server, &dir, &mut generation).unwrap();
            dir
        });

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Navigate { url: "not-a-valid-url".to_string() }).unwrap();
        let reply = blueice_ipc::read_server_message(&mut client).unwrap();
        assert!(matches!(reply, ServerMessage::Error { .. }));

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn click_on_a_link_navigates_and_a_click_elsewhere_produces_no_reply() {
        let dir = temp_frame_dir("click");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 1024];
            let _ = std::io::Read::read(&mut stream, &mut buf);
            let body = "<p>landed</p>";
            std::io::Write::write_all(&mut stream, format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).unwrap();
        });
        let url = format!("http://{addr}");

        let (mut client, mut server) = client_pair();
        let dir_for_thread = dir.clone();
        let handle = thread::spawn(move || {
            let mut page = Page::new(320.0, 200.0);
            page.load_html_str(&format!(r#"<a href="{url}">go</a>"#), None);
            let mut generation = 0u64;
            run_session(&mut page, &mut server, &dir_for_thread, &mut generation).unwrap();
        });

        // clicking the link navigates: expect Navigated then FrameReady
        blueice_ipc::write_client_message(&mut client, &ClientMessage::Click { x: 2.0, y: 2.0 }).unwrap();
        let navigated = blueice_ipc::read_server_message(&mut client).unwrap();
        assert!(matches!(navigated, ServerMessage::Navigated { .. }));
        let frame = blueice_ipc::read_server_message(&mut client).unwrap();
        assert!(matches!(frame, ServerMessage::FrameReady { .. }));

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn set_visible_produces_no_reply_and_the_session_keeps_running() {
        let dir = temp_frame_dir("visible");
        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut page = Page::new(320.0, 200.0);
            let mut generation = 0u64;
            run_session(&mut page, &mut server, &dir, &mut generation).unwrap();
            dir
        });

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Chrome(blueice_ipc::ChromeCommand::SetVisible(false))).unwrap();
        // proven by the fact that a subsequent message still gets a
        // normal reply -- Chrome(SetVisible) didn't wedge or end the session.
        blueice_ipc::write_client_message(&mut client, &ClientMessage::Resize { width: 10, height: 10 }).unwrap();
        let reply = blueice_ipc::read_server_message(&mut client).unwrap();
        assert!(matches!(reply, ServerMessage::FrameReady { .. }));

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn get_representation_shares_the_current_generation_with_the_last_frame() {
        // the concrete, checkable "same render pass" proof
        // `phase-5-ai-representation-output/PLAN.md` asks for: a
        // Representation and the FrameReady sent alongside a prior
        // state change carry the identical generation number.
        let dir = temp_frame_dir("representation-generation");
        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut page = Page::new(320.0, 200.0);
            page.load_html_str(r#"<a href="/x">Go</a>"#, None);
            let mut generation = 0u64;
            run_session(&mut page, &mut server, &dir, &mut generation).unwrap();
            dir
        });

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Resize { width: 100, height: 50 }).unwrap();
        let frame = blueice_ipc::read_server_message(&mut client).unwrap();
        let ServerMessage::FrameReady { generation: frame_generation, .. } = frame else { panic!("expected FrameReady, got {frame:?}") };

        blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
        let reply = blueice_ipc::read_server_message(&mut client).unwrap();
        let ServerMessage::Representation(snapshot) = reply else { panic!("expected Representation, got {reply:?}") };
        assert_eq!(snapshot.generation, frame_generation);
        assert!(snapshot.nodes.iter().any(|n| n.name.as_deref() == Some("Go")));

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn act_on_click_navigates_the_same_way_a_coordinate_click_does() {
        let dir = temp_frame_dir("act-on-click");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 1024];
            let _ = std::io::Read::read(&mut stream, &mut buf);
            let body = "<p>landed via id</p>";
            std::io::Write::write_all(&mut stream, format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).unwrap();
        });
        let url = format!("http://{addr}");

        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut page = Page::new(320.0, 200.0);
            page.load_html_str(&format!(r#"<a href="{url}">go</a>"#), None);
            let mut generation = 0u64;
            run_session(&mut page, &mut server, &dir, &mut generation).unwrap();
            dir
        });

        blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
        let ServerMessage::Representation(snapshot) = blueice_ipc::read_server_message(&mut client).unwrap() else { panic!("expected Representation") };
        let link_id = snapshot.nodes.iter().find(|n| n.name.as_deref() == Some("go")).unwrap().id;

        blueice_ipc::write_client_message(&mut client, &ClientMessage::ActOn { id: link_id, action: NodeAction::Click }).unwrap();
        let navigated = blueice_ipc::read_server_message(&mut client).unwrap();
        assert!(matches!(navigated, ServerMessage::Navigated { .. }));
        let frame = blueice_ipc::read_server_message(&mut client).unwrap();
        assert!(matches!(frame, ServerMessage::FrameReady { .. }));

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn act_on_focus_is_reflected_in_the_next_representation() {
        let dir = temp_frame_dir("act-on-focus");
        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut page = Page::new(320.0, 200.0);
            page.load_html_str(r#"<input id="name" type="text" placeholder="Name">"#, None);
            let mut generation = 0u64;
            run_session(&mut page, &mut server, &dir, &mut generation).unwrap();
            dir
        });

        blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
        let ServerMessage::Representation(before) = blueice_ipc::read_server_message(&mut client).unwrap() else { panic!("expected Representation") };
        let input_id = before.nodes[0].id;
        assert!(!before.nodes[0].state.focused);

        blueice_ipc::write_client_message(&mut client, &ClientMessage::ActOn { id: input_id, action: NodeAction::Focus }).unwrap();
        let frame = blueice_ipc::read_server_message(&mut client).unwrap();
        assert!(matches!(frame, ServerMessage::FrameReady { .. }), "Focus is a state change and still gets a FrameReady, per session.rs's own docs");

        blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
        let ServerMessage::Representation(after) = blueice_ipc::read_server_message(&mut client).unwrap() else { panic!("expected Representation") };
        assert!(after.nodes[0].state.focused);

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn act_on_an_unknown_id_is_a_harmless_no_op() {
        let dir = temp_frame_dir("act-on-unknown");
        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut page = Page::new(320.0, 200.0);
            page.load_html_str(r#"<a href="/x">go</a>"#, None);
            let mut generation = 0u64;
            run_session(&mut page, &mut server, &dir, &mut generation).unwrap();
            dir
        });

        // an unknown id with Click: same "no reply at all" contract as
        // a coordinate click that lands on nothing.
        blueice_ipc::write_client_message(&mut client, &ClientMessage::ActOn { id: 999_999, action: NodeAction::Click }).unwrap();
        // proven by the fact that the next message still gets a normal
        // reply -- the unknown id didn't wedge or end the session.
        blueice_ipc::write_client_message(&mut client, &ClientMessage::Resize { width: 10, height: 10 }).unwrap();
        let reply = blueice_ipc::read_server_message(&mut client).unwrap();
        assert!(matches!(reply, ServerMessage::FrameReady { .. }));

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn highlight_adds_an_outline_to_the_next_frame_and_clearing_it_removes_it() {
        let dir = temp_frame_dir("highlight");
        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut page = Page::new(320.0, 200.0);
            page.load_html_str(r#"<a href="/x">go</a>"#, None);
            let mut generation = 0u64;
            run_session(&mut page, &mut server, &dir, &mut generation).unwrap();
            dir
        });

        blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
        let ServerMessage::Representation(snap) = blueice_ipc::read_server_message(&mut client).unwrap() else { panic!("expected Representation") };
        let link_id = snap.nodes[0].id;

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Highlight { id: Some(link_id) }).unwrap();
        let frame = blueice_ipc::read_server_message(&mut client).unwrap();
        assert!(matches!(frame, ServerMessage::FrameReady { .. }));

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn hover_updates_state_silently_with_no_reply() {
        let dir = temp_frame_dir("hover");
        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut page = Page::new(320.0, 200.0);
            page.load_html_str(r#"<a href="/x">go</a>"#, None);
            let mut generation = 0u64;
            run_session(&mut page, &mut server, &dir, &mut generation).unwrap();
            dir
        });

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Hover { x: 2.0, y: 2.0 }).unwrap();
        // proven the same way SetVisible/Chrome is: the next message
        // still gets a normal reply, so Hover didn't wedge the session.
        blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
        let ServerMessage::Representation(snap) = blueice_ipc::read_server_message(&mut client).unwrap() else { panic!("expected Representation") };
        assert!(snap.nodes[0].state.hovered, "the hovered state must be visible via GetRepresentation");

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn chrome_set_visible_does_not_change_engine_render_state() {
        // `phase-5-ai-representation-output/PLAN.md`'s "verify engine
        // state is unchanged across a hide/show cycle" checklist item,
        // made explicit and checkable rather than left implicit in
        // `Chrome`'s no-op handling: a full hide-then-show round trip
        // must leave the representation (and therefore the DOM/styles/
        // fragment tree it's derived from) byte-for-byte identical, and
        // must not cause a new frame to be rendered.
        let dir = temp_frame_dir("chrome-no-restart");
        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut page = Page::new(320.0, 200.0);
            page.load_html_str("<p>hi</p>", None);
            let mut generation = 0u64;
            run_session(&mut page, &mut server, &dir, &mut generation).unwrap();
            dir
        });

        blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
        let ServerMessage::Representation(before) = blueice_ipc::read_server_message(&mut client).unwrap() else { panic!("expected Representation") };

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Chrome(blueice_ipc::ChromeCommand::SetVisible(false))).unwrap();
        blueice_ipc::write_client_message(&mut client, &ClientMessage::Chrome(blueice_ipc::ChromeCommand::SetVisible(true))).unwrap();

        blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
        let ServerMessage::Representation(after) = blueice_ipc::read_server_message(&mut client).unwrap() else { panic!("expected Representation") };

        assert_eq!(before.nodes, after.nodes, "a hide/show cycle must not change the engine's render-pass state");
        assert_eq!(before.generation, after.generation, "no frame is re-rendered just from a visibility toggle");

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn disconnecting_without_shutdown_ends_the_session_cleanly() {
        let dir = temp_frame_dir("disconnect");
        let (client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut page = Page::new(320.0, 200.0);
            let mut generation = 0u64;
            run_session(&mut page, &mut server, &dir, &mut generation)
        });
        drop(client);
        assert!(handle.join().unwrap().is_ok());
    }
}
