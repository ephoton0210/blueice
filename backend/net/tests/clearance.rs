// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The gatekeeper review through its public API: what it sends, what it
//! takes as clearance, and -- the point of the design -- that every way
//! the gatekeeper can fail to say "yes" produces no token at all
//! (fail-closed, `phase-7-local-ai/PLAN.md`).

mod common;

use blueice_ipc::gatekeeper::GatekeeperRequest;
use blueice_net::download::clearance::{Blocked, Reviewer, UrlCleared};
use blueice_net::download::probe::{probe, Probe};
use blueice_net::download::DownloadOptions;
use common::{body, FakeGatekeeper, GateReply, Resource, TestServer};
use std::time::{Duration, Instant};

fn probed(cleared: &UrlCleared) -> Probe {
    probe(cleared, &DownloadOptions::default()).expect("the local server is probeable")
}

fn reject(reason: &str, category: &str) -> GateReply {
    GateReply::Reject { reason: reason.to_string(), category: category.to_string() }
}

fn unavailable(blocked: &Blocked) -> bool {
    blocked.category == "gatekeeper-unavailable"
}

#[test]
fn a_cleared_url_yields_a_token_that_remembers_the_url() {
    let gate = FakeGatekeeper::clear_all();
    let cleared = Reviewer::new(&gate.socket).review_url("https://example.com/a.bin").unwrap();
    assert_eq!(cleared.url(), "https://example.com/a.bin");
    assert_eq!(gate.requests(), vec![GatekeeperRequest::CheckUrl { url: "https://example.com/a.bin".to_string() }]);
}

#[test]
fn a_rejected_url_yields_the_gatekeepers_own_reason_and_category() {
    let gate = FakeGatekeeper::start(|_| reject("known phishing domain", "known-bad-domain"));
    let blocked = Reviewer::new(&gate.socket).review_url("https://evil.example/").unwrap_err();
    assert_eq!(blocked, Blocked { reason: "known phishing domain".to_string(), category: "known-bad-domain".to_string() });
}

#[test]
fn an_unreachable_gatekeeper_blocks_instead_of_clearing() {
    let dir = common::TempDir::new();
    let blocked = Reviewer::new(dir.join("nobody-listens.sock")).review_url("https://example.com/").expect_err("fail-closed");
    assert!(unavailable(&blocked), "{blocked:?}");
}

#[test]
fn a_gatekeeper_that_never_answers_blocks_after_the_timeout() {
    let gate = FakeGatekeeper::start(|_| GateReply::Hang(Duration::from_secs(10)));
    let started = Instant::now();
    let blocked = Reviewer::new(&gate.socket).with_timeout(Duration::from_millis(200)).review_url("https://example.com/").expect_err("fail-closed");
    assert!(unavailable(&blocked), "{blocked:?}");
    assert!(started.elapsed() < Duration::from_secs(3), "must give up at the timeout, took {:?}", started.elapsed());
}

#[test]
fn a_gatekeeper_that_hangs_up_or_sends_garbage_blocks() {
    for reply in [GateReply::Close, GateReply::Garbage] {
        let name = match reply {
            GateReply::Close => "closes without replying",
            _ => "replies with garbage",
        };
        let gate = FakeGatekeeper::start(move |_| match reply {
            GateReply::Close => GateReply::Close,
            _ => GateReply::Garbage,
        });
        let blocked = Reviewer::new(&gate.socket).review_url("https://example.com/").err().unwrap_or_else(|| panic!("a gatekeeper that {name} must not clear"));
        assert!(unavailable(&blocked), "{name}: {blocked:?}");
    }
}

