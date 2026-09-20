// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The test server is the thing every engine test's credibility rests on,
//! so it is checked here with a plain `TcpStream` client -- no `ureq`, no
//! engine code -- to make sure it does exactly what the tests assume.

mod common;

use common::{body, Resource, TestServer};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

/// Sends one raw request and returns `(status, lowercased headers, body)`.
fn request(server: &TestServer, path: &str, extra_headers: &[(&str, &str)]) -> (u16, Vec<(String, String)>, Vec<u8>) {
    let addr = server.url("").trim_start_matches("http://").to_string();
    let mut stream = TcpStream::connect(&addr).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut req = format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\n");
    for (k, v) in extra_headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str("\r\n");
    stream.write_all(req.as_bytes()).unwrap();
    let mut raw = Vec::new();
    let _ = stream.read_to_end(&mut raw);
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n").expect("no header terminator") + 4;
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let mut lines = head.lines();
    let status: u16 = lines.next().unwrap().split_whitespace().nth(1).unwrap().parse().unwrap();
    let headers = lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string())).collect();
    (status, headers, raw[split..].to_vec())
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
}

#[test]
fn a_plain_get_returns_the_whole_body_with_its_headers() {
    let server = TestServer::start();
    server.serve("/f", Resource::new(body(1_000)));
    let (status, headers, got) = request(&server, "/f", &[]);
    assert_eq!(status, 200);
    assert_eq!(got, body(1_000));
    assert_eq!(header(&headers, "content-length"), Some("1000"));
    assert_eq!(header(&headers, "accept-ranges"), Some("bytes"));
    assert_eq!(header(&headers, "etag"), Some("\"v1\""));
}

#[test]
fn a_range_request_gets_exactly_that_range() {
    let server = TestServer::start();
    server.serve("/f", Resource::new(body(1_000)));
    let (status, headers, got) = request(&server, "/f", &[("Range", "bytes=100-199")]);
    assert_eq!(status, 206);
    assert_eq!(got, body(1_000)[100..200]);
    assert_eq!(header(&headers, "content-range"), Some("bytes 100-199/1000"));
    assert_eq!(header(&headers, "content-length"), Some("100"));
}

#[test]
fn an_open_ended_or_overlong_range_is_clamped_to_the_end() {
    let server = TestServer::start();
    server.serve("/f", Resource::new(body(1_000)));
    let (_, headers, got) = request(&server, "/f", &[("Range", "bytes=900-")]);
    assert_eq!(got, body(1_000)[900..]);
    assert_eq!(header(&headers, "content-range"), Some("bytes 900-999/1000"));
    let (_, headers, got) = request(&server, "/f", &[("Range", "bytes=990-5000")]);
    assert_eq!(got.len(), 10);
    assert_eq!(header(&headers, "content-range"), Some("bytes 990-999/1000"));
}

#[test]
fn a_range_past_the_end_is_416_with_the_total() {
    let server = TestServer::start();
    server.serve("/f", Resource::new(body(1_000)));
    let (status, headers, got) = request(&server, "/f", &[("Range", "bytes=1000-1010")]);
    assert_eq!(status, 416);
    assert!(got.is_empty());
    assert_eq!(header(&headers, "content-range"), Some("bytes */1000"));
}

#[test]
fn a_server_that_does_not_honor_ranges_sends_everything_and_does_not_advertise_them() {
    let server = TestServer::start();
    server.serve("/f", Resource { honor_ranges: false, ..Resource::new(body(1_000)) });
    let (status, headers, got) = request(&server, "/f", &[("Range", "bytes=0-0")]);
    assert_eq!(status, 200);
    assert_eq!(got.len(), 1_000);
    assert_eq!(header(&headers, "accept-ranges"), None);
}

#[test]
fn if_range_with_a_stale_validator_gets_the_full_body_instead_of_a_range() {
    let server = TestServer::start();
    server.serve("/f", Resource::new(body(1_000)));
    let (status, _, got) = request(&server, "/f", &[("Range", "bytes=10-19"), ("If-Range", "\"v1\"")]);
    assert_eq!((status, got.len()), (206, 10), "a matching If-Range still gets the range");
    let (status, _, got) = request(&server, "/f", &[("Range", "bytes=10-19"), ("If-Range", "\"old\"")]);
    assert_eq!((status, got.len()), (200, 1_000), "a stale If-Range means the resource changed: send it all");
}

#[test]
fn updating_a_resource_changes_what_later_requests_see() {
    let server = TestServer::start();
    server.serve("/f", Resource::new(body(100)));
    server.update("/f", |r| r.etag = Some("\"v2\"".to_string()));
    let (_, headers, _) = request(&server, "/f", &[]);
    assert_eq!(header(&headers, "etag"), Some("\"v2\""));
}

#[test]
fn unknown_paths_are_404_and_failure_statuses_are_consumed_in_order() {
    let server = TestServer::start();
    assert_eq!(request(&server, "/missing", &[]).0, 404);
    server.serve("/f", Resource { fail_statuses: vec![503, 429], ..Resource::new(body(10)) });
    assert_eq!(request(&server, "/f", &[]).0, 503);
    assert_eq!(request(&server, "/f", &[]).0, 429);
    assert_eq!(request(&server, "/f", &[]).0, 200);
}

#[test]
fn a_redirect_points_at_the_other_path_on_the_same_server() {
    let server = TestServer::start();
    server.serve("/old", Resource { redirect_to: Some("/new".to_string()), ..Resource::new(body(10)) });
    let (status, headers, _) = request(&server, "/old", &[]);
    assert_eq!(status, 302);
    assert_eq!(header(&headers, "location"), Some(server.url("/new").as_str()));
}

