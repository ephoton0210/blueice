// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_engine::Page;
use blueice_ipc::printing::PrintProfile;
use blueice_paint::PaintCommand;

fn profile() -> PrintProfile {
    PrintProfile {
        width_points: 225.0,
        height_points: 150.0,
        backgrounds: true,
    }
}

#[test]
fn print_reflows_frozen_core_document_and_preserves_live_page() {
    let mut page = Page::new(800.0, 600.0);
    page.load_html_str("<style>#screen{display:block}#paper{display:none}@media print{#screen{display:none}#paper{display:block;color:#ff0000}}</style><p id='screen'>SCREEN ONLY</p><p id='paper'>PAPER ONLY</p><input type='password' value='secret-original'>", None);
    let before = page.render();
    let frozen = page.capture_print_document();
    page.load_html_str("<p>REPLACED</p>", None);
    let printed = frozen.layout(profile()).unwrap();
    let text: String = printed
        .frame
        .commands
        .iter()
        .filter_map(|c| {
            if let PaintCommand::Text { text, .. } = c {
                Some(text.as_str())
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    assert!(text.contains("PAPER"));
    assert!(!text.contains("SCREEN"));
    assert!(!text.contains("REPLACED"));
    assert!(!text.contains("secret-original"));
    assert_eq!(printed.frame.width, 300.0);
    assert!(before
        .commands
        .iter()
        .any(|c| matches!(c,PaintCommand::Text{text,..} if text.contains("SCREEN"))));
    let before = page.render();
    page.capture_print_document().layout(profile()).unwrap();
    assert_eq!(before, page.render());
}

#[test]
fn pagination_preserves_lines_and_refuses_truncation_and_invalid_profiles() {
    let mut page = Page::new(800.0, 600.0);
    page.load_html_str(
        &format!(
            "<style>p{{margin:0;line-height:30px}}</style>{}",
            "<p>Pagination line</p>".repeat(21)
        ),
        None,
    );
    let frozen = page.capture_print_document();
    let printed = frozen.layout(profile()).unwrap();
    assert!(printed.slices.len() > 1);
    for pair in printed.slices.windows(2) {
        assert_eq!(pair[0].0 + pair[0].1, pair[1].0);
    }
    assert!(printed
        .slices
        .iter()
        .all(|(_, height)| *height <= 200.0 && *height > 0.0));
    for bad in [0.0, -1.0, f64::NAN, f64::INFINITY, 2000.0] {
        assert!(frozen
            .layout(PrintProfile {
                width_points: bad,
                ..profile()
            })
            .is_err());
    }
    page.load_html_str("<div style='height:100000px'>Too long</div>", None);
    assert!(page.capture_print_document().layout(profile()).is_err());
}

#[test]
fn paper_background_option_keeps_text_and_form_control_paint() {
    let mut page = Page::new(300.0, 200.0);
    page.load_html_str("<div style='background-color:#ff0000;height:20px'>Ink</div><input type='checkbox' checked>",None);
    let frozen = page.capture_print_document();
    let on = frozen.layout(profile()).unwrap();
    let off = frozen
        .layout(PrintProfile {
            backgrounds: false,
            ..profile()
        })
        .unwrap();
    let red = |frame: &blueice_paint::Frame| {
        frame.commands.iter().any(|c|matches!(c,PaintCommand::Rect{color,..} if *color==blueice_css::Color::Rgba(255,0,0,255)))
    };
    assert!(red(&on.frame));
    assert!(!red(&off.frame));
    assert!(off
        .frame
        .commands
        .iter()
        .any(|c| matches!(c,PaintCommand::Text{text,..} if text=="Ink")));
    assert!(off
        .frame
        .commands
        .iter()
        .any(|c| matches!(c, PaintCommand::BorderEdge { .. })));
}

#[cfg(unix)]
mod protocol {
    use super::*;
    use blueice_engine::{session::run_session, TabManager};
    use blueice_ipc::{
        printing::{PrintAction, PrintReply},
        ClientMessage, ServerMessage,
    };
    use std::{os::unix::net::UnixStream, path::PathBuf, thread::JoinHandle};
    struct Browser {
        client: UnixStream,
        worker: Option<JoinHandle<()>>,
        root: PathBuf,
        id: u64,
    }
    impl Browser {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "bi-print-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir(&root).unwrap();
            let mut tabs = TabManager::new(300.0, 200.0);
            tabs.get_mut(tabs.default_tab()).unwrap().load_html_str(
                "<style>@media print {p {color:red}}</style><p>Original document</p>",
                None,
            );
            let (mut client, mut server) = UnixStream::pair().unwrap();
            client
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let dir = root.clone();
            let worker = std::thread::spawn(move || {
                run_session(
                    &mut tabs,
                    &mut server,
                    &dir,
                    &mut 0,
                    &dir.join("unused.sock"),
                )
                .unwrap()
            });
            blueice_ipc::client_handshake(&mut client).unwrap();
            Self {
                client,
                worker: Some(worker),
                root,
                id: 0,
            }
        }
        fn send(&mut self, tab: u64, msg: ClientMessage) -> ServerMessage {
            self.id += 1;
            blueice_ipc::write_client_message_with_ids(
                &mut self.client,
                Some(tab),
                Some(self.id),
                &msg,
            )
            .unwrap();
            loop {
                let (t, id, msg) =
                    blueice_ipc::read_server_message_with_ids(&mut self.client).unwrap();
                if id == Some(self.id)
                    && !matches!(
                        msg,
                        ServerMessage::FrameReady { .. }
                            | ServerMessage::TextInputState(_)
                            | ServerMessage::ViewportState(_)
                    )
                {
                    if !matches!(msg, ServerMessage::TabOpened { .. }) {
                        assert_eq!(t, Some(tab));
                    }
                    return msg;
                }
            }
        }
    }
    impl Drop for Browser {
        fn drop(&mut self) {
            let _ = self.client.shutdown(std::net::Shutdown::Both);
            let r = self.worker.take().unwrap().join();
            if !std::thread::panicking() {
                r.unwrap();
            }
            assert!(!std::fs::read_dir(&self.root).unwrap().any(|f| f
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("print-")));
            std::fs::remove_dir_all(&self.root).unwrap();
        }
    }
    #[test]
    fn print_protocol_is_correlated_bounded_tab_fenced_and_cleans_pixels() {
        let mut browser = Browser::new();
        let source = blueice_ipc::shm::frame_source_id(&browser.root);
        let ServerMessage::PrintState(PrintReply::Begun { ticket, .. }) = browser.send(
            1,
            ClientMessage::Print(PrintAction::Begin {
                frame_source: source,
                document_generation: 1,
            }),
        ) else {
            panic!("begin")
        };
        let ServerMessage::PrintState(PrintReply::Rendered {
            pages, revision, ..
        }) = browser.send(
            1,
            ClientMessage::Print(PrintAction::Render {
                ticket: ticket.clone(),
                profile: profile(),
            }),
        )
        else {
            panic!("render")
        };
        assert!(matches!(
            browser.send(
                1,
                ClientMessage::Print(PrintAction::Validate {
                    ticket: ticket.clone()
                })
            ),
            ServerMessage::PrintState(PrintReply::Validated { .. })
        ));
        assert_eq!(revision, 1);
        assert_eq!(pages.len(), 1);
        assert_eq!(
            std::fs::metadata(&pages[0].shm_path).unwrap().len(),
            u64::from(pages[0].width) * u64::from(pages[0].height) * 4
        );
        assert!(matches!(
            browser.send(
                1,
                ClientMessage::Print(PrintAction::Render {
                    ticket: ticket.clone(),
                    profile: PrintProfile {
                        width_points: 0.0,
                        ..profile()
                    }
                })
            ),
            ServerMessage::Error { .. }
        ));
        let ServerMessage::TabOpened { tab_id, .. } =
            browser.send(1, ClientMessage::OpenTab { url: None })
        else {
            panic!("open")
        };
        assert!(matches!(
            browser.send(
                tab_id,
                ClientMessage::Print(PrintAction::Render {
                    ticket: ticket.clone(),
                    profile: profile()
                })
            ),
            ServerMessage::Error { .. }
        ));
        assert!(matches!(
            browser.send(
                tab_id,
                ClientMessage::Print(PrintAction::End {
                    ticket: ticket.clone()
                })
            ),
            ServerMessage::Error { .. }
        ));
        assert!(std::path::Path::new(&pages[0].shm_path).exists());
        assert!(matches!(
            browser.send(
                1,
                ClientMessage::Print(PrintAction::End {
                    ticket: ticket.clone()
                })
            ),
            ServerMessage::PrintState(PrintReply::Ended { .. })
        ));
        assert!(!std::path::Path::new(&pages[0].shm_path).exists());
        assert!(matches!(
            browser.send(
                1,
                ClientMessage::Print(PrintAction::Render {
                    ticket,
                    profile: profile()
                })
            ),
            ServerMessage::Error { .. }
        ));
        let first = browser.send(
            1,
            ClientMessage::Print(PrintAction::Begin {
                frame_source: source,
                document_generation: 1,
            }),
        );
        assert!(matches!(first, ServerMessage::PrintState(_)));
        assert!(matches!(
            browser.send(
                1,
                ClientMessage::Print(PrintAction::Begin {
                    frame_source: source,
                    document_generation: 1
                })
            ),
            ServerMessage::PrintState(_)
        ));
        assert!(matches!(
            browser.send(
                1,
                ClientMessage::Print(PrintAction::Begin {
                    frame_source: source,
                    document_generation: 1
                })
            ),
            ServerMessage::Error { .. }
        ));
        assert!(matches!(
            browser.send(
                1,
                ClientMessage::Print(PrintAction::Begin {
                    frame_source: source + 1,
                    document_generation: 1
                })
            ),
            ServerMessage::Error { .. }
        ));
        let ServerMessage::PrintState(PrintReply::Begun { ticket, .. }) = first else {
            unreachable!()
        };
        assert!(matches!(
            browser.send(
                1,
                ClientMessage::Navigate {
                    url: "about:credits".into()
                }
            ),
            ServerMessage::Navigated { .. }
        ));
        assert!(matches!(
            browser.send(
                1,
                ClientMessage::Print(PrintAction::Render {
                    ticket,
                    profile: profile()
                })
            ),
            ServerMessage::Error { .. }
        ));
    }
}
