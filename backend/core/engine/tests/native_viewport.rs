// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![cfg(unix)]
use blueice_engine::{session::run_session, HistorySnapshotMode, TabManager};
use blueice_ipc::input::{TextInputAction, TextInputContext};
use blueice_ipc::viewport::{DisplayViewport, ViewportState};
use blueice_ipc::{AiSnapshot, ClientMessage, ServerMessage};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

struct Browser {
    client: UnixStream,
    worker: Option<JoinHandle<()>>,
    root: PathBuf,
    request: u64,
    pixels: Vec<u8>,
    width: u32,
    height: u32,
}
impl Browser {
    fn new() -> Self {
        Self::with_size(300.0, 200.0)
    }
    fn with_size(width: f64, height: f64) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "bi-viewport-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        let mut tabs = TabManager::new_with_history_snapshot_mode(
            width,
            height,
            HistorySnapshotMode::Snapshot,
        );
        tabs.get_mut(tabs.default_tab()).unwrap().load_html_str("<div style='width:100px;height:40px;background-color:#006400'></div><input aria-label='Editor' value='frost' style='display:block;width:100px;height:32px'><p aria-label='Wrapped'>one two three four five six seven eight nine ten eleven twelve thirteen</p><div style='height:800px'></div><p>Lower frost</p>",Some("https://viewport.test/one".into()));
        tabs.open_tab();
        let (mut client, mut server) = UnixStream::pair().unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let directory = root.clone();
        let worker = thread::spawn(move || {
            run_session(
                &mut tabs,
                &mut server,
                &directory,
                &mut 0,
                &directory.join("unused.sock"),
            )
            .unwrap()
        });
        blueice_ipc::client_handshake(&mut client).unwrap();
        Self {
            client,
            worker: Some(worker),
            root,
            request: 0,
            pixels: vec![],
            width: 0,
            height: 0,
        }
    }
    fn request(&mut self, tab: u64, command: ClientMessage) -> ServerMessage {
        let viewport = matches!(
            command,
            ClientMessage::SetViewport { .. }
                | ClientMessage::SetPageZoom { .. }
                | ClientMessage::GetViewportState
        );
        self.request += 1;
        blueice_ipc::write_client_message_with_ids(
            &mut self.client,
            Some(tab),
            Some(self.request),
            &command,
        )
        .unwrap();
        loop {
            let (reply_tab, request, message) =
                blueice_ipc::read_server_message_with_ids(&mut self.client).unwrap();
            if let ServerMessage::FrameReady {
                ref shm_path,
                width,
                height,
                ..
            } = message
            {
                if reply_tab == Some(tab) && request == Some(self.request) {
                    self.pixels = std::fs::read(shm_path).unwrap();
                    self.width = width;
                    self.height = height;
                    assert_eq!(self.pixels.len(), width as usize * height as usize * 4);
                }
                continue;
            }
            if matches!(message, ServerMessage::NavigationStarted { .. }) {
                continue;
            }
            if matches!(message, ServerMessage::ViewportState(_))
                && (!viewport || reply_tab != Some(tab) || request != Some(self.request))
            {
                continue;
            }
            assert_eq!(request, Some(self.request));
            if !matches!(message, ServerMessage::TabOpened { .. }) {
                assert_eq!(reply_tab, Some(tab));
            }
            return message;
        }
    }
    fn state(&mut self, tab: u64) -> ViewportState {
        let ServerMessage::ViewportState(s) = self.request(tab, ClientMessage::GetViewportState)
        else {
            panic!("viewport state")
        };
        s
    }
    fn configure(&mut self, scale: f64) -> ViewportState {
        let ServerMessage::ViewportState(s) = self.request(
            1,
            ClientMessage::SetViewport {
                viewport: DisplayViewport {
                    width: 300.0,
                    height: 200.0,
                    device_scale: scale,
                    backing_scale: None,
                },
            },
        ) else {
            panic!("configure")
        };
        s
    }
    fn zoom(&mut self, zoom: f64) -> ViewportState {
        let ServerMessage::ViewportState(s) = self.request(1, ClientMessage::SetPageZoom { zoom })
        else {
            panic!("zoom")
        };
        s
    }
    fn snapshot(&mut self) -> AiSnapshot {
        let ServerMessage::Representation(s) = self.request(1, ClientMessage::GetRepresentation)
        else {
            panic!("snapshot")
        };
        s
    }
    fn pixel(&self, x: u32, y: u32) -> &[u8] {
        let i = ((y * self.width + x) * 4) as usize;
        &self.pixels[i..i + 4]
    }
}
impl Drop for Browser {
    fn drop(&mut self) {
        let _ = self.client.shutdown(std::net::Shutdown::Both);
        let result = self.worker.take().unwrap().join();
        if !thread::panicking() {
            result.unwrap();
        }
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn density_changes_pixels_without_css_reflow_and_zoom_changes_layout() {
    let mut browser = Browser::new();
    let one = browser.configure(1.0);
    let before = browser.snapshot();
    let two = browser.configure(2.0);
    let after = browser.snapshot();
    assert_eq!((one.css_width, one.css_height), (300.0, 200.0));
    assert_eq!((two.pixel_width, two.pixel_height), (600, 400));
    let field = |s: &AiSnapshot| {
        s.nodes
            .iter()
            .find(|n| n.name.as_deref() == Some("Editor"))
            .unwrap()
            .bounds
    };
    assert_eq!(field(&before), field(&after));
    assert_eq!(browser.pixel(20, 20), &[0, 100, 0, 255]);
    let wrapped = |s: &AiSnapshot| {
        s.nodes
            .iter()
            .find(|n| n.name.as_deref() == Some("Wrapped"))
            .unwrap()
            .bounds
            .height
    };
    let zoom = browser.zoom(2.0);
    let large = browser.snapshot();
    assert_eq!(zoom.css_width, 150.0);
    assert_eq!((zoom.pixel_width, zoom.pixel_height), (600, 400));
    assert!(wrapped(&large) > wrapped(&after));
    assert_eq!(field(&large).width, field(&after).width);
    assert_eq!(browser.pixel(199, 20), &[0, 100, 0, 255]);
    assert_eq!(browser.pixel(399, 20), &[0, 100, 0, 255]);
    assert_ne!(browser.pixel(400, 20), &[0, 100, 0, 255]);
}

#[test]
fn zoom_rejects_an_unbounded_legacy_viewport_before_opting_into_native_raster() {
    let mut browser = Browser::with_size(4097.0, 20.0);
    assert!(matches!(
        browser.request(1, ClientMessage::SetPageZoom { zoom: 1.1 }),
        ServerMessage::Error { .. }
    ));
    let unchanged = browser.state(1);
    assert_eq!(unchanged.zoom, 1.0);
    assert_eq!(unchanged.width, 4097.0);
    let repaired = browser.configure(2.0);
    assert_eq!(repaired.pixel_width, 600);
    assert_eq!(browser.zoom(1.1).zoom, 1.1);
}

#[test]
fn fractional_viewports_and_zoom_keep_frame_dimensions_equal_to_metadata() {
    let mut browser = Browser::new();
    for viewport in [
        DisplayViewport {
            width: 300.0000000001,
            height: 200.0000000001,
            device_scale: 1.0,
            backing_scale: None,
        },
        DisplayViewport {
            width: 333.5,
            height: 101.25,
            device_scale: 1.5,
            backing_scale: None,
        },
        DisplayViewport {
            width: 4096.0,
            height: 20.0,
            device_scale: 1.0,
            backing_scale: None,
        },
    ] {
        assert!(matches!(
            browser.request(1, ClientMessage::SetViewport { viewport }),
            ServerMessage::ViewportState(_)
        ));
        for zoom in [0.25, 1.0, 1.1, 1.75, 5.0] {
            let state = browser.zoom(zoom);
            let expected = (
                (viewport.width * viewport.device_scale).ceil() as u32,
                (viewport.height * viewport.device_scale).ceil() as u32,
            );
            assert_eq!((browser.width, browser.height), expected);
            assert_eq!((state.pixel_width, state.pixel_height), expected);
            assert!((state.css_width * zoom - viewport.width).abs() < 1e-9);
        }
    }
}

#[test]
fn zoom_is_per_tab_survives_navigation_history_and_new_tabs_start_at_one() {
    let mut browser = Browser::new();
    browser.configure(2.0);
    browser.zoom(1.5);
    assert_eq!(browser.state(2).zoom, 1.0);
    assert!(matches!(
        browser.request(
            1,
            ClientMessage::Navigate {
                url: "about:credits".into()
            }
        ),
        ServerMessage::Navigated { .. }
    ));
    assert_eq!(browser.state(1).zoom, 1.5);
    assert!(matches!(
        browser.request(1, ClientMessage::GoBack),
        ServerMessage::Navigated { .. }
    ));
    assert_eq!(browser.state(1).zoom, 1.5);
    let ServerMessage::TabOpened { tab_id, .. } =
        browser.request(1, ClientMessage::OpenTab { url: None })
    else {
        panic!("new tab")
    };
    let new = browser.state(tab_id);
    assert_eq!(new.zoom, 1.0);
    assert_eq!(new.device_scale, 2.0);
    browser.configure(1.0);
    assert_eq!(browser.state(1).zoom, 1.5);
    assert_eq!(browser.state(2).device_scale, 1.0);
}

#[test]
fn invalid_zoom_scale_dimensions_and_closed_tabs_preserve_live_state() {
    let mut browser = Browser::new();
    let before = browser.configure(2.0);
    for zoom in [0.0, 0.249, 5.001, 1000.0] {
        assert!(matches!(
            browser.request(1, ClientMessage::SetPageZoom { zoom }),
            ServerMessage::Error { .. }
        ));
    }
    for viewport in [
        DisplayViewport {
            width: 0.0,
            height: 200.0,
            device_scale: 2.0,
            backing_scale: None,
        },
        DisplayViewport {
            width: 2049.0,
            height: 200.0,
            device_scale: 2.0,
            backing_scale: None,
        },
        DisplayViewport {
            width: 300.0,
            height: 200.0,
            device_scale: 5.0,
            backing_scale: None,
        },
    ] {
        assert!(matches!(
            browser.request(1, ClientMessage::SetViewport { viewport }),
            ServerMessage::Error { .. }
        ));
    }
    assert_eq!(browser.state(1), before);
    assert!(matches!(
        browser.request(2, ClientMessage::CloseTab),
        ServerMessage::TabClosed { .. }
    ));
    assert!(matches!(
        browser.request(2, ClientMessage::SetPageZoom { zoom: 2.0 }),
        ServerMessage::Error { .. }
    ));
}

#[test]
fn editing_and_find_geometry_remain_css_coordinates_at_zoom_and_retina_density() {
    use blueice_ipc::find::FindAction;
    let mut browser = Browser::new();
    browser.configure(2.0);
    browser.zoom(2.0);
    let s = browser.snapshot();
    let field = s
        .nodes
        .iter()
        .find(|n| n.name.as_deref() == Some("Editor"))
        .unwrap();
    let ServerMessage::ContextMenu(menu) = browser.request(
        1,
        ClientMessage::GetContextMenu {
            tab_id: 1,
            frame_source: s.frame_source,
            frame_generation: s.generation,
            x: field.bounds.x + 20.0,
            y: field.bounds.y + 10.0,
        },
    ) else {
        panic!("editor menu")
    };
    let input = menu.input.unwrap();
    let context = TextInputContext {
        version: input.version,
        frame_source: input.frame_source,
        document_generation: input.document_generation,
        focus_generation: input.focus_generation,
    };
    assert_eq!(input.focused.unwrap().bounds.width, 100.0);
    assert!(matches!(
        browser.request(
            1,
            ClientMessage::TextInput {
                context,
                action: TextInputAction::SelectAll
            }
        ),
        ServerMessage::TextInputState(_)
    ));
    assert!(matches!(
        browser.request(
            1,
            ClientMessage::TextInput {
                context,
                action: TextInputAction::Replace {
                    text: "new frost".into(),
                    replacement: None
                }
            }
        ),
        ServerMessage::TextInputState(_)
    ));
    let ServerMessage::FindState(find) = browser.request(
        1,
        ClientMessage::Find {
            tab_id: 1,
            frame_source: context.frame_source,
            document_generation: context.document_generation,
            action: FindAction::Update {
                query: "frost".into(),
                case_sensitive: false,
            },
        },
    ) else {
        panic!("find")
    };
    assert_eq!(find.match_count, 2);
    assert!(find.rects[0].width < 100.0);
    let ServerMessage::FindState(last) = browser.request(
        1,
        ClientMessage::Find {
            tab_id: 1,
            frame_source: context.frame_source,
            document_generation: context.document_generation,
            action: FindAction::Next { backwards: false },
        },
    ) else {
        panic!("next")
    };
    assert_eq!(last.active_match, Some(2));
    assert!(browser.snapshot().scroll_y > 400.0);
    assert!(
        browser
            .pixels
            .chunks_exact(4)
            .any(|p| p[0] > 200 && p[1] > 100 && p[1] < 210 && p[2] < 80),
        "Active find paint must be visible in the scaled viewport"
    );
}
