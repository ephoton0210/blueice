// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `core`'s client side of the `ai-gatekeeper` protocol
//! (`blueice_ipc::gatekeeper`) -- the background-thread half of
//! `phase-7-local-ai/PLAN.md`'s "Wiring design": [`check_and_fetch`] is
//! meant to run inside a `std::thread::spawn`'d closure
//! (`session.rs`'s own `begin_gated_navigation`), never on `run_
//! session`'s own thread, so a slow/unreachable gatekeeper or a slow
//! fetch never blocks the one shared connection other tabs/clients are
//! also using.
//!
//! Also owns [`GatekeeperClearance`]: the typestate/capability-token
//! `phase-7-local-ai/PLAN.md`'s "Decision: a typestate/capability-token
//! pattern" commits to. It is deliberately **not** `Clone`, has **no
//! public constructor**, and its fields are private to this module --
//! the only way to produce one is [`check_and_fetch`] actually
//! completing both gatekeeper stages successfully. [`crate::Page::
//! apply_fetched`] requires one as a parameter purely for this
//! compile-time effect: skipping the gate becomes a compile error, not
//! a runtime convention a differently-written caller could omit.

use crate::TabId;
use blueice_ipc::gatekeeper::{read_gatekeeper_reply, write_gatekeeper_request, GatekeeperReply, GatekeeperRequest};
use std::io;
use std::os::unix::net::UnixStream;
use std::path::Path;

/// Proof that both gatekeeper stages cleared for `url` on `tab_id`.
/// Unconstructable outside this module (see module docs) -- the exact
/// field shape is otherwise not load-bearing; the type's *existence* as
/// a required, non-`Clone`, privately-constructed parameter is the
/// whole point.
pub(crate) struct GatekeeperClearance {
    #[allow(dead_code)] // present for the typestate's own documentation value, not read
    tab_id: TabId,
    #[allow(dead_code)]
    url: String,
}

/// One background gated-navigation's eventual result -- reported back
/// to `run_session`'s poll loop over an `mpsc` channel, never applied
/// to real `Page`/`TabManager` state on this (background) thread
/// itself.
pub(crate) enum NavOutcome {
    /// Both gatekeeper stages cleared and the fetch succeeded: ready to
    /// apply via [`crate::Page::apply_fetched`].
    Cleared { clearance: GatekeeperClearance, final_url: String, html: String },
    /// Either gatekeeper stage rejected the navigation, or the
    /// gatekeeper itself was unreachable (fail-closed, see module
    /// docs) -- maps to `ServerMessage::GatekeeperBlocked`, distinct
    /// from an ordinary fetch failure.
    GatekeeperBlocked { reason: String, category: String, url: String },
    /// The gatekeeper cleared the URL stage, but the actual network
    /// fetch itself failed -- an ordinary navigation error, mapped to
    /// `ServerMessage::Error` exactly as an unfetchable URL was before
    /// gating existed, *not* `GatekeeperBlocked`.
    FetchFailed { message: String },
}

enum StageOutcome {
    Cleared,
    Rejected { reason: String, category: String },
}

/// One gatekeeper round trip: a short-lived connection (connect ->
/// request -> reply -> disconnect, per the plan doc's "Process &
/// connection shape") to `gatekeeper_socket`. Fail-closed: any I/O
/// failure talking to the gatekeeper (connection refused, dropped
/// mid-exchange, malformed reply) is treated exactly like an explicit
/// `Rejected` -- an unreachable/down gatekeeper must never be
/// mistaken for "safe," per the plan's already-settled failure-mode
/// decision.
fn check_stage(gatekeeper_socket: &Path, request: &GatekeeperRequest) -> StageOutcome {
    let attempt = (|| -> io::Result<GatekeeperReply> {
        let mut stream = UnixStream::connect(gatekeeper_socket)?;
        write_gatekeeper_request(&mut stream, request)?;
        read_gatekeeper_reply(&mut stream)
    })();
    match attempt {
        Ok(GatekeeperReply::Cleared) => StageOutcome::Cleared,
        Ok(GatekeeperReply::Rejected { reason, category }) => StageOutcome::Rejected { reason, category },
        Err(_) => StageOutcome::Rejected { reason: "the gatekeeper is unreachable".to_string(), category: "gatekeeper-unavailable".to_string() },
    }
}

