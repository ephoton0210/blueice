// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `blueice-frontend`: the reference human-facing window, per
//! `phase-4-human-rendering-path/PLAN.md`. This is deliberately *a*
//! frontend, not *the* frontend -- `CLAUDE.md`'s core requirement is
//! that `core` owns the one render pass regardless of who's watching,
//! so this binary is only a native window driven by `core` over the
//! real cross-process protocol in `blueice-ipc`, not something
//! `core`'s design depends on. A WinUI3/SwiftUI/Qt frontend later
//! would sit at exactly this same boundary.
//!
//! Frame delivery is push-based, not request/reply: `core` doesn't
//! reply to every `ClientMessage` (a `Click` that doesn't land on a
//! link produces no message at all, per `blueice_engine::session`), so
//! a background thread reads `ServerMessage`s off the socket
//! continuously and wakes the event loop via `EventLoopProxy` --
//! blocking the UI thread on a read that might never come is not an
//! option for a real windowing frontend.
//!
//! Show/hide (`BROWSER_CORE_PLAN.md` §1's "toggle the window without
//! restarting the engine") is demonstrated here via stdin commands
//! (`show`/`hide`/`quit`) standing in for the AI-facing control channel
//! that doesn't exist yet (Phase 5+) -- toggling calls
//! `Window::set_visible` on this process's own window and never
//! touches `core` at all, which is the point: visibility is purely a
//! `frontend`-side, windowing-layer concern.
//!
//! Stdin commands include `credits`, which navigates to `core`'s built-in
//! `about:credits` page (`blueice_engine::credits`) -- BlueIce's
//! Help/About/Credits screen (`phase-4-human-rendering-path/PLAN.md`) -- and
//! Phase 16's `tab-new`/`tab-close`/`tab N` plus group commands. They remain
//! a testable stand-in for native menu/toolbar controls.
//! `frontend` doesn't depend on `blueice-engine` to know that URL --
//! like any other URL sent over `ClientMessage::Navigate`, it's just a
//! string this process and `core` both happen to agree on, the same
//! way a real platform-native frontend (not necessarily even Rust)
//! would.

use blueice_ipc::downloads::{default_downloads_socket_path, DownloadsClient, TransferInfo};
use blueice_ipc::{shm, ClientMessage, ServerMessage, TabGroupSummary, TabSummary};
use softbuffer::{Context, Surface};
use std::collections::{HashMap, HashSet};
use std::io::BufRead;
use std::num::NonZeroU32;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::rc::Rc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowId};

/// Repacks RGBA8 (as produced by `blueice_raster::Pixmap`) into
/// softbuffer's expected `0x00RRGGBB` word per pixel. The page
/// background is always painted fully opaque (`blueice-raster`'s
/// `BACKGROUND` constant), so alpha is never consulted here -- there's
/// nothing underneath a BlueIce frame to blend against.
fn rgba_to_xrgb(pixels: &[u8]) -> Vec<u32> {
    pixels
        .chunks_exact(4)
        .map(|p| (u32::from(p[0]) << 16) | (u32::from(p[1]) << 8) | u32::from(p[2]))
        .collect()
}

/// `core` is expected to sit next to this binary in the same build
/// output directory (both are workspace members landing in the same
/// `target/<profile>/`) -- this avoids requiring a `--core-exe` flag
/// for the common case while still being explicit about the
/// assumption, rather than silently searching `$PATH`.
fn sibling_core_binary(this_exe: &Path) -> PathBuf {
    let name = if cfg!(windows) {
        "blueice-core.exe"
    } else {
        "blueice-core"
    };
    this_exe
        .parent()
        .map(|dir| dir.join(name))
        .unwrap_or_else(|| PathBuf::from(name))
}

/// Picks a supported locale from a `LANG`-shaped environment value
/// (e.g. `zh_TW.UTF-8`, `en_US.UTF-8`) -- normalizes the `_`-separated,
/// encoding-suffixed POSIX locale form to the `-`-separated BCP-47-ish
/// tag `blueice-i18n`'s resources are keyed by, and falls back to
/// [`blueice_i18n::DEFAULT_LOCALE`] for anything unset or not in
/// [`blueice_i18n::SUPPORTED_LOCALES`]. A real platform-native frontend
/// (Windows/macOS) would read its OS's own locale API instead of
/// `$LANG` -- this is the reference frontend's stand-in for that,
/// same relationship stdin's `show`/`hide`/`credits` commands have to
/// a real AI-facing control channel.
fn detect_locale(lang_env: Option<&str>) -> &'static str {
    let Some(lang_env) = lang_env else {
        return blueice_i18n::DEFAULT_LOCALE;
    };
    let tag = lang_env
        .split('.')
        .next()
        .unwrap_or(lang_env)
        .replace('_', "-");
    blueice_i18n::SUPPORTED_LOCALES
        .iter()
        .find(|candidate| candidate.eq_ignore_ascii_case(&tag))
        .copied()
        .unwrap_or(blueice_i18n::DEFAULT_LOCALE)
}

fn unique_socket_path() -> PathBuf {
    // AF_UNIX paths are capped at ~108 bytes (`SUN_LEN`); a plain
    // system temp dir keeps this short regardless of how deep the
    // caller's own working/scratch directories are nested.
    std::env::temp_dir().join(format!("blueice-{}.sock", std::process::id()))
}

