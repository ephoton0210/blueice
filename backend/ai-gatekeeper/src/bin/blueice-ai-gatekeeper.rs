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
//! -> request -> reply -> disconnect), so handling connections
//! sequentially here is a deliberate match to that shape, not a
//! scalability shortcut -- see `blueice_ai_gatekeeper`'s own module
//! docs.

use blueice_ai_gatekeeper::handle_one_check;
use blueice_ipc::gatekeeper::default_gatekeeper_socket_path;
use std::os::unix::net::UnixListener;

fn main() -> std::io::Result<()> {
    let path = default_gatekeeper_socket_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // A stale socket file from a previous run (e.g. one that crashed
    // instead of exiting cleanly) makes bind() fail with AddrInUse even
    // though nothing is actually listening -- remove it first, same as
    // `blueice-core`'s own binary does for its own socket.
    let _ = std::fs::remove_file(&path);

    let listener = UnixListener::bind(&path)?;
    for mut stream in listener.incoming().flatten() {
        let _ = handle_one_check(&mut stream);
    }
    Ok(())
}