/// Runs both gatekeeper stages plus the fetch in between: `CheckUrl` ->
/// (if cleared) fetch via `blueice-net` -> `CheckContent` -> a
/// [`NavOutcome`]. Meant to run entirely on a background thread --
/// never touches any `Page`/`TabManager` state itself, only produces a
/// value the caller applies back on the main thread once it arrives.
pub(crate) fn check_and_fetch(tab_id: TabId, url: String, gatekeeper_socket: &Path) -> NavOutcome {
    if let StageOutcome::Rejected { reason, category } = check_stage(gatekeeper_socket, &GatekeeperRequest::CheckUrl { url: url.clone() }) {
        return NavOutcome::GatekeeperBlocked { reason, category, url };
    }

    let fetched = match blueice_net::fetch(&url) {
        Ok(fetched) => fetched,
        Err(e) => return NavOutcome::FetchFailed { message: e.to_string() },
    };

    if let StageOutcome::Rejected { reason, category } =
        check_stage(gatekeeper_socket, &GatekeeperRequest::CheckContent { url: fetched.final_url.clone(), html: fetched.body.clone() })
    {
        return NavOutcome::GatekeeperBlocked { reason, category, url: fetched.final_url };
    }

    NavOutcome::Cleared { clearance: GatekeeperClearance { tab_id, url: fetched.final_url.clone() }, final_url: fetched.final_url, html: fetched.body }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blueice_ipc::gatekeeper::{read_gatekeeper_request, write_gatekeeper_reply};
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::os::unix::net::UnixListener;
    use std::thread;

    /// `_label` is purely for call-site readability -- deliberately not
    /// part of the actual path (a Unix domain socket path is capped at
    /// ~100 bytes total, tighter on macOS than Linux -- see `session.
    /// rs`'s identical helper for the full rationale). A counter keeps
    /// concurrent calls within this one test binary unique.
    fn unique_gatekeeper_socket_path(_label: &str) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("bl-gkc-{}-{n}.sock", std::process::id()))
    }

    fn clearing_gatekeeper(label: &str) -> std::path::PathBuf {
        let path = unique_gatekeeper_socket_path(label);
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).unwrap();
        thread::spawn(move || {
            for incoming in listener.incoming() {
                let Ok(mut stream) = incoming else { break };
                let _ = blueice_ai_gatekeeper::handle_one_check(&mut stream);
            }
        });
        path
    }

    fn serve_html(body: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf);
            stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).unwrap();
        });
        format!("http://{addr}")
    }

    #[test]
    fn clears_both_stages_and_returns_the_fetched_body() {
        let gatekeeper = clearing_gatekeeper("cleared");
        let url = serve_html("<p>hi</p>");
        match check_and_fetch(TabId::from_u64(1), url.clone(), &gatekeeper) {
            NavOutcome::Cleared { final_url, html, .. } => {
                assert_eq!(final_url, url);
                assert!(html.contains("hi"));
            }
            _ => panic!("expected Cleared"),
        }
    }

    #[test]
    fn fails_closed_when_no_gatekeeper_is_listening() {
        let gatekeeper = unique_gatekeeper_socket_path("unreachable"); // nothing bound here
        match check_and_fetch(TabId::from_u64(1), "http://127.0.0.1:1/".to_string(), &gatekeeper) {
            NavOutcome::GatekeeperBlocked { category, .. } => assert_eq!(category, "gatekeeper-unavailable"),
            NavOutcome::Cleared { .. } => panic!("an unreachable gatekeeper must never be treated as cleared"),
            NavOutcome::FetchFailed { .. } => panic!("an unreachable gatekeeper must block before ever attempting a fetch"),
        }
    }

    #[test]
    fn a_rejected_url_stage_never_reaches_the_network() {
        let path = unique_gatekeeper_socket_path("rejects-url");
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).unwrap();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let _req = read_gatekeeper_request(&mut stream).unwrap();
            write_gatekeeper_reply(&mut stream, &GatekeeperReply::Rejected { reason: "known-bad domain".to_string(), category: "blocklist".to_string() }).unwrap();
        });

        match check_and_fetch(TabId::from_u64(7), "http://127.0.0.1:1/".to_string(), &path) {
            NavOutcome::GatekeeperBlocked { reason, category, url } => {
                assert_eq!(reason, "known-bad domain");
                assert_eq!(category, "blocklist");
                assert_eq!(url, "http://127.0.0.1:1/");
            }
            _ => panic!("expected GatekeeperBlocked"),
        }
    }

    #[test]
    fn a_rejected_content_stage_reports_the_final_url() {
        let path = unique_gatekeeper_socket_path("rejects-content");
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).unwrap();
        thread::spawn(move || {
            for incoming in listener.incoming() {
                let Ok(mut stream) = incoming else { break };
                let Ok(req) = read_gatekeeper_request(&mut stream) else { continue };
                let reply = match req {
                    GatekeeperRequest::CheckUrl { .. } => GatekeeperReply::Cleared,
                    GatekeeperRequest::CheckContent { .. } => GatekeeperReply::Rejected { reason: "hidden text".to_string(), category: "prompt-injection".to_string() },
                };
                let _ = write_gatekeeper_reply(&mut stream, &reply);
            }
        });
        let url = serve_html("<p>malicious</p>");

        match check_and_fetch(TabId::from_u64(3), url.clone(), &path) {
            NavOutcome::GatekeeperBlocked { reason, category, url: reported_url } => {
                assert_eq!(reason, "hidden text");
                assert_eq!(category, "prompt-injection");
                assert_eq!(reported_url, url);
            }
            _ => panic!("expected GatekeeperBlocked"),
        }
    }

    #[test]
    fn a_cleared_url_but_unfetchable_page_is_a_fetch_failure_not_a_gatekeeper_block() {
        let gatekeeper = clearing_gatekeeper("fetch-fails");
        match check_and_fetch(TabId::from_u64(1), "http://127.0.0.1:1/".to_string(), &gatekeeper) {
            NavOutcome::FetchFailed { .. } => {}
            _ => panic!("expected FetchFailed, not GatekeeperBlocked, for an ordinary connection failure after a cleared URL stage"),
        }
    }
}
