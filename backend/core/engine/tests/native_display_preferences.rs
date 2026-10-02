// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![cfg(unix)]
use blueice_engine::{session::run_session, HistorySnapshotMode, TabManager};
use blueice_ipc::display::{DisplayPreferences, DisplayPreferencesState};
use blueice_ipc::input::{TextInputAction, TextInputContext};
use blueice_ipc::viewport::DisplayViewport;
use blueice_ipc::{AiSnapshot, ClientMessage, ServerMessage};
use std::{
    os::unix::net::UnixStream,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    thread::{self, JoinHandle},
    time::Duration,
};

const HTML: &str = r#"<style>
#surface {width:100px;height:40px;background-color:#aabbcc}
#dark, #more, #reduce {display:none}
@media (prefers-color-scheme:dark) {#surface {background-color:#102030} #light {display:none} #dark {display:block}}
@media (prefers-contrast:more) {#normal {display:none} #more {display:block}}
@media (prefers-reduced-motion:reduce) {#surface {height:20px} #motion {display:none} #reduce {display:block}}
</style><div id='surface'></div>
<p id='light'>Light content</p><p id='dark'>Dark content</p>
<p id='normal'>Standard contrast</p><p id='more'>Increased contrast</p>
<p id='motion'>Motion allowed</p><p id='reduce'>Reduced motion</p>
<input aria-label='Editor' value='retained 中文' style='display:block;width:160px;height:32px'>
<style media='screen and (prefers-color-scheme:dark)'>#surface {width:120px}</style>"#;

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
    fn new(html: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "bi-display-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        let mut tabs =
            TabManager::new_with_history_snapshot_mode(300.0, 240.0, HistorySnapshotMode::Snapshot);
        tabs.get_mut(tabs.default_tab())
            .unwrap()
            .load_html_str(html, Some("https://display.test/one".into()));
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
        let wants_display = matches!(
            command,
            ClientMessage::SetDisplayPreferences { .. } | ClientMessage::GetDisplayPreferences
        );
        let wants_click = matches!(command, ClientMessage::Click { .. });
        let wants_viewport = matches!(
            command,
            ClientMessage::SetViewport { .. } | ClientMessage::SetPageZoom { .. }
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
            let (reply_tab, id, msg) =
                blueice_ipc::read_server_message_with_ids(&mut self.client).unwrap();
            if let ServerMessage::FrameReady {
                ref shm_path,
                width,
                height,
                ..
            } = msg
            {
                if reply_tab == Some(tab) {
                    self.pixels = std::fs::read(shm_path).unwrap();
                    self.width = width;
                    self.height = height;
                    assert_eq!(self.pixels.len(), width as usize * height as usize * 4);
                }
                if wants_click && reply_tab == Some(tab) && id == Some(self.request) {
                    return msg;
                }
                continue;
            }
            if matches!(msg, ServerMessage::NavigationStarted { .. }) {
                continue;
            }
            if matches!(msg, ServerMessage::DisplayPreferencesState(_))
                && (!wants_display || id != Some(self.request) || reply_tab != Some(tab))
            {
                continue;
            }
            if matches!(msg, ServerMessage::ViewportState(_))
                && (!wants_viewport || id != Some(self.request) || reply_tab != Some(tab))
            {
                continue;
            }
            assert_eq!(id, Some(self.request));
            return msg;
        }
    }
    fn prefs(&mut self, preferences: DisplayPreferences) -> DisplayPreferencesState {
        let ServerMessage::DisplayPreferencesState(state) =
            self.request(1, ClientMessage::SetDisplayPreferences { preferences })
        else {
            panic!("display preferences");
        };
        state
    }
    fn state(&mut self, tab: u64) -> DisplayPreferencesState {
        let ServerMessage::DisplayPreferencesState(s) =
            self.request(tab, ClientMessage::GetDisplayPreferences)
        else {
            panic!("display state");
        };
        s
    }
    fn snapshot(&mut self) -> AiSnapshot {
        let ServerMessage::Representation(s) = self.request(1, ClientMessage::GetRepresentation)
        else {
            panic!("snapshot");
        };
        s
    }
    fn viewport(&mut self, width: f64, scale: f64) {
        assert!(matches!(
            self.request(
                1,
                ClientMessage::SetViewport {
                    viewport: DisplayViewport {
                        width,
                        height: 240.0,
                        device_scale: scale,
                        backing_scale: None,
                    }
                }
            ),
            ServerMessage::ViewportState(_)
        ));
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
fn names(snapshot: &AiSnapshot) -> Vec<&str> {
    snapshot
        .nodes
        .iter()
        .filter_map(|n| n.name.as_deref())
        .collect()
}

#[test]
fn preference_changes_repaint_same_document_and_preserve_editor_focus_and_text() {
    let mut browser = Browser::new(HTML);
    browser.viewport(300.0, 2.0);
    let light = browser.prefs(DisplayPreferences::default());
    assert_eq!(browser.pixel(20, 20), &[170, 187, 204, 255]);
    let snapshot = browser.snapshot();
    let editor = snapshot
        .nodes
        .iter()
        .find(|n| n.name.as_deref() == Some("Editor"))
        .unwrap();
    assert!(matches!(
        browser.request(
            1,
            ClientMessage::Click {
                x: editor.bounds.x + 10.0,
                y: editor.bounds.y + 10.0
            }
        ),
        ServerMessage::FrameReady { .. }
    ));
    let ServerMessage::TextInputState(before) =
        browser.request(1, ClientMessage::GetTextInputState)
    else {
        panic!("editor");
    };
    let dark = browser.prefs(DisplayPreferences {
        dark: true,
        high_contrast: true,
        reduced_motion: true,
    });
    assert!(dark.frame_generation > light.frame_generation);
    assert_eq!(dark.frame_source, light.frame_source);
    assert_eq!(browser.pixel(20, 20), &[16, 32, 48, 255]);
    assert_ne!(browser.pixel(20, 41), &[16, 32, 48, 255]);
    let snapshot = browser.snapshot();
    let n = names(&snapshot);
    for wanted in ["Dark content", "Increased contrast", "Reduced motion"] {
        assert!(n.contains(&wanted), "{n:?}");
    }
    for hidden in ["Light content", "Standard contrast", "Motion allowed"] {
        assert!(!n.contains(&hidden), "{n:?}");
    }
    let ServerMessage::TextInputState(after) = browser.request(1, ClientMessage::GetTextInputState)
    else {
        panic!("editor");
    };
    assert_eq!(after.document_generation, before.document_generation);
    assert_eq!(after.focus_generation, before.focus_generation);
    assert_eq!(
        after.focused.unwrap().text.as_deref(),
        Some("retained 中文")
    );
    let context = TextInputContext {
        version: before.version,
        frame_source: before.frame_source,
        document_generation: before.document_generation,
        focus_generation: before.focus_generation,
    };
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
}

#[test]
fn preferences_reach_new_tabs_navigation_and_retained_history_without_new_documents() {
    let mut browser = Browser::new(HTML);
    let prefs = DisplayPreferences {
        dark: true,
        high_contrast: true,
        reduced_motion: true,
    };
    browser.prefs(prefs);
    let ServerMessage::TabOpened { tab_id, .. } =
        browser.request(1, ClientMessage::OpenTab { url: None })
    else {
        panic!("tab");
    };
    assert_eq!(browser.state(tab_id).preferences, prefs);
    assert!(matches!(
        browser.request(
            1,
            ClientMessage::Navigate {
                url: "about:credits".into()
            }
        ),
        ServerMessage::Navigated { .. }
    ));
    assert_eq!(browser.state(1).preferences, prefs);
    browser.prefs(DisplayPreferences::default());
    assert!(matches!(
        browser.request(1, ClientMessage::GoBack),
        ServerMessage::Navigated { .. }
    ));
    let snapshot = browser.snapshot();
    assert!(names(&snapshot).contains(&"Light content"));
    assert!(!names(&snapshot).contains(&"Dark content"));
    assert_eq!(
        browser.state(tab_id).preferences,
        DisplayPreferences::default()
    );
}

#[test]
fn media_reacts_to_css_size_backing_density_and_zoom_through_the_same_core() {
    let html="<style>#s{width:100px;height:40px;background-color:#ff0000}@media (min-width:300px){#s{background-color:#00ff00}}@media (min-resolution:2dppx){#s{height:20px}}</style><div id='s'></div>";
    let mut browser = Browser::new(html);
    browser.viewport(300.0, 1.0);
    browser.prefs(DisplayPreferences::default());
    assert_eq!(browser.pixel(10, 10), &[0, 255, 0, 255]);
    assert_eq!(browser.pixel(10, 30), &[0, 255, 0, 255]);
    browser.viewport(300.0, 2.0);
    assert_eq!(browser.pixel(20, 20), &[0, 255, 0, 255]);
    assert_ne!(browser.pixel(20, 41), &[0, 255, 0, 255]);
    // A capped raster must not change the real screen resolution media feature.
    let viewport = DisplayViewport {
        width: 300.0,
        height: 240.0,
        device_scale: 1.0,
        backing_scale: Some(2.0),
    };
    let ServerMessage::ViewportState(state) =
        browser.request(1, ClientMessage::SetViewport { viewport })
    else {
        panic!("viewport state");
    };
    assert_eq!(state.backing_scale, Some(2.0));
    assert_eq!(state.pixel_width, 300);
    assert_eq!(browser.pixel(10, 10), &[0, 255, 0, 255]);
    assert_ne!(browser.pixel(10, 21), &[0, 255, 0, 255]);
    for invalid in [0.0, 5.0] {
        assert!(matches!(
            browser.request(
                1,
                ClientMessage::SetViewport {
                    viewport: DisplayViewport {
                        backing_scale: Some(invalid),
                        ..viewport
                    }
                }
            ),
            ServerMessage::Error { .. }
        ));
        assert_ne!(browser.pixel(10, 21), &[0, 255, 0, 255]);
    }
    assert!(matches!(
        browser.request(1, ClientMessage::SetPageZoom { zoom: 2.0 }),
        ServerMessage::ViewportState(_)
    ));
    assert_eq!(browser.pixel(20, 20), &[255, 0, 0, 255]);
}

#[test]
fn closed_tab_preference_requests_cannot_mutate_live_pages() {
    let mut browser = Browser::new(HTML);
    let before = browser.prefs(DisplayPreferences::default());
    let ServerMessage::TabOpened { tab_id, .. } =
        browser.request(1, ClientMessage::OpenTab { url: None })
    else {
        panic!("tab");
    };
    assert!(matches!(
        browser.request(tab_id, ClientMessage::CloseTab),
        ServerMessage::TabClosed { .. }
    ));
    assert!(matches!(
        browser.request(
            tab_id,
            ClientMessage::SetDisplayPreferences {
                preferences: DisplayPreferences {
                    dark: true,
                    high_contrast: true,
                    reduced_motion: true
                }
            }
        ),
        ServerMessage::Error { .. }
    ));
    assert_eq!(browser.state(1), before);
}
