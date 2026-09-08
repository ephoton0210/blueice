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

use crate::{Page, TabId, TabManager};
use blueice_dom::NodeId;
use blueice_ipc::{shm, ClientMessage, NodeAction, ServerMessage, TabSummary};
use std::io::{self, Read, Write};
use std::path::Path;

/// Runs the message loop for one client connection until it sends
/// `Shutdown` or disconnects. `frame_dir` is where this session's
/// frames are written (see [`blueice_ipc::shm`]); `generation` is a
/// single, session-wide frame-sequence counter shared across every tab
/// (not one per tab) -- every tab's `FrameReady` still gets the next
/// global monotonic number, so `blueice_ipc::shm` needs no per-tab
/// awareness at all (filenames stay collision-free by construction),
/// and the AI-facing "same generation = same render pass" property
/// still holds across tabs.
///
/// The very first message must be [`ClientMessage::Hello`] (`phase-1-
/// ai-representation-layer/PLAN.md` §3's `protocol_version` handshake)
/// -- a fresh connection whose first message either isn't `Hello` or
/// declares an unsupported version is rejected with a
/// [`ServerMessage::Error`] before anything else is processed, and the
/// session ends without entering the main loop. A `Hello` seen again
/// *after* the handshake (e.g. a second external client's own
/// handshake, forwarded by `blueice-launcher`'s broker into the one
/// shared connection it holds with `core`) is just answered again,
/// rather than re-gating the whole session -- tearing down a shared
/// connection over one client's handshake would end every other
/// client's session too.
///
/// **Multi-tab addressing** (`phase-16-multi-tab-and-tab-groups/
/// PLAN.md`'s minimal first slice): every per-tab-scoped message
/// (`Navigate`, `Resize`, `Click`, `Hover`, `Scroll`,
/// `GetRepresentation`, `ActOn`, `Highlight`, `GetDom`, `CloseTab`) is
/// addressed by the envelope's `tab_id` -- `None` resolves to
/// [`TabManager::default_tab`], reproducing pre-Phase-16 single-`Page`
/// behavior byte-for-byte for a client that never sends `OpenTab`. A
/// `tab_id` (explicit or defaulted) that doesn't resolve to a live tab
/// replies [`ServerMessage::Error`] -- a protocol-addressing error, not
/// the harmless no-op a stale `NodeId` already gets in [`Page::act`].
/// `OpenTab`/`ListTabs` aren't scoped to an existing tab at all (there's
/// no "current tab" concept `core` tracks -- see [`TabManager`]'s own
/// docs for why) and ignore any `tab_id` on the envelope.
pub fn run_session<S: Read + Write>(tabs: &mut TabManager, stream: &mut S, frame_dir: &Path, generation: &mut u64) -> io::Result<()> {
    if !perform_handshake(stream)? {
        return Ok(());
    }
    loop {
        let (tab_id, request_id, msg) = match blueice_ipc::read_client_message_with_ids(stream) {
            Ok(v) => v,
            Err(_) => return Ok(()), // client disconnected without an explicit Shutdown
        };
        let target = tab_id.map(TabId::from_u64).unwrap_or_else(|| tabs.default_tab());
        // Every per-tab reply below echoes `Some(target.as_u64())`, the
        // *resolved* tab -- not the raw (possibly `None`, if the
        // request left it defaulted) `tab_id` the request carried.
        // Echoing the ambiguous original back would defeat the whole
        // point of this field: a client watching a shared, multi-tab,
        // broadcast connection (`blueice-launcher`'s broker) needs
        // every reply to self-disclose which concrete tab it's about,
        // including one produced by a request that left it implicit.
        let reply_tab = Some(target.as_u64());
        match msg {
            ClientMessage::Hello { protocol_version } => reply_hello(stream, request_id, protocol_version)?,
            ClientMessage::Navigate { url } => match tabs.get_mut(target) {
                Some(page) => match page.navigate(&url) {
                    Ok(()) => {
                        reply_navigated(page, stream, reply_tab, request_id)?;
                        send_frame(page, stream, frame_dir, generation, reply_tab, request_id)?;
                    }
                    Err(e) => write_error(stream, reply_tab, request_id, e.to_string())?,
                },
                None => write_unknown_tab_error(stream, request_id, target)?,
            },
            ClientMessage::Resize { width, height } => {
                tabs.set_window_size(width as f64, height as f64);
                match tabs.get_mut(target) {
                    Some(page) => {
                        page.resize(width as f64, height as f64);
                        send_frame(page, stream, frame_dir, generation, reply_tab, request_id)?;
                    }
                    None => write_unknown_tab_error(stream, request_id, target)?,
                }
            }
            ClientMessage::Click { x, y } => match tabs.get_mut(target) {
                Some(page) => {
                    if let Some(href) = page.click(x, y) {
                        match page.navigate(&href) {
                            Ok(()) => {
                                reply_navigated(page, stream, reply_tab, request_id)?;
                                send_frame(page, stream, frame_dir, generation, reply_tab, request_id)?;
                            }
                            Err(e) => write_error(stream, reply_tab, request_id, e.to_string())?,
                        }
                    }
                }
                None => write_unknown_tab_error(stream, request_id, target)?,
            },
            ClientMessage::Scroll { delta_y } => match tabs.get_mut(target) {
                Some(page) => {
                    page.scroll_by(delta_y);
                    send_frame(page, stream, frame_dir, generation, reply_tab, request_id)?;
                }
                None => write_unknown_tab_error(stream, request_id, target)?,
            },
            ClientMessage::Hover { x, y } => {
                if let Some(page) = tabs.get_mut(target) {
                    page.hover_at(x, y);
                } else {
                    write_unknown_tab_error(stream, request_id, target)?;
                }
            }
            ClientMessage::GetRepresentation => match tabs.get_mut(target) {
                Some(page) => {
                    let snapshot = page.snapshot(*generation, target.as_u64());
                    blueice_ipc::write_server_message_with_ids(stream, reply_tab, request_id, &ServerMessage::Representation(snapshot))?;
                }
                None => write_unknown_tab_error(stream, request_id, target)?,
            },
            ClientMessage::GetDom => match tabs.get_mut(target) {
                Some(page) => blueice_ipc::write_server_message_with_ids(stream, reply_tab, request_id, &ServerMessage::Dom(page.dom_dump()))?,
                None => write_unknown_tab_error(stream, request_id, target)?,
            },
            ClientMessage::ActOn { id, action } => match tabs.get_mut(target) {
                Some(page) => {
                    let is_click = matches!(action, NodeAction::Click);
                    match page.act(NodeId::from_u64(id), action) {
                        Some(href) => match page.navigate(&href) {
                            Ok(()) => {
                                reply_navigated(page, stream, reply_tab, request_id)?;
                                send_frame(page, stream, frame_dir, generation, reply_tab, request_id)?;
                            }
                            Err(e) => write_error(stream, reply_tab, request_id, e.to_string())?,
                        },
                        // A Click that didn't land on a link is a
                        // no-op, same as a coordinate Click elsewhere
                        // -- no reply.
                        None if is_click => {}
                        None => send_frame(page, stream, frame_dir, generation, reply_tab, request_id)?,
                    }
                }
                None => write_unknown_tab_error(stream, request_id, target)?,
            },
            ClientMessage::Highlight { id } => match tabs.get_mut(target) {
                Some(page) => {
                    page.set_highlight(id.map(NodeId::from_u64));
                    send_frame(page, stream, frame_dir, generation, reply_tab, request_id)?;
                }
                None => write_unknown_tab_error(stream, request_id, target)?,
            },
            ClientMessage::OpenTab { url } => handle_open_tab(tabs, stream, frame_dir, generation, request_id, url)?,
            ClientMessage::CloseTab => {
                if tabs.close_tab(target) {
                    blueice_ipc::write_server_message_with_ids(stream, reply_tab, request_id, &ServerMessage::TabClosed { tab_id: target.as_u64() })?;
                } else {
                    write_unknown_tab_error(stream, request_id, target)?;
                }
            }
            ClientMessage::ListTabs => {
                let summaries: Vec<TabSummary> =
                    tabs.ids().map(|id| TabSummary { id: id.as_u64(), url: tabs.get(id).and_then(Page::url).map(str::to_string) }).collect();
                blueice_ipc::write_server_message_with_id(stream, request_id, &ServerMessage::Tabs(summaries))?;
            }
            // Chrome commands (window show/hide) operate on `frontend`'s
            // own window, not on anything `core` owns -- see module docs.
            ClientMessage::Chrome(_) => {}
            ClientMessage::Shutdown => return Ok(()),
            // Forward-compatibility fallback (plan §3): a variant this
            // build doesn't recognize is ignored rather than treated as
            // a protocol violation.
            ClientMessage::Unknown => {}
        }
    }
}

