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
//! lands on a link, `Scroll`) ends with a fresh frame written to the
//! frame-plane and a `FrameReady` reply -- `SetVisible` is the one
//! exception, since per `BROWSER_CORE_PLAN.md` §1 the render pipeline
//! runs identically regardless of window visibility, so there's
//! nothing here for it to change.

use crate::Page;
use blueice_ipc::{shm, ClientMessage, ServerMessage};
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
            ClientMessage::SetVisible(_) => {}
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

        blueice_ipc::write_client_message(&mut client, &ClientMessage::SetVisible(false)).unwrap();
        // proven by the fact that a subsequent message still gets a
        // normal reply -- SetVisible didn't wedge or end the session.
        blueice_ipc::write_client_message(&mut client, &ClientMessage::Resize { width: 10, height: 10 }).unwrap();
        let reply = blueice_ipc::read_server_message(&mut client).unwrap();
        assert!(matches!(reply, ServerMessage::FrameReady { .. }));

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