#[test]
fn a_cut_body_is_truncated_only_as_many_times_as_asked_and_only_when_it_applies() {
    let server = TestServer::start();
    server.serve("/f", Resource { cut_body_after: Some(50), cut_times: 1, ..Resource::new(body(200)) });
    // A body no longer than the cut point is unaffected and doesn't use up the cut.
    let (_, _, got) = request(&server, "/f", &[("Range", "bytes=0-0")]);
    assert_eq!(got.len(), 1);
    let (status, headers, got) = request(&server, "/f", &[]);
    assert_eq!(status, 200);
    assert_eq!(header(&headers, "content-length"), Some("200"), "the head still promises the full length");
    assert_eq!(got.len(), 50, "but only 50 bytes arrive before the connection closes");
    let (_, _, got) = request(&server, "/f", &[]);
    assert_eq!(got.len(), 200, "the cut is consumed");
}

#[test]
fn no_content_length_means_the_body_ends_at_connection_close() {
    let server = TestServer::start();
    server.serve("/f", Resource { send_content_length: false, ..Resource::new(body(300)) });
    let (status, headers, got) = request(&server, "/f", &[]);
    assert_eq!(status, 200);
    assert_eq!(header(&headers, "content-length"), None);
    assert_eq!(got.len(), 300);
}

#[test]
fn throttling_slows_the_body_and_concurrent_bodies_are_counted() {
    let server = TestServer::start();
    server.serve("/f", Resource { chunk: 100, delay_per_chunk: Duration::from_millis(20), ..Resource::new(body(500)) });
    let started = Instant::now();
    let addr_server = &server;
    std::thread::scope(|scope| {
        let a = scope.spawn(|| request(addr_server, "/f", &[]));
        let b = scope.spawn(|| request(addr_server, "/f", &[]));
        assert_eq!(a.join().unwrap().2.len(), 500);
        assert_eq!(b.join().unwrap().2.len(), 500);
    });
    assert!(started.elapsed() >= Duration::from_millis(80), "5 chunks at 20ms each");
    assert_eq!(server.peak_concurrency(), 2, "both bodies were streaming at once");
    assert_eq!(server.active(), 0, "and none is left running");
}

#[test]
fn the_slow_hook_can_single_out_one_range_start() {
    let server = TestServer::start();
    let slow: std::sync::Arc<dyn Fn(Option<u64>) -> Duration + Send + Sync> = std::sync::Arc::new(|start| if start == Some(0) { Duration::from_millis(50) } else { Duration::ZERO });
    server.serve("/f", Resource { chunk: 10, slow: Some(slow), ..Resource::new(body(100)) });
    let started = Instant::now();
    request(&server, "/f", &[("Range", "bytes=50-99")]);
    assert!(started.elapsed() < Duration::from_millis(200), "a range not starting at 0 is not slowed");
    let started = Instant::now();
    request(&server, "/f", &[("Range", "bytes=0-49")]);
    assert!(started.elapsed() >= Duration::from_millis(200), "5 chunks at 50ms for the range starting at 0");
}

#[test]
fn a_stalled_body_goes_silent_without_closing() {
    let server = TestServer::start();
    server.serve("/f", Resource { stall_after: Some(20), stall_times: 1, stall_hold: Duration::from_millis(300), ..Resource::new(body(200)) });
    let started = Instant::now();
    let (_, _, got) = request(&server, "/f", &[]);
    assert_eq!(got.len(), 20);
    assert!(started.elapsed() >= Duration::from_millis(300), "the connection was held open for the stall");
}

#[test]
fn every_request_is_logged_with_its_range_and_answer() {
    let server = TestServer::start();
    server.serve("/f", Resource::new(body(100)));
    request(&server, "/f", &[("Range", "bytes=0-9"), ("Accept-Encoding", "identity")]);
    request(&server, "/nope", &[]);
    let log = server.requests();
    assert_eq!(log.len(), 2);
    assert_eq!((log[0].path.as_str(), log[0].range.as_deref(), log[0].status), ("/f", Some("bytes=0-9"), 206));
    assert_eq!(log[0].accept_encoding.as_deref(), Some("identity"));
    assert_eq!((log[1].path.as_str(), log[1].status), ("/nope", 404));
}

#[test]
fn a_content_range_override_lets_a_test_make_the_server_lie() {
    let server = TestServer::start();
    server.serve("/f", Resource { content_range_override: Some("bytes 5-5/1000".to_string()), ..Resource::new(body(1_000)) });
    let (_, headers, _) = request(&server, "/f", &[("Range", "bytes=0-0")]);
    assert_eq!(header(&headers, "content-range"), Some("bytes 5-5/1000"));
}

#[test]
fn the_fake_gatekeeper_records_and_answers_over_a_real_socket() {
    use blueice_ipc::gatekeeper::{read_gatekeeper_reply, write_gatekeeper_request, GatekeeperReply, GatekeeperRequest};
    let gate = common::FakeGatekeeper::start(|req| match req {
        GatekeeperRequest::CheckUrl { url } if url.contains("evil") => common::GateReply::Reject { reason: "known-bad".to_string(), category: "malware".to_string() },
        _ => common::GateReply::Clear,
    });
    for (url, expected) in [
        ("https://good.example/", GatekeeperReply::Cleared),
        ("https://evil.example/", GatekeeperReply::Rejected { reason: "known-bad".to_string(), category: "malware".to_string() }),
    ] {
        let mut stream = std::os::unix::net::UnixStream::connect(&gate.socket).unwrap();
        write_gatekeeper_request(&mut stream, &GatekeeperRequest::CheckUrl { url: url.to_string() }).unwrap();
        assert_eq!(read_gatekeeper_reply(&mut stream).unwrap(), expected);
    }
    assert_eq!(gate.requests().len(), 2);
}