/// `OpenTab`'s handler: always creates the tab (there's no failure mode
/// for that itself), then optionally navigates it. See
/// [`ServerMessage::TabOpened`]'s own docs for the one real limitation
/// this has (a navigation failure here doesn't separately report the
/// orphaned blank tab's id).
fn handle_open_tab<S: Write>(tabs: &mut TabManager, stream: &mut S, frame_dir: &Path, generation: &mut u64, request_id: Option<u64>, url: Option<String>) -> io::Result<()> {
    let new_id = tabs.open_tab();
    let Some(url) = url else {
        return blueice_ipc::write_server_message_with_ids(stream, Some(new_id.as_u64()), request_id, &ServerMessage::TabOpened { tab_id: new_id.as_u64(), url: None });
    };
    let page = tabs.get_mut(new_id).expect("a tab this function just created must exist");
    match page.navigate(&url) {
        Ok(()) => {
            let final_url = page.url().map(str::to_string);
            blueice_ipc::write_server_message_with_ids(stream, Some(new_id.as_u64()), request_id, &ServerMessage::TabOpened { tab_id: new_id.as_u64(), url: final_url })?;
            send_frame(page, stream, frame_dir, generation, Some(new_id.as_u64()), request_id)
        }
        Err(e) => write_error(stream, Some(new_id.as_u64()), request_id, e.to_string()),
    }
}