fn wait_for_socket(path: &Path, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if path.exists() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

/// Waits for a downloads socket that is actually accepting connections,
/// rather than merely for a pathname left behind by a crashed process.
fn wait_for_downloads_connection(path: &Path, timeout: Duration) -> Option<UnixStream> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok(stream) = UnixStream::connect(path) {
            return Some(stream);
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The well-known URL `core`'s built-in credits page lives at
/// (`blueice_engine::credits::CREDITS_URL`) -- duplicated here rather
/// than imported, per the module docs above.
const CREDITS_URL: &str = "about:credits";

/// The built-in downloads page (`blueice_engine::downloads_page::
/// DOWNLOADS_URL`) -- duplicated for the same reason as `CREDITS_URL`.
const DOWNLOADS_URL: &str = "about:downloads";

/// Native window chrome is local to this frontend. `core` receives page
/// coordinates below it, while the tab strip itself is rendered here from the
/// same core-owned tab/group state an MCP observer can inspect.
const TAB_STRIP_HEIGHT: u32 = 34;
const TAB_WIDTH: u32 = 150;
const GROUP_HEADER_WIDTH: u32 = 104;
const NEW_TAB_WIDTH: u32 = 32;
const CHROME_BG: u32 = 0x0020_2228;
const TAB_BG: u32 = 0x0035_3943;
const SELECTED_TAB_BG: u32 = 0x0053_5968;
const TEXT: u32 = 0x00E8_EAF0;

#[derive(Debug)]
enum UserEvent {
    Server {
        tab_id: Option<u64>,
        request_id: Option<u64>,
        message: ServerMessage,
    },
    Disconnected,
    SetVisible(bool),
    Navigate(String),
    OpenTab,
    CloseSelectedTab,
    SelectTab(u64),
    SetTabGroup {
        tab_id: u64,
        group_id: Option<u64>,
    },
    GroupCommand(ClientMessage),
    ToggleGroup(u64),
    /// `download <url>`: ask the downloads process to fetch a URL.
    StartDownload(String),
    Quit,
}

struct CurrentFrame {
    width: u32,
    height: u32,
    generation: u64,
    pixels_xrgb: Vec<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Rect {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

impl Rect {
    fn contains(self, x: f64, y: f64) -> bool {
        x >= f64::from(self.x)
            && y >= f64::from(self.y)
            && x < f64::from(self.x.saturating_add(self.width))
            && y < f64::from(self.y.saturating_add(self.height))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TabStripHit {
    Select(u64),
    Close(u64),
    ToggleGroup(u64),
    NewTab,
}

#[derive(Debug, Clone)]
enum TabStripItem {
    Group {
        rect: Rect,
        group_id: u64,
        color: u32,
        label: String,
    },
    Tab {
        rect: Rect,
        tab_id: u64,
        selected: bool,
        color: Option<u32>,
        label: String,
    },
}

/// The frontend's display-local tab strip. `selected_tab` is deliberately not
/// sent to core: another observer is free to display a different tab.
struct TabStrip {
    items: Vec<TabStripItem>,
    new_tab: Rect,
}

struct App {
    core: Child,
    writer: UnixStream,
    window: Option<Rc<Window>>,
    surface: Option<Surface<Rc<Window>, Rc<Window>>>,
    frames: HashMap<u64, CurrentFrame>,
    tabs: Vec<TabSummary>,
    groups: Vec<TabGroupSummary>,
    selected_tab: Option<u64>,
    pending_open: HashSet<u64>,
    next_request_id: u64,
    window_size: (u32, u32),
    cursor: (f64, f64),
    locale: &'static str,
}

impl App {
    fn send_unscoped(&mut self, msg: &ClientMessage) {
        if let Err(e) =
            blueice_ipc::write_client_message_with_ids(&mut self.writer, None, None, msg)
        {
            eprintln!("blueice-frontend: failed to send {msg:?}: {e}");
        }
    }

    fn send_selected(&mut self, msg: &ClientMessage) {
        if let Err(e) = blueice_ipc::write_client_message_with_ids(
            &mut self.writer,
            self.selected_tab,
            None,
            msg,
        ) {
            eprintln!("blueice-frontend: failed to send {msg:?}: {e}");
        }
    }

    fn send_selected_to(&mut self, tab_id: u64, msg: &ClientMessage) {
        if let Err(e) =
            blueice_ipc::write_client_message_with_ids(&mut self.writer, Some(tab_id), None, msg)
        {
            eprintln!("blueice-frontend: failed to send {msg:?}: {e}");
        }
    }

    fn send_unscoped_with_request(&mut self, msg: &ClientMessage) -> Option<u64> {
        self.next_request_id += 1;
        let request_id = self.next_request_id;
        match blueice_ipc::write_client_message_with_ids(
            &mut self.writer,
            None,
            Some(request_id),
            msg,
        ) {
            Ok(()) => Some(request_id),
            Err(e) => {
                eprintln!("blueice-frontend: failed to send {msg:?}: {e}");
                None
            }
        }
    }

    fn apply_frame(
        &mut self,
        tab_id: u64,
        shm_path: &str,
        width: u32,
        height: u32,
        generation: u64,
    ) {
        if let Some(existing) = self.frames.get(&tab_id) {
            if generation <= existing.generation {
                return; // stale frame, already superseded
            }
        }
        match shm::map_frame(Path::new(shm_path)) {
            Ok(mapped) => {
                self.frames.insert(
                    tab_id,
                    CurrentFrame {
                        width,
                        height,
                        generation,
                        pixels_xrgb: rgba_to_xrgb(&mapped),
                    },
                );
                self.selected_tab.get_or_insert(tab_id);
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            Err(e) => eprintln!("blueice-frontend: failed to map frame {shm_path}: {e}"),
        }
    }

    fn selected_frame(&self) -> Option<&CurrentFrame> {
        self.selected_tab
            .and_then(|tab_id| self.frames.get(&tab_id))
    }

    fn replace_tabs(&mut self, tabs: Vec<TabSummary>) {
        self.tabs = tabs;
        self.frames
            .retain(|id, _| self.tabs.iter().any(|tab| tab.id == *id));
        if self
            .selected_tab
            .is_some_and(|selected| !self.tabs.iter().any(|tab| tab.id == selected))
        {
            self.selected_tab = self.tabs.first().map(|tab| tab.id);
        }
        if self.selected_tab.is_none() {
            self.selected_tab = self.tabs.first().map(|tab| tab.id);
        }
        self.refresh_title();
        self.request_redraw();
    }

    fn upsert_tab(&mut self, tab: TabSummary) {
        if let Some(existing) = self.tabs.iter_mut().find(|current| current.id == tab.id) {
            *existing = tab;
        } else {
            self.tabs.push(tab);
        }
    }

    fn remove_tab(&mut self, tab_id: u64) {
        self.tabs.retain(|tab| tab.id != tab_id);
        self.frames.remove(&tab_id);
        if self.selected_tab == Some(tab_id) {
            self.selected_tab = self.tabs.first().map(|tab| tab.id);
        }
        self.refresh_title();
        self.request_redraw();
    }

    fn replace_groups(&mut self, groups: Vec<TabGroupSummary>) {
        self.groups = groups;
        self.refresh_title();
        self.request_redraw();
    }

    fn upsert_group(&mut self, group: TabGroupSummary) {
        if let Some(existing) = self
            .groups
            .iter_mut()
            .find(|current| current.id == group.id)
        {
            *existing = group;
        } else {
            self.groups.push(group);
        }
        self.refresh_title();
        self.request_redraw();
    }

    fn close_group(&mut self, group_id: u64) {
        self.groups.retain(|group| group.id != group_id);
        for tab in &mut self.tabs {
            if tab.group_id == Some(group_id) {
                tab.group_id = None;
            }
        }
        self.refresh_title();
        self.request_redraw();
    }

    fn set_tab_group_local(&mut self, tab_id: u64, group_id: Option<u64>) {
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == tab_id) {
            tab.group_id = group_id;
        }
        self.refresh_title();
        self.request_redraw();
    }

    fn request_redraw(&self) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn refresh_title(&self) {
        let Some(window) = &self.window else { return };
        let selected = self
            .selected_tab
            .and_then(|id| self.tabs.iter().find(|tab| tab.id == id));
        let group_name = selected
            .and_then(|tab| tab.group_id)
            .and_then(|id| self.groups.iter().find(|group| group.id == id))
            .map(|group| format!("{} · ", group.name))
            .unwrap_or_default();
        let page = selected
            .and_then(|tab| tab.url.as_deref())
            .unwrap_or("new tab");
        window.set_title(&format!("BlueIce — {group_name}{page}"));
    }

    fn open_tab(&mut self) {
        if let Some(request_id) =
            self.send_unscoped_with_request(&ClientMessage::OpenTab { url: None })
        {
            self.pending_open.insert(request_id);
        }
    }

    fn toggle_group(&mut self, group_id: u64) {
        if let Some(group) = self.groups.iter().find(|group| group.id == group_id) {
            self.send_unscoped(&ClientMessage::SetTabGroupCollapsed {
                group_id,
                collapsed: !group.collapsed,
            });
        }
    }

    fn redraw(&mut self) {
        let Some(window) = &self.window else {
            return;
        };
        let size = window.inner_size();
        let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else {
            return;
        };
        let strip = tab_strip(&self.tabs, &self.groups, self.selected_tab, size.width);
        let pixels = compose_window(size.width, size.height, self.selected_frame(), &strip);
        let Some(surface) = &mut self.surface else {
            return;
        };
        if surface.resize(w, h).is_err() {
            return;
        }
        if let Ok(mut buffer) = surface.buffer_mut() {
            buffer.copy_from_slice(&pixels);
            let _ = buffer.present();
        }
        window.pre_present_notify();
    }
}

impl TabStrip {
    fn hit(&self, x: f64, y: f64) -> Option<TabStripHit> {
        for item in self.items.iter().rev() {
            match item {
                TabStripItem::Group { rect, group_id, .. } if rect.contains(x, y) => {
                    return Some(TabStripHit::ToggleGroup(*group_id));
                }
                TabStripItem::Tab { rect, tab_id, .. } if rect.contains(x, y) => {
                    if x >= f64::from(rect.x.saturating_add(rect.width).saturating_sub(18)) {
                        return Some(TabStripHit::Close(*tab_id));
                    }
                    return Some(TabStripHit::Select(*tab_id));
                }
                _ => {}
            }
        }
        self.new_tab.contains(x, y).then_some(TabStripHit::NewTab)
    }
}

fn tab_strip(
    tabs: &[TabSummary],
    groups: &[TabGroupSummary],
    selected_tab: Option<u64>,
    window_width: u32,
) -> TabStrip {
    let mut items = Vec::new();
    let mut seen_groups = HashSet::new();
    let mut x = 0;
    let available_width = window_width.saturating_sub(NEW_TAB_WIDTH);
    for tab in tabs {
        let group = tab
            .group_id
            .and_then(|group_id| groups.iter().find(|group| group.id == group_id));
        if let Some(group) = group {
            if seen_groups.insert(group.id) {
                push_group_item(&mut items, group, &mut x, available_width);
            }
            if group.collapsed {
                continue;
            }
        }
        let rect = Rect {
            x,
            y: 0,
            width: TAB_WIDTH.min(available_width.saturating_sub(x)),
            height: TAB_STRIP_HEIGHT,
        };
        items.push(TabStripItem::Tab {
            rect,
            tab_id: tab.id,
            selected: selected_tab == Some(tab.id),
            color: group.map(|group| parse_group_color(&group.color)),
            label: tab_label(tab),
        });
        x = x.saturating_add(TAB_WIDTH);
    }
    // An empty group is still useful shared organization, and must remain
    // visible so a human can expand/delete it after an AI creates it.
    for group in groups {
        if seen_groups.insert(group.id) {
            push_group_item(&mut items, group, &mut x, available_width);
        }
    }
    TabStrip {
        items,
        new_tab: Rect {
            x: window_width.saturating_sub(NEW_TAB_WIDTH),
            y: 0,
            width: NEW_TAB_WIDTH,
            height: TAB_STRIP_HEIGHT,
        },
    }
}

fn push_group_item(
    items: &mut Vec<TabStripItem>,
    group: &TabGroupSummary,
    x: &mut u32,
    available_width: u32,
) {
    let rect = Rect {
        x: *x,
        y: 0,
        width: GROUP_HEADER_WIDTH.min(available_width.saturating_sub(*x)),
        height: TAB_STRIP_HEIGHT,
    };
    items.push(TabStripItem::Group {
        rect,
        group_id: group.id,
        color: parse_group_color(&group.color),
        label: if group.collapsed {
            format!("▶ {}", group.name)
        } else {
            format!("▼ {}", group.name)
        },
    });
    *x = x.saturating_add(GROUP_HEADER_WIDTH);
}

fn tab_label(tab: &TabSummary) -> String {
    tab.url
        .as_deref()
        .and_then(|url| url.split("//").nth(1).or(Some(url)))
        .unwrap_or("new tab")
        .chars()
        .take(18)
        .collect()
}

fn parse_group_color(color: &str) -> u32 {
    u32::from_str_radix(color.strip_prefix('#').unwrap_or_default(), 16).unwrap_or(0x007C_8799)
}

fn compose_window(
    width: u32,
    height: u32,
    frame: Option<&CurrentFrame>,
    strip: &TabStrip,
) -> Vec<u32> {
    let mut pixels = vec![0x00FF_FFFF; width as usize * height as usize];
    if let Some(frame) = frame {
        let copy_width = width.min(frame.width) as usize;
        let copy_height = height.saturating_sub(TAB_STRIP_HEIGHT).min(frame.height) as usize;
        for row in 0..copy_height {
            let destination = (row + TAB_STRIP_HEIGHT as usize) * width as usize;
            let source = row * frame.width as usize;
            pixels[destination..destination + copy_width]
                .copy_from_slice(&frame.pixels_xrgb[source..source + copy_width]);
        }
    }
    draw_rect(
        &mut pixels,
        width,
        height,
        Rect {
            x: 0,
            y: 0,
            width,
            height: TAB_STRIP_HEIGHT.min(height),
        },
        CHROME_BG,
    );
    for item in &strip.items {
        match item {
            TabStripItem::Group {
                rect, color, label, ..
            } => {
                draw_rect(&mut pixels, width, height, *rect, 0x002B_303A);
                draw_rect(
                    &mut pixels,
                    width,
                    height,
                    Rect { width: 5, ..*rect },
                    *color,
                );
                draw_label(
                    &mut pixels,
                    width,
                    height,
                    rect.x + 9,
                    rect.y + 12,
                    label,
                    TEXT,
                );
            }
            TabStripItem::Tab {
                rect,
                selected,
                color,
                label,
                ..
            } => {
                draw_rect(
                    &mut pixels,
                    width,
                    height,
                    *rect,
                    if *selected { SELECTED_TAB_BG } else { TAB_BG },
                );
                if let Some(color) = color {
                    draw_rect(
                        &mut pixels,
                        width,
                        height,
                        Rect { height: 3, ..*rect },
                        *color,
                    );
                }
                draw_label(
                    &mut pixels,
                    width,
                    height,
                    rect.x + 7,
                    rect.y + 12,
                    label,
                    TEXT,
                );
                draw_close(
                    &mut pixels,
                    width,
                    height,
                    rect.x + rect.width.saturating_sub(12),
                    12,
                );
            }
        }
    }
    draw_label(
        &mut pixels,
        width,
        height,
        strip.new_tab.x + 11,
        12,
        "+",
        TEXT,
    );
    pixels
}

fn draw_rect(pixels: &mut [u32], width: u32, height: u32, rect: Rect, color: u32) {
    let right = rect.x.saturating_add(rect.width).min(width);
    let bottom = rect.y.saturating_add(rect.height).min(height);
    for y in rect.y.min(height)..bottom {
        let start = y as usize * width as usize + rect.x.min(width) as usize;
        let end = y as usize * width as usize + right as usize;
        pixels[start..end].fill(color);
    }
}

fn draw_close(pixels: &mut [u32], width: u32, height: u32, x: u32, y: u32) {
    for offset in 0..7 {
        set_pixel(pixels, width, height, x + offset, y + offset, TEXT);
        set_pixel(pixels, width, height, x + 6 - offset, y + offset, TEXT);
    }
}

fn draw_label(pixels: &mut [u32], width: u32, height: u32, x: u32, y: u32, text: &str, color: u32) {
    for (index, character) in text.chars().take(15).enumerate() {
        let glyph_x = x + index as u32 * 6;
        for (row, bits) in glyph(character).iter().enumerate() {
            for column in 0..5 {
                if bits & (1 << (4 - column)) != 0 {
                    set_pixel(
                        pixels,
                        width,
                        height,
                        glyph_x + column,
                        y + row as u32,
                        color,
                    );
                }
            }
        }
    }
}

fn set_pixel(pixels: &mut [u32], width: u32, height: u32, x: u32, y: u32, color: u32) {
    if x < width && y < height {
        pixels[y as usize * width as usize + x as usize] = color;
    }
}

fn glyph(character: char) -> [u8; 7] {
    match character.to_ascii_uppercase() {
        'A' => [0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0],
        'B' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110,
        ],
        'C' => [
            0b01111, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b01111,
        ],
        'D' => [
            0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110,
        ],
        'E' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
        ],
        'F' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'G' => [
            0b01111, 0b10000, 0b10000, 0b10111, 0b10001, 0b10001, 0b01111,
        ],
        'H' => [
            0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'I' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b11111,
        ],
        'J' => [
            0b00001, 0b00001, 0b00001, 0b00001, 0b10001, 0b10001, 0b01110,
        ],
        'K' => [
            0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001,
        ],
        'L' => [
            0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111,
        ],
        'M' => [
            0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001,
        ],
        'N' => [
            0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001,
        ],
        'O' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'P' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'Q' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101,
        ],
        'R' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001,
        ],
        'S' => [
            0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        'T' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'U' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'V' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100,
        ],
        'W' => [
            0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b10101, 0b01010,
        ],
        'X' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001,
        ],
        'Y' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'Z' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111,
        ],
        '0' => [
            0b01110, 0b10011, 0b10101, 0b10101, 0b11001, 0b10001, 0b01110,
        ],
        '1' => [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        '2' => [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111,
        ],
        '3' => [
            0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        '4' => [
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ],
        '5' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b00001, 0b00001, 0b11110,
        ],
        '6' => [
            0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
        ],
        '7' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
        '8' => [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
        '9' => [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b11100,
        ],
        '-' => [0, 0, 0, 0b11111, 0, 0, 0],
        '.' => [0, 0, 0, 0, 0, 0b01100, 0b01100],
        '/' => [0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0, 0],
        ':' => [0, 0b01100, 0b01100, 0, 0b01100, 0b01100, 0],
        '+' => [0, 0b00100, 0b00100, 0b11111, 0b00100, 0b00100, 0],
        ' ' => [0; 7],
        _ => [
            0b11111, 0b10001, 0b00110, 0b00100, 0b00110, 0b10001, 0b11111,
        ],
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let title = blueice_i18n::translate(self.locale, "frontend", "window-title-default", &[]);
        let attrs = Window::default_attributes().with_title(&title);
        let window = Rc::new(
            event_loop
                .create_window(attrs)
                .expect("failed to create window"),
        );
        let context = Context::new(window.clone()).expect("failed to create softbuffer context");
        let surface =
            Surface::new(&context, window.clone()).expect("failed to create softbuffer surface");
        self.window_size = (window.inner_size().width, window.inner_size().height);
        self.window = Some(window);
        self.surface = Some(surface);
        let (width, height) = self.window_size;
        if width > 0 && height > TAB_STRIP_HEIGHT {
            self.send_selected(&ClientMessage::Resize {
                width,
                height: height - TAB_STRIP_HEIGHT,
            });
        }
        self.refresh_title();
        self.redraw();
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                self.send_unscoped(&ClientMessage::Shutdown);
                event_loop.exit();
            }
            WindowEvent::Resized(size) if size.width > 0 && size.height > 0 => {
                self.window_size = (size.width, size.height);
                self.send_selected(&ClientMessage::Resize {
                    width: size.width,
                    height: size.height.saturating_sub(TAB_STRIP_HEIGHT).max(1),
                });
            }
            WindowEvent::RedrawRequested => self.redraw(),
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = (position.x, position.y);
                // Forwarded so `core` becomes the single source of
                // truth for "what's hovered" -- see
                // `phase-1-ai-representation-layer/PLAN.md` §4 and
                // `blueice_ipc::ClientMessage::Hover`'s own docs.
                if position.y >= f64::from(TAB_STRIP_HEIGHT) {
                    self.send_selected(&ClientMessage::Hover {
                        x: position.x,
                        y: position.y - f64::from(TAB_STRIP_HEIGHT),
                    });
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                let (x, y) = self.cursor;
                if y < f64::from(TAB_STRIP_HEIGHT) {
                    let strip = tab_strip(
                        &self.tabs,
                        &self.groups,
                        self.selected_tab,
                        self.window_size.0,
                    );
                    match strip.hit(x, y) {
                        Some(TabStripHit::Select(tab_id)) => {
                            self.selected_tab = Some(tab_id);
                            self.refresh_title();
                            self.request_redraw();
                        }
                        Some(TabStripHit::Close(tab_id)) => {
                            self.send_selected_to(tab_id, &ClientMessage::CloseTab);
                        }
                        Some(TabStripHit::ToggleGroup(group_id)) => {
                            self.toggle_group(group_id);
                        }
                        Some(TabStripHit::NewTab) => self.open_tab(),
                        None => {}
                    }
                } else {
                    self.send_selected(&ClientMessage::Click {
                        x,
                        y: y - f64::from(TAB_STRIP_HEIGHT),
                    });
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let delta_y = match delta {
                    MouseScrollDelta::LineDelta(_, y) => -y as f64 * 20.0,
                    MouseScrollDelta::PixelDelta(pos) => -pos.y,
                };
                self.send_selected(&ClientMessage::Scroll { delta_y });
            }
            _ => {}
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Server {
                tab_id,
                request_id,
                message,
            } => match message {
                ServerMessage::FrameReady {
                    shm_path,
                    width,
                    height,
                    generation,
                } => {
                    if let Some(tab_id) = tab_id {
                        self.apply_frame(tab_id, &shm_path, width, height, generation);
                    }
                }
                ServerMessage::Navigated { url } => {
                    if let Some(tab_id) = tab_id {
                        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == tab_id) {
                            tab.url = Some(url);
                        }
                    }
                    self.refresh_title();
                    self.request_redraw();
                }
                ServerMessage::TabOpened { tab_id, url } => {
                    self.upsert_tab(TabSummary {
                        id: tab_id,
                        url,
                        group_id: None,
                    });
                    if request_id.is_some_and(|id| self.pending_open.remove(&id)) {
                        self.selected_tab = Some(tab_id);
                    }
                    self.refresh_title();
                    self.request_redraw();
                }
                ServerMessage::TabClosed { tab_id } => self.remove_tab(tab_id),
                ServerMessage::Tabs(tabs) => self.replace_tabs(tabs),
                ServerMessage::TabGroupCreated(group) | ServerMessage::TabGroupUpdated(group) => {
                    self.upsert_group(group);
                }
                ServerMessage::TabGroupAssigned { tab_id, group_id } => {
                    self.set_tab_group_local(tab_id, group_id);
                }
                ServerMessage::TabGroupClosed { group_id } => self.close_group(group_id),
                ServerMessage::TabGroups(groups) => self.replace_groups(groups),
                ServerMessage::Error { message } => {
                    eprintln!("blueice-frontend: core reported an error: {message}");
                }
                // `phase-7-local-ai/PLAN.md`'s gatekeeper: a navigation
                // `core` didn't let through. This reference frontend has no
                // UI for the "detailed risk explanation" the plan calls
                // for yet -- surfaced the same minimal way `Error` is,
                // pending that real UI work.
                ServerMessage::GatekeeperBlocked {
                    reason,
                    category,
                    url,
                } => {
                    eprintln!(
                        "blueice-frontend: core's gatekeeper blocked {url} ({category}): {reason}"
                    );
                }
                // This reference frontend has no AI-facing consumer of its
                // own -- a Representation/Dom only arrives if something
                // else sharing this connection asked for one. An AI-facing
                // client (or the differential-testing harness,
                // `TEST_PLAN.md`) would consume these directly rather than
                // routing them through a human window. `Hello` past the
                // initial handshake (see `main`) is likewise nothing this
                // window needs to react to -- a second external client
                // sharing this connection via `blueice-launcher`'s broker
                // handshaking on its own doesn't change anything here.
                // `Unknown` is the forward-compatibility fallback (plan §3).
                ServerMessage::Representation(_)
                | ServerMessage::Dom(_)
                | ServerMessage::Hello { .. }
                | ServerMessage::Unknown => {}
            },
            UserEvent::Disconnected => {
                eprintln!("blueice-frontend: core disconnected");
                event_loop.exit();
            }
            UserEvent::SetVisible(visible) => {
                if let Some(window) = &self.window {
                    window.set_visible(visible);
                }
                self.send_unscoped(&ClientMessage::Chrome(
                    blueice_ipc::ChromeCommand::SetVisible(visible),
                ));
            }
            UserEvent::Navigate(url) => self.send_selected(&ClientMessage::Navigate {
                url: navigation_url(&url, self.locale),
            }),
            UserEvent::OpenTab => self.open_tab(),
            UserEvent::CloseSelectedTab => {
                if let Some(tab_id) = self.selected_tab {
                    self.send_selected_to(tab_id, &ClientMessage::CloseTab);
                }
            }
            UserEvent::SelectTab(tab_id) => {
                if self.tabs.iter().any(|tab| tab.id == tab_id) {
                    self.selected_tab = Some(tab_id);
                    self.refresh_title();
                    self.request_redraw();
                } else {
                    eprintln!("blueice-frontend: unknown tab {tab_id}");
                }
            }
            UserEvent::SetTabGroup { tab_id, group_id } => {
                self.send_selected_to(tab_id, &ClientMessage::SetTabGroup { group_id })
            }
            UserEvent::GroupCommand(message) => self.send_unscoped(&message),
            UserEvent::ToggleGroup(group_id) => self.toggle_group(group_id),
            UserEvent::StartDownload(url) => {
                // Blocking I/O (and possibly starting a process): off the UI thread.
                std::thread::spawn(move || {
                    match start_download(&url) {
                    Ok(transfer) => eprintln!(
                        "blueice-frontend: download {} queued for {} (type `downloads` to watch it)",
                        transfer.id, transfer.url
                    ),
                    Err(message) => {
                        eprintln!("blueice-frontend: could not start the download: {message}")
                    }
                }
                });
            }
            UserEvent::Quit => {
                self.send_unscoped(&ClientMessage::Shutdown);
                event_loop.exit();
            }
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        let _ = self.core.wait();
    }
}

/// The URL actually sent for a navigation: the bare downloads page is
/// opened in the window's own language (`about:downloads?lang=<locale>`),
/// everything else exactly as given.
fn navigation_url(url: &str, locale: &str) -> String {
    if url == DOWNLOADS_URL {
        format!("{url}?lang={locale}")
    } else {
        url.to_string()
    }
}

/// `blueice-downloads` is expected to sit next to this binary in the same
/// build output directory, like `core`.
fn sibling_downloads_binary(this_exe: &Path) -> PathBuf {
    let name = if cfg!(windows) {
        "blueice-downloads.exe"
    } else {
        "blueice-downloads"
    };
    this_exe
        .parent()
        .map(|dir| dir.join(name))
        .unwrap_or_else(|| PathBuf::from(name))
}

/// Asks the downloads process at `socket` to fetch `url`, starting it first
/// (via `spawn`) if nothing is listening. The frontend talks to the
/// downloads process directly rather than through `core`
/// (`phase-10-download-manager/PLAN.md`): `core` has no downloads command in
/// its protocol, and a starting download needs no page.
fn start_download_at(
    socket: &Path,
    spawn: &dyn Fn() -> std::io::Result<()>,
    url: &str,
) -> Result<TransferInfo, String> {
    let stream = match UnixStream::connect(socket) {
        Ok(stream) => stream,
        Err(_) => {
            spawn().map_err(|e| {
                format!("the downloads service is not running and could not be started: {e}")
            })?;
            wait_for_downloads_connection(socket, Duration::from_secs(5)).ok_or_else(|| {
                "the downloads service was started but did not start listening in time".to_string()
            })?
        }
    };
    let mut client = DownloadsClient::connect(stream).map_err(|e| e.to_string())?;
    client.start(url, None, false).map_err(|e| e.to_string())
}

/// [`start_download_at`] against the well-known socket, starting the
/// sibling `blueice-downloads` binary if need be.
fn start_download(url: &str) -> Result<TransferInfo, String> {
    let socket = default_downloads_socket_path();
    let spawn = || -> std::io::Result<()> {
        let mut child = Command::new(sibling_downloads_binary(&std::env::current_exe()?))
            .arg("--socket")
            .arg(&socket)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(())
    };
    start_download_at(&socket, &spawn, url)
}

/// Maps one trimmed stdin line to the event it requests, or `None` for
/// a blank/unrecognized line -- split out from [`spawn_stdin_commands`]
/// so this mapping is a plain unit-testable function, not something
/// only exercisable by actually piping into the process's stdin.
fn stdin_line_to_event(line: &str) -> Option<UserEvent> {
    let line = line.trim();
    match line {
        "show" => Some(UserEvent::SetVisible(true)),
        "hide" => Some(UserEvent::SetVisible(false)),
        "credits" => Some(UserEvent::Navigate(CREDITS_URL.to_string())),
        "downloads" => Some(UserEvent::Navigate(DOWNLOADS_URL.to_string())),
        "tab-new" => Some(UserEvent::OpenTab),
        "tab-close" => Some(UserEvent::CloseSelectedTab),
        "quit" => Some(UserEvent::Quit),
        other if other.strip_prefix("tab ").is_some() => other["tab ".len()..]
            .trim()
            .parse()
            .ok()
            .map(UserEvent::SelectTab),
        other if other.strip_prefix("group-new ").is_some() => {
            let words: Vec<_> = other["group-new ".len()..].split_whitespace().collect();
            let (Some((color, name_words)), true) = (words.split_last(), words.len() >= 2) else {
                eprintln!("blueice-frontend: group-new needs a name and #RRGGBB color");
                return None;
            };
            Some(UserEvent::GroupCommand(ClientMessage::CreateTabGroup {
                name: name_words.join(" "),
                color: (*color).to_string(),
            }))
        }
        other if other.strip_prefix("group-add ").is_some() => {
            let mut words = other["group-add ".len()..].split_whitespace();
            match (
                words.next().and_then(|word| word.parse().ok()),
                words.next().and_then(|word| word.parse().ok()),
                words.next(),
            ) {
                (Some(tab_id), Some(group_id), None) => Some(UserEvent::SetTabGroup {
                    tab_id,
                    group_id: Some(group_id),
                }),
                _ => {
                    eprintln!("blueice-frontend: group-add needs <tab-id> <group-id>");
                    None
                }
            }
        }
        other if other.strip_prefix("group-remove ").is_some() => other["group-remove ".len()..]
            .trim()
            .parse()
            .ok()
            .map(|tab_id| UserEvent::SetTabGroup {
                tab_id,
                group_id: None,
            }),
        other if other.strip_prefix("group-rename ").is_some() => {
            let mut words = other["group-rename ".len()..].split_whitespace();
            let group_id = words.next().and_then(|word| word.parse().ok())?;
            let name = words.collect::<Vec<_>>().join(" ");
            (!name.is_empty()).then_some(UserEvent::GroupCommand(ClientMessage::RenameTabGroup {
                group_id,
                name,
            }))
        }
        other if other.strip_prefix("group-color ").is_some() => {
            let mut words = other["group-color ".len()..].split_whitespace();
            match (
                words.next().and_then(|word| word.parse().ok()),
                words.next(),
                words.next(),
            ) {
                (Some(group_id), Some(color), None) => {
                    Some(UserEvent::GroupCommand(ClientMessage::SetTabGroupColor {
                        group_id,
                        color: color.to_string(),
                    }))
                }
                _ => None,
            }
        }
        other if other.strip_prefix("group-collapse ").is_some() => other
            ["group-collapse ".len()..]
            .trim()
            .parse()
            .ok()
            .map(UserEvent::ToggleGroup),
        other if other.strip_prefix("group-close ").is_some() => other["group-close ".len()..]
            .trim()
            .parse()
            .ok()
            .map(|group_id| UserEvent::GroupCommand(ClientMessage::CloseTabGroup { group_id })),
        other
            if other
                .strip_prefix("download")
                .is_some_and(|rest| rest.starts_with(char::is_whitespace)) =>
        {
            let url = other["download".len()..].trim();
            if url.is_empty() {
                None
            } else {
                Some(UserEvent::StartDownload(url.to_string()))
            }
        }
        "download" => {
            eprintln!("blueice-frontend: `download` needs a URL (download <url>)");
            None
        }
        other if !other.is_empty() => {
            eprintln!(
                "blueice-frontend: unrecognized command {other:?} (try tab-new/tab-close/tab <id>/group-new <name> <#RRGGBB>/group-add <tab-id> <group-id>/quit)"
            );
            None
        }
        _ => None,
    }
}

/// Reads visibility/page/download commands plus Phase 16's `tab-*` and
/// `group-*` controls from stdin and forwards them as events -- see module
/// docs for why stdin stands in for a native control channel here.
fn spawn_stdin_commands(proxy: EventLoopProxy<UserEvent>) {
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        for line in stdin.lock().lines().map_while(Result::ok) {
            if let Some(event) = stdin_line_to_event(&line) {
                let is_quit = matches!(event, UserEvent::Quit);
                if proxy.send_event(event).is_err() || is_quit {
                    break;
                }
            }
        }
    });
}

