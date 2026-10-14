// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Single-owner private-pipe entry point for a native frontend. This does not
//! expose a public socket or replace launcher supervision/permission services.

use blueice_engine::{session, TabManager};
use std::io;
use std::path::PathBuf;
use std::process::ExitCode;

struct FrameDirectory(PathBuf);
impl Drop for FrameDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run() -> Result<(), String> {
    let mut width = 1024.0;
    let mut height = 640.0;
    let mut frame_dir = None;
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--stdio") => (),
            Some("--frame-dir") => {
                frame_dir = Some(PathBuf::from(
                    args.next().ok_or("--frame-dir needs a path")?,
                ))
            }
            Some("--width" | "--height") => {
                let value = args
                    .next()
                    .and_then(|value| value.to_str().and_then(|value| value.parse::<f64>().ok()))
                    .filter(|value| value.is_finite() && (1.0..=4096.0).contains(value))
                    .ok_or("viewport dimensions must be between 1 and 4096")?;
                if arg == "--width" {
                    width = value;
                } else {
                    height = value;
                }
            }
            _ => {
                return Err(format!(
                    "unsupported private-pipe argument: {}",
                    arg.to_string_lossy()
                ))
            }
        }
    }
    let path = frame_dir.ok_or("--stdio requires a new --frame-dir owned by the frontend")?;
    std::fs::create_dir(&path)
        .map_err(|error| format!("create private frame directory: {error}"))?;
    let frames = FrameDirectory(path);
    let mut tabs = TabManager::new(width, height);
    let mut stream = session::message_pipe::MessagePipe::new(io::stdin(), io::stdout());
    session::run_session(
        &mut tabs,
        &mut stream,
        &frames.0,
        &mut 0,
        &frames.0.join("unavailable-gatekeeper.sock"),
    )
    .map_err(|error| error.to_string())
}

pub(super) fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("blueice-core: {error}");
            ExitCode::FAILURE
        }
    }
}