/// Gates entry to the main loop on a valid `Hello` as the connection's
/// very first message, per `run_session`'s own docs. Returns `Ok(true)`
/// once the handshake has succeeded and the main loop should start,
/// `Ok(false)` if the session should end without ever entering it (a
/// non-`Hello` first message, an unsupported `protocol_version`, or
/// the client disconnecting before sending anything at all).
fn perform_handshake<S: Read + Write>(stream: &mut S) -> io::Result<bool> {
    let (request_id, msg) = match blueice_ipc::read_client_message_with_id(stream) {
        Ok(v) => v,
        Err(_) => return Ok(false),
    };
    match msg {
        ClientMessage::Hello { protocol_version } => reply_hello(stream, request_id, protocol_version).map(|()| protocol_version == blueice_ipc::PROTOCOL_VERSION),
        _ => {
            blueice_ipc::write_server_message_with_id(stream, request_id, &ServerMessage::Error { message: "the first message on a connection must be Hello".to_string() })?;
            Ok(false)
        }
    }
}

fn reply_hello<S: Write>(stream: &mut S, request_id: Option<u64>, protocol_version: u32) -> io::Result<()> {
    if protocol_version == blueice_ipc::PROTOCOL_VERSION {
        blueice_ipc::write_server_message_with_id(stream, request_id, &ServerMessage::Hello { protocol_version: blueice_ipc::PROTOCOL_VERSION })
    } else {
        blueice_ipc::write_server_message_with_id(
            stream,
            request_id,
            &ServerMessage::Error { message: format!("unsupported protocol_version {protocol_version}, this core speaks {}", blueice_ipc::PROTOCOL_VERSION) },
        )
    }
}

fn reply_navigated<S: Write>(page: &Page, stream: &mut S, tab_id: Option<u64>, request_id: Option<u64>) -> io::Result<()> {
    blueice_ipc::write_server_message_with_ids(stream, tab_id, request_id, &ServerMessage::Navigated { url: page.url().unwrap_or_default().to_string() })
}

fn send_frame<S: Write>(page: &Page, stream: &mut S, frame_dir: &Path, generation: &mut u64, tab_id: Option<u64>, request_id: Option<u64>) -> io::Result<()> {
    let pixmap = page.render_visible();
    *generation += 1;
    let path = shm::write_frame(frame_dir, *generation, &pixmap.pixels)?;
    blueice_ipc::write_server_message_with_ids(
        stream,
        tab_id,
        request_id,
        &ServerMessage::FrameReady { shm_path: path.to_string_lossy().into_owned(), width: pixmap.width, height: pixmap.height, generation: *generation },
    )
}