fn spawn_server_reader(mut reader: UnixStream, proxy: EventLoopProxy<UserEvent>) {
    std::thread::spawn(move || loop {
        match blueice_ipc::read_server_message_with_ids(&mut reader) {
            Ok((tab_id, request_id, message)) => {
                if proxy
                    .send_event(UserEvent::Server {
                        tab_id,
                        request_id,
                        message,
                    })
                    .is_err()
                {
                    break;
                }
            }
            Err(_) => {
                let _ = proxy.send_event(UserEvent::Disconnected);
                break;
            }
        }
    });
}

fn main() {
    let url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "https://example.com".to_string());

    let this_exe = std::env::current_exe().expect("failed to resolve own executable path");
    let core_bin = sibling_core_binary(&this_exe);
    let socket_path = unique_socket_path();
    let _ = std::fs::remove_file(&socket_path);

    let core = Command::new(&core_bin)
        .arg("--socket")
        .arg(&socket_path)
        .arg("--width")
        .arg("800")
        .arg("--height")
        .arg("600")
        .spawn()
        .unwrap_or_else(|e| {
            panic!(
                "failed to spawn {} ({e}) -- expected it next to {}",
                core_bin.display(),
                this_exe.display()
            )
        });

    if !wait_for_socket(&socket_path, Duration::from_secs(5)) {
        panic!(
            "blueice-core never created its socket at {}",
            socket_path.display()
        );
    }
    let mut writer = UnixStream::connect(&socket_path).expect("failed to connect to blueice-core");
    // `core` requires the very first message on a fresh connection to
    // be `Hello` (`phase-1-ai-representation-layer/PLAN.md` §3) -- done
    // here, before `spawn_server_reader` starts, so nothing else races
    // to read the handshake reply meant for this call.
    blueice_ipc::client_handshake(&mut writer)
        .expect("blueice-core rejected the protocol_version handshake");
    let reader = writer
        .try_clone()
        .expect("failed to clone the core connection for the reader thread");

    let event_loop = EventLoop::<UserEvent>::with_user_event()
        .build()
        .expect("failed to create the event loop");
    let proxy = event_loop.create_proxy();

    spawn_server_reader(reader, proxy.clone());
    spawn_stdin_commands(proxy);

    let locale = detect_locale(std::env::var("LANG").ok().as_deref());
    let mut app = App {
        core,
        writer,
        window: None,
        surface: None,
        frames: HashMap::new(),
        tabs: Vec::new(),
        groups: Vec::new(),
        selected_tab: None,
        pending_open: HashSet::new(),
        next_request_id: 0,
        window_size: (800, 600),
        cursor: (0.0, 0.0),
        locale,
    };
    app.send_unscoped(&ClientMessage::ListTabs);
    app.send_unscoped(&ClientMessage::ListTabGroups);
    app.send_selected(&ClientMessage::Navigate { url });

    event_loop
        .run_app(&mut app)
        .expect("event loop exited with an error");

    let _ = std::fs::remove_file(&socket_path);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgba_to_xrgb_packs_channels_and_drops_alpha() {
        let pixels = [10, 20, 30, 255, 0, 0, 0, 0, 255, 255, 255, 128];
        let words = rgba_to_xrgb(&pixels);
        assert_eq!(words, vec![0x000A_141E, 0x0000_0000, 0x00FF_FFFF]);
    }

    #[test]
    fn rgba_to_xrgb_matches_hand_computed_word_for_a_known_color() {
        // r=0x12, g=0x34, b=0x56 -> 0x00123456
        let pixels = [0x12, 0x34, 0x56, 0xFF];
        assert_eq!(rgba_to_xrgb(&pixels), vec![0x0012_3456]);
    }

    #[test]
    fn sibling_core_binary_sits_next_to_the_frontend_binary() {
        let exe = PathBuf::from("/some/target/debug/blueice-frontend");
        let core = sibling_core_binary(&exe);
        assert_eq!(core, PathBuf::from("/some/target/debug/blueice-core"));
    }

    #[test]
    fn unique_socket_path_stays_short_enough_for_af_unix() {
        let path = unique_socket_path();
        assert!(
            path.to_string_lossy().len() < 100,
            "AF_UNIX paths are capped around 108 bytes"
        );
    }

    #[test]
    fn wait_for_socket_returns_true_once_the_path_exists() {
        let path = std::env::temp_dir().join(format!("blueice-wait-test-{}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        std::fs::write(&path, b"x").unwrap();
        assert!(wait_for_socket(&path, Duration::from_millis(50)));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn wait_for_socket_times_out_if_the_path_never_appears() {
        let path = std::env::temp_dir().join("blueice-never-appears.sock");
        let _ = std::fs::remove_file(&path);
        assert!(!wait_for_socket(&path, Duration::from_millis(50)));
    }

    #[test]
    fn stdin_show_and_hide_map_to_set_visible_events() {
        assert!(matches!(
            stdin_line_to_event("show"),
            Some(UserEvent::SetVisible(true))
        ));
        assert!(matches!(
            stdin_line_to_event("hide"),
            Some(UserEvent::SetVisible(false))
        ));
    }

    #[test]
    fn stdin_credits_command_navigates_to_the_built_in_credits_page() {
        assert!(
            matches!(stdin_line_to_event("credits"), Some(UserEvent::Navigate(url)) if url == CREDITS_URL)
        );
    }

    #[test]
    fn stdin_quit_command_maps_to_quit() {
        assert!(matches!(stdin_line_to_event("quit"), Some(UserEvent::Quit)));
    }

    #[test]
    fn stdin_tab_commands_keep_selection_frontend_local() {
        assert!(matches!(
            stdin_line_to_event("tab-new"),
            Some(UserEvent::OpenTab)
        ));
        assert!(matches!(
            stdin_line_to_event("tab-close"),
            Some(UserEvent::CloseSelectedTab)
        ));
        assert!(matches!(
            stdin_line_to_event("tab 42"),
            Some(UserEvent::SelectTab(42))
        ));
        assert!(stdin_line_to_event("tab nope").is_none());
    }

    #[test]
    fn stdin_group_commands_address_tabs_and_groups_explicitly() {
        assert!(matches!(
            stdin_line_to_event("group-new Work #4f8cff"),
            Some(UserEvent::GroupCommand(ClientMessage::CreateTabGroup { name, color }))
                if name == "Work" && color == "#4f8cff"
        ));
        assert!(matches!(
            stdin_line_to_event("group-add 2 7"),
            Some(UserEvent::SetTabGroup {
                tab_id: 2,
                group_id: Some(7)
            })
        ));
        assert!(matches!(
            stdin_line_to_event("group-remove 2"),
            Some(UserEvent::SetTabGroup {
                tab_id: 2,
                group_id: None
            })
        ));
        assert!(matches!(
            stdin_line_to_event("group-collapse 7"),
            Some(UserEvent::ToggleGroup(7))
        ));
    }

    #[test]
    fn tab_strip_hides_collapsed_members_but_keeps_group_header_clickable() {
        let tabs = vec![
            TabSummary {
                id: 1,
                url: Some("https://one.example".to_string()),
                group_id: Some(9),
            },
            TabSummary {
                id: 2,
                url: Some("https://two.example".to_string()),
                group_id: None,
            },
        ];
        let groups = vec![TabGroupSummary {
            id: 9,
            name: "Research".to_string(),
            color: "#4f8cff".to_string(),
            collapsed: true,
        }];
        let strip = tab_strip(&tabs, &groups, Some(1), 500);
        assert!(matches!(
            strip.items[0],
            TabStripItem::Group { group_id: 9, .. }
        ));
        assert!(
            !strip
                .items
                .iter()
                .any(|item| matches!(item, TabStripItem::Tab { tab_id: 1, .. })),
            "collapsed group member is hidden from this frontend's strip"
        );
        assert_eq!(strip.hit(10.0, 10.0), Some(TabStripHit::ToggleGroup(9)));
    }

    #[test]
    fn compose_window_reserves_tab_strip_and_offsets_the_selected_frame() {
        let frame = CurrentFrame {
            width: 2,
            height: 2,
            generation: 1,
            pixels_xrgb: vec![0x0011_2233; 4],
        };
        let strip = tab_strip(&[], &[], None, 2);
        let pixels = compose_window(2, TAB_STRIP_HEIGHT + 2, Some(&frame), &strip);
        assert_eq!(pixels[0], CHROME_BG);
        assert_eq!(pixels[TAB_STRIP_HEIGHT as usize * 2], 0x0011_2233);
    }

    #[test]
    fn stdin_commands_are_trimmed_of_surrounding_whitespace() {
        assert!(matches!(
            stdin_line_to_event("  credits  "),
            Some(UserEvent::Navigate(_))
        ));
    }

    #[test]
    fn blank_stdin_line_produces_no_event() {
        assert!(stdin_line_to_event("").is_none());
        assert!(stdin_line_to_event("   ").is_none());
    }

    #[test]
    fn unrecognized_stdin_command_produces_no_event() {
        assert!(stdin_line_to_event("bogus").is_none());
    }

    #[test]
    fn detect_locale_normalizes_a_posix_style_lang_value() {
        assert_eq!(detect_locale(Some("zh_TW.UTF-8")), "zh-TW");
    }

    #[test]
    fn detect_locale_falls_back_to_default_when_lang_is_unset() {
        assert_eq!(detect_locale(None), blueice_i18n::DEFAULT_LOCALE);
    }

    #[test]
    fn detect_locale_falls_back_to_default_for_an_unsupported_lang() {
        assert_eq!(
            detect_locale(Some("fr_FR.UTF-8")),
            blueice_i18n::DEFAULT_LOCALE
        );
    }

    #[test]
    fn detect_locale_is_case_insensitive() {
        assert_eq!(detect_locale(Some("ZH_tw.UTF-8")), "zh-TW");
    }

    // ---- downloads: `downloads` and `download <url>` -------------------

    #[test]
    fn stdin_downloads_command_navigates_to_the_built_in_downloads_page() {
        assert!(
            matches!(stdin_line_to_event("downloads"), Some(UserEvent::Navigate(url)) if url == DOWNLOADS_URL)
        );
        assert!(matches!(
            stdin_line_to_event("  downloads "),
            Some(UserEvent::Navigate(_))
        ));
    }

    #[test]
    fn stdin_download_command_carries_the_url_to_fetch() {
        assert!(
            matches!(stdin_line_to_event("download https://example.com/a.iso"), Some(UserEvent::StartDownload(url)) if url == "https://example.com/a.iso")
        );
        assert!(
            matches!(stdin_line_to_event("  download   https://example.com/b.iso  "), Some(UserEvent::StartDownload(url)) if url == "https://example.com/b.iso")
        );
    }

    #[test]
    fn stdin_download_without_a_url_is_not_a_command() {
        assert!(stdin_line_to_event("download").is_none());
        assert!(stdin_line_to_event("download   ").is_none());
    }

    #[test]
    fn the_downloads_page_is_opened_in_the_windows_own_language() {
        assert_eq!(
            navigation_url("about:downloads", "zh-TW"),
            "about:downloads?lang=zh-TW"
        );
        assert_eq!(
            navigation_url("about:downloads", "en"),
            "about:downloads?lang=en"
        );
        for other in [
            "about:credits",
            "https://example.com/",
            "about:downloads?lang=en",
            "about:blank",
        ] {
            assert_eq!(
                navigation_url(other, "zh-TW"),
                other,
                "only the bare downloads URL is localized here"
            );
        }
    }

    #[test]
    fn sibling_downloads_binary_sits_next_to_the_frontend_binary() {
        let exe = PathBuf::from("/opt/blueice/bin/blueice-frontend");
        assert_eq!(
            sibling_downloads_binary(&exe),
            PathBuf::from("/opt/blueice/bin/blueice-downloads")
        );
    }

    fn fake_downloads_process(socket: &Path) -> std::thread::JoinHandle<()> {
        use blueice_ipc::downloads::{
            read_downloads_request, write_downloads_reply, DownloadsReply, DownloadsRequest,
            DOWNLOADS_PROTOCOL_VERSION,
        };
        let listener = std::os::unix::net::UnixListener::bind(socket).unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                std::thread::spawn(move || {
                    while let Ok((id, request)) = read_downloads_request(&mut stream) {
                        let reply = match request {
                            DownloadsRequest::Hello { .. } => DownloadsReply::Hello {
                                protocol_version: DOWNLOADS_PROTOCOL_VERSION,
                            },
                            DownloadsRequest::Start { url, .. } => {
                                DownloadsReply::Started(TransferInfo {
                                    id: 5,
                                    url,
                                    ..TransferInfo::default()
                                })
                            }
                            _ => DownloadsReply::Ok,
                        };
                        if write_downloads_reply(&mut stream, id, &reply).is_err() {
                            return;
                        }
                    }
                });
            }
        })
    }

    fn scratch_socket(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bf-{label}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("d.sock")
    }

    #[test]
    fn a_download_is_started_on_a_running_downloads_process_without_spawning_anything() {
        let socket = scratch_socket("running");
        let _server = fake_downloads_process(&socket);
        let started = start_download_at(
            &socket,
            &|| panic!("nothing should be spawned"),
            "https://example.com/a.iso",
        )
        .unwrap();
        assert_eq!(
            (started.id, started.url.as_str()),
            (5, "https://example.com/a.iso")
        );
    }

    #[test]
    fn a_download_starts_the_downloads_process_first_when_nothing_is_listening() {
        let socket = scratch_socket("spawn");
        let spawn_socket = socket.clone();
        let spawner = move || -> std::io::Result<()> {
            let socket = spawn_socket.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(100));
                let _ = fake_downloads_process(&socket);
            });
            Ok(())
        };
        let started = start_download_at(&socket, &spawner, "https://example.com/b.iso").unwrap();
        assert_eq!(started.id, 5);
    }

    #[test]
    fn a_stale_downloads_socket_path_is_not_mistaken_for_a_live_service() {
        let socket = scratch_socket("stale");
        std::fs::write(&socket, b"stale socket entry").unwrap();
        let spawn_socket = socket.clone();
        let spawner = move || -> std::io::Result<()> {
            let socket = spawn_socket.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(100));
                let _ = std::fs::remove_file(&socket);
                let _ = fake_downloads_process(&socket);
            });
            Ok(())
        };

        let started =
            start_download_at(&socket, &spawner, "https://example.com/stale.iso").unwrap();
        assert_eq!(started.id, 5);
    }

    #[test]
    fn a_download_that_cannot_reach_or_start_the_service_says_so() {
        let socket = scratch_socket("fail");
        let error = start_download_at(
            &socket,
            &|| Err(std::io::Error::other("no such binary")),
            "https://example.com/c.iso",
        )
        .unwrap_err();
        assert!(error.contains("no such binary"), "{error}");
    }
}
