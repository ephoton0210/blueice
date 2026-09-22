// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `blueice-ai-gatekeeper`: the process binary. Deliberately thin --
//! all the logic it runs (`handle_one_check`, always replying
//! `Cleared`) lives in `blueice_ai_gatekeeper`'s `lib.rs`, already
//! covered by its own unit tests against an in-process `UnixStream`
//! pair. This file is just binding a real `UnixListener` at the
//! well-known gatekeeper socket path and accepting connections --
//! matching how `blueice-core`'s own thin binary is structured (see
//! that crate's `src/bin/blueice-core.rs` docs).
//!
//! `core` opens a short-lived, per-check connection per review (connect
//! -> request -> reply -> disconnect). Each accepted connection runs in its
//! own bounded-time worker, so an idle local peer cannot block other reviews.

use blueice_ai_gatekeeper::handle_one_check;
use blueice_ipc::gatekeeper::default_gatekeeper_socket_path;
use blueice_ipc::local_socket::{bind_private_listener, ensure_private_socket_dir};
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::thread;
use std::time::{Duration, Instant};

const CHECK_TIMEOUT: Duration = Duration::from_secs(10);

struct DeadlineStream {
    stream: UnixStream,
    deadline: Instant,
}

impl DeadlineStream {
    fn new(stream: UnixStream) -> Self {
        DeadlineStream { stream, deadline: Instant::now() + CHECK_TIMEOUT }
    }
}

impl Read for DeadlineStream {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let remaining = self.deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(io::Error::new(io::ErrorKind::ConnectionAborted, "the gatekeeper request exceeded its deadline"));
        }
        self.stream.set_read_timeout(Some(remaining))?;
        self.stream.read(buffer)
    }
}

impl Write for DeadlineStream {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.stream.write(buffer)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
}

fn main() -> std::io::Result<()> {
    let path = default_gatekeeper_socket_path();
    if let Some(parent) = path.parent() {
        ensure_private_socket_dir(parent)?;
    }
    // A stale socket file from a previous run (e.g. one that crashed
    // instead of exiting cleanly) makes bind() fail with AddrInUse even
    // though nothing is actually listening -- remove it first, same as
    // `blueice-core`'s own binary does for its own socket.
    let _ = std::fs::remove_file(&path);

    let listener = bind_private_listener(&path)?;
    for stream in listener.incoming().flatten() {
        thread::spawn(move || {
            let _ = stream.set_write_timeout(Some(CHECK_TIMEOUT));
            let mut stream = DeadlineStream::new(stream);
            let _ = handle_one_check(&mut stream);
        });
    }
    Ok(())
}
