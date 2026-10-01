// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `blueice-ai-gatekeeper`: the process binary. Deliberately thin --
//! all review logic (mandatory versioned rules and an optional local model)
//! lives in `blueice_ai_gatekeeper`'s `GatekeeperService`, already
//! covered by its own unit tests against an in-process `UnixStream`
//! pair. This file is just parsing an optional private socket override,
//! binding a real `UnixListener`, and accepting connections -- matching
//! how `blueice-core`'s own thin binary is structured (see that crate's
//! `src/bin/blueice-core.rs` docs). The override lets `blueice-launcher`
//! give each supervised browser session its own gatekeeper instead of
//! competing for the well-known standalone-development socket.
//!
//! `core` opens a short-lived, per-check connection per review (connect
//! -> request -> reply -> disconnect). Each accepted connection runs in its
//! own bounded-time worker, so an idle local peer cannot block other reviews.

#[cfg(unix)]
#[path = "blueice-ai-gatekeeper/unix.rs"]
mod unix;

#[cfg(unix)]
fn main() -> std::process::ExitCode {
    unix::main()
}

#[cfg(not(unix))]
fn main() -> std::process::ExitCode {
    eprintln!("blueice-ai-gatekeeper requires Unix-domain socket support on this platform");
    std::process::ExitCode::FAILURE
}
