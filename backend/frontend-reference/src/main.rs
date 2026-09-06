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

use blueice_ipc::{shm, ClientMessage, ServerMessage};
use softbuffer::{Context, Surface};
use std::io::BufRead;
use std::num::NonZeroU32;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
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
    pixels.chunks_exact(4).map(|p| (u32::from(p[0]) << 16) | (u32::from(p[1]) << 8) | u32::from(p[2])).collect()
}

/// `core` is expected to sit next to this binary in the same build
/// output directory (both are workspace members landing in the same
/// `target/<profile>/`) -- this avoids requiring a `--core-exe` flag
/// for the common case while still being explicit about the
/// assumption, rather than silently searching `$PATH`.
fn sibling_core_binary(this_exe: &Path) -> PathBuf {
    let name = if cfg!(windows) { "blueice-core.exe" } else { "blueice-core" };
    this_exe.parent().map(|dir| dir.join(name)).unwrap_or_else(|| PathBuf::from(name))
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

#[derive(Debug)]
enum UserEvent {
    Server(ServerMessage),
    Disconnected,
    SetVisible(bool),
    Quit,
}

struct CurrentFrame {
    width: u32,
    height: u32,
    generation: u64,
    pixels_xrgb: Vec<u32>,
}

struct App {
    core: Child,
    writer: UnixStream,
    window: Option<Rc<Window>>,
    surface: Option<Surface<Rc<Window>, Rc<Window>>>,
    frame: Option<CurrentFrame>,
    cursor: (f64, f64),
}

impl App {
    fn send(&mut self, msg: &ClientMessage) {
        if let Err(e) = blueice_ipc::write_client_message(&mut self.writer, msg) {
            eprintln!("blueice-frontend: failed to send {msg:?}: {e}");
        }
    }

    fn apply_frame(&mut self, shm_path: &str, width: u32, height: u32, generation: u64) {
        if let Some(existing) = &self.frame {
            if generation <= existing.generation {
                return; // stale frame, already superseded
            }
        }
        match shm::map_frame(Path::new(shm_path)) {
            Ok(mapped) => {
                self.frame = Some(CurrentFrame { width, height, generation, pixels_xrgb: rgba_to_xrgb(&mapped) });
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            Err(e) => eprintln!("blueice-frontend: failed to map frame {shm_path}: {e}"),
        }
    }

    fn redraw(&mut self) {
        let (Some(window), Some(surface), Some(frame)) = (&self.window, &mut self.surface, &self.frame) else {
            return;
        };
        let (Some(w), Some(h)) = (NonZeroU32::new(frame.width), NonZeroU32::new(frame.height)) else {
            return;
        };
        if surface.resize(w, h).is_err() {
            return;
        }
        if let Ok(mut buffer) = surface.buffer_mut() {
            buffer.copy_from_slice(&frame.pixels_xrgb);
            let _ = buffer.present();
        }
        window.pre_present_notify();
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes().with_title("BlueIce (reference frontend)");
        let window = Rc::new(event_loop.create_window(attrs).expect("failed to create window"));
        let context = Context::new(window.clone()).expect("failed to create softbuffer context");
        let surface = Surface::new(&context, window.clone()).expect("failed to create softbuffer surface");
        self.window = Some(window);
        self.surface = Some(surface);
        self.redraw();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _window_id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                self.send(&ClientMessage::Shutdown);
                event_loop.exit();
            }
            WindowEvent::Resized(size) if size.width > 0 && size.height > 0 => {
                self.send(&ClientMessage::Resize { width: size.width, height: size.height });
            }
            WindowEvent::RedrawRequested => self.redraw(),
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = (position.x, position.y);
            }
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Left, .. } => {
                let (x, y) = self.cursor;
                self.send(&ClientMessage::Click { x, y });
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let delta_y = match delta {
                    MouseScrollDelta::LineDelta(_, y) => -y as f64 * 20.0,
                    MouseScrollDelta::PixelDelta(pos) => -pos.y,
                };
                self.send(&ClientMessage::Scroll { delta_y });
            }
            _ => {}
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Server(ServerMessage::FrameReady { shm_path, width, height, generation }) => {
                self.apply_frame(&shm_path, width, height, generation);
            }
            UserEvent::Server(ServerMessage::Navigated { url }) => {
                if let Some(window) = &self.window {
                    window.set_title(&format!("BlueIce -- {url}"));
                }
            }
            UserEvent::Server(ServerMessage::Error { message }) => {
                eprintln!("blueice-frontend: core reported an error: {message}");
            }
            UserEvent::Disconnected => {
                eprintln!("blueice-frontend: core disconnected");
                event_loop.exit();
            }
            UserEvent::SetVisible(visible) => {
                if let Some(window) = &self.window {
                    window.set_visible(visible);
                }
                self.send(&ClientMessage::SetVisible(visible));
            }
            UserEvent::Quit => {
                self.send(&ClientMessage::Shutdown);
                event_loop.exit();
            }
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        let _ = self.core.wait();
    }
}

/// Reads `show`/`hide`/`quit` lines from stdin and forwards them as
/// events -- see module docs for why stdin stands in for a real
/// AI-facing control channel here.
fn spawn_stdin_commands(proxy: EventLoopProxy<UserEvent>) {
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        for line in stdin.lock().lines().map_while(Result::ok) {
            let event = match line.trim() {
                "show" => Some(UserEvent::SetVisible(true)),
                "hide" => Some(UserEvent::SetVisible(false)),
                "quit" => Some(UserEvent::Quit),
                other if !other.is_empty() => {
                    eprintln!("blueice-frontend: unrecognized command {other:?} (try show/hide/quit)");
                    None
                }
                _ => None,
            };
            if let Some(event) = event {
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
        match blueice_ipc::read_server_message(&mut reader) {
            Ok(msg) => {
                if proxy.send_event(UserEvent::Server(msg)).is_err() {
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
    let url = std::env::args().nth(1).unwrap_or_else(|| "https://example.com".to_string());

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
        .unwrap_or_else(|e| panic!("failed to spawn {} ({e}) -- expected it next to {}", core_bin.display(), this_exe.display()));

    if !wait_for_socket(&socket_path, Duration::from_secs(5)) {
        panic!("blueice-core never created its socket at {}", socket_path.display());
    }
    let writer = UnixStream::connect(&socket_path).expect("failed to connect to blueice-core");
    let reader = writer.try_clone().expect("failed to clone the core connection for the reader thread");

    let event_loop = EventLoop::<UserEvent>::with_user_event().build().expect("failed to create the event loop");
    let proxy = event_loop.create_proxy();

    spawn_server_reader(reader, proxy.clone());
    spawn_stdin_commands(proxy);

    let mut app = App { core, writer, window: None, surface: None, frame: None, cursor: (0.0, 0.0) };
    app.send(&ClientMessage::Navigate { url });

    event_loop.run_app(&mut app).expect("event loop exited with an error");

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
        assert!(path.to_string_lossy().len() < 100, "AF_UNIX paths are capped around 108 bytes");
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
}
