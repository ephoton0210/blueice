// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Debug aids for the native window: a snapshot of exactly what it is drawing
//! (so a developer, or Claude Code reading the file, can *see* the trusted-window
//! panels), and a clear explanation when the display server connection dies.
//!
//! These are **observation only**. Nothing here can press a key or click:
//! injecting input into the trusted window would let a debugger approve its own
//! consent prompts, which is exactly what that window exists to prevent. A
//! person presses the keys; the snapshot and the launcher trace show what happened.
//!
//! The snapshot is enabled by `BLUEICE_FRONTEND_SNAPSHOT=<dir>` and writes
//! `<dir>/native-window.png`, replaced atomically, only when the picture changed
//! and at most a few times a second.

use std::path::PathBuf;
use std::time::{Duration, Instant};

/// The most often the snapshot file is rewritten.
const MIN_INTERVAL: Duration = Duration::from_millis(250);

/// Encodes the window's `0x00RRGGBB` pixel buffer as an opaque RGBA PNG.
pub(super) fn encode_png(pixels: &[u32], width: u32, height: u32) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(pixels.len() * 4);
    for pixel in pixels {
        rgba.extend_from_slice(&[(pixel >> 16) as u8, (pixel >> 8) as u8, *pixel as u8, 0xFF]);
    }
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().expect("PNG header");
        writer.write_image_data(&rgba).expect("PNG data");
    }
    out
}

/// Writes snapshots of the window when asked to by the environment.
pub(super) struct Snapshots {
    dir: PathBuf,
    last_written: Option<Instant>,
    last_fingerprint: u64,
    /// The newest picture that was skipped only because the last write was too
    /// recent. Without it the *final* state of a burst of redraws (a request
    /// sent, then its reply) would never be saved, and the file would show the
    /// intermediate one.
    held: Option<(Vec<u32>, u32, u32)>,
}

impl Snapshots {
    /// `None` unless `BLUEICE_FRONTEND_SNAPSHOT` names a directory.
    pub(super) fn from_env() -> Option<Self> {
        let dir = std::env::var_os("BLUEICE_FRONTEND_SNAPSHOT").map(PathBuf::from)?;
        std::fs::create_dir_all(&dir).ok()?;
        Some(Snapshots {
            dir,
            last_written: None,
            last_fingerprint: 0,
            held: None,
        })
    }

    #[cfg(test)]
    fn in_dir(dir: &std::path::Path) -> Self {
        Snapshots {
            dir: dir.to_path_buf(),
            last_written: None,
            last_fingerprint: 0,
            held: None,
        }
    }

    /// Saves the picture if it changed and enough time has passed. Returns
    /// whether a file was written. Failures are ignored: a debug aid must never
    /// take the window down.
    pub(super) fn maybe_write(
        &mut self,
        pixels: &[u32],
        width: u32,
        height: u32,
        now: Instant,
    ) -> bool {
        let fingerprint = fingerprint(pixels, width, height);
        if fingerprint == self.last_fingerprint {
            self.held = None;
            return false;
        }
        if self
            .last_written
            .is_some_and(|t| now.saturating_duration_since(t) < MIN_INTERVAL)
        {
            self.held = Some((pixels.to_vec(), width, height));
            return false;
        }
        self.held = None;
        let png = encode_png(pixels, width, height);
        let staged = self.dir.join("native-window.png.tmp");
        let done = std::fs::write(&staged, png)
            .and_then(|()| std::fs::rename(&staged, self.dir.join("native-window.png")))
            .is_ok();
        if done {
            self.last_written = Some(now);
            self.last_fingerprint = fingerprint;
        }
        done
    }

    /// When a held picture becomes due to be written, if there is one.
    pub(super) fn next_due(&self) -> Option<Instant> {
        self.held
            .as_ref()
            .map(|_| self.last_written.map_or_else(Instant::now, |t| t + MIN_INTERVAL))
    }

    /// Writes the held picture once its time has come. Returns whether a file
    /// was written.
    pub(super) fn flush_due(&mut self, now: Instant) -> bool {
        if self.next_due().is_none_or(|due| now < due) {
            return false;
        }
        let Some((pixels, width, height)) = self.held.take() else {
            return false;
        };
        self.maybe_write(&pixels, width, height, now)
    }
}