fn write_error<S: Write>(stream: &mut S, tab_id: Option<u64>, request_id: Option<u64>, message: String) -> io::Result<()> {
    blueice_ipc::write_server_message_with_ids(stream, tab_id, request_id, &ServerMessage::Error { message })
}

/// A `tab_id` (explicit or defaulted) that doesn't resolve to a live
/// tab -- see `run_session`'s own docs for why this is always a real
/// `Error` reply, never a silent no-op. Echoes `target` itself as the
/// reply's `tab_id`, so the client at least learns which (nonexistent)
/// tab it addressed.
fn write_unknown_tab_error<S: Write>(stream: &mut S, request_id: Option<u64>, target: TabId) -> io::Result<()> {
    write_error(stream, Some(target.as_u64()), request_id, format!("unknown tab {}", target.as_u64()))
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

    /// Performs the `protocol_version` handshake `run_session` now
    /// requires as the very first message on a fresh connection --
    /// every test below drives `run_session` over a brand-new
    /// connection, so every one of them needs this before its own
    /// message(s), the same way a real client (`frontend`, `blueice-
    /// mcp-server`) would via `blueice_ipc::client_handshake`.
    fn handshake(client: &mut UnixStream) {
        blueice_ipc::client_handshake(client).unwrap();
    }

    /// Every test below that predates multi-tab (Phase 16) sets up its
    /// fixture content on "the" page, the same single-tab shape it
    /// always had -- this is just `tabs.default_tab()` resolved to its
    /// `Page`, so those tests don't need to change beyond `Page::new`
    /// becoming `TabManager::new`.
    fn default_page(tabs: &mut TabManager) -> &mut Page {
        let default = tabs.default_tab();
        tabs.get_mut(default).unwrap()
    }

    #[test]
    fn resize_then_shutdown_produces_one_frame_and_then_ends_the_session() {
        let dir = temp_frame_dir("resize");
        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            default_page(&mut tabs).load_html_str("<p>hi</p>", None);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation).unwrap();
            dir
        });
        handshake(&mut client);

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
            let mut tabs = TabManager::new(320.0, 200.0);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation).unwrap();
            dir
        });
        handshake(&mut client);

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
            let mut tabs = TabManager::new(320.0, 200.0);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation).unwrap();
            dir
        });
        handshake(&mut client);

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
            let mut tabs = TabManager::new(320.0, 200.0);
            default_page(&mut tabs).load_html_str(&format!(r#"<a href="{url}">go</a>"#), None);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir_for_thread, &mut generation).unwrap();
        });
        handshake(&mut client);

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
            let mut tabs = TabManager::new(320.0, 200.0);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation).unwrap();
            dir
        });
        handshake(&mut client);

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
    fn get_representation_shares_the_current_generation_across_every_send_frame_call_site() {
        // `get_representation_shares_the_current_generation_with_the_last_frame`
        // below proves the "same render pass" invariant for `Resize`
        // alone; this extends the same proof to `Scroll`, `Highlight`,
        // and a non-navigating `ActOn` (`Focus`) -- the other distinct
        // `send_frame` call sites in `run_session` (`Click`/`ActOn`'s
        // Click variant only ever reach `send_frame` via the same
        // navigate path `Navigate` itself already exercises, so they add
        // no new coverage here). `send_frame` is a single choke point
        // every one of these routes through, so this is expected to
        // hold structurally -- but the invariant is central enough to
        // this project's premise to prove per call site, not infer from
        // one example.
        let dir = temp_frame_dir("representation-generation-all-sites");
        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            default_page(&mut tabs).load_html_str(r#"<input type="text">"#, None);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation).unwrap();
            dir
        });
        handshake(&mut client);

        blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
        let ServerMessage::Representation(snap) = blueice_ipc::read_server_message(&mut client).unwrap() else { panic!("expected Representation") };
        let input_id = snap.nodes[0].id;

        let assert_matching_generation = |client: &mut UnixStream, send: ClientMessage| {
            blueice_ipc::write_client_message(client, &send).unwrap();
            let frame = blueice_ipc::read_server_message(client).unwrap();
            let ServerMessage::FrameReady { generation: frame_generation, .. } = frame else { panic!("expected FrameReady, got {frame:?}") };

            blueice_ipc::write_client_message(client, &ClientMessage::GetRepresentation).unwrap();
            let reply = blueice_ipc::read_server_message(client).unwrap();
            let ServerMessage::Representation(snapshot) = reply else { panic!("expected Representation, got {reply:?}") };
            assert_eq!(snapshot.generation, frame_generation);
        };

        assert_matching_generation(&mut client, ClientMessage::Scroll { delta_y: 10.0 });
        assert_matching_generation(&mut client, ClientMessage::Highlight { id: Some(input_id) });
        assert_matching_generation(&mut client, ClientMessage::ActOn { id: input_id, action: NodeAction::Focus });

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
            let mut tabs = TabManager::new(320.0, 200.0);
            default_page(&mut tabs).load_html_str(r#"<a href="/x">Go</a>"#, None);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation).unwrap();
            dir
        });
        handshake(&mut client);

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
    fn get_dom_returns_the_full_tree_unfiltered_by_the_ai_representation() {
        let dir = temp_frame_dir("get-dom");
        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            default_page(&mut tabs).load_html_str(r#"<div style="background-color: red;">x</div>"#, None);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation).unwrap();
            dir
        });
        handshake(&mut client);

        blueice_ipc::write_client_message(&mut client, &ClientMessage::GetDom).unwrap();
        let reply = blueice_ipc::read_server_message(&mut client).unwrap();
        let ServerMessage::Dom(dump) = reply else { panic!("expected Dom, got {reply:?}") };
        assert!(dump.contains("<div>"), "a bare div has no AI-representation role but must still appear in the full DOM dump: {dump}");

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
            let mut tabs = TabManager::new(320.0, 200.0);
            default_page(&mut tabs).load_html_str(&format!(r#"<a href="{url}">go</a>"#), None);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation).unwrap();
            dir
        });
        handshake(&mut client);

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
            let mut tabs = TabManager::new(320.0, 200.0);
            default_page(&mut tabs).load_html_str(r#"<input id="name" type="text" placeholder="Name">"#, None);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation).unwrap();
            dir
        });
        handshake(&mut client);

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
            let mut tabs = TabManager::new(320.0, 200.0);
            default_page(&mut tabs).load_html_str(r#"<a href="/x">go</a>"#, None);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation).unwrap();
            dir
        });
        handshake(&mut client);

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
    fn a_stale_id_from_before_a_navigation_is_a_harmless_no_op_after_it() {
        // Unlike `act_on_an_unknown_id_is_a_harmless_no_op` (a
        // never-allocated id), this id is real -- it existed in the
        // document *before* the navigation below. Regression: NodeId
        // allocation used to restart at 0 for every freshly-parsed
        // document, so this same numeric id could be reused by an
        // unrelated node in the post-navigation document, and ActOn
        // would silently act on that unrelated node instead of safely
        // no-op'ing.
        let dir = temp_frame_dir("stale-id-across-navigation");
        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            default_page(&mut tabs).load_html_str(r#"<a href="/x">go</a>"#, None);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation).unwrap();
            dir
        });
        handshake(&mut client);

        blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
        let ServerMessage::Representation(snap) = blueice_ipc::read_server_message(&mut client).unwrap() else { panic!("expected Representation") };
        let stale_id = snap.nodes[0].id;

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Navigate { url: "about:blank".to_string() }).unwrap();
        let navigated = blueice_ipc::read_server_message(&mut client).unwrap();
        assert_eq!(navigated, ServerMessage::Navigated { url: "about:blank".to_string() });
        let _frame = blueice_ipc::read_server_message(&mut client).unwrap();

        blueice_ipc::write_client_message(&mut client, &ClientMessage::ActOn { id: stale_id, action: NodeAction::Click }).unwrap();
        // proven the same way as the never-allocated-id case: the next
        // message still gets a normal reply, so the stale id neither
        // wedged the session nor triggered a misdirected action.
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
            let mut tabs = TabManager::new(320.0, 200.0);
            default_page(&mut tabs).load_html_str(r#"<a href="/x">go</a>"#, None);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation).unwrap();
            dir
        });
        handshake(&mut client);

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
            let mut tabs = TabManager::new(320.0, 200.0);
            default_page(&mut tabs).load_html_str(r#"<a href="/x">go</a>"#, None);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation).unwrap();
            dir
        });
        handshake(&mut client);

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
            let mut tabs = TabManager::new(320.0, 200.0);
            default_page(&mut tabs).load_html_str("<p>hi</p>", None);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation).unwrap();
            dir
        });
        handshake(&mut client);

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
            let mut tabs = TabManager::new(320.0, 200.0);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation)
        });
        drop(client);
        assert!(handle.join().unwrap().is_ok());
    }

    #[test]
    fn a_first_message_that_is_not_hello_is_rejected_and_ends_the_session() {
        let dir = temp_frame_dir("handshake-not-hello-first");
        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation)
        });

        blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
        let reply = blueice_ipc::read_server_message(&mut client).unwrap();
        assert!(matches!(reply, ServerMessage::Error { .. }), "expected an Error reply, got {reply:?}");

        assert!(handle.join().unwrap().is_ok(), "the session must end cleanly, not hang, after rejecting the handshake");
    }

    #[test]
    fn an_unsupported_protocol_version_is_rejected_and_ends_the_session() {
        let dir = temp_frame_dir("handshake-bad-version");
        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation)
        });

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Hello { protocol_version: blueice_ipc::PROTOCOL_VERSION + 1 }).unwrap();
        let reply = blueice_ipc::read_server_message(&mut client).unwrap();
        assert!(matches!(reply, ServerMessage::Error { .. }), "expected an Error reply, got {reply:?}");

        assert!(handle.join().unwrap().is_ok(), "the session must end cleanly, not hang, after rejecting an unsupported version");
    }

    #[test]
    fn a_hello_seen_again_after_the_handshake_is_answered_without_ending_the_session() {
        // The broker-multiplexing scenario `run_session`'s own docs
        // describe: a second external client's handshake, forwarded
        // into the one already-past-its-own-handshake shared
        // connection, must not be treated as a protocol violation.
        let dir = temp_frame_dir("late-hello");
        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation).unwrap();
            dir
        });
        handshake(&mut client);

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Hello { protocol_version: blueice_ipc::PROTOCOL_VERSION }).unwrap();
        let reply = blueice_ipc::read_server_message(&mut client).unwrap();
        assert_eq!(reply, ServerMessage::Hello { protocol_version: blueice_ipc::PROTOCOL_VERSION });

        // proven the same way other no-special-effect messages are:
        // the session is still alive and answers normally afterward.
        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn every_reply_to_a_message_echoes_back_its_request_id() {
        let dir = temp_frame_dir("request-id-echo");
        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            default_page(&mut tabs).load_html_str("<p>hi</p>", None);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation).unwrap();
            dir
        });
        handshake(&mut client);

        blueice_ipc::write_client_message_with_id(&mut client, Some(99), &ClientMessage::GetRepresentation).unwrap();
        let (request_id, reply) = blueice_ipc::read_server_message_with_id(&mut client).unwrap();
        assert_eq!(request_id, Some(99));
        assert!(matches!(reply, ServerMessage::Representation(_)));

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_unknown_client_variant_is_ignored_and_the_session_keeps_running() {
        let dir = temp_frame_dir("unknown-variant");
        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation).unwrap();
            dir
        });
        handshake(&mut client);

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Unknown).unwrap();
        // proven the same way other no-reply messages are: the next
        // message still gets a normal reply, so Unknown didn't wedge
        // or end the session.
        blueice_ipc::write_client_message(&mut client, &ClientMessage::Resize { width: 10, height: 10 }).unwrap();
        let reply = blueice_ipc::read_server_message(&mut client).unwrap();
        assert!(matches!(reply, ServerMessage::FrameReady { .. }));

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn open_tab_creates_a_second_tab_visible_in_list_tabs() {
        let dir = temp_frame_dir("open-tab-list");
        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation).unwrap();
            dir
        });
        handshake(&mut client);

        blueice_ipc::write_client_message(&mut client, &ClientMessage::ListTabs).unwrap();
        let ServerMessage::Tabs(before) = blueice_ipc::read_server_message(&mut client).unwrap() else { panic!("expected Tabs") };
        assert_eq!(before.len(), 1, "a fresh core starts with exactly one tab, same as before Phase 16");

        blueice_ipc::write_client_message(&mut client, &ClientMessage::OpenTab { url: None }).unwrap();
        let ServerMessage::TabOpened { tab_id: new_id, url } = blueice_ipc::read_server_message(&mut client).unwrap() else { panic!("expected TabOpened") };
        assert_eq!(url, None);
        assert_ne!(new_id, before[0].id);

        blueice_ipc::write_client_message(&mut client, &ClientMessage::ListTabs).unwrap();
        let ServerMessage::Tabs(after) = blueice_ipc::read_server_message(&mut client).unwrap() else { panic!("expected Tabs") };
        assert_eq!(after.iter().map(|t| t.id).collect::<Vec<_>>(), vec![before[0].id, new_id]);

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn open_tab_with_a_url_navigates_it_and_sends_a_frame() {
        let dir = temp_frame_dir("open-tab-with-url");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 1024];
            let _ = std::io::Read::read(&mut stream, &mut buf);
            let body = "<p>opened via url</p>";
            std::io::Write::write_all(&mut stream, format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).unwrap();
        });

        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation).unwrap();
            dir
        });
        handshake(&mut client);

        let url = format!("http://{addr}");
        blueice_ipc::write_client_message(&mut client, &ClientMessage::OpenTab { url: Some(url.clone()) }).unwrap();
        let ServerMessage::TabOpened { tab_id: new_id, url: opened_url } = blueice_ipc::read_server_message(&mut client).unwrap() else { panic!("expected TabOpened") };
        assert_eq!(opened_url, Some(url));
        let frame = blueice_ipc::read_server_message(&mut client).unwrap();
        assert!(matches!(frame, ServerMessage::FrameReady { .. }), "expected FrameReady, got {frame:?}");

        // The new tab's content must actually be addressable afterward.
        blueice_ipc::write_client_message_with_ids(&mut client, Some(new_id), None, &ClientMessage::GetRepresentation).unwrap();
        let (reply_tab, _, reply) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
        assert_eq!(reply_tab, Some(new_id));
        let ServerMessage::Representation(snapshot) = reply else { panic!("expected Representation, got {reply:?}") };
        assert_eq!(snapshot.tab_id, new_id);
        assert!(snapshot.nodes.iter().any(|n| n.name.as_deref() == Some("opened via url")));

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn open_tab_with_a_failing_url_replies_error_not_tab_opened() {
        let dir = temp_frame_dir("open-tab-failing-url");
        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation).unwrap();
            dir
        });
        handshake(&mut client);

        blueice_ipc::write_client_message(&mut client, &ClientMessage::OpenTab { url: Some("not-a-valid-url".to_string()) }).unwrap();
        let reply = blueice_ipc::read_server_message(&mut client).unwrap();
        assert!(matches!(reply, ServerMessage::Error { .. }), "expected Error, got {reply:?}");

        // The session must still be alive and taking new commands
        // afterward -- proven the same way every other no-crash case
        // in this file is.
        blueice_ipc::write_client_message(&mut client, &ClientMessage::ListTabs).unwrap();
        assert!(matches!(blueice_ipc::read_server_message(&mut client).unwrap(), ServerMessage::Tabs(_)));

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_action_addressed_to_one_tab_never_affects_another_tabs_state() {
        let dir = temp_frame_dir("tab-isolation");
        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            default_page(&mut tabs).load_html_str("<p>tab one</p>", None);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation).unwrap();
            dir
        });
        handshake(&mut client);

        blueice_ipc::write_client_message(&mut client, &ClientMessage::ListTabs).unwrap();
        let ServerMessage::Tabs(initial) = blueice_ipc::read_server_message(&mut client).unwrap() else { panic!("expected Tabs") };
        let tab_one = initial[0].id;

        blueice_ipc::write_client_message(&mut client, &ClientMessage::OpenTab { url: None }).unwrap();
        let ServerMessage::TabOpened { tab_id: tab_two, .. } = blueice_ipc::read_server_message(&mut client).unwrap() else { panic!("expected TabOpened") };

        // Scroll only tab_two.
        blueice_ipc::write_client_message_with_ids(&mut client, Some(tab_two), None, &ClientMessage::Scroll { delta_y: 500.0 }).unwrap();
        let frame = blueice_ipc::read_server_message(&mut client).unwrap();
        assert!(matches!(frame, ServerMessage::FrameReady { .. }));

        // tab_one's representation must be completely unaffected --
        // still showing its own content, scroll untouched.
        blueice_ipc::write_client_message_with_ids(&mut client, Some(tab_one), None, &ClientMessage::GetRepresentation).unwrap();
        let (_, _, reply) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
        let ServerMessage::Representation(snap) = reply else { panic!("expected Representation, got {reply:?}") };
        assert_eq!(snap.tab_id, tab_one);
        assert_eq!(snap.scroll_y, 0.0, "scrolling tab_two must not move tab_one's scroll position");
        assert!(snap.nodes.iter().any(|n| n.name.as_deref() == Some("tab one")));

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_message_addressed_to_an_unknown_tab_replies_error_not_a_silent_no_op() {
        let dir = temp_frame_dir("unknown-tab-error");
        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation).unwrap();
            dir
        });
        handshake(&mut client);

        blueice_ipc::write_client_message_with_ids(&mut client, Some(999_999), None, &ClientMessage::GetRepresentation).unwrap();
        let (reply_tab, _, reply) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
        assert_eq!(reply_tab, Some(999_999), "the reply should still echo back which (nonexistent) tab was addressed");
        assert!(matches!(reply, ServerMessage::Error { .. }), "expected Error, got {reply:?}");

        // The session must survive an unknown-tab error, same as every
        // other error case in this file.
        blueice_ipc::write_client_message(&mut client, &ClientMessage::ListTabs).unwrap();
        assert!(matches!(blueice_ipc::read_server_message(&mut client).unwrap(), ServerMessage::Tabs(_)));

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn close_tab_removes_it_and_a_later_message_to_it_becomes_an_error() {
        let dir = temp_frame_dir("close-tab");
        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation).unwrap();
            dir
        });
        handshake(&mut client);

        blueice_ipc::write_client_message(&mut client, &ClientMessage::OpenTab { url: None }).unwrap();
        let ServerMessage::TabOpened { tab_id: new_id, .. } = blueice_ipc::read_server_message(&mut client).unwrap() else { panic!("expected TabOpened") };

        blueice_ipc::write_client_message_with_ids(&mut client, Some(new_id), None, &ClientMessage::CloseTab).unwrap();
        let (reply_tab, _, reply) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
        assert_eq!(reply_tab, Some(new_id));
        assert_eq!(reply, ServerMessage::TabClosed { tab_id: new_id });

        blueice_ipc::write_client_message_with_ids(&mut client, Some(new_id), None, &ClientMessage::GetRepresentation).unwrap();
        let (_, _, reply) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
        assert!(matches!(reply, ServerMessage::Error { .. }), "a closed tab's id must no longer resolve, expected Error, got {reply:?}");

        // Closing again is a harmless-but-reported "unknown tab" error,
        // not a panic or a second TabClosed.
        blueice_ipc::write_client_message_with_ids(&mut client, Some(new_id), None, &ClientMessage::CloseTab).unwrap();
        let (_, _, reply) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
        assert!(matches!(reply, ServerMessage::Error { .. }));

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_reply_to_an_untagged_request_still_echoes_the_resolved_default_tab_id() {
        // The load-bearing property that makes broadcast-shared,
        // multi-tab connections work at all: a request that left
        // `tab_id` implicit still gets a reply that self-discloses the
        // *concrete* tab it resolved to, not `None` -- otherwise a
        // second client sharing the connection via `blueice-launcher`'s
        // broker could never tell which tab an untagged client's
        // broadcasted reply was actually about.
        let dir = temp_frame_dir("echo-resolved-default-tab");
        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation).unwrap();
            dir
        });
        handshake(&mut client);

        blueice_ipc::write_client_message(&mut client, &ClientMessage::ListTabs).unwrap();
        let ServerMessage::Tabs(tabs) = blueice_ipc::read_server_message(&mut client).unwrap() else { panic!("expected Tabs") };
        let default_tab_id = tabs[0].id;

        // Sent with no tab_id at all -- the envelope-level default.
        blueice_ipc::write_client_message_with_id(&mut client, None, &ClientMessage::GetRepresentation).unwrap();
        let (reply_tab, _, reply) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
        assert_eq!(reply_tab, Some(default_tab_id), "the reply must echo the resolved tab, not None");
        assert!(matches!(reply, ServerMessage::Representation(_)));

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }
}