#[test]
fn the_download_stage_sends_what_the_probe_learned() {
    let gate = FakeGatekeeper::clear_all();
    let reviewer = Reviewer::new(&gate.socket);
    let server = TestServer::start();
    server.serve("/setup.exe", Resource { content_type: Some("application/x-msdownload".to_string()), ..Resource::new(body(4_096)) });
    let url = server.url("/setup.exe");
    let cleared = reviewer.review_url(&url).unwrap();
    let probe = probed(&cleared);
    reviewer.review_download(cleared, &probe, "setup.exe").expect("cleared");
    assert_eq!(
        gate.requests(),
        vec![
            GatekeeperRequest::CheckUrl { url: url.to_string() },
            GatekeeperRequest::CheckDownload {
                url: url.to_string(),
                file_name: "setup.exe".to_string(),
                content_type: Some("application/x-msdownload".to_string()),
                total_bytes: Some(4_096),
            },
        ]
    );
}

#[test]
fn the_download_stage_reviews_the_final_url_after_redirects() {
    // The requested URL passed the URL stage; where it *redirected to* is
    // what the file actually comes from, so that is what the download
    // stage must look at.
    let gate = FakeGatekeeper::clear_all();
    let reviewer = Reviewer::new(&gate.socket);
    let server = TestServer::start();
    server.serve("/short", Resource { redirect_to: Some("/real.bin".to_string()), ..Resource::new(Vec::new()) });
    server.serve("/real.bin", Resource::new(body(4_096)));
    let cleared = reviewer.review_url(&server.url("/short")).unwrap();
    let redirected = probed(&cleared);
    reviewer.review_download(cleared, &redirected, "real.bin").unwrap();
    match &gate.requests()[1] {
        GatekeeperRequest::CheckDownload { url, .. } => assert_eq!(url, &server.url("/real.bin")),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_rejected_download_yields_no_clearance() {
    let gate = FakeGatekeeper::start(|req| match req {
        GatekeeperRequest::CheckDownload { .. } => reject("executable from an untrusted origin", "dangerous-file-type"),
        _ => GateReply::Clear,
    });
    let reviewer = Reviewer::new(&gate.socket);
    let server = TestServer::start();
    server.serve("/setup.exe", Resource::new(body(4_096)));
    let cleared = reviewer.review_url(&server.url("/setup.exe")).unwrap();
    let probe = probed(&cleared);
    let blocked = reviewer.review_download(cleared, &probe, "setup.exe").unwrap_err();
    assert_eq!(blocked.category, "dangerous-file-type");
    assert_eq!(blocked.reason, "executable from an untrusted origin");
}

#[test]
fn a_gatekeeper_that_goes_away_between_the_stages_blocks_the_download() {
    let gate = FakeGatekeeper::clear_all();
    let socket = gate.socket.clone();
    let reviewer = Reviewer::new(&socket);
    let server = TestServer::start();
    server.serve("/a.bin", Resource::new(body(4_096)));
    let cleared = reviewer.review_url(&server.url("/a.bin")).unwrap();
    let probe = probed(&cleared);
    drop(gate); // the gatekeeper dies after clearing the URL
    let blocked = reviewer.review_download(cleared, &probe, "a.bin").expect_err("fail-closed");
    assert!(unavailable(&blocked), "{blocked:?}");
}

#[test]
fn a_probe_of_a_different_url_than_the_one_cleared_is_refused_without_asking() {
    let gate = FakeGatekeeper::clear_all();
    let reviewer = Reviewer::new(&gate.socket);
    let server = TestServer::start();
    server.serve("/good.bin", Resource::new(body(4_096)));
    server.serve("/other.bin", Resource::new(body(4_096)));
    let cleared = reviewer.review_url(&server.url("/good.bin")).unwrap();
    let other_cleared = reviewer.review_url(&server.url("/other.bin")).unwrap();
    let other_probe = probed(&other_cleared);
    let blocked = reviewer.review_download(cleared, &other_probe, "other.bin").unwrap_err();
    assert_eq!(blocked.category, "clearance-mismatch");
    assert_eq!(gate.requests().len(), 2, "no CheckDownload for a mismatched pair");
}