/// A cheap fingerprint (FNV-1a over the pixels and size) to skip identical frames.
fn fingerprint(pixels: &[u32], width: u32, height: u32) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325 ^ u64::from(width) << 32 ^ u64::from(height);
    for pixel in pixels {
        hash ^= u64::from(*pixel);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// What to tell a person whose window's event loop ended with an error. On
/// Wayland (notably WSLg) the display server connection can drop, which the
/// windowing library reports only as `Io error: Broken pipe`.
pub(super) fn event_loop_failure_hint(error: &dyn std::fmt::Display) -> String {
    format!(
        "blueice-frontend: the window's event loop ended with an error: {error}\n\
         If you saw `Io error: Broken pipe`, the connection to the display server dropped\n\
         (seen on Wayland under WSLg). Try the X11 backend instead:\n\
         \x20   WAYLAND_DISPLAY= WINIT_UNIX_BACKEND=x11 blueice-frontend ...\n\
         (`scripts/dev-stack.sh` does this by default.)"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fe-snap-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_png_round_trips_the_pixels_through_a_real_decoder() {
        let pixels = [0x00FF_0000u32, 0x0000_FF00, 0x0000_00FF, 0x00FF_FFFF];
        let bytes = encode_png(&pixels, 2, 2);
        let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        let mut reader = decoder.read_info().unwrap();
        let mut buffer = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buffer).unwrap();
        assert_eq!((info.width, info.height), (2, 2));
        assert_eq!(
            &buffer[..16],
            [255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255]
        );
    }

    #[test]
    fn a_snapshot_is_written_atomically_only_when_the_picture_changed_and_not_too_often() {
        let dir = temp_dir("write");
        let mut snapshots = Snapshots::in_dir(&dir);
        let start = Instant::now();
        let one = vec![0x0011_2233u32; 16];
        assert!(snapshots.maybe_write(&one, 4, 4, start));
        assert!(dir.join("native-window.png").exists());
        assert!(
            !dir.join("native-window.png.tmp").exists(),
            "no half-written file is left"
        );
        // Identical picture: nothing to write, however long we wait.
        assert!(!snapshots.maybe_write(&one, 4, 4, start + Duration::from_secs(5)));
        // A changed picture too soon after the last write waits...
        let two = vec![0x0044_5566u32; 16];
        assert!(!snapshots.maybe_write(&two, 4, 4, start + Duration::from_millis(50)));
        // ...and is written once the interval has passed.
        assert!(snapshots.maybe_write(&two, 4, 4, start + MIN_INTERVAL));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_last_picture_of_a_burst_is_held_and_written_when_due() {
        let dir = temp_dir("held");
        let mut snapshots = Snapshots::in_dir(&dir);
        let start = Instant::now();
        assert!(snapshots.maybe_write(&vec![1u32; 16], 4, 4, start));
        assert_eq!(snapshots.next_due(), None, "nothing is waiting yet");
        // Two quick redraws (a request, then its reply): only the last is kept.
        let soon = start + Duration::from_millis(10);
        assert!(!snapshots.maybe_write(&vec![2u32; 16], 4, 4, soon));
        assert!(!snapshots.maybe_write(&vec![3u32; 16], 4, 4, soon));
        assert_eq!(snapshots.next_due(), Some(start + MIN_INTERVAL));
        assert!(!snapshots.flush_due(start + Duration::from_millis(100)), "not yet");
        assert!(snapshots.flush_due(start + MIN_INTERVAL));
        assert_eq!(snapshots.next_due(), None);
        let written = std::fs::read(dir.join("native-window.png")).unwrap();
        assert_eq!(written, encode_png(&vec![3u32; 16], 4, 4), "the newest picture won");
        // A picture identical to what is on disk needs no write.
        assert!(!snapshots.maybe_write(&vec![3u32; 16], 4, 4, start + Duration::from_secs(9)));
        assert_eq!(snapshots.next_due(), None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_size_change_alone_is_a_different_picture() {
        assert_ne!(
            fingerprint(&[1, 2, 3, 4], 2, 2),
            fingerprint(&[1, 2, 3, 4], 4, 1)
        );
    }

    #[test]
    fn an_unwritable_directory_is_ignored_not_fatal() {
        let mut snapshots = Snapshots::in_dir(Path::new("/nonexistent/definitely/not/here"));
        assert!(!snapshots.maybe_write(&[0; 4], 2, 2, Instant::now()));
    }

    #[test]
    fn the_environment_variable_turns_snapshots_on_and_absence_leaves_them_off() {
        // Not set in the test environment: off.
        if std::env::var_os("BLUEICE_FRONTEND_SNAPSHOT").is_none() {
            assert!(Snapshots::from_env().is_none());
        }
    }

    #[test]
    fn the_failure_hint_names_the_symptom_and_the_workaround() {
        let hint = event_loop_failure_hint(&"ExitFailure(1)");
        assert!(hint.contains("Broken pipe"));
        assert!(hint.contains("WINIT_UNIX_BACKEND=x11"));
        assert!(hint.contains("ExitFailure(1)"));
    }
}
